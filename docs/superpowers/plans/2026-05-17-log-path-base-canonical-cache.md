# log_path Base Canonicalize Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `log_path` の base canonicalize 結果を再利用できる型を追加し、監査ログの安全契約を維持したまま watcher の繰り返しログ整形で余分な syscall を減らす。

**Architecture:** `src/server/log_path.rs` に `LogBasePath` を追加し、既存の `sanitize_path_for_logging` API は互換層として残す。`src/watcher/strategy.rs` は `WatchStrategy::path_for_log` の繰り返し呼び出しで `LogBasePath` を使えるようにする範囲へ限定し、`server/files/*` の単発ログ呼び出しは変更しない。

**Tech Stack:** Rust, std::path, tempfile, cargo test, repository-local `./verify.sh`.

---

## Files

- Modify: `src/server/log_path.rs`
  - 責務: 監査ログ用の path 相対化、base 外 path の秘匿、制御文字 escape。
  - 今回の変更: `LogBasePath` を追加し、cached canonical base を使う sanitize 経路と unit test を追加する。
- Modify: `src/watcher/strategy.rs`
  - 責務: watcher の監視対象判定、ログ用 path 表示、監視計画生成。
  - 今回の変更: `WatchStrategy` が `LogBasePath` を保持し、`path_for_log` で同じ cached base を再利用する。
- Modify: `docs/todo/BACKLOG.md`
  - 責務: 未完了の低優先・長期改善候補を保持する。
  - 今回の変更: `log_path::canonicalize_status` 項目を完了扱いにして `docs/done/DONE-2026-05.md` へ移すか、少なくとも完了根拠を追記して未完了一覧から外す。
- Reference only: `docs/superpowers/specs/2026-05-17-log-path-base-canonical-cache-design.md`
  - 責務: 承認済み設計。実装中に内容を変更しない。

## Task 1: `LogBasePath` の失敗テストを追加する

**Files:**
- Modify: `src/server/log_path.rs`

- [ ] **Step 1: 既存の log_path テストだけを実行して現状を確認する**

Run:

```bash
cargo test --all-targets --all-features log_path
```

Expected: PASS。もし既存失敗がある場合は、この計画の実装前に失敗内容を記録してユーザーへ確認する。

- [ ] **Step 2: `LogBasePath` の unit test を追加する**

In `src/server/log_path.rs`, inside `#[cfg(test)] mod tests`, add these tests after `test_sanitize_path自体がbaseの場合` and before `test_sanitize_非utf8_file_name`:

```rust
    #[test]
    fn test_log_base_path_cached_baseでもbase配下を相対パスにする() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        std::fs::create_dir_all(base.join("subdir")).unwrap();
        std::fs::write(base.join("subdir/file.md"), "# doc").unwrap();
        let log_base = LogBasePath::new(&base);

        let sanitized = log_base.sanitize(&base.join("subdir/file.md"));

        assert_eq!(sanitized, "subdir/file.md");
    }

    #[cfg(unix)]
    #[test]
    fn test_log_base_path_cached_baseでもsymlink経由のbase外はoutside扱い() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(outside.join("private")).unwrap();
        std::fs::write(outside.join("private/doc.md"), "# doc").unwrap();
        symlink(&outside, base.join("link")).unwrap();
        let log_base = LogBasePath::new(&base);

        let sanitized = log_base.sanitize(&base.join("link/private/doc.md"));

        assert_eq!(sanitized, "<outside-base>/doc.md");
    }

    #[cfg(unix)]
    #[test]
    fn test_log_base_path_cached_baseでもsymlink経由のbase配下は相対化する() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        let nested = base.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("doc.md"), "# doc").unwrap();
        symlink(&nested, base.join("link")).unwrap();
        let log_base = LogBasePath::new(&base);

        let sanitized = log_base.sanitize(&base.join("link/doc.md"));

        assert_eq!(sanitized, "nested/doc.md");
    }

    #[test]
    fn test_log_base_path_base正規化失敗時はlexical_fallbackで継続する() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("missing-base");
        let path = base.join("docs/../file.md");
        let log_base = LogBasePath::new(&base);

        let sanitized = log_base.sanitize(&path);

        assert_eq!(sanitized, "file.md");
    }

    #[test]
    fn test_log_base_path_escapedは制御文字を可視化する() {
        let base = PathBuf::from("/base");
        let log_base = LogBasePath::new(&base);
        let path = base.join("line\n\x1b.md");

        let sanitized = log_base.sanitize_escaped(&path);

        assert_eq!(sanitized, "line\\n\\u{1b}.md");
        assert!(!sanitized.contains('\n'));
        assert!(!sanitized.contains('\x1b'));
    }
```

