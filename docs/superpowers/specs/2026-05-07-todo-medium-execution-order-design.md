# TODO Medium 実行順明確化設計

**作成日**: 2026-05-07
**対象ファイル**: `docs/todo/TODO.md`

## 目的

`docs/todo/TODO.md` の Medium Priority 5件を、次に実行する順が分かる状態へ整理する。

現状は High Priority が空で、Medium Priority が次の実行候補になっている。一方で Medium 内の順序が「どれから手を付けるべきか」を十分に説明していないため、ユーザー影響とリスク低減の観点で並びを明確にする。

## 非目的

- TODO 項目そのものは実装しない。
- Medium 項目を大きく書き換えない。
- Medium から High / BACKLOG への大幅な昇降格はしない。
- `docs/todo/BACKLOG.md` の再分類はしない。
- プロダクションコード、テストコード、設定ファイルは変更しない。

## 実行順方針

Medium Priority はリスク低減順で並べる。

優先する軸は、ユーザーが直接遭遇する起動不能や原因不明の機能低下、セキュリティ境界や silent failure に近い項目である。巨大な構造変更は重要でも、最初に置くと実行単位が重くなりすぎるため、短中期で安全に進められる項目を先に置く。

## 推奨順序

1. `watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する`
2. `BroadcastMessage::Refresh` の memo_refresh 契約と、メモ読込失敗時の degrade 通知を明示する
3. `ブラウザ JS の責務境界を小モジュールへ分割する`
4. `template/mod.rs` のテストをサブモジュールへ分割し、`render_page` の 62 行 `format!` を関数分割する
5. `RenderState` を `enum BlockContext` スタックに置き換えて open/close 対応を型化する

## 順序の根拠

`watcher` の ENOSPC 対応は、大規模ディレクトリで起動失敗や原因不明の監視失敗につながる。`node_modules`、`target`、`.git` の除外と操作可能なエラーメッセージは、ユーザー体験への直接効果が大きい。

`BroadcastMessage::Refresh` と memo degrade 通知は、メモ読込失敗が「メモが消えた」ように見える silent failure を減らす。データ消失ではない状態を UI が区別できるようにするため、watcher の次に扱う。

ブラウザ JS 分割は、`content-renderer` 境界の後続として、検索、リンク解決、履歴、スクロール、副作用の責務を整理する。`innerHTML` 使用やサーバー生成 HTML への信頼境界に近いため、template 分割より先に置く。

template 分割は保守性と属性エスケープ境界の改善として価値がある。ただし主な問題はテスト集中と大きな `format!` であり、前段2件ほどの直接的なユーザー影響や silent failure ではないため、4番目に置く。

`RenderState` の `BlockContext` 化は構造負債として重要だが、変更範囲と回帰リスクが大きい。先に watcher、memo、ブラウザ JS、template の境界を整理してから、独立した大きめの設計・実装単位として扱う。

## 受け入れ条件

- `docs/todo/TODO.md` の Medium Priority が推奨順序に並んでいる。
- Medium 節に、実行順がリスク低減順であることが短く明記されている。
- 5件すべての未完了項目が残っている。
- 各項目のセキュリティ境界、silent failure、ユーザー影響に関する説明が失われていない。
- `docs/todo/BACKLOG.md`、プロダクションコード、テストコード、設定ファイルに変更がない。

## 検証

docs-only 変更のため、TDD ではなく文書検証を行う。

```bash
rg -n "Medium Priority|リスク低減|watcher|BroadcastMessage|ブラウザ JS|template/mod.rs|RenderState" docs/todo/TODO.md docs/superpowers/specs/2026-05-07-todo-medium-execution-order-design.md
rg -n "未[定]|要確[認]" docs/superpowers/specs/2026-05-07-todo-medium-execution-order-design.md
git diff --stat
./verify.sh
```

`./verify.sh` は docs-only 変更には広めの確認だが、このリポジトリの必須検証として実行する。

## セキュリティ考慮

この作業は docs-only であり、実行時のセキュリティ境界は変更しない。

ただし TODO の実行順は将来の修正順へ影響する。Host/Origin 検証、パス検証、HTML サニタイズ、CSP、監視イベント、メモ読込失敗、ブラウザ側 `innerHTML` 信頼境界に関わる記述は、整理時に削らず残す。

TODO の記述は過去レビューやコードベース探索由来の未信頼入力を含むため、実装済みの脆弱性や現在の攻撃可能性として断定しない。実装フェーズでは、現行コードとテストで再確認してから変更する。

## 影響範囲

- 変更対象: `docs/todo/TODO.md`
- 追加対象: `docs/superpowers/specs/2026-05-07-todo-medium-execution-order-design.md`
- 参照対象: `docs/todo/BACKLOG.md`, `docs/superpowers/specs/`, `src/`, `tests/`
- 実装コードへの影響: なし
- テストコードへの影響: なし

## ロールバック

docs-only 変更なので、設計書コミットと TODO 整理コミットを revert すれば元に戻せる。

一部だけ戻す場合は、`docs/todo/TODO.md` の Medium Priority 項目を以前の順序へ戻す。プロダクションコードへ影響しないため、ロールバック時の実行時リスクはない。
