# Windows メモ atomic replace retry 設計

**作成日**: 2026-05-11
**対象 issue**: GitHub Issue #143「Windows メモ atomic replace のエラー分類と retry 条件を細分化する」

## 目的

Windows のメモ保存で使う `MoveFileExW` の失敗分類を明確にし、一時的な共有違反や lock に限って短く retry する。

現行実装では `MoveFileExW` を `spawn_blocking` 経由で呼び、失敗時は `io::Error::last_os_error()` を返している。一方で `spawn_blocking` の `JoinError` は `io::Error::other(...)` に潰され、panic と cancel の区別がログ上残らない。また Windows ではウイルス対策ソフト、同期ソフト、エディタ、インデクサが一瞬だけ対象ファイルを掴み、`ERROR_SHARING_VIOLATION` などで replace が失敗することがある。

今回の変更では HTTP API の外部契約を変えず、Windows-only の `atomic_replace` 内で `MoveFileExW` の `raw_os_error()` を分類する。retry は `MoveFileExW` 呼び出しだけに限定し、tmp 作成、本文書き込み、rename 前検証、cleanup、親ディレクトリ sync の既存契約は維持する。

## 非目的

- `MemoFs` trait、`MemoWriteError`、`save_route_memo` の public-facing な契約は変更しない。
- `/api/memo` の HTTP status、JSON shape、ユーザー向けエラー文言は変更しない。
- 非 Windows の `tokio::fs::rename` 挙動は変更しない。
- tmp path 生成、tmp 書き込み、flush/sync、rename 前の path validation、tmp cleanup、親ディレクトリ sync の順序は変更しない。
- Windows の実ファイル lock を CI で不安定に再現するテストは追加しない。
- retry 対象を広く取りすぎて恒久的な権限エラーを長く隠さない。

## 採用方針

`src/server/files/memo_fs.rs` の Windows-only 実装に、小さな helper を追加する。

`atomic_replace(tmp_path, path)` は `MoveFileExW` を直接1回呼ぶ関数ではなく、初回実行に加えて最大3回の短い retry ループを持つ関数にする。各 attempt は同じ tmp path と final path を使い、`MoveFileExW(tmp, final, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` だけを再実行する。

retry 対象は `raw_os_error()` が次の Windows error code の場合に限定する。

- `ERROR_ACCESS_DENIED = 5`
- `ERROR_SHARING_VIOLATION = 32`
- `ERROR_LOCK_VIOLATION = 33`

`ERROR_ACCESS_DENIED` は権限エラーでも返りうるため、無制限 retry はしない。今回は初回失敗後の retry を `10ms, 25ms, 50ms` の最大3回に留め、短時間 lock の吸収だけを狙う。

## 代替案

### 案A: `atomic_replace` 内に Windows 専用 helper を置く

`MoveFileExW` の直近に分類と retry を閉じ込める。`MemoFs` trait や API 層に波及せず、既存の atomic write 契約を維持できるため採用する。

### 案B: `MemoWriteError` に Windows replace 分類を追加する

上位層で原因を扱えるが、今回は HTTP 契約を変えないため情報の使い道が少ない。trait と API 周辺の影響が増えるため採用しない。

### 案C: atomic write 全体を retry する

tmp 作成から再実行すれば設計は単純に見えるが、本文書き込み、rename 前検証、cleanup の副作用範囲が広がる。今回の問題は `MoveFileExW` の一時失敗なので採用しない。

## アーキテクチャ

対象は `src/server/files/memo_fs.rs` の Windows-only `atomic_replace` に限定する。

追加する責務は次の通り。

- `move_file_ex_replace_once(tmp_path, path)`: `spawn_blocking` 内で `MoveFileExW` を1回呼び、失敗時は `io::Error::last_os_error()` を返す。
- `map_atomic_replace_join_error(error)`: `JoinError::is_panic()` と `is_cancelled()` を分けてログし、外向きには `io::Error::other(...)` を返す。
- `is_retryable_windows_replace_error(error)`: `raw_os_error()` で retry 対象かを判定する。
- `atomic_replace`: `move_file_ex_replace_once` を初回と最大3回の retry で呼び、retry 対象なら `10ms, 25ms, 50ms` の短い待機を挟む。

`MoveFileExW` の仕様上、関数失敗時の詳細は `GetLastError` で取得する。Rust 実装では `io::Error::last_os_error()` がその役割を担うため、`raw_os_error()` による分類を採用する。

## データフロー

### 正常保存

1. `save_route_memo` が保存対象と tmp/final path を検証する。
2. `write_atomic_with_counter` が同一ディレクトリ内 tmp を `create_new` で作る。
3. tmp に本文を書き、flush と sync を行う。
4. `before_rename(final, tmp)` を実行する。
5. Windows では `atomic_replace(tmp, final)` が `MoveFileExW` を呼ぶ。
6. 成功後、既存どおり親ディレクトリ sync を best effort で実行する。

### retry 可能な失敗

1. `MoveFileExW` が `ERROR_SHARING_VIOLATION`、`ERROR_LOCK_VIOLATION`、または `ERROR_ACCESS_DENIED` で失敗する。
2. `atomic_replace` は warning log を残し、短く待って同じ tmp/final path で再試行する。
3. retry 中は tmp を再作成せず、本文も再書き込みしない。
4. 初回を含む最大4回以内に成功すれば保存成功として扱う。

