# Path Log Sanitizer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 監査ログ用のパスサニタイザ `sanitize_path_for_logging` を新設し、`tracing::warn!` で絶対パスを `path.display()` で吐いている異常系経路を base 相対化する。base 外パスは file_name のみ残して `<outside-base>/{file_name}` に丸める。

**Architecture:** 新規モジュール `src/server/log_path.rs` に `pub(crate) fn sanitize_path_for_logging(path: &Path, base: &Path) -> Cow<str>` を実装。`AppMode::base_dir()`（単一ファイルモード時はファイルの parent ディレクトリ）を base として渡す。`base_dir.display()` 自体はサーバー所有者が指定した値であり保守性優先で **保持**する。対象は `src/server/files/{resolve, catalog, memo, content}.rs`、`src/server/routes.rs`、`src/watcher/strategy.rs` の異常系 `tracing::warn!`。

**Tech Stack:** Rust 1.x, std::path::{Path, PathBuf}, std::borrow::Cow, tracing

**関連 spec:** `docs/superpowers/specs/2026-04-23-defensive-observability-hardening-design.md` Part 2

---

## ブランチ戦略

このプランの実装は新規 feature ブランチ `feat/path-log-sanitizer` で行う（CLAUDE.md ブランチルール準拠）。develop へは squash merge。

```bash
git checkout develop
git pull
git checkout -b feat/path-log-sanitizer
```

CSP fail-fast プラン（`feat/csp-fail-fast`）と本プランは独立しているため、並列実装可。

---

## File Structure（新規・既存修正の責務マップ）

| ファイル | 種別 | 責務 |
|---|---|---|
| `src/server/log_path.rs` | 新規 | `sanitize_path_for_logging` 関数本体 + ユニットテスト |
| `src/server.rs` | 修正 | `mod log_path;` 宣言追加 |
| `src/server/files/resolve.rs` | 修正 | 3 statements 置換 + `revalidate_single_file_target` シグネチャ変更（`base_dir` パラメータ追加） |
| `src/server/files/catalog.rs` | 修正 | 6 statements の `path` 側を sanitize（`base_dir` 側は保持） |
| `src/server/files/memo.rs` | 修正 | `ensure_safe_memo_path` 内の 1 statement 置換 |
| `src/server/files/content.rs` | 修正 | `file_label` fallback の sanitize 化 |
| `src/server/routes.rs` | 修正 | `tracing::warn!` の 1 statement で `file_path().display()` を `file_label()` に置換 |
| `src/watcher/strategy.rs` | 修正 | 5 statements の `path` 側を sanitize（`base.display()` は保持） |

---

### Task 1: 新規モジュール `log_path.rs` 作成（テストを先に書く）

**Files:**
- Create: `src/server/log_path.rs`

- [ ] **Step 1: ファイル全体を作成**

`src/server/log_path.rs` を新規作成し、以下の内容を書き込む:

