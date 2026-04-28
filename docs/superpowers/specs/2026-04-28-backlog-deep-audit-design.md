# BACKLOG.md 深掘り監査設計

**作成日**: 2026-04-28
**対象ファイル**: `docs/todo/BACKLOG.md`

## 目的

`docs/todo/BACKLOG.md` を、今後の作業候補として使いやすい状態へ更新する。

現在の `BACKLOG.md` には、実装済みだが未完了のまま残っている項目、粒度が細かすぎる重複項目、現在のコードやテストに照らしてまだ価値がある項目が混在している。今回の監査では、コード・テスト・git 履歴に基づいて各項目を再判定し、実行候補リストを短く保つ。

## 非ゴール

- backlog 項目の実装は行わない。
- `docs/todo/TODO.md` の High / Medium 項目は再設計しない。
- 新しい機能要件やテスト要件を、根拠なく追加しない。
- 完了済み項目の詳細な作業レポートを作らない。
- `docs/superpowers/plans/` の既存計画を更新しない。

## 監査方針

完了済み項目は `Done` に残し、実装根拠を短く添える。今後の作業候補でない重複・陳腐化項目は実行リストから外す。まだ価値がある未完了項目は `P1 / P2 / P3` のいずれかに残す。

分類ルールは以下とする。

- 実装済みが確認できた項目は `Done` へ移す。
- まだコード上に改善余地が残る項目は `P1 / P2 / P3` のどこかに残す。
- 同じリスクをより広い項目が包含しているものは統合する。
- 外部レビュー ID など rot-prone な参照は、可能なら対象コメント除去タスクへ具体化する。
- マイクロ最適化や大規模リファクタは、現在も根拠がある場合だけ P3 に残す。
- セキュリティ境界、回帰検知、検証基盤に近いものほど上に置く。

## 具体的な更新内容

### Done へ移す項目

`E2E テストの DOM クリーンアップ戦略見直し` は `Done` へ移す。

根拠は `1ee8916 test: E2E DOM cleanupを明示ノード削除に変更 (#99)`。現在の `tests/e2e/memo_jump.spec.ts` では `lineHint.remove()` による明示 cleanup が使われており、`container.lastChild` 推測 cleanup は対象範囲から消えている。

`E2E を verify.sh に統合するか検討` は `Done` へ移す。

根拠は `4241390 chore: verifyにE2E opt-inを追加 (#100)`。現在の完了内容は「常時統合」ではなく opt-in 統合なので、本文は現状に合わせて書き換える。

### P2 に残す項目

`augmentHashWithTrailingLineHint` ELEMENT_NODE sibling の直接回帰テストは P2 に残す。

`src/template/assets/js/content.js` は `TEXT_NODE` と `ELEMENT_NODE` の両方で `textContent` を見る実装になっている。一方、現行 `tests/e2e/memo_jump.spec.ts` の直接テストは text node 経路が中心で、`document.createElement('span')` を sibling に置く直接回帰テストは見当たらない。

`memo_jump.spec.ts` の Codex review ID 削除は P2 に残す。

現在も `tests/e2e/memo_jump.spec.ts` に `Codex review #4136142343` という rot-prone comment が残っている。外部レビュー ID ではなく、false-positive パターン自体の説明へ置き換える小タスクとして残す。

`E2E 共通ヘルパー抽出` は P2 に残すが、対象を現在の重複に更新する。

`tests/e2e/helpers.ts` はまだ存在しない。`resetFixtures`、`stabilizeWebSocketHarness`、`requireUpdateContent` 系など、複数 spec で近い helper が残っているため、抽出候補として有効。ただし大きな横断変更になるので P1 には上げない。

TOC pending navigation 関連の 3 項目は 1 つに統合して P2 に残す。

統合対象は以下。

- 猶予期間中の連続 TOC クリックでの挙動検証
- `TOC_NAVIGATION_SLACK_PX` 境界の回帰テスト
- active 遷移フラッシュ厳密検証

いずれも `tests/e2e/text_selection_defer.spec.ts` と `src/template/assets/js/sidebar.js` の pending TOC navigation 回帰検知であり、個別項目のままだと粒度が細かすぎる。統合後の項目には、3 つの受け入れ観点として残す。

