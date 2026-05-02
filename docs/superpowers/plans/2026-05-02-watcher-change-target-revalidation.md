# Watcher Change Target Revalidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** watcher経由のディレクトリ変更イベントを読込直前にHTTP経路と同等の検証へ通し、安全なcanonical pathだけを描画する。

**Architecture:** `resolve_change_target()` をwatcher変更通知の最終検証ゲートにする。ディレクトリモードでは `changed_file` をbase相対文字列に戻して `resolve_file()` 相当の内部検証へ通し、ディレクトリモードの `NotFound` とwatcher由来の `InvalidPath` はbroadcastをスキップする。HTTP/API向け `resolve_file()` は404統一のためcanonicalize失敗を従来通り `NotFound` に揃えるが、watcher変更通知用の内部経路だけは `ErrorKind::NotFound` 以外のcanonicalize I/O失敗を `Io(ErrorKind)` として保持する。単一ファイルモードの `NotFound` は監視対象消失として検証エラーを通知する。存在するが通常ファイルでない `.md` は `NotFile`、存在するbase外ファイルやbase外symlinkは `Traversal`、canonicalizeの一時不在以外のI/O失敗は `Io(ErrorKind)` として検証エラーに分類し、Error broadcastには `PermissionDenied` 等の `ErrorKind` を含める。

**Tech Stack:** Rust, axum, tokio, tempfile, `cargo test`, `./verify.sh`

---

## File Structure

- Modify: `src/server/files/tests.rs`
  - watcher変更通知のresolver境界とbroadcast境界の回帰テストを追加する。
  - `resolve_change_target` を直接テストするため、既存の `revalidate_single_file_target` importへ同居させる。
- Modify: `src/server/broadcast.rs`
  - `notify_update` の既存テスト期待値を、base外は検証エラー、削除済みは送信スキップへ更新する。
- Modify: `src/server/files/resolve.rs`
  - `resolve_change_target()` の単一ファイル分岐を再検証済みcanonical path利用へ寄せる。
  - `resolve_directory_change_target()` と `relative_change_path()` を追加する。
- Modify: `src/server/files/content.rs`
  - watcher変更通知でディレクトリモードの `ResolveFileError::NotFound` をbroadcastスキップへ変換する。
  - watcher由来の `ResolveFileError::InvalidPath` をブラウザへ送らず、ローカルログに留める。
  - WebSocket受信者なしログでもディレクトリモードの `NotFound` はローカルエラー扱いにしない。
- Modify: `docs/todo/TODO.md`
  - 実装と検証完了後、対象High Priority項目を完了済みにする。

---

### Task 1: 失敗テストでwatcher再検証の境界を固定する

**Files:**
- Modify: `src/server/files/tests.rs`
- Modify: `src/server/broadcast.rs`

- [ ] **Step 1: resolver直接テスト用のimportを追加する**

既存のimport:

```rust
use super::resolve::revalidate_single_file_target;
```

を次に変更する。

```rust
use super::resolve::{resolve_change_target, revalidate_single_file_target};
```

- [ ] **Step 2: `test_build_change_broadcast_message_ディレクトリモードでfileを含むupdateを返す` の直後に失敗テストを追加する**

```rust
#[test]
fn test_resolve_change_target_ディレクトリ変更はcanonical_pathへ再解決する() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());
    let changed = dir.path().join("docs/../docs/api.md");
    let expected = dir.path().join("docs/api.md").canonicalize().unwrap();

    let target = resolve_change_target(&state, &changed)
        .expect("watcher change should resolve")
        .expect("directory watcher change should produce a target");

    assert_eq!(target.file_path(), expected.as_path());
    assert_eq!(target.relative_path(), Some("docs/api.md"));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更の隠しパスは拒否する() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());
    let hidden = dir.path().join(".hidden/secret.md");

    let result = resolve_change_target(&state, &hidden);

    assert_eq!(result, Err(ResolveFileError::Hidden));
}

#[test]
fn test_resolve_change_target_ディレクトリ変更のbase外パスは拒否する() {
    let base_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside = outside_dir.path().join("outside.md");
    std::fs::write(&outside, "# outside").unwrap();
    let state = create_directory_state(base_dir.path());

    let result = resolve_change_target(&state, &outside);

    assert_eq!(result, Err(ResolveFileError::Traversal));
}

#[cfg(unix)]
#[test]
fn test_resolve_change_target_ディレクトリ変更のbase外symlinkは拒否する() {
    let base_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let outside = outside_dir.path().join("secret.md");
    std::fs::write(&outside, "# secret").unwrap();
    let link = base_dir.path().join("link.md");
    symlink(&outside, &link).unwrap();
    let state = create_directory_state(base_dir.path());

    let result = resolve_change_target(&state, &link);

    assert_eq!(result, Err(ResolveFileError::Traversal));
}

#[test]
fn test_resolve_change_target_単一ファイル変更は再検証済みpathを返す() {
    let (_dir, file_path) = create_markdown_fixture("target.md", "# target");
    let canonical = file_path.canonicalize().unwrap();
    let state = create_single_file_state(&file_path);

    let target = resolve_change_target(&state, &file_path)
        .expect("single file change should resolve")
        .expect("single file watcher change should produce a target");

    assert_eq!(target.file_path(), canonical.as_path());
}
```

