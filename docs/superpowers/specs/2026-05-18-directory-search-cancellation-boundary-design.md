# ディレクトリ検索キャンセル境界 設計書

## 背景

`docs/todo/BACKLOG.md` の P2 には、ディレクトリ検索のキャンセル境界と allocation 削減の検討が残っている。

現行のディレクトリ検索は、サーバ側で `spawn_blocking` に隔離され、結果数、検索対象ファイル数、総読込 byte 数、query 長の上限も明示されている。ブラウザ側は `documentFetchGeneration` により古い検索レスポンスを UI へ反映しない。一方で、古い検索リクエストの blocking task 自体は、上限到達または完走まで処理を続ける。

今回の目的は、UI 側の世代破棄とサーバ側処理の境界を揃え、より新しい検索が始まった後の古い検索を協調的に早期終了できるようにすること。

## 目的

- ディレクトリモードの検索リクエストごとにサーバ側の検索世代を発行する。
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
- E2E テストの新規追加。

## アーキテクチャ

`AppState` にディレクトリ検索用の世代カウンタを追加する。型は `Arc<AtomicU64>` 相当とし、`AppState` clone や `Arc<AppState>` 共有下でも同じカウンタを見る。

`AppState` は次の小さな API を持つ。

- `next_search_generation()` は検索世代を 1 つ進め、発行された世代を返す。
- `search_generation()` は現在世代を読み取るための共有参照または軽量 handle を返す。

`service::search()` は、query 正規化後にディレクトリモードだけ世代を発行する。単一ファイルモードは従来通り `SearchResponse::empty(query)` を返し、検索世代を進めない。

`search.rs` には `SearchCancellation` を追加する。`SearchCancellation` は、開始時の世代と現在世代を読み取る handle を保持し、`is_cancelled()` で「現在世代が開始世代より新しいか」を判定する。

`search_directory()` は `SearchCancellation` を受け取り、`spawn_blocking` 内の同期検索コアへ渡す。同期検索コアは次の境界でキャンセルを確認する。

1. ファイル列挙後、結果処理へ進む前。
2. 各ファイルの解決・読込・検索処理へ入る前。
3. 各ファイルの検索結果を反映した後。
4. 結果数、ファイル数、byte 数の既存上限判定後。

キャンセル時は `std::io::Error` を返さず、その時点までに作った `SearchResponse` を返す。古いレスポンスはブラウザ側の既存 generation check で破棄されるため、JSON へキャンセル情報を追加しない。

## データフロー

`/api/search?q=...` は `routes.rs` で raw query byte 長と percent encoding を検証する。この入口契約は変更しない。

`service::search()` は `normalize_search_query()` で文字数上限と trim を確認する。ディレクトリモードでなければ空結果を返す。

ディレクトリモードでは `AppState::next_search_generation()` で世代を進め、`SearchCancellation` を作って `search_directory(base_dir, &query, cancellation).await` を呼ぶ。

blocking task 内では、既存の `list_markdown_files_from_canonical_base()`、`resolve_file()`、`read_markdown_with_limit_blocking()`、`extract_search_blocks()`、`find_matches_for_file()` の流れを保つ。キャンセル確認は処理境界にだけ挿入し、読込直前検証や検索予算の意味を変えない。

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

キャンセル関連の状態はプロセス内の `AtomicU64` に閉じ、HTTP response や DOM へ公開しない。外部入力で検索世代を指定できる API は作らない。

## テスト方針

Rust 側の単体テストを中心にする。

- `SearchCancellation` が現在世代の進行を検知することを確認する。
- キャンセル済み `SearchCancellation` を同期検索コアへ渡した場合、ファイル処理へ進まず空結果を返すことを確認する。
- 1 ファイル処理後にキャンセルされた場合、それ以上のファイルへ進まないことを小さい limit またはテスト用 hook で固定する。
- `service::search()` でディレクトリ検索時に世代が進み、単一ファイルモードでは世代が進まないことを確認する。
- 既存の通常検索、結果数上限、ファイル数上限、総読込 byte 上限、query 上限、単一ファイル空結果のテストを維持する。

UI 表示契約を変えないため、新規 E2E は追加しない。既存 `tests/e2e/document_search.spec.ts` は検索 UI の回帰確認として維持する。

## 受け入れ基準

- 新しいディレクトリ検索が開始されると、それ以前の検索は協調的キャンセル判定で早期終了できる。
- キャンセルはユーザー向けエラーにならない。
- `SearchResponse` の JSON 形状は変わらない。
- 単一ファイルモードの `/api/search` は空結果を返し、検索世代を進めない。
- 既存の検索上限と `truncated` / `truncated_reasons` / `limits` / `searched_bytes` 契約は変わらない。
- path validation、Host/Origin 検証、CSP、HTML sanitize 境界を弱めない。
- `cargo test --all-targets --all-features` が通る。
- 可能なら `./verify.sh` が通る。

## 影響範囲

- `src/server/state.rs`
  - 検索世代カウンタと accessor を追加する。
- `src/server/service.rs`
  - ディレクトリ検索時に世代を発行し、検索層へキャンセル handle を渡す。
- `src/server/files/search.rs`
  - `SearchCancellation` と協調的キャンセル判定を追加する。
- `src/server/files/tests` または `src/server/files/search.rs` の test module
  - キャンセル境界の単体テストを追加する。
- `docs/todo/BACKLOG.md`
  - 実装完了後、対象項目のうちキャンセル境界が完了したことと、allocation 削減を残すかどうかを整理する。

## ロールバック

`AppState` の検索世代カウンタ、`SearchCancellation`、`search_directory()` の追加引数、キャンセル判定、関連テストを revert すれば元に戻せる。

`SearchResponse` の JSON 形状や UI を変えないため、ロールバック時にブラウザ側の互換対応は不要。

## 残余リスク

- 協調的キャンセルなので、ファイル読込中や Markdown パース中の処理は即時停止しない。
- 極端に大きい単一ファイルの処理時間は既存のファイルサイズ上限で抑えるが、キャンセル応答性は処理境界単位に留まる。
- blocking thread pool の占有を完全には解消しない。検索インデックス、専用 worker、allocation 削減は後続候補として残る。
- 古い検索の部分結果はサーバから返り得るが、現行 UI の generation check により画面へ反映されない前提を維持する。

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