```rust
//! 監査ログ用のパスサニタイザ。
//!
//! 絶対パスの直接出力（`path.display()`）はディレクトリ構造を漏らすため、
//! base_dir 相対化を経由する。base 外パスは file_name のみ残して
//! `<outside-base>/{file_name}` で出力する。
//!
//! `base_dir` 自体（`base_dir.display()`）の出力はサーバー所有者が指定した
//! 値であり保守性優先で保持する。本ヘルパーの対象は base 配下/外を含む
//! 「path 引数」側のみ。

use std::borrow::Cow;
use std::path::Path;

/// 監査ログ用にパスを base 相対化する。
///
/// - `path` が `base` 配下: 相対パス文字列（例: `"subdir/file.md"`）
/// - `path` が `base` 外: `<outside-base>/{file_name}`
/// - `file_name` 取得不可（ルート等）: `<outside-base>`
pub(crate) fn sanitize_path_for_logging<'a>(path: &'a Path, base: &Path) -> Cow<'a, str> {
    match path.strip_prefix(base) {
        Ok(relative) => Cow::Owned(relative.display().to_string()),
        Err(_) => match path.file_name() {
            Some(name) => Cow::Owned(format!("<outside-base>/{}", name.to_string_lossy())),
            None => Cow::Borrowed("<outside-base>"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_sanitize_base配下を相対パスにする() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/base/subdir/file.md");
        assert_eq!(sanitize_path_for_logging(&path, &base), "subdir/file.md");
    }

    #[test]
    fn test_sanitize_base直下のファイル() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/base/file.md");
        assert_eq!(sanitize_path_for_logging(&path, &base), "file.md");
    }

    #[test]
    fn test_sanitize_base外はfile_nameのみ() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/other/secret.md");
        assert_eq!(
            sanitize_path_for_logging(&path, &base),
            "<outside-base>/secret.md"
        );
    }

    #[test]
    fn test_sanitize_file_nameなしは完全マスク() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/");
        assert_eq!(sanitize_path_for_logging(&path, &base), "<outside-base>");
    }

    #[test]
    fn test_sanitize_path自体がbaseの場合() {
        // strip_prefix 成功で空文字（""）になる。許容挙動。
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/base");
        assert_eq!(sanitize_path_for_logging(&path, &base), "");
    }

    #[cfg(unix)]
    #[test]
    fn test_sanitize_非utf8_file_name() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let base = PathBuf::from("/base");
        let mut path = PathBuf::from("/other");
        // 不正な UTF-8 シーケンスを含む file_name
        path.push(OsStr::from_bytes(b"bad\xff.md"));
        let sanitized = sanitize_path_for_logging(&path, &base);
        // to_string_lossy が U+FFFD を挿入しても panic しないこと
        assert!(sanitized.starts_with("<outside-base>/bad"));
        assert!(sanitized.ends_with(".md"));
    }
}
```

- [ ] **Step 2: モジュール宣言を追加**

`src/server.rs` を編集:

```rust
mod broadcast;
mod files;
mod guards;
mod log_path;
mod messages;
mod routes;
mod session;
mod state;
mod watch;
```

（`mod log_path;` 行を `guards` と `messages` の間にアルファベット順で挿入）

- [ ] **Step 3: テスト単体実行で pass を確認**

Run: `cargo test --lib server::log_path 2>&1 | tail -20`

Expected: 6 件すべて pass。

- [ ] **Step 4: コミット**

```bash
git add src/server/log_path.rs src/server.rs
git commit -m "feat: 監査ログ用パスサニタイザ sanitize_path_for_logging を追加

変更内容:
- src/server/log_path.rs を新規追加
- sanitize_path_for_logging(path, base) を実装: base 配下は相対化、base 外は <outside-base>/{file_name} に丸める
- ユニットテスト 6 件追加（base 配下/直下/外/ルート/base 自体/非UTF8）

変更理由:
- 異常系 tracing::warn! が絶対パスを path.display() で吐いており、画面共有・バグ報告 copy-paste 経由でディレクトリ構造が漏出するリスクがある"
```

---

### Task 2: `src/server/files/resolve.rs` の `tracing::warn!` を sanitize 化

**Files:**
- Modify: `src/server/files/resolve.rs:154-356`

resolve.rs では `revalidate_single_file_target` のシグネチャ変更が必要。先にシグネチャ変更し、続けて 3 statements を置換する。

- [ ] **Step 1: モジュール先頭に use 文を追加**

`src/server/files/resolve.rs` の先頭 use 文ブロック（L1-9 付近）に以下を追加:

```rust
use crate::server::log_path::sanitize_path_for_logging;
```

- [ ] **Step 2: `revalidate_single_file_target` シグネチャに `base_dir` を追加**

L343-353（関数定義部）:

```rust
pub(super) fn revalidate_single_file_target(
    expected_path: &Path,
) -> Result<PathBuf, ResolveFileError> {
    let canonical = expected_path.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] 単一ファイルパス正規化失敗: {} ({})",
            expected_path.display(),
            error
        );
        ResolveFileError::NotFound
    })?;
```

を以下に置換:

