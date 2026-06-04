# Search RSS Plateau Design

## Goal

`docs/todo/TODO.md` の Medium Priority「ディレクトリ検索 many-match の RSS plateau を切り分ける」を、再現可能な測定と最小プローブ追加で進める。

このフェーズの目的は、10MiB 近傍 many-match 検索後に残る RSS / `RssAnon` plateau の支配要因を分類することである。dev/release、cold/warm、単一巨大ファイル/複数ファイル、prefix fast path、安全境界なし巨大 block fallback を分けて測る。測定結果は実パス、full process args、Markdown 本文断片を残さずに記録する。

完了時には、RSS plateau を次のいずれかへ分類する。

- Tokio runtime / blocking pool 起因。
- glibc allocator arena または retained anonymous memory 起因。
- WSL2 や `/proc` 計測特性起因。
- 安全境界なし巨大 block fallback 起因。
- 未特定だが、次に必要な追加測定が明確。

## Non-Goals

- `/api/search` の外部 JSON 契約変更。
- UI、TypeScript、generated JS の変更。
- 検索アルゴリズム、allocator、Tokio runtime 構成の本格改善実装。
- 恒久 telemetry、metrics 基盤、外部依存の追加。
- Host/Origin 検証、path validation、HTML sanitize、CSP、ファイルサイズ上限の変更。

原因が見えても、このフェーズでは改善実装候補を `TODO.md` の結果・残リスクへ落とすところまでに留める。

## Architecture

主な追加対象は `scripts/measure-search-rss-plateau.mjs` とする。Node.js 標準ライブラリだけを使い、測定 fixture 作成、preview server 起動、HTTP 検索、RSS / `/proc` 情報収集、結果 sanitization を一貫して実行する。

script の責務:

- `/tmp/markdown-view-search-rss-plateau.***` に測定 fixture を作る。
- dev は `cargo run -- ...`、release は既存 release binary または `cargo run --release -- ...` で server を起動する。
- localhost の `/api/search?q=needle` へ検索リクエストを投げる。
- request 前、request 中 peak、request 後、5秒後 settled の RSS / status を採る。
- `/proc/<pid>/status`、可能なら `smaps_rollup`、`maps` の概要を読む。
- HTTP JSON から `searched_files`、`searched_bytes`、`truncated`、`truncated_reasons`、`results.length`、response bytes を記録する。
- PID と数値は出すが、実パス、full process args、本文断片は出さない。

Rust 本体は原則変更しない。必要になった場合でも、計測安定化のための小さなテスト専用 helper までに限定し、production 経路の挙動は変えない。

## Measurement Matrix

最低限の測定 matrix:

- dev cold。
- dev warm。
- release cold。
- release warm。
- 10MiB 近傍の単一 many-match file。
- 複数ファイル分散 many-match。
- 安全な Markdown block / paragraph 境界ありの prefix fast path。
- 安全境界なしの巨大単一 block fallback。

各 run で記録する値:

- build mode。
- fixture kind。
- cold / warm。
- elapsed。
- peak RSS。
- after RSS。
- 5秒後 settled RSS。
- `VmRSS`、`RssAnon`、`RssFile`、`RssShmem`。
- `smaps_rollup` が読める場合の rollup 値。
- `maps` の anonymous / file-backed / heap / stack などの分類別 KiB 概要。
- HTTP status。
- `searched_files`。
- `searched_bytes`。
- `truncated`。
- `truncated_reasons`。
- `results.length`。
- response bytes。

`maps` の raw 行は保存しない。分類と KiB 合計だけを記録する。

## Data Handling

検索 query、fixture Markdown、HTTP response、`/proc` から取得した文字列は untrusted input として扱う。script は出力前に情報を絞り込み、次の値を記録しない。

- 実ファイルパス。
- full process args。
- Markdown 本文断片。
- raw `/proc/<pid>/maps` 行。
- query 以外の任意入力文字列。

fixture は repo 外の `/tmp/markdown-view-search-rss-plateau.***` に作成し、repository へ追加しない。committed docs には exact temp path ではなく masked path だけを残す。

## Error Handling

server 起動失敗、port 使用中、HTTP 失敗、`/proc` 読み取り失敗、`smaps_rollup` 不在は、測定失敗または部分測定として明示する。`smaps_rollup` や `maps` が使えない環境でも、`status` と RSS series が取れるなら測定は継続する。

script は server process を終了してから exit する。終了できなかった場合は PID だけを表示し、full args は表示しない。

## Security

この変更は観測性と可用性調査のためのものであり、既存のセキュリティ境界を変更しない。

- Host/Origin 検証を維持する。
- path validation と nofollow open を維持する。
- workspace exclusion を維持する。
- HTML sanitize と CSP を維持する。
- ファイルサイズ上限と検索上限契約を維持する。
- 計測出力に実パス、full process args、本文断片を残さない。

測定 fixture も検索 query も untrusted input として扱う。LLM 生成や外部取得したコマンド列を script に取り込まない。

## Testing And Verification

設計に対する実装計画では、次を検証対象にする。

- `node scripts/measure-search-rss-plateau.mjs --help` が成功する。
- short fixture の smoke 測定が成功する。
- smoke 測定出力に実パス、full process args、本文断片、raw maps 行が含まれない。
- dev/release、cold/warm、単一/複数、prefix/fallback の測定結果を得られる。
- `./verify.sh` が pass する。

Rust 本体を変更しない場合でも、既存検索 API への回帰がないことを `./verify.sh` で確認する。計測 script が環境依存で full matrix を実行できない場合は、実行できた範囲と不足した測定を明記する。

## Impact Scope

変更対象:

- `scripts/measure-search-rss-plateau.mjs`
- `docs/todo/TODO.md`

確認対象:

- `src/server/files/search.rs`
- `docs/superpowers/specs/2026-06-03-search-many-match-streaming-budget-design.md`
- `docs/superpowers/plans/2026-06-03-search-many-match-streaming-budget.md`

Rust production code、UI、TypeScript、generated JS は原則変更しない。

## Rollback

追加 script と `TODO.md` の測定結果更新を戻す。アプリ本体の動作を変えない設計なので、UI、HTTP API、WebSocket、検索アルゴリズムの巻き戻しは不要である。

もし計測安定化のために小さな test-only helper を追加した場合は、その helper と対応テストも同時に戻す。

## Acceptance Criteria

- `scripts/` 配下の計測補助で同じ手順を再実行できる。
- dev/release、cold/warm、単一/複数、prefix/fallback の測定結果が記録される。
- `/proc/*/status` と map 概要から RSS plateau の支配要因を分類できる。
- 未特定の場合でも、次に必要な追加測定が具体化されている。
- 測定結果に実パス、full process args、Markdown 本文断片、raw maps 行が含まれない。
- Host/Origin/path validation、HTML sanitize、CSP、ファイルサイズ上限、検索 API 契約が変わらない。
- `./verify.sh` が pass する、または未実行・失敗理由と残リスクが明記される。

## Effort Estimate

人間の作業見積もり: 0.5-1日。測定 matrix の実行時間と環境差の確認に時間がかかる。

Codex / AI 支援時の見積もり: 2-4時間。script 実装、smoke 測定、結果整理、`TODO.md` 更新までを含む。full matrix の実行時間が長い場合は追加で待ち時間が発生する。
