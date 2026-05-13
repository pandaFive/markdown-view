# Issue 146 Large File Repartition Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `watcher/runtime.rs`、`src/server/files/tests.rs`、`tests/integration_test.rs` を責務単位に分割し、既存挙動とセキュリティ検証を維持したままレビュー局所性を改善する。

**Architecture:** 先に test file を分割して検証境界を読みやすくし、その後 watcher runtime の内部 module を分割する。外部公開面、CLI、HTTP/WebSocket 契約、`cargo test --test integration_test` の呼び出し名は維持する。

**Tech Stack:** Rust、Cargo、Tokio、Axum、notify-debouncer-mini、reqwest、tokio-tungstenite、既存 `./verify.sh`

---

## Source Spec

- [docs/superpowers/specs/2026-05-13-issue-146-large-file-repartition-design.md](/home/propan/personal_dev/markdown-view/docs/superpowers/specs/2026-05-13-issue-146-large-file-repartition-design.md)

## File Structure

### Create

- `src/server/files/tests/mod.rs`: `src/server/files/tests.rs` の親 module。分割先 test module を宣言する。
- `src/server/files/tests/support.rs`: `src/server/files/tests.rs` 由来の共有 test helper。複数 test module から使う helper だけを置く。
- `src/server/files/tests/memo_sidecar.rs`: sidecar filename と sidecar parent の unit tests。
- `src/server/files/tests/resolve.rs`: `resolve_file`、`resolve_change_target`、`revalidate_single_file_target` の tests。
- `src/server/files/tests/catalog.rs`: markdown file listing、canonical directory、recursion guard の tests。
- `src/server/files/tests/content.rs`: read limit、route content/update、error response の tests。
- `src/server/files/tests/memo_route.rs`: memo route、legacy migration、atomic save/delete の tests。
- `src/server/files/tests/socket_update.rs`: initial socket update と lagged recovery message の tests。
- `src/server/files/tests/change_broadcast.rs`: change broadcast message と validation error broadcast の tests。
- `tests/integration/mod.rs`: integration test submodules の親 module。
- `tests/integration/support.rs`: server 起動、AppState 構築、WebSocket 接続、permission guard、JSON error assertion。
- `tests/integration/single_file.rs`: 単一ファイル mode の integration tests。
- `tests/integration/directory.rs`: directory mode の files/content/default/readme/tree tests。
- `tests/integration/memo.rs`: memo API integration tests。
- `tests/integration/security.rs`: Host/Origin、traversal、hidden file、symlink、CSP/security headers tests。
- `tests/integration/websocket.rs`: WebSocket connect、broadcast、close frame、lagged recovery tests。
- `tests/integration/search.rs`: directory search、query length、raw query、result limit tests。
- `tests/integration/rendering.rs`: rendered HTML、line attributes、tab UI visibility tests。
- `src/watcher/runtime/mod.rs`: `Watcher` public methods、`Drop`、module wiring、外部に必要な constants/re-exports。
- `src/watcher/runtime/health.rs`: `WatcherHealth`、`WatcherFailureKind`、`WatcherHealthState`。
- `src/watcher/runtime/error_queue.rs`: priority error channel と queue。
- `src/watcher/runtime/registration.rs`: dynamic directory registry と watch plan registration。
- `src/watcher/runtime/dispatch.rs`: event sender、debounced event processing、event send helpers。
- `src/watcher/runtime/thread.rs`: watcher thread runtime、event loop、panic handling。
- `src/watcher/runtime/shutdown.rs`: thread join、merge forwarder、shutdown diagnostics。

### Modify

- `src/server/files/mod.rs`: `#[cfg(test)] mod tests;` が directory module を指す状態を維持する。
- `tests/integration_test.rs`: test body を `tests/integration/` に移し、`mod integration;` だけを残す。
- `src/watcher/mod.rs`: `mod runtime;` と existing re-export を維持する。必要があれば re-export path だけ調整する。

### Remove

- `src/server/files/tests.rs`: `src/server/files/tests/` への分割完了後に削除する。
- `src/watcher/runtime.rs`: `src/watcher/runtime/` への分割完了後に削除する。