- [ ] **Step 3: broadcast境界の失敗テストを同じ位置に追加する**

```rust
#[tokio::test]
async fn test_build_change_broadcast_message_削除済みディレクトリ変更はbroadcastをスキップする() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("deleted.md");
    std::fs::write(&target, "# deleted").unwrap();
    let state = create_directory_state(dir.path());
    std::fs::remove_file(&target).unwrap();

    let message = build_change_broadcast_message(&state, &target).await;

    assert!(message.is_none(), "削除済みファイルはbroadcastしない");
}

#[tokio::test]
async fn test_build_change_broadcast_message_隠しパスは検証エラーをbroadcastする() {
    let dir = create_test_dir();
    let state = create_directory_state(dir.path());
    let hidden = dir.path().join(".hidden/secret.md");

    let message = build_change_broadcast_message(&state, &hidden)
        .await
        .expect("hidden path should broadcast a validation error");

    match message {
        BroadcastMessage::Error(msg) => {
            assert!(
                msg.contains("ファイル検証エラー"),
                "検証エラーのprefixを期待: {}",
                msg
            );
            assert!(
                msg.contains("隠しファイルへのアクセスは禁止されています"),
                "Hiddenのエラー文言を期待: {}",
                msg
            );
        }
        other => panic!("Errorを期待したが {:?} を受信", other),
    }
}
```

- [ ] **Step 4: `src/server/broadcast.rs` の既存期待値を新仕様へ更新する**

`test_notify_update_ディレクトリモードで相対パス算出失敗時は送信をスキップ` を次に置き換える。

```rust
#[tokio::test]
async fn test_notify_update_ディレクトリモードでbase外パスは検証エラーを送信する() {
    let base_dir = tempfile::tempdir().unwrap();
    std::fs::write(base_dir.path().join("README.md"), "# README").unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    let outside_file = outside_dir.path().join("outside.md");
    std::fs::write(&outside_file, "# outside").unwrap();
    let outside_canonical = outside_file.canonicalize().unwrap();

    let state = create_directory_state(base_dir.path());
    let mut rx = state.tx().subscribe();

    notify_update(&state, &outside_canonical).await;

    let received = rx.recv().await.unwrap();
    match received {
        BroadcastMessage::Error(message) => {
            assert!(message.contains("ファイル検証エラー"));
            assert!(
                message.contains("ディレクトリ外へのアクセスは禁止されています"),
                "Traversalのエラー文言を期待: {}",
                message
            );
        }
        other => panic!("Errorメッセージを期待したが {:?} を受信", other),
    }
}
```

`test_notify_update_ディレクトリモードで読み込み失敗時はerrorを送信する` を次に置き換える。

```rust
#[tokio::test]
async fn test_notify_update_ディレクトリモードで削除済みファイルは送信をスキップする() {
    let base_dir = tempfile::tempdir().unwrap();
    let target = base_dir.path().join("README.md");
    std::fs::write(&target, "# before").unwrap();

    let state = create_directory_state(base_dir.path());
    let mut rx = state.tx().subscribe();

    std::fs::remove_file(&target).unwrap();
    notify_update(&state, &target).await;

    assert!(matches!(
        rx.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
}
```

- [ ] **Step 5: 失敗を確認する**

Run:

```bash
cargo test --all-targets --all-features resolve_change_target
```

Expected:

```text
test_resolve_change_target_ディレクトリ変更はcanonical_pathへ再解決する ... FAILED
test_resolve_change_target_ディレクトリ変更の隠しパスは拒否する ... FAILED
test_resolve_change_target_ディレクトリ変更のbase外パスは拒否する ... FAILED
```

