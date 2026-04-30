# メモ保存の原子化と削除契約の再設計

- 対象: `src/server/files/memo_fs.rs`, `src/server/files/memo.rs`, `src/server/files/test_support.rs`, `src/server/files/tests.rs`
- 関連 TODO: `docs/todo/TODO.md` High Priority「メモ書き込みを `tmp + rename` で原子化する」「`delete_route_memo` を all-or-nothing 化する」
- 作成日: 2026-04-30
- Status: IMPLEMENTED

## 1. 概要

### 1.1 背景

現在の非空メモ保存は `MemoFs::write` から `tokio::fs::write` を呼ぶ。これは内部的に truncate + write になり得るため、電源断、プロセス停止、ディスクエラーのタイミングによって既存メモが空または部分書き込みで壊れる可能性がある。

また、保存前の `ensure_safe_memo_path()` と実 write の間には TOCTOU window が残る。未信頼 workspace や同期ディレクトリでは、メモ sidecar 経路の symlink 差し替えがセキュリティ境界になる。

空白保存の削除経路も、primary sidecar を先に削除してから compat / legacy を削除するため、後続削除で `PermissionDenied` などが起きると「500 を返したが primary は既に消えた」状態になり得る。

### 1.2 ゴール

1. 非空メモ保存を同一ディレクトリ内の `tmp + rename` で原子化する。
2. 保存失敗時に既存 sidecar が壊れない契約をテストで固定する。
3. rename 直前にも保存先安全性を再検証し、symlink race の窓を狭める。
4. tmp 残存時は安全に best-effort cleanup する。
5. 空白保存削除は primary sidecar を最後に削除し、失敗レスポンス時の中間状態を減らす。
6. `MockMemoFs` でも本番と同じ意味の原子保存 API を観測・失敗注入できるようにする。

### 1.3 非ゴール

- メモ保存先の命名規則変更。
- sidecar / compat_sidecar / legacy の読み込み優先順位変更。
- UI の保存表示や autosave 挙動の変更。
- 完全なファイルシステムトランザクションの実現。
- OS 固有 API を使った TOCTOU の完全排除。

## 2. 採用方針

`MemoFs` に原子保存 API を追加し、保存経路はその API へ切り替える。

理由:

- `MemoFs` は既にメモ I/O の抽象境界であり、原子保存の契約を置く場所として自然。
- 本番実装と `MockMemoFs` のセマンティクスを揃えられる。
- `save_route_memo` に tmp/rename の手順を直接埋め込むより、失敗注入と回帰テストが簡潔になる。
- 変更範囲を memo 経路に閉じやすい。

## 3. 非空保存契約

### 3.1 公開フロー

`save_route_memo` の非空保存は次の順で処理する。

1. `raw.len() > MAX_FILE_SIZE` なら `413 PAYLOAD_TOO_LARGE`。
2. `ensure_safe_memo_path(sidecar)` が失敗したら `403 FORBIDDEN`。
3. `MemoFs::create_dir_all(sidecar.parent())` が失敗したら `500 INTERNAL_SERVER_ERROR`。
4. `MemoFs::write_atomic(sidecar, raw.as_bytes(), before_rename_check)` が失敗したら `500 INTERNAL_SERVER_ERROR`。
5. 保存成功後、compat_sidecar / legacy cleanup を best-effort で実行する。
6. `MemoResponse::from_raw(raw, ...)` を返す。

### 3.2 `write_atomic` 契約

`MemoFs::write_atomic(path, content, before_rename)` は次を保証する。

- tmp は `path` と同じ親ディレクトリに作る。
- tmp は `create_new` 相当で作り、既存ファイルを上書きしない。
- tmp へ全バイトを書き込んでから最終パスへ rename する。
- rename 直前に `before_rename(final_path, tmp_path)` を実行し、失敗したら rename しない。
- rename が成功したら、読者は旧内容または新内容のどちらかを見る。空または部分書き込みの最終ファイルを残さない。
- 途中失敗時は最終パスを破壊しない。
- 途中失敗時の tmp cleanup は best-effort とし、cleanup 失敗で本来の I/O エラーを上書きしない。

### 3.3 `TokioMemoFs` 実装方針

本番実装は次の手順にする。