```rust
pub(super) fn revalidate_single_file_target(
    expected_path: &Path,
    base_dir: &Path,
) -> Result<PathBuf, ResolveFileError> {
    let canonical = expected_path.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] 単一ファイルパス正規化失敗: {} ({})",
            sanitize_path_for_logging(expected_path, base_dir),
            error
        );
        ResolveFileError::NotFound
    })?;
```

- [ ] **Step 3: 呼び出し元 3 箇所を修正**

L162（`resolve_single_file_target`）:

```rust
    let validated_path = revalidate_single_file_target(file_path)?;
```

→ 

```rust
    let validated_path = revalidate_single_file_target(file_path, state.mode().base_dir())?;
```

L176（`resolve_change_target`）:

```rust
        revalidate_single_file_target(expected)?;
```

→

```rust
        revalidate_single_file_target(expected, state.mode().base_dir())?;
```

L193（`resolve_request_target`）:

```rust
        let canonical = revalidate_single_file_target(path).map_err(|error| {
```

→

```rust
        let canonical = revalidate_single_file_target(path, state.mode().base_dir()).map_err(|error| {
```

- [ ] **Step 4: `build_resolved_target` 内の warn を修正**

L255-260 付近:

```rust
        tracing::warn!(
            "[markdown-view] {}: {} はベース {} の配下ではありません",
            warn_label,
            file_path.display(),
            state.mode().base_dir().display()
        );
```

を以下に置換:

```rust
        tracing::warn!(
            "[markdown-view] {}: {} はベース {} の配下ではありません",
            warn_label,
            sanitize_path_for_logging(&file_path, state.mode().base_dir()),
            state.mode().base_dir().display()
        );
```

（`base_dir().display()` は **保持** — Q3-i-2 結論）

- [ ] **Step 5: `resolve_file` 内の warn を修正**

L300-304 付近:

```rust
    let canonical = candidate.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ファイルパス正規化失敗: {} ({})",
            candidate.display(),
            error
        );
        ResolveFileError::NotFound
    })?;
```

を以下に置換:

```rust
    let canonical = candidate.canonicalize().map_err(|error| {
        tracing::warn!(
            "[markdown-view] ファイルパス正規化失敗: {} ({})",
            sanitize_path_for_logging(&candidate, base_dir),
            error
        );
        ResolveFileError::NotFound
    })?;
```

L308-313 の `base_dir.canonicalize()` 失敗 warn は `base_dir.display()` を出しているため **変更しない**（保持対象）。

- [ ] **Step 6: ビルドとテスト**

Run: `cargo build 2>&1 | head -40`
Expected: 成功（type errors なし）。失敗時はメッセージから漏れた呼び出し元を特定し修正。

Run: `cargo test --lib server::files::resolve 2>&1 | tail -20`
Expected: 全 pass

- [ ] **Step 7: コミット**

```bash
git add src/server/files/resolve.rs
git commit -m "refactor: resolve.rs の異常系 warn ログを base 相対化

変更内容:
- revalidate_single_file_target に base_dir パラメータを追加
- 呼び出し元 3 箇所 (resolve_single_file_target, resolve_change_target, resolve_request_target) を更新
- 3 statements (build_resolved_target, resolve_file, revalidate_single_file_target) で path 側を sanitize_path_for_logging 経由に変更
- base_dir.display() 自体は保守性優先で保持

変更理由:
- 異常系 warn ログでディレクトリ構造が漏出するのを抑制"
```

---

### Task 3: `src/server/files/catalog.rs` の `tracing::warn!` を sanitize 化

**Files:**
- Modify: `src/server/files/catalog.rs:22-160`

catalog.rs は `list_markdown_files_recursive(base_dir, current_dir, ...)` で base_dir を引数で受けているので追加引数なしで sanitize 可能。`canonicalize_dir_for_cycle(path, label)` には base_dir が無いため、呼び出し元から base_dir を渡すよう拡張する。

- [ ] **Step 1: use 文を追加**

`src/server/files/catalog.rs` の L1-2:

```rust
use std::collections::HashSet;
use std::path::{Path, PathBuf};
```

の直後に以下を追加:

```rust
use crate::server::log_path::sanitize_path_for_logging;
```

- [ ] **Step 2: `canonicalize_dir_for_cycle` のシグネチャに base_dir を追加**

L148-161:

```rust
pub(super) fn canonicalize_dir_for_cycle(path: &Path, label: &str) -> Option<PathBuf> {
    match path.canonicalize() {
        Ok(canonical) => Some(canonical),
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}の正規化に失敗（スキップ）: {} ({})",
                label,
                path.display(),
                error
            );
            None
        }
    }
}
```

を以下に置換:

```rust
pub(super) fn canonicalize_dir_for_cycle(
    path: &Path,
    label: &str,
    base_dir: &Path,
) -> Option<PathBuf> {
    match path.canonicalize() {
        Ok(canonical) => Some(canonical),
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}の正規化に失敗（スキップ）: {} ({})",
                label,
                sanitize_path_for_logging(path, base_dir),
                error
            );
            None
        }
    }
}
```

- [ ] **Step 3: `canonicalize_dir_for_cycle` の 2 つの呼び出し元を修正**

L76（シンボリックリンク経路）:

```rust
                let Some(resolved) = canonicalize_dir_for_cycle(&path, "シンボリックリンク")
```

→

```rust
                let Some(resolved) = canonicalize_dir_for_cycle(&path, "シンボリックリンク", base_dir)
```

L107（通常ディレクトリ経路）:

```rust
                let Some(canonical) = canonicalize_dir_for_cycle(&path, "通常ディレクトリ")
```

→

```rust
                let Some(canonical) = canonicalize_dir_for_cycle(&path, "通常ディレクトリ", base_dir)
```

- [ ] **Step 4: `list_markdown_files_recursive` 内の 6 statements を sanitize 化**

L29-34（深度上限）:

```rust
    if depth >= MAX_DIR_DEPTH {
        tracing::warn!(
            "[markdown-view] ディレクトリ深度上限に到達（スキップ）: {}",
            current_dir.display()
        );
        return Ok(());
    }
```

→

```rust
    if depth >= MAX_DIR_DEPTH {
        tracing::warn!(
            "[markdown-view] ディレクトリ深度上限に到達（スキップ）: {}",
            sanitize_path_for_logging(current_dir, base_dir)
        );
        return Ok(());
    }
```

L42-47（read_dir エントリ失敗）:

```rust
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ディレクトリエントリ読み取りエラー（スキップ）: {} ({})",
                    current_dir.display(),
                    error
                );
                continue;
            }
```

→

```rust
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ディレクトリエントリ読み取りエラー（スキップ）: {} ({})",
                    sanitize_path_for_logging(current_dir, base_dir),
                    error
                );
                continue;
            }
```

L61-66（file_type 取得エラー）:

```rust
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ファイルタイプ取得エラー（スキップ）: {} ({})",
                    path.display(),
                    error
                );
                continue;
            }
```

→

```rust
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] ファイルタイプ取得エラー（スキップ）: {} ({})",
                    sanitize_path_for_logging(&path, base_dir),
                    error
                );
                continue;
            }
```

L83-89（ベース正規化失敗）— `base_dir.display()` 側なので **保持**:

```rust
                        tracing::warn!(
                            "[markdown-view] ベースディレクトリの正規化に失敗（スキップ）: {} ({})",
                            base_dir.display(),
                            error
                        );
```

→ 変更不要

L92-97（base 外 symlink）:

```rust
                if !resolved.starts_with(&canonical_base) {
                    tracing::warn!(
                        "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
                        path.display(),
                        resolved.display()
                    );
                    continue;
                }
```

→

```rust
                if !resolved.starts_with(&canonical_base) {
                    tracing::warn!(
                        "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
                        sanitize_path_for_logging(&path, base_dir),
                        sanitize_path_for_logging(&resolved, base_dir)
                    );
                    continue;
                }
```

L100-104（symlink サイクル検出）:

