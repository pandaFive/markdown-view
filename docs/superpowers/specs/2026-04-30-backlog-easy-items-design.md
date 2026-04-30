# BACKLOG 簡易項目実行設計

**作成日**: 2026-04-30
**対象ファイル**: `docs/todo/BACKLOG.md`, `README.md`

## 目的

`docs/todo/BACKLOG.md` の未完了項目から、簡単に実行できるものをまとめて処理する。

今回の対象は、既に実装済みで未完了欄に残っている P2 項目の整理と、コード変更なしで対応できる `README.md` のアーキテクチャ図更新に限定する。未完了候補の棚卸し精度を上げ、README を現行のモジュール構成に合わせる。

## 非ゴール

- プロダクションコードは変更しない。
- E2E テストや Rust テストの挙動は変更しない。
- インラインブラウザ JS の TypeScript 化は行わない。
- `catalog.rs` のパス構築最適化は行わない。
- BACKLOG 由来の古いレビュー ID や外部文書を、現在の不具合として断定しない。

## 対象項目

### 完了整理

以下の P2 項目は現行コード上で対応済みのため、`Done` へ移動する。

- `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling の直接回帰テスト
- TOC pending navigation 小揺らしテストの grace 内外分離
- `memo_jump.spec.ts` の Codex review ID コメント削除

### README 更新

`README.md` のアーキテクチャ図は、単一ファイル前提の古い表記を現在の分割構成へ合わせる。

主な対応は次のとおり。

- `src/server/` 配下の `session`, `broadcast`, `watch`, `files/` を反映する。
- `src/renderer/` 配下の `render`, `state`, `line`, `security`, `highlight`, `toc` を反映する。
- `src/template/` 配下の `page`, `message`, `assets`, `tree` と埋め込み CSS/JS を反映する。
- データフローとセキュリティ節の記述は、既存方針を維持しながら必要最小限だけ整える。

## 受け入れ基準

- `BACKLOG.md` の P2 未完了欄に、対応済み項目が残っていない。
- `BACKLOG.md` の `Done` に、移動した項目の完了根拠が残っている。
- `README.md` のアーキテクチャ図が、現行の `src/` 配下構成と矛盾しない。
- コード、テスト、設定ファイルには変更がない。
- セキュリティ説明は弱めず、Host/Origin 検証、CSP、HTML サニタイズ、パス検証、ファイルサイズ上限の前提を維持する。

## 検証

docs 変更のため、TDD ではなく文書検証を行う。

```bash
rg -n "augmentHashWithTrailingLineHint|TOC pending|Codex review ID|README のアーキテクチャ図" docs/todo/BACKLOG.md
rg -n "src/server/|src/renderer/|src/template/" README.md
rg -n "T(BD|ODO)" docs/superpowers/specs/2026-04-30-backlog-easy-items-design.md docs/todo/BACKLOG.md README.md
./verify.sh
```

`./verify.sh` は docs-only 変更に対しては広めの確認だが、既存検証に文書変更が副作用を出していないことを確認するために実行する。

## セキュリティ考慮

今回の変更は docs-only であり、セキュリティ境界そのものは変えない。

BACKLOG の項目には外部レビュー由来の記述が含まれるため、現行コードとテストで確認できる事実だけを完了根拠にする。README 更新では localhost 限定、Host/Origin 検証、HTML サニタイズ、CSP、パストラバーサル防止、ファイルサイズ上限の説明を維持し、実装より強い保証を文章で主張しない。

## 影響範囲

- 変更対象: `docs/todo/BACKLOG.md`, `README.md`
- 追加対象: `docs/superpowers/specs/2026-04-30-backlog-easy-items-design.md`
- 参照対象: `tests/e2e/memo_jump.spec.ts`, `tests/e2e/text_selection_defer.spec.ts`, `src/`
- 実装コードへの影響: なし
- テストコードへの影響: なし

## ロールバック

docs-only 変更なので、該当コミットを revert すれば元に戻せる。README 更新と BACKLOG 整理を分けて戻す必要がある場合も、対象ファイル単位で revert できる。
