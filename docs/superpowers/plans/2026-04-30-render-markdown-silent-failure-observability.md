# render_markdown Silent Failure Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `render_markdown` 責務分割後に残った silent fallback を、公開出力互換を維持したまま debug/test で検知しやすくする。

**Architecture:** `src/renderer/render.rs` と `src/renderer/state.rs` に内部契約用の `debug_assert!` を追加し、release では既存 fallback を維持する。未処理 Markdown event/tag は小さな分類 helper を通して `debug!` ログ経路を固定し、code block fallback HTML は helper 化して escape と出力互換を単体テストで固定する。

**Tech Stack:** Rust 2021、pulldown-cmark 0.13、syntect 5、tracing、Cargo test、既存 `./verify.sh`。

---

## File Structure

- Modify: `src/renderer/state.rs`
  - `finish_heading` の active heading 前提を debug/test で検知する。
  - 既存 module tests に `finish_heading` の契約テストを追加する。
- Modify: `src/renderer/render.rs`
  - `heading_line_attrs` / `code_block_line_attrs` の active state 前提を debug/test で検知する。
  - 未処理 Markdown event/tag を分類する private helper を追加し、既存 `debug!` ログ経路を通す。
  - module tests を追加する。
- Modify: `src/renderer/highlight.rs`
  - escaped fallback HTML 生成を private helper に切り出す。
  - fallback が language/code を escape することを module tests で固定する。
- Modify: `tests/renderer_test.rs`
  - 見出しとコードブロックの行属性が観測性強化後も維持されることを公開 API 経由で固定する。
- Modify: `docs/todo/BACKLOG.md`
  - 実装完了後に対象 backlog を Done へ移す。

## Task 1: Heading / Code Block 内部契約を debug/test で固定

**Files:**
- Modify: `src/renderer/state.rs`
- Modify: `src/renderer/render.rs`

- [ ] **Step 1: `finish_heading` の failing test を追加する**

`src/renderer/state.rs` の既存 `#[cfg(test)] mod tests` 内、`test_finish_code_blockは開始なしならpanicする` の前に次を追加する。

```rust
    #[test]
    #[should_panic(expected = "finish_heading: アクティブな見出し")]
    fn test_finish_headingは開始なしならdebug_assertで検知する() {
        let mut state = RenderState::new();

        let _ = state.finish_heading("heading".to_string(), String::new());
    }
```

- [ ] **Step 2: failing test を確認する**

Run:

```bash
cargo test test_finish_headingは開始なしならdebug_assertで検知する --lib
```

Expected: FAIL。`test did not panic as expected` が出る。

- [ ] **Step 3: `finish_heading` に debug 契約を追加する**

`src/renderer/state.rs` の `finish_heading` を次の形に変更する。

```rust
    /// アクティブな見出しがある状態でのみ呼ぶ。
    ///
    /// debug/test では pulldown-cmark の `Start(Heading)` / `End(Heading)` 対応契約に
    /// 反した呼び出しを検知する。release では既存互換の `None` fallback を維持する。
    pub(super) fn finish_heading(&mut self, id: String, heading_attrs: String) -> Option<String> {
        debug_assert!(
            self.heading.is_some(),
            "finish_heading: アクティブな見出しがない状態で呼ばれた"
        );
        let heading = self.heading.take()?;
        Some(format!(
            "<h{} id=\"{}\"{}>{}</h{}>\n",
            heading.level,
            html_escape(&id),
            heading_attrs,
            heading.html,
            heading.level
        ))
    }
```

- [ ] **Step 4: `finish_heading` の test が通ることを確認する**

Run:

```bash
cargo test test_finish_headingは開始なしならdebug_assertで検知する --lib
```

Expected: PASS。

- [ ] **Step 5: `heading_line_attrs` / `code_block_line_attrs` の failing tests を追加する**