1. 親ディレクトリを取得する。親がない場合は `InvalidInput` を返す。
2. 同一親ディレクトリ内に tmp パスを生成する。
3. `OpenOptions::new().write(true).create_new(true)` で tmp を作る。
4. `write_all(content)` で全バイトを書く。
5. `flush()` と可能なら `sync_data()` を呼ぶ。
6. tmp file handle を閉じる。
7. `before_rename(path, tmp_path)` で rename 直前の安全性再検証を実行する。
8. `tokio::fs::rename(tmp, path)` で差し替える。
9. 親ディレクトリ sync を best-effort で試行する。

親ディレクトリ sync は Unix ではディレクトリ open + `sync_all` を試す。プラットフォーム差や permission 差で失敗し得るため、処理自体は best-effort に留める。ただしクラッシュ時の永続性が弱まる運用上重要な劣化なので、失敗は error ログに残す。少なくともファイル内容の部分書き込み防止は rename で担保する。

### 3.4 tmp 名

tmp 名は最終 sidecar 名から派生させる。

例:

```text
.README.md.memo.md.tmp.<pid>.<counter>
```

要件:

- 同一ディレクトリ内に作る。
- `create_new` で衝突時に検出する。
- 衝突時は短い回数だけ suffix を変えて再試行する。
- tmp 名が長くなりすぎる場合は hash suffix を使い、255 byte 境界を超えないようにする。
- tmp cleanup は `ensure_safe_memo_path(tmp)` を通るパスだけに限定する。

## 4. rename 直前の安全性再検証

`save_route_memo` は `write_atomic` に rename 前検証 callback を渡す。これにより、tmp path を知っている `MemoFs` 実装内で rename 直前に同じ検証を必ず実行できる。

```text
save_route_memo
  ensure_safe_memo_path(sidecar)
  create_dir_all(parent)
  fs.write_atomic(sidecar, content, before_rename_check)
```

`before_rename_check` の本体は `memo.rs` 側に置き、既存の `ensure_safe_memo_path` と同じポリシーを使う。`MockMemoFs` でも `before_rename_check` を実行し、rename 直前検証が省略されないことをテスト可能にする。

検証内容:

- final sidecar path に symlink component がない。
- tmp path に symlink component がない。
- final と tmp の親ディレクトリが同一である。

## 5. 空白保存削除契約

### 5.1 現状の問題

現在は primary sidecar を削除してから compat / legacy を削除する。後続削除で失敗すると、HTTP は 500 だが primary は既に消えている。クライアント再試行時の挙動が変わり、ユーザーには「失敗したのにメモが消えた」ように見える。

### 5.2 新しい順序

空白保存は次の順にする。

1. 削除候補を列挙する。
2. primary sidecar は unsafe なら `403 FORBIDDEN`。
3. compat_sidecar / legacy は unsafe なら warn して削除候補から外す。
4. safe な compat_sidecar / legacy を先に削除する。
5. primary sidecar を最後に削除する。
6. 全削除成功または `NotFound` のみなら `MemoResponse::empty(...)` を返す。

### 5.3 失敗時契約

削除は完全な all-or-nothing ではない。ファイルシステムに複数ファイル削除のトランザクションはないため、compat 削除成功後に legacy 削除失敗、または legacy 削除成功後に primary 削除失敗は起こり得る。

ただし、primary sidecar を最後にすることで、最重要の中間状態である「500 だが primary だけ消えた」を避ける。

削除結果:

| 対象 | 結果 | HTTP |
| --- | --- | --- |
| compat / legacy | `Ok(())` or `NotFound` | 続行 |
| compat / legacy | その他 I/O エラー | 500 |
| primary sidecar | `Ok(())` or `NotFound` | 200 |
| primary sidecar | その他 I/O エラー | 500 |

compat / legacy の I/O エラーを warn + 200 に落とす案は採用しない。primary sidecar を削除済みとして 200 を返すと、残った compat / legacy が次回読み込みで復活し、「空保存したのにメモが戻る」状態になるためである。500 を返して primary sidecar を残すほうが、ユーザーに失敗を観測させつつ読み込み優先順位の整合性を保てる。

## 6. テスト設計

### 6.1 追加・更新するユニットテスト

