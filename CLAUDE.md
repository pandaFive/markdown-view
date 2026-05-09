# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## プロジェクト概要

Markdown ファイルの閲覧、横断検索、引用メモ、ファイルツリー、ライブ更新を扱う localhost 専用 Markdown workspace（Rust製）。
ファイル変更を検知して WebSocket 経由でブラウザに即座に反映し、メモ sidecar と検索 API を同じ workspace 境界内で扱う。

## ビルド・テスト・検証コマンド

```bash
# 初回セットアップ（`./verify.sh` と E2E 実行に必要、node_modules を作る）
npm ci                                    # Node 20.11+ が前提（package.json engines で宣言）

# 一括検証（フォーマット・リント・テスト・E2E型チェック）
./verify.sh                               # npm ci 済みが前提。未済なら明示エラーで停止
./verify.sh --e2e                         # 上記に加えて Playwright E2E も実行

# 個別コマンド
cargo fmt --all -- --check       # フォーマットチェック
cargo clippy --all-targets --all-features -- -D warnings  # リント
cargo test --all-targets --all-features   # 全テスト実行
npm run typecheck                        # E2Eテストの型チェック (tsc --noEmit)
npm run test:e2e                          # E2Eテスト実行 (Playwright)

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
  │   ├── state.rs      AppState / AppMode / CanonicalPath
  │   ├── routes.rs     axumルーター、HTTP/WebSocket adapter
  │   ├── service.rs    ページ/本文/メモ/検索の application service
  │   ├── files/        ファイル探索、検証、読み込み、メモ保存、検索
  │   ├── guards.rs     Host/Origin検証、CSP/セキュリティヘッダー
  │   ├── messages.rs   API / WebSocket メッセージ型
  │   ├── broadcast.rs  変更通知ブロードキャスト
  │   ├── session.rs    WebSocket セッション管理
  │   ├── watch.rs      watcher からの変更イベント処理
  │   └── log_path.rs   ログ出力用パスの相対化
  ├── renderer/     Markdown描画モジュール
  │   ├── render.rs     pulldown-cmark event の描画
  │   ├── state.rs      レンダリング状態
  │   ├── line.rs       ソース行属性
  │   ├── security.rs   URL / HTML sanitize
  │   ├── highlight.rs  syntect コードハイライト
  │   └── toc.rs        TOC HTML 生成
  ├── template/     HTMLページ、UpdateMessage、ファイルツリー、埋め込み assets
  │   ├── page.rs
  │   ├── message.rs
  │   ├── tree.rs
  │   └── assets/
  └── watcher/      notify + debouncer → tokio bridge
```

### データフロー

1. **初期表示**: HTTP GET `/` → `service::load_page` → `load_route_update` / `load_route_memo` → `render_page`（フルHTML）
2. **ライブリロード**: notify検知 → `notify_update` → broadcast channel → WebSocket → クライアントJS
3. **API**: GET `/api/content` → `service::load_content` → JSON（`UpdateMessage { content, toc }`）
4. **メモAPI**: GET/PUT `/api/memo` → `service::load_memo` / `service::save_memo` → `MemoResponse`

### 重要な設計判断

- **127.0.0.1のみバインド** + Host/Originヘッダー検証でDNS Rebinding防止
- **raw HTML完全除去**: pulldown-cmarkの`Event::Html`/`Event::InlineHtml`を破棄してXSS防止
- **HTTP adapter / application service分離**: `routes.rs` はHost検証・extractor・HTTP応答変換に寄せ、対象解決以降の手順は `service.rs` に置く
- **TOCTOU対策**: `read_bytes_with_limit`で二段階サイズチェック（metadata + take）
- **メモatomic保存**: 同一ディレクトリ内tmpへ `create_new` + `write_all` + `flush` + `sync_data` 後、renameで最終sidecarへ差し替える
- **メモ保存先再検証**: rename直前にfinal/tmpの親ディレクトリ一致とsymlink component不在を再検証し、差し替えraceを検出する
- **親ディレクトリsync**: rename後の親ディレクトリsyncはbest-effort。失敗しても応答は巻き戻せないため、クラッシュ耐性劣化として `error!` ログに残す
- **CSS/JS完全埋め込み**: 外部ファイル不要、単一HTMLで完結
- **notifyはstd::thread**: notifyがsync APIのため、mpscチャネルでtokioにブリッジ
- **見出し情報共有**: `render_document` は本文 HTML と TOC を同じ `HeadingInfo` から生成する。互換 API の `extract_headings` と検索用 Markdown profile は用途別に別走査する。
- 個人使用前提でも、外部公開 API の互換性破壊は `major change` 扱いにしろ。性能向上のために互換性を壊す場合も、通常変更として紛れ込ませず明示的に扱え

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
- `src/server/files/` 内テスト — ファイル解決、トラバーサル防止、サイズ制限、メモI/O
- `src/server/messages.rs` 内テスト — BroadcastMessage直列化
- `src/server/broadcast.rs` 内テスト — notify_update、遅延回復
- `tests/e2e/*.spec.ts` — Playwright E2Eテスト（TypeScript strict、`npm run test:e2e`で実行）

テスト名は日本語で記述する。

## コード規約

- Create and work on feature/fix branches in git worktrees by default; use normal branch checkouts only for trivial, single-session changes.
- コメント・コミットメッセージは日本語
- `tracing::info!/warn!/error!` マクロで構造化ログ出力（初期化失敗時のみ `eprintln!` フォールバック）
- syntect/pulldown-cmarkの静的リソースは`OnceLock`でlazy初期化
- 公開関数に`///`ドキュメントコメントを付与
- E2Eテストは TypeScript strict で記述し、`tsc --noEmit`（`verify.sh`内）で型検査する
