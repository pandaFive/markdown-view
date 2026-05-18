# BACKLOG P3 Low Risk Batch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** BACKLOG P3 の低リスク2項目を実装し、WS Host bypass メトリクス項目には現時点で実装しない判断を明記する。

**Architecture:** `service.rs` はサイドバー fallback 文言を private 定数で管理し、既存の `sidebar_directory_name()` 境界を維持する。`catalog.rs` は相対パスの `/` 区切り変換だけを private helper に分離し、列挙・検証・除外ルールには触れない。`BACKLOG.md` は実装済み2項目を Done へ移し、メトリクス項目は未完了 P3 に残して判断根拠を追記する。

**Tech Stack:** Rust, axum service layer, std::path, tempfile, cargo test, repository-local `./verify.sh`.

---

## Files

- Modify: `src/server/service.rs`
  - 責務: ページ表示用 service、サイドバー表示モデル、検索とメモの service 境界。
  - 今回の変更: サイドバー fallback 定数 `SIDEBAR_DIRECTORY_FALLBACK_NAME` を追加し、`sidebar_directory_name()` の fallback を `"ドキュメント"` にする。既存の通常ディレクトリ名経路を維持する。
- Modify: `src/server/files/catalog.rs`
  - 責務: ディレクトリ配下の Markdown ファイル列挙、除外、canonicalize 再検証、表示用相対パス生成。
  - 今回の変更: `relative_path_to_slash_string()` を追加し、中間 `Vec` を使わず `/` 区切り文字列を構築する。列挙条件や security validation は変更しない。
- Modify: `docs/todo/BACKLOG.md`
  - 責務: 低優先・長期改善候補と判断根拠の保持。
  - 今回の変更: `"Documents"` fallback と `catalog.rs` allocation 項目を Done へ移す。WS Host bypass メトリクス項目は P3 に残し、既存 `error!` ログと `ws_rejection_class` / `host_recheck_anomaly` field で現時点は十分と判断したことを追記する。
- Reference: `docs/superpowers/specs/2026-05-19-backlog-p3-low-risk-batch-design.md`
  - 責務: 承認済み設計。実装時は参照専用として扱う。

## Task 1: サイドバー fallback 文言をテストで固定する

**Files:**
- Modify: `src/server/service.rs`

- [ ] **Step 1: 既存の service テストを実行して現状を確認する**

Run:

```bash
cargo test --lib server::service
```

Expected: PASS。既存失敗がある場合は実装前に失敗名とエラーを記録し、作業を止めてユーザーへ確認する。

- [ ] **Step 2: fallback 定数と失敗する test を追加する**

In `src/server/service.rs`, add this constant after the existing `use` block and before `#[derive(Debug, Clone, PartialEq, Eq)]`:

```rust
const SIDEBAR_DIRECTORY_FALLBACK_NAME: &str = "ドキュメント";
```

In `src/server/service.rs`, inside `#[cfg(test)] mod tests`, add these tests after `test_sidebar_view_single_fileを作れる`:

```rust
    #[cfg(unix)]
    #[test]
    fn test_sidebar_directory_name_fallbackは日本語名を返す() {
        let state = create_directory_state(std::path::Path::new("/"));

        assert_eq!(sidebar_directory_name(&state), "ドキュメント");
    }

    #[test]
    fn test_sidebar_directory_nameは通常ディレクトリ名を使う() {
        let dir = tempfile::tempdir().unwrap();
        let state = create_directory_state(dir.path());

        assert_eq!(
            sidebar_directory_name(&state),
            dir.path().file_name().unwrap().to_str().unwrap()
        );
    }
```

- [ ] **Step 3: fallback test が未実装挙動で失敗することを確認する**

Run:

```bash
cargo test --lib test_sidebar_directory_name_fallbackは日本語名を返す
```

Expected: FAIL。代表的な失敗は `assertion left == right failed` で、左辺が `"Documents"`、右辺が `"ドキュメント"` になる。

- [ ] **Step 4: `sidebar_directory_name()` の fallback を定数へ差し替える**

In `src/server/service.rs`, replace the current function body:

```rust
fn sidebar_directory_name(state: &AppState) -> &str {
    state
        .mode()
        .directory()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Documents")
}
```

with:

```rust
fn sidebar_directory_name(state: &AppState) -> &str {
    state
        .mode()
        .directory()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(SIDEBAR_DIRECTORY_FALLBACK_NAME)
}
```

- [ ] **Step 5: service テストが通ることを確認する**

Run:

```bash
cargo test --lib server::service
```

Expected: PASS。

