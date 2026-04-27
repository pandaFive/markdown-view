# memo保存契約再設計とテスト基盤刷新 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** memo 保存・読み込みの契約を sidecar 一本化方針で再設計し、`trait MemoFs` ベースの DI テスト基盤を導入し、`chmod` 依存テストを全廃する。

**Architecture:** `AppState` 経由で `Arc<dyn MemoFs>` を注入する。本番は `TokioMemoFs`（zero-sized）、テストは `MockMemoFs` で `(Op, PathBuf)` キーの IO エラー注入。`save_route_memo` は失敗時 fallback を持たず常に sidecar に書く。読み込みは sidecar → compat_sidecar → legacy の優先順、保存成功時に旧形式を best-effort で cleanup。

**Tech Stack:** Rust 2021、axum 0.8、tokio、`async-trait` 0.1（新規依存）、tempfile（既存）

**Spec:** `docs/superpowers/specs/2026-04-28-memo-save-contract-redesign-design.md`

**Branch:** `docs/memo-save-contract-redesign-spec` から派生して `feat/memo-save-contract-redesign` を作成して作業

---

## 前提と注意事項

### 既存コードとの不整合（spec ⇔ 実装）

- spec 5.4 で `AppState` のフィールド名を `sender: broadcast::Sender<...>` / `syntax_theme: String` と書いていたが、実コードは `tx: broadcast::Sender<...>` / `syntax_css: String`。本プランは **実コードの命名 (`tx` / `syntax_css`)** に合わせる。
- spec 5.4 では `pub fn new(mode, sender, theme, dark)` と書いていたが、実シグネチャは `pub fn new(mode: AppMode, dark_mode: bool, theme: Option<String>, tx: broadcast::Sender<BroadcastMessage>) -> Self`。本プランは **実シグネチャ** に合わせる。
- spec で「`tokio::fs::*` 直呼び 12 箇所」と書いていたが実際は **14 箇所**（うち L613 の `tokio::fs::File::open` は trait に存在しないため `read_memo_file` を `MemoFs::read` 経由に書き換え）。

### 実コードの主要 API（テストコード書く際の参照）

- **`ApiError` の実体**: `pub(super) type ApiError = (StatusCode, Json<serde_json::Value>);`（`src/server/messages.rs:28`）。テストでは `let (status, _body) = result.expect_err("...");` でタプル分解。
- **`MemoResponse` の accessor**: `raw() -> &str`、`html() -> &str`、`file() -> Option<&str>`（`src/template/message.rs:43-`）。テスト用の特別な accessor は不要。
- **`RouteTargetRequest` のテストファクトリ**: `RouteTargetRequest::api_memo(None)` と `RouteTargetRequest::api_content()`（用途で使い分け。memo 保存テストは前者を使う）。
- **target 解決**: `resolve_route_target(&state, request) -> Result<ResolvedTarget, ApiError>` を使う（`revalidate_single_file_target` は別用途）。
- **既存テストヘルパー**: `create_directory_state(dir.path())` / `create_single_file_state(&file_path)` / `create_markdown_fixture(name, content)` が tests.rs 内に既存。新仕様で `MemoFs` を差し替えるテストは `make_test_app_state(mode, memo_fs)` を使う。

### `with_memo_fs` の可視性

`test_support.rs` を `#[cfg(test)] mod test_support` として登録するため、`AppState::with_memo_fs` も `#[cfg(test)] pub(crate)` で良い。

---

## Phase A: trait MemoFs と TokioMemoFs の導入（既存挙動不変）

### Task 1: async-trait クレートを Cargo.toml に追加

**Files:**
- Modify: `Cargo.toml:7-23`

- [ ] **Step 1: Cargo.toml に async-trait を追加**

`Cargo.toml` の `[dependencies]` に `async-trait = "0.1"` を追加する。`anyhow` の直後（アルファベット順）。

`[dependencies]` セクションを以下のように変更:

```toml
[dependencies]
anyhow = "1"
async-trait = "0.1"
axum = { version = "0.8", features = ["ws"] }
base64 = "0.22"
clap = { version = "4", features = ["derive"] }
notify = "8"
notify-debouncer-mini = "0.6"
open = "5"
pulldown-cmark = { version = "0.13", features = ["simd"] }
sha2 = "0.10"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
syntect = "5"
tokio = { version = "1", features = ["full"] }
tower-http = { version = "0.6", features = ["set-header"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
```

- [ ] **Step 2: cargo build でクレート取得を確認**

Run: `cargo build`
Expected: ビルド成功（warning は許容、新エラーなし）

- [ ] **Step 3: コミット**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: async-trait依存を追加 (memo保存契約再設計の前準備)"
```

### Task 2: `src/server/files/memo_fs.rs` を新設し、`MemoFs` trait と `TokioMemoFs` を実装

**Files:**
- Create: `src/server/files/memo_fs.rs`

- [ ] **Step 1: memo_fs.rs を新規作成**

`src/server/files/memo_fs.rs` を以下の内容で作成:

```rust
//! メモ保存・読み込みで使用するファイルシステム抽象。
//!
//! 本番では [`TokioMemoFs`] が `tokio::fs::*` を呼び出す薄いラッパーとして動作する。
//! テストでは `MockMemoFs`（`test_support` モジュール）を注入し、
//! 特定パスの I/O エラーを決定論的に再現する。

use std::fs::Metadata;
use std::path::Path;

use async_trait::async_trait;

/// メモ保存先ファイルシステムの抽象。
///
/// `MAX_FILE_SIZE` の二段階チェック等のドメイン責務は呼び出し側で行い、
/// このトレイトはシステムコールの薄いラッパーに専念する。
/// `NotFound` 等の特殊エラー処理も呼び出し側で吸収する。
#[async_trait]
pub(crate) trait MemoFs: Send + Sync + std::fmt::Debug {
    /// パス存在確認。シンボリックリンク要素は呼び出し側で別途検査済み想定。
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool>;

    /// メタデータ取得（サイズ制限の一段目チェック用）
    async fn metadata(&self, path: &Path) -> std::io::Result<Metadata>;

    /// バイト列読み込み。サイズ制限は呼び出し側で再検証する（TOCTOU 二段目）。
    async fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;

    /// 親ディレクトリを再帰的に作成（既存ならエラーを返さない）
    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()>;

    /// バイト列書き込み（atomic は要求しない）
    async fn write(&self, path: &Path, content: &[u8]) -> std::io::Result<()>;

    /// ファイル削除。`NotFound` を含むエラーは透過する（呼び出し側で吸収）。
    async fn remove_file(&self, path: &Path) -> std::io::Result<()>;
}

/// 本番用 [`MemoFs`] 実装。`tokio::fs::*` を直接呼び出す。
#[derive(Debug, Default)]
pub(crate) struct TokioMemoFs;

#[async_trait]
impl MemoFs for TokioMemoFs {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool> {
        tokio::fs::try_exists(path).await
    }

    async fn metadata(&self, path: &Path) -> std::io::Result<Metadata> {
        tokio::fs::metadata(path).await
    }

    async fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        tokio::fs::read(path).await
    }

    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        tokio::fs::create_dir_all(path).await
    }

    async fn write(&self, path: &Path, content: &[u8]) -> std::io::Result<()> {
        tokio::fs::write(path, content).await
    }

    async fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        tokio::fs::remove_file(path).await
    }
}
```

- [ ] **Step 2: files/mod.rs に memo_fs モジュールを登録**

`src/server/files/mod.rs:5` の `mod memo;` の直後に `mod memo_fs;` を追加し、`mod` 群が以下のようになるようにする:

```rust
mod catalog;
mod content;
mod memo;
mod memo_fs;
mod memo_sidecar;
mod resolve;
mod search;

