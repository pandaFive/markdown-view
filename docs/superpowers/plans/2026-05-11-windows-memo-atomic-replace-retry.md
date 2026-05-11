# Windows Memo Atomic Replace Retry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Windows のメモ atomic replace で `JoinError` を分類し、`MoveFileExW` の一時 lock 系エラーだけを短く retry する。

**Architecture:** 変更は `src/server/files/memo_fs.rs` に閉じる。`MemoFs` trait、`MemoWriteError`、HTTP API 契約は変えず、Windows-only `atomic_replace` の内側に retry 判定、JoinError 分類、1回実行 helper を追加する。retry は初回失敗後に `10ms, 25ms, 50ms` の最大3回で、同じ tmp/final path に対する `MoveFileExW` だけを再実行する。

**Tech Stack:** Rust, tokio, windows-sys, cargo test, cargo check cross target, `./verify.sh`

---

## File Structure

- Modify: `src/server/files/memo_fs.rs`
  - Windows raw error code 定数と retry delay 定数を追加する。
  - `is_retryable_windows_replace_error` を追加する。
  - `map_atomic_replace_join_error` を追加し、panic/cancel をログ上分類する。
  - Windows-only `move_file_ex_replace_once` を追加し、`MoveFileExW` の1回呼び出しを閉じ込める。
  - Windows-only `atomic_replace` を retry loop に置き換える。
  - Linux でも実行できる helper unit test と、既存 Windows-only success test を維持する。
- No change: `src/server/files/memo.rs`
  - `MemoWriteError` から API error への変換は既存通り使う。
- No change: `src/server/files/test_support.rs`
  - `MockMemoFs` の atomic write 契約は変えない。
- No change: `tests/integration_test.rs`
  - HTTP 契約を変えないため統合テストは追加しない。

---

### Task 1: Retry 判定と JoinError 分類の failing tests を追加する

**Files:**
- Modify: `src/server/files/memo_fs.rs`

- [ ] **Step 1: retry 判定の failing unit test を追加する**

`src/server/files/memo_fs.rs` の `#[cfg(test)] mod tests` 内、`use super::*;` の直後に次を追加する。この時点では `is_retryable_windows_replace_error` が未定義なので compile failure になる。

```rust
    #[test]
    fn is_retryable_windows_replace_errorは一時lock系windowsエラーだけtrueにする() {
        for code in [
            WINDOWS_ERROR_ACCESS_DENIED,
            WINDOWS_ERROR_SHARING_VIOLATION,
            WINDOWS_ERROR_LOCK_VIOLATION,
        ] {
            let error = io::Error::from_raw_os_error(code);
            assert!(
                is_retryable_windows_replace_error(&error),
                "Windows error {code} should be retryable"
            );
        }
    }

    #[test]
    fn is_retryable_windows_replace_errorは対象外エラーをfalseにする() {
        for error in [
            io::Error::from(io::ErrorKind::AlreadyExists),
            io::Error::from(io::ErrorKind::Other),
            io::Error::from_raw_os_error(12345),
        ] {
            assert!(
                !is_retryable_windows_replace_error(&error),
                "unexpected retryable error: {error}"
            );
        }
    }
```

- [ ] **Step 2: retry 判定 test が失敗することを確認する**

Run:

```bash
cargo test --all-targets --all-features is_retryable_windows_replace_error
```

Expected: FAIL with `cannot find function is_retryable_windows_replace_error in this scope` or missing Windows error constants.

- [ ] **Step 3: JoinError 分類の failing unit test を追加する**

同じ `tests` module に続けて追加する。この時点では `map_atomic_replace_join_error` が未定義なので compile failure になる。

```rust
    #[tokio::test]
    async fn map_atomic_replace_join_errorはpanicを分類する() {
        let handle = tokio::spawn(async {
            panic!("atomic replace panic classification test");
        });
        let join_error = handle.await.expect_err("task should panic");

        let error = map_atomic_replace_join_error(join_error);

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(
            error.to_string().contains("panicked"),
            "panic classification should be visible in io error: {error}"
        );
    }

    #[tokio::test]
    async fn map_atomic_replace_join_errorはcancelledを分類する() {
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        handle.abort();
        let join_error = handle.await.expect_err("task should be cancelled");

        let error = map_atomic_replace_join_error(join_error);

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(
            error.to_string().contains("cancelled"),
            "cancel classification should be visible in io error: {error}"
        );
    }
```

- [ ] **Step 4: JoinError 分類 test が失敗することを確認する**

Run:

```bash
cargo test --all-targets --all-features map_atomic_replace_join_error
```

Expected: FAIL with `cannot find function map_atomic_replace_join_error in this scope`.

- [ ] **Step 5: Task 1 の差分を保持して次へ進む**

Expected: `src/server/files/memo_fs.rs` に failing tests が残っている。失敗する test だけではコミットせず、Task 2 で helper 実装と合わせて pass させてからコミットする。

---

### Task 2: Retry 判定と JoinError 分類 helper を実装する

**Files:**
- Modify: `src/server/files/memo_fs.rs`

- [ ] **Step 1: Windows error code 定数を追加する**

