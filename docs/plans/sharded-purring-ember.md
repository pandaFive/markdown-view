# markdown-view: 軽量・高速 Markdown プレビューア実装プラン

## 1. 背景と目的

ローカルでMarkdownを執筆しながら、保存ごとにブラウザプレビューを即時更新できるCLIツールをRustで実装する。  
配布はシングルバイナリを前提とし、テンプレートや静的アセットを外部ファイルに依存させない。

## 2. スコープ

### 対象
- 単一Markdownファイルのライブプレビュー
- GFM相当の主要記法（表、タスクリスト、取り消し線、フェンスコード）
- TOC表示と見出しジャンプ
- WebSocketによる差し替え更新
- ライト/ダークテーマ切替

### 非対象
- 複数ファイルをまたぐ静的サイト生成
- Mermaidや数式などの高度拡張
- 認証付きリモート公開

## 3. 完了条件（Definition of Done）

- `cargo test` / `cargo clippy` / `cargo fmt --check` がすべてPass
- 200KB程度のMarkdownで、保存から画面反映まで体感1秒未満
- ファイル変更時にブラウザが自動更新され、スクロール位置を維持できる
- サーバーは `127.0.0.1` バインド固定で動作する
- 主要エラー（ファイルなし、権限不足、ポート競合）で利用者向けに明確なメッセージを返す

## 4. アーキテクチャ概要

```text
Editor -> ファイル保存 -> notify -> debounce(300ms)
  -> renderer (pulldown-cmark + syntect + TOC)
  -> tokio::broadcast
  -> WebSocket
  -> Browser DOM更新
```

起動フロー:
1. CLIで対象Markdownを受け取る
2. HTTPサーバーをローカル起動
3. 必要に応じてブラウザを自動オープン
4. ファイル監視イベントをレンダリングへ流し、WebSocketで配信

## 5. モジュール構成

```text
markdown-view/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── server.rs
│   ├── watcher.rs
│   ├── renderer.rs
│   ├── toc.rs
│   └── template.rs
└── tests/
    ├── cli_test.rs
    ├── renderer_test.rs
    ├── toc_test.rs
    └── integration_test.rs
```

## 6. 主要依存クレート

| クレート | 用途 |
|---|---|
| `clap` 4 | CLI引数解析 |
| `tokio` 1 | 非同期ランタイム |
| `axum` 0.8 | HTTP + WebSocket |
| `pulldown-cmark` 0.13 | Markdownパース |
| `syntect` 5 | コードハイライト |
| `notify` 8 / `notify-debouncer-mini` | ファイル監視 |
| `open` 5 | ブラウザ起動 |
| `anyhow` 1 | エラーハンドリング |
| `serde` / `serde_json` | WebSocketメッセージ |
| `ammonia` 4（任意） | raw HTMLサニタイズ |

## 7. CLI仕様

```bash
markdown-view README.md
markdown-view README.md -p 8080
markdown-view README.md --no-open
markdown-view README.md --dark
markdown-view README.md --theme "Solarized (dark)"
```

オプション方針:
- デフォルトポートは `3000`（使用中ならエラー終了）
- デフォルトで `127.0.0.1` のみ待受
- `--no-open` 指定時はURLを標準出力に表示

## 8. レンダリング/フロント仕様

- レイアウト: 左TOC + 右本文（モバイルではTOC折りたたみ）
- テーマ: CSS変数 + `prefers-color-scheme` + `data-theme`
- JS: WebSocket再接続（指数バックオフ）、スクロール位置保持、現在見出しの追跡
- テンプレート埋め込み: `include_str!` で外部ファイル依存を排除

## 9. セキュリティ要件

- `0.0.0.0` で待受しない（ローカル限定）
- 入力パスは `canonicalize()` で正規化し、読み取り不可時は即失敗
- raw HTMLはそのまま描画しない方針を基本とし、必要時はサニタイズを挟む
- 巨大ファイル対策としてサイズ上限を設ける（例: 5MB）
- エラー出力に機密情報（絶対パスや内部スタック詳細）を過剰に含めない

## 10. 実装順序（TDD）

### Phase 1: 初期化 + CLI
1. Cargo設定、最小エントリーポイント作成
2. `tests/cli_test.rs` を先に作成し失敗を確認
3. `src/cli.rs` 実装でテストをPassさせる

### Phase 2: レンダラー + TOC
1. `tests/renderer_test.rs` / `tests/toc_test.rs` を先に作成
2. 空入力、境界ケース、重複見出しを含めて失敗確認
3. `src/renderer.rs` / `src/toc.rs` 実装でPass

### Phase 3: HTTP + テンプレート
1. `src/template.rs` 実装
2. `src/server.rs` 実装（`/`, `/ws`, `/api/content`）
3. 統合テスト作成とPass確認

### Phase 4: 監視 + 配信
1. `src/watcher.rs` 実装（debounce + async bridge）
2. broadcast配信とWebSocket連携
3. 「保存 -> 再描画配信」の統合テストをPass

### Phase 5: 統合 + UX仕上げ
1. `src/main.rs` で全体統合
2. ブラウザ起動とログ出力整備
3. 手動E2Eで挙動最終確認

## 11. テスト観点（必須）

- 空入力: 空ファイルでもクラッシュしない
- 大量データ: 大きめファイルでもタイムアウトやOOMを起こしにくい
- エラー時: ファイル消失・権限不足・ポート競合の扱い
- 境界値: 見出し0件、同名見出し多数、極端に長い1行

## 12. リスクと対策

- 監視イベントの過剰発火: debounce調整 + 変更差分の簡易判定
- 初回ハイライト負荷: キャッシュまたは処理分割を検討
- WebSocket切断: 再接続戦略とフォールバック表示
- テーマ不整合: `prefers-color-scheme` と明示テーマ指定の優先順位を固定

## 13. 検証コマンド

```bash
cargo test
cargo clippy --all-targets --all-features
cargo fmt --check
cargo build --release

echo "# Hello\n\nTest **bold** ~~strike~~\n\n- [x] Done\n- [ ] Todo\n\n\`\`\`rust\nfn main() {}\n\`\`\`" > test.md
./target/release/markdown-view test.md
```
