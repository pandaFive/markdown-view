# watcher 除外監視計画と ENOSPC 文言改善 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ディレクトリモード watcher の監視対象から `.git`、`node_modules`、`target`、隠しディレクトリを notify 登録前に除外し、起動後の新規サブディレクトリにも追従し、ENOSPC 相当の失敗を操作可能な日本語メッセージにする。

**Architecture:** `WatchStrategy` が `WatchPlan` を生成し、runtime は plan entry を `NonRecursive` で登録する。debouncer callback は internal channel へ結果を渡すだけにし、watcher thread 本体が Markdown 変更通知と新規ディレクトリの動的 watch 追加を同じ所有スレッドで処理する。ENOSPC は `notify::ErrorKind::MaxFilesWatch` を主経路に `WatchErrorKind::ResourceExhausted` へ分類する。

**Tech Stack:** Rust, notify 8.2, notify-debouncer-mini 0.6, tokio mpsc, std::sync::mpsc, tracing, cargo test, ./verify.sh.

---

## 目的・非目的

目的:

- notify へ渡す前に不要 subtree を除外して、巨大 repository での watch 数を減らす。
- non-recursive 複数 watch 化しても、起動後に作られた通常サブディレクトリ配下の `.md` 更新通知を維持する。
- 起動時 watch 登録が部分成功した場合は起動しない。
- 起動後の追加 watch 失敗は silent failure にせず、health failure と `WatchEvent::Error` にする。
- ENOSPC 相当の init failure では Linux inotify 上限の確認・調整へ進める文言を出す。

非目的:

- CLI / config の除外パターン指定。
- `.gitignore` 解釈。
- HTTP/API/WebSocket JSON/renderer/template/memo/search の外部契約変更。
- watcher 自動再起動。
- Linux 以外へ inotify 固有対処を要求する表示。

## ファイル構成

- Modify: `src/watcher/strategy.rs`
  - 責務: `AppMode` から watcher の監視戦略を作り、変更 event を Markdown 更新 path へ絞る。今回 `WatchPlan`、`WatchPlanEntry`、`WatchPlanDiagnostics`、`ExcludeReason`、起動時 plan 生成、新規 subtree plan 生成を追加する。
- Modify: `src/watcher/runtime.rs`
  - 責務: watcher thread、debouncer、health、shutdown、notify result forwarding。今回、単一 `watch_dir + recursive_mode` 登録から `WatchPlan` 登録へ変更し、internal event loop で変更通知と動的 watch 追加を処理する。
- Modify: `src/watcher/error.rs`
  - 責務: watcher error の型、分類、user-facing message。今回 `ResourceExhausted` 分類と notify error 判定 helper を追加する。
- Modify: `src/watcher/mod.rs`
  - 責務: watcher public exports。今回 `WatchErrorKind` variant 追加に伴う test だけを更新する可能性がある。
- Modify: `README.md`
  - 責務: 利用者向け説明。今回 Linux inotify 上限と既定除外方針を追記する。
- Modify: `docs/todo/TODO.md`
  - 責務: 未完了 TODO 管理。実装完了時に対象項目を Done Summary へ移す。
- Reference: `docs/superpowers/specs/2026-05-08-watcher-enospc-exclusions-design.md`
  - 責務: 承認済み設計。編集しない。

## 事前条件

- 作業ブランチは `fix/watcher-enospc-exclusions`。
- 設計コミット `f17c4d7` と補強コミット `e16dd3c` がある。
- 作業開始前に未コミット差分がない。

Run:

```bash
git status --short --branch
```

Expected:

```text
## fix/watcher-enospc-exclusions
```

追加の `M` / `??` がある場合は、ユーザー作業を巻き込まないよう内容を確認してから進める。

---

### Task 1: ENOSPC / resource exhausted error contract

**Files:**
- Modify: `src/watcher/error.rs`
- Test: `src/watcher/error.rs`

- [ ] **Step 1: 失敗テストを追加する**

`src/watcher/error.rs` の tests に次を追加する。

```rust
#[test]
fn test_watch_error_resource_exhaustedはinotify上限案内を含む() {
    let error = WatchError::resource_exhausted("OS file watch limit reached.");

    let message = error.user_message();

    assert_eq!(error.kind(), WatchErrorKind::ResourceExhausted);
    assert!(message.contains("監視対象が多すぎるため監視を開始できません"));
    assert!(message.contains("fs.inotify.max_user_watches"));
    assert!(message.contains("Linux"));
    assert!(message.contains("OS file watch limit reached."));
}

#[test]
fn test_notify_error_max_files_watchはresource_exhaustedに変換される() {
    let notify_error = notify::Error::new(notify::ErrorKind::MaxFilesWatch);

    let error = WatchError::from_watch_init_error("ディレクトリ監視の開始に失敗", &notify_error);

    assert_eq!(error.kind(), WatchErrorKind::ResourceExhausted);
    assert!(error.detail().contains("ディレクトリ監視の開始に失敗"));
    assert!(error.detail().contains("OS file watch limit reached."));
}

#[test]
fn test_notify_error_raw_os_error_28はresource_exhaustedに変換される() {
    let io_error = std::io::Error::from_raw_os_error(28);
    let notify_error = notify::Error::io(io_error);

    let error = WatchError::from_watch_init_error("ディレクトリ監視の開始に失敗", &notify_error);

    assert_eq!(error.kind(), WatchErrorKind::ResourceExhausted);
}

#[test]
fn test_notify_error_通常エラーはinitに変換される() {
    let notify_error = notify::Error::generic("generic watch failure");

    let error = WatchError::from_watch_init_error("ディレクトリ監視の開始に失敗", &notify_error);

    assert_eq!(error.kind(), WatchErrorKind::Init);
    assert_eq!(
        error.user_message(),
        "監視の初期化に失敗しました: ディレクトリ監視の開始に失敗: generic watch failure"
    );
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run:

```bash
cargo test watcher::error::tests::test_watch_error_resource_exhausted watcher::error::tests::test_notify_error_max_files_watch watcher::error::tests::test_notify_error_raw_os_error_28 watcher::error::tests::test_notify_error_通常エラーはinitに変換される
```

Expected: `WatchError::resource_exhausted`、`WatchError::from_watch_init_error`、`WatchErrorKind::ResourceExhausted` が未定義で FAIL。

- [ ] **Step 3: 最小実装を追加する**

`src/watcher/error.rs` を次の方針で更新する。

```rust
impl WatchError {
    /// 監視リソース枯渇エラーを生成する
    pub fn resource_exhausted(detail: impl Into<String>) -> Self {
        Self {
            kind: WatchErrorKind::ResourceExhausted,
            detail: detail.into(),
        }
    }