- [ ] **Step 3: 新規テストが未実装で失敗することを確認する**

Run:

```bash
cargo test --all-targets --all-features log_base_path
```

Expected: FAIL。代表的な失敗は `use of undeclared type LogBasePath` または `failed to resolve: use of undeclared type LogBasePath`。

## Task 2: `LogBasePath` を実装して既存 API を互換維持する

**Files:**
- Modify: `src/server/log_path.rs`

- [ ] **Step 1: `LogBasePath` と cached canonical 判定を追加する**

In `src/server/log_path.rs`, replace the existing top-level functions and canonical helpers from `pub(crate) fn sanitize_path_for_logging` through `fn canonicalize_status` with:

```rust
/// 監査ログ用 base path。base の canonicalize 結果を再利用する。
#[derive(Debug, Clone)]
pub(crate) struct LogBasePath {
    base: PathBuf,
    canonical_base: Option<PathBuf>,
}

impl LogBasePath {
    pub(crate) fn new(base: &Path) -> Self {
        Self {
            base: base.to_path_buf(),
            canonical_base: base.canonicalize().ok(),
        }
    }

    /// 監査ログ用にパスを base 相対化する。
    pub(crate) fn sanitize<'a>(&self, path: &'a Path) -> Cow<'a, str> {
        match self.canonicalize_status(path) {
            CanonicalizeStatus::Relative(relative) if relative.as_os_str().is_empty() => {
                Cow::Borrowed(".")
            }
            CanonicalizeStatus::Relative(relative) => Cow::Owned(relative.display().to_string()),
            CanonicalizeStatus::OutsideBase => sanitize_outside_path_for_logging(path),
            CanonicalizeStatus::Unavailable => sanitize_path_for_logging_lexical(path, &self.base),
        }
    }

    /// 監査ログ用 path を相対化し、制御文字を可視化する。
    pub(crate) fn sanitize_escaped(&self, path: &Path) -> String {
        self.sanitize(path).as_ref().escape_debug().to_string()
    }

    fn canonicalize_status(&self, path: &Path) -> CanonicalizeStatus {
        let Some(canonical_base) = &self.canonical_base else {
            return CanonicalizeStatus::Unavailable;
        };
        let canonical_path = match path.canonicalize() {
            Ok(path) => path,
            Err(_) => return CanonicalizeStatus::Unavailable,
        };
        match canonical_path.strip_prefix(canonical_base) {
            Ok(relative) => CanonicalizeStatus::Relative(relative.to_path_buf()),
            Err(_) => CanonicalizeStatus::OutsideBase,
        }
    }
}

/// 監査ログ用にパスを base 相対化する。
///
/// - `path` が `base` 配下: 相対パス文字列（例: `"subdir/file.md"`）
/// - `path == base`: `"."
/// - `path` が `base` 外: `<outside-base>/{file_name}`
/// - `file_name` 取得不可（ルート等）: `<outside-base>`
///
/// 存在するパスでは canonicalize 後の実パスで base 配下判定を優先し、
/// symlink 経由で base 外へ出るパスの誤判定を防ぐ。
pub(crate) fn sanitize_path_for_logging<'a>(path: &'a Path, base: &Path) -> Cow<'a, str> {
    LogBasePath::new(base).sanitize(path)
}

/// 監査ログ用 path を相対化し、制御文字を可視化する。
pub(crate) fn sanitize_path_for_logging_escaped(path: &Path, base: &Path) -> String {
    LogBasePath::new(base).sanitize_escaped(path)
}

/// 監査ログ用 path を字句的に相対化し、制御文字を可視化する。
pub(crate) fn sanitize_path_for_logging_lexical_escaped(path: &Path, base: &Path) -> String {
    sanitize_path_for_logging_lexical(path, base)
        .as_ref()
        .escape_debug()
        .to_string()
}

