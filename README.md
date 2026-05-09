# markdown-view

Markdown workspace for local reading, notes, search, and live preview（Rust製）。
Markdown ファイルの閲覧、横断検索、引用メモ、ファイルツリー、ライブ更新を localhost 上の単一バイナリで扱います。

## 特徴

- **ライブリロード** — ファイル保存時にブラウザが自動更新（WebSocket）
- **ディレクトリモード** — ディレクトリ指定でファイルツリー付き workspace を表示
- **横断検索** — workspace 内の Markdown をサーバー側の上限付き検索で横断
- **引用メモ** — 選択範囲への引用リンクと sidecar メモを保存
- **目次サイドバー** — 見出しから自動生成、スクロール追従
- **シンタックスハイライト** — syntect による多言語対応コードハイライト
- **ダークモード** — OS設定に自動追従 / `--dark` で強制切り替え
- **セキュア設計** — localhost限定バインド、Host/Origin検証、XSS防止、CSPヘッダー
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

### ディレクトリ監視と Linux inotify 上限

ディレクトリモードでは、監視リソースを節約しライブ更新の対象と表示対象を一致させるため、`.git`、`node_modules`、`target`、隠しディレクトリを既定でプレビュー一覧・検索・直接表示・監視対象から除外します。通常の Markdown workspace で作成した新しいサブディレクトリは起動後も自動で監視対象に追加されます。

Linux で「監視対象が多すぎるため監視を開始できません」と表示された場合は、inotify の `fs.inotify.max_user_watches` 上限に到達している可能性があります。現在値は `sysctl fs.inotify.max_user_watches` で確認できます。上限を変更する場合は、利用環境の方針に従って一時変更または永続設定を行ってください。

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
./verify.sh --e2e                # 上記に加えて Playwright E2E も実行

# 個別コマンド
cargo fmt --all -- --check       # フォーマットチェック
cargo clippy --all-targets --all-features -- -D warnings  # リント
cargo test --all-targets --all-features   # 全テスト実行
npm run typecheck                # E2E テストの型チェック (tsc --noEmit)
npm run test:e2e                 # E2E テスト実行（Playwright、ブラウザ自動起動）
```

### アーキテクチャ

主要な分割境界は `src/server/`、`src/renderer/`、`src/template/` です。

```
src/
  main.rs            CLI起動、サーバー初期化、ブラウザ起動
  lib.rs             ライブラリ公開境界
  cli.rs             clap による CLI オプション定義
  watcher/           notify + debouncer から tokio へ変更通知を橋渡し
  server.rs          server モジュールの公開ファサード
  server/
    state.rs         AppState / AppMode / CanonicalPath
    routes.rs        axum ルーター、HTTP API、WebSocket upgrade
    session.rs       WebSocket セッションと close code 送信
    broadcast.rs     ファイル変更通知の broadcast message 構築
    watch.rs         watcher からの変更イベント処理
    guards.rs        Host / Origin 検証、CSP / セキュリティヘッダー
    messages.rs      API / WebSocket メッセージ型、ファイルサイズ上限
    log_path.rs      ログ出力用パスの相対化
    files/
      catalog.rs     ディレクトリ内 Markdown 一覧
      content.rs     Markdown 読み込みと HTML / TOC 生成
      resolve.rs     パス解決と traversal 防止
      search.rs      ファイル検索
      memo*.rs       メモ sidecar の保存、名前生成、ファイル I/O
  renderer/          Markdown -> HTML 変換
    render.rs        pulldown-cmark event の描画
    state.rs         レンダリング状態
    line.rs          ソース行属性
    security.rs      URL / HTML sanitize
    highlight.rs     syntect によるコードハイライト
    toc.rs           Markdown -> 目次 HTML
  template/          HTML ページ、UpdateMessage、ファイルツリー、埋め込み assets
    assets/js/       ブラウザ側の更新、選択、メモ、サイドバー、WebSocket
    assets/css/      ページ / サイドバー / メモ / オーバーレイの CSS
```

### データフロー

1. **初期表示**: HTTP GET `/` → Markdown読み込み・変換 → フルHTML返却
2. **ライブリロード**: ファイル変更検知 → broadcast channel → WebSocket → ブラウザ更新
3. **API**:
   - GET `/api/content` → JSON（content + toc）、ディレクトリモードでは `?file=` で指定
   - GET `/api/search` → ディレクトリ内 Markdown の検索結果 JSON（単一ファイルモードでは空結果）
   - GET/PUT `/api/memo` → 対象 Markdown の sidecar メモ取得・保存
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
