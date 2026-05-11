# /api/search query 長 guard 設計

**作成日**: 2026-05-11
**対象 issue**: GitHub Issue #142「/api/search の query 長 guard を route/service 境界に追加する」

## 目的

`/api/search` の `q` を未信頼入力として扱い、検索処理、JSON応答、ログ周辺へ流れる前に最大長を制限する。

現行実装では `q` を `Option<String>` として受け取り、route/service 境界で長さを制限しないまま検索実装へ渡している。ファイル数、結果数、総読込バイト数、単体ファイルサイズの上限はあるが、入力文字列そのものの境界が薄い。

今回の変更では trim 後の検索クエリを最大 256 文字に制限する。超過時は `/api/search` が `400 Bad Request` の JSON エラーを返し、ディレクトリ列挙や Markdown 読み込みへ進まない。検索実装側にも同じ guard を置き、route を迂回する内部呼び出しに対しても defense-in-depth を維持する。

## 非目的

- 検索結果 JSON の成功時 shape は変更しない。
- 空検索、空白だけの検索、単一ファイルモードの通常検索互換性は変更しない。
- `MAX_SEARCH_RESULTS`、`MAX_SEARCH_FILES`、`MAX_SEARCH_BYTES`、`MAX_FILE_SIZE` は変更しない。
- 検索アルゴリズム、Markdown からの検索ブロック抽出、Unicode 小文字化の仕様は変更しない。
- フロントエンドの検索UI、debounce、表示文言は変更しない。
- Host/Origin validation、CSP、HTML sanitization、path validation は変更しない。

## 採用方針

route handler は薄く保ち、`src/server/service.rs` と `src/server/files/search.rs` の境界で検証を行う。

`src/server/files/search.rs` に検索クエリ上限を表す定数と、trim 済み検索クエリを返す検証関数を置く。最大長は `chars().count()` で判定し、UTF-8 のバイト長ではなくユーザーが入力した文字数として扱う。trim 後が空なら従来通り空結果を返す。trim 後が 256 文字を超える場合は `std::io::ErrorKind::InvalidInput` のエラーを返す。

`src/server/service.rs` は検索実装から返った `InvalidInput` を `400 Bad Request` の JSON エラーへ変換する。それ以外の検索 I/O エラーや blocking task join 失敗は、従来通り `500 Internal Server Error` として扱う。

## 代替案

### 案A: service/search 境界で検証し、search 側にも guard を置く

HTTP API の入力境界を service で持ち、検索実装にも同じ上限を置く。route は query 抽出だけに集中でき、内部呼び出しでも長大入力を拒否できる。issue の defense-in-depth 方針に合うため採用する。

### 案B: route handler だけで検証する

変更箇所は少ないが、`search_directory` を直接呼ぶ単体テストや将来の内部呼び出しでは無制限入力が残る。検索実装を再利用した場合に同じ防御を期待できないため採用しない。

### 案C: search 実装だけで拒否する

検索処理の入口では防げるが、HTTP API として 400 に変換する責務が曖昧になりやすい。service 層のエラー分類を明示する方が API 契約として読みやすいため採用しない。

## アーキテクチャ

### query validation

`src/server/files/search.rs` に次の契約を持つ検証境界を追加する。

1. raw query を trim する。
2. trim 後が空なら空 query として成功させる。
3. trim 後の `chars().count()` が 256 を超えたら `InvalidInput` を返す。
4. 成功時は trim 済み `String` だけを後続検索へ渡す。

`search_directory` は raw query を受け取った直後に検証し、検証済み query を blocking task へ move する。`search_directory_with_limits_blocking` も同じ検証関数を使う。これにより async entry と blocking helper のどちらから入っても同じ上限になる。

### service error mapping

`service::search` は単一ファイルモードかディレクトリモードかを分岐する前に query 検証を通す。単一ファイルモードでも長大 query を空結果として返さず、`400 Bad Request` にする。

ディレクトリモードでは `search_directory` の戻り値を見て、`InvalidInput` なら固定文言の JSON エラーへ変換する。その他の `std::io::ErrorKind` は既存どおり検索失敗として warn log と `500 Internal Server Error` にする。

## データフロー

### 正常 query

1. `GET /api/search?q=alpha%20note` が `routes::api_search_handler` に届く。
2. handler は `query.q.unwrap_or_default()` を `service::search` へ渡す。
3. service は query を検証し、trim 済み query を得る。
4. ディレクトリモードでは `search_directory` へ渡し、検索結果を JSON で返す。
5. 単一ファイルモードでは従来通り空の `SearchResponse` を返す。

