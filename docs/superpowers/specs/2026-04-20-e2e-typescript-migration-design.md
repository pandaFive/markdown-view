# E2EテストのTypeScript移行 設計書

- 日付: 2026-04-20
- 対象: `tests/e2e/*.spec.js`（6ファイル、計2626行）および `playwright.config.js`
- スコープ外: `src/template/assets/js/*.js`（インラインブラウザJS）、E2EヘルパーのDRY化、E2Eの`verify.sh`統合

## 1. 目的と方針

### 目的

PlaywrightによるE2Eテスト（現状 CommonJS + 無型）を TypeScript strict に移行し、`tsc --noEmit` を `verify.sh` に組み込んで静的型検査の恩恵をCIサイクルに取り込む。

### 方針（確定事項）

| 論点 | 決定 |
|------|------|
| 対象 | E2Eテストのみ（ブラウザインラインJSは今回扱わない） |
| 厳格さ | `strict: true` ＋ `tsc --noEmit` の verify.sh 統合 ＋ ESM化 |
| 既存ヘルパーのDRY化 | 行わない（TS化という単一目的を守る、YAGNI） |
| verify.sh 統合 | `tsc --noEmit` のみ追加。`npm ci` は開発者・CI側の責務（責務分離） |
| 移行粒度 | 段階的ファイル単位（ファイル1つずつ、1コミット、各段階で `verify.sh` pass） |
| `__dirname` 相当 | `import.meta.dirname`（Node 20.11+）＋ `engines` 宣言 |

## 2. 成果物

### 新規作成

- `tsconfig.json`

### リネーム（ESM化 + 必要最小限の型注釈）

- `playwright.config.js` → `playwright.config.ts`
- `tests/e2e/memo_sync.spec.js` → `.ts`
- `tests/e2e/memo_quote.spec.js` → `.ts`
- `tests/e2e/memo_jump.spec.js` → `.ts`
- `tests/e2e/document_search.spec.js` → `.ts`
- `tests/e2e/text_selection_defer.spec.js` → `.ts`
- `tests/e2e/markdown_links.spec.js` → `.ts`

### 修正

- `package.json` — `typescript`・`@types/node` devDependencies追加、`typecheck` スクリプト追加、`engines.node >=20.11` 宣言
- `verify.sh` — `tsc --noEmit` ステップ追加、`node_modules` 不在時の明示エラー
- `CLAUDE.md` — E2Eを`.ts`記載、`npm run typecheck` 追記

### 触らない

- `src/template/assets/js/*.js`（スコープ外）
- E2E共通ヘルパーの重複（`resetFixtures` 等）— 別タスクに分離
- `.github/workflows/*`（E2Eは元々CI未統合、現状維持）

## 3. tsconfig.json 設計

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noEmit": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "resolveJsonModule": true,
    "types": ["node"]
  },
  "include": ["tests/e2e/**/*.ts", "playwright.config.ts"]
}
```

### 各設定の根拠

| 設定 | 値 | 理由 |
|------|-----|------|
| `target` | `ES2022` | Node 18+前提（Playwrightの最低要件と整合） |
| `module` / `moduleResolution` | `NodeNext` | Node.js本体のESM解決と同一ルール |
| `strict` | `true` | B案の核。`noImplicitAny`、`strictNullChecks` 等を一括有効 |
| `noEmit` | `true` | Playwrightがランタイムで`.ts`を処理するためトランスパイル出力不要 |
| `esModuleInterop` | `true` | `import fs from 'node:fs/promises'` を自然に書ける |
| `skipLibCheck` | `true` | `node_modules`内の`.d.ts`再検査を省略 |
| `resolveJsonModule` | `true` | 将来的にJSON fixtureを読む余地 |
| `types` | `["node"]` | Node標準API型を明示有効化 |

### あえて入れない

- `noUncheckedIndexedAccess` — strictより厳しい。既存の配列アクセス修正コストに見合わない
- `outDir` / `rootDir` — `noEmit: true` で不要
- `paths` / `baseUrl` — 相対パスで足りる

## 4. package.json 差分

```diff
 {
   "private": true,
+  "engines": {
+    "node": ">=20.11"
+  },
   "scripts": {
-    "test:e2e": "playwright test"
+    "test:e2e": "playwright test",
+    "typecheck": "tsc --noEmit"
   },
   "devDependencies": {
-    "@playwright/test": "^1.58.2"
+    "@playwright/test": "^1.58.2",
+    "@types/node": "^22.0.0",
+    "typescript": "^5.6.0"
   }
 }
