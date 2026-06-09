# 検索 RSS live allocation 次タスク選定設計

## 目的

`docs/todo/BACKLOG.md` の未完了候補から、次に実行する1件として「ディレクトリ検索 prefix many-match RSS plateau の環境差・live allocation 追加診断」を選ぶ。

この作業の目的は、prefix many-match 検索で残る RSS / anonymous memory が、主にどこで増えてどこまで残るのかを timeline で分類することである。分類対象は次の通り。

- request 中だけ大きく増える一時 live allocation。
- response 完了後も残る allocator retained memory。
- server 側 JSON 直列化または response 構築の寄与。
- post-header body drain または socket buffering の寄与。
- prefix many-match 経路固有の挙動。

今回の成果は、次に Rust 側の allocation 削減、allocator profile 追加、または追加 probe へ進む価値があるかを判断できる材料である。

## 非ゴール

- production Rust code の改善実装。
- 検索アルゴリズム、allocator、Tokio runtime 構成の変更。
- `/api/search` の JSON shape や検索結果の意味変更。
- UI、TypeScript、generated JS の変更。
- `MALLOC_ARENA_MAX=1` の常用化判断。
- native Linux や別 allocator build での環境差検証を必須にすること。
- Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約の変更。

## 選定理由

`TODO.md` には現時点で未完了の High / Medium 候補がない。`BACKLOG.md` の未完了 P2 は、prefix many-match RSS plateau の追加診断と、64MiB byte-limit plateau の再確認である。

実装価値を優先する場合、prefix many-match の追加診断を先に扱う。直近の `2026-06-08-search-rss-live-allocation-timeline-design.md` と検索 RSS plateau の履歴に直結しており、次の改善実装へ進むかどうかを判断しやすい。64MiB byte-limit 側は、byte-limit 超過候補の本文読込を既に改善済みであり、残る plateau の切り分け価値はあるが、次の実装判断までの距離がやや長い。

prefix と byte-limit を横断する統一診断設計は長期的には有効だが、今回の「次に実行する1件」としては範囲が広い。そのため採用しない。

## 実装アプローチ

中心は `scripts/measure-search-rss-plateau.mjs` の opt-in 拡張である。既存の測定スクリプトに `--timeline` を追加し、通常 smoke 測定とは分離する。`--timeline` は重い診断なので、`--smoke` と併用不可にする。

timeline では、少なくとも次の snapshot を記録する。

- `server_ready`
- `request_started`
- `headers_received`
- `body_received`
- `settled_1s`
- `settled_5s`

request 中は 50ms 間隔で sample を取り、raw sample 全件は保存しない。report には判断に必要な peak snapshot だけを残す。

- `requestPeakRss`
- `requestPeakAnon`
- `bodyDrainPeakRss`
- `bodyDrainPeakAnon`

比較はまず `release / prefix / full / dense / cold` を主対象にし、allocator profile は `default` と `arena1` を基本にする。prefix 固有性まで結論する必要がある場合だけ、`prefix-full-sparse` と `multifile-full-dense` を追加対照にする。

## データ設計

report は raw data を持たず、判断に必要な派生値を残す。

- `reports[].timeline.derived.settledComparisons[]`: peak-to-settled の差分と比率。
- `comparisons[]`: `default` と `arena1` の settled anonymous memory 差分。
- `scenarioComparisons[]`: prefix dense/sparse や prefix/multifile の比較。
- `acceptanceStatus`: `full`、`partial`、`failed`、`not_applicable`。`not_applicable` は timeline を要求していない通常測定で、timeline acceptance 判定対象外であることを示す。
- `fullAcceptanceMet`: full acceptance を満たしたかどうか。
- `acceptanceReasons`: full acceptance に至らない理由。`timeline_not_requested`、`scenario_comparison_not_ok`、`server_cleanup_failed`、`partial_proc_measurement` など、判断に必要な理由を top-level に集約する。
- `decisionExcludedReason`: report または top-level decision を判断対象外にした主理由。

`headers_received` から `body_received` までの差分は、server、client、socket、undici buffering を含む外部観測値として扱う。server 内部の JSON serialization 完了時点とは同一視しない。

## エラー処理

scenario 単位で `ok` / `partial` / `failed` に分ける。

- `ok`: HTTP 契約、JSON 契約、`/proc` snapshot が揃った。
- `partial`: HTTP 契約と JSON 契約は成功したが、`/proc` または `smaps_rollup` の一部が不完全だった。
- `failed`: server 起動失敗、HTTP failure、JSON 契約不一致、fixture 生成失敗、timeout のいずれかが起きた。

