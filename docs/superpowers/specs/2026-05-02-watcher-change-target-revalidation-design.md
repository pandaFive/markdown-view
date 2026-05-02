# Watcher変更ターゲット再検証設計

## 背景

ディレクトリモードのHTTP経路は `resolve_file()` により、base配下、隠しパス拒否、`.md` 限定、symlink差し替え拒否、通常ファイル確認を行ってから読み込む。一方、watcher経由の変更通知は `collect_directory_changes()` で事前フィルタした後、`resolve_change_target()` が `changed_file` をそのまま `ResolvedTarget` に包み、`read_and_render_file()` が読み込む。

事前フィルタは有用だが、読込直前の最終防衛ではない。削除、rename、symlink差し替え、canonicalize失敗などの競合が起きても、HTTP経路と同じ境界で再検証される必要がある。

## 目的

- watcher経由のディレクトリ変更イベントを、読込直前に `resolve_file()` と同等の検証へ通す。
- 検証済みcanonical pathだけを `read_and_render_file()` に渡す。
- ディレクトリモードのatomic saveや削除で起きる一時的な `NotFound` はUIエラーにせず、broadcastをスキップする。
- traversal、hidden、非Markdown、無効パスは検証エラーとしてbroadcastし、セキュリティ境界の拒否を観測可能にする。
- 単一ファイルモードは既存挙動を維持しつつ、読込対象を再検証済みpathへ寄せる。

## 非目的

- watcherのイベント収集ロジック全面刷新。
- `WatchEvent` の型変更。
- 検索負荷制御、Host middleware化、memo仕様変更。
- UIエラー表示の見た目変更。

## アプローチ

`resolve_change_target()` を watcher由来変更イベントの最終検証ゲートにする。

単一ファイルモードでは、既存の `revalidate_single_file_target(expected, base_dir)` を使う。`ResolvedTarget` には watcherの `changed_file` ではなく、再検証済みcanonical pathを入れる。これにより「検証はexpected、読込はchanged_file」というズレをなくす。

ディレクトリモードでは、`changed_file` から `base_dir` 相対パスを復元し、その文字列表現を `resolve_file(base_dir, relative)` 相当の内部検証に渡す。`resolve_file()` 公開関数はHTTP/API向けの404統一を維持するためcanonicalize失敗を `NotFound` に揃えるが、watcher変更通知用の内部経路だけは `ErrorKind::NotFound` 以外のcanonicalize I/O失敗を `Io(ErrorKind)` として保持する。検証が返したcanonical pathを `ResolvedTarget` に入れるため、HTTP経路と同じ境界検証済みpathだけが後段へ流れる。

相対化できない `changed_file` は、存在するbase外ファイルやbase外symlinkなら `ResolveFileError::Traversal` として扱う。`changed_file` またはbaseが `NotFound` で正規化できない場合は、削除・rename中の一時不在として `ResolveFileError::NotFound` に分類する。その他のcanonicalize I/O失敗は `ResolveFileError::Io(ErrorKind)` に分類し、詳細なOSエラー本文ではなく `ErrorKind` だけを検証エラーに含める。相対化できても `resolve_file()` が拒否した場合は、そのエラー種別を維持する。

relative pathをquery文字列へ戻す際は、各path componentを `to_str()` で確認し、非UTF-8 componentは `InvalidPath` として拒否する。これは `to_string_lossy()` による置換文字混入で別ファイル名に見えることを避けるためである。区切り文字はcomponent単位で `/` にjoinし、Unix上の `back\slash.md` のようなbackslashを含むファイル名は通常文字として保持する。

## エラー処理

watcher変更通知では `ResolveFileError` を次のように扱う。

