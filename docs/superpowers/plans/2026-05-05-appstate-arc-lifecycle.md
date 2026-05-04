# AppState Arc Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `AppState` の共有単位を `Arc<AppState>` に統一し、`MemoFs` を生成時注入へ寄せる。

**Architecture:** `AppState` 自体は `Clone` 不能にし、共有は呼び出し元の `Arc<AppState>` が担当する。production は `new_with_tokio_memo_fs`、テストや mock 注入は `new(..., memo_fs)` を使い、生成後差し替えの `with_memo_fs` を削除する。

**Tech Stack:** Rust, tokio broadcast, axum state, async_trait `MemoFs`, cargo test/clippy/fmt

---

## File Structure

- Modify: `src/server/state.rs`
  - `AppState` constructor を 2 系統に分け、`Clone` と `with_memo_fs` を削除する。
  - constructor 注入の契約テストを追加する。
- Modify: `src/main.rs`
  - production の `AppState` 生成を Tokio 用 constructor へ移す。
- Modify: `src/server/watch.rs`
  - watcher test の `AppState` 生成を Tokio 用 constructor へ移す。
- Modify: `src/server/broadcast.rs`
  - broadcast test helper の `AppState` 生成を Tokio 用 constructor へ移す。
- Modify: `src/server/files/test_support.rs`
  - mock `MemoFs` 注入 helper を constructor 注入へ移す。
- Modify: `src/server/service.rs`
  - `with_memo_fs` 利用テストと通常 test helper を新 constructor へ移す。
- Modify: `src/server/files/tests.rs`
  - `AppState::new` 呼び出しを新シグネチャへ移す。
- Modify: `tests/integration_test.rs`
  - integration test helper の `AppState` 生成を Tokio 用 constructor へ移す。

## Preflight

- [ ] **Step 1: 実装用 worktree を作る**

Run:

```bash
git status --short --branch
git worktree add ../markdown-view-appstate-arc -b fix/appstate-arc-lifecycle develop
```

Expected: 現在ブランチと未コミット差分を確認したうえで、`../markdown-view-appstate-arc` が作成される。以降の実装コマンドは worktree 側で実行する。

- [ ] **Step 2: plan と spec を確認する**

Run:

```bash
sed -n '1,220p' docs/superpowers/specs/2026-05-05-appstate-arc-lifecycle-design.md
sed -n '1,260p' docs/superpowers/plans/2026-05-05-appstate-arc-lifecycle.md
```

Expected: Goal、Non-Goals、Acceptance Criteria がこの plan と一致している。

### Task 1: AppState Constructor Contract

**Files:**
- Modify: `src/server/state.rs`

- [ ] **Step 1: constructor 注入の失敗テストを書く**

`src/server/state.rs` の `#[cfg(test)] mod tests` 内に次のテストを追加する。

```rust
#[test]
fn test_app_state_newはmemo_fsを生成時注入する() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let (tx, _rx) = broadcast::channel(16);
    let memo_fs: Arc<dyn MemoFs> = Arc::new(TokioMemoFs);

    let state = AppState::new(mode, false, None, tx, Arc::clone(&memo_fs));

    assert!(Arc::ptr_eq(&memo_fs, state.memo_fs()));
}

#[test]
fn test_app_state_new_with_tokio_memo_fsは本番用memo_fsを組み込む() {
    let (_dir, file_path) = create_markdown_fixture("test.md", "# test");
    let mode = AppMode::new_single_file(&file_path).unwrap();
    let (tx, _rx) = broadcast::channel(16);

    let state = AppState::new_with_tokio_memo_fs(mode, true, Some("base16-ocean.dark".to_string()), tx);

    assert!(state.dark_mode());
    assert!(!state.syntax_css().is_empty());
    assert_eq!(state.tx().receiver_count(), 1);
}
```

- [ ] **Step 2: 失敗を確認する**

Run:

```bash
cargo test --all-targets --all-features server::state::tests::test_app_state_newはmemo_fsを生成時注入する
```

Expected: `AppState::new` の引数数不一致、または `new_with_tokio_memo_fs` 未実装で FAIL。

- [ ] **Step 3: constructor を実装する**

`src/server/state.rs` の `AppState` 定義と `impl AppState` を次の形へ変更する。

