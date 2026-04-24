# Memo Sidecar Name Hardening Design

**作成日**: 2026-04-24
**対象 TODO**: メモ sidecar fallback 経路の超長ファイル名＋特殊文字テストを追加
**対象ファイル**: `src/server/files/memo.rs`, `src/server/files/memo_sidecar.rs`, `src/server/files/tests.rs`

## Goal

メモ sidecar ファイル名生成の契約を小さな内部ユニットとして明示し、超長ファイル名、特殊文字、UTF-8 境界、hash 衝突回避の回帰テストを追加する。

この作業はパストラバーサル境界の回帰検知を目的とする。`save_route_memo` / `load_route_memo` の保存先選択、legacy fallback、HTTP API の挙動は変更しない。

## Non-Goals

- メモ保存形式や保存先選択ルールの変更
- legacy memo から sidecar への移行仕様変更
- HTTP `/api/memo` のリクエスト/レスポンス仕様変更
- メモ API ボディサイズ制限の見直し
- `ensure_safe_memo_path` のシンボリックリンク検査緩和

## Design

`src/server/files/memo_sidecar.rs` を追加し、sidecar ファイル名生成だけを担当する内部ユニットを置く。`src/server/files/memo.rs` は保存/読込フローを持ち続け、filename construction の詳細をこの module へ委譲する。

想定 API:

```rust
struct SidecarMemoName(String);

impl SidecarMemoName {
    fn from_file_name(file_name: &std::ffi::OsStr) -> Self;
    fn as_str(&self) -> &str;
}
```

この型は crate 外へ公開しない。`sidecar_memo_path_for_target` は `target.file_path().file_name()` から得た `OsStr` を `SidecarMemoName::from_file_name` に渡し、返された plain filename を target parent に `join` する。

`MEMO_SUFFIX`, `MAX_FILENAME_BYTES`, `SIDECAR_HASH_LEN` は sidecar naming unit の近くに置き、現在のアルゴリズムを維持する。

## Naming Rules

UTF-8 filename:

1. `.{file_name}.memo.md` が 255 bytes 以下ならそのまま返す。
2. 255 bytes を超える場合、元 filename の UTF-8 bytes 全体を SHA-256 で hash する。
3. hash の先頭 16 hex chars を suffix に使う。
4. visible prefix は UTF-8 文字境界で切り詰める。
5. `.{prefix}.{hash}.memo.md` を返す。

Non-UTF-8 Unix filename:

1. raw bytes 全体を SHA-256 で hash する。
2. `._bin.{hash}.memo.md` を返す。

Path separator nuance:

通常の runtime 経路では `Path::file_name()` から得た final component だけが入力されるため、Unix の `/` は実ファイル名に含まれない。一方、helper の契約を明確にするため、直接テストでは `../` や `\\..\\` を含む入力を扱う。

helper は返り値を path ではなく filename string として扱わせる。UTF-8 入力に `/` または `\` が含まれる場合は、hash input には元の filename bytes を使い、visible prefix では `_` に正規化する。これにより `PathBuf::join` 時に path component が増えない。通常の runtime 経路では Unix の `/` は到達しないが、direct unit test でこの契約を固定する。

## Test Plan

TDD で以下を追加する。

- `test_sidecar_name_超長名は255バイト以内に短縮される`
- `test_sidecar_name_同一prefixの超長名はhashで衝突しない`
- `test_sidecar_name_特殊文字はパス区切りとして扱われない`
- `test_sidecar_name_utf8境界で切り詰める`
- Unix では `test_sidecar_name_非utf8名はhashで衝突しない` を direct unit test として追加する

既存の `save_route_memo` 系テストは behavior-level safety net として維持する。直接 unit test は filename generation の failure を局所化するために追加する。

## Acceptance Criteria

- sidecar naming unit が `sidecar_memo_path_for_target` から使われている
- overlong UTF-8 filename の sidecar name が 255 bytes 以下になる
- 同じ visible prefix を持つ異なる overlong filename が異なる sidecar name になる
- `../` と `\\..\\` 風の入力が path component として扱われないことをテストで固定する
- multi-byte UTF-8 truncation が文字境界を壊さない
- Unix non-UTF-8 filename の衝突回避が維持される
- `./verify.sh` が pass する

## Security Considerations

この変更はパストラバーサル境界の防御を緩めない。sidecar naming unit は final filename component だけを入力として扱い、保存先の親ディレクトリは既存の resolved target から決まる。

`ensure_safe_memo_path` の symlink component 検査は維持する。特殊文字テストは、AI 生成や外部由来の filename-like text が helper に渡された場合でも、path component として解釈されないことを固定する。

## Impact

- `src/server/files/memo.rs`: sidecar naming helper への委譲に変更。保存/読込フローは維持。
- `src/server/files/memo_sidecar.rs`: sidecar filename construction unit を追加。
- `src/server/files/tests.rs`: direct unit tests を追加。
- 依存ファイル: `tests/integration_test.rs` は原則変更しない。既存 integration tests が保存挙動の回帰検知を担う。

## Rollback

helper extraction と追加テストを revert すれば戻せる。runtime behavior の変更を意図しないため、rollback は局所的でよい。
