# ディレクトリ検索キャンセル境界 設計書

## 背景

実装前の `docs/todo/BACKLOG.md` の P2 には、ディレクトリ検索のキャンセル境界と allocation 削減の検討が残っていた。

実装前のディレクトリ検索は、サーバ側で `spawn_blocking` に隔離され、結果数、検索対象ファイル数、総読込 byte 数、query 長の上限も明示されていた。ブラウザ側は `documentFetchGeneration` により古い検索レスポンスを UI へ反映しなかった。一方で、古い検索リクエストの blocking task 自体は、上限到達または完走まで処理を続けていた。

今回の目的は、UI 側の世代破棄とサーバ側処理の境界を揃え、より新しい検索が始まった後の古い検索を協調的に早期終了できるようにすること。

## 目的

- ディレクトリモードの検索リクエストごとに、ブラウザクライアント単位のサーバ側検索世代を発行する。
- 新しい検索が始まったら、それ以前の検索が安全な区切りで早期終了できるようにする。
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
- 検索 UI の表示契約変更。

## アーキテクチャ

ブラウザは `/api/search` に不透明な検索クライアント ID とページ内単調増加 sequence をヘッダーで送る。ID はページごとに生成し、sequence は通常検索とクリア時キャンセル通知の発行順を表す。サーバは ID を ASCII 英数字、`-`、`_`、最大 64 byte に制限し、不正値は未指定として扱う。sequence は正の JS safe integer 範囲だけを受け付け、ID が有効な場合だけ参照する。

`AppState` にディレクトリ検索用の世代カウンタ map と全体同時実行数を制限する semaphore を追加する。世代 map はクライアント ID から `Arc<AtomicU64>` 相当と最終利用順序への map とし、`AppState` clone や `Arc<AppState>` 共有下でも同じカウンタを見る。map は上限を持ち、不正な ID 乱発でプロセス内状態が伸び続けないようにする。上限到達時の新規 client ID は idle entry があれば bounded LRU で古い idle entry を入れ替え、全 entry が active の場合は新規 client の世代発行を拒否する。semaphore は検索クライアント ID の有無に関係なく、ディレクトリ検索全体の同時実行数を 4 件に制限する。

`AppState` は次の小さな API を持つ。

- `begin_search_generation(client_id, sequence)` は指定クライアントの検索世代を 1 つ進め、発行された世代 handle を返す。sequence 付き client では最新 sequence 以下、または sequence なしの後続 request を stale handle として返し、検索世代を進めない。上限到達時は最終利用が最も古い idle entry を優先して入れ替え、全 entry が active の場合だけ warn log を残して新規 client の世代発行を拒否する。
- `try_acquire_directory_search_permit()` はディレクトリ検索の全体同時実行数が上限未満の場合だけ permit を返す。permit は `search_directory()` へ渡し、`spawn_blocking` closure 内で実検索処理の完了まで保持する。
- `current_search_generation(client_id)` はテストで現在世代を読み取る。

`routes.rs` は検証済みの検索クライアント ID と任意の sequence を `service::search()` へ渡す。`service::search()` は、query 正規化後にディレクトリモードかつ有効なクライアント ID がある場合、全体検索 permit より先に世代を発行する。stale request と空 query は permit を取得せず空結果を返す。単一ファイルモードは従来通り `SearchResponse::empty(query)` を返し、検索 permit も検索世代も進めない。非ブラウザクライアントなど ID がない呼び出しは、既存互換を優先してサーバ側キャンセルなしで検索するが、非空検索は全体同時実行上限の対象にはする。

`search.rs` には `SearchCancellation` を追加する。`SearchCancellation` は、開始時の世代と現在世代を読み取る handle を保持し、`is_cancelled()` で「現在世代が開始世代より新しいか」を判定する。

`search_directory()` は `SearchCancellation` を受け取り、`spawn_blocking` 内の同期検索コアへ渡す。同期検索コアは次の境界でキャンセルを確認する。

1. ファイル列挙中のディレクトリエントリ処理境界。
2. ファイル列挙後、結果処理へ進む前。
3. 各ファイルの解決・読込・検索処理へ入る前。
4. ファイル読込後、byte 集計・Markdown 解析・検索照合へ進む前。
5. 各ファイルの検索結果を反映した後。
6. 結果数、ファイル数、byte 数の既存上限判定後。

