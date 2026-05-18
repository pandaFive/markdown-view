# Directory Search Cancellation Boundary 完了記録

## 概要

ディレクトリ検索で同一ブラウザページの新しい検索が始まったとき、古い検索をサーバ側の安全な区切りで協調的に早期終了できるようにした。

この文書は実行済み計画の記録であり、再実行用の手順書ではない。旧案の単一 `AtomicU64` / 引数なし `begin_search_generation()` は採用していない。

## 最終実装

- `AppState` は検索 client ID ごとの世代 map を持つ。
- `begin_search_generation(client_id, sequence) -> SearchGeneration` は指定 client の世代を進める。sequence 付き client では古い request や sequence なしの後続 request は stale handle を返し、世代を進めない。
- 世代 map は最大 128 client に制限する。上限到達時は最古 idle entry を優先して削除し、全 entry が active の場合は新規 client の世代発行を拒否する。
- ブラウザはページ単位の不透明な検索 client ID と発行順 sequence を `X-Markdown-View-Search-Client` / `X-Markdown-View-Search-Sequence` ヘッダーで送る。
- ルート層は検索 client ID を ASCII 英数字、`-`、`_`、最大 64 byte に制限し、不正値は未指定として扱う。
- client ID が未指定または不正な場合は、互換性のためサーバ側キャンセルなしで検索する。
- `SearchCancellation` は blocking 検索コアへ渡され、列挙中、ファイル処理前、読込直後、検索結果反映後の境界で stale を確認する。
- キャンセルは API エラーにせず、その時点までの `SearchResponse` を返す。UI は既存の generation check で古いレスポンスを破棄する。

## 変更範囲

- サーバ状態: client 単位の検索世代 store と bounded LRU。
- ルート/サービス: 検索 client ID ヘッダー検証と `SearchCancellation` の接続。
- 検索コア: cancellable catalog 列挙とファイル単位の協調キャンセル。
- ブラウザ JS: ページ単位 search client ID の生成と検索 API への送信。
- テスト: state/service/search/catalog/E2E で client 分離、連続検索、列挙中キャンセル、ヘッダー継続性を固定。
- BACKLOG: キャンセル境界を完了扱いにし、allocation 削減だけを残件化。

## セキュリティ

- 検索世代値はプロセス内状態に閉じ、HTTP response や DOM へ公開しない。
- HTTP ヘッダーで受け取るのは不透明な client ID と発行順 sequence のみで、内部世代値は公開しない。
- path validation、hidden path 除外、Markdown 拡張子確認、通常ファイル確認、symlink 再検証は維持する。
- Host middleware、WebSocket Origin 検証、CSP、HTML sanitization、`innerHTML` sink 制限は変更しない。
- キャンセル判断のログには query 全文、絶対パス、本文断片を出さない。

## 検証

実装時に次を確認した。

```bash
cargo test --lib server::state
cargo test --lib server::files::search
cargo test --lib server::service
cargo test --lib server::routes
cargo test --test integration_test search
npx playwright test tests/e2e/document_search.spec.ts
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
./verify.sh
```

## ロールバック

検索世代 store、検索 client ID ヘッダー処理、ブラウザ側ヘッダー送信、`SearchCancellation`、cancellable catalog 列挙、関連テストを revert すれば戻せる。

`SearchResponse` の JSON 形状は変えていないため、ロールバック時にデータ移行や API 互換対応は不要。

## 残余リスク

- 協調キャンセルなので、単一の `read_dir` 呼び出し中、ファイル読込中、Markdown パース中、JSON serialize 中の強制中断はしない。
- API client が検索 client ID を送らない場合はサーバ側キャンセルなしで検索する。
- 128 client すべてが active の状態では、新規 client を拒否する。既存 active 検索を別 client から stale 化できないようにするための可用性保護であり、通常のブラウザ UI では到達しにくい。
- 検索結果コンテキストの allocation 削減は後続 BACKLOG として残す。
