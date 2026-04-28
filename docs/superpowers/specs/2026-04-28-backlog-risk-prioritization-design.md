# BACKLOG.md リスク順グループ化設計

**作成日**: 2026-04-28
**対象ファイル**: `docs/todo/BACKLOG.md`

## 目的

`docs/todo/BACKLOG.md` を、Low Priority の単なる保管場所から「リスク低減順に次の候補を拾えるリスト」へ整理する。

High / Medium は `docs/todo/TODO.md` で管理済みのため、今回の対象は `BACKLOG.md` 内の未完了 Low 項目に限定する。未完了項目は `P1 / P2 / P3` に分類し、完了済み項目は実行候補に混ざらないよう末尾の `Done` セクションへ分離する。

## 非ゴール

- 各バックログ項目を実装しない。
- Low 項目を High / Medium へ昇格しない。
- 項目本文を大幅に書き換えない。
- 古い項目を削除しない。
- 完了判定や削除判定の監査を同時に行わない。

## 分類ルール

### P1: リスク低減・検証基盤

壊れたときにセキュリティ境界、回帰検知、CI の信頼性へ直接効くものを置く。

想定項目:

- E2E の `declare global` ブロックを `tests/e2e/globals.d.ts` に集約
- `updateContent` 型宣言の統一
- `tsconfig.json` への strict flag 追加
- `TestWebSocket` の共通ブラウザハーネス化
- E2E を `verify.sh` に統合するかの検討
- E2E テストの DOM クリーンアップ戦略見直し

### P2: 保守性・局所回帰検知

保守性と局所的な回帰検知を強めるものを置く。P1 より影響範囲は狭いが、将来の変更で壊れやすい仕様を固定する項目を優先する。

想定項目:

- `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling のユニットテスト
- 猶予期間中の連続 TOC クリックでの挙動検証
- `TOC_NAVIGATION_SLACK_PX` 境界の回帰テスト
- active 遷移フラッシュ厳密検証
- E2E 共通ヘルパー抽出
- `as unknown as` double-cast の説明コメント追加
- rot-prone な Codex review ID コメント削除

### P3: 長期改善・低緊急

長期改善、大規模 refactor、費用対効果が薄い最適化、ドキュメント整合を置く。着手前に個別の設計や再評価が必要な項目を含む。

想定項目:

- インラインブラウザ JS の TypeScript 化
- `render_markdown` の責務分割
- `catalog.rs` のパス構築での Vec アロケーション削減
- README のアーキテクチャ図更新

### Done

完了済み `[x]` 項目を置く。本文は残し、実行候補からは外す。

現時点の対象:

- `window.updateContent` を E2E モード限定 expose に変更

## 文書構造

`BACKLOG.md` は以下の構造にする。

```markdown
# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium が TODO.md から捌けてから着手する候補。
未完了項目はリスク低減効果を基準に P1/P2/P3 へ分類する。

## P1: リスク低減・検証基盤

## P2: 保守性・局所回帰検知

## P3: 長期改善・低緊急

## Done
```

各項目の本文は原則そのまま維持する。新しいメタ情報は追加せず、差分を「並べ替え、見出し変更、完了済み分離」に閉じる。

## 受け入れ基準

- 未完了 `[ ]` 項目が `P1 / P2 / P3` のいずれかに分類されている。
- 完了済み `[x]` 項目が `Done` に分離されている。
- P1 から順にリスク低減効果が高い構造になっている。
- 既存項目の `ファイル`、`内容`、`理由`、`由来` が失われていない。
- `TODO.md`、実装コード、テストコードには変更がない。

## 検証

docs 変更のため TDD ではなく、構造検証で確認する。

```bash
grep -c "^- \[ \]" docs/todo/BACKLOG.md
grep -c "^- \[x\]" docs/todo/BACKLOG.md
grep -c "^  - 由来:" docs/todo/BACKLOG.md
```

加えて、次を目視確認する。

- `window.updateContent` の完了済み項目が `Done` にだけ存在する。
- 未完了項目が `P1 / P2 / P3` のいずれかに入っている。
- 項目本文が意図せず要約・削除されていない。

## セキュリティ考慮

`BACKLOG.md` の項目には外部レビューや過去の探索結果を由来とする記述が含まれる。これらは作業候補として扱い、実装済み事実や現在の脆弱性としては扱わない。

今回の整理は優先順位の見直しのみであり、セキュリティ境界を強化したことにはならない。P1 に置いた項目でも、着手時には現在のコード、テスト、実行結果で改めて検証する。

## 影響範囲

- 変更対象: `docs/todo/BACKLOG.md`
- 参照対象: `docs/todo/TODO.md`、関連する既存設計書
- 実装コードへの影響: なし
- テストコードへの影響: なし

## ロールバック

docs-only 変更なので、整理コミットを revert すれば元の並びに戻せる。