#[cfg(test)]
mod tests;
```

`pub use` ブロックの末尾（既存の `pub(in crate::server) use self::search::*;` の直後）に、本プランで `state.rs` から参照するための公開を追加:

```rust
pub(in crate::server) use self::memo_fs::{MemoFs, TokioMemoFs};
```

- [ ] **Step 3: cargo build で trait 定義のコンパイル確認**

Run: `cargo build --all-targets`
Expected: ビルド成功（trait は誰も使っていないので unused warning 可、エラーなし）

- [ ] **Step 4: コミット**

```bash
git add Cargo.toml Cargo.lock src/server/files/memo_fs.rs src/server/files/mod.rs
git commit -m "feat: trait MemoFsとTokioMemoFsを新設 (memo保存契約再設計Phase A)"
```

---

## Phase B: AppState への DI 化と memo.rs 内の tokio::fs 置換（既存挙動不変）

このフェーズで本番 `AppState` を DI 化し、`memo.rs` の `tokio::fs::*` 呼び出しを `state.memo_fs()` 経由に切り替える。**既存の fallback ロジックは残したまま**で、観察可能な挙動は不変。完了時に `cargo test` は全件 pass する想定。

### Task 3: `AppState` に `memo_fs` フィールドと API を追加

**Files:**
- Modify: `src/server/state.rs:200-243`

- [ ] **Step 1: AppState に memo_fs フィールドを追加**

`src/server/state.rs:1-8` の use 群を以下に拡張（`std::sync::Arc` と `MemoFs`/`TokioMemoFs` の取り込み）:

```rust
//! サーバー状態とモード判定を管理する。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::broadcast;

use super::files::{MemoFs, TokioMemoFs};
use super::messages::BroadcastMessage;
use crate::renderer::syntax_theme_css;
```

`AppState` 構造体（`src/server/state.rs:200-206`）を以下に書き換え:

```rust
/// サーバー共有状態
#[derive(Debug, Clone)]
pub struct AppState {
    mode: AppMode,
    dark_mode: bool,
    syntax_css: String,
    tx: broadcast::Sender<BroadcastMessage>,
    memo_fs: Arc<dyn MemoFs>,
}
```

`AppState::new` を以下に書き換え（フィールド初期化に `memo_fs` を追加）:

```rust
impl AppState {
    /// `AppState` を生成する
    pub fn new(
        mode: AppMode,
        dark_mode: bool,
        theme: Option<String>,
        tx: broadcast::Sender<BroadcastMessage>,
    ) -> Self {
        Self {
            syntax_css: syntax_theme_css(theme.as_deref()),
            mode,
            dark_mode,
            tx,
            memo_fs: Arc::new(TokioMemoFs),
        }
    }
```

`AppState::tx` メソッドの直後（`src/server/state.rs:243` の `}` の直前）に `memo_fs` accessor と test 用 `with_memo_fs` を追加:

```rust
    /// メモ保存・読み込みで使用するファイルシステム抽象を返す
    pub(crate) fn memo_fs(&self) -> &Arc<dyn MemoFs> {
        &self.memo_fs
    }

    /// テスト用にメモ用ファイルシステムを差し替える
    #[cfg(test)]
    pub(crate) fn with_memo_fs(mut self, memo_fs: Arc<dyn MemoFs>) -> Self {
        self.memo_fs = memo_fs;
        self
    }
}
```

- [ ] **Step 2: `#[derive(Debug)]` のままで Arc<dyn MemoFs> が compile することを cargo build で確認**

Run: `cargo build --all-targets`
Expected: ビルド成功。`MemoFs: Debug` 制約があるため `Arc<dyn MemoFs>` の `Debug` も導出される。

- [ ] **Step 3: 既存テストがそのまま pass することを確認**

Run: `cargo test --all-targets --all-features --no-run`
Expected: コンパイル成功

Run: `cargo test state::tests::test_app_mode -- --nocapture`
Expected: 既存の `test_app_mode_*` テストが全件 pass

- [ ] **Step 4: コミット**

```bash
git add src/server/state.rs
git commit -m "feat: AppStateにmemo_fs DIフィールドを追加 (Phase B-1)"
```

### Task 4: `memo.rs` 内の `tokio::fs::*` を `state.memo_fs()` 経由に置換（fallback 既存ロジック保持）

**Files:**
- Modify: `src/server/files/memo.rs`（複数箇所、下記参照）

- [ ] **Step 1: 各関数のシグネチャに `fs: &dyn MemoFs` 引数を追加**

呼び出しチェーンの末端から順に書き換える。最終的に `memo.rs` 内のすべての non-pure 関数が `fs: &dyn MemoFs` を受け取る形にする。

対象関数（既存の関数名を維持し、`fs` 引数を追加）:
- `load_route_memo` → 内部で `state.memo_fs().as_ref()` を取得して下に渡す
- `save_route_memo` → 同上
- `resolve_active_memo_path(state, target, request, memo_paths)` → `(state, target, request, memo_paths, fs: &dyn MemoFs)`
- `choose_save_target(state, target, request, memo_paths)` → 同上
- `compat_sidecar_exists(state, target, request, memo_paths)` → 同上
- `legacy_memo_exists(legacy_memo_path, target, request)` → `(legacy_memo_path, target, request, fs)`
- `inspect_legacy_memo(state, target, request, legacy_memo_path)` → `(state, target, request, legacy_memo_path, fs)`
- `save_memo_to_legacy(state, target, request, legacy_path, raw)` → `(state, target, request, legacy_path, raw, fs)`
- `save_memo_to_fallback(...)` → `fs` 追加
- `save_memo_to_existing_compat(...)` → 同上
- `save_memo_to_existing_compat_or_legacy(...)` → 同上
- `cleanup_legacy_memo_if_safe(...)` → `fs` 追加
- `cleanup_compat_sidecar_if_safe(...)` → 同上
- `delete_compat_sidecar_if_safe(...)` → 同上
- `delete_legacy_memo_if_safe_strict(...)` → 同上
- `read_memo_file(memo_path, target, request)` → `(memo_path, target, request, fs)`
- `delete_memo_file_if_exists(memo_path, target, request)` → `(memo_path, target, request, fs)`

`load_route_memo` / `save_route_memo` の本体冒頭で `let fs = state.memo_fs().as_ref();` を一度だけ取得し、内部呼び出しに渡し回す。

- [ ] **Step 2: `tokio::fs::*` 呼び出し 13 箇所を `fs.*().await` に置換**

具体的な置換マッピング（`src/server/files/memo.rs` 内、行番号は基準時点）:

| 行 | 置換前 | 置換後 |
|----|-------|-------|
| 26 | `tokio::fs::try_exists(&memo_path)` | `fs.try_exists(&memo_path)` |
| 74 | `tokio::fs::create_dir_all(parent)` | `fs.create_dir_all(parent)` |
| 82 | `tokio::fs::write(memo_path, raw.as_bytes())` | `fs.write(memo_path, raw.as_bytes())` |
| 206 | `tokio::fs::try_exists(&memo_paths.sidecar)` | `fs.try_exists(&memo_paths.sidecar)` |
| 232 | `tokio::fs::try_exists(&memo_paths.sidecar)` | `fs.try_exists(&memo_paths.sidecar)` |
| 296 | `tokio::fs::try_exists(compat_sidecar)` | `fs.try_exists(compat_sidecar)` |
| 306 | `tokio::fs::try_exists(legacy_memo_path)` | `fs.try_exists(legacy_memo_path)` |
| 372 | `tokio::fs::create_dir_all(parent)` | `fs.create_dir_all(parent)` |
| 376 | `tokio::fs::write(legacy_path, raw.as_bytes())` | `fs.write(legacy_path, raw.as_bytes())` |
| 437 | `tokio::fs::write(compat_sidecar, raw.as_bytes())` | `fs.write(compat_sidecar, raw.as_bytes())` |
| 460 | `tokio::fs::write(compat_sidecar, raw.as_bytes())` | `fs.write(compat_sidecar, raw.as_bytes())` |
| 603 | `tokio::fs::metadata(memo_path)` | `fs.metadata(memo_path)` |
| 638 | `tokio::fs::remove_file(memo_path)` | `fs.remove_file(memo_path)` |

