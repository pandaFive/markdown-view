# TODO 完了済み項目クリーンアップ設計

## 目的

`docs/todo/TODO.md` に残っている完了済み項目を取り除き、未完了の Medium Priority 候補だけが見える状態に戻す。

今回の主対象は、直近コミット `a5b0e80 refactor: AppStateライフサイクルをArc共有へ統一 (#128)` と既存設計書 `docs/superpowers/specs/2026-05-05-appstate-arc-lifecycle-design.md` で完了が確認できる `AppState` ライフサイクル整理項目である。

## 非目的

- Medium Priority 全体の優先度を再評価しない。
- `docs/todo/BACKLOG.md` の棚卸しはしない。
- プロダクションコード、テストコード、設定ファイルは変更しない。
- `TODO.md` 内の参照行番号を全面更新しない。
- 未完了 TODO 項目の実装設計や実装には進まない。

## 変更方針

`docs/todo/TODO.md` の既存構造を維持する。High Priority、Medium Priority、Done Summary の見出しや運用説明は変えない。

`AppState` 項目だけを Medium Priority から削除し、Done Summary に完了根拠つきで追加する。完了根拠には、`AppState` の共有が `Arc<AppState>` に統一されたこと、`with_memo_fs` が削除されたこと、生成時注入の契約へ寄ったことを簡潔に残す。

確認できない項目は動かさない。特に watcher、BroadcastMessage、RenderState、template、catalog、ブラウザ JS のようなセキュリティ境界や silent failure に関わる項目は、今回の整理対象に含めない。

## 影響範囲

- 変更対象: `docs/todo/TODO.md`
- 追加済み設計書: `docs/superpowers/specs/2026-05-05-todo-completed-item-cleanup-design.md`
- 参照対象: `src/server/state.rs`, `src/main.rs`, `docs/superpowers/specs/2026-05-05-appstate-arc-lifecycle-design.md`, 直近 git 履歴

実行時コードへの影響はない。テストコードへの影響もない。

## 検証

docs-only 変更のため、TDD ではなく文書検証を行う。

```bash
rg -n "AppState|with_memo_fs|Arc<AppState>" docs/todo/TODO.md docs/superpowers/specs/2026-05-05-todo-completed-item-cleanup-design.md
git diff -- docs/todo/TODO.md docs/superpowers/specs/2026-05-05-todo-completed-item-cleanup-design.md
```

`TODO.md` の Medium Priority に `AppState` 項目が残っていないこと、Done Summary に完了根拠があること、意図しない範囲の変更がないことを確認する。

`./verify.sh` は必須の最終検証として実行対象にできるが、今回の変更は docs-only であり、主な検証は差分確認と `rg` による構造確認とする。

## セキュリティ考慮

今回の作業は docs-only であり、Host/Origin 検証、パス検証、HTML sanitization、CSP、watcher 境界、memo I/O 境界は変更しない。

ただし TODO の整理は将来の実装順に影響するため、セキュリティ境界や silent failure に関わる未完了項目は移動しない。外部レビューや過去メモ由来の記述は未信頼入力として扱い、現行コードまたは git 履歴で確認できる完了だけを Done Summary へ移す。

## 受け入れ条件

- `docs/todo/TODO.md` の Medium Priority から完了済みの `AppState` 項目が削除されている。
- `docs/todo/TODO.md` の Done Summary に `AppState` 項目の完了根拠が追加されている。
- `docs/todo/BACKLOG.md`、プロダクションコード、テストコード、設定ファイルに変更がない。
- `rg` と `git diff` で意図した docs-only 変更であることを確認できる。

## ロールバック

docs-only 変更なので、設計書追加コミットまたは `docs/todo/TODO.md` の該当差分を revert すれば元に戻せる。

一部だけ戻す場合は、Done Summary へ移した `AppState` 項目を Medium Priority へ戻す。実行時コードに影響しないため、ロールバック時の実行時リスクはない。
