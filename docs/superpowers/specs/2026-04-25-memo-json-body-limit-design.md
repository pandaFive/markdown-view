# Memo JSON Body Limit Design

**作成日**: 2026-04-25
**対象 TODO**: メモ API のボディ制限値を意図明文化し、境界テストを追加
**対象ファイル**: `src/server/routes.rs`, `tests/integration_test.rs`

## Goal

`/api/memo` の PUT request body 制限が、保存可能な raw memo サイズ制限とは別レイヤーであることを明文化し、境界テストで固定する。

現行仕様は維持する。raw memo は `MAX_FILE_SIZE` 由来の 10MB まで保存可能で、JSON request body は文字列 escape により raw より大きくなり得るため、`(MAX_FILE_SIZE * 2) + 4096` まで受け付ける。

## Non-Goals

- `MAX_FILE_SIZE` の変更
- `MEMO_JSON_BODY_LIMIT` の引き締め
- メモ保存形式や `/api/memo` JSON schema の変更
- `/api/content` や WebSocket の制限変更
- JSON parser や axum body limit layer の差し替え

## Design

`src/server/routes.rs` の `MEMO_JSON_BODY_LIMIT` は現行値を維持する。

定数直前に doc コメントを追加し、次の意図を明記する。

- 保存される raw memo の上限は `MAX_FILE_SIZE` で判定する
- HTTP request body の上限は JSON envelope と string escape を含む transport-level 制限である
- raw に backslash や quote が多い場合、JSON body は raw の約 2 倍まで膨らむ
- `+ 4096` は `{"raw": ...}` や将来の小さな JSON envelope 変更を吸収するための余白である
- body limit を raw limit 近くへ縮めると、10MB 未満の合法 memo が JSON escape により拒否される可能性がある

runtime behavior は変更しない。

## Test Plan

既存の `tests/integration_test.rs` の memo API test group に境界テストを追加または補強する。

既存テスト:

- `test_apiメモ_jsonエスケープで膨らんでも上限内rawなら保存できる`
- `test_apiメモ_10mb超過は413で拒否する`

追加・補強する観点:

- `MAX_FILE_SIZE` 以下の raw memo は、JSON escape により request body が raw より大きくなっても保存できる
- `MAX_FILE_SIZE + 1` の raw memo は、body limit ではなく保存層の validation で `413 Payload Too Large` になる
- `MEMO_JSON_BODY_LIMIT` を超える JSON body は axum `DefaultBodyLimit` により `413 Payload Too Large` になる

`MEMO_JSON_BODY_LIMIT` は router 内部の実装詳細として公開しない。body limit 超過テストは、現行式と同じ考え方で十分に大きい JSON body を作ることで HTTP 境界を検証する。

## Acceptance Criteria

- `MEMO_JSON_BODY_LIMIT` の意図が `routes.rs` のコメントから読める
- raw 10MB 以下かつ JSON escape で膨らむ memo が保存できる
- raw 10MB + 1 byte の memo が `413 Payload Too Large` と既存エラーメッセージで拒否される
- JSON request body が transport-level limit を超えた場合に `413 Payload Too Large` で拒否される
- `./verify.sh` が pass する

## Security Considerations

この変更は body limit を緩和しない。HTTP request body の上限を明文化することで、過大 payload によるメモリ消費を無制限にしない意図を保守者が読み取れるようにする。

raw memo の保存上限は従来どおり `save_route_memo` 側で `MAX_FILE_SIZE` により検証する。transport-level limit は JSON parse 前の防御線であり、application-level limit の代替ではない。

外部入力である JSON body は信頼しない。body limit、serde deserialize、raw size validation の順に複数の境界で拒否されることを前提にする。

## Impact

- `src/server/routes.rs`: `MEMO_JSON_BODY_LIMIT` の doc コメント追加のみ。runtime behavior は変更しない。
- `tests/integration_test.rs`: `/api/memo` PUT の境界テストを追加または補強する。

依存ファイルへの影響:

- `src/server/files/memo.rs`: raw memo size validation の既存挙動を前提として参照するが、変更しない。
- `src/server/files/content.rs`: `MAX_FILE_SIZE` 定義元として参照されるが、変更しない。

## Rollback

doc コメントと追加テストを revert すれば戻せる。制限値は変更しないため、runtime behavior の rollback は不要。
