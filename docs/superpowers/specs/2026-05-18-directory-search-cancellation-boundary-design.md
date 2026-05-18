# ディレクトリ検索キャンセル境界 設計書

## 背景

当初の `docs/todo/BACKLOG.md` の P2 には、ディレクトリ検索のキャンセル境界と allocation 削減の検討が残っていた。キャンセル境界の実装後、現在の BACKLOG 残件は allocation 削減の計測ベース検討に限定される。

現行のディレクトリ検索は、サーバ側で `spawn_blocking` に隔離され、結果数、検索対象ファイル数、総読込 byte 数、query 長の上限も明示されている。ブラウザ側は `documentFetchGeneration` により古い検索レスポンスを UI へ反映しない。一方で、古い検索リクエストの blocking task 自体は、上限到達または完走まで処理を続ける。

今回の目的は、UI 側の世代破棄とサーバ側処理の境界を揃え、同じブラウザタブ内でより新しい検索が始まった後の古い検索を協調的に早期終了できるようにすること。別タブ、別クライアント、直接 API 呼び出しの検索を相互にキャンセルしない。

## 目的

- ディレクトリモードの検索リクエストごとに、検証済み検索クライアント ID 単位でサーバ側の検索世代を発行する。
- 同じクライアント ID で新しい検索が始まったら、それ以前の同一クライアント検索が安全な区切りで早期終了できるようにする。
- クライアント ID 未指定または不正 ID の検索は no-cancellation fallback で完走させ、他クライアントをキャンセルしない。
- client ID map は上限 64 件に抑え、上限到達時は最も古く使われた entry を退避して新しい有効 ID を受け入れる。
- キャンセルをユーザー向けエラーにしない。
- `SearchResponse` の JSON 形状、検索 UI、既存の打ち切り契約を維持する。
- path validation、Host/Origin 検証、CSP、HTML sanitize 境界を変更しない。

## 非目標

- 検索インデックスの導入。
- HTTP 接続断や browser abort の低レベル検知。
- ファイル読込中、Markdown パース中、JSON serialize 中の強制中断。
- 検索結果コンテキストの `Cow<str>` 化や allocation 削減。
- 検索 UI の見た目や文言変更。
- `SearchResponse` への `cancelled` フィールド追加。

## アーキテクチャ

`AppState` にディレクトリ検索用の世代カウンタ map を追加する。型は `Mutex<HashMap<String, Arc<AtomicU64>>>` 相当とし、検証済み client ID ごとに別のカウンタを見る。

`AppState` は次の小さな API を持つ。

- `next_search_generation_for_client(client_id: Option<&str>)` は有効 ID の検索世代を 1 つ進め、発行された世代と handle を返す。
- `advance_existing_search_generation_for_client(client_id: Option<&str>)` は既存の有効 ID だけ検索世代を 1 つ進め、未登録 ID では entry を作らない。
- ID が無い、空、長すぎる、または許可文字外の場合は `None` を返す。
- 既存 ID は上限到達後も再利用できる。新規 ID が上限 64 件を超える場合は、最も古く使われた ID を退避して新規 ID を受け入れる。退避した ID の実行中検索は、別 client の結果完全性を保つためキャンセルしない。

有効な client ID は 1-64 bytes、ASCII 英数字、`-`、`_` のみとする。これはキャンセル境界の識別子であり、認証・認可・暗号用途には使わない。

`service::search()` は、ディレクトリモードだけ世代を発行する。長すぎる query でも、同じクライアントの既存検索を stale 化してから 400 JSON を返す。ただし未登録の client ID では新規 entry を作らない。単一ファイルモードは従来通り query 正規化後に `SearchResponse::empty(query)` を返し、検索世代を進めない。有効な検索クライアント ID がない場合は `SearchCancellation::never_cancelled()` を使う。

`search.rs` には `SearchCancellation` を追加する。`SearchCancellation` は、開始時の世代と現在世代を読み取る handle を保持し、`is_cancelled()` で「現在世代が開始世代より新しいか」を判定する。

`search_directory()` は `SearchCancellation` を受け取り、`spawn_blocking` 内の同期検索コアへ渡す。同期検索コアは次の境界でキャンセルを確認する。

