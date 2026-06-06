# 検索 RSS plateau 完了判定設計書

## 目的

`docs/todo/TODO.md` の Medium Priority「ディレクトリ検索 many-match の RSS plateau を切り分ける」を、既存測定に基づいて完了扱いにできる判断へ整理する。

完了理由は RSS plateau の解消ではなく、次の状態へ到達したこととする。

- plateau の発生条件と支配候補を分類できている。
- 残リスクを改善実装または環境差検証の別テーマとして扱える。
- 検索 API 契約と既存セキュリティ境界を弱めていない。

## 非ゴール

- production Rust code の変更。
- `scripts/measure-search-rss-plateau.mjs` の変更。
- 追加の重い HTTP 測定。
- native Linux や別 allocator での追加検証を必須にすること。
- `MALLOC_ARENA_MAX=1` の常用化判断。
- 検索アルゴリズム、allocator、Tokio runtime 構成の改善実装。

## 完了判定

既存の 2026-06-04 と 2026-06-05 の測定から、次の判断を `TODO.md` の Done Summary に移す。

- plateau は単一ファイル prefix many-match 後の anonymous memory が支配的である。
- multifile result-limit と short fallback は同規模の plateau を示しておらず、server 起動直後や result-limit 一般の基礎コストとは切り分け済みである。
- full fallback は 10MiB 未満の fixture で実検索できることを確認済みで、prefix とは別の性能経路として扱える。
- `MALLOC_ARENA_MAX=1` で settled anonymous RSS が下がったため、glibc allocator arena retained memory を主因候補として扱える。
- `arena1` でも settled `RssAnon` は 500MiB 台に残るため、WSL2 RSS/accounting 特性または prefix 経路の live allocation は残候補として明記する。
- これ以上の検証は「切り分ける」タスクではなく、改善実装や環境差検証の新規テーマで扱う。

## 文書更新

主な更新対象は `docs/todo/TODO.md` とする。

未完了 Medium 項目を Done Summary へ移し、完了根拠を圧縮して記録する。High / Medium Priority に未完了項目がなくなる場合は、現時点で実行候補がないことが読み取れるように短い説明を残す。

native Linux、別 allocator、prefix live allocation の追加検証は、今回の完了判定を妨げる未完了 High / Medium タスクにはしない。ただし残診断候補が不可視にならないよう、低優先の将来候補として `BACKLOG.md` P2 に残す。

## エラー処理

既存測定値の転記で矛盾が見つかった場合は、断定を避けて「主因候補」と「残候補」に分ける。別の未完了 High / Medium が見つかった場合は、その項目を維持し、今回の RSS plateau 項目だけを Done Summary へ移す。

文書 validation で placeholder、意図しない未完了 checkbox、または矛盾する完了表現が見つかった場合は、設計範囲内で修正する。

## セキュリティ

測定結果、`/proc` 情報、検索レスポンス、環境変数情報は未信頼入力として扱う。`TODO.md` と設計書には次を新たに書き足さない。

- 実パス。
- Markdown 本文断片。
- full process args。
- raw `/proc/maps` 行。
- 親環境変数の値。

完了根拠には、検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約を弱めていないことを明記する。

## 影響範囲

変更対象:

- `docs/todo/TODO.md`
- `docs/todo/BACKLOG.md`
- `docs/superpowers/specs/2026-06-06-search-rss-plateau-completion-design.md`
- `docs/superpowers/plans/2026-06-06-search-rss-plateau-completion.md`

参照対象:

- `scripts/measure-search-rss-plateau.mjs`
- `docs/superpowers/specs/2026-06-04-search-rss-plateau-design.md`
- `docs/superpowers/specs/2026-06-05-search-rss-allocator-profile-design.md`

production Rust code、測定スクリプト、UI、TypeScript、generated JS は変更しない。

## ロールバック

この4ファイルを含む docs-only commit を revert すればよい。コード挙動、HTTP API、WebSocket、検索アルゴリズムは変更しないため、追加の巻き戻しは不要である。

## 受け入れ基準

- `TODO.md` の RSS plateau 未完了項目が Done Summary へ移っている。
- 完了根拠に既存測定値、主因候補、残候補、セキュリティ境界維持が含まれている。
- High / Medium Priority の未完了項目が意図せず残っていない。
- native Linux、別 allocator、prefix live allocation の追加検証を今回の必須残件にせず、`BACKLOG.md` P2 の低優先候補として明示している。
- `git diff --check` と文書 validation が成功する。
- `./verify.sh` を実行しない場合は、docs-only 変更であることと残リスクを completion report に残す。

## 見積もり

- 人間作業: 20-40 分。
- Codex / AI 支援: 10-20 分。

`./verify.sh` まで実行する場合は追加で 5-15 分を見込む。

## 文書検証

実装後に次を確認する。

```bash
rg -n "RSS plateau|glibc allocator arena|WSL2|Host/Origin|SearchResponse|CSP" docs/todo/TODO.md docs/superpowers/specs/2026-06-06-search-rss-plateau-completion-design.md
! rg -n "^- \\[ \\]" docs/todo/TODO.md
placeholder_matches="$(rg -n -P 'T[B]D|TO[D]O[:：]|未[定]' docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/specs/2026-06-06-search-rss-plateau-completion-design.md docs/superpowers/plans/2026-06-06-search-rss-plateau-completion.md | rg -v 'T\\[B\\]D|TO\\[D\\]O|未\\[定\\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
git diff --check
```
