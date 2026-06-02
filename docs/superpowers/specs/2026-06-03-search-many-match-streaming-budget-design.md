# Search Many-Match Streaming Budget Design

## Goal

ディレクトリ検索の 10MiB 近傍 many-match 経路で、cold run の遅さと RSS 増加の主因を解消する。測定だけではなく、result-limit 到達後も Markdown 全体の block 抽出や巨大 allocation が続く構造をなくす。

完了判断は二段にする。

- ローカル計測では dev/release、cold/warm、単一巨大ファイル/複数ファイルを分け、elapsed、peak RSS、after RSS、5秒後 settled RSS、HTTP JSON の `searched_files`、`searched_bytes`、`truncated`、`truncated_reasons`、結果件数を記録する。
- CI では環境依存の性能しきい値を固定せず、result budget 到達後に後続 block 抽出、巨大ブロック探索、context 生成が進まないことを構造テストで保証する。

## Non-Goals

- `/api/search` の外部 JSON 契約変更。
- `SearchResponse`、`SearchResultItem`、既存 `truncated_reasons` の意味変更。
- UI 表示、TypeScript の検索応答契約、ファイルサイズ上限の変更。
- Host/Origin 検証、path validation、nofollow open、除外ディレクトリ、HTML sanitize、CSP の変更。
- 恒久的な本番 telemetry の追加。

必要な場合はテスト用 hook や局所的な計測 helper を追加するが、本番ログへ実パス、full process args、検索本文断片を出さない。

## Current Problem

現行の検索経路は、各ファイルで Markdown 全体から `Vec<SearchBlockEntry>` を構築し、その後 `find_matches_for_file()` が block 一覧を照合する。前段の result budget 改善により match/context 生成は 100 件で止まるようになったが、block 抽出自体は結果上限に到達できるファイルでも Markdown 末尾まで進む余地がある。

このため、many-match の cold run では次の寄与が混ざる。

- cold cache と dev build の遅さ。
- pulldown-cmark parser の初回処理。
- `SearchBlockEntry` と sentence range の一括構築。
- 巨大 block の direct search と context snippet 生成。
- allocator が保持する RSS plateau。

このフェーズでは、測定で寄与を分けつつ、構造上不要な後続 parse/allocation を止める。

## Architecture

検索の中心を「全 block 抽出 -> 全 block 照合」から「block 逐次確定 -> その場で照合 -> result budget 到達で停止」へ変更する。

新しい内部単位として、`visit_search_blocks_until_cancelled()` または `SearchBlockStream` 相当の helper を追加する。この helper は `Parser::new_ext(markdown, markdown_options(MarkdownProfile::Search))` を使い、現行の `extract_search_blocks_until_cancelled()` と同じ block 判定を維持する。

維持する抽出契約:

- 見出し、段落、引用、表、脚注本文を検索対象にする。
- link label や HTML tag body など、既存で除外している入力を検索対象にしない。
- inline code は含め、code block は既存契約どおり扱う。
- nested list、inline HTML depth、malformed HTML 近傍の既存テスト期待を維持する。

新 helper は全 block の `Vec<SearchBlockEntry>` を返さず、block 確定ごとに callback へ渡す。callback は通常 block search または large-block direct search を行い、次の状態を返す。

- Continue: 次 block へ進む。
- StopResultLimit: result budget 到達として停止する。
- StopCancelled: stale cancellation として停止する。

`search_directory_with_limits_blocking()` は、各ファイルを読むたびに `remaining_results` を計算して逐次検索へ渡す。ファイル内で `limits.max_results` に到達した場合、そのファイルの残り Markdown parse を止め、ディレクトリ検索全体を `result_limit` で終了する。

## Data Flow

1. `search_directory_with_limits_blocking()` が catalog を列挙し、file/result/byte limit と stale を既存どおり確認する。
2. 各ファイルを `read_search_markdown_with_byte_budget()` で読む。
3. 検索対象として受け入れた本文長だけ `searched_bytes` に加算する。
4. `search_file_streaming_blocks(relative, markdown, query, remaining_results, is_cancelled)` 相当の関数を呼ぶ。
5. parser event stream から block を逐次確定し、block ごとに照合する。
6. result-limit、stale、parser 終了、またはファイル終了で停止する。
7. 呼び出し側が既存と同じ `SearchResponse::from_parts()` で応答を組み立てる。

`find_matches_for_large_block()` は bounded snippet、case-fold cancellation、no-match/late-match 再走査抑制を維持する。result budget 到達時は tail 探索を止める。

## Context Handling

`SearchResultItem` の `before/current/after` 契約は原則維持する。逐次化によって後続 block の unlimited lookahead は避けるため、context は次の方針にする。