### retry 枯渇または retry 対象外失敗

1. retry 対象外の `raw_os_error()`、または retry 枯渇後の同じエラーを受け取る。
2. `atomic_replace` は元の `io::Error` を返す。
3. `write_atomic_with_counter` は既存通り tmp を best effort cleanup し、最終 sidecar の既存内容を保持する。
4. 上位 API は既存通り一般的な保存失敗として返す。

## エラー処理

`JoinError` は I/O retry 対象にしない。blocking task の実行異常として分類する。

- `JoinError::is_panic()`: `tracing::error!` で「メモ atomic replace task が panic した」ことを記録する。
- `JoinError::is_cancelled()`: `tracing::warn!` で「メモ atomic replace task が cancelled された」ことを記録する。
- その他の join error: `tracing::warn!` で分類不能な join error として記録する。

外向きの `io::Error` は `io::Error::other(...)` とし、API レスポンスには panic 詳細、絶対パス、内部状態を出さない。

retry ログは path 全体ではなく、可能ならファイル名や sanitized path に留める。メモ本文はログにもレスポンスにも出さない。

## セキュリティ考慮

この変更は未信頼入力の受け入れ範囲を広げない。`save_route_memo` の path validation、sidecar path の安全確認、rename 直前の final/tmp 検査、symlink component 拒否は既存通り維持する。

retry は `before_rename` 後の `MoveFileExW` だけに限定する。tmp path と final path は再解決せず、同じ検証済み path で replace を再試行するため、path traversal や symlink 差し替えを許す新規経路は作らない。

`ERROR_ACCESS_DENIED` は恒久的な権限問題でも返るため、最大3回で打ち切る。長時間待機や無制限 retry によるリソース占有を避ける。

エラー応答にはメモ本文、絶対パス、panic 詳細、Windows raw error の細部を含めない。ログも診断に必要な分類と error 概要に留める。

## テスト方針

TDD で進める。

`src/server/files/memo_fs.rs`:

- `is_retryable_windows_replace_error` が `raw_os_error()` の `5`、`32`、`33` を retry 対象にする。
- `AlreadyExists` や一般的な `Other` は retry 対象にしない。
- `map_atomic_replace_join_error` が panic/cancel を区別した `io::Error` message を返す。可能なら Windows cfg に閉じない helper にして単体テストする。
- 既存の Windows-only `atomic_replaceはwindowsで既存ファイルを置換する` を維持する。
- 既存の Windows-only `atomic_replaceはwindowsで新規ファイルへ移動できる` を維持する。
- `path_to_wide_null` の interior NUL 拒否テストを維持する。

OS の一時 lock を実際に起こすテストは flaky になりやすいため、retry 判定は pure helper で固定する。実際の `MoveFileExW` 成功系は既存 Windows-only test で固定する。

完了前の検証:

- `cargo test --all-targets --all-features`
- `./verify.sh`
- 可能なら `cargo check --target x86_64-pc-windows-gnu`

Windows target がローカルに未導入、または linker 不足で実行できない場合は残リスクとして報告する。

## 受け入れ条件

- Windows の `MoveFileExW` 呼び出しで `JoinError::is_panic()` と `is_cancelled()` がログ上区別される。
- Windows の `ERROR_ACCESS_DENIED`、`ERROR_SHARING_VIOLATION`、`ERROR_LOCK_VIOLATION` だけが短時間 retry 対象になる。
- retry は初回失敗後の `10ms, 25ms, 50ms` の最大3回に留まる。
- retry 失敗後も既存 sidecar 内容を壊さず、tmp cleanup は既存通り best effort で実行される。
- HTTP API の status、JSON shape、ユーザー向けエラー文言は変わらない。
- 非 Windows の rename 挙動は変わらない。
- メモ本文、絶対パス、panic 詳細を API レスポンスに出さない。
- `./verify.sh` が通る。

## 影響範囲

- `src/server/files/memo_fs.rs`: Windows-only `atomic_replace`、Windows error 分類 helper、JoinError 分類 helper、関連単体テスト。

依存する既存ファイル:

- `src/server/files/memo.rs`: `MemoWriteError` の API error mapping は変更しないが、Windows retry 枯渇時に従来通り同じ経路を通る。
- `src/server/files/test_support.rs`: `MockMemoFs` の atomic write 契約は変更しない。
- `tests/integration_test.rs`: HTTP 契約を変えないため新規統合テストは不要。

## ロールバック

問題が出た場合は、`src/server/files/memo_fs.rs` の Windows-only retry/classification helper と関連テストを revert すればよい。`MemoFs` trait、route、service、フロントエンドには触れないため、rollback 範囲は限定的である。

## 工数見積もり

人間の作業見積もり: 45-90 分。Windows-only helper の切り出し、単体テスト、Linux 上の verify、可能なら Windows target check を含む。

Codex / AI 支援込み見積もり: 20-45 分。変更範囲は狭いが、Windows cfg と cross-target check の環境差分確認に時間がかかる可能性がある。
