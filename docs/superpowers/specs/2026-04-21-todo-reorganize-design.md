# TODO.md 再編 — 優先度順フラット化と BACKLOG 分離

**作成日**: 2026-04-21
**対象ファイル**: `docs/todo/TODO.md`（全面書き換え）、`docs/todo/BACKLOG.md`（新規作成）

## 目的

現行 TODO.md は4つのレビュー日ブロック（2026-04-20 PR #76 / PR #77 / PR #80 / E2E TS 移行、2026-04-18 コードベース探索 PR #59）に分かれており、同じトピックの項目が散在し、Low 項目が20件以上蓄積している。結果として「次に何をやるかをすぐ判断できない」状態になっている。

この再編で以下を達成する：

1. **重複の解消** — レビュー日ブロック跨ぎの重複3件を統合
2. **冗長項目の削除** — criticality ≤ 3 または自己申告で「挙動自明」「cosmetic」としている項目5件を削除
3. **視界の整理** — High/Medium のみ TODO.md に残し、Low は BACKLOG.md に退避
4. **構造のフラット化** — レビュー日ベースの時系列構造をやめ、優先度順フラット1本化

## スコープ外

- 新規レビューの実施（既存項目の再編のみ）
- 既存項目の内容（現状/対応/理由）の書き換え — 重複統合時を除き原文維持
- 優先度の再キャリブレーション — 各項目の High/Medium/Low ラベルは現行維持

## 削除項目（5件、criticality ≤ 3 または cosmetic）

いずれも項目自身が「冗長」「自明」「cosmetic」と自己申告しており、実装される可能性が低い：

1. **`augmentHashWithTrailingLineHint` 範囲形式 hash + sibling L の precedence テスト** (現 L35-40)
   - 根拠: criticality 3、「単一行版で分岐は既にカバー済み」
2. **`augmentHashWithTrailingLineHint` `!sibling` 早期 return のユニットテスト** (現 L42-46)
   - 根拠: criticality 2、「挙動自明」
3. **`contentEl` への HTML 代入時の例外可視化** (現 L104-108)
   - 根拠: criticality 3、「本 PR 修正前から同じ挙動、本質的に既存問題」
4. **初回 broadcast 中に付与済みクラスが消失する edge case の検証** (現 L110-114)
   - 根拠: criticality 3、「実用上ユーザーが SSR 直後 100ms 以内に目次クリックする可能性は低い」
5. **`document_search.spec.ts:18` の冗長な `export {};` 削除** (現 L132-135)
   - 根拠: 優先度 Low、「cosmetic」明記

## 重複統合（3件）

PR #76 レビュー（2026-04-20）と PR #59 探索（2026-04-18）に同内容で記載されている3件を、PR #76 側の記述を正として統合：

| 項目 | PR #76 側 | PR #59 側（削除） |
|------|-----------|---------------------|
| CSP フォールバック方針整理 | 現 L7-11 | 現 L213-217 |
| エラー経路ログのパス情報を base 相対化 | 現 L13-17 | 現 L219-223 |
| `is_hidden_relative` のネスト深度削減 | 現 L21-25 | 現 L227-231 |

PR #76 側を正とする理由: 行番号指定がより新しいコードベースに追従している（例: `is_hidden_relative` が PR #76 は L162-195、PR #59 は L162-200）。

## 最終 TODO.md 構造

再編後は High 4件 + Medium 5件 = **9件フラット**、優先度順のみ。

### High Priority（4件、セキュリティ境界、全て PR #59 探索由来）

順序の根拠: セキュリティ境界のリスク順（DNS Rebinding → パストラバーサル → DoS境界 → プロトコル境界）

1. **`is_trusted_host` / `normalize_authority` の IPv6 網羅テスト追加**
   - ファイル: `src/server/guards.rs`
2. **メモ sidecar fallback 経路の超長ファイル名＋特殊文字テスト**
   - ファイル: `src/server/files/memo.rs`, `src/server/files/tests.rs`
3. **メモ API のボディ制限値を意図明文化し、境界テストを追加**
   - ファイル: `src/server/routes.rs`
4. **WebSocket close_code マッピングの統合テスト**
   - ファイル: `tests/integration_test.rs`

### Medium Priority（5件）

順序の根拠: サーバ側 infra（1-3）→ フロントエンド content.js（4-5）の自然なファイル粒度

