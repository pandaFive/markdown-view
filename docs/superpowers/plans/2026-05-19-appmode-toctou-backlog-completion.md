# AppMode TOCTOU Backlog Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** 現行 `AppMode` の metadata 起点判定を検証し、stale になっている `docs/todo/BACKLOG.md` の P2 項目を Done へ移す。

**Architecture:** `src/server/state.rs` は参照対象に留め、`ensure_canonical_file()` / `ensure_canonical_directory()` と既存 unit test で契約を確認する。実際の変更は `docs/todo/BACKLOG.md` の未完了項目削除と Done 追記だけに限定する。

**Tech Stack:** Rust, Cargo unit tests, ripgrep, repository-local `./verify.sh`, Markdown docs.

---

## Files

- Modify: `docs/todo/BACKLOG.md`
  - 責務: 低優先・長期改善候補と完了判断根拠の保持。
  - 今回の変更: `AppMode` TOCTOU 緩和項目を P2 から削除し、Done に完了根拠を追加する。
- Reference: `src/server/state.rs`
  - 責務: `CanonicalPath`、`AppMode`、`AppState` の構築契約を管理する。
  - 今回の扱い: コード変更なし。`metadata_for_mode()`、`ensure_canonical_file()`、`ensure_canonical_directory()`、metadata 失敗時テストを確認する。
- Reference: `docs/superpowers/specs/2026-05-19-appmode-toctou-backlog-completion-design.md`
  - 責務: 承認済み設計。実装時は参照専用として扱う。

## Task 1: AppMode TOCTOU backlog 項目を完了扱いに更新する

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Reference: `src/server/state.rs`
- Reference: `docs/superpowers/specs/2026-05-19-appmode-toctou-backlog-completion-design.md`

- [x] **Step 1: 作業ブランチを確認する**

Run:

```bash
git status --short --branch
```

Expected: 現在のブランチが `docs/appmode-toctou-backlog-complete` である。未コミット差分がある場合は、この plan 自体または今回対象の `docs/todo/BACKLOG.md` だけであることを確認する。`develop` または `main` の場合は作業を止める。

Result: ブランチは `docs/appmode-toctou-backlog-complete`。開始時の未コミット差分はこの plan のみ、BACKLOG 編集後も対象差分だけであることを確認した。

- [x] **Step 2: 現行 state unit test で契約を確認する**

Run:

```bash
cargo test --lib server::state
```

Expected: PASS。失敗する場合は `docs/todo/BACKLOG.md` を変更せず、失敗テスト名とエラー内容を記録してユーザーに確認する。

Result: PASS。`server::state` は 22 passed。

- [x] **Step 3: 根拠となる実装とテストを確認する**

Run:

```bash
rg -n "metadata_for_mode|ensure_canonical_file|ensure_canonical_directory|test_ensure_canonical_file_metadata失敗はnotfileへ集約する|test_ensure_canonical_directory_metadata失敗はnotdirectoryへ集約する" src/server/state.rs
```

Expected: 次の根拠が表示される。

```text
metadata_for_mode
ensure_canonical_file
ensure_canonical_directory
test_ensure_canonical_file_metadata失敗はnotfileへ集約する
test_ensure_canonical_directory_metadata失敗はnotdirectoryへ集約する
```

Result: `metadata_for_mode`、`ensure_canonical_file`、`ensure_canonical_directory`、metadata 失敗を `NotFile` / `NotDirectory` へ集約する unit test が存在することを確認した。

- [x] **Step 4: `docs/todo/BACKLOG.md` から P2 未完了項目を削除する**

In `docs/todo/BACKLOG.md`, remove the entire AppMode TOCTOU unfinished item block from `## P2: 保守性・局所回帰検知`.

Result: P2 から AppMode TOCTOU 未完了項目を削除した。

- [x] **Step 5: `docs/todo/BACKLOG.md` の Done 先頭に完了根拠を追加する**

In `docs/todo/BACKLOG.md`, insert this block immediately after `## Done` and before the existing `サイドバーの "Documents" fallback を日本語化` item:

