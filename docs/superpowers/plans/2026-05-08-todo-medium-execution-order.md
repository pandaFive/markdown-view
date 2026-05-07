# TODO Medium 実行順明確化 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/todo/TODO.md` の Medium Priority 5件を、承認済み設計どおりのリスク低減順へ並べ替える。

**Architecture:** docs-only の変更として、`docs/todo/TODO.md` の Medium 節だけを編集する。5件の項目本文は保持し、Medium 見出し直下に実行順の基準を1文追加する。Rust、JavaScript、テスト、設定、`docs/todo/BACKLOG.md` は変更しない。

**Tech Stack:** Markdown、`sed`、`rg`、`git diff`、`./verify.sh`。

---

## 目的・非目的

目的:

- Medium Priority の次着手順を、リスク低減順として読める状態にする。
- 起動不能、silent failure、ブラウザ側信頼境界に近い項目を先に置く。
- 変更範囲の大きい `RenderState` 型化は重要性を残したまま末尾に置く。

非目的:

- TODO 項目の実装。
- Medium から High / BACKLOG への昇降格。
- 各 TODO 項目本文の大幅な要約や再解釈。
- `docs/todo/BACKLOG.md`、`src/`、`tests/`、設定ファイルの変更。

## ファイル構成

- Modify: `docs/todo/TODO.md`
  - 責務: 現在実行候補の High / Medium 改善項目を管理する。今回の実装では Medium 節の並び替えと短い順序説明だけを担う。
- Reference: `docs/superpowers/specs/2026-05-07-todo-medium-execution-order-design.md`
  - 責務: 承認済み設計と受け入れ条件。今回の実装では参照のみで編集しない。

実装後の Medium 順序:

1. `watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する`
2. `BroadcastMessage::Refresh` の memo_refresh 契約と、メモ読込失敗時の degrade 通知を明示する
3. `ブラウザ JS の責務境界を小モジュールへ分割する`
4. `template/mod.rs` のテストをサブモジュールへ分割し、`render_page` の 62 行 `format!` を関数分割する
5. `RenderState` を `enum BlockContext` スタックに置き換えて open/close 対応を型化する

## 事前条件

- 作業ブランチは `docs/todo-medium-execution-order`。
- 設計書コミット `d62c537` と計画コミット `e381e47` がある。
- 作業開始前に未コミット差分がない。ある場合はユーザー作業の可能性があるため、内容を確認してから進める。

### Task 1: Medium Priority の実行順を反映する

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: 作業ブランチと差分なしを確認する**

Run:

```bash
git status --short --branch
```

Expected:

```text
## docs/todo-medium-execution-order
```

追加の `M` / `??` がある場合は、この計画の実行を止める。`docs/todo/TODO.md` 以外の差分を巻き込むと docs-only の受け入れ条件を壊す。

- [ ] **Step 2: 現在の Medium 節を確認する**

Run:

```bash
sed -n '1,120p' docs/todo/TODO.md
```

Expected:

- `## High Priority` は「現時点で未完了の High Priority はなし。」のまま。
- `## Medium Priority` に未完了5件がある。
- 現状では `RenderState` が `template/mod.rs` より前、`ブラウザ JS` が最後にある。

- [ ] **Step 3: Medium 見出し直下に順序説明を追加する**

Modify: `docs/todo/TODO.md`

`## Medium Priority` の直後に、空行を挟んで次の1文を追加する。

```markdown
以下はリスク低減順に実行する。起動不能や silent failure に近い項目を先に扱い、変更範囲が大きい構造変更は後ろへ置く。
```

Expected snippet:

```markdown
## Medium Priority

以下はリスク低減順に実行する。起動不能や silent failure に近い項目を先に扱い、変更範囲が大きい構造変更は後ろへ置く。

- [ ] watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する
```

- [ ] **Step 4: 5件の項目ブロックを設計順へ並べ替える**

Modify: `docs/todo/TODO.md`

既存の項目本文は編集せず、チェックリスト項目単位でブロックを移動する。Medium 節の並びを次の順にする。