`const ATOMIC_TMP_ATTEMPTS: u8 = 8;` の直後に次を追加する。`ERROR_ACCESS_DENIED` などの windows-sys 定数 import は不要にし、Linux test でも同じ helper を compile できるよう raw code を明示する。

```rust
#[cfg(any(test, windows))]
const WINDOWS_ERROR_ACCESS_DENIED: i32 = 5;
#[cfg(any(test, windows))]
const WINDOWS_ERROR_SHARING_VIOLATION: i32 = 32;
#[cfg(any(test, windows))]
const WINDOWS_ERROR_LOCK_VIOLATION: i32 = 33;
```

- [ ] **Step 2: retry 判定 helper を追加する**

`cleanup_tmp_best_effort` の後、`#[cfg(not(windows))] async fn atomic_replace` の前に次を追加する。

```rust
#[cfg(any(test, windows))]
fn is_retryable_windows_replace_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(
            WINDOWS_ERROR_ACCESS_DENIED
                | WINDOWS_ERROR_SHARING_VIOLATION
                | WINDOWS_ERROR_LOCK_VIOLATION
        )
    )
}
```

- [ ] **Step 3: JoinError 分類 helper を追加する**

`is_retryable_windows_replace_error` の直後に次を追加する。

```rust
#[cfg(any(test, windows))]
fn map_atomic_replace_join_error(error: tokio::task::JoinError) -> io::Error {
    if error.is_panic() {
        tracing::error!(
            "[markdown-view] メモatomic replace blocking taskがpanicしました"
        );
        io::Error::other("memo atomic replace task panicked")
    } else if error.is_cancelled() {
        tracing::warn!(
            "[markdown-view] メモatomic replace blocking taskがcancelledされました"
        );
        io::Error::other("memo atomic replace task cancelled")
    } else {
        tracing::warn!(
            "[markdown-view] メモatomic replace blocking taskのjoinに失敗しました"
        );
        io::Error::other("memo atomic replace task failed")
    }
}
```

- [ ] **Step 4: helper test を実行して pass を確認する**

Run:

```bash
cargo test --all-targets --all-features is_retryable_windows_replace_error
cargo test --all-targets --all-features map_atomic_replace_join_error
```

Expected: PASS. The panic classification test may print a panic message from the spawned task; the test still passes because the `JoinError` is asserted.

- [ ] **Step 5: Task 1 と Task 2 の変更をコミットする**

Run:

```bash
git add src/server/files/memo_fs.rs
git commit -m "fix: Windowsメモatomic replaceの失敗分類を追加"
```

Expected: tests and helper implementation are committed together.

---

### Task 3: Windows atomic_replace に MoveFileExW retry loop を実装する

**Files:**
- Modify: `src/server/files/memo_fs.rs`

- [ ] **Step 1: `move_file_ex_replace_once` helper を追加する**

`const ATOMIC_TMP_ATTEMPTS: u8 = 8;` 周辺に retry delay 定数を追加する。

```rust
#[cfg(windows)]
const WINDOWS_REPLACE_RETRY_DELAYS_MS: [u64; 3] = [10, 25, 50];
```

既存の `#[cfg(windows)] async fn atomic_replace` の前に次を追加する。

```rust
#[cfg(windows)]
async fn move_file_ex_replace_once(tmp_path: Vec<u16>, path: Vec<u16>) -> io::Result<()> {
    tokio::task::spawn_blocking(move || {
        // SAFETY: 両パスはNUL終端済みで、interior NULを拒否したバッファとしてこの呼び出し中は生存する。
        let result = unsafe {
            MoveFileExW(
                tmp_path.as_ptr(),
                path.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })
    .await
    .map_err(map_atomic_replace_join_error)?
}
```

- [ ] **Step 2: `atomic_replace` を retry loop に置き換える**

既存の Windows-only `atomic_replace` 全体を次に置き換える。retry ごとに `Vec<u16>` を clone し、同じ検証済み tmp/final path で `MoveFileExW` だけを再実行する。

```rust
#[cfg(windows)]
async fn atomic_replace(tmp_path: &Path, path: &Path) -> io::Result<()> {
    let tmp_path_wide = path_to_wide_null(tmp_path)?;
    let path_wide = path_to_wide_null(path)?;

    for attempt in 0..=WINDOWS_REPLACE_RETRY_DELAYS_MS.len() {
        match move_file_ex_replace_once(tmp_path_wide.clone(), path_wide.clone()).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                let should_retry = attempt < WINDOWS_REPLACE_RETRY_DELAYS_MS.len()
                    && is_retryable_windows_replace_error(&error);
                if !should_retry {
                    return Err(error);
                }

                let delay_ms = WINDOWS_REPLACE_RETRY_DELAYS_MS[attempt];
                let file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_else(|| "<unknown>".into());
                tracing::warn!(
                    "[markdown-view] メモatomic replaceが一時的なWindowsエラーで失敗したためretryします (attempt {}/{}, file: {}, delay_ms: {}, error: {})",
                    attempt + 1,
                    WINDOWS_REPLACE_RETRY_DELAYS_MS.len(),
                    file_name,
                    delay_ms,
                    error
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
        }
    }

    unreachable!("atomic replace retry loop must return before exhausting attempts")
}
```