`src/renderer/render.rs` の末尾、`code_block_line_attrs` の後に次を追加する。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "heading_line_attrs: アクティブな見出し")]
    fn test_heading_line_attrsは見出し開始なしならdebug_assertで検知する() {
        let line_lookup = LineLookup::new("# title");
        let state = RenderState::new();

        let _ = heading_line_attrs(&line_lookup, &state);
    }

    #[test]
    #[should_panic(expected = "code_block_line_attrs: アクティブなコードブロック")]
    fn test_code_block_line_attrsはコードブロック開始なしならdebug_assertで検知する() {
        let line_lookup = LineLookup::new("```rust\nfn main() {}\n```");
        let state = RenderState::new();

        let _ = code_block_line_attrs(&(0..0), &line_lookup, &state);
    }
}
```

- [ ] **Step 6: failing tests を確認する**

Run:

```bash
cargo test line_attrsは --lib
```

Expected: FAIL。2 tests が `test did not panic as expected` で失敗する。

- [ ] **Step 7: line attrs helper に debug 契約を追加する**

`src/renderer/render.rs` の `heading_line_attrs` と `code_block_line_attrs` を次の形に変更する。

```rust
fn heading_line_attrs(line_lookup: &LineLookup, state: &RenderState) -> String {
    let range = state.heading_range();
    debug_assert!(
        range.is_some(),
        "heading_line_attrs: アクティブな見出しがない状態で呼ばれた"
    );
    range
        .map(|range| line_block_marker_with(source_line_attrs(line_lookup, range)))
        .unwrap_or_default()
}

fn code_block_line_attrs(
    end_range: &Range<usize>,
    line_lookup: &LineLookup,
    state: &RenderState,
) -> String {
    let range = state.code_block_full_range(end_range);
    debug_assert!(
        range.is_some(),
        "code_block_line_attrs: アクティブなコードブロックがない状態で呼ばれた"
    );
    range
        .map(|range| line_block_marker_with(source_line_attrs(line_lookup, &range)))
        .unwrap_or_default()
}
```

- [ ] **Step 8: Task 1 の tests を通す**

Run:

```bash
cargo test line_attrsは --lib
cargo test test_finish_headingは開始なしならdebug_assertで検知する --lib
```

Expected: PASS。

- [ ] **Step 9: Commit Task 1**

```bash
git add src/renderer/state.rs src/renderer/render.rs
git commit -m "test: renderer内部契約違反を固定"
```

## Task 2: 未処理 Markdown event/tag の観測経路を helper で固定

**Files:**
- Modify: `src/renderer/render.rs`

- [ ] **Step 1: failing tests を追加する**

Task 1 で追加した `src/renderer/render.rs` の `#[cfg(test)] mod tests` 内に、既存 tests の後ろへ次を追加する。

```rust
    #[test]
    fn test_未処理markdown_eventは観測対象として分類される() {
        let event = Event::InlineMath(pulldown_cmark::CowStr::from("x"));

        assert_eq!(
            log_ignored_markdown_event(&event),
            IgnoredMarkdownEventKind::Event
        );
    }

    #[test]
    fn test_未処理markdown_start_tagは観測対象として分類される() {
        assert_eq!(
            log_ignored_markdown_start_tag(&Tag::HtmlBlock),
            IgnoredMarkdownEventKind::StartTag
        );
    }

    #[test]
    fn test_未処理markdown_end_tagは観測対象として分類される() {
        assert_eq!(
            log_ignored_markdown_end_tag(&TagEnd::HtmlBlock),
            IgnoredMarkdownEventKind::EndTag
        );
    }
```

- [ ] **Step 2: failing tests を確認する**

Run:

```bash
cargo test 未処理markdown --lib
```

Expected: FAIL。`IgnoredMarkdownEventKind` または `log_ignored_markdown_*` が未定義で失敗する。

- [ ] **Step 3: 観測 helper を追加する**

`src/renderer/render.rs` の `handle_end` の後ろに次を追加する。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IgnoredMarkdownEventKind {
    Event,
    StartTag,
    EndTag,
}

fn log_ignored_markdown_event(event: &Event<'_>) -> IgnoredMarkdownEventKind {
    tracing::debug!(
        "[markdown-view] 未処理のMarkdownイベントを無視: {:?}",
        event
    );
    IgnoredMarkdownEventKind::Event
}

fn log_ignored_markdown_start_tag(tag: &Tag<'_>) -> IgnoredMarkdownEventKind {
    tracing::debug!(
        "[markdown-view] 未処理のMarkdown開始タグを無視: {:?}",
        tag
    );
    IgnoredMarkdownEventKind::StartTag
}