```rust
/// サーバー共有状態
#[derive(Debug)]
pub struct AppState {
    mode: AppMode,
    dark_mode: bool,
    syntax_css: String,
    tx: broadcast::Sender<BroadcastMessage>,
    memo_fs: Arc<dyn MemoFs>,
}

impl AppState {
    /// production 用の `AppState` を生成する。
    pub fn new_with_tokio_memo_fs(
        mode: AppMode,
        dark_mode: bool,
        theme: Option<String>,
        tx: broadcast::Sender<BroadcastMessage>,
    ) -> Self {
        Self::new(mode, dark_mode, theme, tx, Arc::new(TokioMemoFs))
    }

    /// `MemoFs` を注入して `AppState` を生成する。
    pub(crate) fn new(
        mode: AppMode,
        dark_mode: bool,
        theme: Option<String>,
        tx: broadcast::Sender<BroadcastMessage>,
        memo_fs: Arc<dyn MemoFs>,
    ) -> Self {
        Self {
            syntax_css: syntax_theme_css(theme.as_deref()),
            mode,
            dark_mode,
            tx,
            memo_fs,
        }
    }
```

同じ `impl AppState` 内の `with_memo_fs` メソッドは削除する。

- [ ] **Step 4: state tests を実行する**

Run:

```bash
cargo test --all-targets --all-features server::state::tests
```

Expected: `server::state::tests` は PASS。別モジュールの `AppState::new` 呼び出しはまだ壊れていてよい。

- [ ] **Step 5: Task 1 を commit する**

Run:

```bash
git add src/server/state.rs
git commit -m "refactor: AppState生成時にMemoFsを注入可能にする"
```

Expected: constructor 契約テストと `AppState` 本体変更だけが commit される。

### Task 2: Production Constructors

**Files:**
- Modify: `src/main.rs`
- Modify: `src/server/watch.rs`
- Modify: `src/server/broadcast.rs`
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: production 呼び出し元のコンパイル失敗を確認する**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: `AppState::new` が 4 引数で呼ばれている箇所のコンパイルエラーが出る。

- [ ] **Step 2: production 経路を Tokio 用 constructor へ移す**

次の置換を行う。

`src/main.rs`:

```rust
let state = Arc::new(AppState::new_with_tokio_memo_fs(
    mode.clone(),
    args.dark,
    args.theme,
    tx,
));
```

`src/server/watch.rs` の test:

```rust
let state = Arc::new(AppState::new_with_tokio_memo_fs(
    AppMode::new_single_file(&file_path).unwrap(),
    false,
    None,
    tx,
));
```

`src/server/broadcast.rs` の helper:

```rust
AppState::new_with_tokio_memo_fs(
    AppMode::new_single_file(file_path).unwrap(),
    false,
    None,
    tx,
)
```

```rust
AppState::new_with_tokio_memo_fs(AppMode::new_directory(dir_path).unwrap(), false, None, tx)
```

`tests/integration_test.rs` の helper:

```rust
Arc::new(AppState::new_with_tokio_memo_fs(
    AppMode::new_single_file(file_path).unwrap(),
    false,
    None,
    tx,
))
```

```rust
Arc::new(AppState::new_with_tokio_memo_fs(
    AppMode::new_directory(base_dir).unwrap(),
    false,
    None,
    tx,
))
```

- [ ] **Step 3: production 関連の targeted tests を実行する**

Run:

```bash
cargo test --all-targets --all-features server::watch::tests::test_watch_service_開始と停止ができる
cargo test --test integration_test --all-features
```

Expected: 対象テストは PASS、または残る失敗が `AppState::new` 呼び出し未移行だけである。

- [ ] **Step 4: Task 2 を commit する**

Run:

```bash
git add src/main.rs src/server/watch.rs src/server/broadcast.rs tests/integration_test.rs
git commit -m "refactor: 本番AppState生成をTokio MemoFs constructorへ移行"
```

Expected: production と integration helper の移行だけが commit される。

### Task 3: Test MemoFs Injection Call Sites

**Files:**
- Modify: `src/server/files/test_support.rs`
- Modify: `src/server/service.rs`
- Modify: `src/server/files/tests.rs`

- [ ] **Step 1: 残っている旧 API 呼び出しを列挙する**

Run:

```bash
rg "AppState::new\\(|with_memo_fs" src tests -n
```

Expected: `with_memo_fs` と 4 引数 `AppState::new` の残りが表示される。

- [ ] **Step 2: test_support helper を constructor 注入へ移す**

`src/server/files/test_support.rs` の `make_test_app_state` を次に置き換える。

```rust
pub(crate) fn make_test_app_state(mode: AppMode, memo_fs: Arc<dyn MemoFs>) -> AppState {
    let (tx, _rx) = broadcast::channel::<BroadcastMessage>(4);
    AppState::new(mode, false, None, tx, memo_fs)
}
```