`partial` でも JSON report は出す。通常 process exit code は `0` とし、`acceptanceStatus="partial"`、`fullAcceptanceMet=false` にする。`--strict` では `partial` も non-zero にして、明示検証や CI 相当の用途で使えるようにする。timeline を要求していない通常測定が成功した場合は、`acceptanceStatus="not_applicable"`、`fullAcceptanceMet=false`、`decisionExcludedReason="timeline_not_requested"`、`acceptanceReasons=["timeline_not_requested"]` にする。

`scenarioError` は `reports[]` item 直下にだけ置き、通常は `errorCode`、`sanitizedMessage`、`redactedContext` を持つ。cleanup 失敗を伴う場合は、診断用に `cleanupFailed`、`cleanupErrorKind`、`cleanupMessage` も `scenarioError` 直下に追加できる。raw stderr、raw process args、実パス、fixture 本文断片、HTTP response body、raw `/proc` 行、親 process env 値は入れない。

## セキュリティ

fixture 本文、検索 query、HTTP response、`/proc` 出力、外部環境変数は未信頼入力として扱う。取得済みテキスト、検索結果、issue、LLM 出力を shell、SQL、policy、コードとして実行しない。

report や docs には次を残さない。

- 実パス。
- ローカルユーザー名。
- full process args。
- Markdown 本文断片。
- raw `/proc/maps` 行。
- 親 process の環境変数一覧または値。

server process に追加する環境変数は、allocator profile 定義で許可された値に限定する。検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約は弱めない。

## 影響範囲

直接影響するのは診断スクリプトとバックログ文書である。

- 変更対象: `scripts/measure-search-rss-plateau.mjs`
- 変更対象: `docs/todo/BACKLOG.md`
- 確認対象: `src/server/files/search.rs`

`src/server/files/search.rs` は production code を変更しないことの確認対象であり、編集対象ではない。通常の server 起動、検索 API、WebSocket、renderer、watcher、browser UI には影響しない。

## 受け入れ基準

Full acceptance:

- `scripts/measure-search-rss-plateau.mjs` が opt-in の timeline 測定を実行できる。
- timeline report に snapshots、peaks、samplingSummary、body read timing、responseBytes、derived、allocator comparisons、acceptanceStatus、fullAcceptanceMet が含まれる。
- release / prefix / full / dense / cold で `default` と `arena1` の timeline 比較結果を得る。
- `docs/todo/BACKLOG.md` に、live allocation 候補、allocator retained memory 候補、server response 構築候補、post-header body drain 候補、次に必要な作業が記録される。
- 検索 API 契約、Host/Origin/path validation、HTML sanitize、CSP、検索上限契約を弱めていないことが記録される。
- `--self-test`、`--help`、`git diff --check` が成功する。
- `./verify.sh` が pass する。未実行または失敗がある場合は、理由と残リスクを報告する。

prefix 固有性を結論する場合は、prefix-full-sparse/cold と multifile-full-dense/cold の対照測定も成功していることを追加条件にする。

Sandbox-limited partial validation:

- `--self-test`、`--help`、`git diff --check` は成功している。
- loopback bind 制限や承認不可により timeline 実測が未実行の場合、full acceptance 未達として completion report に明記している。
- `docs/todo/BACKLOG.md` を Done 扱いにせず、実測未完了の残リスクと次作業候補を残している。

## 検証

実装後に最低限確認する。

```bash
node scripts/measure-search-rss-plateau.mjs --self-test
node scripts/measure-search-rss-plateau.mjs --help
node scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1
git diff --check
./verify.sh
```

prefix 固有性を結論する場合は、追加で確認する。

```bash
node scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix --runs cold --fixture-scale full --fixture-density dense,sparse --settled-delays 1s,5s --allocator-profiles default,arena1
node scripts/measure-search-rss-plateau.mjs --timeline --strict --modes release --fixtures prefix,multifile --runs cold --fixture-scale full --fixture-density dense --settled-delays 1s,5s --allocator-profiles default,arena1
```

`--timeline` が sandbox の loopback bind で失敗した場合は、承認付きで再実行する。承認されない場合は、未実行または失敗理由を completion report に残し、full acceptance 未達として扱う。

## ロールバック

スクリプト拡張、`BACKLOG.md` 追記、この設計書 commit を revert すればよい。production Rust code は変更しないため、HTTP API、WebSocket、検索アルゴリズム、UI の追加 rollback は不要である。fixture は `/tmp/markdown-view-search-rss-plateau.***` 配下に生成されるため、削除してもリポジトリには影響しない。

## 見積もり

- 人間作業: 1.5-3 時間。
- Codex / AI 支援: 45-90 分。

loopback bind 承認、release build、full fixture timeline 測定、`--settled-delays` に `15s` を含めるかどうかで変動する。