キャンセル時は `std::io::Error` を返さず、その時点までに作った `SearchResponse` を返す。古いレスポンスはブラウザ側の既存 generation check で破棄されるため、JSON へキャンセル情報を追加しない。

## データフロー

`/api/search?q=...` は `routes.rs` で raw query byte 長と percent encoding を検証する。この入口契約は変更しない。

`service::search()` は `normalize_search_query()` で文字数上限と trim を確認する。ディレクトリモードでなければ空結果を返す。

ディレクトリモードでは、有効な検索クライアント ID がある場合にまず `AppState::begin_search_generation(client_id, sequence)` でそのクライアントの世代を進める。stale handle が返った場合、または query が空の場合は、全体検索 permit を取得せず `SearchResponse::empty(query)` を返す。非空かつ stale ではない検索だけ `AppState::try_acquire_directory_search_permit()` で全体検索 permit を取得し、`search_directory(base_dir, &query, cancellation, permit).await` を呼ぶ。ID がない場合は `SearchCancellation::none()` を渡し、非空検索だけ全体検索 permit を取得する。

blocking task 内では、検索 permit を保持したうえで、検索専用の cancellable catalog 列挙、`resolve_file()`、`read_markdown_with_limit_blocking()`、`extract_search_blocks()`、`find_matches_for_file()` の流れを保つ。キャンセル確認は処理境界にだけ挿入し、読込直前検証や検索予算の意味を変えない。HTTP handler future が接続切断などで中断されても、既に開始した blocking 検索が完了するまで permit は解放されない。

## エラー処理

キャンセルはユーザー操作に伴う通常状態として扱い、400/500 の API エラーに変換しない。

キャンセル時のログは原則不要とする。診断上必要になった場合でも `debug!` に留め、query 全文、絶対パス、本文断片を出さない。

既存のエラー扱いは維持する。

- 長すぎる query は 400 JSON。
- ディレクトリ検索の全体同時実行数が上限到達した場合は 429 JSON（`検索が混み合っています`）。有効な client ID 付き検索では、429 を返す場合でも同一 client の旧検索は stale 化済みにする。
- 検索 client 世代 store が全 active で上限到達した場合は 429 JSON（`検索が混み合っています`）。
- `spawn_blocking` の panic は `error!`。
- panic 以外の join error は `warn!`。
- ディレクトリ検索失敗は 500 JSON。
- ファイル単位の解決失敗や読込失敗は warn log に残してスキップ。

## セキュリティ

キャンセル境界を追加しても、検索対象ファイルの安全確認は弱めない。各ファイル読込前に `resolve_file()` を通し、base 配下、hidden、Markdown 拡張子、通常ファイル、symlink 差し替えの検証を維持する。

Host middleware、WebSocket Origin 検証、CSP、HTML sanitize 境界、`innerHTML` sink 制限は変更しない。

キャンセル関連の内部世代値はプロセス内の `AtomicU64` に閉じ、HTTP response や DOM へ公開しない。外部入力で内部世代を指定できる API は作らない。HTTP ヘッダーで受け取るのは不透明なクライアント ID と発行順 sequence だけであり、ID は形式検証し、sequence は正の JS safe integer 範囲に制限して、不正値は無視する。

検索世代をクライアント単位に分けることで、別ブラウザや別タブの検索が互いを stale にしない。これにより、他クライアントの操作で部分結果が正常結果として返る可用性・結果完全性リスクを避ける。

クライアント ID 生成は `crypto.randomUUID()` を優先し、非対応時も `crypto.getRandomValues()` を使う。最後の fallback は認証境界ではないが、ヘッダー同値による意図しないキャンセル干渉を避けるため、可能な限り推測しにくい値を使う。

検索クライアント ID がない互換呼び出しも、全体同時実行上限の対象にする。これにより、ヘッダーを省略したローカルリクエストが `spawn_blocking` 検索を無制限に増やす経路を避ける。

## テスト方針

Rust 側の単体テストを中心にする。

