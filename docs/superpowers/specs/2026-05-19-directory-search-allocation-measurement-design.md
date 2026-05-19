# ディレクトリ検索 allocation 計測 設計書

## 背景

`docs/todo/BACKLOG.md` の P2 には、ディレクトリ検索の allocation 削減を追加計測に基づいて再判断する項目が残っている。

現状のディレクトリ検索は、`spawn_blocking` 隔離、検索結果数・対象ファイル数・総読込 byte 数・query 長の上限、クライアント単位の検索キャンセル境界、全体同時実行上限を持つ。安全境界と負荷上限は既に明示されているため、次に行うべきことは最適化そのものではなく、allocation 削減が実用上必要かどうかを測る判断材料を作ること。

## 目的

- ディレクトリ検索の allocation 削減候補について、実装前に測定可能な判断材料を作る。
- 検索コア寄りの処理と HTTP `/api/search` 経由の実動作を分けて観測する。
- 計測手順、代表 fixture、観測値、判断を docs に残す。
- 計測結果に基づき、最適化する、最適化しない、または保留する判断を明文化する。
- 依存追加なしで実施できる手順を優先する。

## 非目標

- 検索ロジックの変更。
- `SearchResponse` の JSON 形状変更。
- `Cow<str>` 化、検索ブロック処理単位の見直し、検索インデックス導入。
- `criterion` などの dev dependency 追加。
- UI の見た目や文言変更。
- Host/Origin 検証、path validation、HTML sanitize、CSP、検索キャンセル境界、検索上限契約の変更。

## 設計

この作業は docs-only の調査タスクとして扱う。コードへ最適化を入れず、必要なら一時的なローカルコマンドで fixture を作って測定する。永続化するのは、計測手順、代表 fixture の作り方、結果、判断基準、残余リスクに限定する。

計測は次の 2 層に分ける。

1. 検索コア寄りの傾向
   - `src/server/files/search.rs` の `extract_search_blocks()`、`find_matches_for_file()`、検索結果コンテキスト生成の影響を間接的に見る。
   - 関数は private のため、正式な bench harness は追加しない。
   - 検索系 targeted tests を `/usr/bin/time -v` で囲み、実行時間と最大 RSS の傾向を記録する。`/usr/bin/time -v` がない場合、shell の `time` は elapsed 補助に限定し、RSS は `ps` など別手段で記録する。
2. HTTP 経由の実動作
   - 一時ディレクトリに小・中・大の Markdown fixture を作る。
   - `cargo run -- <dir> --port <port>` で localhost サーバを起動する。
   - `curl --fail-with-body /api/search?q=...` を複数回実行し、HTTP status、レスポンス時間、サーバプロセス RSS 系列、JSON の `searched_files`、`searched_bytes`、`truncated`、`truncated_reasons` を記録する。
   - `q=needle` の result-limit 経路と、`q=absentneedle` の full-scan 経路を分けて観測する。

`hyperfine` が利用可能なら反復実行の補助として使ってよい。ただし必須ツールにはしない。`/usr/bin/time -v` がない環境では、shell の `time`、`ps`、`cargo test`、`cargo run`、`curl` に落とす。RSS を測れない場合は、elapsed だけで最適化不要とは判断せず、計測不十分として保留する。

## 判断基準

計測対象と完了基準は、測定前に明示する。代表 fixture だけを測った場合は、その範囲での所見として扱い、100 件結果上限、1000 ファイル上限、64 MiB 総読込上限、10 MiB 単一ファイル上限すべてを検証したとは書かない。

現状の検索上限近傍を含む計測で明確な悪化が見えない場合だけ、最適化しない判断にする。特に、複数ファイルに分散した 100 件 result-limit、64 MiB 近傍 full-scan または byte-limit 近傍、10 MiB 単一ファイルの範囲でレスポンス時間と RSS 系列が実用範囲に収まり、RSS が継続増加せず plateau するなら、`Cow<str>` 化や処理単位変更は YAGNI とする。

次のいずれかが観測された場合だけ、後続の最適化タスクを検討する。

- 大きくない fixture でも `/api/search` のレスポンス時間が体感上問題になる。
- `searched_bytes` に対して RSS 増加が不自然に大きい、または反復実行で plateau せず増え続ける。
- 結果数上限 100 件付近で `before/current/after` や `file` の重複文字列生成が支配的と判断できる。
- 検索コア寄りの targeted tests と HTTP 経由の両方で同じボトルネック傾向が見える。

