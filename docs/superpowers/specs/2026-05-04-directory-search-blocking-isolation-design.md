# ディレクトリ検索 blocking 隔離 設計書

## 背景

`/api/search` はディレクトリモードで Markdown ファイルを横断検索する。PR #121 で、結果数・検索対象ファイル数・総読込 byte 数による打ち切りは API と UI に明示されるようになった。一方、検索処理そのものは async 経路の中で、同期ディレクトリ走査、複数ファイル読込、Markdown パース、検索一致抽出を実行する。

巨大 workspace や連続検索では、この重い処理が Tokio worker を圧迫し、本文表示、メモ API、WebSocket などの応答性に影響する可能性が残る。今回の目的は、検索リクエスト単位で重い処理を `spawn_blocking` に隔離し、既存 API と UI の挙動を維持したまま応答性リスクを下げること。

## 目的

- ディレクトリ検索の重い処理を Tokio worker から外す。
- `search_directory()` の async API を維持する。
- ディレクトリ走査、ファイル読込、Markdown パース、検索一致抽出をリクエスト単位の blocking task 内で実行する。
- PR #121 の `truncated` / `truncated_reasons` / `limits` / `searched_bytes` の契約を維持する。
- 単一ファイルモードの `/api/search` は従来通り空結果を返す。

## 非目標

- 検索インデックスの導入。
- サーバ側キャンセル、古い検索の中断、世代管理。
- 検索結果コンテキストの `Cow<str>` 化。
- Markdown パース回数の削減。
- 検索 UI の変更。
- watcher や memo の同期 I/O 改善。

## 受け入れ条件

- `search_directory()` は async 関数のまま呼び出せる。
- ディレクトリ走査、ファイル読込、Markdown パース、検索一致抽出は `spawn_blocking` 内で実行される。
- `SearchResponse` の JSON 形状は変わらない。
- 通常検索、結果数上限、ファイル数上限、総読込 byte 上限の挙動は PR #121 と同じ。
- 単一ファイルモードの `/api/search` は空結果かつ非打ち切りを返す。
- blocking task の panic は `error!`、cancel など panic 以外の join error は `warn!` に記録され、上位では 500 JSON に変換される。
- base 配下、hidden、Markdown 拡張子、symlink 差し替えの検証は各ファイル読込直前に維持される。
- `./verify.sh` が通る。

## アーキテクチャ

`src/server/files/search.rs` に同期検索コアを追加し、既存の async API から呼び出す。

### async wrapper

`pub(in crate::server) async fn search_directory(base_dir: &Path, raw_query: &str)` は既存の公開境界として残す。

この関数は `base_dir.to_path_buf()` と `raw_query.to_owned()` を作り、`tokio::task::spawn_blocking` に渡す。blocking task 内では同期検索コアを呼び、`JoinError` は `std::io::Error` に変換して返す。

### 同期検索コア

`fn search_directory_blocking(base_dir: &Path, raw_query: &str) -> std::io::Result<SearchResponse>` を追加する。この関数は `SearchLimits::default()` を使って、limit 注入可能な同期実装へ委譲する。

`fn search_directory_with_limits_blocking(base_dir: &Path, raw_query: &str, limits: SearchLimits) -> std::io::Result<SearchResponse>` に、現在の `search_directory_with_limits` の実処理を移す。テストで小さい上限を注入できるように、この関数は `#[cfg(test)]` から直接使える範囲に置く。

処理順は既存の検索予算の意味を保つため変更しない。

1. query を trim し、空なら `SearchResponse::empty` を返す。
2. `list_markdown_files_with_limit(base_dir, limits.max_files + 1)` で候補を取得する。
3. 候補数が上限を超えた場合は `file_limit` を記録する。
4. 各候補を `resolve_file(base_dir, relative)` で最終検証する。
5. 同期読込ヘルパーで Markdown 本文を読む。
6. 総読込 byte 上限を超える場合は `byte_limit` を記録して停止する。
7. `extract_search_blocks()` と `find_matches_for_file()` で検索する。
8. 結果数上限に達した場合は `result_limit` を記録して停止する。