1. **CSP フォールバック時の方針整理**（fail-fast vs 現状運用）
   - ファイル: `src/server/guards.rs`
2. **エラー経路ログのパス情報を base 相対化**
   - ファイル: `src/server/files/resolve.rs` ほか
3. **`is_hidden_relative` のネスト深度を 3 → 2 階層に削減**
   - ファイル: `src/watcher/strategy.rs`
4. **`updateContent` の inverse case (file-switch / data.content 変更時) の再描画検証**
   - ファイル: `tests/e2e/memo_jump.spec.ts`
5. **`updateContent` で `data.content === undefined` を契約違反として明示ログ**
   - ファイル: `src/template/assets/js/content.js`

## 最終 BACKLOG.md 構造

Low 全18件を優先度 Low セクションにフラット化。由来（PR番号）は各項目の末尾に注記として残す（将来「なぜ低優先なのか」の文脈回復用）。

内訳:
- PR #76 残 Low: 2件
  - `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling のユニットテスト
  - E2E テストの DOM クリーンアップ戦略見直し
- PR #77 Low: 3件
  - 猶予期間中の連続 TOC クリックでの挙動検証
  - `TOC_NAVIGATION_SLACK_PX` 境界の回帰テスト
  - L347 を active 遷移フラッシュ厳密検証に強化
- PR #80 残 Low: 1件
  - `window.updateContent` を E2E モード限定 expose に変更
- E2E TypeScript 移行 Low: 9件
  - `declare global` を `tests/e2e/globals.d.ts` に集約
  - `updateContent` 型宣言の統一
  - `as unknown as` double-cast の説明コメント追加
  - `memo_jump.spec.ts:303` の Codex review ID 削除
  - `tsconfig.json` に strict flag 追加
  - E2E 共通ヘルパーを `tests/e2e/helpers.ts` に抽出
  - `TestWebSocket` を `tests/e2e/browser/test-websocket.ts` に抽出
  - E2E を `verify.sh` に統合するか検討
  - インラインブラウザ JS の TS 化
- PR #59 探索 Low: 3件
  - `render_markdown` の責務分割（大規模）
  - `catalog.rs` のパス構築での Vec アロケーション削減
  - README のアーキテクチャ図を実装構成に揃える

合計: 18件

## 各項目のフォーマット

現行の4ブロック形式（ファイル / 現状 / 対応 / 理由）を**維持**する。実装着手時の文脈がその場で読めないと結局 git log の掘り起こしになるため、「判断用タイトル1行 + 詳細ブロック」の二段構造を維持する。

項目タイトルには既存の `- [ ]` チェックボックス形式を維持。

## 成功基準

1. TODO.md の項目数が 9件（High 4 + Medium 5）に減少
2. BACKLOG.md が新規作成され、18 件の Low 項目を含む
3. 重複3件が統合され、PR #59 側の重複記述が削除される
4. 削除候補5件が TODO.md / BACKLOG.md のいずれにも存在しない
5. 全項目のタイトル・ファイルパス・現状/対応/理由が原文から失われない（重複統合分を除く）

## 実装手順

1. `docs/todo/BACKLOG.md` を新規作成し、Low 18件を移植
2. `docs/todo/TODO.md` を全面書き換え（High 4 + Medium 5）
3. 両ファイルの内容を目視で検証（項目数、ファイルパス、説明文の完全性）
4. 1コミットにまとめて develop ブランチへ

## 影響範囲

- `docs/todo/TODO.md`: 全面書き換え（現 255 行 → 推定 100 行前後）
- `docs/todo/BACKLOG.md`: 新規作成（推定 150 行前後）
- 実装コードへの影響: なし（ドキュメントのみの変更）
- テストへの影響: なし

## リスク

- **既存の `/ex-todo` コマンド**（`docs/todo/TODO.md` から優先度順にタスクを選ぶ）が BACKLOG.md を参照するかは未確認。現行コマンドは TODO.md のみ読むため、Low 項目は `/ex-todo` の対象から外れる。これは本再編の意図通り（High/Medium を優先消化する）だが、ユーザー側の操作習慣に影響する可能性がある。
- **`/move-todo` コマンド**（完了タスクを DONE ファイルへ移動）も TODO.md を対象としているはず。BACKLOG.md の完了分が DONE に移動しない可能性があるが、Low は着手頻度が低いため当面の影響は軽微。