```

## 5. verify.sh 差分

関数を追加（スクリプト上部）:

```bash
typecheck_e2e() {
  if [[ ! -d node_modules ]]; then
    echo "エラー: node_modules が存在しません。'npm ci' を先に実行してください。" >&2
    return 1
  fi
  npx --no-install tsc --noEmit
}
```

末尾の実行列に追加:

```diff
 run_step "フォーマットチェック" cargo fmt --all -- --check
 run_step "Lint (clippy)" cargo clippy --all-targets --all-features -- -D warnings
 run_step "テスト実行" cargo test --all-targets --all-features
+run_step "E2E型チェック (tsc)" typecheck_e2e

 echo "==> 検証が正常に完了しました。"
```

### 設計判断

| 項目 | 決定 | 理由 |
|------|------|------|
| ステップ順序 | Rust検証の**末尾** | プロジェクトの重心はRust。TSはperipheral |
| `npx --no-install` | 使う | `node_modules`不在時の暗黙インストール禁止（責務分離） |
| 関数化 | する | `run_step`が`"$@"`実行のため、条件分岐は関数に集約 |
| 不在時の挙動 | `return 1` | `trap ERR`が拾って「失敗: E2E型チェック (tsc)」を表示 |

## 6. コード変換パターン

### ESM import への置換

```diff
-const fs = require('node:fs/promises');
-const path = require('node:path');
-const { test, expect } = require('@playwright/test');
+import fs from 'node:fs/promises';
+import path from 'node:path';
+import { test, expect, type Page } from '@playwright/test';
```

### 型注釈（最小限）

```diff
-async function selectParagraphText(page, text) {
+async function selectParagraphText(page: Page, text: string) {
```

### `__dirname` 相当

```diff
-const fixtureDir = path.join(__dirname, '..', 'fixtures', 'e2e');
+const fixtureDir = path.join(import.meta.dirname, '..', 'fixtures', 'e2e');
```

### `page.evaluate` コールバック内

ブラウザrealmで実行されるためNode型使用不可。引数型は`document`/`window`由来の型のみ使用し、`document.getElementById()` の `HTMLElement | null` は非null断言 `!` か早期throw。

## 7. コミット分割計画

ファイルサイズの小→大順で、型注釈パターンを小さいファイルで確立してから大物に適用する。

| # | 種別 | 内容 | 目安規模 |
|---|------|------|----------|
| 1 | `chore` | TS基盤整備: `tsconfig.json`新規、`package.json`更新、`verify.sh`更新、`playwright.config.ts`へリネーム + ESM化、`npm install`で lockfile 更新 | 最大 |
| 2 | `refactor` | `memo_quote.spec.ts`移行 | 74行 |
| 3 | `refactor` | `memo_sync.spec.ts`移行 | 185行 |
| 4 | `refactor` | `markdown_links.spec.ts`移行 | 378行 |
| 5 | `refactor` | `memo_jump.spec.ts`移行 | 474行 |
| 6 | `refactor` | `text_selection_defer.spec.ts`移行 | 624行 |
| 7 | `refactor` | `document_search.spec.ts`移行 | 891行 |
| 8 | `docs` | `CLAUDE.md`更新（E2Eは`.ts`、`npm run typecheck`追記） | 小 |

### 各コミットの受け入れ条件（DoD）

1. `./verify.sh` が全ステップpass（tscステップ含む）
2. `npm run test:e2e` が全testpass（`cargo run`でサーバーが立つ環境）
3. ESLint/Prettierは導入しない（既存になし）

## 8. リスクと対策

| リスク | 影響 | 対策 |
|--------|------|------|
| strict で既存の潜在null漏れが露出 | 中 | その場で修正。バグ発見＝価値 |
| Playwright型と実runtime差異 | 低 | `skipLibCheck`、挙動は`test:e2e`で担保 |
| CommonJS→ESM移行で`__dirname`不在 | 低 | `import.meta.dirname` に置換、Node 20.11+ を`engines`で宣言 |
| 案2を途中で中断した場合の混在 | 低 | `include`は`**/*.ts`のみ、`.js`残存は素通り。`verify.sh`は通り続ける |

## 9. ロールバック戦略

- 各コミットが独立してpassするので任意の段階で `git revert <sha>` 可能
- PR単位は1本（`feat/e2e-typescript-migration` 想定）
- developへ squash merge

## 10. スコープ外の残課題（別タスク候補）

- E2EヘルパーのDRY化（`tests/e2e/helpers.ts` 抽出、あるいは Playwright Fixture への移行）
- E2Eの`verify.sh`統合（`npm run test:e2e` を検証シーケンスに入れるか）
- インラインブラウザJS（`src/template/assets/js/*.js`）のTS化（ビルドパイプライン追加が必要）