### 同期読込ヘルパー

`fn read_markdown_with_limit_blocking(path: &Path) -> std::io::Result<String>` を検索専用に追加する。

この関数は既存 `read_markdown_with_limit()` と同じ上限契約に揃える。`MAX_FILE_SIZE + 1` まで読み、上限超過時は Markdown パースへ進まずエラーを返す。検索用の同期コア内だけで使い、初期実装では共通ファイル読込 API の大きな再設計はしない。

## データフロー

`service::search()` は現在と同じく `search_directory(base_dir, &query).await` を呼ぶ。

`search_directory()` は blocking task を起動する。

blocking task 内では、`list_markdown_files_with_limit()`、`resolve_file()`、`read_markdown_with_limit_blocking()`、`extract_search_blocks()`、`find_matches_for_file()` を順に実行する。

`SearchResponse`、`SearchLimits`、`SearchStats`、`SearchTruncationReason` の構造は変更しない。

## エラー処理

`spawn_blocking` の `JoinError` は `search.rs` 内で `std::io::Error` に変換する。

- `JoinError::is_panic()` は `tracing::error!` に記録する。
- panic 以外の join error は `tracing::warn!` に記録する。
- レスポンスへ panic 詳細、ローカル絶対パス、内部状態は出さない。

上位の `service::search()` は既存通り、`std::io::Error` を「ディレクトリ検索に失敗しました」の 500 JSON に変換する。

## セキュリティ

blocking 化しても、各ファイル読込直前に `resolve_file(base_dir, relative)` を通す。これにより、base 外、hidden、非 Markdown、非通常ファイル、symlink 差し替えの検証を維持する。

検索対象外になった本文や読込失敗した本文はログにもレスポンスにも出さない。ログは既存方針に合わせ、相対パスとエラー概要に留める。

Host middleware と `/api/search` の不正 Host 拒否経路は変更しない。

## テスト方針

Rust 側:

- `search_directory()` 経由の async テストを残し、wrapper が同期コアと同じ結果を返すことを固定する。
- `search_directory_with_limits_blocking()` の同期テストで、通常検索、結果数上限、ファイル数上限、総読込 byte 上限を検証する。
- `JoinError` 変換を小さな helper に切り出せる場合は、panic と cancel のログ種別を単体テストしやすい形にする。
- 既存の単一ファイルモード `/api/search` 空結果テストを維持する。
- 既存の `/api/search` JSON 互換テストと Host 拒否テストを維持する。

E2E 側:

- UI 変更はないため新規 E2E は追加しない。
- 既存 `tests/e2e/document_search.spec.ts` は、検索 UI と truncated 表示の回帰確認として維持する。

## 影響範囲

- `src/server/files/search.rs`
  - async wrapper、同期検索コア、同期読込ヘルパー、join error 変換を追加する。
- `src/server/service.rs`
  - 呼び出し形は原則変更しない。必要ならログ文脈だけ確認する。
- `tests/integration_test.rs`
  - 既存 `/api/search` 互換テストを維持する。
- `tests/e2e/document_search.spec.ts`
  - 既存テストを維持する。UI 変更がない限り追加しない。

## ロールバック方針

`search_directory()` を元の async 実装へ戻し、追加した blocking wrapper、同期検索コア、同期読込ヘルパー、join error helper、関連テストを削除する。

`SearchResponse` の追加フィールドや UI 警告は PR #121 の成果なので戻さない。今回の変更は検索処理の実行境界に限定する。

## 残余リスク

- `spawn_blocking` に移しても、古い検索のキャンセルや世代管理は行わない。連続検索時、古い検索処理は予算到達または完走まで進む。
- blocking thread pool 側の占有は残るため、極端な連続検索には別途キャンセルや専用検索タスクの検討余地がある。
- 同期読込ヘルパーは検索専用に追加するため、既存 async 読込ヘルパーとの契約重複が残る。
- Markdown パース回数と検索結果文字列の allocation は今回削減しない。

## 見積もり

- 人間作業: 1.5-2.5 時間。
- Codex/AI 支援: 30-60 分。
