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
  -p, --port <PORT>     HTTPサーバーのポート番号 [デフォルト: 3000]
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

### ビルド・テスト

```bash
# 一括検証（フォーマット・リント・テスト）
./verify.sh

# 個別コマンド
cargo fmt --all -- --check       # フォーマットチェック
cargo clippy --all-targets --all-features -- -D warnings  # リント
cargo test --all-targets --all-features   # 全テスト実行
```

### アーキテクチャ

```
main.rs  ── CLI引数パース → バリデーション → サーバー起動
  │
  ├── cli.rs        CLIオプション定義（clap derive）
  ├── server.rs     axumルーター、HTTPハンドラ、WebSocket処理、ファイル読み込み
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
