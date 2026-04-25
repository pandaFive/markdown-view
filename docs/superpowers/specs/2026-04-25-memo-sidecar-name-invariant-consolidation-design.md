# Memo Sidecar Name Invariant Consolidation Design

**作成日**: 2026-04-25
**対象 TODO**: メモ sidecar 名生成の不変条件を `SidecarMemoName` に集約し、境界テストと受容リスクを補強
**前段**: [`2026-04-24-memo-sidecar-name-hardening-design.md`](./2026-04-24-memo-sidecar-name-hardening-design.md)
**対象ファイル**: `src/server/files/memo.rs`, `src/server/files/memo_sidecar.rs`, `src/server/files/tests.rs`

## Goal

`SidecarMemoName` を sidecar ファイル名生成の Single Source of Truth に格上げする。型が任意の入力に対して `as_str().len() <= MAX_FILENAME_BYTES (=255)` を構築時に保証することを doc で明示し、`src/server/files/memo.rs` 側の到達不能な `sidecar_name_too_long` 検査と関連する 3 か所の `sidecar_usable / sidecar_too_long` 分岐を撤去する。境界テストを追加し、既知の受容リスク（64 bit hash 衝突、Windows 非 UTF-8 fallback 集約）を設計書として固定する。

この作業は 2026-04-24 の hardening で `SidecarMemoName` が 255 bytes 以下を構築時保証するようになった結果として生じた、契約と呼び出し側の不整合を解消する。runtime behavior は変えない。

## Non-Goals

- メモ保存先選択ルールおよび legacy / compat fallback の挙動変更
- HTTP `/api/memo` のリクエスト/レスポンス仕様変更
- `SIDECAR_HASH_LEN`（= 16 hex chars / 64 bit）の拡張
- Windows 非 UTF-8 名向けの hash ベース個別化（`OsStrExt::encode_wide()` 経路追加）
- `ensure_safe_memo_path` の symlink 検査緩和
- 既存 sidecar ファイルの命名互換性破壊

## Design

### 不変条件の集約

`SidecarMemoName` の以下 3 つの constructor は、任意の `OsStr` 入力に対して `as_str().len() <= MAX_FILENAME_BYTES (= 255)` を保証する:

- `SidecarMemoName::from_file_name(file_name: &OsStr) -> Self`
- `SidecarMemoName::fallback() -> Self`
- `SidecarMemoName::compat_from_file_name(file_name: &OsStr) -> Option<Self>`

内部実装では以下のいずれかの経路をとる:

- UTF-8 入力かつ `.{name}.memo.md` が 255 bytes 以下: そのまま使用
- UTF-8 入力かつ 255 bytes 超過: `.{prefix}.{16hex hash}.memo.md` 形式に hash truncation
- 空入力 / Unix 非 UTF-8 入力: `._bin.{hash}.memo.md`（Unix）または `.memo.md`（非 Unix）
- legacy compat: `.{file_name}.memo.md` が 255 bytes 以下ならそのまま、超えれば hash truncation

いずれの経路も内部の prefix budget 計算と truncation により最終 byte 長は 255 を超えない。

### memo.rs 側の整理

以下を削除する:

- `sidecar_name_too_long(sidecar_path: &Path) -> bool`（`memo.rs:590-594`）
- `save_route_memo` 内の `if !sidecar_name_too_long(...)` 分岐（`memo.rs:53`）→ 中身を平坦化
- `resolve_active_memo_path` 内の `sidecar_usable` 変数および関連分岐（`memo.rs:221-248`）→ sidecar が常に有効である前提のフローへ簡素化
- `choose_save_target` 内の `sidecar_too_long` 変数および関連分岐（`memo.rs:257-271`）→ 同様に簡素化

`SidecarMemoName` が型として 255 bytes 以下を保証するため、呼び出し側でファイルシステム上限チェックを重複させる必要がない。defense-in-depth は型レベルのテスト（後述）が担う。

### doc コメント補強

`SidecarMemoName` の型 doc に「**不変条件**: 任意の `OsStr` 入力に対して `as_str().len() <= MAX_FILENAME_BYTES (= 255)` を構築時に保証する」を明記する。各 constructor の doc にも 255 bytes 以下保証を 1 行で示す。

## Test Plan

`src/server/files/tests.rs` の既存 sidecar 名テスト群（L48-147）の直後に追加する。TDD で先行追加するが、型が既に不変条件を満たしているため追加時点で全 pass する。**死んだコード削除後にも pass が維持されること** が、不変条件が型に集約された保証となる。

### 個別境界テスト 4 件

