# Memo Atomic Save And Delete Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** メモの非空保存を `tmp + rename` で原子化し、空白保存削除では primary sidecar を最後に削除する契約へ変更する。

**Architecture:** `MemoFs` に `write_atomic` を追加し、本番 `TokioMemoFs` とテスト用 `MockMemoFs` の両方で同じ保存契約を表現する。`save_route_memo` は保存前と rename 直前の安全性検証を行い、空白保存は compat / legacy cleanup 後に primary sidecar を削除する。

**Tech Stack:** Rust 2021、tokio fs、async-trait、sha2、axum HTTP error mapping、既存 `cargo test` / `./verify.sh`。

---

## File Structure

- Modify: `src/server/files/memo_fs.rs`
  - `MemoFs::write_atomic`、`MemoWriteError`、rename 前検証 callback、tmp path 生成、`TokioMemoFs` の tmp + rename 実装を追加する。
- Modify: `src/server/files/memo.rs`
  - 非空保存を `write_atomic` に切り替える。
  - rename 直前検証 callback を作る。
  - `MemoWriteError` を既存 `ApiError` に変換する。
  - 空白保存削除順序を compat / legacy first、primary last に変更する。
- Modify: `src/server/files/test_support.rs`
  - `Op::WriteAtomic` / `Op::AtomicRename` を追加する。
  - atomic write 履歴と remove 順序を観測できるようにする。
  - `MockMemoFs::write_atomic` を本番契約に合わせる。
- Modify: `src/server/files/tests.rs`
  - 原子保存、rename 直前検証、tmp cleanup、削除順序のテストを追加・既存テストを更新する。
- Modify: `tests/integration_test.rs`
  - HTTP PUT 成功後に tmp 残骸がないことを API 境界で確認する。
- Modify: `docs/todo/TODO.md`
  - 実装完了後に対象 High Priority 2 件を完了済みに移す。

## Task 1: `MemoFs` に atomic write 契約を追加

**Files:**
- Modify: `src/server/files/memo_fs.rs`

- [ ] **Step 1: failing compile test 相当として trait 呼び出し先を先に想定する**

この task は trait API 追加から始める。以降の task で `save_route_memo` から呼ぶため、まず `memo_fs.rs` に型と method を入れて compile 可能にする。

- [ ] **Step 2: `memo_fs.rs` の import を拡張する**

`src/server/files/memo_fs.rs` 冒頭の import を次の形にする。

```rust
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
```

- [ ] **Step 3: atomic write 用の error 型と callback 型を追加する**

`MemoReadError` の直後に次を追加する。

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemoBeforeRenameError {
    user_message: String,
}

impl MemoBeforeRenameError {
    pub(crate) fn new(user_message: impl Into<String>) -> Self {
        Self {
            user_message: user_message.into(),
        }
    }

    pub(crate) fn user_message(&self) -> &str {
        &self.user_message
    }
}

#[derive(Debug)]
pub(crate) enum MemoWriteError {
    Io(io::Error),
    BeforeRename(MemoBeforeRenameError),
}

impl From<io::Error> for MemoWriteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<MemoBeforeRenameError> for MemoWriteError {
    fn from(error: MemoBeforeRenameError) -> Self {
        Self::BeforeRename(error)
    }
}

pub(crate) type BeforeRenameCheck =
    dyn Fn(&Path, &Path) -> Result<(), MemoBeforeRenameError> + Send + Sync;

static ATOMIC_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
const ATOMIC_TMP_ATTEMPTS: u8 = 8;
```

- [ ] **Step 4: `MemoFs` trait に `write_atomic` を追加する**

既存の `write` method はテスト helper の内部利用が残るため維持し、その直後に次を追加する。

```rust
    /// バイト列を同一ディレクトリ内 tmp へ書き込み、rename で最終パスへ差し替える。
    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_rename: &BeforeRenameCheck,
    ) -> Result<(), MemoWriteError>;
