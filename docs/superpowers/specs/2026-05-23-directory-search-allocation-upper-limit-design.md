# ディレクトリ検索 allocation 上限近傍計測 設計書

## 背景

`docs/todo/BACKLOG.md` の P2 には、ディレクトリ検索の allocation 削減を追加計測に基づいて再判断する項目が残っている。

前回の計測では、260 files / 約 7.8 MiB の代表 fixture で検索系 targeted tests と HTTP `/api/search` 経由の result-limit / full-scan を確認した。一方で、Backlog 自身が Done 判定に必要としている「複数ファイルに分散した 100 件 result-limit」「64 MiB 近傍 full-scan または byte-limit 近傍」「10 MiB 単一ファイル」「RSS plateau」は未確認のまま残っている。

今回の目的は、検索実装を変更せずに上限近傍の追加計測を行い、allocation 削減を今すぐ実装すべきか、YAGNI として Done 化できるか、または具体的な未達条件付きで P2 に残すかを判断することである。

## 目的

- Backlog に残っている未測定条件を実測で埋める。
- 64 MiB 近傍、10 MiB 単一ファイル、result-limit 100 件、RSS plateau の観測値を `BACKLOG.md` に残す。
- 計測結果に基づき、P2 項目を Done 化するか、未達条件を明確にして残すかを判断する。
- コード、API、UI、依存関係、セキュリティ境界を変更しない。

## 非目標

- 検索ロジックの最適化。
- `Cow<str>` 化、検索ブロック処理単位の変更、検索インデックス導入。
- `SearchResponse` JSON 形状の変更。
- `criterion` などの dev dependency 追加。
- UI 文言や表示の変更。
- Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約の変更。

## 設計

この作業は docs-only の計測調査として扱う。計測 fixture は `/tmp` 配下の一意ディレクトリに生成し、リポジトリには追加しない。永続変更は原則 `docs/todo/BACKLOG.md` の判断記録だけに限定する。

既存の `docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md` と `docs/superpowers/plans/2026-05-19-directory-search-allocation-measurement.md` は代表 fixture 計測の前提として残す。今回の設計は、その続きとして上限近傍の Done 判定条件を明文化する。

### 計測系列

1. focused test baseline
   - `cargo test search --all-targets --all-features` を warm 実行する。
   - `/usr/bin/time -v` で複数回実行し、elapsed と最大 RSS の範囲を記録する。
   - これは検索周辺の既存テストが成功することと、テストプロセスの RSS 傾向を見る補助であり、単独では Done 判定に使わない。

2. result-limit 100 件
   - 複数 Markdown ファイルに `needle` を分散させ、HTTP `/api/search?q=needle` が result-limit 100 件で打ち切られる fixture を作る。
   - `truncated=true`、`truncated_reasons=["result_limit"]`、`searched_files`、`searched_bytes`、レスポンスサイズ、elapsed、server RSS を記録する。
   - 1 ファイルだけで 100 件に到達する構成にはしない。複数ファイルに分散した場合の検索・結果生成を確認する。

3. 64 MiB 近傍 full-scan または byte-limit
   - `MAX_SEARCH_BYTES = 64 MiB` 近傍まで Markdown を読む fixture を作る。
   - `q=missingneedle` のような no-match query で full-scan または byte-limit 到達を確認する。
   - `searched_bytes` が上限近傍であること、`truncated` と `truncated_reasons` が期待どおりであること、反復後 server RSS が継続増加しないことを記録する。
   - fixture 自体が 64 MiB を超え、既存 byte-limit で約 64 MiB 読込後に打ち切られる場合は `truncated_reasons=["byte_limit"]` を期待する。上限内に収める場合は `truncated=false` を期待する。どちらを選んだかを `BACKLOG.md` に明記する。

4. 10 MiB 単一ファイル
   - `MAX_FILE_SIZE` 近傍だが超えない単一 Markdown ファイルを作る。
   - match あり、または match なしのどちらか一方だけを測る場合は、その理由を記録する。
   - Done 判定では、単一大ファイルでも実用外の遅延や server RSS 継続増がないことを確認する。

5. RSS plateau
   - result-limit、64 MiB 近傍、10 MiB 単一ファイルの各 HTTP 経路で、少なくとも複数回反復後の server RSS 系列を記録する。
   - `ps -o pid,rss,comm,args -C markdown-view` で対象 server を確認する。
   - docs には PID と RSS 値だけを残し、絶対パスや full args は残さない。
   - RSS が反復ごとに増え続け、plateau と判断できない場合は Done 化しない。

## 判断基準

Done 化できるのは、次をすべて満たす場合だけである。