- `SearchCancellation` が現在世代の進行を検知することを確認する。
- 検索世代がクライアント ID ごとに独立して進むことを確認する。
- 検索クライアント ID ヘッダーが valid な場合だけ service へ渡されることを確認する。
- キャンセル済み `SearchCancellation` を同期検索コアへ渡した場合、ファイル処理へ進まず空結果を返すことを確認する。
- 1 ファイル処理後にキャンセルされた場合、それ以上のファイルへ進まないことを小さい limit またはテスト用 hook で固定する。
- `service::search()` でディレクトリ検索時に世代が進み、単一ファイルモードでは世代が進まないことを確認する。
- `service::search()` で検索クライアント ID がない場合も、全体同時実行上限到達時に 429 を返すことを確認する。
- `service::search()` で全体同時実行上限到達時も、有効な client ID 付き検索は旧検索を stale 化するために検索世代を進めることを確認する。
- `service::search()` で空 query は全体同時実行上限到達時も permit なしで検索世代を進め、空結果を返すことを確認する。
- 実行中の `search()` が全体同時実行 permit を保持し、追加検索が 429 になることを確認する。
- 全体同時実行上限到達時の空 query が、実行中の同一 client 検索を stale 化することを確認する。
- `search()` future が中断されても、blocking 検索完了までは permit が保持されることを確認する。
- 既存の通常検索、結果数上限、ファイル数上限、総読込 byte 上限、query 上限、単一ファイル空結果のテストを維持する。

UI 表示契約を変えない。既存 `tests/e2e/document_search.spec.ts` の検索競合テストで、同一ページの連続検索が同じ検索クライアント ID を送ることを確認する。

## 受け入れ基準

- 新しいディレクトリ検索が開始されると、それ以前の検索は協調的キャンセル判定で早期終了できる。
- キャンセルはユーザー向けエラーにならない。
- `SearchResponse` の JSON 形状は変わらない。
- 単一ファイルモードの `/api/search` は空結果を返し、検索世代を進めない。
- 別クライアントのディレクトリ検索は互いの検索世代を stale にしない。
- 検索クライアント ID がないディレクトリ検索も全体同時実行上限を迂回しない。
- 既存の検索上限と `truncated` / `truncated_reasons` / `limits` / `searched_bytes` 契約は変わらない。
- path validation、Host/Origin 検証、CSP、HTML sanitize 境界を弱めない。
- `cargo test --all-targets --all-features` が通る。
- 可能なら `./verify.sh` が通る。

## 影響範囲

- `src/server/state.rs`
  - クライアント単位の検索世代カウンタ、全体同時実行数 semaphore、accessor を追加する。
- `src/server/routes.rs`
  - 検索クライアント ID ヘッダーを検証して service 層へ渡す。
- `src/server/service.rs`
  - ディレクトリ検索時に全体検索 permit と世代を発行し、検索層へキャンセル handle を渡す。
- `src/server/files/search.rs`
  - `SearchCancellation`、協調的キャンセル判定、blocking 検索中の permit 保持を追加する。
- `src/template/assets/js/bootstrap.js`, `src/template/assets/js/directory-search.js`
  - ページ単位の検索クライアント ID を生成し、検索 API へ送る。
- `tests/e2e/document_search.spec.ts`
  - 連続検索時の検索クライアント ID 送信を確認する。
- `src/server/files/tests` または `src/server/files/search.rs` の test module
  - キャンセル境界の単体テストを追加する。
- `docs/todo/BACKLOG.md`
  - 実装完了後、対象項目のうちキャンセル境界が完了したことと、allocation 削減を残すかどうかを整理する。

## ロールバック

`AppState` の検索世代 map と全体同時実行数 semaphore、検索クライアント ID ヘッダー処理、クライアント JS のヘッダー送信、`SearchCancellation`、`search_directory()` の追加引数、キャンセル判定、関連テストを revert すれば元に戻せる。

`SearchResponse` の JSON 形状や UI を変えないため、ロールバック時にブラウザ側の互換対応は不要。

## 残余リスク

- 協調的キャンセルなので、ファイル読込中や Markdown パース中の処理は即時停止しない。
- 極端に大きい単一ファイルの処理時間は既存のファイルサイズ上限で抑えるが、キャンセル応答性は処理境界単位に留まる。
- blocking thread pool の占有を完全には解消しない。検索インデックス、専用 worker、allocation 削減は後続候補として残る。
- 古い検索の部分結果は同一クライアント内ではサーバから返り得るが、現行 UI の generation check により画面へ反映されない前提を維持する。
- API クライアントが検索クライアント ID を送らない場合はサーバ側キャンセルなしで検索する。既存互換を優先するための挙動であり、ブラウザ UI の連続検索最適化とは分けて扱う。ただし全体同時実行上限は適用する。

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
