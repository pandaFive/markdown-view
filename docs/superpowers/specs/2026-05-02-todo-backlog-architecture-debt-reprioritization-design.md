# TODO/BACKLOG アーキテクチャ負債軸 再優先度付け設計

**作成日**: 2026-05-02
**対象ファイル**: `docs/todo/TODO.md`, `docs/todo/BACKLOG.md`

## 目的

`docs/todo/TODO.md` と `docs/todo/BACKLOG.md` の未完了項目を、アーキテクチャ負債の大きさを主軸に再分類する。

現行コード、最近のコミット、既存の spec/plan で完了が確認できる項目は Done へ移す。未完了項目は、後続改善の前提になる構造的な負債を上位に置き、局所的な改善や低リスクの品質改善は BACKLOG 側へ寄せる。

## 非目的

- プロダクションコードは変更しない。
- テストコードは変更しない。
- TODO 項目そのものは実装しない。
- `TODO.md` と `BACKLOG.md` の運用構造を大幅に変えない。
- 外部レビュー由来の記述を、現行コードで確認せずに事実扱いしない。

## 整理方針

`TODO.md` は High / Medium の優先項目を扱い、`BACKLOG.md` は Low Priority の蓄積項目を扱う既存構造を維持する。

優先度の主軸はアーキテクチャ負債の大きさとする。具体的には、複数モジュールの境界をまたぐもの、将来の変更時に不変条件を壊しやすいもの、同種の修正を続ける前提になるものを上位へ置く。

セキュリティ境界、silent failure、データ破損につながる項目は補正対象にする。主軸は構造負債だが、Host/Origin 検証、パス検証、HTML サニタイズ、CSP、監視エラー、atomic save のような安全性に関わる項目を根拠なく低優先度化しない。

完了済み判定は保守的に行う。最近のコミット、現行コード、テスト、既存 spec/plan の少なくとも一つで完了根拠を確認できる項目だけを Done へ移す。判断が曖昧なものは未完了に残し、必要なら説明文を現在の状態へ更新する。

## 分類基準

### High Priority

後続作業の前提になる構造変更、または重大な安全性・観測性リスクを含む項目を置く。

例:

- renderer / TOC / search のような複数経路で同じ Markdown 解釈が分岐している項目。
- watcher 由来パスを読込直前に再検証するなど、セキュリティ境界の構造に関わる項目。
- Host 検証の middleware 化のように、守り忘れを設計上防ぐ項目。

### Medium Priority

明確な負債はあるが、単独で後続作業全体を塞いでいない項目を置く。

例:

- async ハンドラ内の同期 I/O 解消。
- shutdown 観測性の統合。
- `AppState` の Arc 二重ラップ解消。
- template や browser JS の責務分割。

### BACKLOG P1

低優先度扱いだが、検証基盤や安全性の補助として価値が高い項目を置く。

例:

- Windows 固有の retry 条件細分化。
- sentinel 衝突回避テスト。
- watcher error の破棄検知強化。

### BACKLOG P2 / P3

局所的な保守性改善、ドキュメント追従、低リスクな API surface 縮小、マイクロ最適化を置く。

P2 は近いうちに処理しやすい品質改善、P3 は長期改善または効果確認が先に必要なものとする。

## 作業手順

1. `TODO.md` と `BACKLOG.md` の未完了項目を一覧化する。
2. 最近のコミットと既存 spec/plan を確認し、完了済み候補を洗い出す。
3. 完了済み候補を現行コードまたはテストで確認する。
4. 未完了項目を High / Medium / BACKLOG P1-P3 へ再分類する。
5. 移動した項目には、由来や判断理由が失われないよう説明を残す。
6. 重複項目や実装済み記述があれば統合する。
7. `rg` と `./verify.sh` で docs-only 変更の副作用を確認する。

## 受け入れ条件

- `TODO.md` に現行コード上で完了済みと確認できる項目が残っていない。
- High / Medium の順序が、アーキテクチャ負債の大きさを主軸に説明できる。
- `BACKLOG.md` へ移した項目にも、由来と優先度判断の根拠が残っている。
- セキュリティ境界、silent failure、データ破損に関わる項目を根拠なく低優先度化していない。
- プロダクションコード、テストコード、設定ファイルに変更がない。
- `./verify.sh` が通る、または docs-only 変更と無関係な失敗であることを報告する。

## 検証

docs 変更のため、TDD ではなく文書検証を行う。

```bash
rg -n "notify_update|receiver=0|完了根拠|TODO|TBD" docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/specs/2026-05-02-todo-backlog-architecture-debt-reprioritization-design.md
rg -n "Host|Origin|sanitize|CSP|path|silent|atomic|watcher" docs/todo/TODO.md docs/todo/BACKLOG.md
git diff --stat
./verify.sh
```

`./verify.sh` は docs-only 変更には広めの確認だが、既存の必須検証として実行する。

## セキュリティ考慮

今回の作業は docs-only であり、セキュリティ境界そのものは変更しない。

ただし TODO/BACKLOG は将来の実装優先度に影響するため、セキュリティ項目の扱いは慎重にする。Host/Origin 検証、パス検証、HTML サニタイズ、CSP、監視イベント、atomic save、ログ出力の機密性に関わる項目は、アーキテクチャ負債軸で見ても低く見積もりすぎない。

外部レビュー、検索結果、過去メモ由来の記述は未信頼入力として扱う。現行コードまたはテストで確認できない主張は断定せず、未確認の改善候補として残す。

## 影響範囲

- 変更対象: `docs/todo/TODO.md`, `docs/todo/BACKLOG.md`
- 追加対象: `docs/superpowers/specs/2026-05-02-todo-backlog-architecture-debt-reprioritization-design.md`
- 参照対象: `docs/superpowers/specs/`, `docs/superpowers/plans/`, `src/`, `tests/`
- 実装コードへの影響: なし
- テストコードへの影響: なし

## ロールバック

docs-only 変更なので、設計書コミットと TODO/BACKLOG 整理コミットを revert すれば元に戻せる。

整理内容の一部だけ戻す必要がある場合は、`docs/todo/TODO.md` と `docs/todo/BACKLOG.md` の該当項目単位で revert または再移動する。プロダクションコードへ影響しないため、ロールバック時の実行時リスクはない。
