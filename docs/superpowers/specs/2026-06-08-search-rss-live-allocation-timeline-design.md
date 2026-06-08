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

`--timeline` は重い診断モードとして扱い、`--smoke` とは併用不可にする。既存 `--smoke` は軽量な dev/prefix/cold/default の契約確認に限定し、timeline sampling を暗黙に走らせない。`--fixture-density` は `dense` / `sparse` を受け取り、既定は `dense` とする。`--settled-delays` は `1s,5s` を既定にし、追加確認が必要な場合だけ `1s,5s,15s` を指定する。

report には既存の `fixtureKind` と `fixtureScale` を維持し、timeline 用に次の field を追加する。

- `fixtureDensity`: `dense` または `sparse`。
- `scenarioId`: `prefix-full-dense` のように fixture kind、scale、density を連結した表示用 ID。
- `timeline.settledDelaysMs`: 実際に採取した settled delay の millisecond 配列。
- `timeline.snapshots[]`: snapshot の時系列配列。
- `timeline.derived`: 判定補助用の差分と比率。
- `comparisons[]`: allocator profile 間の比較結果。

`prefix-full` という語は `--fixtures prefix --fixture-scale full --fixture-density dense` の短い説明名であり、CLI の fixture kind ではない。

timeline 測定では、scenario ごとに次の snapshot を記録する。

- `server_ready`: server 起動後、request 前。
- `request_started`: HTTP request 送信直後。
- `request_peak_rss`: request 中 sampling で `VmRSS` が最大の snapshot。
- `request_peak_anon`: request 中 sampling で `RssAnon` が最大の snapshot。
- `headers_received`: HTTP response headers 受信時。
- `body_received`: response body 受信完了直後。
- `settled_1s`: body 受信完了から 1 秒後。
- `settled_5s`: body 受信完了から 5 秒後。
- `settled_15s`: `--settled-delays` に `15s` が含まれる場合だけ body 受信完了から 15 秒後。

各 snapshot には、取得できる範囲で次を含める。

- `name`。
- `VmRSS`、`RssAnon`、`RssFile`、`RssShmem`。
- `smaps_rollup Anonymous`。
- elapsed milliseconds。
- `/proc` 取得 completeness。
- scenario error。

timeline 用 request path は通常測定の `requestSearch()` と分ける。`fetch()` が response headers を返した直後に `headers_received` を採取し、その後に body stream を `readResponseTextWithLimit()` 相当の上限付き処理で読み切って `body_received` を採取する。response body 読み取りの開始/終了 elapsed と response bytes は report に含める。

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
- `multifile/full/dense`: prefix 経路固有性を確認する必須対照。

`prefix-full-sparse` は、dense と byte size を ±5% 以内に揃えつつ、hit 密度と result-limit 到達位置を変えた場合に peak / settled の差が変わるかを見るための補助 scenario である。`sparse` は `fixtureKind=prefix` の場合だけ意味を持ち、`multifile` と `fallback` では no-op dimension として `scenarioId` と report にだけ残す。測定 matrix が過剰にならないよう、既定 timeline run では `prefix-full-dense` を主対象にし、prefix 固有性を結論する場合だけ `prefix-full-sparse` と `multifile/full/dense` も実行する。fallback は任意の補助測定であり、prefix 固有性の Full acceptance 条件には含めない。

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

`reports[].timeline.derived` には次を入れる。

- `peak_to_settled_delta_kb`: `request_peak_anon.RssAnon` と最長 settled snapshot の `RssAnon` 差。
- `peak_to_settled_ratio`: `request_peak_anon.RssAnon / settled.RssAnon`。
- `request_started_to_headers_delta_kb`: `request_started` と `headers_received` の `RssAnon` 差。
- `headers_to_body_delta_kb`: `headers_received` と `body_received` の `RssAnon` 差。

top-level `comparisons[]` には allocator profile 間比較を入れる。