1. **`test_sidecar_name_255バイト境界はそのまま使う`** — `.{name}.memo.md` がちょうど 255 bytes ぴったりになる input。hash 経路に入らず raw filename が visible のまま使われ、`name.len() == 255` を assert
2. **`test_sidecar_name_256バイト境界はhash経路に入る`** — 1 byte 超過で hash truncation 経路。`name.len() <= 255`、hash suffix（16 hex）を含むことを assert
3. **`test_sidecar_name_utf8マルチバイト境界の直前で切断する`** — prefix budget の境目に 3 byte UTF-8 char を配置。`name.is_char_boundary(name.len())` と `name.len() <= 255` を assert
4. **`test_sidecar_name_パス区切り含む超長名でも255以下_衝突しない`** — 300+ byte の input に `/` `\` を埋め込む。区切り位置が異なる 2 名で衝突しないこと、`/` `\` を含まないこと、255 bytes 以下を assert

### 不変条件テーブル駆動テスト 1 件

5. **`test_sidecar_name_任意入力で常に255バイト以下_不変条件`** — テーブル駆動で以下の入力を網羅:

   - 空文字列
   - 1 byte ASCII
   - 254 / 255 / 256 / 1000 byte ASCII
   - 100 字分の `あ`（UTF-8 多バイト）
   - `../../etc/passwd`
   - `a/b\\c`
   - Unix 限定: `\xff` * 100、`\xfe` * 100（非 UTF-8）

   全ケースで `from_file_name(input).as_str().len() <= MAX_FILENAME_BYTES` を assert。

### 既存テストの扱い

既存の `test_sidecar_name_*` 群と save / load behavior テストは維持する。死んだコード削除後の回帰検知に必要。

## Known Risks（受容）

### 64 bit hash 衝突

`short_hash` は SHA-256 の先頭 16 hex chars（= 64 bit）を sidecar 名の suffix に使う。誕生日問題で約 2^32 ≒ 43 億ファイル / 同一ディレクトリで衝突確率 50%。衝突時は同一 sidecar を 2 ファイルが共有し、後勝ちで一方のメモが上書きされる。

個人使用前提の本ツールでは同一ディレクトリに 43 億ファイルは非現実的であり、このリスクを受容する。将来的に共有環境や大規模利用へ拡張する場合は `SIDECAR_HASH_LEN` を 32 hex chars 以上へ拡張する判断ポイントを残す。**拡張時は既存 sidecar の命名が変わるため、既存ファイルが orphan 化する点に注意**。

### Windows 非 UTF-8 名の fallback 集約

`#[cfg(not(unix))]` 経路では非 UTF-8 `OsString` の入力時 `SidecarMemoName::fallback()` に集約され、全て `.memo.md` を共有する。複数の非 UTF-8 名が同一ディレクトリに存在する場合、sidecar が共有されメモ混線が発生する。

Windows の通常ファイル名は内部 UTF-16 で `OsStr::to_str` できるため、このシナリオは実用上ほぼ到達しない。WSL ベースの Linux 主体開発という本プロジェクトの前提下では受容する。将来 Windows サポートを強化する場合は `OsStrExt::encode_wide()` 経由の raw bytes ベース hash を追加する判断ポイントを残す。

## Acceptance Criteria

- `sidecar_name_too_long` と関連する 3 か所の分岐が `memo.rs` から削除されている
- `SidecarMemoName` の型 doc と各 constructor doc に 255 bytes 以下保証が明記されている
- 個別境界テスト 4 件 + 不変条件テーブル駆動テスト 1 件が `tests.rs` に追加されている
- 既存の `test_sidecar_name_*` および save / load behavior テストが全て pass する
- `./verify.sh` が pass する
- Known Risks セクションが本設計書に含まれ、64 bit hash 衝突と Windows 非 UTF-8 fallback 集約が明文化されている

## Impact

- `src/server/files/memo.rs`: `sidecar_name_too_long` 関数および 3 か所の呼び出し / 分岐削除（約 30-40 行削減）
- `src/server/files/memo_sidecar.rs`: doc コメント補強のみ（実装ロジックは変更しない）
- `src/server/files/tests.rs`: 個別境界テスト 4 件 + 不変条件テーブル駆動テスト 1 件追加
- `tests/integration_test.rs`: 変更なし
- `docs/superpowers/specs/2026-04-24-memo-sidecar-name-hardening-design.md`: 変更なし（履歴性を尊重）

## Rollback

削除した `sidecar_name_too_long` と 3 か所の分岐を復活させ、追加テストを revert すれば戻せる。runtime behavior の変更を意図しないため rollback は局所的でよい。設計書は履歴として残してよい。

## Implementation Order

TDD ベースで以下の順に進める。各ステップで個別コミット、`./verify.sh` の pass を確認する。

1. 本設計書を `docs/superpowers/specs/` に追加してコミット
2. 個別境界テスト 4 件 + 不変条件テスト 1 件を `tests.rs` に追加してコミット（追加時点で pass）
3. `SidecarMemoName` の doc コメント補強をコミット
4. `memo.rs` から `sidecar_name_too_long` と関連分岐を削除してコミット（既存 + 追加テストの pass 維持を確認）
5. `docs/todo/TODO.md` の該当項目を完了マークしてコミット
