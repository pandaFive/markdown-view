# AppState Arc Lifecycle Design

## Goal

`AppState` の所有権を `Arc<AppState>` に一本化し、共有状態のライフサイクルとメモ用ファイルシステム注入の契約を明確にする。

現状は `AppState: Clone` でありながら、実運用では `Arc<AppState>` として route、watcher、broadcast、WebSocket に配布している。さらに `memo_fs` は内部で `Arc<dyn MemoFs>` を持ち、テストだけ `with_memo_fs(mut self)` で差し替えるため、生成後に差し替えできるように見える API と、実際には `Arc<AppState>` を共有する API が噛み合っていない。

この設計では、共有は外側の `Arc<AppState>` が担当し、差し替え可能な依存は `AppState` 生成時に固定する。

## Non-Goals

- route 構造、middleware 構造、watcher 起動順序は変更しない。
- memo の保存仕様、読み込み仕様、ファイル名規則、永続化形式は変更しない。
- `MemoFs` の新しい実装は追加しない。
- broadcast メッセージ仕様や WebSocket 仕様は変更しない。
- `RenderState` など、別項目の型整理には踏み込まない。

## Design

`AppState` から `Clone` derive を削除し、共有状態として clone する単位を `Arc<AppState>` に限定する。`AppState` 内部の `memo_fs` は引き続き `Arc<dyn MemoFs>` として保持するが、差し替えは constructor でのみ行う。

production 経路には Tokio 実装を使う constructor を置く。

```rust
pub fn new_with_tokio_memo_fs(
    mode: AppMode,
    dark_mode: bool,
    theme: Option<String>,
    tx: broadcast::Sender<BroadcastMessage>,
) -> Self
```

テストや依存注入が必要な経路には、`MemoFs` を明示的に渡す constructor を置く。

```rust
pub(crate) fn new(
    mode: AppMode,
    dark_mode: bool,
    theme: Option<String>,
    tx: broadcast::Sender<BroadcastMessage>,
    memo_fs: Arc<dyn MemoFs>,
) -> Self
```

既存の `AppState::new(...)` 呼び出しは、production では `new_with_tokio_memo_fs(...)` へ、テストでは明示注入 constructor へ移す。移行後、`with_memo_fs` は削除する。

builder は導入しない。現状の引数数では builder が実質的な複雑さを増やすため、KISS/YAGNI の観点から constructor 2 種に留める。

## Components

- `src/server/state.rs`
  - `AppState` の `Clone` derive を削除する。
  - Tokio 用 constructor と `MemoFs` 注入 constructor を定義する。
  - `with_memo_fs` を削除する。
- `src/main.rs`
  - production の状態生成を Tokio 用 constructor へ移す。
- `src/server/files/test_support.rs`
  - テスト用 `MemoFs` を constructor 注入する helper に変更する。
- `src/server/service.rs`
  - `with_memo_fs` 利用箇所を constructor 注入へ変更する。
- `src/server/broadcast.rs`、`src/server/watch.rs`、`tests/integration_test.rs`
  - `AppState::new` のシグネチャ変更に追従する。

## Data Flow

起動時またはテスト setup 時に `AppMode`、表示設定、broadcast sender、`MemoFs` を `AppState` に渡す。生成された `AppState` は呼び出し元で `Arc::new(...)` され、router、watcher、session、broadcast helper に `Arc::clone` で渡される。

生成後に `memo_fs` を差し替える経路は存在しない。これにより、route や service が参照している `AppState` の memo backend は、生成時点から終了時点まで固定される。

## Error Handling

新しい runtime error は追加しない。`MemoFs` 注入は型で保証されるため、constructor は失敗しない。

既存の memo 読み書きエラー、ファイル検証、パス検証、broadcast エラー処理は変更しない。

## Security Considerations

状態生成後に `MemoFs` を差し替えられるように見える test-only API を削除し、memo I/O backend を生成時固定にする。これにより、共有済み `AppState` の保存先や読み込み元が後から変わる設計上の余地を閉じる。

この変更はパス検証、HTML sanitization、Host/Origin validation、CSP、localhost binding を弱めない。外部入力や検索結果、Markdown 本文、memo ファイル内容は引き続き信頼しない前提で扱う。

## Acceptance Criteria

- `AppState` は `Clone` を実装しない。
- `AppState` の共有は `Arc<AppState>` に統一される。
- `MemoFs` は `AppState` 生成時に注入できる。
- `with_memo_fs` は削除される。
- production 経路は Tokio 実装の `MemoFs` を使う。
- 既存の route、service、broadcast、watcher、integration test helper が新 API でビルドされる。
- `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features`、`./verify.sh` が通る。

## Rollback

API 変更が想定以上に広がる場合は、`MemoFs` 注入 constructor の追加だけを残し、`Clone` 削除と `with_memo_fs` 削除を別フェーズへ戻す。ただし最終目標は `Arc<AppState>` 一本化であり、縮退は一時的な切り分けに限る。

実装時は設計 doc とコード変更を別コミットにし、必要ならコード変更だけを revert できるようにする。
