# Memo Sidecar Name Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract memo sidecar filename generation into a focused internal unit and add direct regression tests for overlong names, traversal-shaped text, UTF-8 boundaries, and hash collision avoidance.

**Architecture:** Add `src/server/files/memo_sidecar.rs` as the single sidecar filename construction unit. Keep `src/server/files/memo.rs` responsible for memo save/load flow and delegate filename construction to `SidecarMemoName::from_file_name`. Add direct tests in `src/server/files/tests.rs`; existing save/load tests remain behavior-level coverage.

**Tech Stack:** Rust, `sha2::Sha256`, `std::ffi::OsStr`, Unix `OsStrExt`, `cargo test`, `./verify.sh`.

---

## Task 1: Add Failing Tests

- [ ] **Step 1: Update imports**

Change the top imports to include `Path`, then import the future sidecar type:

```rust
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::{fs, os::unix::fs::symlink};

use super::memo_sidecar::SidecarMemoName;
```

- [ ] **Step 2: Add direct tests**

Place these near the existing basic unit tests:

```rust
fn assert_plain_sidecar_filename(name: &str) {
    let path = Path::new(name);
    assert!(path.parent().is_none() || path.parent() == Some(Path::new("")));
    assert_eq!(
        path.file_name().and_then(|file_name| file_name.to_str()),
        Some(name)
    );
}
#[test]
fn test_sidecar_name_超長名は255バイト以内に短縮される() {
    let file_name = format!("{}.md", "a".repeat(251));
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.starts_with("."));
    assert!(name.ends_with(".memo.md"));
    assert!(name.len() <= 255, "sidecar名が長すぎる: {}", name.len());
}
#[test]
fn test_sidecar_name_同一prefixの超長名はhashで衝突しない() {
    let common_prefix = "a".repeat(260);
    let first_name = format!("{common_prefix}-first.md");
    let second_name = format!("{common_prefix}-second.md");
    let first = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&first_name));
    let second = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&second_name));
    assert_plain_sidecar_filename(first.as_str());
    assert_plain_sidecar_filename(second.as_str());
    assert_ne!(first.as_str(), second.as_str());
    assert!(first.as_str().len() <= 255);
    assert!(second.as_str().len() <= 255);
}

#[test]
fn test_sidecar_name_特殊文字はパス区切りとして扱われない() {
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new("../secret\\..\\memo.md"));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.ends_with(".memo.md"));
    assert!(!name.contains('/'), "slashが残ってはいけない: {name}");
    assert!(!name.contains('\\'), "backslashが残ってはいけない: {name}");
    assert!(name.contains(".."), "通常文字としてのdotは保持してよい: {name}");
}

#[test]
fn test_sidecar_name_utf8境界で切り詰める() {
    let file_name = format!("{}終端.md", "あ".repeat(120));
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.ends_with(".memo.md"));
    assert!(name.len() <= 255);
    assert!(name.is_char_boundary(name.len()));
}

#[cfg(unix)]
#[test]
fn test_sidecar_name_非utf8名はhashで衝突しない() {
    let first = SidecarMemoName::from_file_name(std::ffi::OsStr::from_bytes(b"guide-\xff.md"));
    let second = SidecarMemoName::from_file_name(std::ffi::OsStr::from_bytes(b"guide-\xfe.md"));
    assert_plain_sidecar_filename(first.as_str());
    assert_plain_sidecar_filename(second.as_str());
    assert!(first.as_str().starts_with("._bin."));
    assert!(second.as_str().starts_with("._bin."));
    assert!(first.as_str().ends_with(".memo.md"));
    assert!(second.as_str().ends_with(".memo.md"));
    assert_ne!(first.as_str(), second.as_str());
}
```

- [ ] **Step 3: Verify red state**

Run:

```bash
cargo test --all-targets --all-features sidecar_name
```

Expected: FAIL to compile because `memo_sidecar` and `SidecarMemoName` do not exist. Do not commit this red state.

## Task 2: Add Naming Unit And Delegate Memo Code

- [ ] **Step 1: Register module**

In `src/server/files/mod.rs`:

```rust
mod catalog;
mod content;
mod memo;
mod memo_sidecar;
mod resolve;
mod search;
```

- [ ] **Step 2: Create `memo_sidecar.rs`**

Create `src/server/files/memo_sidecar.rs`:

```rust
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::ffi::OsStr;
use sha2::{Digest, Sha256};
const MEMO_SUFFIX: &str = ".memo.md";
const MAX_FILENAME_BYTES: usize = 255;
const SIDECAR_HASH_LEN: usize = 16;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SidecarMemoName(String);
impl SidecarMemoName {
    pub(super) fn from_file_name(file_name: &OsStr) -> Self {
        if let Some(name) = file_name.to_str().filter(|name| !name.is_empty()) {
            return Self(build_from_utf8(name));
        }
        #[cfg(unix)]
        {
            return Self(build_from_bytes(file_name.as_bytes()));
        }
        #[cfg(not(unix))]
        {
            Self(format!(".{}", MEMO_SUFFIX.trim_start_matches('.')))
        }
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}
fn build_from_utf8(file_name: &str) -> String {
    let normalized = normalize_visible_separators(file_name);
    let full = format!(".{normalized}{MEMO_SUFFIX}");
    if full.len() <= MAX_FILENAME_BYTES {
        return full;
    }
    let short_hash = short_hash(file_name.as_bytes());
    let reserved = 1 + 1 + SIDECAR_HASH_LEN + MEMO_SUFFIX.len();
    let prefix_budget = MAX_FILENAME_BYTES.saturating_sub(reserved);
    let prefix = truncate_to_bytes(&normalized, prefix_budget);
    format!(".{prefix}.{short_hash}{MEMO_SUFFIX}")
}
#[cfg(unix)]
fn build_from_bytes(bytes: &[u8]) -> String {
    let short_hash = short_hash(bytes);
    format!("._bin.{short_hash}{MEMO_SUFFIX}")
}
fn normalize_visible_separators(file_name: &str) -> String {
    file_name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' => '_',
            _ => ch,
        })
        .collect()
}
fn short_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let hash = format!("{:x}", digest);
    hash[..SIDECAR_HASH_LEN].to_string()
}
fn truncate_to_bytes(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let mut end = 0;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    &input[..end]
}
```

- [ ] **Step 3: Delegate `memo.rs`**

In `src/server/files/memo.rs`, remove `OsStrExt`, `sha2::{Digest, Sha256}`, `MEMO_SUFFIX`, `MAX_FILENAME_BYTES`, `SIDECAR_HASH_LEN`, `build_sidecar_file_name`, `build_sidecar_file_name_from_utf8`, and `truncate_to_bytes`.

Add:

```rust
use super::memo_sidecar::SidecarMemoName;
```
Replace `sidecar_memo_path_for_target` with:

```rust
fn sidecar_memo_path_for_target(target: &ResolvedTarget) -> PathBuf {
    let target_path = target.file_path();
    let parent = target_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let file_name = target_path
        .file_name()
        .map(SidecarMemoName::from_file_name)
        .unwrap_or_else(|| SidecarMemoName::from_file_name(std::ffi::OsStr::new("memo.md")));
    parent.join(file_name.as_str())
}
```

- [ ] **Step 4: Verify targeted tests**

Run:

```bash
cargo test --all-targets --all-features sidecar_name
cargo test --all-targets --all-features memo
```

Expected: both commands PASS.

- [ ] **Step 5: Commit implementation**

Run:

```bash
git add src/server/files/mod.rs src/server/files/memo_sidecar.rs src/server/files/memo.rs src/server/files/tests.rs
git commit -m "test: メモsidecar名生成の境界テストを追加"
```

## Task 3: Final Verification And Queue Update

- [ ] **Step 1: Mark queue item complete**

In `docs/todo/TODO.md`, change:

```markdown
- [ ] メモ sidecar fallback 経路の超長ファイル名＋特殊文字テストを追加
```

to:

```markdown
- [x] メモ sidecar fallback 経路の超長ファイル名＋特殊文字テストを追加
```

- [ ] **Step 2: Run full verification**

Run:

```bash
./verify.sh
```

Expected: command exits 0 after format check, clippy, Rust tests, and repository-required checks pass.

- [ ] **Step 3: Commit queue update**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: メモsidecar名項目を完了に更新"
```

- [ ] **Step 4: Report final status**

Include changed files, affected dependent files, results for both targeted `cargo test` commands and `./verify.sh`, and residual risk: direct tests cover naming while runtime coverage still relies on existing memo save/load tests.
