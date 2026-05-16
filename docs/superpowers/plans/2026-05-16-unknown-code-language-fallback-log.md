# Unknown Code Language Fallback Log Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 未知言語コードブロックの安全な plain fallback 描画を維持したまま、初回 fallback を debug ログで観測できるようにする。

**Architecture:** `src/renderer/highlight.rs` に未知言語 fallback 専用の小さな helper を追加し、syntax lookup 失敗時だけ `tracing::debug!` を出す。重複抑制は標準ライブラリの `OnceLock<Mutex<HashSet<String>>>` で同一プロセス・同一 language につき 1 回に限定し、HTML 出力経路は変更しない。

**Tech Stack:** Rust, syntect, tracing, standard library `OnceLock` / `Mutex` / `HashSet`, cargo test

---

## File Structure

- Modify: `src/renderer/highlight.rs`
  - 未知言語 fallback のログ、重複抑制、ログ安全な language 表現を追加する。
  - 既存の `plain_code_block_html` と HTML escape 契約は維持する。
- Verify: `tests/renderer_test.rs`
  - 既存の `test_未知言語コードブロックはフォールバック描画される` を変更せず、HTML 出力互換を確認する。

---

### Task 1: 未知言語 fallback helper のテストを追加する

**Files:**
- Modify: `src/renderer/highlight.rs`
- Verify: `tests/renderer_test.rs`

- [ ] **Step 1: helper の failing tests を追加する**

`src/renderer/highlight.rs` の `#[cfg(test)] mod tests` 内に、既存の `test_plain_code_block_htmlは言語なしでも本文をescapeする` の後へ次のテストを追加する。

```rust
    #[test]
    fn test_unknown_language_log_trackerは同じ言語を初回だけ記録対象にする() {
        let tracker = UnknownLanguageLogTracker::new();

        assert!(tracker.mark_seen("unknown-lang"));
        assert!(!tracker.mark_seen("unknown-lang"));
    }

    #[test]
    fn test_unknown_language_log_trackerは異なる言語をそれぞれ初回記録対象にする() {
        let tracker = UnknownLanguageLogTracker::new();

        assert!(tracker.mark_seen("unknown-lang"));
        assert!(tracker.mark_seen("another-lang"));
        assert!(!tracker.mark_seen("unknown-lang"));
        assert!(!tracker.mark_seen("another-lang"));
    }

    #[test]
    fn test_log_safe_languageは制御文字をescapeする() {
        assert_eq!(
            log_safe_language("bad\nlang\t\u{1b}"),
            r#"bad\nlang\t\u{1b}"#
        );
    }
```

- [ ] **Step 2: helper tests がまだ実装前で失敗することを確認する**

Run:

```bash
cargo test --lib renderer::highlight::tests::test_unknown_language_log_tracker -- --nocapture
```

Expected: FAIL。`UnknownLanguageLogTracker` または `log_safe_language` がまだ追加されていないため、または test name filter に一致する複数テストのコンパイル失敗になる。

- [ ] **Step 3: `UnknownLanguageLogTracker` と `log_safe_language` の最小実装を追加する**

`src/renderer/highlight.rs` の先頭付近を次のように更新する。

```rust
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
```

`plain_code_block_html` の前に次の helper を追加する。

```rust
static UNKNOWN_LANGUAGE_LOG_TRACKER: OnceLock<UnknownLanguageLogTracker> = OnceLock::new();

struct UnknownLanguageLogTracker {
    seen: Mutex<HashSet<String>>,
}

impl UnknownLanguageLogTracker {
    fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
        }
    }

    fn mark_seen(&self, language: &str) -> bool {
        let mut seen = self
            .seen
            .lock()
            .expect("未知言語ログの重複抑制状態をロックできること");
        seen.insert(language.to_string())
    }
}

fn unknown_language_log_tracker() -> &'static UnknownLanguageLogTracker {
    UNKNOWN_LANGUAGE_LOG_TRACKER.get_or_init(UnknownLanguageLogTracker::new)
}

fn log_safe_language(language: &str) -> String {
    language.escape_debug().to_string()
}
```

- [ ] **Step 4: helper tests が通ることを確認する**

Run:

```bash
cargo test --lib renderer::highlight::tests::test_unknown_language_log_tracker -- --nocapture
cargo test --lib renderer::highlight::tests::test_log_safe_languageは制御文字をescapeする -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: ここまでをコミットする**

```bash
git add src/renderer/highlight.rs
git commit -m "test: 未知言語fallbackログhelperを固定"
```

---

### Task 2: syntax lookup 失敗時の debug ログを追加する

**Files:**
- Modify: `src/renderer/highlight.rs`
- Verify: `tests/renderer_test.rs`

- [ ] **Step 1: `log_unknown_language_fallback` の failing test を追加する**

`src/renderer/highlight.rs` の `#[cfg(test)] mod tests` 内に次のテストを追加する。

```rust
    #[test]
    fn test_log_unknown_language_fallbackは同じ言語を一度だけログ対象にする() {
        let tracker = UnknownLanguageLogTracker::new();

        assert!(should_log_unknown_language_fallback(&tracker, "unknown-lang"));
        assert!(!should_log_unknown_language_fallback(&tracker, "unknown-lang"));
        assert!(should_log_unknown_language_fallback(&tracker, "another-lang"));
    }
```

- [ ] **Step 2: 新規 test がまだ実装前で失敗することを確認する**

Run:

```bash
cargo test --lib renderer::highlight::tests::test_log_unknown_language_fallbackは同じ言語を一度だけログ対象にする -- --nocapture
```

Expected: FAIL。`should_log_unknown_language_fallback` がまだ追加されていない。

