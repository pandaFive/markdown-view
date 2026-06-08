# 検索 RSS live allocation timeline 設計書

## 目的

`docs/todo/BACKLOG.md` の「ディレクトリ検索 prefix many-match RSS plateau の環境差・live allocation 追加診断」を、Rust production code を変更せずに追加分類する。

今回の主目的は、prefix many-match 検索中の RSS / anonymous memory の時系列を外部から観測し、次のどれに近いかを判断できる材料を増やすことである。

- request 中だけ大きく増える一時 live allocation。
- response 完了後も残る allocator retained memory。
- server 側 JSON 直列化または response 構築の寄与。
- post-header body drain または socket buffering の寄与。
- prefix 経路固有の挙動。

この作業は RSS plateau の解消ではなく、次に Rust 側 probe や allocation 削減実装へ進む価値があるかを判断するための診断である。

## 非ゴール

- production Rust code の変更。
- Rust 側 instrumentation、`#[cfg(test)]` probe、診断ログの追加。
- 検索アルゴリズム、allocator、Tokio runtime 構成の改善実装。
- `MALLOC_ARENA_MAX=1` の常用化判断。
- native Linux や別 allocator build での環境差検証を必須にすること。
- `/api/search` の JSON shape や検索結果の意味変更。
- UI、TypeScript、generated JS の変更。
- Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約の変更。

## 測定アーキテクチャ

`scripts/measure-search-rss-plateau.mjs` に opt-in の timeline 測定モードを追加する。CLI は次を想定する。

```bash
node scripts/measure-search-rss-plateau.mjs \
  --timeline \
  --modes release \
  --fixtures prefix \
  --runs cold \
  --fixture-scale full \
  --fixture-density dense \
  --settled-delays 1s,5s \
  --allocator-profiles default,arena1
```

`--timeline` は重い診断モードとして扱い、`--smoke` とは併用不可にする。既存 `--smoke` は軽量な dev/prefix/cold/default の契約確認に限定し、timeline sampling を暗黙に走らせない。`--fixture-density` と `--settled-delays` は timeline 専用 option とし、`--timeline` なしでは拒否する。`--smoke` は `--timeline`、`--fixture-density`、`--settled-delays` と併用不可にする。`--fixture-density` は `dense` / `sparse` / `dense,sparse` を受け取り、既定は `dense` とする。`dense,sparse` 指定時は同一 JSON report 内に `prefix-full-dense` と `prefix-full-sparse` を生成し、scenario comparison を作れるようにする。逆順、重複、空値は拒否する。`sparse` と `dense,sparse` は `--fixtures prefix` の場合だけ許可し、`--fixtures` に `multifile` または `fallback` が含まれる場合は拒否する。`--settled-delays` は `1s,5s` を既定にし、追加確認が必要な場合だけ `1s,5s,15s` を指定する。許可する値は `1s,5s` と `1s,5s,15s` の 2 通りだけで、逆順、重複、`15s` 単独、baseline 欠落は拒否する。

report には既存の `fixtureKind` を維持し、既存通り `fixtureScale` は `measurementContext.cliOptions.fixtureScale` に置く。`reports[]` 直下には `fixtureScale` を追加しない。timeline 用に次の field を追加する。schema 階層はここで固定し、実装と self-test は同じ階層を検証する。

- top-level fields: `comparisons[]`、`scenarioComparisons[]`、`acceptanceStatus`、`fullAcceptanceMet`。
- `reports[]` item fields: `fixtureDensity`、`scenarioId`、`status`、failed scenario の `scenarioError`。
- `reports[].timeline` fields: `settledDelaysMs`、`snapshots[]`、`peaks`、`samplingSummary`、`bodyReadStartElapsedMs`、`bodyReadEndElapsedMs`、`responseBytes`、`derived`。
- `reports[].timeline.derived.settledComparisons[]`: delay 別 settled 比較。top-level や `reports[]` item 直下には置かない。

`reports[].fixtureDensity` は `dense` または `sparse` とする。`reports[].scenarioId` は `prefix-full-dense` のように fixture kind、scale、density を連結した表示用 ID とする。`reports[].timeline.responseBytes` は response body の byte 数であり、raw body は保存しない。

`prefix-full` という語は `--fixtures prefix --fixture-scale full --fixture-density dense` の短い説明名であり、CLI の fixture kind ではない。