- [ ] **Step 3: targeted tests を実行する**

Run:

```bash
cargo test --all-targets --all-features is_retryable_windows_replace_error
cargo test --all-targets --all-features map_atomic_replace_join_error
cargo test --all-targets --all-features write_atomicはtmpを書いてからrenameする
```

Expected: PASS on Linux/macOS. The existing Windows-only `atomic_replace` tests are compiled out on non-Windows.

- [ ] **Step 4: Windows target check を試す**

Run:

```bash
cargo check --target x86_64-pc-windows-gnu
```

Expected: PASS if the Windows target and linker are installed. If it fails because the target or linker is missing, record the exact failure in the completion report and continue with Linux verification.

- [ ] **Step 5: Task 3 の変更をコミットする**

Run:

```bash
git add src/server/files/memo_fs.rs
git commit -m "fix: Windowsメモatomic replaceを短時間retryする"
```

Expected: commit succeeds.

---

### Task 4: Final verification と TODO 追跡更新

**Files:**
- Modify: `docs/todo/TODO.md`
- No change expected: source files beyond `src/server/files/memo_fs.rs`

- [ ] **Step 1: issue #143 相当の TODO 記載を確認する**

Run:

```bash
rg -n "143|Windows メモ原子保存|Windows メモ atomic|MoveFileExW|atomic replace" docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected: `docs/todo/TODO.md` の High Priority に `Windows メモ原子保存のエラー処理と retry 条件を細分化する` が見つかる。

実行後の現在状態: 対象 item は High Priority から Done Summary へ移動済み。

- [ ] **Step 2: TODO の open item を Done Summary へ移す**

`docs/todo/TODO.md` の High Priority から次の block を削除する。

```markdown
- [ ] Windows メモ原子保存のエラー処理と retry 条件を細分化する
  - ファイル: `src/server/files/memo_fs.rs`
  - 現状: Windows の `MoveFileExW` 呼び出しは `spawn_blocking` 経由だが、`JoinError` は `ErrorKind::Other` に潰している。また tmp 作成 retry は `AlreadyExists` のみを対象にしており、Windows の共有違反・削除保留・ウイルス対策ソフトによる一時ロックを retry しない
  - 対応: `JoinError::is_panic()` / `is_cancelled()` を分けて `tracing::error!` に残す。Windows では `raw_os_error()` で sharing violation / delete pending 相当を判定し、短い retry 対象に含める。Windows CI または `cargo check --target x86_64-pc-windows-gnu` が通る環境で検証する
  - 昇格理由: メモ保存はデータ安全性に関わり、失敗理由を潰すと復旧判断が弱くなるため High とする
  - 由来: メモ原子保存 PR 3rd レビュー (2026-04-30)
```

同じ `docs/todo/TODO.md` の `## Done Summary` 直下に次を追加する。

```markdown
- [x] Windows メモ原子保存のエラー処理と retry 条件を細分化する
  - 完了根拠: `MoveFileExW` の `JoinError` を panic / cancelled / その他 join error に分類してログと `io::Error` message に残す構成にした。Windows の `ERROR_ACCESS_DENIED`、`ERROR_SHARING_VIOLATION`、`ERROR_LOCK_VIOLATION` だけを一時 lock 系として扱い、初回失敗後に `10ms`、`25ms`、`50ms` の最大3回だけ同じ tmp/final path で retry する。retry は `MoveFileExW` 呼び出しだけに限定し、tmp 作成、本文書き込み、rename 前検証、cleanup、親ディレクトリ sync、HTTP API 契約は変更していない。retry 対象判定と JoinError 分類は unit test で固定し、Windows target check の可否を完了報告に残す
```

Expected: source behavior changes are complete before this doc update; this step only moves the tracking item.

- [ ] **Step 3: full Rust test を実行する**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS.

- [ ] **Step 4: required verification を実行する**

Run:

```bash
./verify.sh
```

Expected: PASS for format, clippy, and tests.

- [ ] **Step 5: final status を確認する**

Run:

```bash
git status --short --branch
```

Expected: only intentional `docs/todo/TODO.md` update remains, or clean if the TODO update was already committed.

- [ ] **Step 6: TODO 更新をコミットする**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: Windowsメモatomic replace retry完了を反映"
```

Expected: commit succeeds.

---

## Completion Report Requirements

実装完了時は次を報告する。

- Changed files with reason and rough line impact.
- Affected dependent files: `src/server/files/memo.rs`, `src/server/files/test_support.rs`, `tests/integration_test.rs` are contract-adjacent but should remain unchanged.
- Verification results: targeted tests, `cargo test --all-targets --all-features`, `./verify.sh`, and `cargo check --target x86_64-pc-windows-gnu` result or skip reason.
- Security considerations: API contract unchanged, path validation unchanged, retry limited to MoveFileExW, no memo body or absolute path in API responses.
- Residual risk: no real Windows transient lock reproduction unless Windows CI or a Windows local environment ran the Windows-only tests.