1. ファイル列挙の再帰境界と entry loop。
2. ファイル列挙後、結果処理へ進む前。
3. 各ファイルの解決・読込・検索処理へ入る前。
4. 1 ファイル内の一致走査中、結果上限またはキャンセルに到達した時点。
5. 各ファイルの検索結果を反映した後。
6. 結果数、ファイル数、byte 数の既存上限判定後。

キャンセル時は `std::io::Error` を返さず、その時点までに作った `SearchResponse` を返す。古いレスポンスはブラウザ側の既存 generation check で破棄されるため、JSON へキャンセル情報を追加しない。

## データフロー

`/api/search?q=...` は `routes.rs` で raw query byte 長と percent encoding を検証する。この入口契約は変更しない。`X-Markdown-View-Search-Client` ヘッダーがあれば service へ渡し、検証は `AppState` 側で行う。

`service::search()` はディレクトリモードかどうかを先に判定する。単一ファイルモードでは `normalize_search_query()` で文字数上限と trim を確認して空結果を返す。

ディレクトリモードでは `normalize_search_query()` で文字数上限と trim を確認する。正規化に失敗した場合は `AppState::advance_existing_search_generation_for_client()` で既存 client のみ stale 化してから 400 JSON を返す。正常 query なら `AppState::next_search_generation_for_client()` で世代を進め、`SearchCancellation` を作って `search_directory(base_dir, &query, cancellation).await` を呼ぶ。世代 handle が返らない場合は no-cancellation handle を渡す。

ブラウザ UI は `bootstrap.js` でページ単位の安定 ID を生成し、reload 時だけ同じ `sessionStorage` 値を再利用する。複製タブ相当の通常 navigation では新しい ID を発行し、`directory-search.js` の `/api/search` fetch で `X-Markdown-View-Search-Client` ヘッダーとして送る。レスポンス JSON shape は変更しない。

blocking task 内では、検索専用のキャンセル対応ファイル列挙、`resolve_file()`、`read_markdown_with_limit_blocking()`、`extract_search_blocks()`、`find_matches_for_file()` の流れを保つ。キャンセル確認は処理境界にだけ挿入し、読込直前検証や検索予算の意味を変えない。

## エラー処理

キャンセルはユーザー操作に伴う通常状態として扱い、400/500 の API エラーに変換しない。

キャンセル時のログは原則不要とする。診断上必要になった場合でも `debug!` に留め、query 全文、絶対パス、本文断片を出さない。

既存のエラー扱いは維持する。

- 長すぎる query は 400 JSON。
- `spawn_blocking` の panic は `error!`。
- panic 以外の join error は `warn!`。
- ディレクトリ検索失敗は 500 JSON。
- ファイル単位の解決失敗や読込失敗は warn log に残してスキップ。

## セキュリティ

キャンセル境界を追加しても、検索対象ファイルの安全確認は弱めない。各ファイル読込前に `resolve_file()` を通し、base 配下、hidden、Markdown 拡張子、通常ファイル、symlink 差し替えの検証を維持する。

Host middleware、WebSocket Origin 検証、CSP、HTML sanitize 境界、`innerHTML` sink 制限は変更しない。

キャンセル関連の状態はプロセス内の `AtomicU64` に閉じ、HTTP response へ公開しない。外部入力の client ID は長さと許可文字を検証し、不正値は no-cancellation fallback にする。この flow では client ID、query、body を新たにログへ出さない。既存のファイル単位 warn log は、制御文字 escape 済みの相対パスを出す場合がある。

## テスト方針

Rust 側の単体テストを中心にする。

- `SearchCancellation` が現在世代の進行を検知することを確認する。
- `AppState` が同一 client ID の世代だけを進め、別 client ID の世代を進めないことを確認する。
- client ID 未指定と不正 ID が cancellation handle を返さないことを確認する。
- 上限到達時に最も古い ID が退避され、新規の有効 ID が cancellation handle を受け取れることを確認する。
- 上限到達時に退避された ID の実行中検索 handle が進まないことを確認する。
- キャンセル済み `SearchCancellation` を同期検索コアへ渡した場合、ファイル処理へ進まず空結果を返すことを確認する。
- 1 ファイル処理後にキャンセルされた場合、それ以上のファイルへ進まないことを小さい limit またはテスト用 hook で固定する。
- ファイル列挙中のキャンセルと、1 ファイル内で結果上限に達した後に追加一致を作らないことを確認する。
- `service::search()` でディレクトリ検索時に有効 client ID の世代が進み、client ID 未指定と単一ファイルモードでは世代が進まないことを確認する。
- ディレクトリモードでは長すぎる query でも同一 client ID の既存検索が stale 化され、未登録 client ID の entry は作らないことを確認する。
- 既存の通常検索、結果数上限、ファイル数上限、総読込 byte 上限、query 上限、単一ファイル空結果のテストを維持する。

