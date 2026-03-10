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
  ├── server.rs     公開ファサード（モジュール再エクスポート）
  │   ├── state.rs      サーバー状態とモード判定（AppState, AppMode, CanonicalPath）
  │   ├── routes.rs     axumルーター、HTTP/WebSocketハンドラ
  │   ├── files.rs      ファイル探索、検証、読み込み、描画、ファイルサイズ定数
  │   ├── guards.rs     Host/Origin検証、CSPヘッダー構築
  │   ├── messages.rs   ブロードキャストメッセージ型、APIエラー型
  │   └── websocket.rs  WebSocketセッション管理、変更通知ブロードキャスト
  ├── renderer/     Markdown描画モジュール
  │   ├── mod.rs        Markdown→HTML変換（pulldown-cmark + syntectハイライト）
  │   └── toc.rs        Markdown→目次HTML生成
  ├── template/     HTMLテンプレートモジュール
  │   ├── mod.rs        公開API再エクスポート
  │   ├── page.rs       ページレンダリング
  │   ├── message.rs    UpdateMessage型、エラーJSON生成
  │   ├── tree.rs       ファイルツリーHTML生成
  │   └── assets.rs     CSS/JSバンドル、CSPハッシュ生成
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
- `src/server/guards.rs` 内テスト — Host/Origin検証ユニットテスト
- `src/server/state.rs` 内テスト — AppMode構築・バリデーション
- `src/server/files.rs` 内テスト — ファイル解決、トラバーサル防止、サイズ制限
- `src/server/messages.rs` 内テスト — BroadcastMessage直列化
- `src/server/websocket.rs` 内テスト — notify_update、遅延回復

テスト名は日本語で記述する。

## コード規約

- コメント・コミットメッセージは日本語
- `tracing::info!/warn!/error!` マクロで構造化ログ出力（初期化失敗時のみ `eprintln!` フォールバック）
- syntect/pulldown-cmarkの静的リソースは`OnceLock`でlazy初期化
- 公開関数に`///`ドキュメントコメントを付与
