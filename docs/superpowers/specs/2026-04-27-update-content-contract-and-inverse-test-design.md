---
title: updateContent 契約違反 warn 追加と cache invariant 逆方向 E2E テスト設計
date: 2026-04-27
status: draft
related:
  - docs/todo/TODO.md
  - src/template/assets/js/content.js
  - src/template/message.rs
  - tests/e2e/memo_jump.spec.ts
---

# updateContent 契約違反 warn 追加と cache invariant 逆方向 E2E テスト設計

## 1. 概要

`docs/todo/TODO.md` の Medium Priority 未完了項目のうち以下 2 件を 1 PR で配信する。

1. `updateContent` で `data.content === undefined` を契約違反として明示ログ
2. `updateContent` の inverse case (file-switch / data.content 変更時) の再描画検証

TODO #3 (`build_lagged_recovery_message` の IO エラー統合テスト) は本設計の **非ゴール**。「再現性が低いため将来の宿題」として TODO.md に残す。

## 2. 背景と動機

### 2.1 silent failure の構造

`UpdateMessage` (`src/template/message.rs`) の `content` / `toc` フィールドには `skip_serializing_if` が付いていないため、サーバー契約として両 field は常に serialize される。しかし現状の `updateContent` (`src/template/assets/js/content.js:1242-1248`) は `data.content === undefined` の場合 no-op で黙殺する（`!== undefined` 判定に `false` で落ちて DOM 反映ブロックをスキップ）。

サーバー契約変更や中継プロキシ改変で content / toc が欠落すると「ファイル編集してもプレビュー更新されない」というサイレント失敗になる。WebSocket フレームを直接覗かない限りデバッグ不能。

### 2.2 cache invariant 逆方向の未検証

既存 E2E テスト「同じdata.contentでの2回目updateContentは.jump-highlightを消さない」(`tests/e2e/memo_jump.spec.ts:437-482`) は cache 一致時の no-op を検証する。一方で **逆方向** (cache 不一致 → 必ず再描画) を直接検証するテストが存在しない。cache 比較条件が逆転する書き換え (例: `data.content === lastAppliedContent`) や常に false 化する誤りを既存 suite では検出できない。

## 3. スコープ

### In scope

- `src/template/assets/js/content.js` への契約違反 warn ブロック追加（content と toc の両方を対称的にチェック）
- `tests/e2e/memo_jump.spec.ts` への新規テスト 2 本追加
- `tests/e2e/memo_jump.spec.ts:7` の `window.updateContent` 型宣言を `toc?: string`（optional）→ `toc: string`（required）へ修正（契約と一致させる）
- `docs/todo/TODO.md` の該当 2 件を完了マーク

### Out of scope

- TODO #3 (`build_lagged_recovery_message` の統合テスト)
- `data.content === null` のハンドリング
- `UpdateMessage` の Rust 側契約変更（`skip_serializing_if` 追加など）
- prime ヘルパー抽出
- 既存テスト「同じdata.contentでの2回目updateContent...」の改修
- `console.warn` 以外のテレメトリ送出（サーバー側 POST 等）

## 4. 設計

### 4.1 #2 契約違反 warn 実装

**変更箇所**: `src/template/assets/js/content.js` の `updateContent` 関数冒頭、`options = options || {};` の直後（現状の L1228 周辺）。

**追加するロジック（擬似コード）**:

1. 空配列 `missing` を用意
2. `data.content === undefined` なら `missing.push('content')`
3. `data.toc === undefined` なら `missing.push('toc')`
4. `missing.length > 0` なら `console.warn('[markdown-view] updateContent: ' + missing.join(', ') + ' が欠落 (契約違反)', data)` を発火
5. その後の処理（既存の `pendingUpdateTimer` クリア以降）はそのまま継続

**コメント**: `UpdateMessage` (src/template/message.rs) の契約が根拠であることを JSDoc 風コメントで明示。`skip_serializing_if` が付いていない事実とそれが warn の根拠になっていることを記す。