## Task 1: Baseline と作業ブランチ確認

**Files:**
- Read: `docs/superpowers/specs/2026-05-13-issue-146-large-file-repartition-design.md`
- Read: `src/server/files/tests.rs`
- Read: `tests/integration_test.rs`
- Read: `src/watcher/runtime.rs`

- [ ] **Step 1: ブランチと作業ツリーを確認する**

Run:

```bash
git status --short --branch
```

Expected:

```text
## docs/issue-146-large-file-repartition-design
```

作業ツリーに未コミット変更がある場合は、ユーザー変更か前タスク変更かを確認してから進める。ユーザー変更は revert しない。

- [ ] **Step 2: baseline test を実行する**

Run:

```bash
cargo test --lib server::files
cargo test --test integration_test
cargo test --lib watcher
```

Expected: 3 commands がすべて PASS。

失敗した場合は実装に入らず、失敗 test 名、失敗理由、既存失敗かどうかを記録してユーザーへ確認する。

- [ ] **Step 3: 現在の巨大ファイル行数を記録する**

Run:

```bash
wc -l src/server/files/tests.rs tests/integration_test.rs src/watcher/runtime.rs
```

Expected: `src/server/files/tests.rs`、`tests/integration_test.rs`、`src/watcher/runtime.rs` の行数が表示される。

## Task 2: `src/server/files/tests.rs` を module ごとに分割する

**Files:**
- Create: `src/server/files/tests/mod.rs`
- Create: `src/server/files/tests/support.rs`
- Create: `src/server/files/tests/memo_sidecar.rs`
- Create: `src/server/files/tests/resolve.rs`
- Create: `src/server/files/tests/catalog.rs`
- Create: `src/server/files/tests/content.rs`
- Create: `src/server/files/tests/memo_route.rs`
- Create: `src/server/files/tests/socket_update.rs`
- Create: `src/server/files/tests/change_broadcast.rs`
- Delete: `src/server/files/tests.rs`
- Test: `cargo test --lib server::files`

- [ ] **Step 1: 分割先 directory と親 module を作る**

Create `src/server/files/tests/mod.rs`:

```rust
mod catalog;
mod change_broadcast;
mod content;
mod memo_route;
mod memo_sidecar;
mod resolve;
mod socket_update;
mod support;
```

- [ ] **Step 2: 共有 import と helper を `support.rs` に移す**

Create `src/server/files/tests/support.rs` with shared helpers moved from the bottom and top of `src/server/files/tests.rs`:

```rust
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt};

use crate::server::files::test_support::TempWorkspace;
use crate::server::{AppMode, AppState, CanonicalPath};

pub(super) fn create_test_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("テスト用一時ディレクトリを作成できる")
}

pub(super) fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
    let temp_dir = create_test_dir();
    let path = temp_dir.path().join(name);
    std::fs::write(&path, content).expect("テスト用markdownを書き込める");
    (temp_dir, path)
}

#[cfg(unix)]
pub(super) struct PermissionGuard {
    path: PathBuf,
    mode: u32,
}

#[cfg(unix)]
impl Drop for PermissionGuard {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}

#[cfg(unix)]
pub(super) fn make_dir_unsearchable(dir: &Path, probe: &Path) -> Option<PermissionGuard> {
    let metadata = fs::metadata(dir).ok()?;
    let mode = metadata.permissions().mode();
    fs::set_permissions(dir, fs::Permissions::from_mode(0o000)).ok()?;
    if probe.try_exists().is_ok() {
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(mode));
        return None;
    }
    Some(PermissionGuard {
        path: dir.to_path_buf(),
        mode,
    })
}

pub(super) fn create_single_file_state(file_path: &Path) -> AppState {
    AppState::new(
        AppMode::SingleFile(CanonicalPath::new(file_path.to_path_buf())),
        TempWorkspace::new().tx,
    )
}

pub(super) fn create_directory_state(dir_path: &Path) -> AppState {
    AppState::new(
        AppMode::Directory(CanonicalPath::new(dir_path.to_path_buf())),
        TempWorkspace::new().tx,
    )
}
```