```rust
                if !visited_dirs.insert(resolved) {
                    tracing::warn!(
                        "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                        path.display()
                    );
                    continue;
                }
```

→

```rust
                if !visited_dirs.insert(resolved) {
                    tracing::warn!(
                        "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                        sanitize_path_for_logging(&path, base_dir)
                    );
                    continue;
                }
```

L135-139（相対パス算出不可）— **第1引数のみ sanitize、`base_dir.display()` は保持**:

```rust
                Err(_) => {
                    tracing::warn!(
                        "[markdown-view] 相対パス算出不可（スキップ）: {} (ベース: {})",
                        path.display(),
                        base_dir.display()
                    );
                }
```

→

```rust
                Err(_) => {
                    tracing::warn!(
                        "[markdown-view] 相対パス算出不可（スキップ）: {} (ベース: {})",
                        sanitize_path_for_logging(&path, base_dir),
                        base_dir.display()
                    );
                }
```

- [ ] **Step 5: ビルドとテスト**

Run: `cargo build 2>&1 | head -30`
Expected: 成功

Run: `cargo test --lib server::files::catalog 2>&1 | tail -10`
Expected: 既存テストが pass。catalog.rs に独自テストがない場合は次の cargo test で全 pass を確認。

Run: `cargo test --all-targets --all-features 2>&1 | tail -10`
Expected: 全 pass

- [ ] **Step 6: コミット**

```bash
git add src/server/files/catalog.rs
git commit -m "refactor: catalog.rs の異常系 warn ログを base 相対化

変更内容:
- canonicalize_dir_for_cycle に base_dir パラメータを追加（呼び出し元 2 箇所も更新）
- list_markdown_files_recursive 内 6 statements で path 側を sanitize_path_for_logging 経由に変更
- base_dir.display() 自体（L83 と L138）は保守性優先で保持

変更理由:
- 異常系 warn ログでディレクトリ構造が漏出するのを抑制"
```

---

### Task 4: `src/server/files/memo.rs` の `ensure_safe_memo_path` warn を sanitize 化

**Files:**
- Modify: `src/server/files/memo.rs:435-456`

memo.rs は **1 statement のみ**変更（他の warn は既に `target.file_label()` 経由で relative-safe）。

- [ ] **Step 1: use 文を追加**

`src/server/files/memo.rs` の use ブロックに以下を追加:

```rust
use crate::server::log_path::sanitize_path_for_logging;
```

- [ ] **Step 2: `ensure_safe_memo_path` 内の warn を修正**

L441-453 の該当ブロック:

```rust
    let base_dir = state.mode().base_dir();
    if let Some(unsafe_component) = first_symlink_component(base_dir, memo_path) {
        tracing::warn!(
            "[markdown-view] {}メモパスがシンボリックリンクを含むため拒否 ({} -> {}): {}",
            request.read_error_log_label(),
            target.file_label(),
            memo_path.display(),
            unsafe_component.display()
        );
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "メモ保存先にシンボリックリンクが含まれているため操作できません",
        ));
    }
```

を以下に置換:

```rust
    let base_dir = state.mode().base_dir();
    if let Some(unsafe_component) = first_symlink_component(base_dir, memo_path) {
        tracing::warn!(
            "[markdown-view] {}メモパスがシンボリックリンクを含むため拒否 ({} -> {}): {}",
            request.read_error_log_label(),
            target.file_label(),
            sanitize_path_for_logging(memo_path, base_dir),
            sanitize_path_for_logging(&unsafe_component, base_dir)
        );
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "メモ保存先にシンボリックリンクが含まれているため操作できません",
        ));
    }
```

- [ ] **Step 3: ビルドとテスト**

Run: `cargo build 2>&1 | head -20`
Expected: 成功

Run: `cargo test --lib server::files::memo 2>&1 | tail -10`
Expected: 全 pass

- [ ] **Step 4: コミット**