timeline 測定では、scenario ごとに次の event snapshot を `timeline.snapshots[]` へ記録する。

- `server_ready`: server 起動後、request 前。
- `request_started`: HTTP request 送信直後。
- `headers_received`: HTTP response headers 受信時。
- `body_received`: response body 受信完了直後。
- `settled_1s`: body 受信完了から 1 秒後。
- `settled_5s`: body 受信完了から 5 秒後。
- `settled_15s`: `--settled-delays` に `15s` が含まれる場合だけ body 受信完了から 15 秒後。

`timeline.peaks` には、固定順 snapshot ではなく、event snapshot または request sampling から選んだ peak snapshot の inline copy を入れる。request sampling 全体は report に全件保存しない。

- `requestPeakRss`: `request_started` から `body_received` までの inclusive window で `VmRSS` が最大の snapshot。
- `requestPeakAnon`: `request_started` から `body_received` までの inclusive window で `RssAnon` が最大の snapshot。
- `bodyDrainPeakRss`: `headers_received` から `body_received` までの inclusive window で `VmRSS` が最大の snapshot。
- `bodyDrainPeakAnon`: `headers_received` から `body_received` までの inclusive window で `RssAnon` が最大の snapshot。

inclusive window は、区間境界の event snapshot と区間内 request sampling の両方を候補にする。sampling が 0 件でも、`request_started`、`headers_received`、`body_received` の境界 event snapshot を候補にして peak を選ぶ。必要な snapshot または metric が欠損して peak を選べない場合は、derived field を `null` にし、`excludedReason` または `partialMeasurementReasons` に理由を残す。

peak snapshot は実測時刻に依存するため、`timeline.snapshots[]` の順序検証とは分ける。`timeline.snapshots[]` は `elapsedMs` 昇順だけを契約にし、`timeline.peaks.*` は `phase`、`metric`、`capturedFrom`、`sampleIndex`、`name`、`elapsedMs`、`status`、`smapsRollup`、`maps`、`procComplete` を持つ。`capturedFrom` は `event` または `sampling` とし、`sampleIndex` は request sampling 由来なら 0 始まりの整数、event snapshot 由来なら `null` とする。memory fields は既存 `readProcSnapshot()` と同じ `status.{VmRSS,RssAnon,RssFile,RssShmem}`、`smapsRollup`、`maps` の構造を維持し、timeline 専用の flat schema へ変換しない。

`timeline.samplingSummary` には次を入れる。

- `samplingIntervalMs`: `50`。
- `clock`: monotonic clock。
- `sampleCount`: 採取した request sampling 数。
- `maxSamples`: sampling 上限。
- `missedSampleReasons[]`: 採取できなかった理由。なければ空配列。
- `droppedSampleReasons[]`: report から落とした sample がある理由。request sampling 全件を保存しない通常動作では `raw_samples_omitted` を入れる。

`timeline.peaks` の JSON shape は次の形に固定する。

```json
{
  "requestPeakAnon": {
    "phase": "request",
    "metric": "RssAnon",
    "capturedFrom": "sampling",
    "sampleIndex": 3,
    "name": "sample_0003",
    "elapsedMs": 123.4,
    "status": { "available": true, "RssAnon": 1024, "VmRSS": 2048, "RssFile": 512, "RssShmem": 0 },
    "smapsRollup": { "available": true, "Anonymous": 1024 },
    "maps": { "available": true },
    "procComplete": true
  }
}
```

各 snapshot には、取得できる範囲で次を含める。

- `name`。
- `VmRSS`、`RssAnon`、`RssFile`、`RssShmem`。
- `smaps_rollup Anonymous`。
- elapsed milliseconds。
- `/proc` 取得 completeness。

`scenarioError` は snapshot 配下ではなく、failed scenario の `reports[]` item 直下にだけ置く。snapshot は `/proc` probe の local reason だけを持ち、server 起動失敗、HTTP failure、fixture 生成失敗のように snapshot が作れない失敗を表現しない。

timeline 用 request path は通常測定の `requestSearch()` と分ける。`fetch()` が response headers を返した直後に `headers_received` を採取し、その後に body stream を `readResponseTextWithLimit()` 相当の上限付き処理で読み切って `body_received` を採取する。sampling は `request_started` から `body_received` まで継続し、body drain 中の一時 peak も `timeline.peaks` に含める。response body 読み取りの開始/終了 elapsed と response bytes は `timeline.bodyReadStartElapsedMs`、`timeline.bodyReadEndElapsedMs`、`timeline.responseBytes` として report に含める。raw response body は report に保存しない。

