# markdown-view

軽量・高速な Markdown プレビューア（Rust製）。
ファイルの変更を検知し、WebSocket 経由でブラウザにリアルタイム反映します。

## 特徴

- **ライブリロード** — ファイル保存時にブラウザが自動更新（WebSocket）
- **ディレクトリモード** — ディレクトリ指定でファイル一覧付きプレビュー
- **目次サイドバー** — 見出しから自動生成、スクロール追従
- **シンタックスハイライト** — syntect による多言語対応コードハイライト
- **ダークモード** — OS設定に自動追従 / `--dark` で強制切り替え
- **セキュア設計** — localhost限定バインド、XSS防止、CSPヘッダー
- **ゼロ設定** — 外部ファイル不要、単一バイナリで完結
- **複数同時起動** — 使用中ポートを避けて別ディレクトリのプレビューを並行起動可能

## インストール

```bash
cargo install --path .
```

または開発用にビルド:

```bash
cargo build --release
```

## 使い方

### 単一ファイルモード

```bash
markdown-view README.md
```

ブラウザが自動で開き、Markdown のプレビューが表示されます。
ファイルを編集・保存すると、ブラウザに即座に反映されます。

### ディレクトリモード

```bash
markdown-view docs/
```

ディレクトリ内の `.md` ファイルをサイドバーに一覧表示し、クリックで切り替えられます。
ディレクトリ内のファイル変更も自動検知されます。

- デフォルトで `README.md` を表示（なければアルファベット順で最初のファイル）
- サブディレクトリの `.md` ファイルも再帰的に列挙
- 隠しファイル/ディレクトリ（`.`開始）は除外

### オプション

```
引数:
  <PATH>                プレビューするMarkdownファイルまたはディレクトリのパス

オプション:
  -p, --port <PORT>     優先するHTTPサーバーのポート番号（使用中なら次の空きポートを探す） [デフォルト: 3000]
      --no-open         ブラウザの自動起動を無効にする
      --dark            ダークモードを強制する
      --theme <THEME>   シンタックスハイライトのテーマ名
  -h, --help            ヘルプを表示
  -V, --version         バージョンを表示
```

### 使用例

```bash
# ポートを指定して起動
markdown-view docs/guide.md --port 8080

# 2つのディレクトリを同時に起動（2つ目は空きポートへ自動フォールバック）
markdown-view docs/
markdown-view notes/

# ディレクトリを指定してプレビュー
markdown-view ./docs

# ダークモードで起動
markdown-view README.md --dark

# テーマを指定してブラウザ自動起動なし
markdown-view README.md --theme "base16-ocean.dark" --no-open

# 組み合わせ
markdown-view README.md --port 4000 --dark --theme "base16-mocha.dark"
```

## 開発

### 必要環境

- Rust 1.70+
- Node.js 20.11+ （E2Eテストと `./verify.sh` の型チェックステップに必要）

### セットアップ

```bash
npm ci   # E2E 依存（Playwright / TypeScript）を取得。`./verify.sh` 実行前に一度だけ必要
```

`node_modules/` が無い状態で `./verify.sh` を実行すると E2E 型チェックステップで停止する（対応: 上記 `npm ci` を実行）。Rust のみを扱う場合も `./verify.sh` は `npm ci` 済みを前提とするため、初回セットアップ時に必須。

### ビルド・テスト

```bash
# 一括検証（フォーマット・リント・テスト・E2E型チェック）
./verify.sh

# 個別コマンド
cargo fmt --all -- --check       # フォーマットチェック
cargo clippy --all-targets --all-features -- -D warnings  # リント
cargo test --all-targets --all-features   # 全テスト実行
npm run typecheck                # E2E テストの型チェック (tsc --noEmit)
npm run test:e2e                 # E2E テスト実行（Playwright、ブラウザ自動起動）
```

### アーキテクチャ

```
main.rs  ── CLI引数パース → バリデーション → サーバー起動
  │
  ├── cli.rs        CLIオプション定義（clap derive）
  ├── server.rs     公開ファサード（モジュール再エクスポート）
  │   ├── state.rs      サーバー状態とモード判定（AppState, AppMode, CanonicalPath）
  │   ├── routes.rs     axumルーター、HTTP/WebSocketハンドラ
  │   ├── files.rs      ファイル探索、検証、読み込み、描画
  │   ├── guards.rs     Host/Origin検証、CSPヘッダー構築
  │   ├── messages.rs   ブロードキャストメッセージ型、APIエラー型、ファイルサイズ定数
  │   └── websocket.rs  WebSocketセッション管理、変更通知ブロードキャスト
  ├── renderer.rs   Markdown→HTML変換（pulldown-cmark + syntectハイライト）
  ├── toc.rs        Markdown→目次HTML生成
  ├── template.rs   HTMLテンプレート（CSS/JS埋め込み、UpdateMessage型）
  └── watcher.rs    ファイル監視（notify + debouncer → tokioブリッジ）
```

### データフロー

1. **初期表示**: HTTP GET `/` → Markdown読み込み・変換 → フルHTML返却
2. **ライブリロード**: ファイル変更検知 → broadcast channel → WebSocket → ブラウザ更新
3. **API**:
   - GET `/api/content` → JSON（content + toc）、ディレクトリモードでは `?file=` で指定
   - GET `/api/files` → ディレクトリ内の `.md` ファイル一覧（単一ファイルモードでは空配列）

## セキュリティ

- **localhost限定**: `127.0.0.1` のみにバインド（外部アクセス不可）
- **DNS Rebinding防止**: Host/Origin ヘッダー検証
- **XSS防止**: raw HTML を完全除去（pulldown-cmark の HTML イベントを破棄）
- **リンクサニタイズ**: `http`/`https`/`mailto`/`tel` とローカルパスのみ許可
- **CSP/セキュリティヘッダー**: `X-Frame-Options: DENY`、`X-Content-Type-Options: nosniff`
- **ディレクトリトラバーサル防止**: canonicalize + starts_with でベースディレクトリ外アクセスを遮断
- **ファイルサイズ上限**: 10MB（TOCTOU対策の二段階チェック）

## ライセンス

MIT
