# RouteContext の HTTP adapter / application service 分割設計

- 作成日: 2026-04-30
- 対象 TODO: `docs/todo/TODO.md` Medium Priority「`RouteContext` を HTTP adapter と application service に分割する」
- 主対象: `src/server/routes.rs`
- 関連対象: `src/server/files/{resolve,content,memo,search,catalog}.rs`, `src/server/guards.rs`, `src/server/state.rs`

## 1. 目的

`src/server/routes.rs` に集中している HTTP 変換とアプリケーション手順を分離する。

現状の `RouteContext` は Host 検証、対象ファイル解決、本文ロード、メモロード/保存、メモ保存後 broadcast、sidebar 構築、memo broadcast 用 file label 生成をまとめて扱っている。HTTP handler も `RouteContext` を通じてアプリケーション手順を直接調停しており、新しい API を追加するときに責務の置き場所が曖昧になりやすい。

今回の設計では、HTTP handler を axum の入力抽出と HTTP 応答変換に寄せ、対象解決以降の手順を application service に移す。これにより、変更局所性を上げ、Host 検証やパス検証の守り忘れを起こしにくい構造へ近づける。

## 2. ゴール

1. `routes.rs` を HTTP adapter として薄くする。
2. 対象解決、本文ロード、メモロード/保存、sidebar 構築、memo broadcast を application service 側へ移す。
3. 既存の外部挙動を変えずに責務境界だけを整理する。
4. 将来の Host 検証 middleware 化と矛盾しない構造にする。
5. 未検証の query string を直接 I/O に渡さない service 契約を明示する。

## 3. 非ゴール

- この設計フェーズではコードを変更しない。
- Host 検証 middleware 化そのものは同時実装しない。
- 検索負荷制御、watcher 再検証、同期 I/O 解消など他 TODO は混ぜない。
- API レスポンス形式、HTML 出力、WebSocket 契約は変更しない。
- メモ保存、検索、ファイル一覧取得の内部アルゴリズムは変更しない。

## 4. 採用方針

小さな application service を追加し、`RouteContext` を段階的に縮小する。

service の置き場所は `src/server/service.rs` または用途別の `src/server/page.rs` / `src/server/memo_service.rs` を候補にする。第一候補は `src/server/service.rs` とする。`src/server/files/` はファイル I/O と対象解決の部品が中心であり、「ページ表示を組み立てる」「メモ保存後に broadcast する」といったユースケース手順は server 層に置く方が責務が明確になる。

HTTP handler は次の責務に限定する。

- axum extractor から headers / query / JSON body / websocket upgrade を受け取る。
- body limit や CSP など HTTP 層の設定を保持する。
- service 結果を `Json` / `Html` / `IntoResponse` に変換する。
- Host 検証は当面維持し、将来 middleware へ移せるよう対象解決とは切り離す。

application service は次の責務を持つ。

- request DTO を受け取り、`RouteTargetRequest` を組み立てる。
- `resolve_route_target` を通じて `ResolvedTarget` を取得する。
- `load_route_update` / `load_route_memo` / `save_route_memo` など既存の低レベル関数を呼ぶ。
- ページ表示に必要な `title`, `content`, `toc`, `memo`, `sidebar` を組み立てる。
- メモ保存成功後の broadcast を実行する。

## 5. 提案 API

最初の実装では、service に次の request / response を置く。

```rust
pub(super) struct PageRequest<'a> {
    pub file: Option<&'a str>,
}

pub(super) struct ContentRequest<'a> {
    pub file: Option<&'a str>,
}

pub(super) struct MemoRequest<'a> {
    pub file: Option<&'a str>,
}

pub(super) struct SaveMemoRequest<'a> {
    pub file: Option<&'a str>,
    pub raw: String,
}

pub(super) struct PageView {
    pub title: String,
    pub update: UpdateMessage,
    pub memo: MemoResponse,
    pub sidebar: SidebarView,
}
```

`PageView` は `render_page` に渡しやすい形にする。`SidebarView` は service 内部の owned 中間型として定義し、`SingleFile` または `Directory { directory_name, file_list, current_file }` を表す。handler は `SidebarView` を `SidebarParams` に変換して `render_page` へ渡す。これにより、service が `ResolvedTarget` の借用を外へ漏らさず、HTTP adapter 側のライフタイムを単純に保つ。

service 関数は次の形を想定する。

```rust
pub(super) async fn load_page(
    state: &AppState,
    request: PageRequest<'_>,
) -> Result<PageView, ApiError>;

pub(super) async fn load_content(
    state: &AppState,
    request: ContentRequest<'_>,
) -> Result<UpdateMessage, ApiError>;

pub(super) async fn load_memo(
    state: &AppState,
    request: MemoRequest<'_>,
) -> Result<MemoResponse, ApiError>;

pub(super) async fn save_memo(
    state: &AppState,
    request: SaveMemoRequest<'_>,
) -> Result<MemoResponse, ApiError>;

pub(super) fn list_files(state: &AppState) -> Result<Vec<String>, ApiError>;

pub(super) async fn search(
    state: &AppState,
    query: String,
) -> Result<SearchResponse, ApiError>;
```

## 6. データフロー

### 6.1 `GET /`

1. `index_handler` が Host 検証を通す。
2. `PageRequest { file }` を作る。
3. `load_page(state, request)` を呼ぶ。
4. service が `RouteTargetRequest::page(file)` で対象を解決する。
5. service が本文 update と memo を読み込む。
6. memo 読み込み失敗時は現状どおり warn ログを出し、空メモへフォールバックする。
7. handler が `render_page` で HTML を返す。

### 6.2 `GET /api/content`