```bash
git add src/server/files/memo.rs
git commit -m "refactor: memo.rs の symlink 拒否 warn ログを base 相対化

変更内容:
- ensure_safe_memo_path 内で memo_path と unsafe_component を sanitize_path_for_logging 経由に変更

変更理由:
- 異常系 warn ログでディレクトリ構造が漏出するのを抑制"
```

---

### Task 5: `src/server/files/content.rs` の file_label fallback を sanitize 化

**Files:**
- Modify: `src/server/files/content.rs:170-182`

- [ ] **Step 1: use 文を追加**

`src/server/files/content.rs` の use ブロックに以下を追加:

```rust
use crate::server::log_path::sanitize_path_for_logging;
```

- [ ] **Step 2: file_label 構築式を修正**

L170-182 の該当ブロック:

```rust
        ValidateRenderOutcome::ResolveFailed(error) => {
            let file_label = state
                .mode()
                .single_file()
                .map(file_display_name)
                .unwrap_or_else(|| changed_file.display().to_string());
            tracing::warn!(
                "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
                file_label,
                error
            );
```

を以下に置換:

```rust
        ValidateRenderOutcome::ResolveFailed(error) => {
            let file_label = state
                .mode()
                .single_file()
                .map(file_display_name)
                .unwrap_or_else(|| {
                    sanitize_path_for_logging(changed_file, state.mode().base_dir()).into_owned()
                });
            tracing::warn!(
                "[markdown-view] 更新時ファイル検証失敗 ({}): {}",
                file_label,
                error
            );
```

- [ ] **Step 3: ビルドとテスト**

Run: `cargo build 2>&1 | head -20`
Expected: 成功

Run: `cargo test --lib server::files::content 2>&1 | tail -10`
Expected: 全 pass

- [ ] **Step 4: コミット**

```bash
git add src/server/files/content.rs
git commit -m "refactor: content.rs の file_label fallback を base 相対化

変更内容:
- 更新時ファイル検証失敗 warn の file_label fallback を sanitize_path_for_logging 経由に変更

変更理由:
- 異常系 warn ログでディレクトリ構造が漏出するのを抑制"
```

---

### Task 6: `src/server/routes.rs` の index 描画 warn を `file_label()` 経由に変更

**Files:**
- Modify: `src/server/routes.rs:200-213`

routes.rs では既に `context.target()` から `file_label()`（relative-safe）を取得できるため、`sanitize_path_for_logging` を使わず最簡置換で対応する。

- [ ] **Step 1: warn 内の `file_path().display()` を `file_label()` に置換**

L206-210 の該当ブロック:

```rust
        Err(error) => {
            tracing::warn!(
                "[markdown-view] index描画ではメモ読み込み失敗を空メモへフォールバック ({}): {:?}",
                context.target().file_path().display(),
                error
            );
            MemoResponse::empty(context.target().relative_path().map(ToOwned::to_owned))
        }
```

を以下に置換:

```rust
        Err(error) => {
            tracing::warn!(
                "[markdown-view] index描画ではメモ読み込み失敗を空メモへフォールバック ({}): {:?}",
                context.target().file_label(),
                error
            );
            MemoResponse::empty(context.target().relative_path().map(ToOwned::to_owned))
        }
```

- [ ] **Step 2: `ResolvedTarget::file_label()` の可視性を確認**

Run: `grep -n 'fn file_label' src/server/files/resolve.rs`
Expected: `pub(super) fn file_label(&self) -> &str` が見つかる。`pub(super)` は同 `files` モジュール内のみ可視。`routes.rs` から呼ぶには可視性を `pub(in crate::server)` に拡張する必要がある。

- [ ] **Step 3: 必要なら `file_label` の可視性を拡張**

`src/server/files/resolve.rs:50-52` の以下:

```rust
    pub(super) fn file_label(&self) -> &str {
        &self.file_label
    }
```

を以下に置換:

```rust
    pub(in crate::server) fn file_label(&self) -> &str {
        &self.file_label
    }
```

- [ ] **Step 4: ビルドとテスト**

Run: `cargo build 2>&1 | head -20`
Expected: 成功