    /// notify の watch 登録失敗を初期化エラーへ変換する
    pub fn from_watch_init_error(prefix: &str, error: &notify::Error) -> Self {
        let detail = format!("{}: {}", prefix, error);
        if is_watch_resource_exhausted(error) {
            Self::resource_exhausted(detail)
        } else {
            Self::init(detail)
        }
    }

    /// 利用者向けメッセージを返す
    pub fn user_message(&self) -> String {
        match self.kind {
            WatchErrorKind::Init => format!("監視の初期化に失敗しました: {}", self.detail),
            WatchErrorKind::Notify => {
                format!("通知ライブラリエラーが発生しました: {}", self.detail)
            }
            WatchErrorKind::ThreadPanic => {
                format!("監視スレッドがパニックで停止しました: {}", self.detail)
            }
            WatchErrorKind::ResourceExhausted => format!(
                "監視対象が多すぎるため監視を開始できません。Linux の inotify 上限 fs.inotify.max_user_watches に到達した可能性があります。不要な大規模ディレクトリは既定で除外されますが、それでも失敗する場合は現在値を確認し、利用環境の方針に従って上限を引き上げてください。詳細: {}",
                self.detail
            ),
        }
    }
}

fn is_watch_resource_exhausted(error: &notify::Error) -> bool {
    match &error.kind {
        notify::ErrorKind::MaxFilesWatch => true,
        notify::ErrorKind::Io(io_error) => io_error.raw_os_error() == Some(28),
        notify::ErrorKind::Generic(message) => {
            message.contains("ENOSPC") || message.contains("No space left on device")
        }
        _ => {
            let message = error.to_string();
            message.contains("ENOSPC") || message.contains("No space left on device")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchErrorKind {
    Init,
    Notify,
    ThreadPanic,
    ResourceExhausted,
}
```

既存 `WatchErrorKind` 定義は置換し、doc comment は日本語で各 variant に付ける。

- [ ] **Step 4: error tests を通す**

Run:

```bash
cargo test watcher::error::tests
```

Expected: `watcher::error::tests` が PASS。

- [ ] **Step 5: コミットする**

Run:

```bash
git add src/watcher/error.rs
git commit -m "feat: watcherリソース枯渇エラーを分類する"
```

---

### Task 2: WatchPlan と除外判定

**Files:**
- Modify: `src/watcher/strategy.rs`

- [ ] **Step 1: WatchPlan の失敗テストを追加する**

`src/watcher/strategy.rs` の tests に次を追加する。

```rust
fn plan_paths(plan: &super::WatchPlan) -> Vec<PathBuf> {
    let mut paths = plan
        .entries()
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn test_watch_plan_単一ファイルは親ディレクトリをnonrecursiveで登録する() {
    let (_dir, target) = create_markdown_fixture("target.md", "# target");
    let strategy = WatchStrategy::SingleFile {
        target_path: CanonicalPath::try_from_path(&target).unwrap(),
    };

    let plan = strategy.watch_plan().unwrap();

    assert_eq!(plan.entries().len(), 1);
    assert_eq!(plan.entries()[0].path(), target.parent().unwrap());
    assert_eq!(plan.entries()[0].recursive_mode(), RecursiveMode::NonRecursive);
}

#[test]
fn test_watch_plan_ディレクトリは除外対象を登録しない() {
    let dir = tempfile::tempdir().unwrap();
    for path in [
        "docs",
        "docs/nested",
        ".git",
        ".git/objects",
        "node_modules",
        "node_modules/pkg",
        "target",
        "target/debug",
        ".draft",
        ".draft/notes",
    ] {
        std::fs::create_dir_all(dir.path().join(path)).unwrap();
    }
    let strategy = WatchStrategy::Directory {
        base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
    };

    let plan = strategy.watch_plan().unwrap();
    let paths = plan_paths(&plan);

    assert!(paths.contains(&dir.path().canonicalize().unwrap()));
    assert!(paths.contains(&dir.path().join("docs").canonicalize().unwrap()));
    assert!(paths.contains(&dir.path().join("docs/nested").canonicalize().unwrap()));
    assert!(!paths.iter().any(|path| path.ends_with(".git")));
    assert!(!paths.iter().any(|path| path.ends_with("objects")));
    assert!(!paths.iter().any(|path| path.ends_with("node_modules")));
    assert!(!paths.iter().any(|path| path.ends_with("pkg")));
    assert!(!paths.iter().any(|path| path.ends_with("target")));
    assert!(!paths.iter().any(|path| path.ends_with("debug")));
    assert!(!paths.iter().any(|path| path.ends_with(".draft")));
    assert!(!paths.iter().any(|path| path.ends_with("notes")));
}

#[test]
fn test_watch_plan_隠しbase自体は登録する() {
    let parent = tempfile::tempdir().unwrap();
    let hidden_base = parent.path().join(".workspace");
    std::fs::create_dir_all(hidden_base.join("docs")).unwrap();
    let strategy = WatchStrategy::Directory {
        base_dir: CanonicalPath::try_from_path(&hidden_base).unwrap(),
    };

    let plan = strategy.watch_plan().unwrap();
    let paths = plan_paths(&plan);

    assert!(paths.contains(&hidden_base.canonicalize().unwrap()));
    assert!(paths.contains(&hidden_base.join("docs").canonicalize().unwrap()));
}

#[test]
#[cfg(unix)]
fn test_watch_plan_シンボリックリンクディレクトリは辿らない() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(outside.path().join("docs")).unwrap();
    symlink(outside.path(), dir.path().join("linked")).unwrap();
    let strategy = WatchStrategy::Directory {
        base_dir: CanonicalPath::try_from_path(dir.path()).unwrap(),
    };

    let plan = strategy.watch_plan().unwrap();
    let paths = plan_paths(&plan);

    assert!(paths.contains(&dir.path().canonicalize().unwrap()));
    assert!(!paths.iter().any(|path| path.ends_with("linked")));
    assert!(!paths.iter().any(|path| path.ends_with("docs")));
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run:

```bash
cargo test watcher::strategy::tests::test_watch_plan_ -- --nocapture
```

Expected: `watch_plan`、`WatchPlan`、`entries` accessor が未定義で FAIL。

- [ ] **Step 3: WatchPlan 型と accessor を追加する**

`src/watcher/strategy.rs` の `WatchStrategy` 定義直後に追加する。

```rust
#[derive(Debug, Clone)]
pub(super) struct WatchPlan {
    entries: Vec<WatchPlanEntry>,
    diagnostics: WatchPlanDiagnostics,
}

impl WatchPlan {
    pub(super) fn entries(&self) -> &[WatchPlanEntry] {
        &self.entries
    }

    pub(super) fn diagnostics(&self) -> &WatchPlanDiagnostics {
        &self.diagnostics
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WatchPlanEntry {
    path: PathBuf,
    recursive_mode: RecursiveMode,
}

impl WatchPlanEntry {
    pub(super) fn new(path: PathBuf, recursive_mode: RecursiveMode) -> Self {
        Self {
            path,
            recursive_mode,
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn recursive_mode(&self) -> RecursiveMode {
        self.recursive_mode
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WatchPlanDiagnostics {
    registered_candidates: usize,
    excluded_subtrees: usize,
    excluded_by_reason: std::collections::BTreeMap<ExcludeReason, usize>,
}

impl WatchPlanDiagnostics {
    fn record_registered(&mut self) {
        self.registered_candidates += 1;
    }

    fn record_excluded(&mut self, reason: ExcludeReason) {
        self.excluded_subtrees += 1;
        *self.excluded_by_reason.entry(reason).or_insert(0) += 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ExcludeReason {
    Git,
    NodeModules,
    Target,
    Hidden,
    Symlink,
    MetadataError,
}
```

- [ ] **Step 4: plan 生成を実装する**

`WatchStrategy` impl に追加する。既存 `watch_dir()` / `recursive_mode()` は Task 4 で runtime 移行後に削除する。

```rust
    pub(super) fn watch_plan(&self) -> Result<WatchPlan> {
        match self {
            Self::SingleFile { target_path } => {
                let watch_dir = target_path
                    .as_path()
                    .parent()
                    .map(Path::to_path_buf)
                    .context("親ディレクトリが取得できません")?;
                let mut diagnostics = WatchPlanDiagnostics::default();
                diagnostics.record_registered();
                Ok(WatchPlan {
                    entries: vec![WatchPlanEntry::new(watch_dir, RecursiveMode::NonRecursive)],
                    diagnostics,
                })
            }
            Self::Directory { base_dir } => build_directory_watch_plan(base_dir.as_path()),
        }
    }
```

`normalize_lexical_path` の前に helper を追加する。

```rust
fn build_directory_watch_plan(base_dir: &Path) -> Result<WatchPlan> {
    let mut entries = Vec::new();
    let mut diagnostics = WatchPlanDiagnostics::default();
    collect_watch_plan_entries(base_dir, true, &mut entries, &mut diagnostics)?;
    Ok(WatchPlan {
        entries,
        diagnostics,
    })
}

fn collect_watch_plan_entries(
    dir: &Path,
    is_root: bool,
    entries: &mut Vec<WatchPlanEntry>,
    diagnostics: &mut WatchPlanDiagnostics,
) -> Result<()> {
    if !is_root {
        if let Some(reason) = exclude_reason_for_dir(dir) {
            diagnostics.record_excluded(reason);
            return Ok(());
        }
    }

    let metadata = match std::fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) if is_root => {
            return Err(error).with_context(|| {
                format!(
                    "監視対象ディレクトリのメタデータ取得に失敗: {}",
                    sanitize_path_for_logging(dir, dir)
                )
            });
        }
        Err(error) => {
            diagnostics.record_excluded(ExcludeReason::MetadataError);
            tracing::warn!(
                "[markdown-view] watcher監視計画: メタデータ取得失敗（除外）: {} ({})",
                sanitize_path_for_logging(dir, dir),
                error
            );
            return Ok(());
        }
    };

    if metadata.file_type().is_symlink() {
        if !is_root {
            diagnostics.record_excluded(ExcludeReason::Symlink);
            return Ok(());
        }
    }
    if !metadata.is_dir() {
        return Ok(());
    }

    entries.push(WatchPlanEntry::new(
        normalize_lexical_path(dir),
        RecursiveMode::NonRecursive,
    ));
    diagnostics.record_registered();

    let children = match std::fs::read_dir(dir) {
        Ok(children) => children,
        Err(error) if is_root => {
            return Err(error).with_context(|| {
                format!(
                    "監視対象ディレクトリの読み取りに失敗: {}",
                    sanitize_path_for_logging(dir, dir)
                )
            });
        }
        Err(error) => {
            diagnostics.record_excluded(ExcludeReason::MetadataError);
            tracing::warn!(
                "[markdown-view] watcher監視計画: ディレクトリ読み取り失敗（除外）: {} ({})",
                sanitize_path_for_logging(dir, dir),
                error
            );
            return Ok(());
        }
    };

    for child in children {
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                diagnostics.record_excluded(ExcludeReason::MetadataError);
                tracing::warn!(
                    "[markdown-view] watcher監視計画: ディレクトリエントリ読み取り失敗（除外）: {}",
                    error
                );
                continue;
            }
        };
        collect_watch_plan_entries(&child.path(), false, entries, diagnostics)?;
    }

    Ok(())
}

fn exclude_reason_for_dir(path: &Path) -> Option<ExcludeReason> {
    let name = path.file_name()?.to_string_lossy();
    match name.as_ref() {
        ".git" => Some(ExcludeReason::Git),
        "node_modules" => Some(ExcludeReason::NodeModules),
        "target" => Some(ExcludeReason::Target),
        _ if name.starts_with('.') => Some(ExcludeReason::Hidden),
        _ => None,
    }
}
```

- [ ] **Step 5: strategy tests を通す**

Run:

```bash
cargo test watcher::strategy::tests::test_watch_plan_ -- --nocapture
```

Expected: 追加した `test_watch_plan_...` が PASS。

- [ ] **Step 6: コミットする**

Run:

```bash
git add src/watcher/strategy.rs
git commit -m "feat: watcher監視計画で除外対象を事前分類する"
```

---

### Task 3: 新規 subtree 用 plan と Markdown 回復通知

**Files:**
- Modify: `src/watcher/strategy.rs`

- [ ] **Step 1: 新規 subtree plan の失敗テストを追加する**

`src/watcher/strategy.rs` の tests に追加する。

```rust
#[test]
fn test_watch_plan_new_subtreeは既存登録済みpathを除外する() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs/nested")).unwrap();
    let root = dir.path().canonicalize().unwrap();
    let docs = dir.path().join("docs").canonicalize().unwrap();
    let mut registered = std::collections::HashSet::new();
    registered.insert(root);
    registered.insert(docs);

    let plan = super::WatchPlan::for_new_subtree(
        &dir.path().join("docs"),
        &registered,
    )
    .unwrap();

    let paths = plan_paths(&plan);
    assert_eq!(paths, vec![dir.path().join("docs/nested").canonicalize().unwrap()]);
}

#[test]
fn test_watch_plan_new_subtreeは除外対象を含めない() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs/ok")).unwrap();
    std::fs::create_dir_all(dir.path().join("docs/node_modules/pkg")).unwrap();
    std::fs::create_dir_all(dir.path().join("docs/.hidden")).unwrap();
    let registered = std::collections::HashSet::new();

    let plan = super::WatchPlan::for_new_subtree(&dir.path().join("docs"), &registered).unwrap();
    let paths = plan_paths(&plan);

    assert!(paths.contains(&dir.path().join("docs").canonicalize().unwrap()));
    assert!(paths.contains(&dir.path().join("docs/ok").canonicalize().unwrap()));
    assert!(!paths.iter().any(|path| path.ends_with("node_modules")));
    assert!(!paths.iter().any(|path| path.ends_with("pkg")));
    assert!(!paths.iter().any(|path| path.ends_with(".hidden")));
}

#[test]
fn test_collect_markdown_files_for_recoveryは除外対象を読まない() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs/ok")).unwrap();
    std::fs::create_dir_all(dir.path().join("docs/target")).unwrap();
    let ok = dir.path().join("docs/ok/new.md");
    let ignored = dir.path().join("docs/target/ignored.md");
    std::fs::write(&ok, "# ok").unwrap();
    std::fs::write(&ignored, "# ignored").unwrap();

    let mut files = super::collect_markdown_files_for_recovery(&dir.path().join("docs"));
    files.sort();

    assert_eq!(files, vec![ok.canonicalize().unwrap()]);
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run:

```bash
cargo test watcher::strategy::tests::test_watch_plan_new_subtree watcher::strategy::tests::test_collect_markdown_files_for_recovery -- --nocapture
```

Expected: `WatchPlan::for_new_subtree` と `collect_markdown_files_for_recovery` が未定義で FAIL。

- [ ] **Step 3: 新規 subtree plan と回復列挙を実装する**

`impl WatchPlan` に追加する。

```rust
    pub(super) fn for_new_subtree(
        subtree: &Path,
        registered_paths: &std::collections::HashSet<PathBuf>,
    ) -> Result<Self> {
        let mut plan = build_directory_watch_plan(subtree)?;
        plan.entries
            .retain(|entry| !registered_paths.contains(entry.path()));
        plan.diagnostics.registered_candidates = plan.entries.len();
        Ok(plan)
    }
```

`exclude_reason_for_dir` の後に追加する。

```rust
pub(super) fn collect_markdown_files_for_recovery(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_markdown_files_for_recovery_inner(root, &mut files);
    files
}

fn collect_markdown_files_for_recovery_inner(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Some(_reason) = exclude_reason_for_dir(dir) {
        return;
    }
    let metadata = match std::fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] watcher回復列挙: メタデータ取得失敗（スキップ）: {} ({})",
                sanitize_path_for_logging(dir, dir),
                error
            );
            return;
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return;
    }
    let children = match std::fs::read_dir(dir) {
        Ok(children) => children,
        Err(error) => {
            tracing::warn!(
                "[markdown-view] watcher回復列挙: ディレクトリ読み取り失敗（スキップ）: {} ({})",
                sanitize_path_for_logging(dir, dir),
                error
            );
            return;
        }
    };
    for child in children.flatten() {
        let path = child.path();
        if path.is_dir() {
            collect_markdown_files_for_recovery_inner(&path, files);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            files.push(normalize_lexical_path(&path));
        }
    }
}
```

- [ ] **Step 4: strategy tests を通す**

Run:

```bash
cargo test watcher::strategy::tests::test_watch_plan_new_subtree watcher::strategy::tests::test_collect_markdown_files_for_recovery -- --nocapture
```

Expected: 追加テストが PASS。

- [ ] **Step 5: コミットする**

Run:

```bash
git add src/watcher/strategy.rs
git commit -m "feat: watcher新規subtreeの監視計画を生成する"
```

---

### Task 4: runtime を WatchPlan 登録へ移行する

**Files:**
- Modify: `src/watcher/runtime.rs`
- Modify: `src/watcher/strategy.rs`

- [ ] **Step 1: 複数 entry 登録 helper の失敗テストを追加する**

`src/watcher/runtime.rs` の tests に追加する。

```rust
#[derive(Default)]
struct FakeWatchRegistrar {
    fail_on: Option<PathBuf>,
    watched: Vec<(PathBuf, notify::RecursiveMode)>,
}

impl FakeWatchRegistrar {
    fn watch(&mut self, path: &std::path::Path, mode: notify::RecursiveMode) -> notify::Result<()> {
        if self.fail_on.as_deref() == Some(path) {
            return Err(notify::Error::generic("watch registration failed"));
        }
        self.watched.push((path.to_path_buf(), mode));
        Ok(())
    }
}

#[test]
fn test_register_watch_plan_全entryをnonrecursiveで登録する() {
    let dir = tempfile::tempdir().unwrap();
    let plan = crate::watcher::strategy::WatchPlan::from_entries_for_test(vec![
        crate::watcher::strategy::WatchPlanEntry::new(
            dir.path().join("a"),
            notify::RecursiveMode::NonRecursive,
        ),
        crate::watcher::strategy::WatchPlanEntry::new(
            dir.path().join("b"),
            notify::RecursiveMode::NonRecursive,
        ),
    ]);
    let mut registrar = FakeWatchRegistrar::default();
    let mut registered = std::collections::HashSet::new();

    register_watch_plan_with(&plan, &mut registered, |path, mode| registrar.watch(path, mode))
        .expect("watch plan registration should succeed");

    assert_eq!(registrar.watched.len(), 2);
    assert!(registered.contains(&dir.path().join("a")));
    assert!(registered.contains(&dir.path().join("b")));
}

#[test]
fn test_register_watch_plan_一部失敗ならerrorを返す() {
    let dir = tempfile::tempdir().unwrap();
    let fail_path = dir.path().join("b");
    let plan = crate::watcher::strategy::WatchPlan::from_entries_for_test(vec![
        crate::watcher::strategy::WatchPlanEntry::new(
            dir.path().join("a"),
            notify::RecursiveMode::NonRecursive,
        ),
        crate::watcher::strategy::WatchPlanEntry::new(
            fail_path.clone(),
            notify::RecursiveMode::NonRecursive,
        ),
    ]);
    let mut registrar = FakeWatchRegistrar {
        fail_on: Some(fail_path),
        watched: Vec::new(),
    };
    let mut registered = std::collections::HashSet::new();

    let error = register_watch_plan_with(&plan, &mut registered, |path, mode| {
        registrar.watch(path, mode)
    })
    .expect_err("partial registration should fail");

    assert!(error.to_string().contains("watch registration failed"));
    assert_eq!(registrar.watched.len(), 1);
}
```

`WatchPlan::from_entries_for_test` は test-only constructor として Task 4 Step 3 で追加する。

- [ ] **Step 2: テストが失敗することを確認する**

Run:

```bash
cargo test watcher::runtime::tests::test_register_watch_plan_ -- --nocapture
```

Expected: `register_watch_plan_with` と `WatchPlan::from_entries_for_test` が未定義で FAIL。

- [ ] **Step 3: test-only constructor を追加する**

`src/watcher/strategy.rs` の `impl WatchPlan` に追加する。

```rust
    #[cfg(test)]
    pub(super) fn from_entries_for_test(entries: Vec<WatchPlanEntry>) -> Self {
        Self {
            diagnostics: WatchPlanDiagnostics {
                registered_candidates: entries.len(),
                excluded_subtrees: 0,
                excluded_by_reason: std::collections::BTreeMap::new(),
            },
            entries,
        }
    }
```

- [ ] **Step 4: 登録 helper を実装する**

`src/watcher/runtime.rs` に import を追加する。

```rust
use std::collections::HashSet;
use std::path::{Path, PathBuf};
```

既存 `use std::path::PathBuf;` は置換する。

`handle_debounced_watch_result` の前に追加する。

```rust
fn register_watch_plan_with<F>(
    plan: &super::strategy::WatchPlan,
    registered_paths: &mut HashSet<PathBuf>,
    mut watch: F,
) -> notify::Result<()>
where
    F: FnMut(&Path, notify::RecursiveMode) -> notify::Result<()>,
{
    for entry in plan.entries() {
        watch(entry.path(), entry.recursive_mode())?;
        registered_paths.insert(entry.path().to_path_buf());
    }
    tracing::debug!(
        registered_candidates = plan.diagnostics().registered_candidates,
        excluded_subtrees = plan.diagnostics().excluded_subtrees,
        "[markdown-view] watcher監視計画を登録しました"
    );
    Ok(())
}
```

- [ ] **Step 5: spawn_watcher_thread を WatchPlan 登録へ移行する**

`Watcher::spawn` の `watch_dir` 算出を削除し、`watch_plan` を作る。

```rust
let strategy = WatchStrategy::from_mode(&mode)?;
let watch_plan = strategy.watch_plan()?;
```

`spawn_watcher_thread` signature を変更する。

```rust
fn spawn_watcher_thread(
    strategy: WatchStrategy,
    watch_plan: super::strategy::WatchPlan,
    tx: mpsc::Sender<WatchEvent>,
    init_tx: oneshot::Sender<InitResult>,
    thread_shutdown_flag: Arc<AtomicBool>,
    health_state: WatcherHealthState,
) -> Result<std::thread::JoinHandle<()>>
```

呼び出しも `watch_dir` ではなく `watch_plan` を渡す。

`spawn_watcher_thread` 内の `recursive_mode` 変数を削除し、watch 登録箇所を置換する。

```rust
let mut registered_paths = HashSet::new();
if let Err(e) = register_watch_plan_with(&watch_plan, &mut registered_paths, |path, mode| {
    debouncer.watcher().watch(path, mode)
}) {
    send_init_result(
        &mut init_tx,
        Err(WatchError::from_watch_init_error(start_error_prefix, &e)),
    );
    return;
}
```

- [ ] **Step 6: strategy の旧 API を削除する**

`src/watcher/strategy.rs` から `watch_dir()` と `recursive_mode()` を削除する。対応する tests の assertion も削除する。

`test_strategy_単一ファイルモードのラベルとモードを返す` から次を削除する。

```rust
let parent = target.parent().unwrap().to_path_buf();
assert_eq!(strategy.watch_dir().unwrap(), parent);
assert_eq!(strategy.recursive_mode(), RecursiveMode::NonRecursive);
```

`test_strategy_ディレクトリモードのラベルとモードを返す` から次を削除する。

```rust
assert_eq!(strategy.watch_dir().unwrap(), dir.path());
assert_eq!(strategy.recursive_mode(), RecursiveMode::Recursive);
```

- [ ] **Step 7: runtime / strategy tests を通す**

Run:

```bash
cargo test watcher::runtime::tests::test_register_watch_plan_ watcher::strategy::tests::test_strategy_ -- --nocapture
```

Expected: PASS。

- [ ] **Step 8: コミットする**

Run:

```bash
git add src/watcher/runtime.rs src/watcher/strategy.rs
git commit -m "refactor: watcher runtimeを監視計画登録へ移行する"
```

---

### Task 5: internal event loop と動的 watch 追加

**Files:**
- Modify: `src/watcher/runtime.rs`
- Modify: `src/watcher/strategy.rs`

- [ ] **Step 1: internal event 処理の失敗テストを追加する**

`src/watcher/runtime.rs` の tests に追加する。

```rust
#[test]
fn test_process_internal_events_新規ディレクトリをwatch追加して既存markdownを通知する() {
    let dir = tempfile::tempdir().unwrap();
    let new_dir = dir.path().join("new");
    std::fs::create_dir_all(&new_dir).unwrap();
    let md = new_dir.join("created.md");
    std::fs::write(&md, "# created").unwrap();
    let strategy = WatchStrategy::Directory {
        base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
    };
    let events = vec![notify_debouncer_mini::DebouncedEvent::new(
        new_dir.clone(),
        notify_debouncer_mini::DebouncedEventKind::Any,
    )];
    let health_state = WatcherHealthState::new_alive();
    let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
    let mut registered = std::collections::HashSet::new();
    registered.insert(dir.path().canonicalize().unwrap());
    let mut watched = Vec::new();

    process_debounced_events_with_watch(
        events,
        &strategy,
        &tx,
        &health_state,
        &mut registered,
        |path, mode| {
            watched.push((path.to_path_buf(), mode));
            Ok(())
        },
    );

    assert!(watched.iter().any(|(path, mode)| {
        path == &new_dir.canonicalize().unwrap() && *mode == notify::RecursiveMode::NonRecursive
    }));
    assert_eq!(
        rx.try_recv().expect("recovery markdown notificationを期待"),
        WatchEvent::FileChanged(md)
    );
}

#[test]
fn test_process_internal_events_追加watch失敗はhealth_failedとerror_eventを送る() {
    let dir = tempfile::tempdir().unwrap();
    let new_dir = dir.path().join("new");
    std::fs::create_dir_all(&new_dir).unwrap();
    let strategy = WatchStrategy::Directory {
        base_dir: crate::server::CanonicalPath::try_from_path(dir.path()).unwrap(),
    };
    let events = vec![notify_debouncer_mini::DebouncedEvent::new(
        new_dir,
        notify_debouncer_mini::DebouncedEventKind::Any,
    )];
    let health_state = WatcherHealthState::new_alive();
    let (tx, mut rx) = mpsc::channel::<WatchEvent>(4);
    let mut registered = std::collections::HashSet::new();
    registered.insert(dir.path().canonicalize().unwrap());

    process_debounced_events_with_watch(
        events,
        &strategy,
        &tx,
        &health_state,
        &mut registered,
        |_path, _mode| Err(notify::Error::generic("dynamic watch failed")),
    );

    assert_eq!(
        health_state.load(),
        WatcherHealth::Failed(WatcherFailureKind::Notify)
    );
    match rx.try_recv().expect("dynamic watch error eventを期待") {
        WatchEvent::Error(error) => {
            assert_eq!(error.kind(), WatchErrorKind::Notify);
            assert!(error.detail().contains("dynamic watch failed"));
        }
        WatchEvent::FileChanged(path) => {
            panic!("Errorを期待したがFileChanged({:?})を受信", path)
        }
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run:

```bash
cargo test watcher::runtime::tests::test_process_internal_events_ -- --nocapture
```

Expected: `process_debounced_events_with_watch` が未定義で FAIL。

- [ ] **Step 3: 新規ディレクトリ候補 helper を strategy に追加する**

`src/watcher/strategy.rs` の `impl WatchStrategy` に追加する。

```rust
    pub(super) fn collect_new_directory_candidates(
        &self,
        events: &[DebouncedEvent],
    ) -> Vec<PathBuf> {
        match self {
            Self::SingleFile { .. } => Vec::new(),
            Self::Directory { base_dir } => collect_directory_candidates(base_dir, events),
        }
    }
```

`collect_directory_changes` の後に追加する。

```rust
fn collect_directory_candidates(base_dir: &CanonicalPath, events: &[DebouncedEvent]) -> Vec<PathBuf> {
    let mut candidates = HashSet::new();
    let base_path = base_dir.as_path();
    for event in events {
        if !is_content_change_event(&event.kind) {
            continue;
        }
        let path = normalize_lexical_path(&event.path);
        if !path.is_dir() {
            continue;
        }
        let Some(base_relative_check) = path_for_base_relative_checks(&path, base_path) else {
            continue;
        };
        if base_relative_check.is_hidden {
            continue;
        }
        candidates.insert(base_relative_check.normalized_event_path);
    }
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort();
    candidates
}
```

- [ ] **Step 4: internal event 処理 helper を実装する**

`src/watcher/runtime.rs` の `handle_debounced_watch_result` を残しつつ、新 helper を追加する。

```rust
fn process_debounced_events_with_watch<F>(
    events: Vec<notify_debouncer_mini::DebouncedEvent>,
    strategy: &WatchStrategy,
    tx: &mpsc::Sender<WatchEvent>,
    health_state: &WatcherHealthState,
    registered_paths: &mut HashSet<PathBuf>,
    mut watch: F,
) where
    F: FnMut(&Path, notify::RecursiveMode) -> notify::Result<()>,
{
    for changed_path in strategy.collect_changed_paths(&events) {
        send_watch_event(
            tx,
            WatchEvent::FileChanged(changed_path),
            strategy.change_label(),
        );
    }

    for candidate in strategy.collect_new_directory_candidates(&events) {
        let plan = match super::strategy::WatchPlan::for_new_subtree(&candidate, registered_paths) {
            Ok(plan) => plan,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] 新規ディレクトリの監視計画生成に失敗: {}",
                    error
                );
                continue;
            }
        };
        if let Err(error) = register_watch_plan_with(&plan, registered_paths, |path, mode| {
            watch(path, mode)
        }) {
            health_state.store_failed(WatcherFailureKind::Notify);
            let watch_error = WatchError::notify(error.to_string());
            tracing::warn!(
                "[markdown-view] 新規ディレクトリの監視追加に失敗: {}",
                watch_error.detail()
            );
            send_watch_event(tx, WatchEvent::Error(watch_error), strategy.error_label());
            continue;
        }
        for markdown in super::strategy::collect_markdown_files_for_recovery(&candidate) {
            send_watch_event(
                tx,
                WatchEvent::FileChanged(markdown),
                strategy.change_label(),
            );
        }
    }
}
```

- [ ] **Step 5: watcher thread を internal channel loop に変更する**

`spawn_watcher_thread` closure 内で `new_debouncer` 前に internal channel を作る。

```rust
let (internal_tx, internal_rx) = std::sync::mpsc::channel::<
    std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>,
>();
```

callback は channel へ渡すだけにする。

```rust
let debouncer = new_debouncer(
    Duration::from_millis(DEBOUNCE_MS),
    move |res: std::result::Result<
        Vec<notify_debouncer_mini::DebouncedEvent>,
        notify::Error,
    >| {
        if internal_tx.send(res).is_err() {
            tracing::warn!("[markdown-view] watcher internal channel が閉じています");
        }
    },
);
```

`send_init_result(&mut init_tx, Ok(())); keep_watcher_thread_alive(...)` を次へ置換する。

```rust
send_init_result(&mut init_tx, Ok(()));
run_watcher_event_loop(
    &mut debouncer,
    internal_rx,
    &strategy,
    &rt_tx,
    &health_state,
    &thread_shutdown_flag,
    &mut registered_paths,
);
```

`keep_watcher_thread_alive` の近くに追加する。

```rust
fn run_watcher_event_loop(
    debouncer: &mut notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    internal_rx: std::sync::mpsc::Receiver<
        std::result::Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>,
    >,
    strategy: &WatchStrategy,
    tx: &mpsc::Sender<WatchEvent>,
    health_state: &WatcherHealthState,
    shutdown_flag: &AtomicBool,
    registered_paths: &mut HashSet<PathBuf>,
) {
    while !shutdown_flag.load(Ordering::Acquire) {
        match internal_rx.recv_timeout(Duration::from_millis(WATCHER_THREAD_PARK_MS)) {
            Ok(Ok(events)) => {
                process_debounced_events_with_watch(
                    events,
                    strategy,
                    tx,
                    health_state,
                    registered_paths,
                    |path, mode| debouncer.watcher().watch(path, mode),
                );
            }
            Ok(Err(error)) => {
                handle_debounced_watch_result(Err(error), strategy, tx, health_state);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!("[markdown-view] watcher internal channel が切断されました");
                break;
            }
        }
    }
}
```

`keep_watcher_thread_alive` は未使用になるので削除し、tests の `spawn_idle_watcher_thread` は専用ループへ置換する。

```rust
fn spawn_idle_watcher_thread(shutdown_flag: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !shutdown_flag.load(Ordering::Acquire) {
            std::thread::park_timeout(Duration::from_millis(1));
        }
    })
}
```

- [ ] **Step 6: runtime tests を通す**

Run:

```bash
cargo test watcher::runtime::tests -- --nocapture
```

Expected: `watcher::runtime::tests` が PASS。

- [ ] **Step 7: コミットする**

Run:

```bash
git add src/watcher/runtime.rs src/watcher/strategy.rs
git commit -m "feat: watcher新規ディレクトリを動的に監視追加する"
```

---

### Task 6: 統合テストで新規サブディレクトリ追従を固定する

**Files:**
- Modify: `src/watcher/runtime.rs`

- [ ] **Step 1: watcher spawn 経路の統合的テストを追加する**

`src/watcher/runtime.rs` の tests に追加する。

```rust
#[tokio::test]
async fn test_watcher_spawn_ディレクトリモードで起動後新規サブディレクトリを監視する() {
    let dir = tempfile::tempdir().unwrap();
    let initial = dir.path().join("initial.md");
    tokio::fs::write(&initial, "# initial").await.unwrap();
    let mode = AppMode::new_directory(dir.path()).unwrap();
    let (watcher, mut rx) = Watcher::spawn(mode).await.unwrap();

    let new_dir = dir.path().join("new-section");
    tokio::fs::create_dir_all(&new_dir).await.unwrap();
    let new_file = new_dir.join("note.md");
    tokio::fs::write(&new_file, "# before").await.unwrap();

    let first = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("新規ディレクトリ作成後の回復通知または更新通知を受信できる")
        .expect("watch eventを受信できる");
    match first {
        WatchEvent::FileChanged(path) => {
            assert_eq!(path.file_name(), Some(std::ffi::OsStr::new("note.md")));
        }
        WatchEvent::Error(error) => panic!("FileChangedを期待したがError({})を受信", error),
    }

    tokio::fs::write(&new_file, "# after").await.unwrap();

    let second = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("新規サブディレクトリ配下の更新通知を受信できる")
        .expect("watch eventを受信できる");
    match second {
        WatchEvent::FileChanged(path) => {
            assert_eq!(path.file_name(), Some(std::ffi::OsStr::new("note.md")));
        }
        WatchEvent::Error(error) => panic!("FileChangedを期待したがError({})を受信", error),
    }

    watcher.shutdown();
}
```

- [ ] **Step 2: テストを実行する**

Run:

```bash
cargo test watcher::runtime::tests::test_watcher_spawn_ディレクトリモードで起動後新規サブディレクトリを監視する -- --nocapture
```

Expected: PASS。失敗した場合は、event順序が `new_dir` と `new_file` で揺れている可能性があるため、最大5イベントまで受け取り `note.md` の `FileChanged` を探す helper に変更する。

- [ ] **Step 3: watcher runtime 全体を確認する**

Run:

```bash
cargo test watcher::runtime::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 4: コミットする**

Run:

```bash
git add src/watcher/runtime.rs
git commit -m "test: watcher新規サブディレクトリ追従を固定する"
```

---

### Task 7: README と TODO を更新する

**Files:**
- Modify: `README.md`
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: README の該当箇所を確認する**

Run:

```bash
sed -n '1,220p' README.md
```

Expected: watcher / directory mode / troubleshooting の近くに追記できる位置を確認する。

- [ ] **Step 2: README に watcher 除外と inotify 上限を追記する**

`README.md` の watcher 説明またはトラブルシューティング相当の節に次を追加する。既存構成に合わせて見出し位置は調整するが、本文はこの内容を維持する。

```markdown
### ディレクトリ監視と Linux inotify 上限

ディレクトリモードでは、監視リソースを節約しライブ更新の対象と表示対象を一致させるため、`.git`、`node_modules`、`target`、隠しディレクトリを既定でプレビュー一覧・検索・直接表示・監視対象から除外します。通常の Markdown workspace で作成した新しいサブディレクトリは起動後も自動で監視対象に追加されます。

Linux で「監視対象が多すぎるため監視を開始できません」と表示された場合は、inotify の `fs.inotify.max_user_watches` 上限に到達している可能性があります。現在値は `sysctl fs.inotify.max_user_watches` で確認できます。上限を変更する場合は、利用環境の方針に従って一時変更または永続設定を行ってください。
```

- [ ] **Step 3: TODO を Done Summary へ移す**

`docs/todo/TODO.md` の Medium Priority から次の項目ブロックを削除する。

```markdown
- [ ] watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する
```

`## Done Summary` の先頭へ次を追加する。

```markdown
- [x] watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する
  - 完了根拠: ディレクトリモードの監視登録を `WatchPlan` 経由にし、`.git`、`node_modules`、`target`、隠しディレクトリ、symlink directory を notify 登録前に除外する構成にした。起動後に作成された通常サブディレクトリは internal event loop で動的に `NonRecursive` watch へ追加し、既存 Markdown の回復通知も送る。watch 登録の部分成功は init failure とし、起動後の追加 watch 失敗は health failure と `WatchEvent::Error` に分類する。ENOSPC 相当は `WatchErrorKind::ResourceExhausted` として Linux inotify 上限の確認へ進める日本語メッセージを返す。レビュー反映で `.git`、`node_modules`、`target`、隠しディレクトリはプレビュー一覧・検索・直接表示からも共通除外し、README に既定除外と inotify 上限を明記した
```

- [ ] **Step 4: docs 検証を行う**

Run:

```bash
rg -n "watcher 再帰監視|ディレクトリ監視と Linux inotify|ResourceExhausted|node_modules|fs.inotify.max_user_watches" README.md docs/todo/TODO.md
```

Expected:

- README に inotify 節がある。
- TODO の Medium から対象 `[ ]` が消えている。
- Done Summary に対象 `[x]` がある。

- [ ] **Step 5: コミットする**

Run:

```bash
git add README.md docs/todo/TODO.md
git commit -m "docs: watcher除外監視とinotify上限を記録する"
```

---

### Task 8: 全体検証と仕上げ

**Files:**
- Inspect: `src/watcher/strategy.rs`
- Inspect: `src/watcher/runtime.rs`
- Inspect: `src/watcher/error.rs`
- Inspect: `README.md`
- Inspect: `docs/todo/TODO.md`

- [ ] **Step 1: watcher 関連テストをまとめて実行する**

Run:

```bash
cargo test watcher:: -- --nocapture
```

Expected: PASS。

- [ ] **Step 2: 関連統合テストを実行する**

Run:

```bash
cargo test test_ファイル変更でwebsocket更新 test_ディレクトリモード_atomic_save後にwebsocket更新 test_監視エラーがwebsocketクライアントにエラーjsonとして届く -- --nocapture
```

Expected: PASS。

- [ ] **Step 3: lint / format / full verification を実行する**

Run:

```bash
./verify.sh
```

Expected:

```text
==> 検証が正常に完了しました。
```

- [ ] **Step 4: 差分範囲を確認する**

Run:

```bash
git diff --stat origin/develop...HEAD
git status --short --branch
```

Expected:

- 変更ファイルは `src/watcher/strategy.rs`, `src/watcher/runtime.rs`, `src/watcher/error.rs`, `README.md`, `docs/todo/TODO.md`, plan/spec docs の範囲。
- 作業ツリーは clean。

- [ ] **Step 5: 最終コミットが必要なら作成する**

Step 1-4 で修正が出た場合のみ実行する。

```bash
git add src/watcher/strategy.rs src/watcher/runtime.rs src/watcher/error.rs README.md docs/todo/TODO.md
git commit -m "fix: watcher除外監視の検証結果を反映する"
```

Expected: 修正がなければこの step は実行しない。修正がある場合は focused commit が作成される。

## 自己レビュー

Spec coverage:

- 事前除外: Task 2。
- 起動後新規ディレクトリ追従: Task 3, Task 5, Task 6。
- 部分 watch 登録失敗で起動しない: Task 4。
- 起動後追加 watch 失敗の health/error 化: Task 5。
- ENOSPC / inotify user message: Task 1, Task 7。
- README 追記: Task 7。
- TODO 完了化: Task 7。
- 全検証: Task 8。

Placeholder scan:

- 未確定作業を示す語はファイル名 `docs/todo/TODO.md` 以外に残していない。
- 各コード変更 step は対象 file と具体コードを示した。

Residual implementation risks:

- notify の event 順序は OS と backend で揺れる。Task 6 の統合テストが不安定な場合は、固定順序 assertion ではなく最大5イベントの中から `note.md` を探す helper に変更する。
- `path.is_dir()` は symlink を辿るため、runtime の候補抽出では `WatchPlan::for_new_subtree()` 側の `symlink_metadata` が最終防御になる。実装時に symlink directory が登録されない unit test を必ず維持する。
- 起動後に除外対象名へ rename された既存登録済み directory の unwatch は今回扱わない。読み込み前再検証と hidden/base 検証で読み込みは拒否されるが、監視資源の完全回収は将来改善候補に残る。
