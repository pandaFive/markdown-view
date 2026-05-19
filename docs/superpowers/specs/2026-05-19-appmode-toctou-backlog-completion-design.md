# AppMode TOCTOU backlog completion design

## 背景

`docs/todo/BACKLOG.md` の P2 には、`AppMode` 構築時の `is_file()` / `is_dir()` 判定の TOCTOU 緩和が未完了として残っている。

現行の `src/server/state.rs` を確認すると、`CanonicalPath::try_from_path()` で canonicalize した後、`ensure_canonical_file()` と `ensure_canonical_directory()` が `metadata_for_mode()` 経由で `std::fs::metadata()` を取得し、その `file_type()` から file / directory を判定している。さらに canonicalize 後に対象が消えた場合を `NotFile` / `NotDirectory` へ集約する単体テストも存在する。

そのため、この backlog 項目は新規実装ではなく、現行コードで達成済みの契約を再確認して完了処理する。

## ゴール

- `AppMode` 構築時の file / directory 判定が metadata 起点になっていることを現行コードで確認する。
- canonicalize 後に対象が消えた場合のエラー契約が unit test で固定されていることを確認する。
- `docs/todo/BACKLOG.md` の P2 項目を Done へ移し、完了根拠を残す。
- path validation、canonicalize 境界、Markdown 拡張子チェック、Host / Origin 検証、CSP、HTML sanitize を弱めない。

## 非ゴール

- `CanonicalPath` の API 再設計。
- 起動後の filesystem race 全般の完全解消。
- symlink / canonicalize 境界の広範な見直し。
- `main.rs` の起動時 path 判定や server 起動フローの変更。
- ファイル読込、ディレクトリ列挙、watcher、memo、renderer の挙動変更。

## 設計

### 現行契約の扱い

`AppMode::new_single_file()` と `AppMode::new_directory()` は、まず `CanonicalPath::try_from_path()` で入力 path を canonicalize する。その後、単一ファイルモードは `ensure_canonical_file()`、ディレクトリモードは `ensure_canonical_directory()` を通る。

両 helper は canonicalize 済み path に対して `metadata_for_mode()` を呼び、取得した `Metadata` の `file_type()` で file / directory を判定する。これにより、canonicalize 済み path に対する後続判定は `Path::is_file()` / `Path::is_dir()` の再呼び出しではなく、明示的な metadata 取得結果に基づく。

canonicalize と metadata の間に rename / unlink が起きる race window 自体は filesystem の性質上ゼロにはできないが、metadata 取得に失敗した場合は単一ファイルモードでは `AppModeBuildError::NotFile`、ディレクトリモードでは `AppModeBuildError::NotDirectory` へ集約される。現行テスト `test_ensure_canonical_file_metadata失敗はnotfileへ集約する` と `test_ensure_canonical_directory_metadata失敗はnotdirectoryへ集約する` がこの契約を固定している。

### BACKLOG 更新

`docs/todo/BACKLOG.md` の P2 未完了項目から `AppMode` TOCTOU 緩和を削除し、同ファイルの Done へ移す。

Done には、以下を完了根拠として残す。

- `ensure_canonical_file()` / `ensure_canonical_directory()` が `metadata_for_mode()` の結果で file / directory を判定している。
- metadata 取得失敗時の `NotFile` / `NotDirectory` 集約が unit test で固定されている。
- `.md` 拡張子チェック、canonical path 保持、base_dir / single_file / directory の公開契約は変更していない。

## 受け入れ条件

- `docs/todo/BACKLOG.md` の P2 から `AppMode` TOCTOU 緩和項目が消えている。
- `docs/todo/BACKLOG.md` の Done に完了根拠が追加されている。
- `cargo test --lib server::state` が通る。
- `rg -n "TOCTOU|metadata_for_mode|ensure_canonical_file|ensure_canonical_directory|NotFile|NotDirectory" docs/todo/BACKLOG.md src/server/state.rs` で根拠を追跡できる。
- 挙動変更がないため、コード差分は原則として発生しない。既存テスト名や説明の補強が必要な場合のみ最小限にする。

## テストと検証

実装フェーズでは、まず現行の state unit test を実行する。

```bash
cargo test --lib server::state
```

`BACKLOG.md` 更新後は、文書検証として以下を確認する。

```bash
rg -n "TOCTOU|metadata_for_mode|ensure_canonical_file|ensure_canonical_directory|NotFile|NotDirectory" docs/todo/BACKLOG.md src/server/state.rs
rg -n "TB[D]|TO[D]O|未[定]" docs/superpowers/specs/2026-05-19-appmode-toctou-backlog-completion-design.md docs/todo/BACKLOG.md
```

最終確認として repository 標準の検証を実行する。

```bash
./verify.sh
```

## セキュリティ考慮

この作業は path safety に関する backlog 完了処理だが、設計上は挙動変更を行わない。`CanonicalPath::try_from_path()` による canonicalize、単一ファイルモードの `.md` 拡張子チェック、ディレクトリモードの canonical path 保持を維持する。

外部入力由来の path は引き続き未信頼入力として扱う。今回の文書更新では Host / Origin 検証、HTML sanitize、CSP、memo sidecar path validation、directory traversal 防止、watcher の再検証境界には触れない。

## 影響範囲

- `docs/todo/BACKLOG.md`
  - P2 未完了項目の削除と Done への完了根拠追加。
- `src/server/state.rs`
  - 参照対象。原則としてコード変更なし。

依存的に、`AppMode` を使う `main.rs`、server route、watcher、file resolve、integration tests は確認対象になるが、契約変更はない。

## ロールバック

`BACKLOG.md` 更新コミットを revert すれば、未完了項目として戻せる。データ移行、設定変更、ユーザー操作は不要。

## 見積もり

- 人間作業: 15〜25分。
- Codex / AI 支援: 5〜10分。

主な変動要因は、`server::state` テストまたは `./verify.sh` に既存失敗がある場合の切り分け時間。

## 残留リスク

- canonicalize と metadata の間の race window は完全には消えない。現行設計は window を縮め、失敗時のエラー契約を明確にする範囲に留める。
- `BACKLOG.md` の stale 項目整理であり、実行時の security boundary をさらに強化する作業ではない。