```markdown
- [ ] watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する
  - ファイル: `src/watcher/strategy.rs` L43-48, `src/watcher/runtime.rs` L192-198
  - 現状: ディレクトリモードは `RecursiveMode::Recursive` を無条件で適用し、`is_hidden_relative` でイベント受信後にフィルタする。Linux 既定の `fs.inotify.max_user_watches` (8192) を `node_modules`/`target`/`.git` を含む大規模ツリーで枯渇させ、`ENOSPC` 時に `WatchError::init` がそのまま漏れる
  - 対応: `notify` に渡す前段で `.git`/`node_modules`/`target` を最低限除外する。`debouncer.watcher().watch(...)` のエラーが `ENOSPC` 相当のときは `inotify` 上限引き上げ手順を含む user_message に置き換える。CLI/README にも上限の存在を明記
  - 理由: 巨大リポジトリで再現性のある起動失敗を、原因不明の panic ではなく操作可能なメッセージで案内する

- [ ] `BroadcastMessage::Refresh` の memo_refresh 契約と、メモ読込失敗時の degrade 通知を明示する
  - ファイル: `src/server/messages.rs` L38-40, `src/server/routes.rs` L188-220
  - 現状: `Refresh` variant は `serde_json::json!({ "refresh": true })` を返すだけで、ディレクトリモードの遅延回復経路（`content.rs:133`）が memo の再取得指示を欠く。一方 `index_handler` のメモ読込失敗は `MemoResponse::empty` に silent fallback し、ユーザーには「メモが消えた」ように見える
  - 対応: `Refresh` variant に `memo_refresh: bool` を追加するか、仕様コメントを `messages.rs` に明記。`MemoResponse` に degrade flag を追加し、HTML 側でバナー表示できるよう情報を渡す
  - 理由: 仕様契約を型・コメントに固定し、UI が「メモ機能の一時的な機能低下」を区別できるようにする

- [ ] ブラウザ JS の責務境界を小モジュールへ分割する
  - ファイル: `src/template/assets/js/{bootstrap,content,content-renderer,fetch,memo,selection,sidebar,websocket}.js`, `src/template/assets/inline_script.rs`
  - 現状: `docs/superpowers/plans/2026-04-30-browser-js-deglobalization.md` の実行で production の `window` 露出は IIFE と `appContext` 集約により解消済み。E2E用内部操作も `window.__MV_E2E__ === true` 時の `markdownViewTestHooks` に限定した。さらに `content-renderer.js` で `updateContent` の payload 契約、契約違反 warn、`#content` / `#toc` への sanitize 済み HTML 反映、TOC HTML 正規化を明示境界へ切り出した。一方、`content.js` は検索、リンク解決、履歴、スクロール、引用ジャンプ、描画後副作用をまだまとめて扱う巨大ファイルのままで、controller API と依存境界は未整理
  - 対応: 次の分割単位を `document-search` / `directory-search`、`navigation` / `link-resolution`、`createContentController(ctx, deps)` の順で切る。`innerHTML` 使用箇所は引き続き信頼境界を明示し、検索やメモを削る、または純プレビューモードへ戻すことは非目標
  - 理由: 問題は「機能が多いこと」ではなく、workspace として成長した中核機能群の境界がブラウザ JS 内で十分に表現されていないこと。`content-renderer` により最重要の XSS 信頼境界は狭まったが、巨大ファイルと暗黙の `appContext` 依存が残ると将来の入力経路追加で状態遷移を壊しやすい

- [ ] `template/mod.rs` のテストをサブモジュールへ分割し、`render_page` の 62 行 `format!` を関数分割する
  - ファイル: `src/template/mod.rs` (740 行), `src/template/page.rs` L45-104
  - 現状: `template/mod.rs` の 740 行のうち約 720 行が `cfg(test) mod tests` で、page/tree/assets/message にまたがる horizontal integration test を吸収。`render_page` は `<!DOCTYPE html>` から `</html>` までを 62 行の単一 `format!` で組み、`html_escape(memo_file_attr)` などの属性挿入が場当たり的
  - 対応: テストを各サブモジュール（page/tree/assets/message）の `#[cfg(test)] mod tests` に局所化し、`mod.rs` には公開 API 契約テスト（CSP 整合性など）のみ残す。`render_page` は `render_head` / `render_body` / `attr(name, value)` ヘルパーへ分割
  - 理由: CLAUDE.md「300 行を超えたファイルは分割を提案」に該当。エスケープ漏れの一発リスクを集約しないために属性挿入をヘルパー化する