```markdown
- [x] `AppMode` 構築時の `is_file()`/`is_dir()` 判定の TOCTOU を緩和する
  - 完了根拠: `src/server/state.rs` は `CanonicalPath::try_from_path()` で canonicalize した後、`ensure_canonical_file()` / `ensure_canonical_directory()` が `metadata_for_mode()` 経由で取得した `Metadata` の `file_type()` から file / directory を判定している。canonicalize 後に対象が消えた場合も `NotFile` / `NotDirectory` へ集約することを unit test で固定済み。`.md` 拡張子チェック、canonical path 保持、base_dir / single_file / directory の公開契約、Host/Origin 検証、HTML sanitize、CSP、path validation は変更していない

```

Result: `## Done` 直下に AppMode TOCTOU 完了根拠を追加した。

- [x] **Step 6: BACKLOG 上の未完了 P2 が期待どおり減ったことを確認する**

Run:

```bash
sed -n '1,120p' docs/todo/BACKLOG.md
```

Expected: `## P2: 保守性・局所回帰検知` の未完了項目は `ディレクトリ検索の allocation 削減を計測結果に基づいて検討する` だけになる。`## Done` の先頭に `AppMode` TOCTOU 完了根拠がある。

Result: P2 未完了項目は検索 allocation だけになり、Done 先頭に AppMode TOCTOU 完了根拠があることを確認した。

- [x] **Step 7: 文書内の根拠追跡を確認する**

Run:

```bash
rg -n "TOCTOU|metadata_for_mode|ensure_canonical_file|ensure_canonical_directory|NotFile|NotDirectory" docs/todo/BACKLOG.md src/server/state.rs
```

Expected: `docs/todo/BACKLOG.md` の Done 根拠と `src/server/state.rs` の helper / tests が表示される。未完了 P2 に `AppMode` TOCTOU 項目が残っていない。

Result: Done 根拠と `src/server/state.rs` の helper / tests が追跡でき、未完了 P2 に AppMode TOCTOU 項目が残っていないことを確認した。

- [x] **Step 8: プレースホルダーや曖昧語がないことを確認する**

Run:

```bash
rg -n "TB[D]|TO[D]O|未[定]|要[確]認" docs/superpowers/specs/2026-05-19-appmode-toctou-backlog-completion-design.md docs/superpowers/plans/2026-05-19-appmode-toctou-backlog-completion.md docs/todo/BACKLOG.md
```

Expected: 0 matches。既存文脈として意図的な hit がある場合は、今回追加した文言ではないことを確認して最終報告に残す。

Result: 3 hits。いずれも既存 `docs/todo/BACKLOG.md` 冒頭の `TODO.md` 参照のみで、今回追加した文言ではない。

- [x] **Step 9: repository 標準検証を実行する**

Run:

```bash
./verify.sh
```

Expected: PASS。失敗する場合は、今回の `BACKLOG.md` 更新と関係するかを切り分ける。関係がない既存失敗の場合は、失敗コマンドと代表エラーを最終報告に残す。

Result: PASS。`./verify.sh` は正常完了した。

- [x] **Step 10: 変更をコミットする**

Run:

```bash
git add docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-19-appmode-toctou-backlog-completion.md
git commit -m "docs: AppMode TOCTOU backlog項目を完了扱いにする"
```

Expected: commit が作成される。commit には `docs/todo/BACKLOG.md` とこの plan が含まれる。

Result: commit `2afbaeb` を作成済み。

## Self-Review

- Spec coverage: 承認済み spec のゴール、非ゴール、受け入れ条件、検証、セキュリティ考慮、影響範囲、ロールバックは Task 1 の各 step で扱い、実行済みとしてチェック済み。
- Placeholder scan: plan 内では unresolved placeholder を使わず、検出コマンド内の語は bracket pattern で自己一致を避けている。実行結果として 3 hits があったが、既存 `BACKLOG.md` 冒頭の `TODO.md` 参照のみで今回追加文ではないことを記録済み。
- Type consistency: コード変更なし。参照する関数名とテスト名は現行 `src/server/state.rs` の名前に一致している。
- Execution record: `cargo test --lib server::state`、根拠確認、placeholder scan、`./verify.sh`、commit 作成結果を各 step の `Result:` に記録済み。
