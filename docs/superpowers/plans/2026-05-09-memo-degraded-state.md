# Memo Degraded State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** メモ読み込み失敗を `memo_state: "degraded"` と専用バナーで明示し、WebSocket refresh 契約にも `memo_refresh: true` を含める。

**Architecture:** `MemoResponse` に `MemoState` enum を持たせ、Rust の初期 HTML とブラウザ JS の動的更新が同じ degraded contract を見る構成にする。degraded 時はメモ編集と autosave を止め、本文閲覧は継続する。`BroadcastMessage::Refresh` は本文とメモの再取得を同時に表す JSON 契約へ揃える。

**Tech Stack:** Rust, serde, axum, existing no-build browser JavaScript, Playwright E2E, cargo test, ./verify.sh.

---

## 参照設計

- `docs/superpowers/specs/2026-05-08-memo-degraded-state-design.md`

## ファイル構成

- Modify: `src/template/message.rs`
  - 責務: HTTP/API/HTML で共有する `MemoResponse` の JSON 契約。`MemoState` を追加し、`ready` / `degraded` を serialize する。
- Modify: `src/template/mod.rs`
  - 責務: template 公開 API と template 統合テスト。`MemoState` を re-export し、degraded HTML の回帰テストを更新する。
- Modify: `src/template/page.rs`
  - 責務: 初期 HTML のメモパネル描画。degraded バナー、status、textarea disabled を `memo_state` から決める。
- Modify: `src/template/assets/js/memo.js`
  - 責務: メモ API 応答と WebSocket 由来 reload を UI に反映する。degraded バナー DOM と editor disabled を動的に更新する。
- Modify: `src/template/assets/css/memo.css`
  - 責務: メモパネルの見た目。degraded バナーの最小スタイルを追加する。
- Modify: `src/server/service.rs`
  - 責務: 初期ページ用 service。メモ読込失敗 fallback が degraded state を返すことをテストで固定する。
- Modify: `src/server/messages.rs`
  - 責務: WebSocket broadcast JSON 契約。`Refresh` に `memo_refresh: true` を追加する。
- Modify: `tests/e2e/globals.d.ts`
  - 責務: E2E payload 型。`memo_state?: 'ready' | 'degraded'` を追加する。
- Modify: `tests/e2e/memo_quote.spec.ts`
  - 責務: メモ degraded UI と復旧の E2E 回帰テスト。
- Reference: `src/template/assets/js/websocket.js`
  - `Refresh` payload は既存の `isMemoRefreshMessage` と `queueRemoteMemoReload` 経路で処理されるため、基本的に編集しない。

## 事前条件

- 作業ブランチは `docs/memo-degraded-state-design` から実装用 branch / worktree を作る。
- 設計コミット `278f90c` が含まれている。
- 作業開始前に未コミット差分がない。

Run:

```bash
git status --short --branch
```

Expected:

```text
## <implementation-branch>
```

追加の `M` / `??` がある場合は、ユーザー作業を巻き込まないよう内容を確認してから進める。

---

### Task 1: `MemoResponse` に `MemoState` 契約を追加する

**Files:**
- Modify: `src/template/message.rs`
- Modify: `src/template/mod.rs`

- [ ] **Step 1: 失敗テストを追加する**

`src/template/message.rs` の tests に次を追加する。

```rust
#[test]
fn test_memo_response_from_rawはready_stateを直列化する() {
    let memo = MemoResponse::from_raw("memo".to_string(), Some("docs/guide.md".to_string()));

    assert_eq!(memo.memo_state(), MemoState::Ready);
    let value = serde_json::to_value(memo).unwrap();

    assert_eq!(value["memo_state"], "ready");
    assert_eq!(value["raw"], "memo");
    assert!(value.get("load_error").is_none());
}

#[test]
fn test_memo_response_empty_with_load_errorはdegraded_stateを直列化する() {
    let memo = MemoResponse::empty_with_load_error(
        Some("docs/guide.md".to_string()),
        "メモ読み込み失敗",
    );

    assert_eq!(memo.memo_state(), MemoState::Degraded);
    let value = serde_json::to_value(memo).unwrap();

    assert_eq!(value["memo_state"], "degraded");
    assert_eq!(value["load_error"], "メモ読み込み失敗");
    assert_eq!(value["raw"], "");
}
```