- 非空保存は `MemoFs::write_atomic` を呼ぶ。
- `write_atomic` 失敗時、既存 sidecar 内容が残る。
- rename 失敗時、既存 sidecar 内容が残り、tmp は best-effort cleanup される。
- tmp 作成衝突時、別 suffix で再試行する。
- tmp cleanup 失敗は本来の保存エラーを上書きしない。
- final sidecar が symlink の場合は 403。
- rename 直前に final sidecar が symlink 化された場合は 403。
- 空白保存は compat / legacy を primary より先に削除する。
- 空白保存で compat / legacy 削除失敗時、primary sidecar は残る。
- 空白保存で primary 削除失敗時、500 を返す。

### 6.2 `MockMemoFs` の変更

`Op::Write` は既存テスト互換のため一時的に残してもよいが、保存経路の失敗注入は `Op::WriteAtomic` へ移す。

`MockMemoFs` は次を記録する。

- `atomic_writes: Vec<(PathBuf, Vec<u8>)>`
- 削除順序を観測するための `operations: Vec<OpEvent>`
- 必要なら `before_rename` 実行回数

### 6.3 統合テスト

既存 API 境界の保存・再取得テストは維持する。追加は最小限にし、原子性の細部はユニットテストで固定する。

統合テストで確認する候補:

- HTTP PUT 成功後、sidecar は新内容、tmp は残らない。
- HTTP PUT で symlink sidecar は 403。

## 7. セキュリティ考慮

- tmp は必ず final sidecar と同じ親ディレクトリに作る。別ディレクトリや `/tmp` は使わない。
- tmp 作成は `create_new` で既存ファイル上書きを避ける。
- final path と tmp path の symlink component を検査する。
- rename 直前にも安全性を再検証し、保存前検証と実 rename の間の差し替えを検出しやすくする。
- retrieved text やメモ本文は信頼しない。今回の変更は保存手順のみで、HTML sanitization 契約は変更しない。
- 完全な TOCTOU 排除は本設計の非ゴール。OS 固有の `openat` / `renameat` / `O_NOFOLLOW` までは導入しない。

## 8. 影響範囲

直接影響:

- `src/server/files/memo_fs.rs`: `MemoFs` API と `TokioMemoFs` の保存実装。
- `src/server/files/memo.rs`: 非空保存フロー、rename 直前検証、空白保存削除順。
- `src/server/files/test_support.rs`: `MockMemoFs` の失敗注入と操作履歴。
- `src/server/files/tests.rs`: メモ保存・削除契約テスト。

間接影響:

- `tests/integration_test.rs`: 必要に応じて API 境界の tmp 残骸なし確認を追加。
- `docs/todo/TODO.md`: 実装完了時に該当 High TODO を完了へ移す。

## 9. 受け入れ基準

- 通常メモ保存成功後、sidecar は新内容で tmp は残らない。
- 原子保存の書込失敗または rename 失敗時、既存 sidecar 内容は破壊されない。
- symlink を含む保存先は 403。
- rename 直前の symlink 差し替え検証がテストで固定されている。
- 空白保存で compat / legacy 削除失敗が起きても primary sidecar は残る。
- `MockMemoFs` が原子保存 API の呼び出しと削除順序を観測できる。
- `cargo fmt --all -- --check` が通る。
- `cargo clippy --all-targets --all-features -- -D warnings` が通る。
- `cargo test --all-targets --all-features` が通る。
- 最終的に `./verify.sh` が通る。

## 10. ロールバック

保存データ形式は変えない。sidecar / compat_sidecar / legacy のファイル名や読み込み優先順位も変えない。

問題が出た場合は、保存経路を従来の直接 write に戻せる。主な revert 対象は `MemoFs` API 追加、`TokioMemoFs::write_atomic`、`save_route_memo` の保存呼び出し、空白保存削除順序、関連テストである。

ロールバック後も既存の sidecar / legacy メモは読み込み可能。

## 11. 残余リスク

- rename は同一ファイルシステム内では原子的だが、ディレクトリエントリ永続化は OS とファイルシステムに依存する。
- 親ディレクトリ sync の扱いはプラットフォーム差があるため、実装時に検証結果を見て fatal / warn を決める必要がある。
- symlink TOCTOU は rename 直前再検証で狭めるが、完全には閉じない。
- 空白保存削除は primary を最後にするが、複数ファイル削除の完全 all-or-nothing ではない。