- [ ] **Step 3: service tests の mock 注入を移す**

`src/server/service.rs` の `create_directory_state(dir.path()).with_memo_fs(memo_fs)` を次の形へ置き換える。

```rust
let (tx, _rx) = broadcast::channel::<BroadcastMessage>(16);
let state = AppState::new(
    AppMode::new_directory(dir.path()).unwrap(),
    false,
    None,
    tx,
    memo_fs,
);
```

同じファイル内の通常 helper は Tokio 用 constructor へ移す。

```rust
fn create_directory_state(base_dir: &Path) -> AppState {
    let (tx, _rx) = broadcast::channel(16);
    AppState::new_with_tokio_memo_fs(AppMode::new_directory(base_dir).unwrap(), false, None, tx)
}
```

```rust
fn create_single_file_state(file_path: &Path) -> AppState {
    let (tx, _rx) = broadcast::channel(16);
    AppState::new_with_tokio_memo_fs(
        AppMode::new_single_file(file_path).unwrap(),
        false,
        None,
        tx,
    )
}
```

- [ ] **Step 4: files tests の helper を Tokio 用 constructor へ移す**

`src/server/files/tests.rs` 末尾付近の helper を次の形へ置き換える。

```rust
fn create_single_file_state(file_path: &Path) -> AppState {
    let (tx, _rx) = broadcast::channel(16);
    AppState::new_with_tokio_memo_fs(
        AppMode::new_single_file(file_path).unwrap(),
        false,
        None,
        tx,
    )
}

fn create_directory_state(dir_path: &Path) -> AppState {
    let (tx, _rx) = broadcast::channel(16);
    AppState::new_with_tokio_memo_fs(AppMode::new_directory(dir_path).unwrap(), false, None, tx)
}
```

- [ ] **Step 5: 残存旧 API がないことを確認する**

Run:

```bash
rg "with_memo_fs" src tests -n
rg "AppState::new\\([^\\n]*$" src tests -n
```

Expected: `with_memo_fs` は 0 件。`AppState::new` は 5 引数の注入用途だけが残る。

- [ ] **Step 6: service/files tests を実行する**

Run:

```bash
cargo test --all-targets --all-features server::service::tests
cargo test --all-targets --all-features server::files::tests
```

Expected: 対象テストは PASS。

- [ ] **Step 7: Task 3 を commit する**

Run:

```bash
git add src/server/files/test_support.rs src/server/service.rs src/server/files/tests.rs
git commit -m "refactor: テスト用MemoFs差し替えを生成時注入へ移行"
```

Expected: mock 注入と test helper 移行だけが commit される。

### Task 4: Full Verification And Cleanup

**Files:**
- Verify: entire repository
- Modify only if needed: files touched in Tasks 1-3

- [ ] **Step 1: formatting を確認する**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS。失敗した場合は `cargo fmt --all` を実行し、format 差分を確認する。

- [ ] **Step 2: clippy を確認する**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS。`AppState::new` の可視性や未使用 import が警告になる場合は、該当 import または visibility を最小限で直す。

- [ ] **Step 3: full test を確認する**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS。

- [ ] **Step 4: repository verify を確認する**

Run:

```bash
./verify.sh
```

Expected: PASS。

- [ ] **Step 5: API 残存と差分を確認する**

Run:

```bash
rg "with_memo_fs|derive\\(Debug, Clone\\).*AppState|pub struct AppState" src/server/state.rs src tests -n
git diff --stat develop...HEAD
git diff --check
```

Expected: `with_memo_fs` は 0 件。`AppState` は `#[derive(Debug)]`。`git diff --check` は空。

- [ ] **Step 6: Task 4 を commit する**

Run:

```bash
git add src/server/state.rs src/main.rs src/server/watch.rs src/server/broadcast.rs src/server/files/test_support.rs src/server/service.rs src/server/files/tests.rs tests/integration_test.rs
git commit -m "test: AppStateライフサイクル整理の検証を反映"
```

Expected: formatting や小さな修正があった場合だけ commit が作成される。差分がなければ commit は作らない。

## Self-Review

- Spec coverage: `Clone` 削除、`Arc<AppState>` 統一、`MemoFs` 生成時注入、`with_memo_fs` 削除、production Tokio 実装、検証コマンドを各 task に割り当て済み。
- Placeholder scan: 後回し前提の空欄や曖昧な作業指示は含めていない。
- Type consistency: `AppState::new_with_tokio_memo_fs(mode, dark_mode, theme, tx)` と `AppState::new(mode, dark_mode, theme, tx, memo_fs)` で統一している。