- [ ] **Step 3: `read_memo_file` の `tokio::fs::File::open` + `read_bytes_with_limit` を `fs.read` ベースに書き換え**

`src/server/files/memo.rs:598-631` の `read_memo_file` を以下に書き換え（`use super::content::read_bytes_with_limit` は不要になる、削除）:

```rust
async fn read_memo_file(
    memo_path: &Path,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    fs: &dyn MemoFs,
) -> Result<String, ApiError> {
    let metadata = fs
        .metadata(memo_path)
        .await
        .map_err(|error| io_api_error(target, request, "メタデータ取得", error))?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }

    let bytes = fs
        .read(memo_path)
        .await
        .map_err(|error| io_api_error(target, request, "読込", error))?;
    if bytes.len() as u64 > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }
    String::from_utf8(bytes).map_err(|error| {
        tracing::warn!(
            "[markdown-view] {}メモUTF-8デコード失敗 ({}): {}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
        json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "メモはUTF-8テキストである必要があります",
        )
    })
}
```

`src/server/files/memo.rs:5` の use 文を以下に変更:

```rust
use super::content::{ReadMarkdownError, MAX_FILE_SIZE};
```

（`read_bytes_with_limit` を削除し、`ReadMarkdownError` のみ残す。`read_error_to_api_error` 関数は依然 `ReadMarkdownError` を引数で取るため `ReadMarkdownError` の use は維持。ただし `read_memo_file` から呼ばれなくなるため、`read_error_to_api_error` 自体が dead code になる可能性 → コンパイラの dead_code 警告で確認）

`memo.rs` の use 文に `MemoFs` を追加:

```rust
use super::memo_fs::MemoFs;
```

- [ ] **Step 4: `read_error_to_api_error` が dead code になっていないか確認**

`memo.rs` 内で `read_error_to_api_error` の参照が他に無い場合は削除する:

Run: `grep -n "read_error_to_api_error" src/server/files/memo.rs`
Expected: 1 件（定義箇所のみ）→ 削除する。0 件なら既に削除済み。

該当する場合、関数定義（およそ `src/server/files/memo.rs:664-689`）と use 文の `ReadMarkdownError` を削除し、use を以下にする:

```rust
use super::content::MAX_FILE_SIZE;
```

- [ ] **Step 5: cargo build で型エラーがないことを確認**

Run: `cargo build --all-targets`
Expected: ビルド成功（unused 警告は無視可）

- [ ] **Step 6: 既存の memo 系テストが全件 pass することを確認**

Run: `cargo test --all-targets --all-features memo`
Expected: 既存 16+ テストが全件 pass（chmod 依存テストも root 以外なら pass）。挙動は変えていないため。

- [ ] **Step 7: 全テスト実行**

Run: `cargo test --all-targets --all-features`
Expected: 全テスト pass

- [ ] **Step 8: コミット**

```bash
git add src/server/files/memo.rs
git commit -m "refactor: memo.rs内のtokio::fs::*をMemoFs経由に置換 (Phase B-2)"
```

---

## Phase C: テスト共通基盤 `test_support` の整備

### Task 5: `src/server/files/test_support.rs` を新設し、`TempWorkspace` を実装

**Files:**
- Create: `src/server/files/test_support.rs`
- Modify: `src/server/files/mod.rs`

- [ ] **Step 1: test_support.rs を新規作成（TempWorkspace 部分）**

`src/server/files/test_support.rs` を以下の内容で作成（この時点では `TempWorkspace` のみ。`MockMemoFs` と `make_test_app_state` は次タスクで追加）:

```rust
//! memo 経路テストの共通基盤。
//!
//! - [`TempWorkspace`] は tempdir + 権限戻しガード
//! - `MockMemoFs` は `(Op, PathBuf)` キーで I/O エラーを決定論的に注入する
//! - [`make_test_app_state`] はテスト用 `AppState` を組み立てる

#![cfg(test)]

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// tempdir + 権限戻しガード付きワークスペース。
///
/// `Drop` で記録された権限を逆順に戻してから tempdir を削除する。
/// `MockMemoFs` 経由のエラー注入を主な手段とするため、本来 `chmod` は使わないが、
/// 万一テスト本体が権限を変更しても tempdir 削除がブロックされないよう保険として保持する。
pub(crate) struct TempWorkspace {
    dir: tempfile::TempDir,
    permission_resets: Mutex<Vec<(PathBuf, std::fs::Permissions)>>,
}

impl TempWorkspace {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            dir: tempfile::tempdir()?,
            permission_resets: Mutex::new(Vec::new()),
        })
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// 相対パスにファイルを書き込む（必要なら親ディレクトリを作成）
    pub fn write_file(&self, rel: &Path, content: &str) -> io::Result<PathBuf> {
        let full = self.dir.path().join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full, content)?;
        Ok(full)
    }

    /// 拡張子を補わずに `.md` ファイルを書く糖衣
    pub fn write_md(&self, rel: &Path, content: &str) -> io::Result<PathBuf> {
        self.write_file(rel, content)
    }

    /// `Drop` で復元する権限を記録する（chmod を使う既存テスト互換用、将来的には未使用化を期待）
    #[cfg(unix)]
    #[allow(dead_code)]
    pub fn record_permissions(&self, path: &Path) -> io::Result<()> {
        let perms = std::fs::metadata(path)?.permissions();
        self.permission_resets
            .lock()
            .expect("permission_resets mutex poisoned")
            .push((path.to_path_buf(), perms));
        Ok(())
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        if let Ok(mut resets) = self.permission_resets.lock() {
            while let Some((path, perms)) = resets.pop() {
                let _ = std::fs::set_permissions(&path, perms);
            }
        }
    }
}
```

- [ ] **Step 2: files/mod.rs に test_support モジュールを登録**

`src/server/files/mod.rs:10-11` の `#[cfg(test)] mod tests;` の直前に `test_support` を追加:

```rust
#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
```

- [ ] **Step 3: cargo build で確認**

Run: `cargo build --tests`
Expected: ビルド成功（テスト走らせなくて OK）

- [ ] **Step 4: コミット**

```bash
git add src/server/files/test_support.rs src/server/files/mod.rs
git commit -m "test: TempWorkspaceを新設 (Phase C-1)"
```

### Task 6: `MockMemoFs` を `test_support.rs` に追加

**Files:**
- Modify: `src/server/files/test_support.rs`

- [ ] **Step 1: MockMemoFs と Op enum を追加**

`src/server/files/test_support.rs` の末尾に以下を追加:

```rust

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;

use super::memo_fs::{MemoFs, TokioMemoFs};

/// 失敗注入のキーとなる操作種別
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum Op {
    TryExists,
    Metadata,
    Read,
    CreateDirAll,
    Write,
    RemoveFile,
}

/// テスト用 `MemoFs` 実装。
///
/// デフォルトでは内部の [`TokioMemoFs`] に委譲し、実 tempdir 上で動作する。
/// `fail_at` で `(Op, PathBuf)` の組ごとに任意の `io::ErrorKind` を返すよう注入できる。
/// `writes` で書き込み履歴を取得し、暗黙移行や cleanup の挙動を検証できる。
#[derive(Debug, Default)]
pub(crate) struct MockMemoFs {
    inner: TokioMemoFs,
    failures: Mutex<HashMap<(Op, PathBuf), io::ErrorKind>>,
    write_observer: AsyncMutex<Vec<(PathBuf, Vec<u8>)>>,
}

impl MockMemoFs {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 指定 `(op, path)` で次回以降のオペレーションを `kind` エラーで失敗させる
    pub fn fail_at(&self, op: Op, path: impl Into<PathBuf>, kind: io::ErrorKind) -> &Self {
        self.failures
            .lock()
            .expect("failures mutex poisoned")
            .insert((op, path.into()), kind);
        self
    }

    /// すべての注入済み失敗を解除
    #[allow(dead_code)]
    pub fn clear_failures(&self) -> &Self {
        self.failures
            .lock()
            .expect("failures mutex poisoned")
            .clear();
        self
    }

    /// 書き込み履歴を取得（順序は呼び出し順）
    pub async fn writes(&self) -> Vec<(PathBuf, Vec<u8>)> {
        self.write_observer.lock().await.clone()
    }

    fn lookup_failure(&self, op: Op, path: &Path) -> Option<io::ErrorKind> {
        self.failures
            .lock()
            .expect("failures mutex poisoned")
            .get(&(op, path.to_path_buf()))
            .copied()
    }
}

#[async_trait]
impl MemoFs for MockMemoFs {
    async fn try_exists(&self, path: &Path) -> io::Result<bool> {
        if let Some(kind) = self.lookup_failure(Op::TryExists, path) {
            return Err(io::Error::from(kind));
        }
        self.inner.try_exists(path).await
    }

    async fn metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        if let Some(kind) = self.lookup_failure(Op::Metadata, path) {
            return Err(io::Error::from(kind));
        }
        self.inner.metadata(path).await
    }

    async fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        if let Some(kind) = self.lookup_failure(Op::Read, path) {
            return Err(io::Error::from(kind));
        }
        self.inner.read(path).await
    }

    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.lookup_failure(Op::CreateDirAll, path) {
            return Err(io::Error::from(kind));
        }
        self.inner.create_dir_all(path).await
    }

    async fn write(&self, path: &Path, content: &[u8]) -> io::Result<()> {
        if let Some(kind) = self.lookup_failure(Op::Write, path) {
            return Err(io::Error::from(kind));
        }
        self.write_observer
            .lock()
            .await
            .push((path.to_path_buf(), content.to_vec()));
        self.inner.write(path, content).await
    }

    async fn remove_file(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.lookup_failure(Op::RemoveFile, path) {
            return Err(io::Error::from(kind));
        }
        self.inner.remove_file(path).await
    }
}
```

- [ ] **Step 2: cargo build で MockMemoFs のコンパイル確認**

Run: `cargo build --tests`
Expected: ビルド成功

- [ ] **Step 3: コミット**

```bash
git add src/server/files/test_support.rs
git commit -m "test: MockMemoFsを新設 (Phase C-2)"
```

### Task 7: `make_test_app_state` ヘルパーを追加

**Files:**
- Modify: `src/server/files/test_support.rs`

- [ ] **Step 1: make_test_app_state を追加**

`src/server/files/test_support.rs` の末尾に以下を追加:

```rust

use crate::server::state::{AppMode, AppState};
use crate::server::messages::BroadcastMessage;
use tokio::sync::broadcast;

/// テスト用 `AppState` を組み立てる。
/// 既存の `create_*_state` ヘルパーは tests.rs 内に残置するが、
/// 新仕様でメモ用 `MemoFs` を差し替えるテストは本ヘルパーを経由する。
pub(crate) fn make_test_app_state(mode: AppMode, memo_fs: Arc<dyn MemoFs>) -> AppState {
    let (tx, _rx) = broadcast::channel::<BroadcastMessage>(4);
    AppState::new(mode, false, None, tx).with_memo_fs(memo_fs)
}
```

- [ ] **Step 2: cargo build で確認**

Run: `cargo build --tests`
Expected: ビルド成功

- [ ] **Step 3: コミット**

```bash
git add src/server/files/test_support.rs
git commit -m "test: make_test_app_stateヘルパーを追加 (Phase C-3)"
```

---

## Phase D: memo.rs を新契約に書き直し

このフェーズで観察可能な挙動が変わる（fallback 廃止）。各タスク完了後、既存テストは旧契約前提のため大量に失敗する想定。Phase E ですべて修正する。

### Task 8: `save_route_memo` を新契約に書き直し（4.1.1 通常 + 4.1.2 空保存）

**Files:**
- Modify: `src/server/files/memo.rs`

- [ ] **Step 1: save_route_memo を新契約に書き直す**

`src/server/files/memo.rs` の `save_route_memo`（L43-102）を以下に置換:

```rust
/// メモを保存し、保存後のプレビューHTML付き応答を返す。
///
/// 仕様: docs/superpowers/specs/2026-04-28-memo-save-contract-redesign-design.md §4.1
/// - 保存先は同階層 sidecar のみ。fallback なし。
/// - IO エラーは種別を問わず 500 として透過する。
/// - 保存成功後、compat_sidecar / legacy が存在し safe なら best-effort で削除する。
pub(in crate::server) async fn save_route_memo(
    state: &AppState,
    target: &ResolvedTarget,
    raw: String,
    request: RouteTargetRequest<'_>,
) -> Result<MemoResponse, ApiError> {
    let fs = state.memo_fs().clone();
    let fs = fs.as_ref();
    let trimmed = raw.trim();
    let memo_paths = memo_paths_for_target(state, target);

    if trimmed.is_empty() {
        return delete_route_memo(state, target, request, &memo_paths, fs).await;
    }

    if raw.len() as u64 > MAX_FILE_SIZE {
        return Err(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "メモサイズが上限（10MB）を超えています",
        ));
    }

    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;

    if let Some(parent) = memo_paths.sidecar.parent() {
        fs.create_dir_all(parent)
            .await
            .map_err(|error| io_api_error(target, request, "ディレクトリ作成", error))?;
    }
    fs.write(&memo_paths.sidecar, raw.as_bytes())
        .await
        .map_err(|error| io_api_error(target, request, "保存", error))?;

    cleanup_compat_sidecar_best_effort(state, target, request, &memo_paths, fs).await;
    cleanup_legacy_memo_best_effort(state, target, request, &memo_paths, fs).await;

    Ok(MemoResponse::from_raw(
        raw,
        target.relative_path().map(ToOwned::to_owned),
    ))
}

/// 空保存（メモ削除）契約。仕様 §4.1.2。
///
/// - sidecar は `remove_file` を試行し、`NotFound` のみ緩和、その他 IO は 500。
/// - compat_sidecar / legacy は best-effort で削除する。
async fn delete_route_memo(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) -> Result<MemoResponse, ApiError> {
    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;

    match fs.remove_file(&memo_paths.sidecar).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_api_error(target, request, "削除", error)),
    }

    cleanup_compat_sidecar_best_effort(state, target, request, memo_paths, fs).await;
    cleanup_legacy_memo_best_effort(state, target, request, memo_paths, fs).await;

    Ok(MemoResponse::empty(
        target.relative_path().map(ToOwned::to_owned),
    ))
}
```

- [ ] **Step 2: best-effort cleanup ヘルパーを追加**

`src/server/files/memo.rs` の末尾近く（`io_api_error` の前など）に以下を追加:

```rust
/// compat_sidecar を best-effort で削除する。失敗・unsafe は warn で続行。
async fn cleanup_compat_sidecar_best_effort(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) {
    let Some(compat_sidecar) = &memo_paths.compat_sidecar else {
        return;
    };
    if ensure_safe_memo_path(compat_sidecar, state, target, request).is_err() {
        tracing::warn!(
            "[markdown-view] {}unsafeな互換sidecarメモは削除せず無視します ({})",
            request.read_error_log_label(),
            target.file_label()
        );
        return;
    }
    match fs.try_exists(compat_sidecar).await {
        Ok(false) => return,
        Ok(true) => {}
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}互換sidecarメモ存在確認失敗を無視します ({}): {}",
                request.read_error_log_label(),
                target.file_label(),
                error
            );
            return;
        }
    }
    if let Err(error) = fs.remove_file(compat_sidecar).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "[markdown-view] {}互換sidecarメモcleanup失敗を無視します ({}): {}",
                request.read_error_log_label(),
                target.file_label(),
                error
            );
        }
    }
}

/// legacy memo を best-effort で削除する。失敗・unsafe は warn で続行。
async fn cleanup_legacy_memo_best_effort(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) {
    if ensure_safe_memo_path(&memo_paths.legacy, state, target, request).is_err() {
        tracing::warn!(
            "[markdown-view] {}unsafeなlegacyメモは削除せず無視します ({})",
            request.read_error_log_label(),
            target.file_label()
        );
        return;
    }
    match fs.try_exists(&memo_paths.legacy).await {
        Ok(false) => return,
        Ok(true) => {}
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}legacyメモ存在確認失敗を無視します ({}): {}",
                request.read_error_log_label(),
                target.file_label(),
                error
            );
            return;
        }
    }
    if let Err(error) = fs.remove_file(&memo_paths.legacy).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "[markdown-view] {}legacyメモcleanup失敗を無視します ({}): {}",
                request.read_error_log_label(),
                target.file_label(),
                error
            );
        }
    }
}
```

- [ ] **Step 3: cargo build を試みて未使用関数の警告を確認（旧 fallback 系はまだ削除していないため警告が出る想定）**

Run: `cargo build --all-targets 2>&1 | grep "warning:" | head -30`
Expected: 旧関数（`choose_save_target` / `save_memo_to_legacy` 等）に対する unused 警告が出る。次タスクで削除する。

### Task 10: `load_route_memo` と `resolve_active_memo_path` を新契約 (4.2) に書き直し

**Files:**
- Modify: `src/server/files/memo.rs`

- [ ] **Step 1: resolve_active_memo_path を「unsafe ならスキップして次へ」式に書き直す**

`src/server/files/memo.rs:199-223` の `resolve_active_memo_path` を以下に置換:

```rust
/// 読み込み対象のメモパスを優先順位（sidecar → compat_sidecar → legacy）で解決する。
///
/// 仕様: §4.2。unsafe path（シンボリックリンク含む）は warn のみで次優先へスキップする。
async fn resolve_active_memo_path(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) -> Result<Option<PathBuf>, ApiError> {
    if let Some(path) = pick_existing_safe_path(state, target, request, fs, &memo_paths.sidecar)
        .await?
    {
        return Ok(Some(path));
    }
    if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
        if let Some(path) =
            pick_existing_safe_path(state, target, request, fs, compat_sidecar).await?
        {
            return Ok(Some(path));
        }
    }
    if let Some(path) = pick_existing_safe_path(state, target, request, fs, &memo_paths.legacy)
        .await?
    {
        return Ok(Some(path));
    }
    Ok(None)
}

/// `path` が safe かつ `try_exists() == true` ならその `PathBuf` を返す。
/// unsafe なら warn ログを残して `None`。`try_exists` が IO エラーなら 500 を返す。
async fn pick_existing_safe_path(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    fs: &dyn MemoFs,
    path: &Path,
) -> Result<Option<PathBuf>, ApiError> {
    if let Err(error) = ensure_safe_memo_path(path, state, target, request) {
        tracing::warn!(
            "[markdown-view] {}unsafeなメモパスはスキップ ({}): {:?}",
            request.read_error_log_label(),
            target.file_label(),
            error
        );
        return Ok(None);
    }
    match fs.try_exists(path).await {
        Ok(true) => Ok(Some(path.to_path_buf())),
        Ok(false) => Ok(None),
        Err(error) => Err(io_api_error(target, request, "存在確認", error)),
    }
}
```

- [ ] **Step 2: load_route_memo を「resolve が None なら empty を返す」式に書き直す**

`src/server/files/memo.rs:18-40` の `load_route_memo` を以下に置換:

```rust
/// メモを読み込み、プレビューHTML付き応答へ変換する。
///
/// 仕様: §4.2。読み込み優先順位 sidecar → compat_sidecar → legacy。
pub(in crate::server) async fn load_route_memo(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<MemoResponse, ApiError> {
    let fs = state.memo_fs().clone();
    let fs = fs.as_ref();
    let memo_paths = memo_paths_for_target(state, target);

    let memo_path = match resolve_active_memo_path(state, target, request, &memo_paths, fs).await? {
        Some(path) => path,
        None => {
            return Ok(MemoResponse::empty(
                target.relative_path().map(ToOwned::to_owned),
            ));
        }
    };

    let raw = read_memo_file(&memo_path, target, request, fs).await?;
    Ok(MemoResponse::from_raw(
        raw,
        target.relative_path().map(ToOwned::to_owned),
    ))
}
```

- [ ] **Step 3: 旧 inspect_legacy_memo / legacy_memo_exists / compat_sidecar_exists を削除**

これらは新 `pick_existing_safe_path` で代替されたため不要。`memo.rs` 内の以下を削除:
- `fn legacy_memo_exists`（L301-309）
- `fn inspect_legacy_memo`（L311-332）
- `fn compat_sidecar_exists`（L278-299）
- `enum LegacyMemoState`（L126-131）

合わせて Task 8 リストの旧関数群（`choose_save_target` / `save_memo_to_legacy` / `save_memo_to_fallback` / `save_memo_to_existing_compat` / `save_memo_to_existing_compat_or_legacy` / `sidecar_fallback_for_error` / `is_name_too_long_error` / `enum SidecarFallback` / `struct SaveTarget` / `delete_legacy_memo_if_safe_strict` / `cleanup_legacy_memo_if_safe` / `cleanup_compat_sidecar_if_safe` / `delete_compat_sidecar_if_safe` / `delete_memo_file_if_exists`）も削除する。

`delete_memo_file_if_exists` は新コードでは使われなくなるため削除（新 `delete_route_memo` が `fs.remove_file` を直接呼ぶ）。

- [ ] **Step 4: cargo build で未使用警告がないことを確認**

Run: `cargo build --all-targets 2>&1 | grep -E "warning|error" | head -20`
Expected: `memo.rs` 由来の unused warning が出ない（既存テストの fail は別問題、コンパイル warning のみ確認）。

- [ ] **Step 5: cargo build --all-targets が通ることを確認**

Run: `cargo build --all-targets`
Expected: ビルド成功

- [ ] **Step 6: コミット**

```bash
git add src/server/files/memo.rs
git commit -m "refactor: memo保存契約を新仕様に書き直し (Phase D)"
```

注意: この時点で既存テストは旧契約前提のため多数失敗する想定。Phase E で全件修正する。

---

## Phase E: 既存テスト 16+ 件の移行

### Task 11: 削除対象 4 テストを削除

**Files:**
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: 以下のテスト関数を削除**

`src/server/files/tests.rs` から以下のテスト関数を削除する（spec 6.3 マッピング表の「削除」分）:

1. `test_save_route_memo_書込不可サブディレクトリではlegacyへfallbackする`（L1016 周辺）
2. `test_save_route_memo_旧形式backslash_sidecarは新形式作成不可なら既存compatへfallbackする`（L1314 周辺）
3. `test_save_route_memo_既存compatが書込不可なら既存legacyへfallbackする`（L1348 周辺）
4. `test_save_route_memo_単一ファイルモードでも既存legacyがあればpermission_denied時にfallbackする`（L1445 周辺）

各関数の `#[tokio::test]` または `#[test]` アトリビュートから直前の空行までを `git rm`-相当で削除。

- [ ] **Step 2: cargo build --tests が通ることを確認**

Run: `cargo build --tests`
Expected: ビルド成功

- [ ] **Step 3: コミット**

```bash
git add src/server/files/tests.rs
git commit -m "test: fallback廃止により旧契約テスト4件を削除 (Phase E-1)"
```

### Task 12: 「permission_denied 時 fallback しない」テストを「500 を返す」に転用

**Files:**
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: 既存テストを書き換える**

`test_save_route_memo_単一ファイルモードではpermission_deniedでもlegacyへfallbackしない`（L1417 周辺）を以下に置換（テスト名変更含む）。実 API（`ApiError` はタプル、`RouteTargetRequest::api_memo(None)`、`resolve_route_target`）を使用:

```rust
#[tokio::test]
async fn test_save_route_memo_単一ファイルモードでpermission_deniedなら500を返す() {
    use super::test_support::{make_test_app_state, MockMemoFs, Op};
    use std::sync::Arc;

    let workspace = super::test_support::TempWorkspace::new().unwrap();
    let file_path = workspace.write_md(Path::new("note.md"), "# note").unwrap();
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let sidecar = file_path
        .canonicalize()
        .unwrap()
        .with_file_name(".note.md.memo.md");
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    memo_fs.fail_at(
        Op::Write,
        sidecar.clone(),
        std::io::ErrorKind::PermissionDenied,
    );
    let state = make_test_app_state(mode, memo_fs.clone());

    let target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let result = save_route_memo(
        &state,
        &target,
        "メモ本文".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, _body) = result.expect_err("permission_denied で 500 になるはず");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
```

- [ ] **Step 2: テスト単体実行で pass を確認**

Run: `cargo test --all-targets test_save_route_memo_単一ファイルモードでpermission_deniedなら500を返す`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add src/server/files/tests.rs
git commit -m "test: permission_denied時500返却テストに転用 (Phase E-2)"
```

### Task 13: 「safe_legacy 削除失敗ならエラー」テストを「warn のみで 200」に挙動変更

**Files:**
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: テスト関数を書き換える**

`test_save_route_memo_空白保存でsafe_legacy削除失敗ならエラーにする`（L942 周辺）を以下に置換（テスト名変更含む）:

```rust
#[tokio::test]
async fn test_save_route_memo_空白保存_safe_legacy削除失敗は警告のみで200を返す() {
    use super::test_support::{make_test_app_state, MockMemoFs, Op};
    use std::sync::Arc;

    // 周辺維持テスト test_save_route_memo_保存成功後のlegacy削除失敗は成功扱いにする と
    // 同じファイル配置（dir モード + README.md + .markdown-view/memos/README.md）を作る
    let workspace = super::test_support::TempWorkspace::new().unwrap();
    workspace.write_md(Path::new("README.md"), "# README").unwrap();
    let canonical_dir = workspace.path().canonicalize().unwrap();
    let legacy_dir = canonical_dir.join(".markdown-view/memos");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    let legacy_path = legacy_dir.join("README.md");
    std::fs::write(&legacy_path, "old memo").unwrap();

    let mode = AppMode::new_directory(workspace.path()).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        legacy_path.clone(),
        std::io::ErrorKind::PermissionDenied,
    );
    let state = make_test_app_state(mode, memo_fs.clone());

    let target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    // 空保存（trim 後 0 byte）→ legacy 削除失敗でも 200 で返ってくる
    let memo = save_route_memo(
        &state,
        &target,
        "   ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("legacy 削除失敗でも 200 を返す（warn のみ）");

    assert_eq!(memo.raw(), "");
    // legacy ファイルは削除されずに残っている
    assert!(legacy_path.exists());
}
```

- [ ] **Step 2: テスト単体実行で pass を確認**

Run: `cargo test --all-targets test_save_route_memo_空白保存_safe_legacy削除失敗`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add src/server/files/tests.rs
git commit -m "test: safe_legacy削除失敗を警告200挙動に変更 (Phase E-3)"
```

### Task 14: chmod 依存の維持テスト群を MockMemoFs に置換

**Files:**
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: chmod を使う維持対象テストを特定**

以下の **維持** カテゴリのテストで `chmod 0o555` / `0o444` / `set_permissions` を使うものを、`MockMemoFs::fail_at(...)` 注入に置換:

1. `test_save_route_memo_保存成功後のlegacy削除失敗は成功扱いにする`（L978 周辺、`fs::set_permissions(legacy_parent, 0o555)` を `MockMemoFs::fail_at(Op::RemoveFile, legacy_path, PermissionDenied)` に置換）
2. その他、`grep -n "set_permissions" src/server/files/tests.rs` で見つかる維持対象テスト

各テスト関数の中で:
- 既存の `let original_mode = fs::metadata(...).permissions().mode();` ブロックを削除
- 既存の `fs::set_permissions(..., Permissions::from_mode(0o555))` を `memo_fs.fail_at(Op::Write/RemoveFile, target_path, PermissionDenied)` に置換
- 既存の AppState 構築（`create_directory_state` / `create_single_file_state`）を `make_test_app_state(mode, memo_fs.clone())` に置換
- 既存の `fs::set_permissions(..., Permissions::from_mode(original_mode))` を削除（権限を変えていないので戻し不要）

- [ ] **Step 2: 各テストを書き換えた後、cargo test で個別 pass 確認**

Run: `cargo test --all-targets test_save_route_memo_保存成功後のlegacy削除失敗`
Expected: PASS

`grep -n "set_permissions\|chmod" src/server/files/tests.rs` で残っている `set_permissions` 呼び出しを確認し、すべて消えていれば次へ。

- [ ] **Step 3: 全 memo 関連テストを実行**

Run: `cargo test --all-targets memo`
Expected: 維持対象テスト全件 pass。削除対象は既に削除済み、転用対象は Task 12-13 で対応済み。

- [ ] **Step 4: コミット**

```bash
git add src/server/files/tests.rs
git commit -m "test: chmod依存をMockMemoFs注入に置換 (Phase E-4)"
```

### Task 15: 全 memo テスト pass を verify

**Files:**
- N/A（検証のみ）

- [ ] **Step 1: cargo test --all-targets を実行**

Run: `cargo test --all-targets --all-features`
Expected: 全テスト pass。memo 関連の旧契約テストはすべて移行/削除済みのため失敗なし。

失敗があれば Task 11-14 を見直す。

- [ ] **Step 2: chmod / set_permissions が tests.rs から完全に消えていることを確認**

Run: `grep -n "set_permissions\|chmod" src/server/files/tests.rs`
Expected: マッチなし、または `record_permissions`（test_support.rs 内部用）のみ。

---

## Phase F: 新規テスト追加（spec §6.4 全 9 件）

### Task 16: 保存系新規テスト 6 件を追加

**Files:**
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: 6 件のテストを追加**

`src/server/files/tests.rs` の memo 関連テスト群の末尾（`test_load_route_memo_*` 群の直前）に以下の 6 件を追加。各テストは `MockMemoFs` で I/O エラーを注入する。