```

- [ ] **Step 5: tmp path 生成 helper を追加する**

`TokioMemoFs` の `impl MemoFs` より前に次を追加する。

```rust
fn atomic_tmp_path(path: &Path, attempt: u8) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "memo path must have a parent directory",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "memo path must have a file name")
    })?;

    let counter = ATOMIC_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let suffix = format!(".tmp.{}.{}.{}", std::process::id(), counter, attempt);
    let base = file_name.to_string_lossy();
    let mut tmp_name = format!("{base}{suffix}");

    if tmp_name.len() > 255 {
        let mut hasher = Sha256::new();
        hasher.update(file_name.as_encoded_bytes());
        hasher.update(counter.to_le_bytes());
        hasher.update([attempt]);
        let hash = format!("{:x}", hasher.finalize());
        tmp_name = format!(".memo.{}.tmp", &hash[..32]);
    }

    Ok(parent.join(tmp_name))
}

async fn sync_parent_dir_best_effort(parent: &Path) {
    let file = match tokio::fs::OpenOptions::new()
        .read(true)
        .open(parent)
        .await
    {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] メモ保存後の親ディレクトリopenに失敗しました: {}",
                error
            );
            return;
        }
    };

    if let Err(error) = file.sync_all().await {
        tracing::warn!(
            "[markdown-view] メモ保存後の親ディレクトリsyncに失敗しました: {}",
            error
        );
    }
}

async fn cleanup_tmp_best_effort(tmp_path: &Path) {
    match tokio::fs::remove_file(tmp_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(
                "[markdown-view] メモ一時ファイルcleanup失敗を無視します ({}): {}",
                tmp_path.display(),
                error
            );
        }
    }
}
```

- [ ] **Step 6: `TokioMemoFs::write_atomic` を実装する**

`impl MemoFs for TokioMemoFs` 内に次を追加する。

```rust
    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_rename: &BeforeRenameCheck,
    ) -> Result<(), MemoWriteError> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "memo path must have a parent directory",
            )
        })?;

        let mut last_already_exists = None;
        for attempt in 0..ATOMIC_TMP_ATTEMPTS {
            let tmp_path = atomic_tmp_path(path, attempt)?;
            let mut tmp_file = match tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)
                .await
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    last_already_exists = Some(error);
                    continue;
                }
                Err(error) => return Err(MemoWriteError::Io(error)),
            };

            if let Err(error) = tmp_file.write_all(content).await {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::Io(error));
            }
            if let Err(error) = tmp_file.flush().await {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::Io(error));
            }
            if let Err(error) = tmp_file.sync_data().await {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::Io(error));
            }
            drop(tmp_file);

            if let Err(error) = before_rename(path, &tmp_path) {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::BeforeRename(error));
            }

            if let Err(error) = tokio::fs::rename(&tmp_path, path).await {
                cleanup_tmp_best_effort(&tmp_path).await;
                return Err(MemoWriteError::Io(error));
            }

            sync_parent_dir_best_effort(parent).await;
            return Ok(());
        }

        Err(MemoWriteError::Io(last_already_exists.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "memo temporary file already exists",
            )
        })))
    }
```

- [ ] **Step 7: compile して trait 追加の未実装箇所を確認する**

Run:

```bash
cargo test --lib
```

Expected: FAIL。`MockMemoFs` が `write_atomic` を実装していない、または未使用 import 警告が出る。

- [ ] **Step 8: Commit Task 1**

この task はまだ compile が赤の可能性があるため、commit は Task 2 で mock 実装まで入れてから行う。

## Task 2: `MockMemoFs` に atomic write と操作履歴を追加

**Files:**
- Modify: `src/server/files/test_support.rs`
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: `test_support.rs` の import を更新する**

`use super::memo_fs` 行を次に変更する。

```rust
use super::memo_fs::{
    BeforeRenameCheck, MemoFs, MemoReadError, MemoWriteError, TokioMemoFs,
};
```

- [ ] **Step 2: `Op` に atomic 用 variant を追加する**

`Op` enum を次に変更する。

```rust
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum Op {
    TryExists,
    Metadata,
    Read,
    CreateDirAll,
    Write,
    WriteAtomic,
    AtomicRename,
    RemoveFile,
}
```

- [ ] **Step 3: 操作履歴型と `MockMemoFs` フィールドを追加する**

`Op` enum の直後に次を追加する。

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpEvent {
    WriteAtomic(PathBuf),
    AtomicRename(PathBuf),
    RemoveFile(PathBuf),
}
```

`MockMemoFs` のフィールドを次に変更する。

