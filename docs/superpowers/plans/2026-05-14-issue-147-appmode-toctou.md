# Issue 147 AppMode TOCTOU Mitigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `AppMode` 構築と起動時モード分岐で、`canonicalize` 後の `is_file()` / `is_dir()` 判定を `metadata().file_type()` ベースへ置き換える。

**Architecture:** `CanonicalPath` は既存どおり canonical path の境界として残し、`src/server/state.rs` 内の private helper で metadata 取得と種別判定を集約する。`src/main.rs` は canonical path に対して metadata を一度取得し、その `file_type()` と `len()` を使ってモード分岐とサイズチェックを行う。

**Tech Stack:** Rust, std::fs metadata, anyhow context, cargo test, repository `./verify.sh`

---

## File Structure

- Modify: `src/server/state.rs`
  - `AppModeBuildError` の public variant set は維持する。
  - `metadata_for_mode()`, `ensure_canonical_file()`, `ensure_canonical_directory()` を private helper として追加する。
  - `AppMode::new_single_file()` と `AppMode::new_directory()` を helper 経由に変更する。
  - unit tests に metadata 取得失敗を既存 variant へ集約する regression test を追加する。
- Modify: `src/main.rs`
  - `path.is_file()` / `path.is_dir()` による起動時分岐をやめる。
  - `std::fs::metadata(&path)` の戻り値から `file_type()` と `len()` を使う。
- No production API changes:
  - `AppMode::new_single_file()`, `AppMode::new_directory()`, `single_file()`, `directory()`, `base_dir()` の呼び出し形は維持する。

## Task 1: AppMode metadata failure keeps existing public variants

**Files:**
- Modify: `src/server/state.rs`
- Test: `src/server/state.rs`

- [ ] **Step 1: Write failing metadata failure unit tests**

Add these tests inside the existing `#[cfg(test)] mod tests` in `src/server/state.rs`, near the other `AppModeBuildError` / `AppMode` tests.

```rust
    #[test]
    fn test_ensure_canonical_file_metadata失敗はnotfileへ集約する() {
        let (_dir, file_path) = create_markdown_fixture("vanish.md", "# vanish");
        let canonical = file_path.canonicalize().unwrap();
        let canonical_path = CanonicalPath(canonical.clone());
        std::fs::remove_file(&file_path).unwrap();

        let result = ensure_canonical_file(&canonical_path);

        assert!(matches!(
            result,
            Err(AppModeBuildError::NotFile(path)) if path == canonical
        ));
    }

    #[test]
    fn test_ensure_canonical_directory_metadata失敗はnotdirectoryへ集約する() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let canonical_path = CanonicalPath(canonical.clone());
        std::fs::remove_dir(dir.path()).unwrap();

        let result = ensure_canonical_directory(&canonical_path);

        assert!(matches!(
            result,
            Err(AppModeBuildError::NotDirectory(path)) if path == canonical
        ));
    }
```

- [ ] **Step 2: Run the focused test and confirm it fails**

Run:

```bash
cargo test --lib server::state::tests::test_ensure_canonical_file_metadata失敗はnotfileへ集約する
cargo test --lib server::state::tests::test_ensure_canonical_directory_metadata失敗はnotdirectoryへ集約する
```

Expected: failure because metadata failure is not yet mapped to the existing `NotFile` / `NotDirectory` variants.

- [ ] **Step 3: Keep `AppModeBuildError` public variants unchanged**

In `src/server/state.rs`, keep `AppModeBuildError` limited to the existing public variants:

```rust
pub enum AppModeBuildError {
    /// canonicalize済みパスの生成に失敗
    CanonicalPath(CanonicalPathError),
    /// 単一ファイルモードでファイル以外が指定された
    NotFile(PathBuf),
    /// ディレクトリモードでディレクトリ以外が指定された
    NotDirectory(PathBuf),
    /// 単一ファイルモードで.md以外が指定された
    NotMarkdown(PathBuf),
}
```