Run:

```bash
cargo test --all-targets --all-features build_change_broadcast_message
```

Expected:

```text
test_build_change_broadcast_message_削除済みディレクトリ変更はbroadcastをスキップする ... FAILED
test_build_change_broadcast_message_隠しパスは検証エラーをbroadcastする ... FAILED
```

Run:

```bash
cargo test --all-targets --all-features notify_update
```

Expected:

```text
test_notify_update_ディレクトリモードでbase外パスは検証エラーを送信する ... FAILED
test_notify_update_ディレクトリモードで削除済みファイルは送信をスキップする ... FAILED
```

- [ ] **Step 6: テスト追加をコミットする**

```bash
git add src/server/files/tests.rs src/server/broadcast.rs
git commit -m "test: watcher変更ターゲット再検証の境界を追加"
```

---

### Task 2: watcher変更ターゲットを読込直前に再検証する

**Files:**
- Modify: `src/server/files/resolve.rs`
- Modify: `src/server/files/content.rs`

- [ ] **Step 1: `resolve_change_target()` を再検証済みpath利用へ変更する**

`src/server/files/resolve.rs` の既存関数:

```rust
pub(super) fn resolve_change_target(
    state: &AppState,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    if let Some(expected) = state.mode().single_file() {
        revalidate_single_file_target(expected, state.mode().base_dir())?;
        Ok(Some(build_resolved_target(
            state,
            changed_file.to_path_buf(),
            None,
            "更新対象の相対パス算出失敗",
        )))
    } else {
        Ok(build_update_target(state, changed_file))
    }
}
```

を次に置き換える。

```rust
pub(super) fn resolve_change_target(
    state: &AppState,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    if let Some(expected) = state.mode().single_file() {
        let validated_path = revalidate_single_file_target(expected, state.mode().base_dir())?;
        return Ok(Some(build_resolved_target(
            state,
            validated_path,
            None,
            "更新対象の相対パス算出失敗",
        )));
    }

    resolve_directory_change_target(state, changed_file)
}
```

- [ ] **Step 2: `resolve_directory_change_target()` と相対化ヘルパーを追加する**

`build_update_target()` の前に次を追加する。

```rust
fn resolve_directory_change_target(
    state: &AppState,
    changed_file: &Path,
) -> Result<Option<ResolvedTarget>, ResolveFileError> {
    let Some(base_dir) = state.mode().directory() else {
        tracing::error!("[markdown-view] 未知のAppModeです");
        return Err(ResolveFileError::InternalState);
    };

    let relative = relative_change_path(base_dir, changed_file)?;
    let relative_string = relative_change_path_to_query(&relative)?;
    let validated_path = resolve_file(base_dir, &relative_string)?;
    Ok(Some(build_resolved_target(
        state,
        validated_path,
        None,
        "更新対象の相対パス算出失敗",
    )))
}

fn relative_change_path(base_dir: &Path, changed_file: &Path) -> Result<PathBuf, ResolveFileError> {
    if let Ok(relative) = changed_file.strip_prefix(base_dir) {
        return Ok(relative.to_path_buf());
    }

    let canonical_base = base_dir.canonicalize().map_err(|error| {
        let error_kind = error.kind();
        tracing::warn!(
            "[markdown-view] watcher変更ターゲット: ベース正規化失敗: {} ({})",
            base_dir.display(),
            error
        );
        resolve_canonicalize_error(error_kind)
    })?;

    let canonical_changed = changed_file.canonicalize().map_err(|error| {
        let error_kind = error.kind();
        tracing::warn!(
            "[markdown-view] watcher変更ターゲット: パス正規化失敗: {} ({})",
            sanitize_path_for_logging(changed_file, base_dir),
            error
        );
        resolve_canonicalize_error(error_kind)
    })?;

    canonical_changed
        .strip_prefix(&canonical_base)
        .map(Path::to_path_buf)
        .map_err(|_| ResolveFileError::Traversal)
}

fn relative_change_path_to_query(relative: &Path) -> Result<String, ResolveFileError> {
    relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or(ResolveFileError::InvalidPath)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}

fn resolve_canonicalize_error(error_kind: std::io::ErrorKind) -> ResolveFileError {
    if error_kind == std::io::ErrorKind::NotFound {
        ResolveFileError::NotFound
    } else {
        ResolveFileError::Io(error_kind)
    }
}
```