After the initial move, preserve existing helper bodies verbatim. If compile errors remain, fix imports and visibility only; keep helper names unchanged when tests already call them.

- [ ] **Step 3: sidecar tests を `memo_sidecar.rs` に移す**

Move these tests and their private helper from `src/server/files/tests.rs`:

```text
assert_plain_sidecar_filename
test_sidecar_name_*
test_sidecar_parent_*
```

Start `src/server/files/tests/memo_sidecar.rs` with:

```rust
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use super::super::memo::{sidecar_parent_for_target_path, sidecar_parent_or_base};
use super::super::memo_sidecar::SidecarMemoName;

fn assert_plain_sidecar_filename(name: &str) {
    let path = Path::new(name);
    assert!(path.parent().is_none() || path.parent() == Some(Path::new("")));
    assert_eq!(
        path.file_name().and_then(|file_name| file_name.to_str()),
        Some(name)
    );
}
```

- [ ] **Step 4: resolve tests を `resolve.rs` に移す**

Move these test groups:

```text
test_resolve_file_*
test_resolve_change_target_*
test_revalidate_single_file_target_*
test_resolve_file_error_*
```

Start `src/server/files/tests/resolve.rs` with:

```rust
#[cfg(unix)]
use std::os::unix::fs::symlink;

use super::support::{create_directory_state, create_markdown_fixture, create_test_dir, make_dir_unsearchable};
use super::super::resolve::{resolve_change_target, revalidate_single_file_target};
use super::super::*;
```

- [ ] **Step 5: catalog tests を `catalog.rs` に移す**

Move these test groups:

```text
test_list_markdown_files_*
test_list_markdown_files_from_canonical_base_*
test_resolve_recursable_directory_*
test_ensure_current_dir_still_canonical_*
test_search_directory_canonical_base_再canonicalizeなしで検索する
test_search_directory_生成物ディレクトリ配下を検索しない
```

Start `src/server/files/tests/catalog.rs` with:

```rust
#[cfg(unix)]
use std::os::unix::fs::symlink;

use super::support::{create_markdown_fixture, create_test_dir};
use super::super::catalog::{
    canonicalize_dir_for_cycle, ensure_current_dir_still_canonical,
    list_markdown_files_from_canonical_base, MAX_DIR_DEPTH, MAX_FILE_LIST,
};
use super::super::search::search_directory;
```

- [ ] **Step 6: content tests を `content.rs` に移す**

Move these test groups:

```text
test_read_bytes_with_limit_*
test_read_markdown_error_into_response_*
test_resolve_route_target_*
test_load_route_update_*
```

Start `src/server/files/tests/content.rs` with:

```rust
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::support::{create_directory_state, create_markdown_fixture};
use super::super::content::{read_bytes_with_limit, ReadMarkdownError};
use super::super::resolve::RouteTargetRequest;
use super::super::*;
```

- [ ] **Step 7: memo route tests を `memo_route.rs` に移す**

Move these test groups and memo-specific test doubles:

```text
SymlinkBeforeRenameMemoFs
TmpSymlinkBeforeRenameMemoFs
MismatchedTmpParentMemoFs
test_load_route_memo_*
test_save_route_memo_*
```

Start `src/server/files/tests/memo_route.rs` with:

```rust
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
use super::super::memo_fs::MemoBeforeRenameError;
use super::support::{create_directory_state, create_markdown_fixture, create_single_file_state};
use super::super::memo_fs::{
    before_rename_future, BeforeRenameCheck, MemoFs, MemoReadError, MemoWriteError, TokioMemoFs,
};
use super::super::*;
```

- [ ] **Step 8: socket update tests を `socket_update.rs` に移す**

Move these test groups:

```text
test_load_initial_socket_update_*
test_build_lagged_recovery_message_*
```

Start `src/server/files/tests/socket_update.rs` with:

```rust
use super::support::{create_directory_state, create_markdown_fixture, create_single_file_state};
use super::super::*;
```

- [ ] **Step 9: change broadcast tests を `change_broadcast.rs` に移す**

Move these test groups:

```text
test_build_change_broadcast_message_*
```

