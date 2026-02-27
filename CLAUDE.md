# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## プロジェクト概要

Markdownファイルをブラウザでリアルタイムプレビューする軽量CLIツール（Rust製）。
ファイル変更を検知してWebSocket経由でブラウザに即座に反映する。

## ビルド・テスト・検証コマンド

```bash
# 一括検証（フォーマット・リント・テスト）
./verify.sh

# 個別コマンド
cargo fmt --all -- --check       # フォーマットチェック
cargo clippy --all-targets --all-features -- -D warnings  # リント
cargo test --all-targets --all-features   # 全テスト実行

# 単一テスト実行
cargo test test_見出しにidが付与される     # テスト名で指定
cargo test --test renderer_test           # テストファイル単位
cargo test server::tests::               # モジュール単位

# 実行
cargo run -- README.md                   # 基本起動
cargo run -- README.md --port 8080 --dark --theme "base16-ocean.dark"
```

## アーキテクチャ

```
main.rs  ── CLI引数パース → バリデーション → サーバー起動
  │
  ├── cli.rs        CLIオプション定義（clap derive）
  ├── server.rs     axumルーター、HTTPハンドラ、WebSocket処理、ファイル読み込み
  ├── renderer.rs   Markdown→HTML変換（pulldown-cmark + syntectハイライト）
  ├── toc.rs        Markdown→目次HTML生成
  ├── template.rs   HTMLテンプレート（CSS/JS埋め込み、UpdateMessage型）
  └── watcher.rs    ファイル監視（notify + debouncer → tokio bridge）
```

### データフロー

1. **初期表示**: HTTP GET `/` → `read_and_render_file` → `render_page`（フルHTML）
2. **ライブリロード**: notify検知 → `notify_update` → broadcast channel → WebSocket → クライアントJS
3. **API**: GET `/api/content` → JSON（`UpdateMessage { content, toc }`）

### 重要な設計判断

- **127.0.0.1のみバインド** + Host/Originヘッダー検証でDNS Rebinding防止
- **raw HTML完全除去**: pulldown-cmarkの`Event::Html`/`Event::InlineHtml`を破棄してXSS防止
- **TOCTOU対策**: `read_markdown_with_limit`で二段階サイズチェック（metadata + take）
- **CSS/JS完全埋め込み**: 外部ファイル不要、単一HTMLで完結
- **notifyはstd::thread**: notifyがsync APIのため、mpscチャネルでtokioにブリッジ
- **見出しパースが2回実行される**: `slugify`/`generate_unique_id`/`extract_headings`は共有済みだが、`render_markdown`と`generate_toc`で別々にpulldown-cmarkパースが走る（既知のトレードオフ）

## セキュリティレイヤー

- CSPヘッダー（`script-src 'sha256-...'`、`style-src 'sha256-...'`、`frame-ancestors 'none'`）
- `X-Content-Type-Options: nosniff`、`X-Frame-Options: DENY`
- `sanitize_href`: 安全なスキーム（http/https/mailto/tel）とローカルパスのみ許可
- `is_trusted_host`: localhost/127.0.0.0/8/::1のみ信頼
- ファイルサイズ上限: 10MB（`MAX_FILE_SIZE`）

## テスト構成

- `tests/cli_test.rs` — CLI引数パース（clap の `try_parse_from` 使用）
- `tests/renderer_test.rs` — Markdown変換、XSSサニタイズ、コードハイライト
- `tests/toc_test.rs` — 目次生成、ネスト、重複ID
- `tests/integration_test.rs` — HTTP/WebSocket統合テスト（実サーバー起動）
- `src/server.rs` 内テスト — Host/Origin検証ユニットテスト

テスト名は日本語で記述する。

## コード規約

- コメント・コミットメッセージは日本語
- `tracing::info!/warn!/error!` マクロで構造化ログ出力（初期化失敗時のみ `eprintln!` フォールバック）
- syntect/pulldown-cmarkの静的リソースは`OnceLock`でlazy初期化
- 公開関数に`///`ドキュメントコメントを付与