- `scenarioId`。
- `settledDelayMs`。
- `baseProfile`: `default`。
- `compareProfile`: `arena1`。
- `default_vs_arena1_settled_delta_kb`。
- `comparisonStatus`: `ok`、`partial`、`skipped` のいずれか。
- `excludedReason`: 比較から除外する理由。比較可能なら省略する。
- `missingProfiles`: 指定されなかった profile 名の配列。欠損がなければ空配列。

`default` と `arena1` が両方 `ok` の場合だけ `comparisonStatus="ok"` とする。片方が `partial` の場合は `comparisonStatus="partial"` とし、片方が `failed` または未指定の場合は `comparisonStatus="skipped"` とする。

`peak_to_settled_delta_kb` と `peak_to_settled_ratio` が大きい場合は一時 live allocation 候補、`request_started_to_headers_delta_kb` が目立つ場合は server 側 JSON 直列化または response 構築候補、`headers_to_body_delta_kb` が目立つ場合は post-header body drain または socket buffering 候補、`default_vs_arena1_settled_delta_kb` が目立つ場合は glibc allocator retained memory 候補として記録する。数値 threshold は CI や自動判定に入れず、`docs/todo/BACKLOG.md` には実測値と解釈を併記する。

- `prefix-full-dense` だけが高く、`prefix-full-sparse` や multifile 対照が低い場合は、prefix many-match 経路固有の問題として扱う。
- `/proc` 取得が不完全な scenario は判定から除外し、部分測定として記録する。

RSS は OS、allocator、CPU、ディスクキャッシュ、同時実行 process の影響を受けるため、CI test に絶対しきい値は入れない。

## エラー処理

scenario ごとに `status` を記録し、他 scenario は可能な限り継続する。

- `ok`: HTTP 契約、JSON 契約、`/proc` snapshot がすべて揃った。
- `partial`: HTTP 契約と JSON 契約は成功したが、`/proc` または `smaps_rollup` の一部が不完全だった。
- `failed`: server 起動、HTTP、JSON 契約、fixture 生成、body parse、timeout のいずれかが失敗した。

`partial` は `partialMeasurementReasons` と `decisionExcludedReason` を持ち、Done 判断や比較結論の根拠から除外する。既存の `procComplete=false` は `partial` status へ対応付ける。

`runMeasuredScenario()` の成功結果を `runMeasurement()` が report 化するとき、`procComplete ? "ok" : "partial"` で `status` を決める。`partial` は JSON report 生成自体は成功とみなし、process exit code は `0` とする。ただし completion report と `docs/todo/BACKLOG.md` では Full acceptance 未達として扱う。`failed` が 1 件以上ある場合だけ process exit code を `1` にする。

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

- `scripts/measure-search-rss-plateau.mjs`: `--timeline`、`--fixture-density`、`--settled-delays`、snapshot 名、sampling 間隔、timeline report、derived fields、self-test。
- `docs/todo/BACKLOG.md`: 測定結果、判断、残件、セキュリティ境界維持。

設計書としてこのファイルを追加する。

production Rust code、UI、TypeScript、generated JS は変更しない。

## テストと検証

docs/config/script 変更なので、ceremonial TDD ではなく validation を使う。ただしスクリプトの契約は self-test で固定する。

実装後に確認する項目は次の通り。

```bash
node scripts/measure-search-rss-plateau.mjs --self-test-sanitization
node scripts/measure-search-rss-plateau.mjs --help
node scripts/measure-search-rss-plateau.mjs --timeline --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1
git diff --check
./verify.sh
```

prefix 固有性を結論する場合は、追加で次を確認する。

```bash
node scripts/measure-search-rss-plateau.mjs --timeline --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density sparse --settled-delays 1s,5s --allocator-profiles default,arena1
node scripts/measure-search-rss-plateau.mjs --timeline --modes release --fixtures multifile --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1
```

`--timeline` が sandbox の loopback bind で失敗した場合は、承認付きで再実行する。承認されない場合は、未実行または失敗理由を completion report に残し、full acceptance 未達として扱う。

self-test では次を固定する。

