# TOC level=0 Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `HeadingInfo.level=0` を含む内部入力でも TOC 生成が panic しないことをテストで固定する。

**Architecture:** `src/renderer/toc.rs` の private unit test から `build_toc_html` を直接呼び、`level=0` が h1 相当に正規化される境界を固定する。production code は既存の `level.min(...).max(1)` 正規化を維持し、必要なコメントだけで仕様意図を明示する。

**Tech Stack:** Rust, cargo test, `./verify.sh`

---

## File Structure

- Modify: `src/renderer/toc.rs`
  - `#[cfg(test)] mod tests` を拡張する。
  - `HeadingInfo.level=0` の単独、連続、ネスト復帰の境界テストを置く。
  - level 正規化コメントで `1 <= level <= current_level + 1` と underflow/OOB 防止対象を明示する。
- Unchanged: `docs/todo/TODO.md`
  - このPRでは変更しない。TODO更新は merge 後の後続作業として扱う。
- Reference only: `docs/superpowers/specs/2026-05-02-toc-level-zero-guard-design.md`
  - 実装判断の根拠。変更しない。

---

### Task 1: TOC level=0 境界テストを拡張する

**Files:**
- Modify: `src/renderer/toc.rs`

- [ ] **Step 1: 境界テストを拡張する**

`src/renderer/toc.rs` の既存テストモジュールで、単独 `level=0` ケースを維持しつつ、連続 `level=0` と `level=1 -> level=2 -> level=0` の復帰ケースを追加する。

単独ケースの `id` は `r#"zero"level"#` 形式で表現し、期待値では既存どおり `&quot;` に escape されることを固定する。

- [ ] **Step 2: 既存の正規化挙動が固定されることを確認する**

Run:

```bash
cargo test --all-targets --all-features test_build_toc_htmlはlevel0をh1相当に正規化する
```

Expected:

```text
test renderer::toc::tests::test_build_toc_htmlはlevel0をh1相当に正規化する ... ok
```

このリポジトリの現行実装では、すでに `level.max(1)` 正規化があるため、このテストは追加直後から成功する見込み。成功した場合は「既存実装の防御挙動を回帰テストで固定できた」と扱う。

- [ ] **Step 3: 正規化コメントを補う**

`src/renderer/toc.rs` の `let level = ...` 直前のコメントを次の趣旨へ更新する。

```rust
        // 1 <= level <= current_level + 1 に正規化し、急な深化（h1→h4）はh1→h2として扱う
        // HeadingInfo.levelはu8なので、内部境界として0が渡ってもh1相当に正規化する
        // これにより<ul>の直接ネストと、後段のcurrent_level - 1によるunderflow/OOBを防ぐ
        let level = heading.level.min(current_level.saturating_add(1)).max(1);
```

- [ ] **Step 4: 対象テストを再実行する**

Run:

```bash
cargo test --all-targets --all-features test_build_toc_htmlはlevel0をh1相当に正規化する
```

Expected:

```text
test renderer::toc::tests::test_build_toc_htmlはlevel0をh1相当に正規化する ... ok
```

- [ ] **Step 5: TOC 関連テストを実行する**

Run:

```bash
cargo test --all-targets --all-features toc
```

Expected:

```text
test result: ok
```

- [ ] **Step 6: 変更をコミットする**

Run:

```bash
git add src/renderer/toc.rs
git commit -m "test: TOC level=0ガードを固定"
```

Expected:

```text
[docs/toc-level-zero-guard <hash>] test: TOC level=0ガードを固定
```

---

### Task 2: 全体検証を完了する

**Files:**
- Unchanged: `docs/todo/TODO.md`

- [ ] **Step 1: 全体検証を実行する**

Run:

```bash
./verify.sh
```

Expected:

```text
All checks passed
```

実際のスクリプト出力が異なる場合でも、format、clippy、test がすべて成功して終了コード `0` なら合格とする。

- [ ] **Step 2: 最終状態を確認する**

Run:

```bash
git status --short --branch
```

Expected:

```text
## docs/toc-level-zero-guard
```

---

## Security Notes

- `HeadingInfo` は renderer 内部データだが、TOC HTML は `SanitizedHtml` として配信される。追加テストでは `id` と `text` に HTML 特殊文字を含め、`html_escape` の維持を同時に固定する。
- `level=0` を h1 相当に正規化することで、不正または将来の内部データ不整合が panic による可用性低下へつながる経路を閉じる。
- raw HTML の許可、HTML sanitizer、CSP、HTTP/WS の検証境界には触れない。

## Residual Risk

- `HeadingInfo.level` は引き続き `u8` のため、型レベルで `0` を禁止する設計ではない。今回の目的は、既存の正規化挙動を境界テストで固定することに限定する。
- `NonZeroU8` 化は公開APIと renderer 呼び出し側への波及が大きいため、この作業では扱わない。
- `docs/todo/TODO.md` の該当項目は、このPRの merge 後に後続作業で完了扱いへ更新する。