```rust
#[derive(Debug, Default)]
pub(crate) struct MockMemoFs {
    inner: TokioMemoFs,
    failures: Mutex<HashMap<(Op, PathBuf), io::ErrorKind>>,
    write_observer: AsyncMutex<Vec<(PathBuf, Vec<u8>)>>,
    atomic_write_observer: AsyncMutex<Vec<(PathBuf, Vec<u8>)>>,
    operations: AsyncMutex<Vec<OpEvent>>,
}
```

- [ ] **Step 4: `MockMemoFs` に観測 helper を追加する**

`writes()` の直後に次を追加する。

```rust
    pub async fn atomic_writes(&self) -> Vec<(PathBuf, Vec<u8>)> {
        self.atomic_write_observer.lock().await.clone()
    }

    pub async fn operations(&self) -> Vec<OpEvent> {
        self.operations.lock().await.clone()
    }
```

- [ ] **Step 5: `MockMemoFs::write_atomic` を実装する**

`impl MemoFs for MockMemoFs` 内、`write` の後に次を追加する。

```rust
    async fn write_atomic(
        &self,
        path: &Path,
        content: &[u8],
        before_rename: &BeforeRenameCheck,
    ) -> Result<(), MemoWriteError> {
        if let Some(kind) = self.lookup_failure(Op::WriteAtomic, path) {
            return Err(MemoWriteError::Io(io::Error::from(kind)));
        }

        let tmp_path = path.with_extension("memo-atomic-test-tmp");
        before_rename(path, &tmp_path).map_err(MemoWriteError::BeforeRename)?;

        if let Some(kind) = self.lookup_failure(Op::AtomicRename, path) {
            return Err(MemoWriteError::Io(io::Error::from(kind)));
        }

        self.operations
            .lock()
            .await
            .push(OpEvent::WriteAtomic(path.to_path_buf()));
        self.atomic_write_observer
            .lock()
            .await
            .push((path.to_path_buf(), content.to_vec()));
        self.operations
            .lock()
            .await
            .push(OpEvent::AtomicRename(path.to_path_buf()));
        self.inner
            .write_atomic(path, content, &|_, _| Ok(()))
            .await
            .map_err(|error| match error {
                MemoWriteError::Io(error) => MemoWriteError::Io(error),
                MemoWriteError::BeforeRename(error) => MemoWriteError::BeforeRename(error),
            })
    }
```

- [ ] **Step 6: `remove_file` で削除順序を記録する**

`MockMemoFs::remove_file` を次に変更する。

```rust
    async fn remove_file(&self, path: &Path) -> io::Result<()> {
        if let Some(kind) = self.lookup_failure(Op::RemoveFile, path) {
            return Err(io::Error::from(kind));
        }

        self.operations
            .lock()
            .await
            .push(OpEvent::RemoveFile(path.to_path_buf()));
        self.inner.remove_file(path).await
    }
```

- [ ] **Step 7: compile を確認する**

Run:

```bash
cargo test --lib
```

Expected: PASS、または `save_route_memo` がまだ `write` を呼ぶため atomic 関連の未使用警告が出る。警告が出る場合は Task 3 で解消する。

- [ ] **Step 8: Commit Task 1-2**

```bash
git add src/server/files/memo_fs.rs src/server/files/test_support.rs
git commit -m "refactor: メモFSに原子保存APIを追加"
```

## Task 3: 非空保存を `write_atomic` に切り替える

**Files:**
- Modify: `src/server/files/memo.rs`
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: `memo.rs` の import を更新する**

`use super::memo_fs` 行を次に変更する。

```rust
use super::memo_fs::{
    BeforeRenameCheck, MemoBeforeRenameError, MemoFs, MemoReadError, MemoWriteError,
};
```

- [ ] **Step 2: rename 前検証 helper を追加する**

`ensure_safe_memo_path` の直後に次を追加する。