enum CanonicalizeStatus {
    Relative(PathBuf),
    OutsideBase,
    Unavailable,
}
```

Important: keep the rest of the file unchanged, including `sanitize_path_for_logging_lexical`, `sanitize_outside_path_for_logging`, `normalize_lexical_path`, and existing tests.

- [ ] **Step 2: Fix the doc comment typo before running tests**

In the code inserted above, fix this doc line:

```rust
/// - `path == base`: `"."
```

to:

```rust
/// - `path == base`: `"."`
```

- [ ] **Step 3: Run focused tests**

Run:

```bash
cargo test --all-targets --all-features log_path
```

Expected: PASS. The new `test_log_base_path_*` tests and existing `test_sanitize_*` tests pass.

- [ ] **Step 4: Format**

Run:

```bash
cargo fmt --all
```

Expected: command exits 0. If formatting changes `src/server/log_path.rs`, keep those changes.

- [ ] **Step 5: Commit `log_path` implementation**

Run:

```bash
git add src/server/log_path.rs
git commit -m "refactor: log_pathのbase正規化結果を再利用"
```

Expected: commit succeeds and includes only `src/server/log_path.rs`.

## Task 3: watcher の `path_for_log` で held cached base を使う

**Files:**
- Modify: `src/watcher/strategy.rs`

- [ ] **Step 1: Import `LogBasePath`**

In `src/watcher/strategy.rs`, replace:

```rust
use crate::server::log_path::sanitize_path_for_logging;
```

with:

```rust
use crate::server::log_path::{sanitize_path_for_logging, LogBasePath};
```

- [ ] **Step 2: `WatchStrategy` の variant に cached log base を追加する**

Replace the enum definition:

```rust
pub(super) enum WatchStrategy {
    SingleFile { target_path: CanonicalPath },
    Directory { base_dir: CanonicalPath },
}
```

with:

```rust
pub(super) enum WatchStrategy {
    SingleFile {
        target_path: CanonicalPath,
        log_base: LogBasePath,
    },
    Directory {
        base_dir: CanonicalPath,
        log_base: LogBasePath,
    },
}
```

- [ ] **Step 3: `WatchStrategy` の constructor helper を追加する**

In `impl WatchStrategy`, before `pub(super) fn from_mode(mode: &AppMode) -> Result<Self>`, add:

```rust
    fn single_file(target_path: CanonicalPath) -> Self {
        let base = target_path
            .as_path()
            .parent()
            .unwrap_or_else(|| target_path.as_path());
        Self::SingleFile {
            log_base: LogBasePath::new(base),
            target_path,
        }
    }

    fn directory(base_dir: CanonicalPath) -> Self {
        Self::Directory {
            log_base: LogBasePath::new(base_dir.as_path()),
            base_dir,
        }
    }
```

- [ ] **Step 4: `from_mode` を helper 経由にする**

Replace:

```rust
        if let Some(file_path) = mode.single_file_canonical() {
            Ok(Self::SingleFile {
                target_path: file_path.clone(),
            })
        } else if let Some(dir_path) = mode.directory_canonical() {
            Ok(Self::Directory {
                base_dir: dir_path.clone(),
            })
        } else {
            anyhow::bail!("未知のAppModeです")
        }
```

with:

```rust
        if let Some(file_path) = mode.single_file_canonical() {
            Ok(Self::single_file(file_path.clone()))
        } else if let Some(dir_path) = mode.directory_canonical() {
            Ok(Self::directory(dir_path.clone()))
        } else {
            anyhow::bail!("未知のAppModeです")
        }