fn log_ignored_markdown_end_tag(tag: &TagEnd) -> IgnoredMarkdownEventKind {
    tracing::debug!(
        "[markdown-view] 未処理のMarkdown終了タグを無視: {:?}",
        tag
    );
    IgnoredMarkdownEventKind::EndTag
}
```

- [ ] **Step 4: 既存 match 分岐を helper 経由にする**

`dispatch_event` の `other` 分岐を次の形に変更する。

```rust
        other => {
            let _ = log_ignored_markdown_event(&other);
        }
```

`handle_start` の `other` 分岐を次の形に変更する。

```rust
        other => {
            let _ = log_ignored_markdown_start_tag(&other);
        }
```

`handle_end` の `other` 分岐を次の形に変更する。

```rust
        other => {
            let _ = log_ignored_markdown_end_tag(&other);
        }
```

- [ ] **Step 5: Task 2 の tests を通す**

Run:

```bash
cargo test 未処理markdown --lib
```

Expected: PASS。

- [ ] **Step 6: render module 全体を確認する**

Run:

```bash
cargo test renderer::render --lib
```

Expected: PASS。

- [ ] **Step 7: Commit Task 2**

```bash
git add src/renderer/render.rs
git commit -m "refactor: 未処理markdown eventの観測経路を固定"
```

## Task 3: Code block fallback HTML を helper 化して escape を固定

**Files:**
- Modify: `src/renderer/highlight.rs`

- [ ] **Step 1: failing tests を追加する**

`src/renderer/highlight.rs` の末尾に次を追加する。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_code_block_htmlは言語と本文をescapeする() {
        let html = plain_code_block_html(Some("bad\"lang"), "<x>&", " data-line-block");

        assert_eq!(
            html,
            "<pre class=\"code-block\" data-line-block><code class=\"syn-code language-bad&quot;lang\">&lt;x&gt;&amp;</code></pre>\n"
        );
    }

    #[test]
    fn test_plain_code_block_htmlは言語なしでも本文をescapeする() {
        let html = plain_code_block_html(None, "<x>&", "");

        assert_eq!(
            html,
            "<pre class=\"code-block\"><code class=\"syn-code\">&lt;x&gt;&amp;</code></pre>\n"
        );
    }
}
```

- [ ] **Step 2: failing tests を確認する**

Run:

```bash
cargo test plain_code_block_html --lib
```

Expected: FAIL。`plain_code_block_html` が未定義で失敗する。

- [ ] **Step 3: fallback helper を実装する**

`src/renderer/highlight.rs` の `render_code_block_html` の後ろ、`highlighted_code_html` の前に次を追加する。

```rust
fn plain_code_block_html(language: Option<&str>, code: &str, line_attrs: &str) -> String {
    if let Some(lang) = language {
        return format!(
            "<pre class=\"code-block\"{}><code class=\"syn-code language-{}\">{}</code></pre>\n",
            line_attrs,
            html_escape(lang),
            html_escape(code)
        );
    }

    format!(
        "<pre class=\"code-block\"{}><code class=\"syn-code\">{}</code></pre>\n",
        line_attrs,
        html_escape(code)
    )
}
```

- [ ] **Step 4: `render_code_block_html` の fallback を helper 経由にする**

`src/renderer/highlight.rs` の `render_code_block_html` を次の形に変更する。

```rust
pub(super) fn render_code_block_html(
    syntax_set: &SyntaxSet,
    language: Option<&str>,
    code: &str,
    line_attrs: &str,
) -> String {
    if let Some(lang) = language {
        if let Some(highlighted) = highlighted_code_html(syntax_set, lang, code) {
            return format!(
                "<pre class=\"code-block\"{}><code class=\"syn-code language-{}\">{}</code></pre>\n",
                line_attrs,
                html_escape(lang),
                highlighted
            );
        }

        return plain_code_block_html(Some(lang), code, line_attrs);
    }

    plain_code_block_html(None, code, line_attrs)
}
```

- [ ] **Step 5: Task 3 の tests を通す**

Run:

```bash
cargo test plain_code_block_html --lib
```

Expected: PASS。

- [ ] **Step 6: warn 経路が残っていることを確認する**

Run:

```bash
rg -n "コードハイライトエラー|tracing::warn!" src/renderer/highlight.rs
```

Expected: `コードハイライトエラー` を含む `tracing::warn!` が `highlighted_code_html` 内に残っている。

- [ ] **Step 7: Commit Task 3**

```bash
git add src/renderer/highlight.rs
git commit -m "refactor: code block fallback htmlを明示"
```