```rust
#[tokio::test]
async fn test_save_route_memo_sidecar書込不可で500を返す() {
    use super::test_support::{make_test_app_state, MockMemoFs, Op};
    use std::sync::Arc;

    let workspace = super::test_support::TempWorkspace::new().unwrap();
    let file_path = workspace.write_md(Path::new("note.md"), "# note").unwrap();
    let canonical = file_path.canonicalize().unwrap();
    let sidecar = canonical.with_file_name(".note.md.memo.md");
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    memo_fs.fail_at(Op::Write, sidecar, std::io::ErrorKind::PermissionDenied);
    let state = make_test_app_state(mode, memo_fs.clone());

    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let result = save_route_memo(
        &state,
        &target,
        "本文".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, _body) = result.expect_err("write 失敗で 500");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_save_route_memo_create_dir_all失敗で500を返す() {
    use super::test_support::{make_test_app_state, MockMemoFs, Op};
    use std::sync::Arc;

    let workspace = super::test_support::TempWorkspace::new().unwrap();
    let file_path = workspace.write_md(Path::new("note.md"), "# note").unwrap();
    let canonical = file_path.canonicalize().unwrap();
    let parent = canonical.parent().unwrap().to_path_buf();
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    memo_fs.fail_at(Op::CreateDirAll, parent, std::io::ErrorKind::PermissionDenied);
    let state = make_test_app_state(mode, memo_fs.clone());

    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let result = save_route_memo(
        &state,
        &target,
        "本文".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, _body) = result.expect_err("create_dir_all 失敗で 500");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_save_route_memo_disk_full系IO失敗で500を返す() {
    use super::test_support::{make_test_app_state, MockMemoFs, Op};
    use std::sync::Arc;

    let workspace = super::test_support::TempWorkspace::new().unwrap();
    let file_path = workspace.write_md(Path::new("note.md"), "# note").unwrap();
    let canonical = file_path.canonicalize().unwrap();
    let sidecar = canonical.with_file_name(".note.md.memo.md");
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    memo_fs.fail_at(Op::Write, sidecar, std::io::ErrorKind::Other);
    let state = make_test_app_state(mode, memo_fs.clone());

    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let result = save_route_memo(
        &state,
        &target,
        "本文".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, _body) = result.expect_err("Other IO エラーでも fallback せず 500");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_保存成功後のcompat削除失敗は200を返す() {
    use super::test_support::{make_test_app_state, MockMemoFs, Op};
    use std::sync::Arc;

    // 周辺テスト test_save_route_memo_旧形式backslash_sidecarを新形式へ移行する と
    // 同じファイル配置: 'a\\b.md' + '.a\\b.md.memo.md' (compat_sidecar)
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("a\\b.md");
    let compat_sidecar = dir.path().join(".a\\b.md.memo.md");
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    std::fs::write(&file_path, "# title").unwrap();
    std::fs::write(&compat_sidecar, "compat content").unwrap();

    let mode = AppMode::new_single_file(&file_path).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        compat_sidecar.clone(),
        std::io::ErrorKind::PermissionDenied,
    );
    let state = make_test_app_state(mode, memo_fs.clone());

    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let memo = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("compat 削除失敗でも 200 を返す（warn のみ）");

    assert_eq!(memo.raw(), "new memo");
    // 新 sidecar には書かれている
    assert_eq!(std::fs::read_to_string(&new_sidecar).unwrap(), "new memo");
    // compat_sidecar は削除失敗で残っている
    assert!(compat_sidecar.exists());
}

#[tokio::test]
async fn test_save_route_memo_空保存_sidecarが既にない場合は冪等的に200を返す() {
    use super::test_support::{make_test_app_state, MockMemoFs};
    use std::sync::Arc;

    let workspace = super::test_support::TempWorkspace::new().unwrap();
    let file_path = workspace.write_md(Path::new("note.md"), "# note").unwrap();
    let canonical = file_path.canonicalize().unwrap();
    let sidecar = canonical.with_file_name(".note.md.memo.md");
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    let state = make_test_app_state(mode, memo_fs.clone());

    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let memo = save_route_memo(
        &state,
        &target,
        "   ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("sidecar 不在でも 200 (NotFound 緩和)");

    assert_eq!(memo.raw(), "");
    assert!(!sidecar.exists());
}

#[tokio::test]
async fn test_save_route_memo_空保存_sidecar削除失敗は500を返す() {
    use super::test_support::{make_test_app_state, MockMemoFs, Op};
    use std::sync::Arc;

    let workspace = super::test_support::TempWorkspace::new().unwrap();
    let file_path = workspace.write_md(Path::new("note.md"), "# note").unwrap();
    let canonical = file_path.canonicalize().unwrap();
    let sidecar = canonical.with_file_name(".note.md.memo.md");
    std::fs::write(&sidecar, "old").unwrap();
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    memo_fs.fail_at(
        Op::RemoveFile,
        sidecar.clone(),
        std::io::ErrorKind::PermissionDenied,
    );
    let state = make_test_app_state(mode, memo_fs.clone());

    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let result = save_route_memo(
        &state,
        &target,
        String::new(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, _body) = result.expect_err("sidecar 削除失敗で 500");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
```

注意: 各テストの `sidecar` パスは `file_path.canonicalize()` 後に `with_file_name` するのが安全（`.note.md.memo.md` のような hidden file 系 sidecar 名は `SidecarMemoName::from_file_name` で生成された名前と一致する）。tempdir のシンボリックリンク解決有無を吸収するため必ず canonical 経由で組み立てること。

- [ ] **Step 2: 各テストを cargo test で個別 pass 確認**

Run: `cargo test --all-targets test_save_route_memo_sidecar書込不可で500を返す`
Expected: PASS

Run: `cargo test --all-targets test_save_route_memo_create_dir_all失敗で500を返す`
Expected: PASS

Run: `cargo test --all-targets test_save_route_memo_disk_full系IO失敗で500を返す`
Expected: PASS

Run: `cargo test --all-targets test_save_route_memo_保存成功後のcompat削除失敗は200を返す`
Expected: PASS

Run: `cargo test --all-targets test_save_route_memo_空保存_sidecarが既にない場合は冪等的に200を返す`
Expected: PASS

Run: `cargo test --all-targets test_save_route_memo_空保存_sidecar削除失敗は500を返す`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add src/server/files/tests.rs
git commit -m "test: 保存契約の新規テスト6件を追加 (Phase F-1)"
```

### Task 17: 読み込み系新規テスト 3 件を追加

**Files:**
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: 3 件のテストを追加**

`src/server/files/tests.rs` の `test_load_route_memo_*` 群の末尾に以下を追加:

```rust
#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_sidecar優先_compat_legacy両方存在しても新sidecarを返す() {
    use super::test_support::{make_test_app_state, MockMemoFs};
    use std::sync::Arc;

    // ディレクトリモード + 'a\\b.md' という backslash 名で
    // sidecar / compat_sidecar / legacy の 3 つを並存させる
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a\\b.md"), "# title").unwrap();
    let new_sidecar_name = SidecarMemoName::from_file_name(std::ffi::OsStr::new("a\\b.md"));
    let new_sidecar = dir.path().join(new_sidecar_name.as_str());
    std::fs::write(&new_sidecar, "new sidecar").unwrap();
    std::fs::write(dir.path().join(".a\\b.md.memo.md"), "compat memo").unwrap();
    std::fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    std::fs::write(
        dir.path().join(".markdown-view/memos/a\\b.md"),
        "legacy memo",
    )
    .unwrap();

    let mode = AppMode::new_directory(dir.path()).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    let state = make_test_app_state(mode, memo_fs.clone());

    let target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("a\\b.md"))).unwrap();
    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(Some("a\\b.md")))
        .await
        .expect("sidecar から読み込み成功");

    assert_eq!(memo.raw(), "new sidecar");
}

