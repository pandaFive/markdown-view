# Issue 147: AppMode 構築時 filesystem 種別判定 TOCTOU 緩和設計

## 背景

Issue 147 は、`AppMode::new_single_file` / `new_directory` が `canonicalize` 済みパスに対して `is_file()` / `is_dir()` を呼び、`canonicalize` と種別判定の間に rename/unlink される TOCTOU window がある問題を扱う。

現状では起動時の `main.rs` でも同じ形で、`args.path.canonicalize()` 後に `path.is_file()` / `path.is_dir()` でモードを分岐している。単一ファイルモードでは、その後さらに `metadata()` を取得してサイズ上限を確認しているため、種別判定とサイズ判定の根拠が分かれている。

## ゴール

- `AppMode::new_single_file` / `new_directory` で、`canonicalize` 後の `is_file()` / `is_dir()` 呼び出しをやめる。
- `main.rs` の起動時モード分岐も、取得済み `metadata.file_type()` に基づく判定へ寄せる。
- 単一ファイルの `.md` 拡張子チェックと `MAX_FILE_SIZE` チェックを維持する。
- 既存の public API、public error variant、CLI の正常系挙動、server/watch/rendering の契約を変えない。

## 非ゴール

- filesystem race の完全排除。今回の対象は起動時限定の window を縮める緩和であり、open 後の inode 固定や platform-specific handle 管理は行わない。
- `CanonicalFile` / `CanonicalDirectory` の新型導入。
- symlink 解決ポリシー、`.md` 判定、ファイルサイズ上限、path validation、Host/Origin/CSP、HTML sanitization の変更。
- watcher、renderer、memo、search の挙動変更。

## 実装方針

`CanonicalPath::try_from_path()` は維持する。その上で `src/server/state.rs` に private helper を追加し、`CanonicalPath` 生成後の種別確認を `metadata()` 一回に集約する。

想定 helper:

```text
metadata_for_mode(&CanonicalPath) -> std::io::Result<std::fs::Metadata>
ensure_canonical_file(&CanonicalPath) -> Result<(), AppModeBuildError>
ensure_canonical_directory(&CanonicalPath) -> Result<(), AppModeBuildError>
```

`ensure_canonical_file()` は `metadata_for_mode()` が返した `Metadata` の `file_type().is_file()` を確認し、違えば既存の `AppModeBuildError::NotFile` を返す。`new_single_file()` はこの helper を通してから、既存どおり canonical path の拡張子で `.md` を判定する。

`ensure_canonical_directory()` は同じく `file_type().is_dir()` を確認し、違えば既存の `AppModeBuildError::NotDirectory` を返す。`new_directory()` はこの helper を通してから `AppModeKind::Directory` を作る。

metadata 取得失敗は、public error variant を増やさず、単一ファイル側では既存の `AppModeBuildError::NotFile`、ディレクトリ側では既存の `AppModeBuildError::NotDirectory` に安全側で集約する。CLI 起動時の metadata 取得失敗は context 付き error とし、対象パスを含めて原因を追えるようにする。

`main.rs` は `args.path.canonicalize()` 後の `if path.is_file() ... else if path.is_dir()` をやめる。代わりに `std::fs::metadata(&path)` を一度取得し、その `file_type()` で単一ファイルかディレクトリかを分岐する。単一ファイル時の `MAX_FILE_SIZE` チェックは同じ metadata の `len()` を使う。

## セキュリティ考慮

起動引数のパスは未信頼入力として扱う。今回の変更では canonical path 境界を維持したまま、種別判定に使う情報を `metadata()` の戻り値に固定し、`canonicalize` 後に `is_file()` / `is_dir()` が内部で別途 metadata を取り直す形を避ける。

この変更は TOCTOU の完全な解消ではない。`canonicalize` と `metadata` の間、または metadata 判定後の実ファイル open までには race が残る。ただし issue の対象である `canonicalize` 後の種別判定 window は縮み、判定とサイズチェックの根拠が読み取りやすくなる。

symlink、path traversal、Host/Origin/CSP、HTML sanitization、file size guard の既存ポリシーは緩めない。特に単一ファイルのサイズ上限は、起動時 metadata と実読込時の既存 guard の両方を維持する。

## 受け入れ条件

- `AppMode::new_single_file` / `new_directory` が `canonicalize` 後の `is_file()` / `is_dir()` を使わない。
- `main.rs` の起動時モード分岐も `path.is_file()` / `path.is_dir()` を使わない。
- 種別判定は取得済み `metadata.file_type()` に基づく。
- 単一ファイルの `.md` 拡張子チェックと `MAX_FILE_SIZE` チェックが維持される。
- metadata 取得失敗時は既存の `AppModeBuildError` variant または起動時 context 付き error として伝播する。
- 既存の public API とユーザー可視の正常系挙動が変わらない。
- `./verify.sh` が通る。

## テスト方針

実装時は TDD 寄りに進める。まず `src/server/state.rs` の unit tests で、既存の正常系と逆指定 regression を維持する。

追加・維持する観点:

- `.md` ファイルは単一ファイルモードとして受理される。
- ディレクトリはディレクトリモードとして受理される。
- ディレクトリを `new_single_file()` に渡すと `NotFile`。
- ファイルを `new_directory()` に渡すと `NotDirectory`。
- 存在しないパスは `CanonicalPath` error。
- 非 `.md` ファイルは `NotMarkdown`。

metadata 取得後にパスが消える race は deterministic な unit test にしにくいため、直接の race 再現は必須にしない。portable 性を保った regression test を優先し、Unix 固有の special file テストは必要になった場合だけ追加する。

想定コマンド:

```text
cargo test --lib server::state
cargo test --all-targets --all-features app_mode
./verify.sh
```

docs-only の本設計書作成では、Markdown の placeholder、矛盾、scope、曖昧さを自己レビューし、`git diff --check` を validation として実行する。

## 影響範囲

- `src/server/state.rs`: private helper の追加、`new_single_file` / `new_directory` の種別判定変更。
- `src/main.rs`: 起動時の file/directory 分岐と単一ファイルサイズチェックを `metadata.file_type()` ベースへ変更。
- `src/server/state.rs` の unit tests: regression coverage の追加または調整。

dependent file として、`src/server.rs` の re-export、watcher、server/files、integration tests は `AppMode` の公開 API を通じて影響を受ける。ただし API 形は維持するため、呼び出し側の変更は不要にする。

## Rollback path

実装コミットを revert すれば、`canonicalize` 後の `is_file()` / `is_dir()` による旧判定へ戻せる。設定変更、データ移行、ユーザー操作は不要。

## 見積もり

- Human effort: 30〜50分
- Codex/AI-assisted effort: 15〜30分

`main.rs` と `AppMode` のエラー文言を既存挙動に近づける調整、ならびに `./verify.sh` の実行時間で上下する。

## 残留リスク

- `metadata()` と実際の file open の間には race が残る。
- metadata 取得失敗を既存 variant に集約するため、詳細な io error source は `AppModeBuildError` からは参照できない。
- 起動時 error message の context がわずかに変わる可能性があるため、CLI error snapshot 相当の検証がある場合は確認する。
