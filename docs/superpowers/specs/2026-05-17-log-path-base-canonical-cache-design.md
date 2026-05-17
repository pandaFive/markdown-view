# log_path base canonicalize cache 設計

**作成日**: 2026-05-17
**対象ファイル**: `src/server/log_path.rs`, `src/watcher/strategy.rs`

## 目的

ログ用パスサニタイズで `base.canonicalize()` を毎回実行している構造を整理し、base 側の canonical path を再利用できる境界を作る。

`src/server/log_path.rs` は監査ログ向けに path を base 相対化し、base 外 path は file name だけにマスクする責務を持つ。現状は `sanitize_path_for_logging(path, base)` のたびに `path.canonicalize()` と `base.canonicalize()` を試す。warn/error 時中心の呼び出しではあるが、watcher のログ storm や再帰監視の診断ログでは同じ base に対する呼び出しが重なりやすい。

今回の目的は、base の canonicalize 結果を小さな型に閉じ込めて再利用し、既存の安全契約を維持したまま余分な syscall を減らすことである。

## 非目的

- ログの表示形式を変更しない。
- base 外 path の秘匿ルールを変更しない。
- symlink 経由で base 外へ出る path の検出を弱めない。
- lexical fallback を削除しない。
- `server/files/*` など全呼び出し元を一括で置換しない。
- グローバルキャッシュや workspace 横断キャッシュは導入しない。

## 方針

### `LogBasePath` を追加する

`src/server/log_path.rs` に crate 内部向けの小さな型を追加する。

```rust
pub(crate) struct LogBasePath {
    base: PathBuf,
    canonical_base: Option<PathBuf>,
}
```

`LogBasePath::new(base: &Path)` は `base.to_path_buf()` と `base.canonicalize().ok()` を保持する。canonicalize 失敗はエラーにせず、ログ出力を継続する。

`LogBasePath` は次の操作を提供する。

- `sanitize(&self, path: &Path) -> Cow<'_, str>`
- `sanitize_escaped(&self, path: &Path) -> String`

既存の `sanitize_path_for_logging(path, base)` と `sanitize_path_for_logging_escaped(path, base)` は public 契約として残し、内部で一時的な `LogBasePath` を作る互換 API にする。単発呼び出しの利用者は変更不要にする。

### canonical 判定

`LogBasePath` が `canonical_base` を持ち、かつ `path.canonicalize()` が成功した場合だけ canonical 判定を行う。

- `canonical_path.strip_prefix(canonical_base)` に成功し、relative が空なら `"."`
- `canonical_path.strip_prefix(canonical_base)` に成功し、relative が空でなければ relative path
- strip に失敗した場合は `<outside-base>/{file_name}` または `<outside-base>`

`canonical_base` がない場合、または `path.canonicalize()` が失敗した場合は、現行と同じ lexical fallback を使う。

この方針により、base が存在する通常ケースでは symlink 経由の base 外脱出を canonical 判定で検出し、base が消えた場合でもログ出力自体は止めない。

### 呼び出し側への適用

まず `src/watcher/strategy.rs` のように同じ `log_base` で繰り返しログ整形する箇所へ限定して適用する。

単発の `src/server/files/*` 呼び出しは既存 API のまま残す。広範囲の churn を避け、今回の変更は「再利用できる境界を作ること」と「高頻度候補へ限定適用すること」に絞る。

## セキュリティ

この変更は監査ログの秘匿境界に触れるため、既存契約を弱めないことを最優先にする。

- base 配下 path は相対 path として表示する。
- path が base 自身なら `"."` として表示する。
- base 外 path は file name だけを残して `<outside-base>/{file_name}` とする。
- file name が取れない場合は `<outside-base>` とする。
- symlink 経由で base 外へ出る path は base 外として扱う。
- 制御文字は escaped API で可視化し、ログ行を壊さない。

`LogBasePath` は canonical base を生成時に固定する。これにより、起動後に base path 自体が差し替えられるような特殊な race では、固定済み canonical base に対する判定になる。これは watcher や server state が canonical path を基準に扱う既存設計と整合する。base canonicalize に失敗した場合は現行同様 lexical fallback になり、symlink 判定の精度は上がらないが、ログ出力を落とさない既存挙動を維持する。

外部検索結果や取得テキストは使わず、リポジトリ内の既存コードと文書だけを入力として扱う。

## テスト

`src/server/log_path.rs` の既存 unit test を維持し、`LogBasePath` 経由の契約を追加する。

追加または拡張する観点:

- cached base でも base 配下 path を相対化する。
- cached base でも symlink 経由で base 外へ出る path を outside 扱いにする。
- cached base でも symlink 経由で base 配下へ解決される path を canonical relative にする。
- base canonicalize に失敗する場合、lexical fallback でログ出力を継続する。
- escaped API が制御文字を可視化する。

呼び出し側の変更が `watcher/strategy.rs` に及ぶ場合は、既存 watcher strategy test を壊さないことを確認する。振る舞いの外部契約はログ文字列だけなので、新規 integration test は不要とする。

## 受け入れ条件

- `sanitize_path_for_logging` / `sanitize_path_for_logging_escaped` の既存利用者がソース互換のまま残る。
- `LogBasePath` 経由でも既存サニタイズ契約と同じ出力になる。
- 繰り返しログ整形する watcher 経路で base canonicalize 結果を再利用できる。
- symlink base 外脱出、base 内 symlink、base 自身、file name なし、制御文字 escape の契約がテストで固定される。
- `cargo test --all-targets --all-features log_path` が通る。
- 最終確認として `./verify.sh` を実行し、失敗した場合は原因と残リスクを報告する。

## 影響範囲

- `src/server/log_path.rs`: `LogBasePath` と既存 API の委譲先を追加する。既存サニタイズ契約とテストを維持する。
- `src/watcher/strategy.rs`: 同じ base で繰り返すログ整形に `LogBasePath` を渡せる範囲で限定適用する。
- `docs/todo/BACKLOG.md`: 実装後に対象項目を完了または更新する。

HTTP API、WebSocket payload、HTML sanitize、CSP、Host/Origin validation、path validation、memo sidecar、file size limit には影響しない。

## ロールバック

`LogBasePath` 追加と `watcher/strategy.rs` の呼び出し側差分を revert すれば戻せる。既存 API を削除しない設計のため、ロールバック時に他モジュールの利用者を広く修正する必要はない。

## 見積もり

- 人間作業: 1-2 時間
- Codex/AI 支援: 30-60 分