### 空 query

1. `q` 未指定、空文字、または空白だけの query が届く。
2. trim 後 query は空文字になる。
3. 従来通り `SearchResponse::empty("")` を `200 OK` で返す。

### 長大 query

1. trim 後 257 文字以上の query が届く。
2. service/search の検証で拒否する。
3. `/api/search` は `400 Bad Request` の JSON エラーを返す。
4. ディレクトリ列挙、Markdown ファイル読み込み、検索ブロック抽出、結果 JSON 生成には進まない。

## エラー処理

- query 長超過: `400 Bad Request`、JSON エラー文言は固定の「検索クエリが長すぎます」とする。
- query 値そのもの、入力長、ファイルパスはレスポンスにもログにも含めない。
- 検索対象ファイルの解決失敗、読込失敗、UTF-8 失敗は既存どおり対象ファイルをスキップして検索継続する。
- ディレクトリ列挙など検索全体の I/O 失敗は既存どおり `500 Internal Server Error` にする。
- blocking task panic や join 失敗は既存どおり `500 Internal Server Error` にする。

## テスト方針

TDD で進める。

`tests/integration_test.rs`:

- ディレクトリモードで trim 後 257 文字の `/api/search?q=...` が `400 Bad Request` と JSON エラーを返す。
- 単一ファイルモードで trim 後 257 文字の `/api/search?q=...` が空結果ではなく `400 Bad Request` を返す。
- 既存の通常検索、結果数打ち切り、巨大ファイルスキップ、不正 Host 拒否は維持する。

`src/server/files/search.rs`:

- `search_directory_with_limits_blocking` に 257 文字 query を渡すと `InvalidInput` で失敗する。
- 256 文字 query は検証を通り、検索処理へ進む。
- trim 後空 query は `SearchResponse::empty("")` を返す。
- Unicode を含む query でも文字数ベースで判定される。

完了前の検証は `cargo test --all-targets --all-features` と `./verify.sh` を実行する。

## 受け入れ条件

- `/api/search` は trim 後 256 文字以内の query を従来通り処理する。
- `/api/search` は trim 後 257 文字以上の query を `400 Bad Request` で拒否する。
- 長大 query の拒否時にディレクトリ列挙や Markdown 読み込みへ進まない。
- 単一ファイルモードでも長大 query は空結果にならず `400 Bad Request` になる。
- 検索実装を直接呼ぶ内部経路でも同じ上限を守る。
- エラー応答とログに query 本文を含めない。
- 成功時の `SearchResponse` JSON shape は変わらない。
- `./verify.sh` が通る。

## セキュリティ考慮

`q` は外部から渡される未信頼入力である。長大 query を検索処理、JSON、ログへ流す前に制限し、CPU、メモリ、ログ肥大化のリスクを下げる。

拒否時のエラー文言は固定にし、入力値をレスポンスやログへ出さない。これにより、ログ注入、機微文字列の保存、巨大ログ生成を避ける。

今回の変更は query 長の入力境界に限定する。既存の `127.0.0.1` binding、Host validation、Origin validation、HTML sanitization、CSP、path validation、file size limit、directory traversal protection は変更しない。検索結果は既存どおり JSON として返し、query を HTML として解釈する新規経路は作らない。

## 影響範囲

- `src/server/files/search.rs`: query 長上限定数、検証関数、search entry の guard、単体テスト。
- `src/server/service.rs`: `InvalidInput` から `400 Bad Request` への変換、単一ファイルモード前の入力検証。
- `src/server/routes.rs`: handler shape を維持し、query 抽出と service 呼び出しだけを続ける。
- `tests/integration_test.rs`: `/api/search` の長大 query 拒否テストを追加。
- `docs/todo/TODO.md`: 実装完了時に issue 142 の追跡状態を更新対象にする。

## ロールバック

実装コミットを revert すれば、`/api/search` の query 長制限は旧挙動に戻る。

問題が service の HTTP 400 変換に限定される場合は、`InvalidInput` mapping を削除して従来の検索エラー変換へ戻せる。問題が search 実装側の guard に限定される場合は、検証関数の呼び出しを外せば、既存の検索上限だけを使う状態へ戻せる。

## 工数見積もり

人間の作業見積もり: 45-90 分。service/search 境界の小変更、統合テストと単体テスト追加、`./verify.sh` を含む。

Codex / AI 支援込み見積もり: 20-45 分。変更範囲は狭いが、単一ファイルモードと内部検索経路の両方で同じ拒否仕様を固定する必要がある。