UI 表示契約は変えない。`tests/e2e/document_search.spec.ts` では `/api/search` にクライアント ID ヘッダーが送られ、reload 時は同じ ID を再利用し、複製タブ相当では新しい ID を発行することを確認する。

## 受け入れ基準

- 同一クライアントで新しいディレクトリ検索が開始されると、それ以前の同一クライアント検索は協調的キャンセル判定で早期終了できる。
- 別クライアント、client ID 未指定、不正 ID の検索は互いにキャンセルしない。
- client ID map が上限に達しても、新規の有効 ID は最古 entry の退避によりキャンセル境界を利用できる。
- キャンセルはユーザー向けエラーにならない。
- `SearchResponse` の JSON 形状は変わらない。
- 単一ファイルモードの `/api/search` は空結果を返し、検索世代を進めない。
- 既存の検索上限と `truncated` / `truncated_reasons` / `limits` / `searched_bytes` 契約は変わらない。
- path validation、Host/Origin 検証、CSP、HTML sanitize 境界を弱めない。
- `cargo test --all-targets --all-features` が通る。
- 可能なら `./verify.sh` が通る。

## 影響範囲

- `src/server/state.rs`
  - client-scoped 検索世代 map と accessor を追加する。
- `src/server/service.rs`
  - ディレクトリ検索時に client ID 単位で世代を発行し、検索層へキャンセル handle を渡す。
- `src/server/routes.rs`
  - `/api/search` で検索クライアント ID ヘッダーを受け取り service へ渡す。
- `src/server/files/search.rs`
  - `SearchCancellation` と協調的キャンセル判定を追加する。
- `src/template/assets/js/bootstrap.js`
  - タブ内検索クライアント ID を生成して保持する。
- `src/template/assets/js/directory-search.js`
  - `/api/search` fetch に検索クライアント ID ヘッダーを付ける。
- `src/server/files/tests` または `src/server/files/search.rs` の test module
  - キャンセル境界の単体テストを追加する。
- `docs/todo/BACKLOG.md`
  - 実装完了後、対象項目のうちキャンセル境界が完了したことと、allocation 削減を残すかどうかを整理する。

## ロールバック

`AppState` の検索世代 map、`SearchCancellation`、`search_directory()` の追加引数、routes/service の client ID 受け渡し、UI ヘッダー付与、キャンセル判定、関連テストを revert すれば元に戻せる。

`SearchResponse` の JSON 形状や UI を変えないため、ロールバック時にブラウザ側の互換対応は不要。

## 残余リスク

- 協調的キャンセルなので、ファイル読込中や Markdown パース中の処理は即時停止しない。
- 極端に大きい単一ファイルの処理時間は既存のファイルサイズ上限で抑えるが、キャンセル応答性は処理境界単位に留まる。
- blocking thread pool の占有を完全には解消しない。検索インデックス、専用 worker、allocation 削減は後続候補として残る。
- 古い検索の部分結果はサーバから返り得るが、現行 UI の generation check により同一タブの画面へ反映されない前提を維持する。
- client ID map は上限 64 件で増加を止める。上限到達時は最も古く使われた entry を退避するが、その entry の世代は進めない。退避済み ID の実行中検索は完走し得る。同じ client ID が後で戻った場合は新しい entry として扱われる。

## 見積もり

- 人間作業: 1.5-2.5 時間。
- Codex/AI 支援: 30-60 分。

## 検証

設計書自体は docs-only なので、TDD ではなく文書検証で確認する。

```bash
rg -n "キャンセル|SearchCancellation|SearchResponse|Host|CSP|ロールバック|見積もり" docs/superpowers/specs/2026-05-18-directory-search-cancellation-boundary-design.md
rg -n "T[B]D|TO[D]O|未[定]" docs/superpowers/specs/2026-05-18-directory-search-cancellation-boundary-design.md
```

実装フェーズでは、少なくとも次を実行する。

```bash
cargo test --all-targets --all-features
./verify.sh
```