- result-limit 100 件が複数ファイル分散 fixture で確認できている。
- 64 MiB 近傍 full-scan または byte-limit 近傍が確認できている。
- 10 MiB 単一ファイルが確認できている。
- HTTP status が 200 で、`SearchResponse` の必須 field が維持されている。
- elapsed とレスポンスサイズが個人向け localhost preview tool として実用範囲にある。
- server RSS が反復後に継続増加せず、plateau と判断できる。
- 検索ロジック、`SearchResponse` JSON、Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約を変更していない。
- fixture は `/tmp` 配下で生成し、repo に追加していない。

どれかを満たせない場合は、P2 項目を Done に移さない。未測定条件、観測されたリスク、または計測不能理由を具体的に `BACKLOG.md` に残す。

## エラー処理

計測中に HTTP 400、403、429、500、curl 失敗、JSON field 欠落、または対象 server の PID/RSS 判別不能が起きた場合は、検索実装を変更しない。まず計測条件を確認する。

- Host header が `127.0.0.1:<port>` になっているか。
- port が既存 server と衝突していないか。
- fixture path が `/tmp` の今回生成ディレクトリを指しているか。
- query が意図した result-limit / full-scan / byte-limit 経路に対応しているか。
- 既存の検索上限に到達しているか。

原因を切り分けられない場合は、計測不十分として P2 に残す。

## セキュリティ

計測用 Markdown は固定文字列で生成し、外部から取得した Markdown、検索結果、Issue、LLM 出力を fixture としてそのまま使わない。取得済みテキストを shell、SQL、policy、コードとして実行しない。

検索対象制限は迂回しない。`127.0.0.1` binding、Host/Origin 検証、path validation、hidden/生成物ディレクトリ除外、ファイルサイズ上限、HTML sanitize、CSP は変更しない。

ログや docs には、ローカルユーザー名、絶対パス、本文断片、full `ps args` を残さない。必要な情報は fixture 規模、検索統計、elapsed、RSS 系列に限定する。

## 受け入れ基準

- 上限近傍計測の目的、非目標、判断基準が明文化されている。
- result-limit 100 件、64 MiB 近傍、10 MiB 単一ファイル、RSS plateau の扱いが明確になっている。
- Done 化条件と Done 化しない条件が明確になっている。
- コード、API、UI、依存関係、セキュリティ境界を変更しない方針が明確になっている。
- 計測 fixture を repo 外に作り、取得テキストを実行しないセキュリティ方針が明記されている。
- `BACKLOG.md` に記録すべき測定値と残余リスクが明確になっている。

## 影響範囲

- `docs/superpowers/specs/2026-05-23-directory-search-allocation-upper-limit-design.md`
  - 今回の上限近傍計測設計を追加する。
- `docs/todo/BACKLOG.md`
  - 後続計測後に、P2 項目を Done 化するか、未達条件付きで残す。
- 一時 fixture
  - `/tmp` 配下に生成し、作業後に削除する。

`src/`、`tests/`、`Cargo.toml`、`Cargo.lock`、ブラウザ assets は変更しない。

## ロールバック

設計書追加 commit は docs-only なので、該当 commit を revert すれば元に戻せる。後続で `BACKLOG.md` を更新した場合も、docs commit の revert で復旧できる。

計測 fixture は `/tmp` 配下の一意ディレクトリなので、対象ディレクトリを確認して削除すれば永続影響は残らない。

## 残余リスク

- RSS や elapsed は OS、CPU、ディスクキャッシュ、ビルド種別、同時実行中の別 process の影響を受ける。
- `ps` による server RSS は request 後 snapshot であり、server process のリクエスト中 peak RSS ではない。後続再計測で peak が必要な場合は、対象 PID の短周期 sampling または server process を `/usr/bin/time -v` 配下で起動する。
- private 関数単位の allocation 分解は行わないため、問題が見えた場合も追加設計が必要になる。
- JSON 直列化では最終的に所有データが必要なため、局所的な `Cow<str>` 化が体感改善に効くとは限らない。
- 今回問題が見えなくても、検索上限や UI 要件を将来変える場合は再計測が必要になる。

## 見積もり

- 人間作業: 60-120 分。
- Codex/AI 支援: 25-60 分。

64 MiB 近傍 fixture の生成、HTTP 反復、RSS plateau 判断が主な変動要因である。

## 検証

設計書自体は docs-only なので、TDD ではなく文書検証で確認する。

```bash
rg -n "目的|非目標|受け入れ基準|セキュリティ|影響範囲|ロールバック|見積もり" docs/superpowers/specs/2026-05-23-directory-search-allocation-upper-limit-design.md
placeholder_matches="$(rg -n "T[B]D|TO[D]O|未[定]" docs/superpowers/specs/2026-05-23-directory-search-allocation-upper-limit-design.md | rg -v 'T\[B\]D|TO\[D\]O|未\[定\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
git diff --check
```
