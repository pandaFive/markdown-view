# BACKLOG 次タスク選定設計

**作成日**: 2026-04-28
**対象ファイル**: `docs/todo/BACKLOG.md`

## 目的

`docs/todo/BACKLOG.md` の P1 項目から、次に着手する backlog 項目を選定する。

次タスクは `E2E テストの DOM クリーンアップ戦略見直し` とする。`verify.sh` に E2E を統合する前に、既知の test pollution リスクを局所的に下げ、E2E テスト自体の信頼性を上げる。

## 非ゴール

- この設計では実装しない。
- `verify.sh` への E2E 統合は同時に行わない。
- E2E 共通ヘルパーを `tests/e2e/helpers.ts` に抽出しない。
- `augmentHashWithTrailingLineHint` の新しい仕様テストは追加しない。
- プロダクションコード、Rust 側、Playwright 設定は変更しない。

## 選定理由

P1 には `E2E テストの DOM クリーンアップ戦略見直し` と `E2E を verify.sh に統合するか検討` が残っている。

`verify.sh` 統合は見落としを減らす効果が大きい。一方で、現状の Playwright E2E は `cargo run` の webServer、fixture の読み書き、`workers: 1` の直列実行、`node_modules` とブラウザ install 前提に依存する。`verify.sh` に入れる前に、実行対象の E2E テストに残る既知の cleanup 脆弱性を先に減らす方が、リスク低減と実装コストのバランスがよい。

`tests/e2e/memo_jump.spec.ts` では、`augmentHashWithTrailingLineHint` 系テストが DOM にリンクと隣接テキストノードを追加し、`finally` で `container.lastChild` が `TEXT_NODE` なら削除している。この cleanup は、並行する DOM 変更や将来のテスト修正で別ノードを末尾に追加した場合、意図しないノードを削除する余地がある。対象範囲は同 spec 内の 7 箇所に限られるため、次タスクとして扱いやすい。

## 方針

次タスクでは、`tests/e2e/memo_jump.spec.ts` の cleanup を、末尾ノード推測ではなく明示的な復元へ寄せる。

実装時の第一候補は、テスト内で追加した `Text` ノード参照を保持し、`finally` でその参照を削除する方式とする。`memo-preview` 全体の `innerHTML` スナップショット復元も候補だが、既存 DOM に対する副作用が大きくなりやすいため、まずは対象ノード単位の cleanup を優先する。

共通化が自然な場合でも、同ファイル内の小さな helper に留める。`tests/e2e/helpers.ts` への抽出は P2 の別 backlog として残す。

## 受け入れ基準

- 次タスクとして `E2E テストの DOM クリーンアップ戦略見直し` を選ぶことが明記されている。
- `verify.sh` E2E 統合検討は後続候補として残っている。
- 実装時の主対象が `tests/e2e/memo_jump.spec.ts` に限定されている。
- 既存テストの検証意図を変えず、cleanup の安全性だけを上げる方針になっている。
- `tests/e2e/helpers.ts` 抽出や新規仕様テスト追加を今回スコープから外している。

## 検証

この設計は docs-only の選定記録なので、TDD ではなく文書検証で確認する。

```bash
rg -n "E2E テストの DOM クリーンアップ戦略見直し|verify.sh|tests/e2e/memo_jump.spec.ts" docs/superpowers/specs/2026-04-28-backlog-next-task-selection-design.md
rg -n "T(BD|ODO)" docs/superpowers/specs/2026-04-28-backlog-next-task-selection-design.md
```

実装フェーズに進む場合は、別途 plan を作成し、`npm run test:e2e -- memo_jump.spec.ts` または同等の対象 E2E、`npm run typecheck`、必要に応じて `./verify.sh` で検証する。

## セキュリティ考慮

この選定は直接のセキュリティ修正ではない。E2E の cleanup 信頼性を上げることで、将来の回帰検知を安定させるための作業である。

`BACKLOG.md` の項目は外部レビュー由来の懸念を含むため、実装済みの脆弱性や現在の攻撃可能性として断定しない。実装時には、現行のテストコードと実行結果に基づいて再確認する。

## 影響範囲

- 変更対象: `docs/superpowers/specs/2026-04-28-backlog-next-task-selection-design.md`
- 次タスクの主対象: `tests/e2e/memo_jump.spec.ts`
- 後続候補: `verify.sh` への E2E 統合可否検討
- 実装コードへの影響: なし
- テストコードへの影響: この設計時点ではなし

## ロールバック

docs-only の選定記録なので、この spec 追加コミットを revert すれば元に戻せる。

実装フェーズで cleanup を変更した場合も、変更範囲を `tests/e2e/memo_jump.spec.ts` に閉じ、必要なら当該テスト変更だけを revert できる形にする。