- `NotFound`: ディレクトリモードではbroadcastをスキップする。削除済みファイル、rename中、atomic save中の一時不在を通常操作として扱う。ログは `tracing::debug!` で、サニタイズ済みパスとスキップ理由を残す。単一ファイルモードでは監視対象そのものが消えた状態として、検証エラーをbroadcastし、受信者がいない場合もローカルwarnログに残す。
- `InvalidPath`: watcher由来pathが非UTF-8等で表示・query化できない場合は、ブラウザへError broadcastせずローカルログに留める。HTTP/APIとWebSocket初期ロードのvalidationは従来通りuser-facing errorとして扱う。
- `NotFile` / `Traversal` / `Hidden` / `NotMarkdown` / `EmptyPath` / `Io` / `InternalState`: 既存の `ResolveFailed` 経路に流し、`BroadcastMessage::Error` として通知する。`Io` は `ErrorKind` を表示して切り分け可能にし、`InternalState` は内部不整合として `error!` ログにする。
- `TooLarge` / UTF-8不正 / I/O失敗: 解決後の読込エラーとして既存の `ReadFailed` 経路を維持する。

ログやエラーメッセージでファイルパスを扱う場合は、既存の `sanitize_path_for_logging()` を使い、外部入力由来になりうるwatcher pathをそのまま露出しない。

## 変更対象

- `src/server/files/resolve.rs`
  - `resolve_change_target()` のディレクトリモード分岐を再検証方式へ変更する。
  - `resolve_directory_change_target()` と相対化ヘルパーを追加する。
- `src/server/files/content.rs`
  - `NotFound` をbroadcastスキップへ変換するため、`build_change_broadcast_message()` と `build_change_error_log_message_without_receivers()` の分岐を調整する。
- `src/server/files/tests.rs`
  - resolverとbroadcastの境界テストを追加する。

## テスト方針

TDDで次の失敗テストを先に追加する。

- watcher由来の通常 `.md` 変更は、`resolve_file()` 後のcanonical pathを返す。
- watcher由来のhidden pathは `Hidden` で拒否される。
- watcher由来の `.md` ディレクトリは `NotFile` で拒否され、Error broadcastになる。
- watcher由来の既存base外pathは `Traversal` で拒否される。
- ディレクトリモードのwatcher由来の存在しないpathまたは消えたbaseは `NotFound` でbroadcastをスキップする。
- watcher由来のcanonicalize I/O失敗は `Io(ErrorKind)` としてError broadcastになり、`PermissionDenied` 等の種別を含む。
- watcher由来の非UTF-8 pathはError broadcastせず、ローカルログのみになる。
- watcher由来のsymlinkがbase外へ向く場合は `Traversal` で拒否される。
- ディレクトリモードのwatcher由来の削除済み `.md` はbroadcastスキップ相当になる。
- `build_change_broadcast_message()` でディレクトリモードの `NotFound` は `None`、単一ファイルモードの `NotFound` や非通常ファイル・境界違反は `Some(BroadcastMessage::Error(_))` になる。

完了前に `./verify.sh` を実行する。途中確認では対象unit testと `cargo test --test integration_test` を使う。

## 受け入れ条件

- watcher経由のディレクトリ変更イベントは、読込直前にHTTP経路と同等のファイル検証を通る。
- `read_and_render_file()` へ渡るpathは検証済みcanonical pathである。
- ディレクトリモードの削除済みまたは一時不在の `.md` はUIに不要なエラーを出さない。
- 単一ファイルモードの監視対象消失は検証エラーとしてUIとローカルログで観測できる。
- base外、hidden、非Markdown、非通常ファイル、base外symlinkはError broadcastになる。
- 単一ファイルモードの既存動作は維持される。
- HTTP、memo、search経路の仕様は変わらない。
- 追加テストと `./verify.sh` が通る。

## 影響範囲

主な影響は watcher変更通知からファイル読込までの経路に限定する。HTTP、memo、search、watcherのイベント収集自体は仕様変更しない。

セキュリティ上の影響は、watcher pathを信頼せず、最終読込前に既存のHTTP向け検証へ通す点にある。これによりTOCTOU、symlink差し替え、削除競合時の境界がHTTP経路と揃う。

## ロールバック

更新通知が過剰に落ちるなどの問題が出た場合は、`resolve_change_target()` のディレクトリモード分岐を旧 `build_update_target(state, changed_file)` 呼び出しへ戻す。追加したhelperとテストは同じ変更単位で戻せる粒度に保つ。
