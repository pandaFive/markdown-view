# async 経路の同期 I/O 境界整理 設計

**作成日**: 2026-05-04
**対象ファイル**: `src/server/files/catalog.rs`, `src/server/files/resolve.rs`, `src/server/files/search.rs`, `src/server/files/memo.rs`, `src/server/service.rs`, `src/watcher/strategy.rs`

## 背景

`docs/todo/TODO.md` の Medium Priority に、async ハンドラ内の同期 I/O と watcher callback 内の `canonicalize` 散在が残っている。

現状は `service::list_files()` で `list_markdown_files()` を `spawn_blocking` に隔離している一方、`resolve.rs` のデフォルトファイル解決やファイル一覧同梱、`search.rs` の検索対象列挙、`memo.rs` のメモ保存先 symlink 検査には同期 I/O が残る。さらに `watcher::strategy` は notify callback 側で `path.canonicalize()` と `base.canonicalize()` をイベントごとに呼び、削除イベントでは lexical fallback に頼る。

今回の目的は、HTTP/API の async 経路から同期ディスク走査を外し、起動時に検証済みの `CanonicalPath` を catalog と watcher に引き回すことで、応答性とセキュリティ境界の読みやすさを改善すること。

## 目的

- async HTTP/API 経路で、同期ディレクトリ走査や同期 metadata 検査を直接実行しない。
- `AppMode` が持つ起動時 canonical base を catalog と watcher の境界へ渡し、再帰中の `base_dir.canonicalize()` 繰り返しをなくす。
- watcher callback のディレクトリモードでは、イベントパスの存在を前提にしない lexical 判定を主経路にし、イベントごとの `canonicalize()` を最小化する。
- base 外 symlink、hidden component、非 Markdown、非通常ファイルの拒否仕様を維持する。
- 外部 API の JSON 形状、UI、CLI 引数は変更しない。

## 非目的

- `AppState` の Arc 二重ラップ解消。
- `MemoFs` / `AppState::with_memo_fs` の builder 化。
- watcher の除外パターンや ENOSPC ユーザー文言追加。
- 検索インデックス、検索キャンセル、検索世代管理の導入。
- Markdown レンダリング仕様、検索結果形式、メモ sidecar 命名規則の変更。
- Host / Origin / CSP など routing security policy の変更。

## 受け入れ条件

- ディレクトリモードのファイル一覧、デフォルトファイル解決、ファイル一覧同梱、検索対象列挙は `spawn_blocking` 経由で同期走査を実行する。
- async handler 内のメモ保存先 symlink component 検査は `tokio::fs::symlink_metadata` を使う。
- `catalog` の再帰処理は canonical base を引数で受け、再帰中に `base_dir.canonicalize()` を呼ばない。
- `watcher::strategy` のディレクトリモードは canonical base を前提にした lexical base 判定と相対化を使い、削除済み base 配下 `.md` イベントを安全に扱える。
- base 外、hidden、非 Markdown、base 外 symlink、symlink cycle の既存拒否・スキップ仕様を維持する。
- `cargo test --all-targets --all-features` と `./verify.sh` が通る。

## 推奨アプローチ

`CanonicalPath` を「起動時に検証済みの実パス境界」として広げる。

`server::files::catalog` には canonical base を受ける内部 API を追加する。既存の `list_markdown_files(&Path)` は互換用に残し、入口で一度だけ canonicalize して新 API に委譲する。`service` や `resolve` など `AppMode` にアクセスできる経路では、`AppMode::directory_canonical()` から canonical base を取り出して新 API を使う。

HTTP/API から同期走査を呼ぶ箇所は、`spawn_blocking` に閉じる小 helper を用意して呼び出す。既存 `service::list_files()` の join error 変換を基準に、panic は `error!`、panic 以外は `warn!` とし、上位には既存文言の 500 JSON を返す。

`memo.rs` の symlink component 検査は async 化する。`first_unsafe_memo_path_component()` を async 関数へ変更し、`std::fs::symlink_metadata` を `tokio::fs::symlink_metadata` に置き換える。呼び出し元の保存・削除前検査は `.await` する。

`watcher::strategy` は既存の `WatchStrategy::Directory { base_dir: CanonicalPath }` を活かす。ディレクトリイベントでは canonicalize に依存せず、`normalize_lexical_path()` と canonical base からの lexical `strip_prefix` を中心に base 配下判定、hidden 判定、重複排除を行う。最終読み込み時の `resolve_change_target()` 再検証は防御層として残す。