**設計判断**:
- **副作用は維持**: warn を出した上で TOC 更新等の既存副作用は継続する。早期 return しない。`data.content` だけ欠落で `data.toc` だけ届く部分回復ケースを許容するため。
- **field 別判定 → 1 件 warn**: 複数欠落時に warn が連続発火すると DevTools が騒がしくなる。デバッグ時は欠落 field を一目で識別したい。
- **`null` は対象外**: `=== undefined` のみチェック。`null` は既存コードの `!== undefined` 判定で素通り（既存挙動）になり、ここで扱うとスコープが膨らむ。

### 4.2 #1 inverse case E2E テスト

**追加先**: `tests/e2e/memo_jump.spec.ts` の末尾。既存テストには touch しない。

**新テスト名**: `data.contentが変わるとupdateContentは再描画される (cache invariantの逆方向)`

**テストの 4 ステップ**:

| Step | 操作 | 検証 |
|---|---|---|
| 1 | `fetch('/api/content?file=long.md')` で取得した content/toc で prime → `window.updateContent(data, {})` を呼ぶ | `data.content` が string で長さ > 0 |
| 2 | `#content h2` の最初の要素に CSS class `jump-highlight` を付与（再描画 sentinel） | （操作のみ） |
| 3 | ダミー HTML `<h1 data-test-changed>changed content</h1>` を `content` に、`<ul></ul>` を `toc` に渡して `updateContent` 呼び出し | (a) `.jump-highlight` が消えている (= 再描画された) (b) `[data-test-changed]` 要素が存在する (= ダミー HTML が反映された) |
| 4 | 再度 `[data-test-changed]` 要素に `jump-highlight` を付与 → 同一ダミー HTML で再呼び出し | `.jump-highlight` が残存している (= cache 更新後の no-op が成立、`lastAppliedContent` が新値で更新済み) |

**設計判断**:
- **prime は実 fetch を維持**: Step 1 を `/api/content?file=long.md` 経由にすることで通常運用 → ダミー HTML 注入の自然な遷移を表現。`lastAppliedContent` への初期投入も既存テストと同形。
- **ダミー HTML は `data-test-changed` 属性で識別**: 再描画後 DOM の検証用に明示的なマーカーを置く。`<h1>changed</h1>` のみだと long.md 内見出しと衝突する可能性があるため安全側に倒す。
- **toc は `<ul></ul>`**: 4.1 の契約違反検査をパスするため、空ではなく形式上有効な値を渡す。warn 混入によるテストノイズを防ぐ。
- **sentinel 二重利用**: Step 2 と Step 4 の両方で `.jump-highlight` を使い、cache 状態の変化を「sentinel が消えるか残るか」の boolean 比較で検証。
- **prime ブロックはインライン**: 既存テストの Step 1/Step 3 と同形でエラーメッセージ prefix を `'prime fetch failed'` 等に分けて識別性を維持。

### 4.3 #2 警告ロジックの捕捉テスト

**追加先**: `tests/e2e/memo_jump.spec.ts` の inverse case テスト直後。

**新テスト名**: `updateContentはdata.content/toc欠落時に契約違反warnを出す`

**テスト構造**:

1. `page.on('console')` で `'warning'` タイプの message text を配列 `warnings` に蓄積
2. 4 ケースを順次 `page.evaluate` で実行：
   - **ケース 1** `{ content: '<p>ok</p>', toc: '<ul></ul>' }` → warn 出ない期待
   - **ケース 2** `{ toc: '<ul></ul>' }`（`as any` キャスト）→ `'content が欠落'` warn
   - **ケース 3** `{ content: '<p>ok</p>' }`（`as any`）→ `'toc が欠落'` warn
   - **ケース 4** `{}`（`as any`）→ `'content, toc が欠落'` warn
3. `await page.waitForTimeout(50)` で console event の非同期 flush を待つ
4. `warnings` を `'updateContent'` AND `'契約違反'` を含むものに filter → `contractWarnings`
5. assert:
   - `contractWarnings.length === 3`（ケース 1 は出ない）
   - `contractWarnings[0]` は `'content が欠落'` を含み `'toc'` を含まない
   - `contractWarnings[1]` は `'toc が欠落'` を含み `'content,'` prefix を含まない（ケース 4 との誤検知ガード）
   - `contractWarnings[2]` は `'content, toc が欠落'` を含む