判断を保留する場合は、保留理由と追加で必要な計測を `BACKLOG.md` に短く残す。HTTP status、JSON 形状、RSS 系列、result-limit / full-scan のどれかが欠けている場合も保留する。

## エラー処理

計測補助ツールがない場合は代替コマンドへ落とす。利用できないツールを理由に作業を失敗扱いにしない。

計測中に `/api/search` が 400、429、500 を返した場合、または必須 JSON field が欠ける場合は、まず query、Host、検索対象ディレクトリ、既存の検索上限到達を確認する。検索機能の実装変更は行わず、計測条件の問題として切り分け、判断は保留する。

数値は環境依存として扱い、OS、ビルド種別、fixture 内容、実行コマンド、反復回数をセットで記録する。単一回の数値だけで構造変更を決めない。

## セキュリティ

計測 fixture は一時ディレクトリまたは repo 外の作業領域に作る。外部から取得した Markdown、検索結果、Issue、LLM 出力を fixture としてそのまま使わない。必要な内容は固定文字列で生成し、取得済みテキストをコード、shell、SQL、policy として実行しない。

検索対象制限は迂回しない。`127.0.0.1` binding、Host 検証、path validation、hidden/生成物ディレクトリ除外、ファイルサイズ上限、HTML sanitize、CSP 境界は変更しない。

ログや docs には絶対パス、ローカルユーザー名、本文断片を不用意に残さない。必要な場合は相対的な説明や要約値に留める。

## 受け入れ基準

- 計測手順が依存追加なしで再実行できる形で文書化されている。
- 検索コア寄りと HTTP 経由の両方について、何を観測するかが明確になっている。
- fixture の規模と意図が文書化されている。
- HTTP status、必須 JSON field、elapsed、サーバ RSS 系列の扱いが明確になっている。
- RSS を測れない場合や上限近傍を測れていない場合に、最適化不要として Done 化しない方針が明確になっている。
- 観測値をどう判断するかが明確になっている。
- 最適化する、最適化しない、または保留する判断を `BACKLOG.md` または関連 docs に残す方針がある。
- コード、API、UI、セキュリティ境界を変更しない。
- docs-only 変更として、プレースホルダ、矛盾、曖昧な未決事項が残っていない。

## 影響範囲

- `docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md`
  - 本設計を追加する。
- 後続 plan
  - 実際の計測コマンド、作業順序、結果記録先を定義する。
- `docs/todo/BACKLOG.md`
  - 計測後に、項目を Done 化するか、保留理由付きで残すか、具体的な後続最適化候補へ分割する。

## ロールバック

この設計は docs-only なので、設計書追加 commit を revert すれば元に戻せる。計測 fixture は一時ディレクトリに作る前提のため、リポジトリへの永続影響は残さない。

後続 plan で `BACKLOG.md` を更新した場合も、コード変更を伴わないため、該当 docs commit の revert で復旧できる。

## 残余リスク

- プロセス単位の RSS や実行時間は OS、CPU、ディスクキャッシュ、ビルド種別の影響を受ける。
- private 関数へ直接 bench harness を追加しないため、検索コア内の allocation 箇所を完全には分解できない。
- `String` 生成の削減余地があっても、JSON 直列化では最終的に所有データが必要になるため、局所最適化が API 体感に効くとは限らない。
- 計測で問題が見えない場合でも、将来の検索要件や上限変更で再検討が必要になる可能性はある。

## 見積もり

- 人間作業: 45-75 分。
- Codex/AI 支援: 15-35 分。

## 検証

設計書自体は docs-only なので、TDD ではなく文書検証で確認する。

```bash
placeholder_matches="$(rg -n "T[B]D|TO[D]O|未[定]" docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md | rg -v 'T\[B\]D|TO\[D\]O|未\[定\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
rg -n "目的|非目標|受け入れ基準|セキュリティ|影響範囲|ロールバック|見積もり" docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md
git diff -- docs/superpowers/specs/2026-05-19-directory-search-allocation-measurement-design.md
```

実装計画フェーズでは、計測作業後に少なくとも `git diff` でコード変更がないことを確認する。必要に応じて検索系 targeted tests を実行する。