```rust
fn ensure_safe_memo_rename_paths(
    final_path: &Path,
    tmp_path: &Path,
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
) -> Result<(), MemoBeforeRenameError> {
    let base_dir = state.mode().base_dir();
    if final_path.parent() != tmp_path.parent() {
        tracing::warn!(
            "[markdown-view] {}メモ一時ファイルが保存先と同一ディレクトリではないため拒否 ({}): {} -> {}",
            request.read_error_log_label(),
            target.file_label(),
            sanitize_path_for_logging(final_path, base_dir),
            sanitize_path_for_logging(tmp_path, base_dir)
        );
        return Err(MemoBeforeRenameError::new(
            "メモ保存先にシンボリックリンクが含まれているため操作できません",
        ));
    }

    ensure_safe_memo_path(final_path, state, target, request).map_err(|_| {
        MemoBeforeRenameError::new(
            "メモ保存先にシンボリックリンクが含まれているため操作できません",
        )
    })?;
    ensure_safe_memo_path(tmp_path, state, target, request).map_err(|_| {
        MemoBeforeRenameError::new(
            "メモ保存先にシンボリックリンクが含まれているため操作できません",
        )
    })?;
    Ok(())
}
```

- [ ] **Step 3: `save_route_memo` の write 呼び出しを置き換える**

`fs.write(memo_path, raw.as_bytes())...` ブロックを次に置き換える。

```rust
    let before_rename = |final_path: &Path, tmp_path: &Path| {
        ensure_safe_memo_rename_paths(final_path, tmp_path, state, target, request)
    };
    fs.write_atomic(
        memo_path,
        raw.as_bytes(),
        &before_rename as &BeforeRenameCheck,
    )
    .await
    .map_err(|error| memo_write_error_to_api_error(target, request, error))?;
```

- [ ] **Step 4: write error 変換 helper を追加する**

`io_api_error` の前に次を追加する。

```rust
fn memo_write_error_to_api_error(
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    error: MemoWriteError,
) -> ApiError {
    match error {
        MemoWriteError::Io(error) => io_api_error(target, request, "保存", error),
        MemoWriteError::BeforeRename(error) => {
            tracing::warn!(
                "[markdown-view] {}メモrename直前検証エラー ({}): {}",
                request.read_error_log_label(),
                target.file_label(),
                error.user_message()
            );
            json_error(StatusCode::FORBIDDEN, error.user_message())
        }
    }
}
```

- [ ] **Step 5: 既存の write 履歴テストを atomic write 履歴へ更新する**

`src/server/files/tests.rs` で `memo_fs.writes().await` を使っている保存成功系 assertion を `memo_fs.atomic_writes().await` に変更する。

例として `test_save_route_memo_保存成功後のcompat削除失敗は200を返す` の assertion を次にする。

```rust
    assert_eq!(
        memo_fs.atomic_writes().await,
        vec![(new_sidecar_path.clone(), b"new memo".to_vec())]
    );
```

- [ ] **Step 6: `Op::Write` failure injection を `Op::WriteAtomic` に更新する**

`src/server/files/tests.rs` の保存失敗系で `memo_fs.fail_at(Op::Write, ...)` を使っている箇所を `Op::WriteAtomic` に変更する。

代表例:

```rust
    memo_fs.fail_at(
        Op::WriteAtomic,
        &sidecar_path,
        std::io::ErrorKind::PermissionDenied,
    );
```

- [ ] **Step 7: targeted tests を実行する**

Run:

```bash
cargo test save_route_memo --lib
```

Expected: PASS。

- [ ] **Step 8: Commit Task 3**

```bash
git add src/server/files/memo.rs src/server/files/tests.rs
git commit -m "fix: メモ保存を原子書き込みに切り替え"
```

## Task 4: 原子保存の回帰テストを追加する

**Files:**
- Modify: `src/server/files/tests.rs`
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: atomic rename 失敗時に旧内容が残る unit test を追加する**

`test_save_route_memo_sidecar書込不可で500を返す` の後に次を追加する。

```rust
#[tokio::test]
async fn test_save_route_memo_atomic_rename失敗時は既存メモを保持する() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "old memo")
        .expect("sidecar memo should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(Op::AtomicRename, &sidecar_path, std::io::ErrorKind::Other);
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("rename failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(json["error"], "メモファイルの操作に失敗しました");
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), "old memo");
}
```

- [ ] **Step 2: atomic write 失敗時に旧内容が残る unit test を追加する**

同じ近辺に次を追加する。

