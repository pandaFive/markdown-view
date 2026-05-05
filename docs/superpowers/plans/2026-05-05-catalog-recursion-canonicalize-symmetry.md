# Catalog Recursion Canonicalize Symmetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `catalog.rs` の通常ディレクトリとシンボリックリンクディレクトリの再帰可否判定を同じ helper に集約する。

**Architecture:** `list_markdown_files_recursive` はエントリ種別の大枠だけを見て、正規化、base 配下確認、metadata によるディレクトリ判定、visited 登録を `resolve_recursable_directory` に委譲する。ファイル一覧 API と検索 API の公開契約は変えず、既存の安全側スキップとログ方針を維持する。

**Tech Stack:** Rust, std filesystem, tempfile, Unix symlink tests, cargo test, clippy, `./verify.sh`

---

## File Structure

- Modify: `src/server/files/catalog.rs`
  - `resolve_recursable_directory` を追加する。
  - `list_markdown_files_recursive` の通常ディレクトリ/symlink 分岐を helper 呼び出しへ寄せる。
  - `canonicalize_dir_for_cycle` は正規化失敗時の共通ログ helper として維持する。
- Modify: `src/server/files/tests.rs`
  - helper 契約テストを追加する。
  - 既存の base 外 symlink、base 内 symlink、symlink cycle、自己参照 symlink、基本列挙テストを回帰確認に使う。

## Preflight

- [ ] **Step 1: ブランチと差分を確認する**

Run:

```bash
git status --short --branch
git log --oneline -3
```

Expected: `docs/catalog-recursion-canonicalize-symmetry` 上にいて、作業ツリーが clean。直近に `docs: catalog再帰判定対称化の設計を追加` がある。

- [ ] **Step 2: spec と plan を読む**

Run:

```bash
sed -n '1,180p' docs/superpowers/specs/2026-05-05-catalog-recursion-canonicalize-symmetry-design.md
sed -n '1,260p' docs/superpowers/plans/2026-05-05-catalog-recursion-canonicalize-symmetry.md
```

Expected: 目的、非目的、受け入れ条件、セキュリティ考慮が一致している。

### Task 1: Recursable Directory Helper Contract

**Files:**
- Modify: `src/server/files/catalog.rs`
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: helper 契約の失敗テストを書く**

`src/server/files/tests.rs` の catalog 周辺テスト、既存の `test_list_markdown_files_recursive_通常ディレクトリcanonicalize失敗時はスキップ扱い` の直後に次のテストを追加する。

```rust
#[test]
fn test_resolve_recursable_directory_通常ディレクトリをvisitedに登録する() {
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("child");
    std::fs::create_dir(&child).unwrap();
    let canonical_base = dir.path().canonicalize().unwrap();
    let mut visited_dirs = std::collections::HashSet::new();
    visited_dirs.insert(canonical_base.clone());

    let resolved = super::catalog::resolve_recursable_directory(
        &child,
        false,
        &canonical_base,
        &mut visited_dirs,
        dir.path(),
    )
    .expect("通常ディレクトリは再帰対象になる");

    assert_eq!(resolved, child.canonicalize().unwrap());
    assert!(visited_dirs.contains(&resolved));
}

#[cfg(unix)]
#[test]
fn test_resolve_recursable_directory_base外symlinkはvisitedに登録しない() {
    let base = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let link = base.path().join("linked");
    symlink(outside.path(), &link).unwrap();
    let canonical_base = base.path().canonicalize().unwrap();
    let outside_canonical = outside.path().canonicalize().unwrap();
    let mut visited_dirs = std::collections::HashSet::new();
    visited_dirs.insert(canonical_base.clone());

    let resolved = super::catalog::resolve_recursable_directory(
        &link,
        true,
        &canonical_base,
        &mut visited_dirs,
        base.path(),
    );

    assert!(resolved.is_none());
    assert!(!visited_dirs.contains(&outside_canonical));
}
```

