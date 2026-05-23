# ディレクトリ検索 many-match 早期停止 設計書

## 背景

`docs/todo/TODO.md` の Medium Priority には、ディレクトリ検索の 10MiB 近傍 many-match 経路を早期停止・処理単位見直しで抑制する項目がある。

2026-05-23 の上限近傍計測では、10MiB 近傍単一ファイルに `needle` が大量に含まれる経路で、HTTP `/api/search?q=needle` が `result_limit` に到達するにもかかわらず elapsed 約 36-38 秒、server RSS 約 0.9-1.3GiB まで増えた。no-match 経路は約 1.4 秒だったため、巨大単一ブロック内で全 match と context を生成してから外側で 100 件に切る処理単位が主な疑いである。

現行実装では `search_directory_with_limits_blocking()` がファイルごとに `extract_search_blocks()` を実行し、その後 `find_matches_for_file()` がファイル内の全 match を `Vec<SearchResultItem>` として返す。外側はその後で `limits.max_results` に到達したかを判定するため、1ファイル内に大量 match がある場合、必要な 100 件を超えて match/context 生成が続く。

## 目的

- `max_results` 到達後に同一ファイル内の match/context 生成を続けない。
- `SearchResponse` JSON、`truncated`、`truncated_reasons`、`searched_files`、`searched_bytes` の外部契約を維持する。
- 10MiB 近傍 many-match fixture の elapsed と server RSS を手元計測し、現状値からの改善を確認する。
- CI では性能値ではなく、結果予算を超えて生成しない構造的な回帰テストを固定する。

## 非目標

- 検索インデックスの導入。
- result-limit 用の `extract_search_blocks()` 逐次化。
- Markdown parser profile、検索対象ブロックの仕様変更。
- `SearchResponse` JSON 形状の変更。
- Host/Origin 検証、path validation、HTML sanitize、CSP、ファイルサイズ上限の変更。
- elapsed や RSS の固定閾値を CI に入れること。

## 設計

`search_directory_with_limits_blocking()` は、現在の `results.len()` と `limits.max_results` から残り件数を計算し、`find_matches_for_file()` に渡す。

`find_matches_for_file()` は `remaining_results: usize` を受け取り、生成済み件数が予算に達したら現在の match ループとブロックループを終了する。`remaining_results == 0` の場合は追加結果を生成せず、空の `Vec<SearchResultItem>` を返す。各 result の `before` / `current` / `after` は検索 UI 用 snippet として上限内に切り詰め、巨大単一文の重複 clone で JSON 応答が肥大化しないようにする。

外側の `search_directory_with_limits_blocking()` は従来どおり、`results.len() >= limits.max_results` になった時点で `SearchTruncationReason::Result` を付与して検索を終了する。これにより `truncated=true` と `truncated_reasons=["result_limit"]` の公開契約は維持される。

今回の変更では result-limit 到達前の通常検索について `extract_search_blocks()` は全ブロック抽出のまま残す。stale cancellation 時だけは block 抽出中と file 内 match loop 中にも cancellation を伝播し、蓄積済み結果も含めて古い検索の部分結果を返さず中断する。実装後の 10MiB many-match 計測で改善が不十分な場合は、次段として検索処理の逐次化を別設計で扱う。

## コンポーネント

- `src/server/files/search.rs`
  - `search_directory_with_limits_blocking()` がファイル内検索へ残り結果予算を渡す。
  - `find_matches_for_file()` が予算を超えた match/context 生成を止め、巨大 context を bounded snippet として返す。
  - 既存の `SearchLimits`、`SearchStats`、`SearchTruncationReason` は変更しない。

## データフロー

1. `/api/search` が query を受け取り、既存どおり directory search を開始する。
2. `search_directory_with_limits_blocking()` が Markdown ファイルを列挙し、ファイルを読み込む。
3. 読込後、`results.len()` から残り件数を計算する。
4. `extract_search_blocks_until_cancelled()` が検索対象ブロックを抽出し、stale 化された検索では中断する。
5. `find_matches_for_file()` が残り件数まで match/context snippet を生成し、stale 化された検索では中断する。
6. 外側が結果を `results` に追加し、上限到達時に `result_limit` を記録して終了する。

## エラー処理

query 正規化、長すぎる query の 400、読込失敗ファイルの skip、file/byte/result limit の扱いは既存どおり維持する。残り件数予算は内部制御だけなので、HTTP status や error body は増やさない。

予算 0 で `find_matches_for_file()` が呼ばれた場合は空結果を返す。これは異常ではなく、呼び出し側がすでに `max_results` に達している状態として扱う。

stale cancellation で block 抽出または file 内 match loop を中断した場合は、蓄積済み結果を破棄して古い検索の部分結果を返さず、query や本文断片を含まない debug log で観測可能にする。公開 JSON には cancellation 専用 field を追加しない。

## テスト

CI に入れるテストは性能閾値ではなく構造確認にする。