1. handler が Host 検証を通す。
2. `ContentRequest { file }` を作る。
3. service が `RouteTargetRequest::api_content(file)` で対象を解決する。
4. service が `load_route_update` を呼ぶ。
5. handler が `Json(update)` を返す。

### 6.3 `GET /api/memo`

1. handler が Host 検証を通す。
2. `MemoRequest { file }` を作る。
3. service が `RouteTargetRequest::api_memo(file)` で対象を解決する。
4. service が `load_route_memo` を呼ぶ。
5. handler が `Json(memo)` を返す。

### 6.4 `PUT /api/memo`

1. handler が Host 検証を通す。
2. `SaveMemoRequest { file, raw }` を作る。
3. service が `RouteTargetRequest::api_memo(file)` で対象を解決する。
4. service が `save_route_memo` を呼ぶ。
5. 保存成功後だけ service が `BroadcastMessage::MemoUpdate` を送る。
6. `receiver_count == 0` の早期 return は現状維持とし、この設計では silent failure TODO を扱わない。
7. handler が `Json(memo)` を返す。

### 6.5 `GET /api/files`

1. handler が Host 検証を通す。
2. service が directory mode なら `list_markdown_files` を呼ぶ。
3. single file mode なら現状どおり空配列を返す。

### 6.6 `GET /api/search`

1. handler が Host 検証を通す。
2. service が query を trim 可能な形で受け取る。
3. directory mode なら `search_directory` を呼ぶ。
4. single file mode なら現状どおり空結果を返す。
5. 検索負荷制御は別 TODO に残し、この移行では挙動を変えない。

## 7. 移行順

1. service module を追加し、`GET /` のページ組み立てだけを移す。
2. `GET /api/content` を service 経由にする。
3. `GET /api/memo` と `PUT /api/memo` を service 経由にする。
4. `GET /api/files` と `GET /api/search` を service 経由にする。
5. `RouteContext` の application service 的メソッドを削除する。
6. `RouteContext` が不要になったら削除する。必要な場合も HTTP input 用の小さな request builder に縮小する。

この順番により、各ステップで既存テストを通しながら変更できる。

## 8. テスト方針

実装時は TDD を使い、service 境界のテストを先に追加する。

追加・更新するテスト観点:

- `page` request から `api_memo` 相当の memo request が同じ `file` query を使う。
- index page の memo 読み込み失敗は空メモへフォールバックする。
- `api/content` は `RouteTargetRequest::api_content` のエラー文言を維持する。
- `api/memo` は `RouteTargetRequest::api_memo` のエラー文言を維持する。
- memo 保存成功時だけ `MemoUpdate` が broadcast される。
- memo 保存失敗時は broadcast されない。
- single file mode の files/search は現状どおり空応答になる。
- 不正 Host は HTTP route で拒否される。
- WebSocket は引き続き Host + Origin の二段検証を行う。

既存の HTTP 統合テストは外部挙動の回帰検知として維持する。

## 9. セキュリティ考慮

- Host 検証を弱めない。middleware 化は非ゴールだが、HTTP route の処理手順から Host 検証を消さない。
- WebSocket は現状どおり Host 検証と Origin 検証を両方通す。
- service は `resolve_route_target` が返した `ResolvedTarget` の `file_path` だけを I/O に渡す。
- 未検証の query string、JSON body の `file`、外部入力由来のパス文字列を直接 `std::fs` / `tokio::fs` / `read_and_render_file` に渡さない。
- メモ本文や Markdown 本文は信頼しない。HTML sanitization、CSP、パス検証、ファイルサイズ上限の既存契約は変更しない。
- service 分割により、セキュリティ境界を「handler のついで」ではなく「対象解決済みデータだけを扱う層」として表現する。

## 10. 影響範囲

直接影響:

- `src/server/routes.rs`: handler を薄くし、`RouteContext` を削除または縮小する。
- `src/server/service.rs`: application service を追加する。
- `src/server/mod.rs`: 新 module を公開範囲内に追加する可能性がある。

間接影響:

- `src/server/files/resolve.rs`: request 型や helper の公開範囲を必要最小限だけ調整する可能性がある。
- `src/server/files/content.rs`: service から呼びやすいよう公開範囲を調整する可能性がある。
- `src/server/files/memo.rs`: service から呼びやすいよう公開範囲を調整する可能性がある。
- `src/server/files/search.rs`, `src/server/files/catalog.rs`: files/search service から呼ぶ既存関数のまま使う。
- `tests/integration_test.rs`: 外部挙動確認は既存テストを維持し、必要なら service 移行後の回帰テストを追加する。

## 11. 受け入れ基準

- `routes.rs` の handler が HTTP input/output 変換中心になっている。
- `RouteContext` の application service 的メソッドが消えている。
- 対象解決、本文ロード、メモロード/保存、sidebar 構築、memo broadcast の手順が service 側にまとまっている。
- 既存 API、HTML、WebSocket の外部挙動が変わらない。
- 不正 Host と不正 Origin の拒否契約が維持されている。
- 未検証パスを I/O に渡さない契約が service テストまたは境界テストで固定されている。
- `cargo fmt --all -- --check` が通る。
- `cargo clippy --all-targets --all-features -- -D warnings` が通る。
- `cargo test --all-targets --all-features` が通る。
- 最終確認として `./verify.sh` が通る。

## 12. ロールバック

service 追加と handler 移行は、できるだけ小さなコミットに分ける。問題が出た場合は該当コミットを revert すれば `routes.rs` 中心の現状構造へ戻せるようにする。

設計書のみの段階では、この spec 追加コミットを revert すれば元に戻せる。