## 代替案

### 最小限の `spawn_blocking` 追加

`resolve.rs` と `search.rs` の同期走査呼び出しだけを `spawn_blocking` に寄せ、`memo.rs` を `tokio::fs::symlink_metadata` に置き換える案。

実装量は小さいが、`catalog` の canonical base 再利用や watcher callback の同期正規化削減が残る。今回の受け入れ条件を満たさないため採用しない。

### FS 境界 service trait への全面切り出し

catalog、resolve、search、memo、watcher のファイルシステム操作を専用 trait に集約し、同期・非同期・blocking 境界を一元管理する案。

長期的には整理されるが、`AppState` の `MemoFs` 整理やテスト注入 API と絡み、今回の TODO に対して変更量が大きい。別 TODO の API 整合解消と同時に検討する。

## コンポーネント設計

### `server::state`

新しい状態フィールドは追加しない。

既存の `AppMode::directory_canonical()` と `AppMode::single_file_canonical()` を、canonical base を必要とする処理の入口として使う。`AppState: Clone` や外側 `Arc<AppState>` の設計は今回触らない。

### `server::files::catalog`

`list_markdown_files_from_canonical_base(base: &CanonicalPath)` を追加する。公開範囲は `pub(super)` とし、`server::files` 内の resolve/search/catalog テストから使える境界にする。

再帰関数は次を引数に持つ。

- logging 用の base path
- canonical base path
- current directory
- visited set
- depth
- max files

symlink directory は canonicalize 結果で base 内外と cycle を判定する。通常 directory も同じ visited set 経由で cycle 防止する。`path.is_dir()` の同期 syscall による symlink directory 判定は避け、`entry.file_type()` と canonicalize 結果から再帰可否を決める。

### `server::service` / `server::files::resolve` / `server::files::search`

ディレクトリ走査を含む処理は `spawn_blocking` に移す。

`resolve_route_target()` は現在同期関数だが、デフォルトファイル選択やファイル一覧同梱時に走査するため、async 化する。上位の `load_page`、`load_content`、`load_memo`、`save_memo` から `.await` する。外部レスポンスの status と JSON 形状は変えない。

検索は既に PR #122 で重い処理を blocking task に隔離済みだが、catalog の canonical base API へ呼び出しを寄せる。`search_directory()` は canonical base から作った `PathBuf` を task に move し、task 内では base を再 canonicalize しない。

### `server::files::memo`

メモ保存・削除前の安全確認を async 化する。

`ensure_safe_memo_path()` と `ensure_safe_memo_rename_paths()` は async 関数へ変更し、内部の `first_unsafe_memo_path_component()` も async 化する。`std::fs::symlink_metadata` は `tokio::fs::symlink_metadata` に置き換える。`MemoFs` trait には今回 `symlink_metadata` を追加しない。

### `watcher::strategy`

`collect_directory_changes()` は `&CanonicalPath` または canonical base `&Path` を受ける。

`is_within_base_dir()` と `try_strip_base()` は、canonical base を前提にした lexical helper へ寄せる。削除イベントでは `event.path.canonicalize()` が失敗しうるため、存在することを前提にしない。base 外パスや相対化不能パスは安全側でスキップし、ログには `sanitize_path_for_logging()` を通した値だけを出す。

単一ファイルモードの `is_target_file()` は、atomic save や inode 切り替え耐性に関わるため、既存の canonicalize fallback を必要最小限維持する。

## データフロー

ディレクトリ一覧 API は `service::list_files()` から canonical base を取得し、blocking helper に渡す。blocking helper は catalog の canonical base API を呼び、`Vec<String>` を返す。

デフォルトファイル解決は `resolve_route_target().await` 内で、query file がない場合だけ blocking helper で一覧を取得する。取得した一覧から `README.md` 優先でデフォルトを選び、選択した相対パスは既存通り `resolve_file()` で最終検証する。

検索は blocking task 内で canonical base API から候補を取得し、各候補を `resolve_file()` で読込直前に再検証する。

watcher は notify event を受け、ディレクトリモードでは lexical helper で base 配下、hidden、`.md`、重複を判定する。通知された changed path は、後段の update 読込前に `resolve_change_target()` で再検証される。

## エラー処理

`spawn_blocking` の join error は、既存 `service::list_files()` と同じ分類にする。

