# Codebase Audit: バグ修正タスク提案（2026-04-25）

## 概要

`README.md` / `CLAUDE.md` / 既存 TODO 群と、`cargo test --all-targets --all-features` の実行結果をもとに、
「今すぐ修正効果が高い順」でタスクを洗い出した。

- 対象領域: メモ保存経路、ファイル監視、WebSocket/HTTP エラー境界、E2E 保守性、ドキュメント整合
- 方針: 既存 TODO の未完了項目 + 今回のテスト実行で観測した不安定ポイントを統合

---

## 優先度 High（先に着手）

- [ ] **memo 保存の permission/fallback 契約を再定義し、実装・テストを一致させる**
  - 背景: `save_route_memo` 系ユニットで 6 件失敗し、single-file / legacy fallback / safe legacy cleanup の期待が食い違っている。
  - 対象: `src/server/files/memo.rs`, `src/server/files/tests.rs`
  - 完了条件:
    - fallback ルールを仕様としてコメント化（single-file と directory の差分含む）
    - permission エラー時の挙動を 1 つに決め、全関連テストをその仕様へ統一
    - root 実行時でも再現可能な失敗誘発手法（`chmod` 依存を減らす）へ更新

- [ ] **`test_save_route_memo_*` の環境依存を排除（root 実行でも安定させる）**
  - 背景: 現在の `chmod 0o555` などは root では期待通り失敗せず、CI/ローカル差異を生みやすい。
  - 対象: `src/server/files/tests.rs`
  - 完了条件:
    - 書込失敗注入を filesystem permission ではなく、テストダブル/依存注入で制御
    - Linux/macOS/CI コンテナで同一結果を確認

- [ ] **`/api/content` の 500 IO エラー統合テスト追加**
  - 背景: WebSocket 側は close_code マッピングが厚くなったが、HTTP 側の IO=500 の境界が薄い。
  - 対象: `tests/integration_test.rs`
  - 完了条件:
    - IO failure を起こし、status 500 + error JSON を検証
    - TooLarge/NotUtf8 との取り違えがないことを確認

- [ ] **`build_change_broadcast_message` の実 WebSocket 経路テスト追加**
  - 背景: 初期化経路の close code は強化済みだが、更新通知経路は回帰検知が弱い。
  - 対象: `tests/integration_test.rs`, 必要なら `src/server/broadcast.rs`
  - 完了条件:
    - ファイル変更イベントから Error broadcast までを E2E で検証

---

## 優先度 Medium（次点）

- [ ] **`is_hidden_relative` のネスト解消とログ重複削減**
  - 対象: `src/watcher/strategy.rs`
  - 完了条件: 相対パス計算ヘルパー抽出、canonicalize 失敗ログの重複排除

- [ ] **`updateContent` inverse case の回帰テスト追加**
  - 対象: `tests/e2e/memo_jump.spec.ts`
  - 完了条件: 同一 content no-op だけでなく、変更時再描画→再度 no-op の 2 段検証

- [ ] **`updateContent` で `data.content === undefined` を契約違反ログ化**
  - 対象: `src/template/assets/js/content.js`
  - 完了条件: silent no-op を `console.warn` で可視化し、TOC 更新副作用は維持

- [ ] **E2E の `declare global` 散在を `globals.d.ts` へ集約**
  - 対象: `tests/e2e/*.spec.ts`, `tsconfig.json`
  - 完了条件: Window 拡張型の重複定義を撤去、TS2717 由来の将来リスクを解消

- [ ] **`updateContent` 型宣言の統一（spec ごとの不整合を除去）**
  - 対象: `tests/e2e/memo_jump.spec.ts`, `tests/e2e/document_search.spec.ts`
  - 完了条件: `opts` 必須/任意の差異と payload 形を単一定義へ統合

- [ ] **`window.updateContent` の本番露出を E2E 限定に切替**
  - 対象: `src/template/assets/js/content.js`, `tests/e2e/*`
  - 完了条件: `window.__MV_E2E__` フラグ時のみ expose

---

## 優先度 Low（時間があるとき）

- [ ] **E2E 共通ヘルパー抽出 (`helpers.ts`)**
- [ ] **`TestWebSocket` 定義の共有化**
- [ ] **`tsconfig.json` strict 強化（`noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`）**
- [ ] **`render_markdown` 大関数の段階的分割（挙動等価を維持）**
- [ ] **`catalog.rs` の micro-alloc 最適化（計測前提）**
- [ ] **README アーキテクチャ図の実装追従更新**

---

## 追加で提案する「実装前タスク」

- [ ] **memo 保存仕様の設計ノート作成（2〜3 ページ）**
  - fallback 優先順位、single/directory 差分、legacy cleanup 失敗時の扱いを表形式で固定
- [ ] **テスト失敗分類ラベル運用**
  - `logic regression` / `environment-dependent` / `test assumption drift` を導入
- [ ] **監視系テストの共通ユーティリティ整備**
  - 一時ディレクトリ作成、権限変更、クリーンアップを 1 箇所へ集約

---

## 実行順（推奨）

1. memo 保存仕様の確定（High-1）
2. 環境依存テストの除去（High-2）
3. HTTP/WS 境界テストの追加（High-3,4）
4. watcher と updateContent の保守性改善（Medium）
5. E2E 型/共通化の債務返済（Medium〜Low）


---

## 運用方針: GitHub Issue と Markdown 管理どちらが良いか

結論としては **ハイブリッド運用** が最適。

- **GitHub Issue が向くもの**
  - 実装着手する単位（担当者・期限・レビュー対象がある）
  - 議論ログを時系列で残したいもの
  - PR と自動連携してクローズ管理したいもの
- **リポジトリ内 Markdown が向くもの**
  - 全体バックログの俯瞰（優先度・実行順の一覧）
  - 設計背景や文脈を含む長文ノート
  - オフライン/ローカルでも参照したい運用ドキュメント

### 推奨ルール

1. このファイルは「親台帳（index）」として維持する。
2. `High` の各項目は着手前に 1 Issue 化する。
3. Issue 作成時にこのファイルへ Issue 番号を追記（相互リンク）。
4. クローズ済み Issue は `docs/done/` に成果を要約して棚卸しする。

この方式だと、Markdown の一覧性と Issue の実行管理を両立できる。