Use `source()` only for `CanonicalPath`:

```rust
impl std::error::Error for AppModeBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppModeBuildError::CanonicalPath(error) => Some(error),
            AppModeBuildError::NotFile(_)
            | AppModeBuildError::NotDirectory(_)
            | AppModeBuildError::NotMarkdown(_) => None,
        }
    }
}
```

- [ ] **Step 4: Run the focused test and confirm it passes**

Run:

```bash
cargo test --lib server::state::tests::test_ensure_canonical_file_metadata失敗はnotfileへ集約する
cargo test --lib server::state::tests::test_ensure_canonical_directory_metadata失敗はnotdirectoryへ集約する
```

Expected: PASS.

## Task 2: AppMode file/directory checks use one metadata result

**Files:**
- Modify: `src/server/state.rs`
- Test: `src/server/state.rs`

- [ ] **Step 1: Run existing regression tests before editing behavior**

Run:

```bash
cargo test --lib server::state::tests::test_app_mode_new_single_file_ディレクトリ指定は拒否
cargo test --lib server::state::tests::test_app_mode_new_directory_ファイル指定は拒否
```

Expected: both commands PASS before implementation. These tests preserve the visible `NotFile` / `NotDirectory` behavior while the internals change.

- [ ] **Step 2: Add private metadata helper functions**

Add these functions after `impl std::error::Error for AppModeBuildError` and before `enum AppModeKind` in `src/server/state.rs`:

```rust
fn metadata_for_mode(canonical: &CanonicalPath) -> std::io::Result<std::fs::Metadata> {
    std::fs::metadata(canonical.as_path())
}

fn ensure_canonical_file(canonical: &CanonicalPath) -> Result<(), AppModeBuildError> {
    let metadata = metadata_for_mode(canonical)
        .map_err(|_| AppModeBuildError::NotFile(canonical.as_path().to_path_buf()))?;
    if !metadata.file_type().is_file() {
        return Err(AppModeBuildError::NotFile(
            canonical.as_path().to_path_buf(),
        ));
    }
    Ok(())
}

fn ensure_canonical_directory(canonical: &CanonicalPath) -> Result<(), AppModeBuildError> {
    let metadata = metadata_for_mode(canonical)
        .map_err(|_| AppModeBuildError::NotDirectory(canonical.as_path().to_path_buf()))?;
    if !metadata.file_type().is_dir() {
        return Err(AppModeBuildError::NotDirectory(
            canonical.as_path().to_path_buf(),
        ));
    }
    Ok(())
}
```

- [ ] **Step 3: Update `AppMode::new_single_file()`**

Replace the current `is_file()` block in `new_single_file()`:

```rust
        if !canonical.as_path().is_file() {
            return Err(AppModeBuildError::NotFile(
                canonical.as_path().to_path_buf(),
            ));
        }
```

with:

```rust
        ensure_canonical_file(&canonical)?;
```

Keep the existing `.md` extension check unchanged.

- [ ] **Step 4: Update `AppMode::new_directory()`**

Replace the current `is_dir()` block in `new_directory()`:

```rust
        if !canonical.as_path().is_dir() {
            return Err(AppModeBuildError::NotDirectory(
                canonical.as_path().to_path_buf(),
            ));
        }
```

with:

```rust
        ensure_canonical_directory(&canonical)?;
```

- [ ] **Step 5: Run AppMode state tests**

Run:

```bash
cargo test --lib server::state
```

Expected: PASS.

- [ ] **Step 6: Confirm `state.rs` no longer uses `is_file()` / `is_dir()` for AppMode construction**

Run:

```bash
rg -n "canonical\\.as_path\\(\\)\\.is_(file|dir)|path\\.is_(file|dir)" src/server/state.rs
```

Expected: no output.

- [ ] **Step 7: Commit AppMode changes**

Run:

```bash
git add src/server/state.rs
git commit -m "fix: AppMode種別判定をmetadataベースに変更"
```

