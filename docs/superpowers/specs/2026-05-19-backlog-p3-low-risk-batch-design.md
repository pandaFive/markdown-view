# BACKLOG P3 低リスク判断込みバッチ設計

## 背景

`docs/todo/BACKLOG.md` の P3 には、長期改善・低緊急の候補が残っている。今回の対象は、そのうち低リスクで独立した小変更2件と、実装しない判断を明確にすべき観測性項目1件に絞る。

P3 全体には TypeScript 化のようなビルド方針を変える候補もあるが、Node 依存追加は設計判断が重く、今回の小粒バッチとは分けて扱う。

## ゴール

- サイドバーのディレクトリ名 fallback を `"Documents"` から `"ドキュメント"` に変更する。
- `src/server/files/catalog.rs` の相対パス文字列化で、`Vec<String>` を作って `join("/")` する一時 allocation を避ける。
- WS Host middleware bypass 兆候のメトリクス化は、現時点では実装しない判断を `BACKLOG.md` に追記する。
- 実装した P3 項目の完了根拠を `BACKLOG.md` に残す。
- Host/Origin 検証、CSP、HTML sanitize、path validation、除外ディレクトリ、symlink/canonicalize 検証を弱めない。

## 非ゴール

- i18n 基盤の導入。
- UI 全体の文言整理。
- ファイル一覧、検索、除外ルール、canonicalize 再検証の仕様変更。
- WS Host/Origin 検証ロジックの変更。
- メトリクス基盤、カウンタ、依存 crate の追加。
- インラインブラウザ JS の TypeScript 化。

## 設計

### サイドバー fallback

`src/server/service.rs` の `sidebar_directory_name()` は、ディレクトリモードで `file_name()` が取得でき、かつ UTF-8 文字列として空でない場合はその名前を返す。取得できない場合だけ fallback を返す。

今回の変更では、この fallback を `"Documents"` から `"ドキュメント"` に変更する。通常のディレクトリ名が取得できる場合の表示は変えない。

fallback 文字列は private 定数へ切り出し、unit test で固定する。通常ディレクトリ名を使う既存経路も、temp directory を使った unit test で固定する。

### catalog 相対パス文字列化

`src/server/files/catalog.rs` では、Markdown ファイルの表示用相対パスを `/` 区切りの文字列に変換している。現状は `components().map(...).collect::<Vec<_>>().join("/")` で、ファイルごとに `Vec` を確保する。

この処理を private helper に切り出す。

```text
relative_path_to_slash_string(relative: &Path) -> String
```

helper は `relative.components()` を1回走査し、2要素目以降の前に `/` を `push` する。各 component は既存と同じく `component.as_os_str().to_string_lossy()` で文字列化する。これにより非 UTF-8 path の lossy 表現契約は維持しつつ、中間 `Vec` を避ける。

変更するのは文字列構築だけであり、走査対象、sort、件数上限、除外ルール、canonicalize 再検証、symlink handling は変更しない。

### WS Host bypass メトリクス判断

`src/server/guards.rs` は Host middleware 後段に到達した Host 系 `WsOriginRejection` を `error!` ログとして出力し、`ws_rejection_class` と `host_recheck_anomaly` の構造化 field も持っている。

個人向け localhost ツールという現状では、この監査ログで異常検知の最低限の信号は足りる。メトリクス基盤やカウンタを追加すると、依存・状態・検証範囲が増える一方、現時点の運用要件に対する利益が薄い。

そのため今回メトリクス実装は行わない。`BACKLOG.md` の P3 項目は未完了のまま残し、「実運用で継続集計が必要になった場合だけ再検討する」「現時点では既存の `error!` ログと構造化 field を根拠に YAGNI とする」と追記する。

## 受け入れ条件

- fallback 経路のサイドバー名が `"ドキュメント"` になる。
- 通常のディレクトリ名が取得できる場合は、その名前を表示する。
- `catalog.rs` の相対パス文字列化が中間 `Vec` を作らない helper 経由になる。
- Markdown ファイル一覧の出力順、件数上限、除外ルール、相対パスの `/` 区切り契約が変わらない。
- WS Host bypass メトリクス項目は未完了のまま残り、実装しない判断理由が明記される。
- 実装した2項目の完了根拠が `BACKLOG.md` に残る。
- `./verify.sh` が通る。

## テスト方針

コード変更は小さいが、挙動固定のため unit test を追加または更新する。

- `sidebar_directory_name()` の fallback が `"ドキュメント"` になることを固定する。
- 通常のディレクトリ名が取得できる場合は fallback せず、その名前を使うことを確認する。
- `relative_path_to_slash_string()` がネストした path を `/` 区切りにすることを確認する。
- 既存の catalog listing tests で出力互換を確認する。

実装フェーズでは次を実行する。

```bash
cargo test --lib server::service
cargo test --lib server::files::tests::catalog
cargo test --lib relative_path_to_slash_string
./verify.sh
```

`BACKLOG.md` 更新は docs/config 相当の変更として、TDD ではなく文書検証を行う。

```bash
rg -n "Documents|ドキュメント|メトリクス|ws_rejection_class|host_recheck_anomaly" docs/todo/BACKLOG.md src/server/service.rs src/server/files/catalog.rs
rg -n "TBD|TODO|未定" docs/superpowers/specs/2026-05-19-backlog-p3-low-risk-batch-design.md docs/todo/BACKLOG.md
```

## セキュリティ考慮

`catalog.rs` の変更では、未信頼の filesystem path を表示用文字列に変換する処理だけを変更する。base 配下検証、hidden/excluded directory 除外、Markdown 拡張子判定、symlink/canonicalize 再検証は変更しない。ログ出力や HTML 反映の経路も変更しない。

サイドバー fallback は文言変更のみであり、path validation や HTML sanitize 境界には触れない。実ディレクトリ名の表示 escape は既存 template 側の契約を維持する。

WS Host bypass メトリクスは実装しないため、Host/Origin 検証の実装、拒否応答、監査ログの既存信号を弱めない。外部入力由来の Host/Origin 値は引き続き未信頼入力として扱い、ログ用 sentinel と構造化 field の既存方針を維持する。

## 影響範囲

- `src/server/service.rs`
  - サイドバー fallback 文言と関連 unit test。
- `src/server/files/catalog.rs`
  - 相対パス文字列化 helper と関連 unit test。
- `docs/todo/BACKLOG.md`
  - 実装した P3 項目の完了根拠、WS Host bypass メトリクス項目の判断追記。

依存的に、サイドバー表示を確認する template / integration / E2E tests と、ファイル一覧を使う directory/search 系 tests が回帰検知の対象になる。外部 HTTP API、WebSocket payload、CSP、Host/Origin 検証、memo、renderer の契約は変更しない。

## ロールバック

実装コミットを revert すれば、サイドバー fallback、相対パス文字列化、BACKLOG 記述を元に戻せる。データ移行、設定変更、ユーザー操作は不要。

## 見積もり

- 人間作業: 30〜45分。
- Codex/AI 支援: 15〜25分。

主な変動要因は、private 定数の fallback test と既存 catalog tests の配置に合わせて追加テストを書く時間。

## 残留リスク

- `catalog.rs` の helper は allocation を減らすが、`to_string_lossy()` による component 単位の必要な文字列化コストは残る。
- fallback 文言変更は日本語 UI には自然だが、英語環境の利用者には表示が変わる。
- WS Host bypass 兆候の継続集計は未実装のままなので、将来 localhost 外の本格運用や継続監視が必要になった場合は再設計が必要になる。
