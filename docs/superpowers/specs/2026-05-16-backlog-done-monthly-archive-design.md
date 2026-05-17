# BACKLOG Done 月別アーカイブ設計

**作成日**: 2026-05-16
**対象ファイル**: `docs/todo/BACKLOG.md`, `docs/done/DONE-2026-05.md`

## 目的

`docs/todo/BACKLOG.md` を未完了の低優先候補に集中させる。

現在の `BACKLOG.md` は P2 / P3 の未完了項目よりも `Done` 履歴が大きくなっており、次に見るべき低優先候補を把握しにくい。完了済み履歴は参照価値があるため削除せず、2026年5月分の月別アーカイブとして `docs/done/DONE-2026-05.md` へ移す。

## 非目的

- P2 / P3 の優先順位は変更しない。
- 完了項目の内容、完了根拠、由来を要約・改変しない。
- `TODO.md` の Done Summary は今回移動しない。
- コード、テスト、設定、ビルド手順は変更しない。
- TODO / BACKLOG の運用全体をロードマップ形式へ置き換えない。

## 方針

### 月別アーカイブ

`docs/done/DONE-2026-05.md` を新規作成し、`docs/todo/BACKLOG.md` の `## Done` 配下にある完了項目を原文ほぼそのまま移す。

新規ファイルの見出しは既存の `docs/done/DONE-YYYY-MM-DD.md` と揃え、次のような構造にする。

```markdown
# 完了タスク (2026-05)

このファイルには 2026-05 に `docs/todo/BACKLOG.md` から移動した完了済みタスクを記録します。

---

## BACKLOG 完了履歴

- [x] ...
```

日別ではなく月別にする理由は、今回の対象が単一作業日の完了記録ではなく、`BACKLOG.md` に蓄積した複数日の完了履歴だからである。既存の `docs/done/` 配下へ置き、完了履歴の探索場所は増やさない。

### BACKLOG の軽量化

`BACKLOG.md` の冒頭説明を更新し、未完了の Low / 長期改善候補を置く場所であることを明確にする。

`## Done` の長い履歴は削除し、代わりに `docs/done/DONE-2026-05.md` への参照を短く残す。参照文は、完了履歴が削除されたのではなくアーカイブへ移動したことを明示する。

例:

```markdown
## Done

完了済み履歴は [`docs/done/DONE-2026-05.md`](../done/DONE-2026-05.md) へ移動した。
```

## セキュリティ

docs-only 変更であり、実行時の Host / Origin 検証、CSP、HTML sanitize、path validation、memo sidecar 検証、file size limit には触れない。

ただし `BACKLOG.md` の Done 履歴には、セキュリティ境界、silent failure、データ安全性、監視不能性に関する完了根拠が含まれる。これらを要約で失わないよう、完了項目の本文、完了根拠、由来を原文ほぼそのまま月別アーカイブへ移す。

外部検索結果や取得テキストは使わず、リポジトリ内の既存文書だけを入力として扱う。

## 受け入れ条件

- `BACKLOG.md` を開くと、未完了の P1 / P2 / P3 が主な内容として読める。
- `BACKLOG.md` から長い `Done` 履歴が取り除かれ、`docs/done/DONE-2026-05.md` への参照が残っている。
- `docs/done/DONE-2026-05.md` に、移動前の `BACKLOG.md` の Done 項目が原文ほぼそのまま残っている。
- 各完了項目のタイトル、内容、完了根拠、由来が移動で失われていない。
- P2 / P3 の未完了項目の順序と内容は変更されていない。
- 新規設計書と新規アーカイブに、プレースホルダーや先送り表現が残っていない。

## 検証

docs-only 変更のため、Rust のテスト実行は必須にしない。文書整合性を次で確認する。

```bash
rg -n "^## |^- \\[ \\]|^- \\[x\\]|DONE-2026-05" docs/todo/BACKLOG.md docs/done/DONE-2026-05.md
rg -n "T[B]D|未[定]|あと[で]" docs/todo/BACKLOG.md docs/done/DONE-2026-05.md docs/superpowers/specs/2026-05-16-backlog-done-monthly-archive-design.md
git diff -- docs/todo/BACKLOG.md docs/done/DONE-2026-05.md docs/superpowers/specs/2026-05-16-backlog-done-monthly-archive-design.md
```

必要なら、移動前後の完了項目数を `rg -n "^- \\[x\\]"` で比較する。

## 影響範囲

- `docs/todo/BACKLOG.md`: 未完了候補中心の文書へ軽量化する。
- `docs/done/DONE-2026-05.md`: BACKLOG 由来の2026年5月完了履歴を保存する。
- `docs/superpowers/specs/2026-05-16-backlog-done-monthly-archive-design.md`: 本設計判断を保存する。

コード、テスト、設定、CI、永続データ、生成物への影響はない。

## ロールバック

`docs/done/DONE-2026-05.md` を削除し、`docs/todo/BACKLOG.md` の差分を戻せば復旧できる。

設計書だけを戻す場合は、このファイルを revert する。コードや永続データを変更しないため、ロールバック時の実行時リスクはない。

## 見積もり

- 人間作業: 20-40 分
- Codex/AI 支援: 10-20 分