`headers_received` から `body_received` までの差分は、server、client、socket、undici buffering を含む外部観測値であり、server 内部の JSON serialization 完了時点とは同一視しない。

HTTP response については、既存と同じ契約値を記録する。

- status。
- `searched_files`。
- `searched_bytes`。
- `truncated`。
- `truncated_reasons`。
- result count。

## 比較 fixture

比較対象は最小限に抑える。

- `prefix-full-dense`: `--fixtures prefix --fixture-scale full --fixture-density dense` の主対象。
- `prefix-full-sparse`: `--fixtures prefix --fixture-scale full --fixture-density sparse` の対照。
- `multifile-full-dense`: prefix 経路固有性を確認する必須対照。

`prefix-full-sparse` は、dense と byte size を ±5% 以内に揃えつつ、hit 密度と result-limit 到達位置を変えた場合に peak / settled の差が変わるかを見るための補助 scenario である。`sparse` は `fixtureKind=prefix` の場合だけ意味を持つ。`--fixtures` に `multifile` または `fallback` が含まれる場合、`--fixture-density sparse` と `--fixture-density dense,sparse` は拒否する。測定 matrix が過剰にならないよう、既定 timeline run では `prefix-full-dense` を主対象にし、prefix 固有性を結論する場合だけ `prefix-full-sparse` と `multifile-full-dense` も実行する。fallback は任意の補助測定であり、prefix 固有性の Full acceptance 条件には含めない。

`prefix-full-sparse` の生成規則は次の通り固定する。

- `createFixture(root, fixtureKind, scale, density)` へ拡張し、workspace 名または `scenarioId` に density を含める。
- file count は dense と同じ 1 file。
- fixture byte size は `prefix/full/dense` の ±5% 以内にする。
- `needle` は 101 個以上配置し、ファイル全体へ概ね均等に分散する。
- `resultsLength=100`、`truncated_reasons` に `result_limit`、`searched_files=1` を維持する。
- `searched_bytes` は fixture bytes の ±5% 以内とし、self-test で dense/sparse の file count、byte range、result-limit 契約を固定する。

allocator profile は `default` と `arena1` を基本にする。`arena2` は必要時の任意 profile とし、timeline の受け入れ基準には含めない。

## 判定方針

timeline の判定は絶対 RSS ではなく、scenario 内と profile 間の相対差で行う。script は自動分類を行わず、scenario-local な `timeline.derived` と top-level `comparisons[]` を人間判断用の補助値として出力する。

`reports[].timeline.derived` には次を入れる。delta はすべて `left - right` で計算し、必要な snapshot または metric が欠損している場合は field を省略せず `null` を出す。ratio は denominator が欠損または `0` の場合に `null` を出し、理由を `partialMeasurementReasons` または `decisionExcludedReason` に残す。

- `settledComparisons[]`: `reports[].timeline.derived.settledComparisons[]` として置く、`timeline.settledDelaysMs` の各 delay ごとの peak-to-settled 比較。
- `request_started_to_headers_delta_kb`: `headers_received.status.RssAnon - request_started.status.RssAnon`。
- `headers_to_body_delta_kb`: `body_received.status.RssAnon - headers_received.status.RssAnon`。
- `headers_to_body_peak_delta_kb`: `timeline.peaks.bodyDrainPeakAnon.status.RssAnon - headers_received.status.RssAnon`。
- `body_peak_to_body_received_delta_kb`: `timeline.peaks.bodyDrainPeakAnon.status.RssAnon - body_received.status.RssAnon`。

`reports[].timeline.derived.settledComparisons[]` の各 entry は次を持つ。

- `settledDelayMs`: `1000`、`5000`、任意の `15000`。
- `settledSnapshotName`: `settled_1s`、`settled_5s`、`settled_15s`。
- `peak_to_settled_delta_kb`: `timeline.peaks.requestPeakAnon.status.RssAnon - settled snapshot の status.RssAnon`。
- `peak_to_settled_ratio`: `timeline.peaks.requestPeakAnon.status.RssAnon / settled snapshot の status.RssAnon`。
- `excludedReason`: 比較から除外する理由。比較可能なら省略する。