Expected: commit succeeds with only `src/server/state.rs` staged.

## Task 3: main.rs startup mode detection uses metadata file_type

**Files:**
- Modify: `src/main.rs`
- Test: `src/main.rs`

- [ ] **Step 1: Extract startup mode helper and cover it with unit tests**

Add `build_app_mode_for_canonical_path(&Path) -> Result<AppMode>` and cover these cases:

- `.md` file becomes single-file mode.
- directory becomes directory mode.
- oversized Markdown is rejected before `AppMode` construction.
- metadata failure includes the canonical path in the startup context error.

The helper should centralize startup metadata retrieval:

```rust
fn build_app_mode_for_canonical_path(path: &Path) -> Result<AppMode> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("パスのメタデータ取得に失敗: {}", path.display()))?;
    let file_type = metadata.file_type();

    if file_type.is_file() {
        if metadata.len() > MAX_FILE_SIZE {
            bail!(
                "ファイルサイズが上限（{}MB）を超えています: {}",
                MAX_FILE_SIZE / 1024 / 1024,
                path.display()
            );
        }
        AppMode::new_single_file(path).context("単一ファイルモードの初期化に失敗")
    } else if file_type.is_dir() {
        AppMode::new_directory(path).context("ディレクトリモードの初期化に失敗")
    } else {
        bail!(
            "指定されたパスはファイルでもディレクトリでもありません: {}",
            path.display()
        );
    }
}
```

Then replace the startup branch with:

```rust
    // ファイルかディレクトリかを判定してモードを決定
    let mode = build_app_mode_for_canonical_path(&path)?;
```

- [ ] **Step 2: Format and run targeted compile/test**

Run:

```bash
cargo fmt --all
cargo test --bin markdown-view build_app_mode_for_canonical_path
cargo test --all-targets --all-features app_mode
```

Expected: format succeeds and tests containing `app_mode` pass.

- [ ] **Step 3: Confirm startup branch no longer calls `path.is_file()` / `path.is_dir()`**

Run:

```bash
rg -n "path\\.is_(file|dir)\\(\\)" src/main.rs
```

Expected: no output.

- [ ] **Step 4: Commit startup branch changes**

Run:

```bash
git add src/main.rs
git commit -m "fix: 起動時モード判定をmetadataベースに変更"
```

Expected: commit succeeds with only `src/main.rs` staged.

## Task 4: Full verification

**Files:**
- Verify: whole repository

- [ ] **Step 1: Run acceptance static checks**

Run:

```bash
rg -n "canonical\\.as_path\\(\\)\\.is_(file|dir)|path\\.is_(file|dir)\\(\\)" src/server/state.rs src/main.rs
```

Expected: no output.

- [ ] **Step 2: Run focused tests**

Run:

```bash
cargo test --lib server::state
cargo test --all-targets --all-features app_mode
```

Expected: both commands PASS.

- [ ] **Step 3: Run full verification**

Run:

```bash
./verify.sh
```

Expected: format, clippy, and tests all PASS.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git status --short --branch
git log --oneline -3
```

Expected: clean working tree on the implementation branch, with the two implementation commits on top of the plan/spec commits.

## Security Notes

- 起動引数の path は未信頼入力として扱い続ける。
- この計画は `canonicalize` 後の種別判定 window を縮める緩和であり、filesystem race を完全には排除しない。
- Host/Origin/CSP、HTML sanitization、path traversal、file size guard は変更しない。
- `MAX_FILE_SIZE` は起動時 metadata と既存の実読込時 guard の両方で維持する。

## Rollback Path

実装に問題が出た場合は、Task 3 の commit を revert すれば起動時分岐だけを旧実装へ戻せる。Task 2 の commit を revert すれば `AppMode` の内部判定も旧実装へ戻せる。docs/spec/plan commit は挙動に影響しないため、実装修正だけを切り戻せる。

## Implementation Approval Gate

この plan は実装前の計画である。実装に入る前に、利用者から実行方法の承認を得る。
