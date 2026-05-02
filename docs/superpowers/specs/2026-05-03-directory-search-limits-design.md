# ディレクトリ検索負荷制御の明示化 設計書

## 背景

`/api/search` はディレクトリモードで Markdown ファイルを横断検索する。現在は検索結果が 100 件に達すると処理を止めるが、レスポンスには打ち切り理由が出ない。検索対象ファイル数は `list_markdown_files()` の 1000 件上限に依存し、検索リクエスト全体の総読込量にも上限がない。そのため、巨大ワークスペースでは「一部だけ検索された」状態が API/UI から分かりにくい。

今回の最優先事項は、検索が上限で打ち切られた事実と理由を API と UI に明示すること。

## 目的

- `/api/search` の silent truncation をなくす。
- 結果数、検索対象ファイル数、総読込バイト数の上限を検索処理の明示的な予算として扱う。
- 上限到達時に JSON レスポンスへ打ち切り情報を返す。
- ブラウザの検索パネルに「一部のみ表示」の最小メッセージを出す。
- 既存の検索結果表示と単一ファイルモードの挙動を維持する。

## 非目標

- 検索インデックスの導入。
- 検索結果の完全網羅保証。
- サーバ側キャンセル、古い検索の破棄、専用 worker pool 化。
- Markdown パース回数の削減。
- ブラウザ JS の大規模分割。
- 検索 UI で byte 数や上限値の詳細を表示すること。

## 受け入れ条件

- 結果数 100 件に達した検索は `truncated: true` と `result_limit` を返す。
- 検索対象ファイル数 1000 件に達した検索は `truncated: true` と `file_limit` を返す。
- 総読込 64 MiB に達した検索は `truncated: true` と `byte_limit` を返す。
- 複数上限に達した場合、`truncated_reasons` は重複なしで複数理由を返す。
- 通常検索では `truncated: false` と空の `truncated_reasons` を返す。
- UI は truncated response を受けたとき、検索結果一覧の先頭に「上限により一部のみ表示しています。」を出す。
- 古い fixture や新フィールドを持たないレスポンスでも JS は従来通り動く。
- 単一ファイルモードの `/api/search` は従来通り空結果を返し、新フィールドも非打ち切りとして整合する。

## API 変更

既存の `SearchResponse` フィールドは維持する。

- `query`
- `results`
- `searched_files`
- `skipped_files`

以下を追加する。

```json
{
  "truncated": true,
  "truncated_reasons": ["result_limit", "byte_limit"],
  "limits": {
    "max_results": 100,
    "max_files": 1000,
    "max_bytes": 67108864
  },
  "searched_bytes": 12345678
}
```

`truncated_reasons` は安定した文字列 enum として扱う。

- `result_limit`
- `file_limit`
- `byte_limit`

`searched_files` は本文読込まで進んだファイル数を表す。`skipped_files` は従来通り、解決失敗や読込失敗で検索できなかったファイル数を表す。`searched_bytes` は検索対象として受理した Markdown 本文の byte 長合計を表す。

## 検索予算

`src/server/files/search.rs` に `SearchLimits` と `SearchStats` という内部構造を追加する。

初期値:

- `max_results`: 100
- `max_files`: 1000
- `max_bytes`: 64 MiB

検索処理は次の順序で予算を適用する。

1. `list_markdown_files(base_dir)` で候補を取得する。
2. 候補が `max_files` 以上なら `file_limit` を記録し、処理対象を `max_files` 件に制限する。
3. 各ファイルは従来通り `resolve_file()` で最終検証してから読む。
4. 読み込んだ Markdown の byte 長を確認する。
5. そのファイルを含めると `searched_bytes` が `max_bytes` を超える場合、そのファイルは検索対象に含めず `byte_limit` を記録して停止する。
6. 検索結果が `max_results` に達したら `result_limit` を記録して停止する。

64 MiB を超過させるファイルを検索対象に含めない。`read_markdown_with_limit()` が返した `String` は一時的にメモリへ載るが、予算超過時は Markdown パースへ進めず破棄する。これにより、総読込予算の意味をレスポンス上の `searched_bytes` と一致させる。

## UI 変更

`src/template/assets/js/content.js` のディレクトリ検索レスポンス処理で、以下を後方互換的に保持する。

- `currentDirectoryTruncated`
- `currentDirectoryTruncatedReasons`

`data.truncated === true` または `truncated_reasons` が空でない場合、検索結果一覧の先頭に非操作の状態行を追加する。表示文言は最小に留める。

```text
上限により一部のみ表示しています。
```

詳細な上限値、byte 数、ファイル数は初回 UI では表示しない。

## エラー処理とセキュリティ

- `resolve_file()` による base 配下、hidden、Markdown 拡張子、symlink 境界の検証は維持する。
- 解決失敗や読込失敗は従来通り `skipped_files` に加算し、warn ログへ出す。
- レスポンスには絶対パス、ローカル環境の詳細、エラー本文を含めない。
- Host middleware と `/api/search` の拒否挙動は変更しない。
- 検索対象外になった巨大ファイルの本文はログやレスポンスに出さない。

## テスト方針

Rust 側:

- 通常検索で `truncated: false`、`truncated_reasons: []`、`searched_bytes` が返ること。
- 結果数上限到達で `result_limit` が返ること。
- ファイル数上限到達で `file_limit` が返ること。
- 総読込 byte 上限到達で `byte_limit` が返り、超過ファイルが検索対象に含まれないこと。
- 単一ファイルモードが空結果かつ非打ち切りになること。
- `/api/search` の Host 拒否テストが維持されること。

E2E 側:

- `truncated: true` を返す mocked `/api/search` に対して、検索結果一覧に最小メッセージが表示されること。
- 新フィールドを持たない既存 mocked response でも結果表示が壊れないこと。

## 影響範囲

- `src/server/files/search.rs`: 検索予算、統計、レスポンス追加フィールド。
- `src/server/service.rs`: 単一ファイルモードの空レスポンス整合。
- `src/template/assets/js/content.js`: truncated response の保持と最小表示。
- `tests/integration_test.rs` または `src/server/service.rs` tests: API/検索予算の検証。
- `tests/e2e/document_search.spec.ts`: UI の最小表示検証。

## ロールバック方針

既存フィールドは維持するため、UI 表示だけを戻しても API は後方互換のまま残せる。完全に戻す場合は、追加した `SearchLimits`、`SearchStats`、`SearchResponse` 追加フィールド、UI の状態行、関連テストを削除する。検索結果の既存構造とルーティングは変更しないため、ロールバック範囲は検索機能内に限定される。

## 残余リスク

- `list_markdown_files()` 自体が 1000 件で truncate するため、列挙段階の正確な総候補数は初回設計では分からない。
- 予算超過判定のため、byte 上限を超えるファイルも `read_markdown_with_limit()` の単一ファイル上限までは一時的に読み込まれる。
- Markdown パースは引き続き async handler の流れで実行されるため、Tokio worker への負荷対策は後続課題として残る。
- サーバ側キャンセルは入れないため、クライアントが新しい検索を開始しても古い検索処理は完走または予算到達まで進む。
