# E2E High Priority テスト失敗修正 設計

- 作成日: 2026-04-20
- 対象: `docs/todo/TODO.md` の「E2E テスト失敗（2026-04-20 発見）」High Priority 2件
- 方針: 各失敗の根本原因に合わせ 1件はプロダクトコード、1件はテスト期待値を修正

## 背景

`npx playwright test` 全体実行で 5件失敗のうち 3件が develop baseline で再現。その中で High Priority に分類された 2件を対象とする。

- `tests/e2e/markdown_links.spec.js:124` — 壊れたフラグメントリンクの hash クリア + 警告
- `tests/e2e/document_search.spec.js:389` — ディレクトリモード検索 API 結果の一覧表示

両テストとも機能自体は動作しており、失敗しているのは特定の assertion のみ。

## Bug #1: 壊れたフラグメントリンクの警告文言不一致

### 失敗内容

テスト `markdown_links.spec.js:124` は同一ファイル内の壊れたアンカー (`README.md#missing`) をクリックしたとき、以下をすべて検証する。

1. URL の hash がクリアされる
2. `scrollY === 0`
3. `#toc a.active` が消える
4. `console.warn` に `'見出しが見つかりません'` を含むメッセージが出る

このうち 1〜3 は passing。4 のみ fail している。

### 原因

`src/template/assets/js/content.js:402` の warning 文言のみ `'同一ファイル内のジャンプ先が見つかりません:'` で、他2箇所と用語が不一致。

| 位置 | 文言 | 用語 |
|------|------|------|
| L287 | `'履歴復元時に見出しが見つかりません:'` | 見出し |
| L402 | `'同一ファイル内のジャンプ先が見つかりません:'` | ジャンプ先 |
| L1264 | `'リンク先の見出しが見つかりません:'` | 見出し |

L402 の検索対象は `applyContentAnchorNavigation` 経由の `parsed.headingId`（`document.getElementById` で heading ID を引く）ため、用語としても「見出し」が正確。

### 修正

`src/template/assets/js/content.js:402` を次のように変更する。

```js
// before
console.warn('[markdown-view] 同一ファイル内のジャンプ先が見つかりません:', target.hash);
// after
console.warn('[markdown-view] 同一ファイル内の見出しが見つかりません:', target.hash);
```

## Bug #2: ディレクトリモード検索サマリーの期待値ズレ

### 失敗内容

テスト `document_search.spec.js:389` は `/api/search` を mock で 2件返したとき、サマリーが `'0 / 2 件'` になることを期待している。実際には `'1 / 2 件'`（先頭要素が auto-selected）となる。

### 原因

PR #51（`feat: ディレクトリモードの全文検索を追加`）で auto-selection 機構が導入された。

- `runDirectorySearch` 呼び出し前に `getPreferredDirectorySearchSelection` で preferred selection を確定
- preferred selection は以下の優先順位で決まる:
  1. `pendingDirectorySearchNavigation`（ナビゲーション進行中）
  2. 既存の `currentDirectorySearchIndex`
  3. `currentFile` + `currentDocumentSearchIndex`（本文側のヒットインデックス）
- API 応答到着時、`resolveDirectorySearchIndex` が preferred selection を results 内で探索して index を確定

本テストは以下の条件で auto-selection 3 が発動する。

- テスト手動で `isDirMode = true` 設定
- `updateContent` で本文に `'Alpha note appears here.'` を流す
- `syncDocumentSearchAfterContentUpdate` が走り `currentDocumentSearchIndex = 0` にセット
- `setDocumentSearchQuery(page, 'note')` で directory search 起動
- preferred selection `{file: currentFile, fileMatchIndex: 0}` が results 先頭 (`README.md`, `file_match_index=0`) と一致
- `currentDirectorySearchIndex = 0` → summary `'1 / 2 件'`

### 修正方針

auto-selection は仕様として維持する。根拠:

- 兄弟テスト `document_search.spec.js:437` 「ディレクトリモードでは現在ファイルの本文ヒットを検索結果選択に反映する」が auto-selection を明示的に保証
- ユーザ体験として「今開いているファイル内の該当箇所を優先選択」は自然
- ロジックを変更すると L437 を破壊し、仕様変更の波及が大きい

テスト L389 の意図は「API 結果が 2件一覧表示される」こと。auto-selection の有無はこのテストの主要関心事ではない。

### 修正

`tests/e2e/document_search.spec.js:429` を次のように変更する。

```js
// before
await expect(page.locator('#document-search-summary')).toHaveText('0 / 2 件');
// after
await expect(page.locator('#document-search-summary')).toHaveText('1 / 2 件');
```

他の assertion（件数 2件、先頭が README.md、2件目が notes.md）は変更しない。

## 検証

1. `npx playwright test tests/e2e/markdown_links.spec.js:124` → pass
2. `npx playwright test tests/e2e/document_search.spec.js:389` → pass
3. `npx playwright test tests/e2e/markdown_links.spec.js tests/e2e/document_search.spec.js` → 周辺テスト回帰なし
4. `./verify.sh` → Rust 側影響なしを確認

## 非対象

TODO.md の以下項目は本設計のスコープ外。

- Medium: Flaky E2E 2件の安定化（`markdown_links.spec.js:41`、`memo_sync.spec.js:48`）
- Low: PR #76 追加ユニットテスト 3件
- Low: PR #77 TOC ナビゲーション回帰テスト 3件
- Low: E2E DOM クリーンアップ戦略見直し