Start `src/server/files/tests/change_broadcast.rs` with:

```rust
use super::support::{create_directory_state, create_markdown_fixture};
use super::super::*;
```

- [ ] **Step 10: 元ファイルを削除して module 解決を確認する**

Run:

```bash
cargo test --lib server::files
```

Expected: compile errors are limited to imports/visibility from the moved tests, then PASS after imports are corrected.

Do not change production behavior to make moved tests pass. Fix imports, module paths, and test helper ownership only.

- [ ] **Step 11: format と差分確認を実行する**

Run:

```bash
cargo fmt --all -- --check
git diff --check
wc -l src/server/files/tests/*.rs
```

Expected: format check PASS、diff check no output、分割後 test files の行数が表示される。

- [ ] **Step 12: 段階コミットする**

Run:

```bash
git add src/server/files/tests src/server/files/tests.rs
git commit -m "test: server files testsを責務別moduleへ分割"
```

Expected: `src/server/files/tests.rs` deletion and new `src/server/files/tests/*.rs` files are committed.

## Task 3: `tests/integration_test.rs` を spec 単位に分割する

**Files:**
- Modify: `tests/integration_test.rs`
- Create: `tests/integration/mod.rs`
- Create: `tests/integration/support.rs`
- Create: `tests/integration/single_file.rs`
- Create: `tests/integration/directory.rs`
- Create: `tests/integration/memo.rs`
- Create: `tests/integration/security.rs`
- Create: `tests/integration/websocket.rs`
- Create: `tests/integration/search.rs`
- Create: `tests/integration/rendering.rs`
- Test: `cargo test --test integration_test`

- [ ] **Step 1: integration 親 module を作る**

Replace `tests/integration_test.rs` content with:

```rust
mod integration;
```

Create `tests/integration/mod.rs`:

```rust
mod directory;
mod memo;
mod rendering;
mod search;
mod security;
mod single_file;
mod support;
mod websocket;
```

- [ ] **Step 2: shared helper を `support.rs` に移す**

Move helper types and functions from the bottom of the original `tests/integration_test.rs`:

```text
WsStream
WsReadHalf
build_single_file_state
build_dir_state
spawn_test_server
setup_single_file_server_from_path
atomic_save_markdown_file
setup_single_file_server_with_bytes
assert_json_error_for_paths
FilePermissionGuard
make_file_unreadable
setup_single_file_server
setup_dir_server
connect_ws
connect_ws_with_host
next_ws_message
assert_close_frame_message
```

Start `tests/integration/support.rs` with:

```rust
use std::path::Path;
use std::sync::Arc;

#[cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt};

use futures_util::{stream::SplitStream, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use markdown_view::server::{AppMode, AppState, WatchService};

pub(super) type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
pub(super) type WsReadHalf = SplitStream<WsStream>;
```

Keep helper behavior identical. Mark helpers as `pub(super)` only when another integration module uses them.

- [ ] **Step 3: single file tests を `single_file.rs` に移す**

Move:

```text
test_indexページ取得
test_apiコンテンツ取得
test_単一ファイルモードでfileクエリは無視される
test_api_content_io_エラーで500を返す
test_存在しないファイル時は404を返す
test_non_utf8ファイル読み込み時は422を返す
test_ファイルサイズ上限超過で413を返す
test_ファイルサイズ上限ちょうど10mbは200を返す
test_単一ファイルモード_シンボリックリンク差し替えを拒否する
test_単一ファイルモードの後方互換_api_filesは空配列
test_単一ファイルモードの後方互換_api_searchは空結果
test_単一ファイルモード_api_searchは長すぎるqueryを400で拒否する
```

Start `tests/integration/single_file.rs` with:

```rust
use super::support::{
    assert_json_error_for_paths, setup_single_file_server, setup_single_file_server_from_path,
    setup_single_file_server_with_bytes,
};
```

- [ ] **Step 4: memo tests を `memo.rs` に移す**

Move all tests beginning with:

```text
test_apiメモ_
```

Start `tests/integration/memo.rs` with:

```rust
use std::time::Duration;

use tokio::sync::broadcast;

use markdown_view::server::BroadcastMessage;

use super::support::setup_single_file_server;
```

- [ ] **Step 5: security tests を `security.rs` に移す**

Move:

```text
test_host_middlewareは*
assert_forbidden_with_security_headers
test_apiメモ_getは不正hostを拒否する
test_apiメモ_putは不正hostを拒否する
test_websocketは異なるoriginを拒否する
test_websocketはoriginポート不一致を拒否する
test_websocketはhost_middlewareで不正hostを拒否する
test_websocketはrebind相当のhost_origin一致を拒否する
test_セキュリティヘッダが設定されている
test_ディレクトリモード_トラバーサル攻撃拒否
test_ディレクトリモード_メモapiのパストラバーサルを拒否する
test_ディレクトリモード_隠しファイルの直接アクセスが拒否される
test_ディレクトリモード_api_filesは不正hostを拒否する
```

Start `tests/integration/security.rs` with:

```rust
use reqwest::Response;

use super::support::{
    connect_ws_with_host, setup_dir_server, setup_single_file_server,
};
```

- [ ] **Step 6: websocket tests を `websocket.rs` に移す**

Move:

```text
test_websocket接続
test_websocketブロードキャスト受信
test_ファイル変更でwebsocket更新
test_単一ファイルモード_atomic_save後にwebsocket更新
test_ファイル変更_io_エラーでwebsocketエラー通知
test_websocket切断時に購読が速やかに解放される
test_ディレクトリモード_websocket更新にfileフィールドが含まれる
test_ディレクトリモード_atomic_save後にwebsocket更新
test_ディレクトリモード_websocket更新はbackslashファイル名を保持する
test_ディレクトリモード_websocket初期メッセージが送信されない
test_監視エラーがwebsocketクライアントにエラーjsonとして届く
test_websocket_non_utf8ファイルでclose_frameにuser_messageが含まれる
test_websocket_削除済みファイルでclose_frameにuser_messageが含まれる
test_websocket_サイズ超過ファイルでclose_frameにuser_messageが含まれる
test_websocket_ioエラーでclose_frameが1011を返す
test_websocket_lagged_recovery_ioエラーでerror_jsonを送信する
```

Start `tests/integration/websocket.rs` with:

```rust
use std::time::Duration;

use markdown_view::server::BroadcastMessage;

use super::support::{
    assert_close_frame_message, atomic_save_markdown_file, connect_ws, make_file_unreadable,
    next_ws_message, setup_dir_server, setup_single_file_server, setup_single_file_server_from_path,
};
```

- [ ] **Step 7: directory tests を `directory.rs` に移す**

Move directory mode tests not assigned to security/search/websocket/rendering:

```text
test_ディレクトリモード_indexページ取得
test_ディレクトリモード_ファイル一覧api
test_ディレクトリモード_ファイル指定コンテンツ取得
test_ディレクトリモード_api_content_file空文字は404を返す
test_ディレクトリモード_存在しないファイル
test_ディレクトリモード_非mdファイル拒否
test_ディレクトリモード_ファイル指定でindex取得
test_ディレクトリモード_アクティブファイルマーカーが表示される
test_ディレクトリモード_ファイル名のhtmlエスケープ
test_ディレクトリモード_空ディレクトリで404を返す
test_ディレクトリモード_readmeなし時はアルファベット順最初のファイルがデフォルト
test_ディレクトリモード_readmeがデフォルト表示される
test_ディレクトリモード_ファイルツリーにディレクトリ構造が含まれる
```

Start `tests/integration/directory.rs` with:

```rust
use super::support::setup_dir_server;
```

- [ ] **Step 8: search tests を `search.rs` に移す**

Move:

```text
test_ディレクトリモード_検索apiは複数ファイルから結果を返す
test_ディレクトリモード_api_searchは長すぎるqueryを400で拒否する
test_ディレクトリモード_api_searchは削除済みbaseでも長すぎるqueryを400で拒否する
test_ディレクトリモード_api_searchはraw_query上限超過を400で拒否する
test_ディレクトリモード_api_searchは不正percent_encodingを400で拒否する
test_ディレクトリモード_api_searchは結果数打ち切りをjsonで返す
test_ディレクトリモード_検索apiは巨大ファイルをスキップする
test_ディレクトリモード_api_searchは不正hostを拒否する
```