## Task 4: 公開 API 経由の出力互換を固定

**Files:**
- Modify: `tests/renderer_test.rs`

- [ ] **Step 1: regression test を追加する**

`tests/renderer_test.rs` の `test_render_markdown_複合入力の公開api出力を固定する` の後ろに次を追加する。

```rust
#[test]
fn test_見出しとコードブロックの行属性は観測性強化後も維持される() {
    let md = "# Title\n\n```unknown-lang\n<x>&\n```";
    let html = render_markdown(md);
    let html = html.as_str();

    assert!(html.contains(
        r#"<h1 id="title" data-line-block data-source-start-line="1" data-source-end-line="1">"#
    ));
    assert!(html.contains(
        r#"<pre class="code-block" data-line-block data-source-start-line="3" data-source-end-line="5"><code class="syn-code language-unknown-lang">&lt;x&gt;&amp;"#
    ));
}
```

- [ ] **Step 2: regression test を実行する**

Run:

```bash
cargo test test_見出しとコードブロックの行属性は観測性強化後も維持される --test renderer_test
```

Expected: PASS。

- [ ] **Step 3: renderer test 全体を実行する**

Run:

```bash
cargo test --test renderer_test
```

Expected: PASS。

- [ ] **Step 4: Commit Task 4**

```bash
git add tests/renderer_test.rs
git commit -m "test: renderer行属性の出力互換を固定"
```

## Task 5: Backlog を完了扱いに更新

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: P2 の未完了項目を削除する**

`docs/todo/BACKLOG.md` の P2 から次の未完了項目を削除する。

```markdown
- [ ] `render_markdown` 責務分割後の silent failure 観測性強化
  - ファイル: `src/renderer/{render,state,highlight}.rs`
  - 内容: `heading_line_attrs` / `code_block_line_attrs` / `finish_heading` の `None` 経路、未処理 Markdown event ログ、コードハイライト fallback の観測性を整理する
  - 理由: 責務分割 PR では挙動互換を優先して silent fallback を温存した。次PRで debug_assert / tracing / fallback marker の要否をまとめて判断し、見出し・コードブロック・未処理 event の静かな退行を検知しやすくする
  - 由来: render_markdown 責務分割 PR レビュー (2026-04-29)
```

- [ ] **Step 2: Done に完了項目を追加する**

`docs/todo/BACKLOG.md` の `## Done` 直下に次を追加する。

```markdown
- [x] `render_markdown` 責務分割後の silent failure 観測性強化
  - ファイル: `src/renderer/{render,state,highlight}.rs`, `tests/renderer_test.rs`
  - 内容: `heading_line_attrs` / `code_block_line_attrs` / `finish_heading` の active state 前提を debug/test で検知する契約として固定し、未処理 Markdown event/tag の debug ログ経路と code block fallback HTML の escaped fallback を module test で保護した
  - 完了根拠: `cargo test --all-targets --all-features` と `./verify.sh` が pass
  - 由来: render_markdown 責務分割 PR レビュー (2026-04-29)
```

- [ ] **Step 3: Backlog 差分を確認する**

Run:

```bash
git diff -- docs/todo/BACKLOG.md
```

Expected: P2 から対象未完了項目が消え、Done に完了項目が追加されている。

- [ ] **Step 4: Commit Task 5**

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: render_markdown観測性backlogを完了"
```

## Task 6: 全体検証

**Files:**
- Verify only.

- [ ] **Step 1: format を確認する**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS。

- [ ] **Step 2: clippy を確認する**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS。

- [ ] **Step 3: full test を確認する**

Run:

```bash
cargo test --all-targets --all-features
```

Expected: PASS。

- [ ] **Step 4: repository verification を確認する**

Run:

```bash
./verify.sh
```

Expected: PASS。

- [ ] **Step 5: working tree を確認する**

Run:

```bash
git status --short --branch
```

Expected: 変更なし。現在ブランチは実装用ブランチ。

## Residual Risk

- `debug_assert!` は release では無効なため、release 実行時の挙動は意図的に既存 fallback のまま残る。
- syntect の parse error は安定して発火させにくいため、warn ログの直接捕捉ではなく fallback helper と `rg` による warn 経路確認で保護する。
- 未処理 event/tag helper はログ出力そのものを捕捉しない。依存追加を避けるため、分類 helper を通すことで観測経路の削除を検知する。