```rust
#[tokio::test]
async fn test_save_route_memo_atomic_write失敗時は既存メモを保持する() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "old memo")
        .expect("sidecar memo should be written");

    let memo_fs = MockMemoFs::new();
    memo_fs.fail_at(Op::WriteAtomic, &sidecar_path, std::io::ErrorKind::Other);
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs);
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let result = save_route_memo(
        &state,
        &target,
        "new memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, _body) = result.expect_err("atomic write failure should be fatal");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), "old memo");
}
```

- [ ] **Step 3: rename 直前 symlink 差し替え unit test を追加する**

Unix-only で、`test_save_route_memo_新メモファイルがシンボリックリンクなら拒否する` の後に次を追加する。

```rust
#[cfg(unix)]
#[tokio::test]
async fn test_save_route_memo_rename直前にsidecarがsymlinkなら拒否する() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace.path().join(".note.md.memo.md");
    let outside_dir = tempfile::tempdir().unwrap();
    fs::write(outside_dir.path().join("memo.md"), "outside").unwrap();

    let memo_fs = MockMemoFs::new();
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs.clone());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    symlink(outside_dir.path().join("memo.md"), &sidecar_path).unwrap();
    let result = save_route_memo(
        &state,
        &target,
        "memo".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await;

    let (status, body) = result.expect_err("rename precheck should reject symlink");
    assert_eq!(status, StatusCode::FORBIDDEN);
    let json = serde_json::to_value(body.0).unwrap();
    assert_eq!(
        json["error"],
        "メモ保存先にシンボリックリンクが含まれているため操作できません"
    );
    assert!(memo_fs.atomic_writes().await.is_empty());
}
```

- [ ] **Step 4: HTTP PUT 成功後に tmp が残らない integration test を追加する**

`tests/integration_test.rs` の `test_apiメモ_保存と再取得ができる` の後に次を追加する。

```rust
#[tokio::test]
async fn test_apiメモ_保存成功後にtmpファイルが残らない() {
    let (_state, addr, tmp_dir) = setup_single_file_server("# Memo\n\nBody").await;
    let client = reqwest::Client::new();

    let save = client
        .put(format!("http://{}/api/memo", addr))
        .json(&serde_json::json!({
            "raw": "atomic memo"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(save.status(), 200);
    assert_eq!(
        tokio::fs::read_to_string(tmp_dir.path().join(".test.md.memo.md"))
            .await
            .unwrap(),
        "atomic memo"
    );

    let mut entries = tokio::fs::read_dir(tmp_dir.path()).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.contains(".tmp."),
            "atomic temp file should be cleaned up: {name}"
        );
    }
}
```

- [ ] **Step 5: targeted tests を実行する**

Run:

```bash
cargo test atomic --lib
cargo test test_apiメモ_保存成功後にtmpファイルが残らない --test integration_test
```

Expected: PASS。

- [ ] **Step 6: Commit Task 4**

```bash
git add src/server/files/tests.rs tests/integration_test.rs
git commit -m "test: メモ原子保存の回帰テストを追加"
```

## Task 5: 空白保存削除を primary last に変更する

**Files:**
- Modify: `src/server/files/memo.rs`
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: `delete_route_memo` を primary last に書き換える**

`delete_route_memo` を次に置き換える。

```rust
async fn delete_route_memo(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
    fs: &dyn MemoFs,
) -> Result<MemoResponse, ApiError> {
    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;

    cleanup_compat_sidecar_required(state, target, request, memo_paths, fs).await?;
    cleanup_legacy_memo_required(state, target, request, &memo_paths.legacy, fs).await?;

    match fs.remove_file(&memo_paths.sidecar).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_api_error(target, request, "削除", error)),
    }

    Ok(MemoResponse::empty(
        target.relative_path().map(ToOwned::to_owned),
    ))
}
```

- [ ] **Step 2: 削除順序 test を追加する**

`test_save_route_memo_空保存_sidecar削除失敗は500を返す` の前に次を追加する。