**設計判断**:
- **`page.on('console')` 方式**: Playwright 標準。MCP には依存しない。
- **4 ケース網羅**: 正常 / content のみ / toc のみ / 両方。warn 出力形式 3 パターン (`content`, `toc`, `content, toc`) を直接検証。
- **`waitForTimeout(50)` で flush**: `console` イベントは page → test runner へ非同期配信される。assertion 前に buffer 到達を待つ。
- **`as any` キャスト**: TypeScript strict 設定下で意図的に型違反呼び出しを行う必要がある（実運用で起こりうる契約違反を再現）。
- **filter で warn を絞り込む**: 他箇所の warn 混入を排除。
- **prefix 誤検知ガード**: ケース 2 が `'content,'` を含まないことを `not.toContain('content,')` で確認。ケース 4 との混同を防ぐ。

## 5. ファイル変更一覧

| ファイル | 変更種別 | 内容 |
|---|---|---|
| `src/template/assets/js/content.js` | edit | `updateContent` 冒頭に契約違反 warn ブロックを追加（4.1） |
| `tests/e2e/memo_jump.spec.ts` | edit | (a) L7 の `window.updateContent` 型宣言を `toc?: string` → `toc: string` に修正 (b) 新規テスト 2 本を末尾に追加（4.2, 4.3） |
| `docs/todo/TODO.md` | edit | 該当 2 件を `[x]` に更新 |

## 6. コミット計画

squash merge 前提だが、レビュー時の差分整理のため小コミットで配信する。

| # | type | コミットメッセージ | 含む変更 |
|---|---|---|---|
| 1 | feat | `feat: updateContentにdata.content/toc欠落時の契約違反warnを追加` | content.js の warn ブロック + memo_jump.spec.ts L7 の型宣言修正（`toc: string` 必須化） |
| 2 | test | `test: updateContent契約違反warnのE2Eテストを追加` | warn 捕捉テスト（4.3） |
| 3 | test | `test: updateContentのcache invariant逆方向 (再描画) E2Eテストを追加` | inverse case テスト（4.2） |
| 4 | chore | `chore: TODO.mdの完了項目を更新` | TODO.md チェック |

**コミット順の根拠**: 1 → 2 で「実装と直接の検証」を組にし、各 commit が単独で意味を持つ。型宣言修正は warn の契約強化と同じ意図なので 1 にまとめる。3 は別目的（cache 不変条件の逆方向）なので独立。4 は最後。

## 7. エッジケースとリスク

### 確認済み（既存挙動を維持）

- `data.content === null`: 既存動作維持。今回スコープ外
- `data` 自体が `undefined` / `null`: 既存コードでアクセス時 TypeError 発生。新規 warn 検知ブロックも同じアクセスパターンなので既存と同じ振る舞い
- `data.content === ''`: 既存動作維持

### 設計で防御済み

- inverse case テストの dummy が long.md fixture と衝突: `data-test-changed` 属性で識別ガード
- warn 捕捉テストでケース 1 の prime が後続テストへ漏出: Playwright のテスト分離（独立 page context）で防御済み

### 残リスク

- **`waitForTimeout(50)` の flake リスク**: CI で 1-2 回の flake が観測されたら 100ms に増やす。それでも flake 化する場合は `page.on('console')` を Promise 化して `Promise.race` で同期化する設計に切り替える（boilerplate 増を許容）。設計時点では fail-fast 対応として留める。

## 8. 検証

```bash
./verify.sh   # fmt + clippy + cargo test + npm run typecheck
npm run test:e2e -- --grep "updateContent"
```

すべて pass することを完了条件とする。

## 9. 残 TODO

- TODO #3 (`build_lagged_recovery_message` の統合テスト) は引き続き「将来の宿題」として TODO.md に残す
- 本 PR の `waitForTimeout(50)` が flake 化した場合の追従
