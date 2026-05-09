# TODO/BACKLOG 重要度・将来影響度 再優先度付け設計

**作成日**: 2026-05-09
**対象ファイル**: `docs/todo/TODO.md`, `docs/todo/BACKLOG.md`

## 目的

`docs/todo/TODO.md` と `docs/todo/BACKLOG.md` を、重要度と将来への影響度を主軸に再整理する。

`TODO.md` は次に実行する優先候補に限定し、`BACKLOG.md` は低優先・長期候補の保管場所として維持する。重要度が高い項目は `BACKLOG.md` から `TODO.md` の High / Medium へ昇格させ、次に見るべき作業候補を迷いにくくする。

## 非目的

- プロダクションコードは変更しない。
- テストコードは変更しない。
- TODO/BACKLOG 項目そのものは実装しない。
- `TODO.md` と `BACKLOG.md` の基本運用を、別形式のロードマップへ置き換えない。
- 外部レビュー由来の記述を、現行コードで確認せずに現在の脆弱性として断定しない。

## 整理方針

`TODO.md` は実行優先候補、`BACKLOG.md` は保管候補として役割を分ける。

`High Priority` は、放置するとセキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目だけにする。`Medium Priority` は、すぐ重大事故ではないが、後続改修の前提、設計負債、検証基盤として効く項目にする。

`BACKLOG.md` は P1 / P2 / P3 の構造を維持する。P1 は昇格しないが次回の整理や計画で優先して見る候補、P2 は保守性・局所回帰検知、P3 は長期改善・低緊急・計測後でよい最適化を置く。

各項目には、対象ファイル、現状、対応、由来を過不足なく残す。移動や順位変更を行う場合は、なぜその位置に置いたか分かる短い判断根拠を残す。長文化は避け、将来の作業選定に必要な情報だけを残す。

## 分類基準

### High Priority

以下のいずれかを満たす項目を置く。

- Host / Origin / CSP / HTML sanitize / path validation のようなセキュリティ境界に関わる。
- エラーや異常状態が破棄され、ユーザーまたはログから検知できなくなる。
- データ保存、復元、メモ、ファイル読込で、失敗時にデータ安全性へ影響する。
- 既存の防御層を弱める変更を将来誘発しやすい構造負債である。

### Medium Priority

以下のいずれかを満たす項目を置く。

- 複数機能の前提になる設計メモ、契約明文化、テスト基盤である。
- route 追加、renderer 変更、watcher 変更など、今後の作業で守り忘れを防ぐ。
- 将来の改修コストを下げるが、現在の実行時リスクは限定的である。

### BACKLOG P1

昇格はしないが、次の整理・計画時に優先して見るべき候補を置く。検証基盤や観測性に効くが、影響範囲が局所的なものを含める。

### BACKLOG P2 / P3

P2 は保守性・局所回帰検知に効く項目を置く。P3 は長期改善、低緊急、計測してから判断すべき最適化を置く。

純粋なマイクロ最適化、UI 文言の局所調整、実運用で必要性が出てからでよいメトリクス化は P3 に置く。

## 作業手順

1. `TODO.md` と `BACKLOG.md` の未完了項目を一覧化する。
2. `BACKLOG.md` の各項目を、重要度と将来影響度で High / Medium / BACKLOG P1-P3 へ仮分類する。
3. セキュリティ境界、未信頼入力、silent failure、データ安全性に関わる項目を低く見積もっていないか確認する。
4. 昇格対象を `TODO.md` へ移し、`BACKLOG.md` 側から重複しないよう整理する。
5. `BACKLOG.md` の残項目を P1 / P2 / P3 内で重要度と将来影響度順に並べ直す。
6. 完了済み項目は現行通り Done 側に置き、未完了候補に混ぜない。
7. `rg` と `git diff` で見出し、未完了件数、由来、重複、読みやすさを確認する。

## 受け入れ条件

- `TODO.md` の High / Medium に、昇格すべき項目だけが移動している。
- `BACKLOG.md` の未完了項目が、重要度と将来影響度順に再配置されている。
- 各未完了項目に、対象ファイル、現状、対応、由来が過不足なく残っている。
- セキュリティ境界、未信頼入力、silent failure、データ安全性に関わる項目が低く見積もられていない。
- docs-only 変更として、実装やテストコードには触れていない。
- `rg` と `git diff` で、見出し構造、未完了項目、由来、重複を確認できている。

## 検証

docs 変更のため、TDD ではなく文書検証を行う。

```bash
rg -n "^## |^- \\[ \\]|^- \\[x\\]|由来:" docs/todo/TODO.md docs/todo/BACKLOG.md
rg -n "Host|Origin|CSP|sanitize|path|silent|watcher|atomic|memo|innerHTML|未信頼" docs/todo/TODO.md docs/todo/BACKLOG.md
rg -n "TBD|未定|要確認|あとで|完了済みだが未完了" docs/todo/TODO.md docs/todo/BACKLOG.md
git diff -- docs/todo/TODO.md docs/todo/BACKLOG.md
```

`./verify.sh` は必須ではない。今回の設計は docs-only の整理方針であり、実装後の計画段階で必要な検証を改めて定義する。

## セキュリティ考慮

今回の作業は docs-only であり、セキュリティ境界そのものは変更しない。

ただし TODO/BACKLOG は将来の実装順に影響する。Host / Origin 検証、HTML サニタイズ、CSP、パス検証、メモ保存、watcher エラー、ログ観測性のような安全性に関わる項目は、低く見積もりすぎない。

外部レビュー、検索結果、過去メモ由来の記述は未信頼入力として扱う。現行コードまたはテストで確認できない主張は断定せず、未確認の改善候補として残す。

## 影響範囲

- 変更対象: `docs/todo/TODO.md`, `docs/todo/BACKLOG.md`
- 追加対象: `docs/superpowers/specs/2026-05-09-backlog-importance-impact-reprioritization-design.md`
- 実装コードへの影響: なし
- テストコードへの影響: なし
- 間接影響: 今後の作業順、PR 作成順、設計書作成順

## ロールバック

docs-only 変更なので、設計書コミットと TODO/BACKLOG 整理コミットを revert すれば元に戻せる。

整理内容の一部だけ戻す必要がある場合は、`docs/todo/TODO.md` と `docs/todo/BACKLOG.md` の該当項目単位で revert または再移動する。プロダクションコードへ影響しないため、ロールバック時の実行時リスクはない。