`relative_change_path_to_query()` はcomponent単位で `/` joinする。非UTF-8 componentは `to_string_lossy()` で置換せず `InvalidPath` にし、Unix上の `back\slash.md` のbackslashは区切りではなくファイル名の通常文字として保持する。

- [ ] **Step 3: 未使用になった `build_update_target()` を削除する**

`src/server/files/resolve.rs` から次の関数全体を削除する。

```rust
fn build_update_target(state: &AppState, changed_file: &Path) -> Option<ResolvedTarget> {
    let target = build_resolved_target(
        state,
        changed_file.to_path_buf(),
        None,
        "相対パス算出失敗のためブロードキャストをスキップ",
    );
    if state.mode().is_directory() && target.relative_path.is_none() {
        return None;
    }
    Some(target)
}
```

- [ ] **Step 4: watcher変更通知で `NotFound` をスキップする**

`src/server/files/content.rs` の `build_change_broadcast_message()` 内の `ResolveFailed` 分岐:

```rust
ValidateRenderOutcome::ResolveFailed(error) => {
    let file_label = change_error_file_label(state, changed_file);
    tracing::warn!(
        "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
        file_label,
        error
    );
    Some(BroadcastMessage::Error(format!(
        "ファイル検証エラー ({}): {}",
        file_label, error
    )))
}
```

を次に置き換える。

```rust
ValidateRenderOutcome::ResolveFailed(ResolveFileError::NotFound) => {
    let file_label = change_error_file_label(state, changed_file);
    tracing::debug!(
        "[markdown-view] 更新対象が削除または一時不在のためbroadcastをスキップ: {}",
        file_label
    );
    None
}
ValidateRenderOutcome::ResolveFailed(error) => {
    let file_label = change_error_file_label(state, changed_file);
    tracing::warn!(
        "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
        file_label,
        error
    );
    Some(BroadcastMessage::Error(format!(
        "ファイル検証エラー ({}): {}",
        file_label, error
    )))
}
```

- [ ] **Step 5: WebSocket受信者なしログでも `NotFound` をスキップする**

`build_change_error_log_message_without_receivers()` の `Err(error)` 分岐:

```rust
Err(error) => {
    let file_label = change_error_file_label(state, changed_file);
    return Some(format!(
        "更新時ファイル検証失敗 ({}): {}",
        file_label, error
    ));
}
```

を次に置き換える。

```rust
Err(ResolveFileError::NotFound) => return None,
Err(error) => {
    let file_label = change_error_file_label(state, changed_file);
    return Some(format!(
        "更新時ファイル検証失敗 ({}): {}",
        file_label, error
    ));
}
```

- [ ] **Step 6: 対象テストを通す**

Run:

```bash
cargo test --all-targets --all-features resolve_change_target
```

Expected:

```text
test result: ok.
```

Run:

```bash
cargo test --all-targets --all-features build_change_broadcast_message
```

Expected:

```text
test result: ok.
```

Run:

```bash
cargo test --all-targets --all-features notify_update
```

Expected:

```text
test result: ok.
```

- [ ] **Step 7: 変更をコミットする**

```bash
git add src/server/files/resolve.rs src/server/files/content.rs
git commit -m "fix: watcher変更ターゲットを最終読込前に再検証"
```

---

### Task 3: TODO更新と全体検証で完了状態にする

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: `docs/todo/TODO.md` の対象項目を完了済みにする**

次の行:

```markdown
- [ ] 監視イベント経由の変更ファイルを最終読込前に再検証する
```

を次に変更する。

```markdown
- [x] 監視イベント経由の変更ファイルを最終読込前に再検証する
```

- [ ] **Step 2: 統合テストを実行する**

Run:

```bash
cargo test --test integration_test --all-features
```

Expected:

```text
test result: ok.
```

- [ ] **Step 3: 必須検証を実行する**

Run:

```bash
./verify.sh
```

Expected:

```text
すべての検証が成功する
```

- [ ] **Step 4: TODO更新をコミットする**

```bash
git add docs/todo/TODO.md
git commit -m "docs: watcher変更ターゲット再検証を完了扱いにする"
```

- [ ] **Step 5: 完了前の差分と履歴を確認する**

Run:

```bash
git status --short
```

Expected:

```text
出力なし
```

Run:

```bash
git log --oneline -4
```

Expected:

```text
先頭4件に、テスト追加、実装、TODO更新、設計または直前コミットが並ぶ
```