- [ ] **Step 3: `should_log_unknown_language_fallback` と debug ログを実装する**

`log_safe_language` の後に次を追加する。

```rust
fn should_log_unknown_language_fallback(
    tracker: &UnknownLanguageLogTracker,
    language: &str,
) -> bool {
    tracker.mark_seen(language)
}

fn log_unknown_language_fallback(language: &str) {
    if should_log_unknown_language_fallback(unknown_language_log_tracker(), language) {
        tracing::debug!(
            "[markdown-view] 未知のコードブロック言語のためプレーン表示にフォールバックしました (lang={})",
            log_safe_language(language)
        );
    }
}
```

`highlighted_code_html` の syntax lookup を `?` から明示分岐に変える。

```rust
fn highlighted_code_html(syntax_set: &SyntaxSet, language: &str, code: &str) -> Option<String> {
    let Some(syntax) = syntax_set
        .find_syntax_by_token(language)
        .or_else(|| syntax_set.find_syntax_by_extension(language))
    else {
        log_unknown_language_fallback(language);
        return None;
    };

    let mut generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        syntax_set,
        ClassStyle::SpacedPrefixed { prefix: "syn-" },
    );

    for line in LinesWithEndings::from(code) {
        if let Err(e) = generator.parse_html_for_line_which_includes_newline(line) {
            tracing::warn!(
                "[markdown-view] コードハイライトエラー (lang={}): {}",
                language,
                e
            );
            return None;
        }
    }

    Some(generator.finalize())
}
```

- [ ] **Step 4: 新規 test が通ることを確認する**

Run:

```bash
cargo test --lib renderer::highlight::tests::test_log_unknown_language_fallbackは同じ言語を一度だけログ対象にする -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: renderer fallback の既存契約が変わっていないことを確認する**

Run:

```bash
cargo test --test renderer_test test_未知言語コードブロックはフォールバック描画される -- --nocapture
```

Expected: PASS。HTML に `<pre class="code-block"><code class="syn-code language-unknown-lang">` と `let x = 1;` が残る。

- [ ] **Step 6: ここまでをコミットする**

```bash
git add src/renderer/highlight.rs
git commit -m "fix: 未知言語fallbackをdebugログ化"
```

---

### Task 3: 全体検証と BACKLOG 更新判断

**Files:**
- Modify: `docs/todo/BACKLOG.md` only if the implemented item should be marked done or moved to Done.
- Verify: `src/renderer/highlight.rs`, `tests/renderer_test.rs`

- [ ] **Step 1: 対象テストを実行する**

Run:

```bash
cargo test --lib renderer::highlight -- --nocapture
cargo test --test renderer_test test_コードハイライトはクラスベースでインラインstyleを出力しない -- --nocapture
cargo test --test renderer_test test_未知言語コードブロックはフォールバック描画される -- --nocapture
```

Expected: PASS。

- [ ] **Step 2: 全 Rust テストを実行する**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS。

- [ ] **Step 3: required verification を実行する**

Run:

```bash
./verify.sh
```

Expected: PASS。format、clippy、tests が通る。

- [ ] **Step 4: `docs/todo/BACKLOG.md` の対象項目を完了扱いにするか判断する**

実装後、次の項目だけを確認する。

```bash
rg -n "未知言語コードブロックの silent fallback に警告ログを追加" docs/todo/BACKLOG.md
```

実装と検証が完了している場合は、`docs/todo/BACKLOG.md` の該当項目を `Done` へ移すか、既存のタスク文書運用に合わせて完了サマリへ圧縮する。コード変更と backlog 整理を同じ PR に含める場合は、次のような完了根拠を残す。

```markdown
- [x] 未知言語コードブロックの silent fallback に警告ログを追加
  - ファイル: `src/renderer/highlight.rs`, `tests/renderer_test.rs`
  - 内容: 未知言語の syntax lookup 失敗時に、同一 language につき初回だけ `tracing::debug!` を出すようにした。HTML fallback 出力は維持し、ログに出す language は制御文字を escape する。
  - 完了根拠: `cargo test --lib renderer::highlight`, `cargo test --test renderer_test test_未知言語コードブロックはフォールバック描画される`, `cargo test --all-targets --all-features`, `./verify.sh`
```

docs 更新を別 PR に分ける場合は、`BACKLOG.md` はこのタスクでは変更しない。

- [ ] **Step 5: 最終 diff を確認する**

Run:

```bash
git diff -- src/renderer/highlight.rs tests/renderer_test.rs docs/todo/BACKLOG.md
git status --short
```

Expected: 変更が `src/renderer/highlight.rs` と、必要に応じて `docs/todo/BACKLOG.md` に限定されている。`tests/renderer_test.rs` は原則変更なし。

- [ ] **Step 6: 最終コミットを作成する**

`BACKLOG.md` を更新した場合:

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: 未知言語fallbackログ完了を記録"
```

`BACKLOG.md` を更新しない場合は、この step で追加コミットは不要。

---

## Self-Review

- Spec coverage: syntax lookup 失敗時だけ debug ログ、同一 language 1 回だけの重複抑制、制御文字 escape、HTML 出力維持、parse error の既存 warn 維持を各タスクで扱う。
- Placeholder scan: `TB[D]`、`TO[DO]`、未確定の抽象 step は含めない。BACKLOG 更新は実装完了後の運用判断として具体的な選択肢と文面を示した。
- Type consistency: `UnknownLanguageLogTracker`, `mark_seen`, `unknown_language_log_tracker`, `log_safe_language`, `should_log_unknown_language_fallback`, `log_unknown_language_fallback` の名前は全タスクで一致している。