### P3 に残す項目

`インラインブラウザJS の TS 化` は P3 に残す。

ブラウザ側 JS は現在も `src/template/assets/js/*.js` として存在する。Rust 単体ビルドの明快さと Node 依存追加のトレードオフが大きいため、長期改善として扱う。

`render_markdown` の責務分割は P3 に残す。

`src/renderer/mod.rs` は現在も大きく、`render_markdown` 周辺の責務分割余地がある。大規模リファクタなので、着手時は先にテストカバレッジ確認と小分け計画が必要。

`catalog.rs` のパス構築での Vec アロケーション削減は P3 に残す。

`src/server/files/catalog.rs` には `components().map(...).collect::<Vec<_>>().join("/")` が残っている。これは性能上のマイクロ最適化なので P3 のまま維持する。

`README` のアーキテクチャ図更新は P3 に残す。

`README.md` の図は `files.rs`、`websocket.rs`、`renderer.rs`、`template.rs` など単一ファイル前提の記述が残っている。現在の実装は `src/server/files/`、`src/server/session.rs`、`src/renderer/`、`src/template/` に分割済みなので、ドキュメント rot として残す。

## 受け入れ基準

- 実装済みの P1 項目が未完了リストから外れ、`Done` に移っている。
- `E2E を verify.sh に統合するか検討` の説明が opt-in 統合済みの現状に合っている。
- TOC pending navigation 関連の 3 項目が 1 項目に統合され、3 つの検証観点が失われていない。
- `augmentHashWithTrailingLineHint` の ELEMENT_NODE sibling 直接回帰テスト、Codex review ID 削除、E2E helper 抽出が P2 に残っている。
- P3 の長期改善 4 項目が残り、現在のコードに基づく理由になっている。
- 今後の実行候補ではない重複・陳腐化項目が未完了リストに残っていない。
- 実装コード、テストコード、`verify.sh` は変更しない。

## 検証

docs-only 変更のため TDD は行わず、文書検証で確認する。

```bash
rg -n "E2E テストの DOM クリーンアップ戦略見直し|verify.sh|TOC|augmentHashWithTrailingLineHint|Codex review|E2E 共通ヘルパー|render_markdown|catalog.rs|README" docs/todo/BACKLOG.md
rg -n "container\\.lastChild|lastChild\\.remove\\(|Codex review #4136142343|collect::<Vec<_>>|\\.join\\(\"/\"\\)|websocket\\.rs|renderer\\.rs|template\\.rs|files\\.rs" tests/e2e/memo_jump.spec.ts src/server/files/catalog.rs README.md
rg -n "^- \\[ \\]|^- \\[x\\]|^## P1|^## P2|^## P3|^## Done" docs/todo/BACKLOG.md
git diff -- docs/todo/BACKLOG.md
```

目視では、未完了リストが「次に着手できる候補」に絞られていること、`Done` が実行候補を邪魔しない位置にあること、外部レビュー由来の記述を現在の脆弱性として断定していないことを確認する。

## セキュリティ考慮

`BACKLOG.md` の由来には外部レビュー、過去の探索結果、AI 生成物が含まれる。これらは未検証入力として扱い、現在の脆弱性や実装済み事実としては断定しない。

今回の変更は backlog 整理であり、セキュリティ境界そのものを強化しない。ただし、検証基盤や回帰検知に関わる項目を適切に残すことで、将来のセキュリティ関連回帰を見落としにくくする。

## 影響範囲

- 変更対象: `docs/todo/BACKLOG.md`
- 参照対象: `tests/e2e/memo_jump.spec.ts`、`tests/e2e/text_selection_defer.spec.ts`、`src/template/assets/js/content.js`、`src/template/assets/js/sidebar.js`、`src/server/files/catalog.rs`、`README.md`
- 実装コードへの影響: なし
- テストコードへの影響: なし
- 依存ファイルへの影響: なし

## ロールバック

docs-only 変更なので、`docs/todo/BACKLOG.md` の差分を revert すれば元に戻せる。

設計書自体を戻す場合は、この spec 追加コミットを revert する。