```

- [ ] **Step 5: enum pattern matches に `..` を追加する**

In `src/watcher/strategy.rs`, update every `WatchStrategy` match arm that destructures a variant but does not need `log_base`.

Use these replacements:

```rust
Self::SingleFile { target_path } => {
```

becomes:

```rust
Self::SingleFile { target_path, .. } => {
```

```rust
Self::SingleFile { .. } => Vec::new(),
```

stays unchanged.

```rust
Self::Directory { base_dir } => {
```

becomes:

```rust
Self::Directory { base_dir, .. } => {
```

```rust
Self::Directory { .. } => "ディレクトリ監視エラー",
```

stays unchanged.

- [ ] **Step 6: `path_for_log` を held cached base 経由にする**

Replace the current `path_for_log` body:

```rust
    pub(super) fn path_for_log(&self, path: &Path) -> String {
        match self {
            Self::SingleFile { target_path } => {
                let base = target_path
                    .as_path()
                    .parent()
                    .unwrap_or_else(|| target_path.as_path());
                sanitize_path_for_logging(path, base).into_owned()
            }
            Self::Directory { base_dir } => {
                sanitize_path_for_logging(path, base_dir.as_path()).into_owned()
            }
        }
    }
```

with:

```rust
    pub(super) fn path_for_log(&self, path: &Path) -> String {
        match self {
            Self::SingleFile { log_base, .. } | Self::Directory { log_base, .. } => {
                log_base.sanitize(path).into_owned()
            }
        }
    }
```

- [ ] **Step 7: Update existing direct `WatchStrategy` construction in tests**

Inside `#[cfg(test)] mod tests`, add these helper functions after `create_markdown_fixture`:

```rust
    fn single_file_strategy(target: &Path) -> WatchStrategy {
        WatchStrategy::single_file(CanonicalPath::try_from_path(target).unwrap())
    }

    fn directory_strategy(base: &Path) -> WatchStrategy {
        WatchStrategy::directory(CanonicalPath::try_from_path(base).unwrap())
    }
```

Then replace direct test constructions like:

```rust
        let strategy = WatchStrategy::SingleFile {
            target_path: CanonicalPath::try_from_path(&target).unwrap(),
        };
```

with:

```rust
        let strategy = single_file_strategy(&target);
```

And replace direct test constructions like:

```rust
        let strategy = WatchStrategy::Directory {
            base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
        };
```

with:

```rust
        let strategy = directory_strategy(dir.path());
```

Run this search to find remaining direct constructions:

```bash
rg -n "WatchStrategy::(SingleFile|Directory)" src/watcher/strategy.rs
```

Expected after replacements: direct constructions remain only in the two helper functions added above.

- [ ] **Step 8: Add watcher path log regression tests**

In `src/watcher/strategy.rs`, inside `#[cfg(test)] mod tests`, add these tests near the existing `WatchStrategy` tests:

```rust
    #[test]
    fn test_watch_strategy_path_for_log_単一ファイルは親ディレクトリ相対で表示する() {
        let (_dir, target) = create_markdown_fixture("target.md", "# target");
        let strategy = single_file_strategy(&target);

        let path = target.parent().unwrap().join("nested.md");

        assert_eq!(strategy.path_for_log(&path), "nested.md");
    }

    #[test]
    fn test_watch_strategy_path_for_log_ディレクトリはbase相対で表示する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        let path = dir.path().join("docs/note.md");
        std::fs::write(&path, "# note").unwrap();
        let strategy = directory_strategy(dir.path());

        assert_eq!(strategy.path_for_log(&path), "docs/note.md");
    }
```

- [ ] **Step 9: Run watcher strategy focused tests**

Run:

```bash
cargo test --all-targets --all-features watcher::strategy
```

Expected: PASS. If the filter does not match tests in this crate layout, run the fallback command:

```bash
cargo test --all-targets --all-features watch_strategy
```

Expected: PASS or no matching tests only if the first command passed. Do not treat zero matched tests as verification.

- [ ] **Step 10: Commit watcher integration**

Run:

```bash
git add src/watcher/strategy.rs
git commit -m "refactor: watcherログでbase正規化結果を再利用"
```

Expected: commit succeeds and includes only `src/watcher/strategy.rs`.

## Task 4: BACKLOG を更新して検証する

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Modify: `docs/done/DONE-2026-05.md`

- [ ] **Step 1: BACKLOG の対象項目を確認する**

Run:

```bash
rg -n "log_path::canonicalize_status|P2:|DONE-2026-05" docs/todo/BACKLOG.md docs/done/DONE-2026-05.md
```

Expected: `docs/todo/BACKLOG.md` の P2 に `log_path::canonicalize_status` 項目が表示される。

- [ ] **Step 2: `docs/done/DONE-2026-05.md` に完了項目を追加する**

Append this block under `## BACKLOG 完了履歴` near the top of `docs/done/DONE-2026-05.md`:

```markdown
- [x] `log_path::canonicalize_status` の毎回 syscall を削減する
  - 完了根拠: `src/server/log_path.rs` に `LogBasePath` を追加し、base の canonicalize 結果を生成時に保持して再利用できるようにした。既存の `sanitize_path_for_logging` / `sanitize_path_for_logging_escaped` は互換 API として残し、単発呼び出しの契約は維持している。`WatchStrategy::path_for_log` は `LogBasePath` 経由でログ用 path を相対化する構成にした。base 配下、base 自身、base 外、symlink 経由の base 外脱出、symlink 経由の base 内解決、base canonicalize 失敗時の lexical fallback、制御文字 escape を unit test で固定した。HTTP API、WebSocket payload、HTML sanitize、CSP、Host/Origin validation、path validation、memo sidecar、file size limit は変更していない。
  - 由来: アーキテクチャレビュー (2026-04-30)
```

- [ ] **Step 3: `docs/todo/BACKLOG.md` から対象項目を削除する**

Remove this entire unchecked P2 block from `docs/todo/BACKLOG.md`:

```markdown
- [ ] `log_path::canonicalize_status` の毎回 syscall を削減する
  - ファイル: `src/server/log_path.rs` L52-65
  - 現状: ログ出力ごとに `path` と `base` を canonicalize する。warn/error 時のみ呼ばれるが、ログ storm 状況下では I/O が増える
  - 対応: `base` の canonicalize 結果を起動時に一度だけ算出してキャッシュし、ログ経路では path 側のみ canonicalize する。または `OnceLock` で base を保持
  - 判断: ログ storm 時の効率化であり、現行の安全性を弱めていないため BACKLOG P2 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)
```

- [ ] **Step 4: Documentation consistency check**

Run:

```bash
rg -n "log_path::canonicalize_status|LogBasePath|DONE-2026-05" docs/todo/BACKLOG.md docs/done/DONE-2026-05.md docs/superpowers/specs/2026-05-17-log-path-base-canonical-cache-design.md
```

Expected:

- `docs/todo/BACKLOG.md` has no `log_path::canonicalize_status` unchecked item.
- `docs/done/DONE-2026-05.md` has the new completed item.
- The design spec references `LogBasePath`.

- [ ] **Step 5: Commit docs update**

Run:

```bash
git add docs/todo/BACKLOG.md docs/done/DONE-2026-05.md
git commit -m "docs: log_path syscall削減を完了履歴へ移動"
```

Expected: commit succeeds and includes only the two docs files.

## Task 5: 全体検証と最終確認

**Files:**
- Verify: `src/server/log_path.rs`
- Verify: `src/watcher/strategy.rs`
- Verify: `docs/todo/BACKLOG.md`
- Verify: `docs/done/DONE-2026-05.md`

- [ ] **Step 1: Focused Rust tests**

Run:

```bash
cargo test --all-targets --all-features log_path
```

Expected: PASS.

- [ ] **Step 2: Watcher-related tests**

Run:

```bash
cargo test --all-targets --all-features watcher
```

Expected: PASS. If this filter matches too broadly but passes, continue. If it matches zero tests, run:

```bash
cargo test --all-targets --all-features strategy
```

Expected: PASS with watcher strategy tests included in output.

- [ ] **Step 3: Full repository verification**

Run:

```bash
./verify.sh
```

Expected: PASS for format, clippy, and tests.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git status --short --branch
git diff --stat HEAD
git diff -- src/server/log_path.rs src/watcher/strategy.rs docs/todo/BACKLOG.md docs/done/DONE-2026-05.md
```

Expected: no unstaged implementation changes if all task commits were made. If the docs plan file is still uncommitted, commit it separately with:

```bash
git add docs/superpowers/plans/2026-05-17-log-path-base-canonical-cache.md
git commit -m "docs: log_path base canonicalize cacheの実装計画を追加"
```

- [ ] **Step 5: Report completion**

Final report must include:

- Changed files and rough line impact.
- Affected dependent files: `src/watcher/strategy.rs` callers, `server/files/*` unchanged callers, docs archive.
- Verification results from `cargo test --all-targets --all-features log_path`, watcher-focused tests, and `./verify.sh`.
- Security note: base outside masking and symlink escape detection remain covered by tests.
- Residual risk: cached base follows startup/canonical base identity; if base path is replaced after construction, logs are judged against the cached canonical base by design.