top-level `comparisons[]` には allocator profile 間比較を入れる。

- `mode`。
- `runKind`。
- `fixtureKind`。
- `fixtureScale`。
- `fixtureDensity`。
- `scenarioId`。
- `settledDelayMs`。
- `settledSnapshotName`: `settledDelayMs` に対応する `settled_1s`、`settled_5s`、`settled_15s`。
- `baseProfile`: `default`。
- `compareProfile`: `arena1`。
- `default_vs_arena1_settled_delta_kb`: `default` の `settledSnapshotName` snapshot の `status.RssAnon` から、`arena1` の同じ `settledSnapshotName` snapshot の `status.RssAnon` を引いた値。
- `comparisonStatus`: `ok`、`partial`、`skipped` のいずれか。
- `excludedReason`: 比較から除外する理由。比較可能なら省略する。
- `missingProfiles`: 指定されなかった profile 名の配列。欠損がなければ空配列。

comparison の group key は `mode`、`runKind`、`fixtureKind`、`fixtureScale`、`fixtureDensity`、`settledDelayMs`、`settledSnapshotName`、`baseProfile`、`compareProfile` の組み合わせで一意にする。`scenarioId` は表示用 ID であり、単独では comparison key にしない。

`default` と `arena1` が両方 `ok` の場合だけ `comparisonStatus="ok"` とし、`default_vs_arena1_settled_delta_kb = default.timeline.snapshots[name=settledSnapshotName].status.RssAnon - arena1.timeline.snapshots[name=settledSnapshotName].status.RssAnon` を出す。正の値は `arena1` の settled anonymous memory が `default` より低いことを示し、負の値は `arena1` の方が高いことを示す。片方が `partial` の場合は `comparisonStatus="partial"` とし、delta 値は出さず、`excludedReason` と partial reason だけを出す。片方が `failed` または未指定の場合は `comparisonStatus="skipped"` とする。

top-level `scenarioComparisons[]` には fixture scenario 間比較を入れる。

- `comparisonType`: `scenario`。
- `mode`。
- `runKind`。
- `allocatorProfile`。
- `baseScenarioId`。
- `compareScenarioId`。
- `metric`: `peak_to_settled_delta_kb`、`headers_to_body_peak_delta_kb` など。
- `settledDelayMs`: settled 系 metric では必須。body drain 系 metric では `null`。
- `settledSnapshotName`: settled 系 metric では必須。body drain 系 metric では `null`。
- `deltaKb`: `compare - base`。比較できない場合は `null`。
- `ratio`: `compare / base`。比較できない場合は `null`。
- `comparisonStatus`: `ok`、`partial`、`skipped` のいずれか。
- `excludedReason`: 比較から除外する理由。比較可能なら省略する。

`scenarioComparisons[]` は、比較対象となる対照 fixture scenario が同一 JSON report 内にある場合だけ entry を生成する。primary run だけで対照 fixture が未実行の場合は空配列にする。`--fixture-density dense,sparse` で `prefix-full-dense` と `prefix-full-sparse` を同時実行した場合、および `--fixtures prefix,multifile --fixture-density dense` で `prefix-full-dense` と `multifile-full-dense` を同時実行した場合に scenario comparison を report に残す。別 process の JSON report 同士は merge しない。片方が `partial` の場合は `comparisonStatus="partial"`、片方が `failed` または未実行の場合は `comparisonStatus="skipped"` とし、delta と ratio は `null` にする。

settled 系 metric の `scenarioComparisons[]` は、`settled_1s`、`settled_5s`、任意の `settled_15s` を delay 別の entry として出す。body drain 系 metric では `settledDelayMs=null`、`settledSnapshotName=null` とする。

`scenarioComparisons[].ratio = compare / base` は、base metric が finite かつ `> 0` の場合だけ計算する。base metric が欠損、`0`、負値、非数値の場合は `ratio=null`、`comparisonStatus="partial"` とし、`excludedReason` に理由を残す。片方が `failed` または未実行の場合は `comparisonStatus="skipped"`、delta と ratio は `null` にする。