- panic: `error!`
- panic 以外: `warn!`

API レスポンスには panic 詳細、絶対パス、内部状態を出さない。既存の「ファイル一覧の取得に失敗しました」「ディレクトリ検索に失敗しました」などの文言を維持する。

catalog の個別 entry 読み取り失敗、file type 取得失敗、canonicalize 失敗は既存通り warn してスキップする。base 自体の canonicalize 失敗は互換 API ではエラー、canonical base API では型上起きない。

watcher の相対化不能パスや base 外パスはスキップし、warn ログに留める。

## セキュリティ考慮

base directory は起動時に `CanonicalPath` として検証済みの値を使う。これにより、catalog と watcher が守る root を明確にし、再帰中に base を取り直すことで判定がぶれる余地を減らす。

base 外 symlink は引き続き拒否する。catalog では symlink directory の canonicalize 結果が canonical base 配下であることを確認してから再帰する。watcher ではイベントパスが存在しない場合もあるため、lexical 判定だけで読み込まず、後段の `resolve_change_target()` と `resolve_file()` の再検証を維持する。

hidden component と `.md` 拡張子のフィルタは catalog と watcher の両方で維持する。外部から来るパス、notify event、query parameter は未信頼入力として扱い、ログには sanitize 済みの値のみを出す。

この変更は Host / Origin / CSP を緩めない。localhost-only 前提、path traversal 防御、HTML sanitization には触らない。

## テスト方針

TDD で進める。

catalog:

- canonical base API の基本一覧、ソート、上限、hidden 除外を固定する。
- base 外 symlink と symlink cycle を引き続きスキップする。
- canonical base API では base canonicalize 失敗が起きない構造をテスト名と API 形状で明示する。

resolve / service / search:

- デフォルトファイル解決と file list 同梱が既存と同じ結果を返す。
- 単一ファイルモードの挙動が変わらない。
- 検索の JSON 形状、打ち切り理由、base 外 symlink 再検証が変わらない。

memo:

- symlink を含むメモ保存先が拒否される。
- metadata 取得失敗は安全確認失敗として拒否される。
- 保存・削除成功時の既存 memo response と broadcast 契約を維持する。

watcher:

- 削除済み base 配下 `.md` パスが base 配下として扱われ、hidden でなければ changed path に残る。
- base 外、hidden、非 Markdown はスキップされる。
- 単一ファイルモードの atomic save 耐性に関わる既存テストを維持する。

最後に以下を実行する。

```bash
cargo test --all-targets --all-features
./verify.sh
```

## 影響範囲

- `src/server/files/catalog.rs`
  - canonical base API と再帰引数整理。
- `src/server/files/resolve.rs`
  - `resolve_route_target` の async 化と blocking 一覧取得。
- `src/server/files/search.rs`
  - catalog canonical base API への呼び出し整理。
- `src/server/files/memo.rs`
  - symlink metadata 検査の async 化。
- `src/server/service.rs`
  - async 化された resolve 呼び出しへの追従。
- `src/watcher/strategy.rs`
  - directory event 判定 helper の lexical 化。
- `src/server/files/tests.rs`, `tests/integration_test.rs`
  - catalog、memo、resolve、search 境界テストの更新。

## ロールバック

実装コミットを revert すれば元に戻せる。

変更は実行境界、catalog API、watcher 判定 helper、memo metadata 検査に閉じる。データ形式、メモファイル名、検索レスポンス、WebSocket メッセージ形式は変更しないため、データ移行は不要。

問題が出た場合は、まず `resolve_route_target()` の async 化と catalog canonical base API への切り替えを戻し、既存の `list_markdown_files(&Path)` 呼び出しへ復帰する。watcher helper 変更も単独 revert 可能に実装を分ける。

## 残余リスク

- `spawn_blocking` に移しても、巨大 workspace の走査自体は完走または上限到達まで実行される。キャンセルや検索世代管理は別 TODO とする。
- watcher の lexical 判定は削除イベントを扱うために必要だが、存在する symlink の実体判定としては canonicalize より弱い。最終読込前の `resolve_change_target()` 再検証を防御層として維持する。
- `resolve_route_target()` の async 化は呼び出し連鎖に触れるため、コンパイル時には検出できるが変更範囲は広めになる。

## 見積もり

- 人間作業: 3-5 時間。
- Codex/AI 支援: 60-120 分。