既存の `test_memo_response_load_errorは直列化される` には次の assert を追加する。

```rust
assert_eq!(memo.memo_state(), MemoState::Degraded);
assert_eq!(value["memo_state"], "degraded");
```

- [ ] **Step 2: テストが失敗することを確認する**

Run:

```bash
cargo test template::message::tests::test_memo_response_from_rawはready_stateを直列化する template::message::tests::test_memo_response_empty_with_load_errorはdegraded_stateを直列化する
```

Expected: `MemoState` と `memo_state()` が未定義で FAIL。

- [ ] **Step 3: `MemoState` と `memo_state` field を追加する**

`src/template/message.rs` の `MemoResponse` より前に次を追加する。

```rust
/// メモ機能の利用可能状態。
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoState {
    /// メモを読み書きできる通常状態。
    Ready,
    /// メモ読み込みに失敗し、内容保護のため編集を止めている状態。
    Degraded,
}
```

`MemoResponse` に field を追加する。

```rust
memo_state: MemoState,
```

`MemoResponse::from_raw` の戻り値を次の形にする。

```rust
Self {
    raw,
    html,
    file,
    memo_state: MemoState::Ready,
    load_error: None,
}
```

`MemoResponse::empty_with_load_error` は `load_error` と同時に degraded を設定する。

```rust
pub fn empty_with_load_error(file: Option<String>, message: impl Into<String>) -> Self {
    let mut response = Self::empty(file);
    response.memo_state = MemoState::Degraded;
    response.load_error = Some(message.into());
    response
}
```

`MemoResponse` impl に accessor を追加する。

```rust
/// メモ機能の利用可能状態を返す
pub fn memo_state(&self) -> MemoState {
    self.memo_state
}
```

- [ ] **Step 4: `MemoState` を公開する**

`src/template/mod.rs` の re-export を次へ変更する。

```rust
pub use self::message::{
    error_message_json, MemoResponse, MemoState, MemoUpdateMessage, UpdateMessage,
};
```

- [ ] **Step 5: message tests を通す**

Run:

```bash
cargo test template::message::tests
```

Expected: `template::message::tests` が PASS。

- [ ] **Step 6: コミットする**

```bash
git add src/template/message.rs src/template/mod.rs
git commit -m "feat: メモ応答にdegraded stateを追加"
```

---

### Task 2: 初期 HTML に degraded バナーを追加する

**Files:**
- Modify: `src/template/page.rs`
- Modify: `src/template/assets/css/memo.css`
- Modify: `src/template/mod.rs`

- [ ] **Step 1: 失敗テストを更新する**

`src/template/mod.rs` の `test_メモ読み込み失敗時はエラー表示して編集を無効化する` を次の assert に更新する。

```rust
assert!(html.contains("id=\"memo-degraded-banner\""));
assert!(html.contains("role=\"status\""));
assert!(html.contains("内容を保護するため編集を無効化"));
assert!(html.contains("data-state=\"error\""));
assert!(html.contains(">読込失敗</span>"));
assert!(html.contains("id=\"memo-editor\""));
assert!(html.contains("disabled aria-disabled=\"true\""));
assert!(html.contains("data.load_error"));
assert!(html.contains("setMemoEditorDisabled(true)"));
```

同じ tests module に ready 時にバナーが出ないテストを追加する。