`reports[].timeline.derived.settledComparisons[]` の `peak_to_settled_delta_kb` と `peak_to_settled_ratio` が大きい場合は一時 live allocation 候補、`request_started_to_headers_delta_kb` が目立つ場合は server 側 JSON 直列化または response 構築候補、`headers_to_body_delta_kb` と `headers_to_body_peak_delta_kb` が目立つ場合は post-header body drain または socket buffering 候補、`body_peak_to_body_received_delta_kb` が大きい場合は body drain 中の一時 peak が body 完了時点で下がった候補、`default_vs_arena1_settled_delta_kb` が正方向に目立つ場合は glibc allocator retained memory 候補として記録する。数値 threshold は CI や自動判定に入れず、`docs/todo/BACKLOG.md` には実測値と解釈を併記する。

- `prefix-full-dense` だけが高く、`prefix-full-sparse` や multifile 対照が低い場合は、prefix many-match 経路固有の問題として扱う。
- `/proc` 取得が不完全な scenario は判定から除外し、部分測定として記録する。

RSS は OS、allocator、CPU、ディスクキャッシュ、同時実行 process の影響を受けるため、CI test に絶対しきい値は入れない。

## エラー処理

scenario ごとに `status` を記録し、他 scenario は可能な限り継続する。

- `ok`: HTTP 契約、JSON 契約、`/proc` snapshot がすべて揃った。
- `partial`: HTTP 契約と JSON 契約は成功したが、`/proc` または `smaps_rollup` の一部が不完全だった。
- `failed`: server 起動、HTTP、JSON 契約、fixture 生成、body parse、timeout のいずれかが失敗した。

`partial` は `partialMeasurementReasons` と `decisionExcludedReason` を持ち、Done 判断や比較結論の根拠から除外する。既存の `procComplete=false` は `partial` status へ対応付ける。

`runMeasuredScenario()` の成功結果を `runMeasurement()` が report 化するとき、`procComplete ? "ok" : "partial"` で `status` を決める。top-level report には `acceptanceStatus: "full" | "partial" | "failed"` と `fullAcceptanceMet: boolean` を必ず出す。`partial` は JSON report 生成自体は成功とみなし、通常 process exit code は `0` とする。ただし `acceptanceStatus="partial"`、`fullAcceptanceMet=false` とし、completion report と `docs/todo/BACKLOG.md` では Full acceptance 未達として扱う。`failed` が 1 件以上ある場合は `acceptanceStatus="failed"`、`fullAcceptanceMet=false`、通常 process exit code `1` にする。

fixture 生成も per-fixture / per-scenario の catch 対象にする。既存実装のように `createFixture(root, fixtureKind, scale)` が scenario loop の外で例外を投げる形にはしない。`createFixture(root, fixtureKind, scale, density)` が失敗した場合は、該当 `fixtureKind`、`fixtureDensity`、`mode`、`runKind`、`allocatorProfile` の report を `status="failed"`、`scenarioError.errorCode="fixture_failed"`、sanitized error fields 付きで出し、残り matrix は可能な限り継続する。`fixtureScale` は既存方針通り `measurementContext.cliOptions.fixtureScale` から参照し、failed report でも `reports[]` 直下には追加しない。

`scenarioError` は `reports[]` item 直下にだけ置き、field は `errorCode`、`sanitizedMessage`、`redactedContext` に限定する。raw stderr、raw process args、実パス、fixture 本文断片、HTTP response body、raw `/proc` 行、親 process env 値を error fields に入れない。

自動実行や CI 用に `--strict` を追加する。`--strict` では `acceptanceStatus` が `full` でない場合、つまり `partial` または `failed` が 1 件以上ある場合に process exit code を non-zero にする。

記録対象の失敗は次を含める。

- port 衝突。
- server 起動失敗。
- HTTP timeout。
- response body parse failure。
- JSON 契約不一致。
- fixture 生成失敗。

HTTP 契約が成功している場合の `/proc` 欠落と `smaps_rollup` 権限不足は `failed` ではなく `partial` とする。

sandbox の loopback bind 制限で server 起動が失敗する場合は、承認付き実行が必要になる。承認されない場合は、self-test と文書検証までを `Sandbox-limited partial validation` とし、full acceptance 未達として completion report に残す。

## データ更新

実装時の主な変更対象は次の 2 ファイルである。