```rust
#[tokio::test]
async fn test_save_route_memo_空保存はprimary_sidecarを最後に削除する() {
    let workspace = TempWorkspace::new().expect("workspace should be created");
    let file_path = workspace
        .write_md(Path::new("note.md"), "# note")
        .expect("target markdown should be written");
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "sidecar")
        .expect("sidecar memo should be written");
    let legacy_path = workspace
        .write_file(Path::new(".markdown-view/memos/note.md"), "legacy")
        .expect("legacy memo should be written");

    let memo_fs = MockMemoFs::new();
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let state = make_test_app_state(mode, memo_fs.clone());
    let target = resolve_route_target(&state, RouteTargetRequest::api_memo(None)).unwrap();

    let memo = save_route_memo(
        &state,
        &target,
        " ".to_string(),
        RouteTargetRequest::api_memo(None),
    )
    .await
    .expect("blank save should delete all memo files");

    assert_eq!(memo.raw(), "");
    assert_eq!(
        memo_fs.operations().await,
        vec![
            OpEvent::RemoveFile(legacy_path),
            OpEvent::RemoveFile(sidecar_path),
        ]
    );
}
```

`src/server/files/tests.rs` の import を次に変更して `OpEvent` を使えるようにする。

```rust
use super::test_support::{make_test_app_state, MockMemoFs, Op, OpEvent, TempWorkspace};
```

- [ ] **Step 3: compat / legacy 削除失敗時に primary が残る test を追加する**

既存の `test_save_route_memo_空白保存_safe_legacy削除失敗は500を返す` を次の assertion へ更新する。

```rust
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), "memo");
```

同じ test 内で `sidecar_path` が `PathBuf` として保持されていない場合は、既存作成行を次の形にする。

```rust
    let sidecar_path = workspace
        .write_file(Path::new(".note.md.memo.md"), "memo")
        .expect("sidecar memo should be written");
```

- [ ] **Step 4: compat 削除失敗 test も primary 残存を確認する**

既存の `test_save_route_memo_空白保存_safe_compat削除失敗は500を返す` に次を追加する。

```rust
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), "memo");
```

- [ ] **Step 5: targeted tests を実行する**

Run:

```bash
cargo test 空保存 --lib
```

Expected: PASS。

- [ ] **Step 6: Commit Task 5**

```bash
git add src/server/files/memo.rs src/server/files/tests.rs
git commit -m "fix: メモ空保存削除をprimary最後に変更"
```

## Task 6: task list document 更新と全体検証

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: `docs/todo/TODO.md` の対象2件を完了へ移す**

High Priority の次の2項目を `[x]` に変更し、完了済み領域へ移動する。

```markdown
- [x] メモ書き込みを `tmp + rename` で原子化する
- [x] `delete_route_memo` を all-or-nothing 化する
```

項目本文は残し、実装内容と残余リスクが読める状態を維持する。

- [ ] **Step 2: format を確認する**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS。失敗したら次を実行して整形し、差分を確認する。

```bash
cargo fmt --all
git diff -- src/server/files/memo_fs.rs src/server/files/memo.rs src/server/files/test_support.rs src/server/files/tests.rs tests/integration_test.rs
```

- [ ] **Step 3: clippy を実行する**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS。

- [ ] **Step 4: test suite を実行する**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS。

- [ ] **Step 5: full verification を実行する**

Run:

```bash
./verify.sh
```

Expected: PASS。

- [ ] **Step 6: Commit Task 6**

```bash
git add docs/todo/TODO.md
git commit -m "docs: メモ原子化TODOを完了に更新"
```

## Self-Review

- Spec coverage:
  - 非空保存の `tmp + rename`: Task 1, Task 3, Task 4。
  - rename 直前 symlink 再検証: Task 3, Task 4。
  - tmp cleanup と tmp 残骸なし: Task 1, Task 4。
  - `MockMemoFs` の同セマンティクス模倣: Task 2, Task 4, Task 5。
  - 空白保存削除の primary last: Task 5。
  - tests / verification / task list 更新: Task 4, Task 5, Task 6。
- Placeholder scan:
  - 未記入セクションなし。
  - `docs/todo/TODO.md` は実ファイル名としての記載であり、未実装 placeholder ではない。
- Type consistency:
  - `MemoWriteError`, `MemoBeforeRenameError`, `BeforeRenameCheck`, `OpEvent`, `Op::WriteAtomic`, `Op::AtomicRename` は定義 task と使用 task が対応している。
  - `save_route_memo` 側の error mapping は `MemoWriteError::BeforeRename` を 403 に変換し、既存 `io_api_error` は IO 500 を維持する。