#[cfg(unix)]
#[tokio::test]
async fn test_load_route_memo_compat優先_legacy存在でも新compatを返す() {
    use super::test_support::{make_test_app_state, MockMemoFs};
    use std::sync::Arc;

    // sidecar 不在、compat_sidecar と legacy のみ存在
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a\\b.md"), "# title").unwrap();
    std::fs::write(dir.path().join(".a\\b.md.memo.md"), "compat content").unwrap();
    std::fs::create_dir_all(dir.path().join(".markdown-view/memos")).unwrap();
    std::fs::write(
        dir.path().join(".markdown-view/memos/a\\b.md"),
        "legacy memo",
    )
    .unwrap();

    let mode = AppMode::new_directory(dir.path()).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    let state = make_test_app_state(mode, memo_fs.clone());

    let target =
        resolve_route_target(&state, RouteTargetRequest::api_memo(Some("a\\b.md"))).unwrap();
    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(Some("a\\b.md")))
        .await
        .expect("compat_sidecar から読み込み成功");

    assert_eq!(memo.raw(), "compat content");
}

#[tokio::test]
async fn test_load_route_memo_全て不在なら空メモ() {
    use super::test_support::{make_test_app_state, MockMemoFs};
    use std::sync::Arc;

    let workspace = super::test_support::TempWorkspace::new().unwrap();
    let file_path = workspace.write_md(Path::new("note.md"), "# note").unwrap();
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let memo_fs: Arc<MockMemoFs> = MockMemoFs::new();
    let state = make_test_app_state(mode, memo_fs.clone());

    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();
    let memo = load_route_memo(&state, &target, RouteTargetRequest::api_memo(None))
        .await
        .expect("メモ未作成でも 200");

    assert_eq!(memo.raw(), "");
}
```

注意: `RouteTargetRequest::api_memo(Some("a\\b.md"))` のように相対パスを渡せるかは既存テスト `test_load_route_memo_旧形式backslash_sidecarを読み込む`（L1268）と同じパターンで動作する。実装時に compile error が出る場合は周辺テストの呼び方を確認する。

- [ ] **Step 2: 各テストを cargo test で個別 pass 確認**

Run: `cargo test --all-targets test_load_route_memo_sidecar優先`
Expected: PASS

Run: `cargo test --all-targets test_load_route_memo_compat優先`
Expected: PASS

Run: `cargo test --all-targets test_load_route_memo_全て不在`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add src/server/files/tests.rs
git commit -m "test: 読み込み契約の新規テスト3件を追加 (Phase F-2)"
```

---

## Phase G: 検証と TODO 更新

### Task 18: 全テスト & verify.sh 実行（root / non-root 両方）

**Files:**
- N/A（検証のみ）

- [ ] **Step 1: 全 cargo test を実行**

Run: `cargo test --all-targets --all-features`
Expected: 全件 PASS、failed 0

- [ ] **Step 2: verify.sh を実行**

Run: `./verify.sh`
Expected: 全段階（fmt / clippy / test / typecheck）PASS

- [ ] **Step 3: chmod / set_permissions が tests.rs から消えていることを最終確認**

Run: `grep -n "set_permissions\|chmod 0" src/server/files/tests.rs`
Expected: マッチなし

- [ ] **Step 4: 旧 API が完全に削除されていることを確認**

Run: `grep -nE "SidecarFallback|SaveTarget|choose_save_target|save_memo_to_fallback|save_memo_to_existing_compat|save_memo_to_legacy|sidecar_fallback_for_error|is_name_too_long_error|delete_legacy_memo_if_safe_strict|cleanup_legacy_memo_if_safe|cleanup_compat_sidecar_if_safe" src/server/files/memo.rs`
Expected: マッチなし

- [ ] **Step 5: root 環境で再実行（任意）**

Run: `sudo -E cargo test --all-targets --all-features`
Expected: 全件 PASS（chmod 依存がなくなったので root でも non-root と同一結果）

### Task 19: TODO.md / 監査プランの更新

**Files:**
- Modify: `docs/todo/TODO.md`
- Modify: `docs/superpowers/plans/2026-04-25-codebase-audit-task-proposals.md`

- [ ] **Step 1: TODO.md の High-1 / High-2 / Process-1 / Process-3 を完了マーク**

`docs/todo/TODO.md` の以下を `- [ ]` から `- [x]` に変更し、完了済み項目はファイル末尾の「完了済み項目は git history および ...」コメントに従って削除（または DONE-2026-04-28.md へ移動）:

- High-1: 「memo 保存の permission/fallback 契約を再定義し、実装・テストを一致させる」
- High-2: 「`test_save_route_memo_*` の環境依存を排除（root 実行でも安定させる）」
- Process-1: 「memo 保存仕様の設計ノート作成（2〜3 ページ）」
- Process-3 のうち memo 経路分（残作業: watcher 系の共通化）。Process-3 そのものは「memo 経路完了、watcher 系は別 spec で扱う」と注記して `- [ ]` のまま残す。

- [ ] **Step 2: 監査プランの High-1 / High-2 を完了に更新**

`docs/superpowers/plans/2026-04-25-codebase-audit-task-proposals.md` の以下を更新:
- 優先度 High セクションの「memo 保存の permission/fallback 契約を再定義」「`test_save_route_memo_*` の環境依存を排除」を `- [x]` に変更
- 末尾に「完了 spec: docs/superpowers/specs/2026-04-28-memo-save-contract-redesign-design.md」と注記を追加

- [ ] **Step 3: 改めて verify.sh で markdown 等の検証**

Run: `./verify.sh`
Expected: 全段階 PASS（ドキュメントだけの変更でも壊れていないこと）

- [ ] **Step 4: コミット**

```bash
git add docs/todo/TODO.md docs/superpowers/plans/2026-04-25-codebase-audit-task-proposals.md
git commit -m "docs: TODO/監査プランから完了項目をマーク (Phase G)"
```

### Task 20: PR 準備

**Files:**
- N/A（git 操作のみ）

- [ ] **Step 1: ブランチ確認とリベース**

Run: `git log --oneline develop..HEAD`
Expected: Phase A〜G に対応する複数のコミットが並ぶ

- [ ] **Step 2: PR 用の本文を準備**

PR タイトル: `feat: memo保存契約をsidecar一本化方針に再設計しテスト基盤をDI化`
PR 本文には以下を含める:
- spec へのリンク
- 主な変更（fallback 廃止、`trait MemoFs` 導入、chmod 依存テスト全廃）
- 互換性破壊（書込不可ディレクトリ時 500）
- テスト追加件数
- TODO 完了項目

- [ ] **Step 3: ユーザーに `gh pr create` 実行可否を確認**

ユーザーに確認後、`gh pr create --base develop --title "..." --body "..."` を実行する。

---

## 完了条件チェックリスト

spec §9 の完了条件:

- [ ] `cargo test --all-targets --all-features` が non-root / root 両方で同一結果（chmod 依存テスト排除済み）
- [ ] `./verify.sh` 通過
- [ ] spec 6.3 マッピング表のとおりテストが書き換わっている（維持・転用・削除）
- [ ] spec 6.4 新規追加テスト一覧の全件が pass
- [ ] `SidecarFallback` enum / `SaveTarget` struct / `choose_save_target` / `save_memo_to_fallback` / `save_memo_to_existing_compat*` / `save_memo_to_legacy` / `sidecar_fallback_for_error` / `is_name_too_long_error` がコードベースから消えている
- [ ] `TODO.md` の High-1 / High-2 / Process-1 / Process-3 (memo 経路分) が完了マーク