- `scripts/measure-search-rss-plateau.mjs`: `--timeline`、`--strict`、`--self-test`、`--fixture-density`、`--settled-delays`、snapshot 名、sampling 間隔、timeline peaks、timeline report、derived fields、self-test。
- `docs/todo/BACKLOG.md`: 測定結果、判断、残件、セキュリティ境界維持。

設計書としてこのファイルを追加する。

production Rust code、UI、TypeScript、generated JS は変更しない。

## テストと検証

docs/config/script 変更なので、ceremonial TDD ではなく validation を使う。ただしスクリプトの契約は self-test で固定する。

実装後に確認する項目は次の通り。

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
node scripts/measure-search-rss-plateau.mjs --help
node scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1
git diff --check
./verify.sh
```

prefix 固有性を結論する場合は、追加で次を確認する。

```bash
node scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density dense,sparse --settled-delays 1s,5s --allocator-profiles default,arena1
node scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix,multifile --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1
```

`--timeline` が sandbox の loopback bind で失敗した場合は、承認付きで再実行する。承認されない場合は、未実行または失敗理由を completion report に残し、full acceptance 未達として扱う。

self-test では次を固定する。

- `--self-test` の parse。既存 `--self-test-sanitization` は後方互換 alias として残す。
- `--timeline` と `--strict` の parse。
- `--timeline`、`--fixture-density`、`--settled-delays` と `--smoke` の併用拒否。
- `--fixture-density` と `--settled-delays` の timeline 専用 parse と不正値拒否。
- `--fixture-density` は `dense`、`sparse`、`dense,sparse` だけを許可し、逆順、重複、空値を拒否すること。
- `--fixtures` に `multifile` または `fallback` が含まれる場合、`--fixture-density sparse` と `--fixture-density dense,sparse` を拒否すること。
- `--settled-delays` は `1s,5s` と `1s,5s,15s` だけを許可し、逆順、重複、`15s` 単独、baseline 欠落を拒否すること。
- timeline event snapshot 名、`elapsedMs` 昇順、settled delay の単調性。
- fake sampler と event snapshot による inclusive window での `timeline.peaks.requestPeakRss`、`requestPeakAnon`、`bodyDrainPeakRss`、`bodyDrainPeakAnon` の個別算出。
- fake HTTP response による `headers_received` と `body_received` の分離。
- `settled_1s` / `settled_5s` / 任意の `settled_15s` の delay 設定。
- body drain 中だけ一時的に増える fake sample が `headers_to_body_peak_delta_kb` と正の減少量としての `body_peak_to_body_received_delta_kb` に反映されること。
- `timeline.bodyReadStartElapsedMs`、`timeline.bodyReadEndElapsedMs`、`timeline.responseBytes` が出て、raw response body は出ないこと。
- `reports[].timeline.derived.settledComparisons[]` が `settledDelayMs` ごとに出て、`settled_1s`、`settled_5s`、任意の `settled_15s` の peak-to-settled delta と ratio が期待値になること。
- sampling summary に `samplingIntervalMs=50`、monotonic clock、`sampleCount`、`maxSamples`、`missedSampleReasons[]`、`droppedSampleReasons[]` が出ること。
- `--self-test` が必須 field の schema 階層を固定し、top-level `comparisons[]`、top-level `scenarioComparisons[]`、`reports[].timeline.derived.settledComparisons[]`、`reports[].timeline.responseBytes` を検証すること。
- `--help` が `--timeline`、`--strict`、`--self-test`、`--fixture-density`、`--settled-delays` を表示すること。
- timeline report sanitization が実パス、ユーザー名、本文断片、raw maps 行を出さないこと。
- `scenarioError` が `reports[]` item 直下にだけあり、field が `errorCode`、`sanitizedMessage`、`redactedContext` に限定され、raw stderr、raw process args、実パス、本文断片、raw `/proc`、親 process env 値を出さないこと。
- `fixtureDensity=sparse` が既存 fixture matrix と矛盾せず、`resultsLength=100` と `result_limit` 契約を維持すること。
- fake `/proc` 欠落と `smaps_rollup` 権限不足が `status="partial"`、`partialMeasurementReasons`、`decisionExcludedReason` を出すこと。
- server 起動失敗、HTTP failure、JSON failure、fixture 生成失敗、timeout が `status="failed"` になること。
- fixture 生成失敗が `scenarioError.errorCode="fixture_failed"` として report 化され、残り matrix が継続されること。
- failed scenario 後も残り scenario が report され、process exit code が `1` になること。
- 全 scenario が `ok` の場合は `acceptanceStatus="full"`、`fullAcceptanceMet=true`、`--strict` でも exit code `0` になること。
- partial だけの場合は通常 process exit code が `0`、`acceptanceStatus="partial"`、`fullAcceptanceMet=false` になり、`--strict` では non-zero になること。
- `comparisons[]` が `mode`、`runKind`、`fixtureKind`、`fixtureScale`、`fixtureDensity`、`settledDelayMs`、`settledSnapshotName`、profile pair で一意になり、partial 比較では delta 値を出さないこと。
- `comparisons[]` が `settledSnapshotName` に対応する `settled_*` snapshot の `status.RssAnon` を使って delta を出すこと。
- `comparisons[]` が top-level にあり、`reports[]` 配下に出ないこと。
- default のみ指定、arena1 failed、片側欠損の fake report group が `comparisonStatus="skipped"`、`missingProfiles`、`excludedReason`、delta 非出力になること。
- `scenarioComparisons[]` が top-level にあり、primary run だけなら空配列、同一 JSON report 内の対照 fixture 実行時は prefix dense/sparse と prefix dense/multifile の比較を `ok` / `partial` / `skipped` で固定すること。
- settled 系 `scenarioComparisons[]` が `settled_1s`、`settled_5s`、任意の `settled_15s` を delay 別 entry として出すこと。
- `scenarioComparisons[].ratio` は base metric が finite かつ `> 0` の場合だけ計算し、それ以外では `ratio=null` と `excludedReason` を出すこと。

## セキュリティ

検索 query、fixture 本文、HTTP response、`/proc` 出力、外部環境変数は未信頼入力として扱う。取得済みテキスト、検索結果、issue、LLM 出力を shell、SQL、policy、コードとして実行しない。

出力 sanitization は維持し、docs と JSON report に次を残さない。

- 実パス。
- ローカルユーザー名。
- full process args。
- Markdown 本文断片。
- raw `/proc/maps` 行。
- 親 process の環境変数一覧または値。

server process に追加する環境変数は、allocator profile 定義で許可された値に限定する。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めない。

JSON report の `measurementContext` には再現性に必要な `node`、`rustc -V`、`cargo -V`、platform、OS release、git HEAD、build mode、allocator profile、settled delays を含める。ただし full env、実パス、親 process の環境変数値は含めない。

## 影響範囲

直接影響するのは診断スクリプトとバックログ文書である。通常の server 起動、検索 API、WebSocket、renderer、watcher、browser UI には影響しない。

確認対象は次の通り。

- `scripts/measure-search-rss-plateau.mjs`
- `docs/todo/BACKLOG.md`
- `src/server/files/search.rs`

`src/server/files/search.rs` は production code を変更しないことの確認対象であり、編集対象ではない。

## ロールバック

スクリプト拡張、`BACKLOG.md` 追記、この設計書 commit を revert すればよい。production Rust code は変更しないため、HTTP API、WebSocket、検索アルゴリズム、UI の追加 rollback は不要である。fixture は `/tmp/markdown-view-search-rss-plateau.***` 配下に生成されるため、削除してもリポジトリには影響しない。

## 受け入れ基準

Full acceptance:

- `scripts/measure-search-rss-plateau.mjs` が opt-in の timeline 測定を実行できる。
- timeline report に top-level `comparisons[]`、top-level `scenarioComparisons[]`、`reports[].scenarioId`、`reports[].fixtureDensity`、`reports[].timeline.snapshots[]`、`reports[].timeline.peaks`、`reports[].timeline.samplingSummary`、`reports[].timeline.bodyReadStartElapsedMs`、`reports[].timeline.bodyReadEndElapsedMs`、`reports[].timeline.responseBytes`、`reports[].timeline.derived`、`reports[].timeline.derived.settledComparisons[]`、`reports[].timeline.settledDelaysMs`、`acceptanceStatus`、`fullAcceptanceMet` が含まれる。
- release / prefix / full / dense / cold で `default` と `arena1` の timeline 比較結果を得る。
- prefix 固有性を結論する場合は、prefix-full-sparse/cold と multifile-full-dense/cold の対照測定も成功している。
- `docs/todo/BACKLOG.md` に、live allocation 候補、allocator retained memory 候補、server response 構築候補、post-header body drain 候補、次に必要な作業が記録される。
- 検索 API 契約、Host/Origin/path validation、HTML sanitize、CSP、検索上限契約を弱めていないことが記録される。
- self-test と文書 validation が成功する。
- `./verify.sh` が pass する。未実行または失敗がある場合は、理由と残リスクを報告する。

Sandbox-limited partial validation:

- self-test、help、文書 validation、`git diff --check` は成功している。
- loopback bind 制限や承認不可により timeline 実測が未実行の場合、full acceptance 未達として completion report に明記している。
- `docs/todo/BACKLOG.md` を Done 扱いにせず、実測未完了の残リスクと次作業候補を残している。

completion report には、変更ファイルと理由、rough line impact、影響する依存ファイル、実行した検証、未実行検証、残リスク、次作業候補、セキュリティ境界維持、`rustc -V`、`cargo -V`、platform、OS release、git HEAD、build mode、allocator profile、settled delays を含める。

## 見積もり

- 人間作業: 1.5-3 時間。
- Codex / AI 支援: 45-90 分。

loopback bind 承認、release build、full fixture timeline 測定、`--settled-delays` に `15s` を含めるかどうかで変動する。

## 文書検証

設計書自体は docs-only なので、次で検証する。

```bash
set -euo pipefail
for heading in \
  "目的" "非ゴール" "測定アーキテクチャ" "比較 fixture" "判定方針" "エラー処理" \
  "データ更新" "テストと検証" "セキュリティ" "影響範囲" "ロールバック" \
  "受け入れ基準" "見積もり" "文書検証"