- [ ] **Step 2: 失敗を確認する**

Run:

```bash
cargo test --all-targets --all-features test_resolve_recursable_directory -- --nocapture
```

Expected: `resolve_recursable_directory` が未定義のため FAIL。

- [ ] **Step 3: helper を最小実装する**

`src/server/files/catalog.rs` の `canonicalize_dir_for_cycle` の前に次の helper を追加する。

```rust
pub(super) fn resolve_recursable_directory(
    path: &Path,
    is_symlink: bool,
    canonical_base_dir: &Path,
    visited_dirs: &mut HashSet<PathBuf>,
    log_base_dir: &Path,
) -> Option<PathBuf> {
    let label = if is_symlink {
        "シンボリックリンク"
    } else {
        "通常ディレクトリ"
    };
    let resolved = canonicalize_dir_for_cycle(path, label, log_base_dir)?;

    if is_symlink && !resolved.starts_with(canonical_base_dir) {
        tracing::warn!(
            "[markdown-view] ベースディレクトリ外を指すシンボリックリンク（スキップ）: {} -> {}",
            sanitize_path_for_logging(path, log_base_dir),
            sanitize_path_for_logging(&resolved, log_base_dir)
        );
        return None;
    }

    match std::fs::metadata(&resolved) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) if is_symlink => {
            tracing::debug!(
                "[markdown-view] シンボリックリンクが通常ファイルを指すためスキップ: {} -> {}",
                path.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_else(|| path.as_os_str().to_string_lossy()),
                sanitize_path_for_logging(&resolved, log_base_dir)
            );
            return None;
        }
        Ok(_) => {
            tracing::warn!(
                "[markdown-view] 通常ディレクトリの正規化先がディレクトリではないためスキップ: {} -> {}",
                sanitize_path_for_logging(path, log_base_dir),
                sanitize_path_for_logging(&resolved, log_base_dir)
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(
                "[markdown-view] {}のメタデータ取得に失敗（スキップ）: {} ({})",
                label,
                sanitize_path_for_logging(&resolved, log_base_dir),
                error
            );
            return None;
        }
    }

    if !visited_dirs.insert(resolved.clone()) {
        if is_symlink {
            tracing::warn!(
                "[markdown-view] シンボリックリンクのサイクルを検出（スキップ）: {}",
                sanitize_path_for_logging(path, log_base_dir)
            );
        }
        return None;
    }

    Some(resolved)
}
```

- [ ] **Step 4: helper 契約テストを通す**

Run:

```bash
cargo test --all-targets --all-features test_resolve_recursable_directory -- --nocapture
```

Expected: 追加した helper 契約テストが PASS。

- [ ] **Step 5: Task 1 を commit する**

Run:

```bash
git add src/server/files/catalog.rs src/server/files/tests.rs
git commit -m "test: catalog再帰判定helperの契約を追加"
```

Expected: helper と helper 契約テストだけが commit される。

### Task 2: Use Helper From Recursive File Listing

**Files:**
- Modify: `src/server/files/catalog.rs`
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: 既存挙動の局所テストを先に実行する**

Run:

```bash
cargo test --all-targets --all-features test_list_markdown_files -- --nocapture
```

Expected: 既存の Markdown ファイル一覧、ソート、最大件数、深度上限、symlink 系テストが PASS。

- [ ] **Step 2: `list_markdown_files_recursive` の分岐を helper 呼び出しへ置き換える**

`src/server/files/catalog.rs` の `if file_type.is_dir() || file_type.is_symlink()` ブロック内で、`if file_type.is_symlink() { ... } else { ... }` の分岐全体を削除し、次の形へ置き換える。

```rust
            if resolve_recursable_directory(
                &path,
                file_type.is_symlink(),
                canonical_base_dir,
                visited_dirs,
                log_base_dir,
            )
            .is_none()
            {
                continue;
            }
```

置き換え後のブロック全体は次の形になる。