- [ ] `RenderState` を `enum BlockContext` スタックに置き換えて open/close 対応を型化する
  - ファイル: `src/renderer/state.rs` L42-336, `src/renderer/render.rs` L33-55
  - 現状: 14 個の `pub(super)` メソッド（`push_html`/`push_soft_break`/`finish_heading`/`finish_code_block` 等）で State Machine が implicit。`finish_heading` は `debug_assert! + take().?` で release fallback、`finish_code_block` は release でも `unreachable!`、と契約強制が混在
  - 対応: `enum BlockContext { Heading(HeadingState), CodeBlock(CodeBlockState), Image(ImageState), TableCell(TableCellState), ... }` のスタックを `RenderState` に持たせ、`finish_*` を `Result<_, RenderStateMismatch>` 化。`render::dispatch_event` 側は match で完全列挙
  - 理由: 状態機械の不変条件をコメントではなく型で表現し、open/close 不整合を型エラーで弾く
```

Do not edit the `## Done Summary` section.

- [ ] **Step 5: 差分範囲を確認する**

Run:

```bash
git diff --stat
```

Expected: `docs/todo/TODO.md` だけが表示される。`docs/todo/BACKLOG.md`、`docs/superpowers/specs/`、`docs/superpowers/plans/`、`src/`、`tests/`、設定ファイルが出た場合は修正範囲を戻す。

- [ ] **Step 6: Medium 順序を検索で確認する**

Run:

```bash
rg -n "Medium Priority|リスク低減順|watcher 再帰監視|BroadcastMessage::Refresh|ブラウザ JS|template/mod.rs|RenderState" docs/todo/TODO.md
```

Expected: 最初に出る Medium 節の行番号が次の順に増加する。

```text
Medium Priority
リスク低減順
watcher 再帰監視
BroadcastMessage::Refresh
ブラウザ JS
template/mod.rs
RenderState
```

Done Summary に同じ語が出てもよいが、Medium 節の最初の出現順が上記と一致していることを確認する。

- [ ] **Step 7: 受け入れ条件を差分で確認する**

Run:

```bash
git diff -- docs/todo/TODO.md
```

Expected:

- Medium 未完了項目は5件のまま。
- 項目本文の `現状`、`対応`、`理由` は削られていない。
- watcher の ENOSPC、memo の silent fallback、ブラウザ JS の `innerHTML` / XSS 信頼境界に関する説明が残っている。
- `## Done Summary` は変更されていない。

- [ ] **Step 8: 必須検証を実行する**

Run:

```bash
./verify.sh
```

Expected:

```text
==> 検証が正常に完了しました。
```

失敗した場合は、docs-only 変更と無関係な既存失敗か、今回の変更が壊した失敗かを切り分ける。今回の変更が原因でないと判断する場合も、最終報告に失敗コマンド、失敗箇所、残リスクを書く。

- [ ] **Step 9: TODO 並べ替えをコミットする**

Run:

```bash
git add docs/todo/TODO.md
git commit -m "docs: TODO Mediumの実行順を明確化"
```

Expected: `docs/todo/TODO.md` だけを変更する docs commit が1つ作成される。

## 自己レビュー

Spec coverage:

- Medium 節を承認済みリスク低減順へ並べる: Task 1 Step 4 / Step 6。
- Medium 見出し直下に順序説明を追加する: Task 1 Step 3。
- 5件すべてを残す: Task 1 Step 4 / Step 7。
- セキュリティ境界、silent failure、ユーザー影響の説明を失わない: Task 1 Step 4 / Step 7。
- `BACKLOG.md`、プロダクションコード、テストコード、設定ファイルを変更しない: Task 1 Step 1 / Step 5。
- 必須検証を実行する: Task 1 Step 8。
- コミットする: Task 1 Step 9。

Placeholder scan:

- 未解決プレースホルダーはない。
- `TODO` は対象ファイル名・見出し・項目名としてのみ出る。
- `...` は既存 TODO 項目中の `BlockContext` 例に含まれる省略であり、この計画の未確定箇所ではない。

Security considerations:

- この計画は docs-only で、実行時の Host/Origin 検証、パス検証、HTML サニタイズ、CSP、監視イベント処理は変更しない。
- TODO の実行順は将来の修正順へ影響するため、watcher の起動失敗、memo の silent fallback、ブラウザ JS の XSS 信頼境界に近い項目を先に置く。
- TODO 項目本文は過去レビューやコードベース探索由来の未信頼入力として扱う。実装フェーズでは現行コードとテストで再確認してから変更する。

## ロールバック

この実装で作る TODO 並べ替えコミットを revert すれば、`docs/todo/TODO.md` の順序変更を戻せる。docs-only のため、実行時ロールバック手順やデータ移行は不要。