do
  rg -q "^## ${heading}$" docs/superpowers/specs/2026-06-08-search-rss-live-allocation-timeline-design.md \
    || { printf 'missing heading: %s\n' "$heading"; exit 1; }
done
content_before_validation="$(sed '/^## 文書検証$/,$d' docs/superpowers/specs/2026-06-08-search-rss-live-allocation-timeline-design.md)"
for term in \
  'fixtureDensity' 'scenarioId' 'timeline\.snapshots\[\]' 'timeline\.peaks' \
  'timeline\.samplingSummary' 'timeline\.derived' 'comparisons\[\]' 'scenarioComparisons\[\]' \
  '--fixture-density' 'dense,sparse' '--settled-delays' '--strict' '--self-test' \
  'requestPeakRss' 'requestPeakAnon' 'bodyDrainPeakRss' \
  'bodyDrainPeakAnon' 'headers_to_body_peak_delta_kb' 'partialMeasurementReasons' \
  'decisionExcludedReason' 'acceptanceStatus' 'fullAcceptanceMet' 'fixture_failed' \
  'samplingIntervalMs' 'droppedSampleReasons' 'settledComparisons\[\]' \
  'settledSnapshotName' 'bodyReadStartElapsedMs' 'bodyReadEndElapsedMs' 'responseBytes' \
  'scenarioError' 'errorCode' 'sanitizedMessage' 'excludedReason' \
  'reports\[\]\.timeline\.derived\.settledComparisons\[\]' \
  'timeline\.peaks\.bodyDrainPeakAnon\.status\.RssAnon - body_received\.status\.RssAnon' \
  'inclusive window' 'multifile.*fallback.*--fixture-density sparse' \
  '--timeline.*--strict.*--self-test.*--fixture-density.*--settled-delays'
do
  printf '%s\n' "$content_before_validation" | rg -q -- "${term}" \
    || { printf 'missing term: %s\n' "$term"; exit 1; }
done
placeholder_matches="$(rg -n -P 'T[B]D|TO[D]O(?!\\.md| Issues)|未[定]' docs/superpowers/specs/2026-06-08-search-rss-live-allocation-timeline-design.md | rg -v 'T\\[B\\]D|TO\\[D\\]O|未\\[定\\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
stale_matches="$(printf '%s\n' "$content_before_validation" | rg -n 'settled\.status\.RssAnon|default\.settled\.RssAnon|arena1\.settled\.RssAnon|errorKind|各 snapshot には.*scenarioError|body_received\.status\.RssAnon - timeline\.peaks\.bodyDrainPeakAnon\.status\.RssAnon|no-op dimension' || :)"
test -z "$stale_matches" || { printf '%s\n' "$stale_matches"; exit 1; }
git diff --check
```