```rust
        if file_type.is_dir() || file_type.is_symlink() {
            if files.len() >= max_files {
                return Ok(());
            }

            if resolve_recursable_directory(
                &path,
                file_type.is_symlink(),
                canonical_base_dir,
                visited_dirs,
                log_base_dir,
            )
            .is_none()
            {
                continue;
            }

            list_markdown_files_recursive(
                log_base_dir,
                canonical_base_dir,
                &path,
                files,
                visited_dirs,
                depth + 1,
                max_files,
            )?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
```

- [ ] **Step 3: symlink と通常列挙の回帰テストを実行する**

Run:

```bash
cargo test --all-targets --all-features test_list_markdown_files_from_canonical_base -- --nocapture
cargo test --all-targets --all-features test_list_markdown_files_シンボリックリンク -- --nocapture
```

Expected: base 外 symlink は除外、base 内 symlink ディレクトリは列挙、symlink cycle は列挙されない。

- [ ] **Step 4: catalog 周辺の広めのテストを実行する**

Run:

```bash
cargo test --all-targets --all-features test_list_markdown_files -- --nocapture
cargo test --all-targets --all-features test_search_directory_canonical_base_再canonicalizeなしで検索する
```

Expected: list/search の public 経路が PASS。

- [ ] **Step 5: Task 2 を commit する**

Run:

```bash
git add src/server/files/catalog.rs src/server/files/tests.rs
git commit -m "refactor: catalog再帰判定をhelperに集約"
```

Expected: `list_markdown_files_recursive` の分岐整理が commit される。

### Task 3: Final Verification And TODO Update

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: full verification を実行する**

Run:

```bash
./verify.sh
```

Expected: format、clippy、Rust tests がすべて PASS。

- [ ] **Step 2: TODO を完了済みに移す**

`docs/todo/TODO.md` の Medium Priority から次の項目を削除する。

```markdown
- [ ] `canonicalize` 失敗時の再帰挙動の非対称を解消する
```

同じファイルの Done Summary へ次の項目を追加する。

```markdown
- [x] `canonicalize` 失敗時の再帰挙動の非対称を解消する
  - 完了根拠: `catalog.rs` の通常ディレクトリとシンボリックリンクディレクトリの再帰可否判定を `resolve_recursable_directory` へ集約し、正規化、base 配下確認、metadata によるディレクトリ判定、visited 登録を同じ経路に揃えた。base 外 symlink、通常ファイル symlink、symlink cycle、基本列挙の回帰テストで固定している
```

- [ ] **Step 3: docs-only TODO 変更を確認する**

Run:

```bash
rg -n "canonicalize.*再帰挙動|resolve_recursable_directory" docs/todo/TODO.md src/server/files/catalog.rs src/server/files/tests.rs
git diff -- docs/todo/TODO.md
```

Expected: Medium Priority から対象項目が消え、Done Summary に完了根拠がある。

- [ ] **Step 4: TODO 更新を commit する**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: catalog再帰判定TODOを完了済みに移動"
```

Expected: TODO 更新だけが commit される。

- [ ] **Step 5: 最終状態を確認する**

Run:

```bash
git status --short --branch
git log --oneline -5
```

Expected: 作業ツリーが clean。実装 commit と TODO 更新 commit が並んでいる。

## Self-Review Checklist

- Spec coverage: helper 集約、canonicalize 失敗時スキップ、base 外 symlink 拒否、通常ファイル symlink 除外、cycle 防止、既存一覧挙動維持を Task 1-3 で扱う。
- Placeholder scan: plan 内に未確定の `TBD`、`TODO`、抽象的な「適切に処理する」は残さない。
- Type consistency: helper 名は `resolve_recursable_directory`、引数は `(&Path, bool, &Path, &mut HashSet<PathBuf>, &Path)` に統一する。
- Security: base 外 symlink、TOCTOU、正規化失敗、サイクル検出を未信頼入力として扱い、安全側スキップを維持する。