```rust
#[test]
fn test_通常メモではdegradedバナーを表示しない() {
    let content = test_content();
    let toc = test_toc();
    let memo = MemoResponse::from_raw("通常メモ".to_string(), Some("README.md".to_string()));
    let syntax_css = syntax_theme_css(Some("base16-ocean.dark"));
    let html = render_page(RenderPageParams {
        title: "Test",
        content: &content,
        toc: &toc,
        memo: &memo,
        dark_mode: false,
        syntax_css: &syntax_css,
        sidebar: SidebarParams::SingleFile,
    });

    assert!(!html.contains("id=\"memo-degraded-banner\""));
    assert!(html.contains("data-state=\"saved\""));
    assert!(html.contains(">保存済み</span>"));
    assert!(!html.contains("disabled aria-disabled=\"true\""));
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run:

```bash
cargo test template::tests::test_メモ読み込み失敗時はエラー表示して編集を無効化する template::tests::test_通常メモではdegradedバナーを表示しない
```

Expected: degraded バナーが未実装で FAIL。

- [ ] **Step 3: `page.rs` に degraded バナー描画を追加する**

`src/template/page.rs` の import を次へ変更する。

```rust
use super::message::{MemoResponse, MemoState};
```

`render_memo_panel` の先頭を次の形に置き換える。

```rust
fn render_memo_panel(memo: &MemoResponse) -> String {
    let is_degraded = memo.memo_state() == MemoState::Degraded;
    let status_state = if is_degraded { "error" } else { "saved" };
    let status_text = if is_degraded { "読込失敗" } else { "保存済み" };
    let textarea_attrs = if is_degraded {
        " disabled aria-disabled=\"true\""
    } else {
        ""
    };
    let degraded_message = memo.load_error().unwrap_or(
        "メモを読み込めませんでした。内容を保護するため編集を無効化しています。本文の閲覧は継続できます。",
    );
    let degraded_banner = if is_degraded {
        format!(
            r##"      <div id="memo-degraded-banner" class="memo-degraded-banner" role="status">{message}</div>
"##,
            message = html_escape(degraded_message),
        )
    } else {
        String::new()
    };
```

同じ関数の format template で toolbar の直後、`<label class="memo-field">` の直前に `{degraded_banner}` を追加する。

```rust
        <span id="memo-save-status" class="memo-save-status" data-state="{status_state}">{status_text}</span>
      </div>
{degraded_banner}      <label class="memo-field">
```

format arguments に次を追加する。

```rust
degraded_banner = degraded_banner,
```

- [ ] **Step 4: degraded バナー CSS を追加する**

`src/template/assets/css/memo.css` の `.memo-save-status[data-state="error"]` の後に追加する。

```css
.memo-degraded-banner {
  border: 1px solid rgba(217, 119, 6, 0.45);
  border-radius: 12px;
  background: rgba(217, 119, 6, 0.12);
  color: var(--fg);
  padding: 0.75rem 0.85rem;
  font-size: 0.82rem;
  line-height: 1.55;
}
```

- [ ] **Step 5: template tests を通す**

Run:

```bash
cargo test template::tests::test_メモ読み込み失敗時はエラー表示して編集を無効化する template::tests::test_通常メモではdegradedバナーを表示しない
```

Expected: 対象 tests が PASS。

- [ ] **Step 6: コミットする**

```bash
git add src/template/page.rs src/template/assets/css/memo.css src/template/mod.rs
git commit -m "feat: メモ読込失敗をバナー表示"
```

---

### Task 3: service fallback と Refresh JSON 契約を固定する

**Files:**
- Modify: `src/server/service.rs`
- Modify: `src/server/messages.rs`

- [ ] **Step 1: service test に degraded assert を追加する**

`src/server/service.rs` の tests import に `MemoState` を追加する。

```rust
use crate::template::MemoState;
```

`test_load_page_壊れたメモは空メモへフォールバックする` と `test_load_pageとload_memoはメモread失敗時の非対称仕様を保持する` の page assert に次を追加する。

```rust
assert_eq!(page.memo.memo_state(), MemoState::Degraded);
```

`test_load_page_単一ファイル表示情報を組み立てる` には次を追加する。

```rust
assert_eq!(page.memo.memo_state(), MemoState::Ready);
```

- [ ] **Step 2: Refresh JSON test を更新する**

`src/server/messages.rs` の `test_broadcast_message_refreshのjson直列化` を次へ変更する。

```rust
#[test]
fn test_broadcast_message_refreshのjson直列化() {
    let json = BroadcastMessage::Refresh.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(
        value,
        serde_json::json!({
            "refresh": true,
            "memo_refresh": true
        })
    );
}
```

- [ ] **Step 3: テストが失敗することを確認する**

Run:

```bash
cargo test server::messages::tests::test_broadcast_message_refreshのjson直列化 server::service::tests::test_load_page_壊れたメモは空メモへフォールバックする
```

Expected: Refresh JSON が `memo_refresh` を含まず FAIL。service 側は Task 1 実装後なら PASS してよい。

- [ ] **Step 4: Refresh JSON に `memo_refresh` を追加する**

`src/server/messages.rs` の `BroadcastMessage::Refresh` arm を次へ変更する。

```rust
BroadcastMessage::Refresh => serde_json::to_string(&serde_json::json!({
    "refresh": true,
    "memo_refresh": true
})),
```

- [ ] **Step 5: 対象 tests を通す**

Run:

```bash
cargo test server::messages::tests::test_broadcast_message_refreshのjson直列化 server::service::tests::test_load_page_単一ファイル表示情報を組み立てる server::service::tests::test_load_page_壊れたメモは空メモへフォールバックする server::service::tests::test_load_pageとload_memoはメモread失敗時の非対称仕様を保持する
```

Expected: 対象 tests が PASS。

- [ ] **Step 6: コミットする**

```bash
git add src/server/service.rs src/server/messages.rs
git commit -m "feat: refreshにmemo再取得契約を追加"
```

---

### Task 4: JS の degraded 動的反映を実装する

**Files:**
- Modify: `src/template/assets/js/memo.js`
- Modify: `tests/e2e/globals.d.ts`
- Modify: `tests/e2e/memo_quote.spec.ts`

- [ ] **Step 1: E2E 型を更新する**

`tests/e2e/globals.d.ts` の `MvE2E.UpdateMessage` に次を追加する。

```ts
memo_state?: 'ready' | 'degraded';
```

- [ ] **Step 2: E2E 失敗テストを更新する**

`tests/e2e/memo_quote.spec.ts` の `メモ読み込み失敗中は引用挿入から保存しない` で、既存の status assert の後に追加する。

```ts
await expect(page.locator('#memo-degraded-banner')).toContainText('内容を保護するため編集を無効化');
```

`メモ保存応答がload_errorを含んでも編集中の内容を消さない` の stub body に `memo_state` を追加する。

```ts
memo_state: 'degraded',
```

同じテストの status assert の後に追加する。

```ts
await expect(page.locator('#memo-degraded-banner')).toContainText('編集を無効化');
```

`メモload_error後もファイル切替で編集を再開できる` の notes 応答へ `memo_state: 'degraded'`、README 応答へ `memo_state: 'ready'` を追加し、notes 選択後と README 復旧後に次を追加する。

```ts
await expect(page.locator('#memo-degraded-banner')).toContainText('編集を無効化');
```

README 復旧後:

```ts
await expect(page.locator('#memo-degraded-banner')).toHaveCount(0);
```

- [ ] **Step 3: E2E が失敗することを確認する**

Run:

```bash
npx playwright test tests/e2e/memo_quote.spec.ts --grep "メモ読み込み失敗中|メモ保存応答がload_error|メモload_error後"
```

Expected: 動的応答では `#memo-degraded-banner` が作られず FAIL。

- [ ] **Step 4: `memo.js` に degraded helper を追加する**

`src/template/assets/js/memo.js` の `setMemoSaveStatus` の後に追加する。

```javascript
function isMemoDegraded(data) {
  return !!data && (data.memo_state === 'degraded' || !!data.load_error);
}

function getMemoDegradedMessage(data) {
  return data && data.load_error
    ? data.load_error
    : 'メモを読み込めませんでした。内容を保護するため編集を無効化しています。本文の閲覧は継続できます。';
}

function setMemoDegradedBanner(message) {
  if (!appContext.elements.memoEditorEl) return;
  var layout = appContext.elements.memoEditorEl.closest('.memo-layout');
  if (!layout) return;
  var existing = document.getElementById('memo-degraded-banner');
  if (!message) {
    if (existing) existing.remove();
    return;
  }
  if (!existing) {
    existing = document.createElement('div');
    existing.id = 'memo-degraded-banner';
    existing.className = 'memo-degraded-banner';
    existing.setAttribute('role', 'status');
    var toolbar = layout.querySelector('.memo-toolbar');
    if (toolbar && toolbar.nextSibling) {
      layout.insertBefore(existing, toolbar.nextSibling);
    } else if (toolbar) {
      layout.appendChild(existing);
    } else {
      layout.insertBefore(existing, layout.firstChild);
    }
  }
  existing.textContent = message;
}
```

- [ ] **Step 5: `applyMemoData` を `memo_state` 主条件へ変更する**

`applyMemoData` の `if (data.load_error) { ... }` block を次へ置き換える。

```javascript
if (isMemoDegraded(data)) {
  // degraded 時は raw を上書きしない。読み込み失敗応答でユーザ編集中の内容を破壊しないため。
  cancelMemoAutosave();
  updateMemoPreview(data);
  setMemoEditorDisabled(true);
  setMemoDegradedBanner(getMemoDegradedMessage(data));
  setMemoSaveStatus('error', '読込失敗');
  return false;
}
setMemoDegradedBanner('');
```

同じ関数の正常終了直前は既存のままでよい。

```javascript
setMemoEditorDisabled(false);
return true;
```

- [ ] **Step 6: `loadMemo` catch でもバナーを出す**

`loadMemo` の catch 内で、`setMemoSaveStatus('error', getMemoErrorMessage(err));` の直前に追加する。

```javascript
setMemoDegradedBanner(getMemoErrorMessage(err));
```

- [ ] **Step 7: E2E 対象 tests を通す**

Run:

```bash
npx playwright test tests/e2e/memo_quote.spec.ts --grep "メモ読み込み失敗中|メモ保存応答がload_error|メモload_error後"
```

Expected: 対象 E2E が PASS。

- [ ] **Step 8: コミットする**

```bash
git add src/template/assets/js/memo.js tests/e2e/globals.d.ts tests/e2e/memo_quote.spec.ts
git commit -m "feat: メモdegraded状態を動的表示"
```

---

### Task 5: refresh payload による memo reload を E2E で固定する

**Files:**
- Modify: `tests/e2e/memo_sync.spec.ts`
- Reference: `src/template/assets/js/websocket.js`
- Reference: `src/template/assets/js/memo.js`

- [ ] **Step 1: 失敗テストを追加する**

`tests/e2e/memo_sync.spec.ts` に import されている helper は既存のまま使う。既存の `memo_refresh` 系テストの近くに次を追加する。

```ts
test('refresh payloadのmemo_refreshでメモを再取得する', async ({ page }) => {
  await page.addInitScript(installTestWebSocketHarness, { setE2EFlag: true });
  let memoGetCount = 0;

  await page.route('**/api/memo*', async (route) => {
    if (route.request().method() !== 'GET') {
      await route.continue();
      return;
    }
    memoGetCount += 1;
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        memo_state: 'ready',
        raw: memoGetCount === 1 ? 'initial memo' : 'memo after refresh',
        html: memoGetCount === 1 ? '<p>initial memo</p>' : '<p>memo after refresh</p>',
        file: 'README.md'
      })
    });
  });

  await page.goto('/');
  await stabilizeWebSocketHarness(page);
  await openMemoTab(page);
  await expect(page.locator('#memo-editor')).toHaveValue('initial memo');

  await dispatchWsMessage(page, { refresh: true, memo_refresh: true });

  await expect(page.locator('#memo-editor')).toHaveValue('memo after refresh');
  await expect.poll(() => memoGetCount).toBeGreaterThanOrEqual(2);
});
```

- [ ] **Step 2: テストが現在の JS 経路で通るか確認する**

Run:

```bash
npx playwright test tests/e2e/memo_sync.spec.ts --grep "refresh payloadのmemo_refresh"
```

Expected: PASS。既存の `isMemoRefreshMessage` が `memo_refresh: true` を見ているため、実装変更は不要な想定。

- [ ] **Step 3: WebSocket 分岐順が契約どおりであることを確認する**

`src/template/assets/js/websocket.js` の `socket.onmessage` 内で、次の順序になっていることを確認する。

```javascript
if (isMemoRefreshMessage(data)) {
  if (deps.queueRemoteMemoReload(data)) {
    hideWsServerErrorBanner();
    hideFileFetchErrorBanner();
  }
}
if (data.refresh && ctx.config.isDirMode) {
```

Expected: `isMemoRefreshMessage(data)` が `data.refresh && ctx.config.isDirMode` より前にある。コード変更は不要。

- [ ] **Step 4: 対象 E2E を通す**

Run:

```bash
npx playwright test tests/e2e/memo_sync.spec.ts --grep "refresh payloadのmemo_refresh"
```

Expected: PASS。

- [ ] **Step 5: コミットする**

```bash
git add tests/e2e/memo_sync.spec.ts src/template/assets/js/websocket.js
git commit -m "test: refreshのメモ再取得契約を固定"
```

`src/template/assets/js/websocket.js` に変更がなかった場合は、次でよい。

```bash
git add tests/e2e/memo_sync.spec.ts
git commit -m "test: refreshのメモ再取得契約を固定"
```

---

### Task 6: TODO を完了扱いへ更新し、全体検証する

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 1: TODO の対象項目を Done Summary へ移す**

`docs/todo/TODO.md` の Medium Priority から次の項目を削除する。

```markdown
- [ ] `BroadcastMessage::Refresh` の memo_refresh 契約と、メモ読込失敗時の degrade 通知を明示する
```

`## Done Summary` 直下に次を追加する。

```markdown
- [x] `BroadcastMessage::Refresh` の memo_refresh 契約と、メモ読込失敗時の degrade 通知を明示する
  - 完了根拠: `MemoResponse` に `memo_state` を追加し、通常時は `ready`、読込失敗時は `degraded` として直列化する契約にした。初期ページ描画ではメモ読込失敗を degraded response に変換し、メモパネルに専用バナーを表示して textarea と autosave を止める。ブラウザ側の `applyMemoData` も `memo_state: "degraded"` を主条件にし、読込失敗応答で編集中本文を上書きしない。`BroadcastMessage::Refresh` は `refresh: true` と `memo_refresh: true` を含む JSON へ揃え、refresh payload によるメモ再取得を E2E で固定した
```

- [ ] **Step 2: Rust tests を通す**

Run:

```bash
cargo test template::message::tests template::tests::test_メモ読み込み失敗時はエラー表示して編集を無効化する template::tests::test_通常メモではdegradedバナーを表示しない server::messages::tests::test_broadcast_message_refreshのjson直列化 server::service::tests::test_load_page_単一ファイル表示情報を組み立てる server::service::tests::test_load_page_壊れたメモは空メモへフォールバックする server::service::tests::test_load_pageとload_memoはメモread失敗時の非対称仕様を保持する
```

Expected: すべて PASS。

- [ ] **Step 3: E2E targeted tests を通す**

Run:

```bash
npx playwright test tests/e2e/memo_quote.spec.ts --grep "メモ読み込み失敗中|メモ保存応答がload_error|メモload_error後"
npx playwright test tests/e2e/memo_sync.spec.ts --grep "refresh payloadのmemo_refresh"
```

Expected: すべて PASS。

- [ ] **Step 4: 全体検証を実行する**

Run:

```bash
./verify.sh
```

Expected: format、clippy、test がすべて PASS。

- [ ] **Step 5: 差分を確認する**

Run:

```bash
git diff --stat
git diff --check
git status --short --branch
```

Expected:

- `git diff --check` が出力なし。
- 変更ファイルがこの計画の対象に収まる。

- [ ] **Step 6: コミットする**

```bash
git add docs/todo/TODO.md
git commit -m "docs: メモdegraded対応をTODO完了へ移動"
```

---

## 完了時の報告項目

- 変更ファイルと理由、概算行数。
- 影響した依存ファイル。
- 実行した Rust test、E2E test、`./verify.sh` の結果。
- セキュリティ観点: エラー詳細の UI 露出、sanitized HTML 境界、degraded 時 autosave 停止の確認。
- 残余リスク: ブラウザ別の disabled textarea 表示差、既存 `load_error` 後方互換分岐の将来削除タイミング。