- [ ] **Step 6: サイドバー fallback 変更をコミットする**

Run:

```bash
git add src/server/service.rs
git commit -m "fix: サイドバーfallback名を日本語化"
```

Expected: commit が作成される。

## Task 2: catalog の相対パス文字列化 helper を追加する

**Files:**
- Modify: `src/server/files/catalog.rs`

- [ ] **Step 1: 既存の catalog テストを実行して現状を確認する**

Run:

```bash
cargo test --lib server::files::tests::catalog
```

Expected: PASS。既存失敗がある場合は実装前に失敗名とエラーを記録し、作業を止めてユーザーへ確認する。

- [ ] **Step 2: helper の失敗する unit test を追加する**

At the end of `src/server/files/catalog.rs`, after `canonicalize_dir_for_cycle()`, add:

```rust
#[cfg(test)]
mod tests {
    use super::relative_path_to_slash_string;
    use std::path::Path;

    #[test]
    fn test_relative_path_to_slash_stringはネストしたpathをslash区切りにする() {
        let relative = Path::new("docs").join("guide").join("setup.md");

        assert_eq!(relative_path_to_slash_string(&relative), "docs/guide/setup.md");
    }

    #[test]
    fn test_relative_path_to_slash_stringは単一componentをそのまま返す() {
        assert_eq!(
            relative_path_to_slash_string(Path::new("README.md")),
            "README.md"
        );
    }
}
```

- [ ] **Step 3: helper 未実装で失敗することを確認する**

Run:

```bash
cargo test --lib relative_path_to_slash_string
```

Expected: FAIL。代表的な失敗は `unresolved import super::relative_path_to_slash_string`。

- [ ] **Step 4: `relative_path_to_slash_string()` を実装する**

In `src/server/files/catalog.rs`, add this helper before `fn list_markdown_files_recursive`:

```rust
fn relative_path_to_slash_string(relative: &Path) -> String {
    let mut output = String::new();
    for component in relative.components() {
        if !output.is_empty() {
            output.push('/');
        }
        output.push_str(&component.as_os_str().to_string_lossy());
    }
    output
}
```

- [ ] **Step 5: 既存の `Vec` + `join` 経路を helper に置き換える**

In `src/server/files/catalog.rs`, replace this block inside `list_markdown_files_recursive()`:

```rust
                    let relative_str = relative
                        .components()
                        .map(|component| component.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    files.push(relative_str);
```

with:

```rust
                    files.push(relative_path_to_slash_string(relative));
```

- [ ] **Step 6: helper test と catalog test が通ることを確認する**

Run:

```bash
cargo test --lib relative_path_to_slash_string
cargo test --lib server::files::tests::catalog
```

Expected: both PASS。

- [ ] **Step 7: `collect::<Vec<_>>().join(\"/\")` が catalog から消えたことを確認する**

Run:

```bash
rg -n "collect::<Vec<_>>\\(\\)\\.join\\(\"/\"\\)|relative_path_to_slash_string" src/server/files/catalog.rs
```

Expected: `relative_path_to_slash_string` の定義、呼び出し、test だけが表示される。`collect::<Vec<_>>().join("/")` は表示されない。

- [ ] **Step 8: catalog helper 変更をコミットする**

Run:

```bash
git add src/server/files/catalog.rs
git commit -m "refactor: catalog相対パス文字列化のallocationを削減"
```

Expected: commit が作成される。

## Task 3: BACKLOG を更新して判断を残す

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: BACKLOG の対象項目を確認する**

Run:

```bash
rg -n "WS Host middleware bypass|Documents|catalog.rs のパス構築" docs/todo/BACKLOG.md
```

Expected: P3 に3項目が表示される。

- [ ] **Step 2: WS Host bypass メトリクス項目の判断文を追記する**

In `docs/todo/BACKLOG.md`, under the `WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する` item, replace the `対応:` line:

```markdown
  - 対応: 実運用で bypass 兆候を集計する必要が出た場合のみ、軽量なカウンタや structured logging 連携を検討する。現時点では依存追加やメトリクス基盤導入は YAGNI とする
```

with:

```markdown
  - 対応: 実運用で bypass 兆候を継続集計する必要が出た場合のみ、軽量なカウンタや structured logging 連携を検討する。現時点では既存の `error!` ログ、`ws_rejection_class`、`host_recheck_anomaly` field で異常兆候を確認でき、依存追加やメトリクス基盤導入は YAGNI とする
```

- [ ] **Step 3: 実装済み2項目を Done へ移す**

In `docs/todo/BACKLOG.md`, remove these two unchecked P3 items from the unfinished list:

```markdown
- [ ] サイドバーの "Documents" 文字列を i18n または日本語化
  - ファイル: `src/server/routes.rs` L33-41 (`sidebar_directory_name`)
  - 現状: `unwrap_or("Documents")` で英語固定。日本語 UI でも同名が出る
  - 対応: 日本語デフォルト（"ドキュメント"）にするか、ディレクトリ名取得失敗時のフォールバック挙動をコメントで明示
  - 判断: UI 文言の局所改善であり、安全性や後続設計への影響は小さいため BACKLOG P3 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `catalog.rs` のパス構築での Vec アロケーション削減
  - ファイル: `src/server/files/catalog.rs`
  - 現状: 相対パス構築で `components().map(...).collect::<Vec<_>>().join("/")` を使っている。上限 1000 件だが呼出あたり Vec アロケーションが発生する
  - 対応: 計測または必要性確認のうえ、イテレータ駆動で直接 String を構築する（`itertools::Itertools::join()` もしくは手書き fold）
  - 判断: マイクロ最適化であり、実装前に効果確認が必要なため BACKLOG P3 に残す
  - 由来: PR #59 探索 (2026-04-18)
```

Then under the `## Done` heading, before the existing archive sentence, add:

```markdown
- [x] サイドバーの "Documents" fallback を日本語化
  - 完了根拠: `src/server/service.rs` の `sidebar_directory_name()` fallback を private 定数 `SIDEBAR_DIRECTORY_FALLBACK_NAME` 経由の `"ドキュメント"` に変更した。通常のディレクトリ名が取得できる場合は従来どおり実ディレクトリ名を使うことを unit test で固定した。i18n 基盤、UI 全体の文言、HTML sanitize、path validation は変更していない

- [x] `catalog.rs` の相対パス構築で中間 Vec allocation を避ける
  - 完了根拠: `src/server/files/catalog.rs` に `relative_path_to_slash_string()` を追加し、`components().map(...).collect::<Vec<_>>().join("/")` を使わずに `/` 区切り文字列を構築するようにした。ファイル列挙の sort、件数上限、除外ルール、canonicalize 再検証、symlink handling は変更していない。helper の単一 component とネスト path の出力を unit test で固定した
```

- [ ] **Step 4: BACKLOG 文言を検証する**

Run:

```bash
rg -n "Documents|ドキュメント|メトリクス|ws_rejection_class|host_recheck_anomaly|catalog.rs の相対パス" docs/todo/BACKLOG.md
```

Expected: `"ドキュメント"` の Done 根拠、WS Host bypass の判断追記、catalog Done 根拠が表示される。未完了項目として `"Documents"` fallback と `catalog.rs` Vec allocation は残らない。

- [ ] **Step 5: BACKLOG 更新をコミットする**

Run:

```bash
git add docs/todo/BACKLOG.md
git commit -m "docs: BACKLOG P3低リスク項目を整理"
```

Expected: commit が作成される。

## Task 4: 全体検証と最終確認

**Files:**
- Verify: `src/server/service.rs`
- Verify: `src/server/files/catalog.rs`
- Verify: `docs/todo/BACKLOG.md`

- [ ] **Step 1: format check を実行する**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS。失敗した場合は `cargo fmt --all` を実行し、整形差分を確認してから `git add` / `git commit -m "style: Rustコードを整形"` で整形のみをコミットする。

- [ ] **Step 2: targeted tests を実行する**

Run:

```bash
cargo test --lib server::service
cargo test --lib server::files::tests::catalog
cargo test --lib relative_path_to_slash_string
```

Expected: all PASS。

- [ ] **Step 3: full verification を実行する**

Run:

```bash
./verify.sh
```

Expected: PASS。`cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features` がすべて通る。

- [ ] **Step 4: security-sensitive strings と対象外変更を確認する**

Run:

```bash
git diff --stat develop...HEAD
rg -n "require_allowed_request_host|is_allowed_ws_origin|Content-Security-Policy|sanitize|resolve_recursable_directory|ensure_current_dir_still_canonical" src/server/service.rs src/server/files/catalog.rs src/server/guards.rs
```

Expected: diff stat は `src/server/service.rs`、`src/server/files/catalog.rs`、`docs/todo/BACKLOG.md` を中心に小さい。Host/Origin 検証、CSP、HTML sanitize、canonicalize 再検証関数の挙動変更は含まれない。

- [ ] **Step 5: final worktree status を確認する**

Run:

```bash
git status --short --branch
```

Expected: clean working tree on the feature branch, ahead of base by the new commits.