Run: `cargo test --all-targets --all-features 2>&1 | tail -10`
Expected: 全 pass

- [ ] **Step 5: コミット**

```bash
git add src/server/files/resolve.rs src/server/routes.rs
git commit -m "refactor: routes.rs の index warn ログを file_label 経由に変更

変更内容:
- index 描画時のメモ読み込み失敗 warn で file_path().display() を file_label() に置換
- ResolvedTarget::file_label の可視性を pub(super) から pub(in crate::server) に拡張

変更理由:
- file_label は既に relative-safe で、同等情報を漏出なく出力できるため最簡置換とする"
```

---

### Task 7: `src/watcher/strategy.rs` の 5 statements を sanitize 化

**Files:**
- Modify: `src/watcher/strategy.rs:117-267`

watcher は `src/server` の外側にあるため、`crate::server::log_path` を `pub(crate)` として参照する。`is_hidden_relative` および `is_within_base_dir` は既に `base` を引数で受けている。`is_target_file` は `target_path` を base として擬似的に使う必要があるため、関数全体に base 概念を渡す。

- [ ] **Step 1: use 文を追加**

`src/watcher/strategy.rs` の use ブロック（L4 付近）に以下を追加:

```rust
use crate::server::log_path::sanitize_path_for_logging;
```

- [ ] **Step 2: `collect_directory_changes` 内の warn を修正（L131-136）**

該当ブロック:

```rust
        if !is_within_base_dir(&event.path, base_dir) {
            tracing::warn!(
                "[markdown-view] ベースディレクトリ外のパスを検出（スキップ）: {}",
                event.path.display()
            );
            continue;
        }
```

→

```rust
        if !is_within_base_dir(&event.path, base_dir) {
            tracing::warn!(
                "[markdown-view] ベースディレクトリ外のパスを検出（スキップ）: {}",
                sanitize_path_for_logging(&event.path, base_dir)
            );
            continue;
        }
```

- [ ] **Step 3: `is_hidden_relative` 内の path 側 2 statements を修正（L171-174, L193-196）**

L168-176 の該当ブロック:

```rust
            let canonical_path = match path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: パス正規化失敗（元パスで再試行）: {} ({})",
                        path.display(), e
                    );
                    path.to_path_buf()
                }
            };
```

→

```rust
            let canonical_path = match path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: パス正規化失敗（元パスで再試行）: {} ({})",
                        sanitize_path_for_logging(path, base), e
                    );
                    path.to_path_buf()
                }
            };
```

L178-187 の `base.canonicalize()` 失敗 warn は **`base.display()` 側のため変更しない**（保持対象）。

L188-198（最終 strip_prefix 失敗）:

```rust
                Err(_) => {
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: 相対パス算出不可（安全側で除外）: {}",
                        path.display()
                    );
                    true
                }
```

→

```rust
                Err(_) => {
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: 相対パス算出不可（安全側で除外）: {}",
                        sanitize_path_for_logging(path, base)
                    );
                    true
                }
```

- [ ] **Step 4: `is_target_file` 内の warn を修正（L210-216）**

該当ブロック:

```rust
        Err(e) => {
            tracing::warn!(
                "[markdown-view] パス正規化に失敗（ファイル名比較にフォールバック）: {} ({})",
                event_path.display(),
                e
            );
```

`is_target_file(event_path, target_path)` は base 概念がないため、target_path の **parent** を擬似 base として使う。target_path 自体が base 配下とは限らないため、parent を取れない場合は `Path::new("")` を渡してフォールバック（strip_prefix 必ず失敗 → file_name のみ表示）。

修正後:

```rust
        Err(e) => {
            let log_base: &Path = target_path.parent().unwrap_or_else(|| Path::new(""));
            tracing::warn!(
                "[markdown-view] パス正規化に失敗（ファイル名比較にフォールバック）: {} ({})",
                sanitize_path_for_logging(event_path, log_base),
                e
            );
```

- [ ] **Step 5: `is_within_base_dir` 内の warn を修正（L256-261）**

該当ブロック:

```rust
        Err(e) => {
            tracing::warn!(
                "[markdown-view] ベース配下判定: パス正規化失敗（相対化で再試行）: {} ({})",
                path.display(),
                e
            );
```

→

```rust
        Err(e) => {
            tracing::warn!(
                "[markdown-view] ベース配下判定: パス正規化失敗（相対化で再試行）: {} ({})",
                sanitize_path_for_logging(path, base),
                e
            );
```

- [ ] **Step 6: ビルドとテスト**

Run: `cargo build 2>&1 | head -30`
Expected: 成功

Run: `cargo test --lib watcher 2>&1 | tail -20`
Expected: 既存 watcher テストが全 pass

Run: `cargo test --all-targets --all-features 2>&1 | tail -10`
Expected: 全 pass

- [ ] **Step 7: コミット**

```bash
git add src/watcher/strategy.rs
git commit -m "refactor: watcher/strategy.rs の異常系 warn ログを base 相対化

変更内容:
- collect_directory_changes / is_hidden_relative / is_target_file / is_within_base_dir 内の path 側 5 statements を sanitize_path_for_logging 経由に変更
- is_target_file では target_path.parent() を擬似 base として使用
- base.display() 側 (L183) は保守性優先で保持

変更理由:
- 異常系 warn ログでディレクトリ構造が漏出するのを抑制"
```

---

### Task 8: 統合検証と網羅確認

**Files:** 追加修正なし、確認のみ

- [ ] **Step 1: `.display()` 残存箇所の grep 確認**

Run:
```bash
grep -rn '\.display()' src/server/files src/watcher --include='*.rs' | grep -v 'mod tests\|#\[test\]\|^[^:]*test[^:]*:'
```

Expected: 残存する `.display()` は以下のみ:
- `src/server/files/resolve.rs:259` `state.mode().base_dir().display()` (保持)
- `src/server/files/resolve.rs:311` `base_dir.display()` (保持)
- `src/server/files/resolve.rs:417` `path.display().to_string()` （`file_display_name` 内 fallback）
- `src/server/files/catalog.rs:85` `base_dir.display()` (保持)
- `src/server/files/catalog.rs:138` `base_dir.display()` (保持)
- `src/watcher/strategy.rs:183` `base.display()` (保持)

それ以外が残っていれば該当 Task に戻る。

- [ ] **Step 2: lint と format**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: pass

Run: `cargo fmt --all -- --check`
Expected: pass

- [ ] **Step 3: 統合検証**

Run: `./verify.sh`
Expected: 全 phase pass

- [ ] **Step 4: 手動起動確認**

任意のディレクトリで起動:

```bash
cargo run -- /path/to/markdown/dir --port 18080
```

別シェルで base 外パスをトラバーサル試行:

```bash
curl -s -i 'http://127.0.0.1:18080/api/content?file=../../../etc/passwd' -H 'Host: localhost:18080'
```

サーバーログを確認し、`[markdown-view] ファイルパス正規化失敗:` の warn 出力に **絶対パスが含まれていないこと**を確認する（具体的には `<outside-base>/...` 形式または短い相対パスになっていること）。

確認後 Ctrl+C で停止。

- [ ] **Step 5: 必要に応じて clippy 自動修正の追加コミット**

clippy/fmt で差分が出た場合のみ:

```bash
cargo fmt --all
git add -u
git commit -m "chore: cargo fmt による自動整形"
```

差分なしならスキップ。

---

## Self-Review チェックリスト（実装担当者向け）

実装後、以下を確認:

- [ ] `cargo test --all-targets --all-features` が全 pass
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` が pass
- [ ] `cargo fmt --all -- --check` が pass
- [ ] `./verify.sh` が pass
- [ ] Task 8 Step 1 の grep で残存 `.display()` が保持対象のみ
- [ ] 手動 curl で異常系 warn ログに絶対パスが含まれない

---

## 完了後

`feat/path-log-sanitizer` ブランチを develop に **squash merge**。マージ後ローカルブランチ削除。