Start `tests/integration/search.rs` with:

```rust
use super::support::setup_dir_server;
```

- [ ] **Step 9: rendering tests を `rendering.rs` に移す**

Move:

```text
test_ディレクトリモード_タブuiが表示される
test_単一ファイルモード_タブが表示されない
test_本文htmlにソース行番号属性と引用ボタンが含まれる
```

Start `tests/integration/rendering.rs` with:

```rust
use markdown_view::renderer::render_markdown;
use markdown_view::toc::generate_toc;

use super::support::{setup_dir_server, setup_single_file_server};
```

- [ ] **Step 10: integration test binary 名を維持して実行する**

Run:

```bash
cargo test --test integration_test
```

Expected: PASS。test binary name は `integration_test` のまま。

- [ ] **Step 11: format と差分確認を実行する**

Run:

```bash
cargo fmt --all -- --check
git diff --check
wc -l tests/integration_test.rs tests/integration/*.rs
```

Expected: format check PASS、diff check no output、分割後 test files の行数が表示される。

- [ ] **Step 12: 段階コミットする**

Run:

```bash
git add tests/integration_test.rs tests/integration
git commit -m "test: integration testsをspec単位へ分割"
```

Expected: integration test split is committed.

## Task 4: `src/watcher/runtime.rs` を内部責務ごとに分割する

**Files:**
- Create: `src/watcher/runtime/mod.rs`
- Create: `src/watcher/runtime/health.rs`
- Create: `src/watcher/runtime/error_queue.rs`
- Create: `src/watcher/runtime/registration.rs`
- Create: `src/watcher/runtime/dispatch.rs`
- Create: `src/watcher/runtime/thread.rs`
- Create: `src/watcher/runtime/shutdown.rs`
- Delete: `src/watcher/runtime.rs`
- Modify: `src/watcher/mod.rs` only if compiler requires path/re-export adjustment
- Test: `cargo test --lib watcher`
- Test: `cargo test --test integration_test`

- [ ] **Step 1: `runtime/` module directory を作り、親 module を作る**

Create `src/watcher/runtime/mod.rs`:

```rust
mod dispatch;
mod error_queue;
mod health;
mod registration;
mod shutdown;
mod thread;

pub(crate) use self::shutdown::WATCH_SHUTDOWN_TIMEOUT_SECS;
pub use self::health::{WatcherFailureKind, WatcherHealth};

pub struct Watcher {
    runtime: WatchRuntime,
}

use self::thread::WatchRuntime;
```

Then move the existing `Watcher` impl and `Drop` impl from `src/watcher/runtime.rs` into this file. Keep method bodies unchanged first.

- [ ] **Step 2: health state を `health.rs` に移す**

Move:

```text
WatcherHealth
WatcherFailureKind
WatcherHealthState
impl WatcherHealthState
```

Start `src/watcher/runtime/health.rs` with:

```rust
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatcherHealth {
    Starting,
    Alive,
    Failed(WatcherFailureKind),
    Stopping,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatcherFailureKind {
    Notify,
    ThreadPanic,
    ShutdownTaskPanic,
    ShutdownTimedOut,
    ForwarderTaskPanic,
    ForwarderStopped,
}
```

Set `WatcherHealthState` visibility to `pub(super)`.

- [ ] **Step 3: priority error queue を `error_queue.rs` に移す**

Move:

```text
WATCHER_ERROR_QUEUE_CAPACITY
WATCHER_ERROR_MESSAGE_BUFFER
PriorityErrorSender
PriorityErrorReceiver
PriorityErrorQueue
PriorityErrorQueueState
priority_error_channel
impl Clone for PriorityErrorSender
impl Drop for PriorityErrorSender
impl Drop for PriorityErrorReceiver
```

Start `src/watcher/runtime/error_queue.rs` with:

```rust
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use super::super::WatchError;

const WATCHER_ERROR_QUEUE_CAPACITY: usize = 64;
#[cfg(test)]
const WATCHER_ERROR_MESSAGE_BUFFER: usize = 8;
```

Set sender/receiver types and `priority_error_channel` to `pub(super)`.

- [ ] **Step 4: directory registration を `registration.rs` に移す**

Move:

```text
WatchRegistrationFailure
WatchDirectoryRegistry
normalize_watch_registry_path
register_watch_plan_with
```

Start `src/watcher/runtime/registration.rs` with:

```rust
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use notify::RecursiveMode;

use super::super::strategy::WatchStrategy;
```

Set `register_watch_plan_with` to `pub(super)` because thread startup uses it.

- [ ] **Step 5: dispatch helpers を `dispatch.rs` に移す**

Move:

```text
WATCHER_MESSAGE_BUFFER
WATCHER_INTERNAL_EVENT_BUFFER
InternalWatchResult
WatchEventSenders
BestEffortFileSender
WatcherDiagnostics
send_merged_file_changed_event
send_file_changed_event
send_error_event
send_internal_watch_result
handle_debounced_watch_result
process_debounced_events_with_watch
process_debounced_events_with_watch_and_unwatch
handle_internal_channel_disconnected
```

Start `src/watcher/runtime/dispatch.rs` with:

```rust
use std::path::{Path, PathBuf};

use tokio::sync::{mpsc, oneshot};

use super::error_queue::PriorityErrorSender;
use super::health::{WatcherFailureKind, WatcherHealthState};
use super::registration::WatchDirectoryRegistry;
use super::super::{WatchError, WatchEvent};

type InternalWatchResult =
    std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>;
```

Keep dynamic directory registration order unchanged.

- [ ] **Step 6: shutdown helpers を `shutdown.rs` に移す**

Move:

```text
WATCH_SHUTDOWN_TIMEOUT_SECS
WATCHER_THREAD_PARK_MS
WATCHER_DIAGNOSTIC_SEND_TIMEOUT_MS
WatcherThreadStopResult
MergeForwarderHandle
ForwarderDoneOnDrop
impl Drop for ForwarderDoneOnDrop
impl MergeForwarderHandle
join_watcher_thread_with_timeout
spawn_watch_event_merge_forwarder
record_shutdown_task_panic
```

Start `src/watcher/runtime/shutdown.rs` with:

```rust
use std::thread;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use super::dispatch::send_merged_file_changed_event;
use super::health::{WatcherFailureKind, WatcherHealthState};
use super::super::WatchEvent;

pub(crate) const WATCH_SHUTDOWN_TIMEOUT_SECS: u64 = 2;
const WATCHER_THREAD_PARK_MS: u64 = 250;
const WATCHER_DIAGNOSTIC_SEND_TIMEOUT_MS: u64 = 200;
```

- [ ] **Step 7: watcher thread runtime を `thread.rs` に移す**

Move:

```text
DEBOUNCE_MS
InitResult
WatchRuntime
impl WatchRuntime
spawn_watcher_thread
handle_watcher_panic
send_init_result
run_watcher_event_loop
```

Start `src/watcher/runtime/thread.rs` with:

```rust
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_mini::new_debouncer;
use tokio::sync::{mpsc, oneshot};

use super::dispatch::{handle_internal_channel_disconnected, send_internal_watch_result, WatchEventSenders};
use super::health::{WatcherFailureKind, WatcherHealth, WatcherHealthState};
use super::registration::register_watch_plan_with;
use super::shutdown::{join_watcher_thread_with_timeout, spawn_watch_event_merge_forwarder, WatcherThreadStopResult};
use super::super::{WatchError, WatchEvent};
use crate::server::AppMode;

const DEBOUNCE_MS: u64 = 300;
type InitResult = std::result::Result<(), WatchError>;
```

Set `WatchRuntime` to `pub(super)`.

- [ ] **Step 8: tests を最寄り module に移す**

Move tests from the old `mod tests` in `src/watcher/runtime.rs` to the module that owns the tested type/function:

```text
health.rs tests:
  WatcherHealthState tests

error_queue.rs tests:
  PriorityErrorSender/Receiver/Queue tests

registration.rs tests:
  WatchDirectoryRegistry and normalize_watch_registry_path tests

dispatch.rs tests:
  send event, debounced event processing, internal channel disconnect tests

shutdown.rs tests:
  join timeout, merge forwarder, shutdown diagnostic tests

thread.rs tests:
  spawn/init/panic/event loop tests
```

Each module test block should start with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
}
```

Add extra imports inside each test module only where needed. Do not broaden production visibility just for tests if `use super::*` is enough.

- [ ] **Step 9: old `runtime.rs` を削除して compile errors を収束する**

Run:

```bash
cargo test --lib watcher
```

Expected: import/visibility errors first, then PASS after corrections.

Rules for fixes:

```text
- Prefer pub(super) for runtime-internal items.
- Use pub(crate) only for items already consumed outside runtime.
- Do not change event ordering, timeout values, queue capacity, or health transition semantics.
- Do not trust WatchEvent::FileChanged(PathBuf) as validated input.
```

- [ ] **Step 10: integration regression を実行する**

Run:

```bash
cargo test --test integration_test
```

Expected: PASS。

- [ ] **Step 11: format と差分確認を実行する**

Run:

```bash
cargo fmt --all -- --check
git diff --check
wc -l src/watcher/runtime/*.rs
```

Expected: format check PASS、diff check no output、分割後 runtime files の行数が表示される。

- [ ] **Step 12: 段階コミットする**

Run:

```bash
git add src/watcher/runtime src/watcher/runtime.rs src/watcher/mod.rs
git commit -m "refactor: watcher runtimeを責務別moduleへ分割"
```

Expected: old `src/watcher/runtime.rs` deletion and new `src/watcher/runtime/*.rs` files are committed.

## Task 5: 全体検証と issue 146 完了確認

**Files:**
- Read: `docs/superpowers/specs/2026-05-13-issue-146-large-file-repartition-design.md`
- Read: `docs/superpowers/plans/2026-05-13-issue-146-large-file-repartition.md`
- Verify: all changed files

- [ ] **Step 1: full verification を実行する**

Run:

```bash
./verify.sh
```

Expected: format、clippy、tests がすべて PASS。

- [ ] **Step 2: 分割後の行数を確認する**

Run:

```bash
wc -l src/server/files/tests/*.rs tests/integration_test.rs tests/integration/*.rs src/watcher/runtime/*.rs
```

Expected: `src/server/files/tests.rs`、`tests/integration_test.rs`、`src/watcher/runtime.rs` への追記集中が解消され、責務別 file の行数が表示される。

- [ ] **Step 3: public API と test binary 名を確認する**

Run:

```bash
cargo test --test integration_test -- --list
cargo test --lib watcher -- --list
```

Expected:

```text
integration_test
```

The exact test list is long; confirm the command succeeds and `integration_test` binary remains callable.

- [ ] **Step 4: final diff を確認する**

Run:

```bash
git status --short --branch
git diff --stat HEAD~3..HEAD
```

Expected: 3 implementation commits exist after this plan commit, with changes limited to planned files.

- [ ] **Step 5: completion report 用の残留リスクを記録する**

Record these points in the final response:

```text
- search.rs and guards.rs were observed but not split in this scope.
- WatchEvent::FileChanged(PathBuf) remains untrusted input; validation stays in server/files revalidation.
- If any watcher test was skipped due platform permission semantics, report the exact skip output.
```

## Self Review

- Spec coverage: server/files tests 分割、integration tests 分割、watcher runtime 分割、security considerations、verification、rollback path を各 task に対応させた。
- Placeholder scan: 未確定のまま残した作業項目はない。
- Type consistency: `WatcherHealth`、`WatcherFailureKind`、`WatcherHealthState`、`WatchRuntime`、`PriorityErrorSender`、`WatchDirectoryRegistry`、`WatchEvent::FileChanged(PathBuf)` の名前は spec と既存 code に合わせた。
- Scope check: `search.rs` と `guards.rs` は設計通り観察対象に留め、実装 task には含めない。