- 同一 block 内の sentence context を優先する。
- 直前 block context は小さな ring buffer で保持する。
- 次 block context が必要な場合は最大1 block だけ先読みし、先読みが result-limit 停止と衝突する場合は同一 block snippet を優先する。

既存 unit test と integration test の期待値を先に確認し、外部 JSON と UI 表示が不必要に変わらないようにする。もし完全互換が過剰に複雑になる場合は、隣接 block context の範囲を明示的に狭める設計判断を別途記録してから変更する。

## Error Handling

読込失敗は現行どおり warn log 後に対象ファイルをスキップする。UTF-8 不正、byte-limit、base directory 差し替え検出、blocking task panic/join error の分類も変えない。

逐次 parser 経路で内部不整合が起きても、外部応答やログへ実パス、full process args、検索本文断片を出さない。stale cancellation は、catalog 列挙、ファイル処理前、読込後、block 抽出中、large-block 探索中、通常 block 照合中で確認する。

## Security

検索 query と Markdown 本文は untrusted input として扱う。逐次化により早期停止は強化するが、セキュリティ境界は変更しない。

- path validation と nofollow open を維持する。
- workspace exclusion を維持する。
- Host/Origin 検証、security headers、CSP を維持する。
- HTML sanitize と DOM API による検索結果描画の前提を維持する。
- ログと計測記録には実パス、full process args、本文断片を残さない。

この変更は主に可用性とメモリ使用量の改善であり、XSS や path traversal の防御を弱めてはならない。

## Testing

TDD で進め、先に現行の弱点を表す failing test を追加する。

構造テスト:

- result budget 到達後、同一ファイルの後続 block 抽出が進まない。
- 巨大 many-match block で 100 件到達後、large-block direct search が tail 全体を走査しない。
- stale cancellation が block 抽出中、large-block 探索中、通常 block 照合中で止まる。
- context 生成回数が result budget を超えて増えない。

契約維持テスト:

- HTML 除外、脚注、見出し、表、inline code、code block、nested list の block 抽出契約。
- Unicode case-fold offset。
- bounded context snippet。
- `/api/search` の HTTP JSON 契約。
- `truncated_reasons=["result_limit"]`、`searched_files`、`searched_bytes` の既存意味。
- 不正 Host 拒否と security headers。

完了前に `./verify.sh` を実行する。検索の targeted tests は実装中に必要な粒度で追加実行する。

## Measurement

測定 fixture は `/tmp/markdown-view-search-many-match-streaming.XXXXXX` のような一時ディレクトリに生成し、repo へ追加しない。

測定 matrix:

- dev build cold run。
- dev build warm run。
- release build cold run。
- release build warm run。
- 単一 10MiB 近傍 many-match file。
- 複数ファイル分散 many-match。

記録する値:

- elapsed。
- request 中 peak RSS。
- request 後 after RSS。
- 5秒待機後 settled RSS。
- HTTP status。
- `searched_files`。
- `searched_bytes`。
- `truncated`。
- `truncated_reasons`。
- `results.len()`。
- response size。

完了判断では、現行再計測値から release/dev ともに result-limit 経路の cold elapsed と RSS が明確に下がったことを確認する。ただし CI には固定秒数や固定 RSS 上限を入れない。

## Impact Scope

主な変更対象:

- `src/server/files/search.rs`

確認対象:

- `tests/integration/search.rs`
- `tests/integration/security.rs`
- `src/template/assets/ts/types.d.ts`
- `src/template/assets/generated-js/directory-search.js`

UI/TS の契約変更は非ゴールなので、TypeScript と generated JS は原則確認対象に留める。

## Rollback

逐次 block visitor とそれを使う検索経路を戻し、従来の `extract_search_blocks_until_cancelled()` -> `find_matches_for_file()` へ戻す。外部 API 契約を変えないため、ロールバック時に UI や client 側を巻き戻す必要はない想定。

実装中に context 互換や parser state の複雑性が過剰になった場合は、実装を進める前にこの設計へ戻り、隣接 block context の契約または処理単位を再承認する。

## Acceptance Criteria

- 検索結果上限到達後に後続 block 抽出と巨大 block tail 探索が止まる。
- `/api/search` の外部 JSON 契約が変わらない。
- Host/Origin/path validation、HTML sanitize、CSP、ファイルサイズ上限が変わらない。
- dev/release、cold/warm、単一/複数ファイルの測定結果が実パスや full process args なしで記録される。
- `./verify.sh` が pass する。
- `docs/todo/TODO.md` の該当項目に、実装結果、測定値、残リスクが反映される。