- `--timeline` の parse。
- `--timeline` と `--smoke` の併用拒否。
- `--fixture-density` と `--settled-delays` の parse と不正値拒否。
- `--settled-delays` は `1s`、`5s`、`15s` の組み合わせだけ許可すること。
- timeline snapshot 名、順序、elapsed 単調性。
- fake sampler による `request_peak_rss` と `request_peak_anon` の個別算出。
- fake HTTP response による `headers_received` と `body_received` の分離。
- `settled_1s` / `settled_5s` / 任意の `settled_15s` の delay 設定。
- timeline report sanitization が実パス、ユーザー名、本文断片、raw maps 行を出さないこと。
- `fixtureDensity=sparse` が既存 fixture matrix と矛盾せず、`resultsLength=100` と `result_limit` 契約を維持すること。
- fake `/proc` 欠落と `smaps_rollup` 権限不足が `status="partial"`、`partialMeasurementReasons`、`decisionExcludedReason` を出すこと。
- server 起動失敗、HTTP failure、JSON failure、fixture 生成失敗、timeout が `status="failed"` になること。
- failed scenario 後も残り scenario が report され、process exit code が `1` になること。
- partial だけの場合は process exit code が `0` になり、Full acceptance 未達として報告できること。

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
- timeline report に `timeline.snapshots[]`、`timeline.derived`、`comparisons[]`、`scenarioId`、`fixtureDensity`、`timeline.settledDelaysMs` が含まれる。
- release / prefix / full / dense / cold で `default` と `arena1` の timeline 比較結果を得る。
- prefix 固有性を結論する場合は、prefix / full / sparse / cold と multifile / full / dense / cold の対照測定も成功している。
- `docs/todo/BACKLOG.md` に、live allocation 候補、allocator retained memory 候補、server response 構築候補、post-header body drain 候補、次に必要な作業が記録される。
- 検索 API 契約、Host/Origin/path validation、HTML sanitize、CSP、検索上限契約を弱めていないことが記録される。
- self-test と文書 validation が成功する。
- `./verify.sh` が pass する。未実行または失敗がある場合は、理由と残リスクを報告する。

Sandbox-limited partial validation:

- self-test、help、文書 validation、`git diff --check` は成功している。
- loopback bind 制限や承認不可により timeline 実測が未実行の場合、full acceptance 未達として completion report に明記している。
- `docs/todo/BACKLOG.md` を Done 扱いにせず、実測未完了の残リスクと次作業候補を残している。

completion report には、変更ファイルと理由、rough line impact、影響する依存ファイル、実行した検証、未実行検証、残リスク、次作業候補、セキュリティ境界維持を含める。

## 見積もり

- 人間作業: 1.5-3 時間。
- Codex / AI 支援: 45-90 分。

loopback bind 承認、release build、full fixture timeline 測定、`--settled-delays` に `15s` を含めるかどうかで変動する。

## 文書検証

設計書自体は docs-only なので、次で検証する。

```bash
rg -n "目的|非ゴール|測定アーキテクチャ|比較 fixture|判定方針|エラー処理|データ更新|テストと検証|セキュリティ|影響範囲|ロールバック|受け入れ基準|Full acceptance|Sandbox-limited partial validation|見積もり" docs/superpowers/specs/2026-06-08-search-rss-live-allocation-timeline-design.md
rg -n "fixtureDensity|scenarioId|timeline\\.snapshots|timeline\\.derived|comparisons\\[\\]|--fixture-density|--settled-delays|request_peak_rss|request_peak_anon|request_started_to_headers_delta_kb|partialMeasurementReasons|decisionExcludedReason|process exit code" docs/superpowers/specs/2026-06-08-search-rss-live-allocation-timeline-design.md
placeholder_matches="$(rg -n -P 'T[B]D|TO[D]O(?!\\.md| Issues)|未[定]' docs/superpowers/specs/2026-06-08-search-rss-live-allocation-timeline-design.md | rg -v 'T\\[B\\]D|TO\\[D\\]O|未\\[定\\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
git diff --check
```