- `find_matches_for_file()` が `remaining_results` を超えて `SearchResultItem` を生成しないことを unit test で固定する。
- 1つの巨大ブロック、または多数文に大量の `needle` がある場合でも、指定した予算件数だけ返ることを確認する。
- 巨大単一文でも `before` / `current` / `after` が bounded snippet になり、serialized JSON が肥大化しないことを確認する。
- block 抽出中と file 内 match loop 中の stale cancellation が中断され、ログで観測できることを確認する。
- `search_directory_with_limits_blocking()` は `max_results` 到達時に `SearchTruncationReason::Result` を付け、`results.len() == max_results` を維持することを既存テストの補強で確認する。
- テスト名は既存方針どおり日本語にする。

## 手元計測

10MiB 近傍 many-match fixture を `/tmp` 配下に生成し、HTTP `/api/search?q=needle` を複数回実行する。記録する値は次のとおり。

- 実行環境。
- fixture のファイル数、合計サイズ、match 分布。
- HTTP status。
- `truncated`、`truncated_reasons`、`results.len()`、`searched_files`、`searched_bytes`。
- elapsed。
- server RSS の反復後 snapshot。

比較対象は `TODO.md` に記録済みの現状値である elapsed 約 36-38 秒、server RSS 約 0.9-1.3GiB とする。絶対値は OS、CPU、disk cache、ビルド状態で変動するため、`./verify.sh` の合否条件にはしない。

## セキュリティ

許可 Host からの `/api/search` で可用性低下を起こせる点を今回の主リスクとして扱う。localhost-only 前提でも、巨大 many-match Markdown が開かれた状態で検索すると preview server の応答性を落とせるため、result-limit 到達後の不要な work を抑える。

`127.0.0.1` binding、Host/Origin 検証、path validation、hidden/生成物ディレクトリ除外、ファイルサイズ上限、HTML sanitize、CSP、`SearchResponse` JSON 形状は変更しない。検索キャンセルは既存の stale generation を維持しつつ、block 抽出と file 内 match loop に追加伝播する。

計測 fixture は固定文字列で生成する。外部文書、Issue、検索結果、LLM 出力をそのまま shell、SQL、policy、コードとして実行しない。計測記録には絶対パス、本文断片、full process args を残さない。

## 受け入れ基準

- `find_matches_for_file()` が残り結果予算を受け取り、予算到達時に同一ファイル内探索を止める。
- 巨大単一文でも検索結果 context が上限内の snippet になり、100 件返っても JSON 応答サイズが bounded になる。
- stale cancellation が block 抽出中と file 内 match loop 中に効き、本文や query を漏らさない log で観測できる。
- `/api/search` の JSON 契約と result/file/byte limit の意味が変わらない。
- result-limit 到達時の `truncated_reasons=["result_limit"]` が維持される。
- 10MiB 近傍 many-match の手元計測で、現状の 36-38 秒、約 0.9-1.3GiB RSS と比べて改善傾向が確認できる。
- 改善が不十分な場合、`TODO.md` または完了報告に次段の `extract_search_blocks()` 途中停止／逐次化を残す。
- `./verify.sh` が通過する。

## 影響範囲

- `src/server/files/search.rs`
  - 検索結果予算をファイル内 match 生成へ渡す。
  - 早期停止の unit test を追加または更新する。
- `tests/integration/search.rs`
  - 必要に応じて `/api/search` の result-limit 契約を補強する。
- `docs/todo/TODO.md`
  - 実装と計測後、Medium 項目を完了扱いにするか、残余リスク付きで更新する。

`SearchResponse` の公開 JSON、UI、テンプレート、ブラウザ assets、設定ファイル、依存関係は変更しない。

## ロールバック

実装 commit を revert すれば元に戻せる。外部 API、永続データ形式、設定形式を変えないため、データ移行やユーザー操作は不要である。

計測 fixture は `/tmp` 配下に生成するため、対象ディレクトリを削除すれば永続影響は残らない。

## 残余リスク

- `extract_search_blocks()` は今回も全ブロックを作るため、巨大 Markdown の parsing と block allocation は残る。
- 検索 context は bounded snippet 化済みだが、result 100 件分の snippet 生成では巨大文に対する走査コストが残る可能性がある。
- elapsed と RSS は環境差が大きく、手元計測だけで全環境の性能を保証できない。
- 今回の変更で改善が不十分な場合は、ブロック抽出の途中停止または逐次 search iterator 化を別設計で検討する。

## 見積もり

- 人間作業: 60-120 分。
- Codex/AI 支援: 25-60 分。

実装自体は小さいが、10MiB 近傍 fixture の計測と、改善が不十分な場合の切り分けで変動する。

## 検証

設計書自体は docs-only なので、TDD ではなく文書検証で確認する。後続実装では TDD を使い、構造テストを先に追加する。

```bash
rg -n "目的|非目標|受け入れ基準|セキュリティ|影響範囲|ロールバック|見積もり" docs/superpowers/specs/2026-05-23-search-many-match-budget-design.md
placeholder_matches="$(rg -n -P 'T[B]D|TO[D]O(?!\.md| Issues)|未[定]' docs/superpowers/specs/2026-05-23-search-many-match-budget-design.md | rg -v 'T\[B\]D|TO\[D\]O|未\[定\]' || :)"
test -z "$placeholder_matches" || { printf '%s\n' "$placeholder_matches"; exit 1; }
git diff --check
```
