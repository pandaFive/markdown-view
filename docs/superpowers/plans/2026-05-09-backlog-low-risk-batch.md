# BACKLOG Low-Risk Batch Implementation Plan

> **履歴資料:** この plan は 2026-05-09 に実行済みの作業計画です。現在のユーザー指示、`AGENTS.md`、明示承認なしに再実行しないでください。チェックボックスとコマンドは当時の計画記録であり、現在の進捗や実行指示ではありません。
>
> **Original agentic-worker note:** This plan originally required `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` for task-by-task execution.

**Goal:** `docs/todo/BACKLOG.md` の低リスク項目を、docs 整理と挙動互換の小さなコード契約整理として一括消化する。

**Architecture:** docs は現行の `src/server/service.rs`、`src/renderer/`、`src/template/` 構成に追従させ、Markdown workspace というプロダクト定義に揃える。コード変更は public surface と将来 variant 追加時の判断漏れを狭めるだけに留め、HTTP API、WebSocket payload、HTML 出力仕様は変えない。

**Tech Stack:** Rust 2021、axum、pulldown-cmark、syntect、Markdown docs、`./verify.sh`。

---

## File Structure

- Modify: `README.md`
  - 冒頭説明を Markdown workspace に寄せる。
  - メモ、引用、検索、ファイルツリーを中核機能として記載する。
  - セキュリティ説明は既存より弱めない。
- Modify: `CLAUDE.md`
  - プロジェクト概要、アーキテクチャ図、データフロー、見出しパース説明を現行実装へ合わせる。
  - `service.rs`、`watcher/`、`renderer/`、`template/assets/` の分割を反映する。
- Modify: `Cargo.toml`
  - `description` を Markdown workspace 寄りにする。
- Modify: `src/server/files/resolve.rs`
  - `RouteTargetKind::include_file_list` を `match` 完全列挙へ変更する。
- Modify: `src/renderer/mod.rs`
  - `extract_headings("")` の早期 return を追加する。
- Modify: `src/server.rs`
  - `CanonicalPathError` の re-export を `pub(crate)` に絞る。
- Modify: `docs/todo/BACKLOG.md`
  - 完了した対象項目を `Done` へ移動し、完了根拠とセキュリティ上の残余判断を残す。

## Task 1: Docs とプロダクト定義を現行構成へ揃える

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `Cargo.toml`

- [ ] **Step 1: 現行 docs の古い表現を確認する**

Run:

```bash
rg -n "軽量・高速な Markdown プレビューア|軽量・高速 Markdown プレビューア|server/files.rs|watcher.rs|template/mod.rs|見出しパースが2回|service.rs|Markdown workspace" README.md CLAUDE.md Cargo.toml
```

Expected: README / CLAUDE / Cargo に previewer 寄りの説明や古い構成記述が表示される。

- [ ] **Step 2: `README.md` の冒頭と特徴を更新する**

Edit `README.md`:

```markdown
# markdown-view

Markdown workspace for local reading, notes, search, and live preview（Rust製）。
Markdown ファイルの閲覧、横断検索、引用メモ、ファイルツリー、ライブ更新を localhost 上の単一バイナリで扱います。
```

Update the feature list so these bullets exist:

```markdown
- **ライブリロード** — ファイル保存時にブラウザが自動更新（WebSocket）
- **ディレクトリモード** — ディレクトリ指定でファイルツリー付き workspace を表示
- **横断検索** — workspace 内の Markdown をサーバー側の上限付き検索で横断
- **引用メモ** — 選択範囲への引用リンクと sidecar メモを保存
- **目次サイドバー** — 見出しから自動生成、スクロール追従
- **シンタックスハイライト** — syntect による多言語対応コードハイライト
- **セキュア設計** — localhost限定バインド、Host/Origin検証、XSS防止、CSPヘッダー
- **ゼロ設定** — 外部ファイル不要、単一バイナリで完結
```

Do not remove the existing security section. Keep the existing localhost, DNS rebinding, XSS, CSP, traversal, and size-limit bullets.

- [ ] **Step 3: `Cargo.toml` の description を更新する**

Edit `Cargo.toml`:

```toml
description = "Markdown workspace for local preview, notes, and search"
```

- [ ] **Step 4: `CLAUDE.md` の概要とアーキテクチャを更新する**

Replace the project overview with:

```markdown
Markdown ファイルの閲覧、横断検索、引用メモ、ファイルツリー、ライブ更新を扱う localhost 専用 Markdown workspace（Rust製）。
ファイル変更を検知して WebSocket 経由でブラウザに即座に反映し、メモ sidecar と検索 API を同じ workspace 境界内で扱う。
```

Replace the architecture tree with the current module layout:

```markdown
main.rs  ── CLI引数パース → バリデーション → サーバー起動
  │
  ├── cli.rs        CLIオプション定義（clap derive）
  ├── server.rs     公開ファサード（モジュール再エクスポート）
  │   ├── state.rs      AppState / AppMode / CanonicalPath
  │   ├── routes.rs     axumルーター、HTTP/WebSocket adapter
  │   ├── service.rs    ページ/本文/メモ/検索の application service
  │   ├── files/        ファイル探索、検証、読み込み、メモ保存、検索
  │   ├── guards.rs     Host/Origin検証、CSP/セキュリティヘッダー
  │   ├── messages.rs   API / WebSocket メッセージ型
  │   ├── broadcast.rs  変更通知ブロードキャスト
  │   ├── session.rs    WebSocket セッション管理
  │   ├── watch.rs      watcher からの変更イベント処理
  │   └── log_path.rs   ログ出力用パスの相対化
  ├── renderer/     Markdown描画モジュール
  │   ├── render.rs     pulldown-cmark event の描画
  │   ├── state.rs      レンダリング状態
  │   ├── line.rs       ソース行属性
  │   ├── security.rs   URL / HTML sanitize
  │   ├── highlight.rs  syntect コードハイライト
  │   └── toc.rs        TOC HTML 生成
  ├── template/     HTMLページ、UpdateMessage、ファイルツリー、埋め込み assets
  │   ├── page.rs
  │   ├── message.rs
  │   ├── tree.rs
  │   └── assets/
  └── watcher/      notify + debouncer → tokio bridge
```

Update the heading parse note:

```markdown
- **見出し情報共有**: `render_document` は本文 HTML と TOC を同じ `HeadingInfo` から生成する。互換 API の `extract_headings` と検索用 Markdown profile は用途別に別走査する。
```

- [ ] **Step 5: docs 整合を確認する**

Run:

```bash
rg -n "server/files.rs|watcher.rs|template/mod.rs|見出しパースが2回|軽量・高速な Markdown プレビューア|軽量・高速 Markdown プレビューア" README.md CLAUDE.md Cargo.toml
```

Expected: no output.

- [ ] **Step 6: docs 変更をコミットする**

Run:

```bash
git add README.md CLAUDE.md Cargo.toml
git commit -m "docs: Markdown workspace定義へ更新"
```

Expected: commit succeeds.

## Task 2: 小さなコード契約を整理する

**Files:**
- Modify: `src/server/files/resolve.rs`
- Modify: `src/renderer/mod.rs`
- Modify: `src/server.rs`
- Test: `tests/renderer_test.rs`

- [ ] **Step 1: `extract_headings` の空入力テストを追加する**

Add this test near the existing `extract_headings` test in `tests/renderer_test.rs`:

```rust
#[test]
fn test_extract_headingsは空入力で空配列を返す() {
    assert!(extract_headings("").is_empty());
}
```

- [ ] **Step 2: targeted test を実行して現状の互換動作を確認する**

Run:

```bash
cargo test --test renderer_test test_extract_headingsは空入力で空配列を返す
```

Expected: PASS. This confirms the behavior already exists before implementation; the next change is an explicit fast path, not a behavior change.

- [ ] **Step 3: `extract_headings` に早期 return を追加する**

Change `src/renderer/mod.rs`:

```rust
pub fn extract_headings(input: &str) -> Vec<HeadingInfo> {
    if input.is_empty() {
        return Vec::new();
    }

    render_document(input).headings
}
```

- [ ] **Step 4: `RouteTargetKind::include_file_list` を完全列挙にする**

Change `src/server/files/resolve.rs`:

```rust
fn include_file_list(self) -> bool {
    match self.kind {
        RouteTargetKind::Page => true,
        RouteTargetKind::ApiContent | RouteTargetKind::ApiMemo => false,
    }
}
```

- [ ] **Step 5: `CanonicalPathError` の re-export を crate 内へ絞る**

Change `src/server.rs`:

```rust
pub(crate) use self::state::{CanonicalPath, CanonicalPathError};
pub use self::state::{AppMode, AppModeBuildError, AppState};
```

Keep `AppModeBuildError` public because `AppMode::new_single_file` and `AppMode::new_directory` expose it in public signatures.

- [ ] **Step 6: Rust 検証を実行する**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Expected: all commands PASS.

- [ ] **Step 7: コード契約整理をコミットする**

Run:

```bash
git add src/server/files/resolve.rs src/renderer/mod.rs src/server.rs tests/renderer_test.rs
git commit -m "refactor: 小さな公開境界と列挙契約を整理"
```

Expected: commit succeeds.

## Task 3: Superpowers 保存方針と BACKLOG を整理する

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Modify or Create: `docs/superpowers/README.md`

- [ ] **Step 1: Superpowers docs 保存方針ファイルを確認する**

Run:

```bash
test -f docs/superpowers/README.md && sed -n '1,200p' docs/superpowers/README.md || true
```

Expected: file content is printed if it exists; no output if absent.

- [ ] **Step 2: `docs/superpowers/README.md` を作成または更新する**

Ensure this content exists in `docs/superpowers/README.md`:

```markdown
# Superpowers Artifacts

このディレクトリは、設計判断と実装計画を保存する。

## `specs/`

長期参照する設計判断を置く。目的、非目的、受け入れ基準、セキュリティ考慮、影響範囲、ロールバック方針を含める。

## `plans/`

実装前の作業計画を置く。実行チェックリスト、想定コマンド、コミット単位を含むため、完了後は実行ログではなく「当時の計画」として扱う。

完了済み plan を残す基準:

- 後続作業で分割判断や検証方針を参照する価値がある。
- spec だけでは実装順序や検証コマンドの意図が分からない。
- 長すぎる実行ログになっている場合は、spec または `docs/todo/` 側に要点を残して plan を圧縮または削除する。
```

- [ ] **Step 3: `docs/todo/BACKLOG.md` の対象項目を Done へ移動する**

Move these items from P2/P3 to `## Done`:

```markdown
- [x] Superpowers spec/plan の長期保存方針を整理する
  - ファイル: `docs/superpowers/README.md`
  - 内容: `specs/` は長期参照する設計判断、`plans/` は実装前計画として扱う方針を明文化した。完了済み plan は参照価値がある場合に残し、実行ログとして rot する場合は要点化する基準を追加した。
  - 完了根拠: `docs/superpowers/README.md` の保存方針

- [x] CLAUDE.md のアーキテクチャ記述を現在の実装構成に揃える
  - ファイル: `CLAUDE.md`
  - 内容: `server/service.rs`、`watcher/`、`renderer/`、`template/assets/` を含む現行構成へ更新し、見出し情報共有の説明を `render_document` 起点に修正した。
  - 完了根拠: `CLAUDE.md` のアーキテクチャ図と設計判断

- [x] プロダクト定義を Markdown previewer から Markdown workspace へ明文化する
  - ファイル: `README.md`, `CLAUDE.md`, `Cargo.toml`
  - 内容: メモ、引用、横断検索、ファイルツリー、ライブ更新を Markdown workspace の中核機能として説明した。純プレビュー化や `--no-memo` は今回も非目標として扱い、既存セキュリティ説明は弱めていない。
  - 完了根拠: README 冒頭、特徴一覧、Cargo description、CLAUDE.md 概要

- [x] `render_markdown` と `extract_headings` の早期 return 非対称を解消する
  - ファイル: `src/renderer/mod.rs`, `tests/renderer_test.rs`
  - 内容: `extract_headings("")` を明示的な早期 return にし、空入力が空配列を返す契約をテストで固定した。
  - 完了根拠: `test_extract_headingsは空入力で空配列を返す`

- [x] `CanonicalPathError` などの内部利用型を `pub(crate)` に絞る
  - ファイル: `src/server.rs`
  - 内容: `CanonicalPathError` の re-export を crate 内へ絞った。`AppModeBuildError` は public constructor の戻り値に含まれるため public のまま残した。
  - 完了根拠: `cargo test --all-targets --all-features`

- [x] `RouteTargetKind::include_file_list` を match 完全列挙に変更する
  - ファイル: `src/server/files/resolve.rs`
  - 内容: `Page` / `ApiContent` / `ApiMemo` を `match` で完全列挙し、新 variant 追加時に file list 要否を見直す構造にした。
  - 完了根拠: `cargo test --all-targets --all-features`
```

- [ ] **Step 4: BACKLOG から対象未完了項目が消えたことを確認する**

Run:

```bash
rg -n "Superpowers spec/plan|CLAUDE.md のアーキテクチャ|Markdown previewer|早期 return 非対称|内部利用型を `pub\\(crate\\)`|include_file_list" docs/todo/BACKLOG.md
```

Expected: matching lines are under `## Done`, not under P1/P2/P3 unchecked sections.

- [ ] **Step 5: docs placeholder と古い表現を確認する**

Run:

```bash
rg -n "TB[D]|TO[DO]|未" docs/superpowers/README.md docs/todo/BACKLOG.md README.md CLAUDE.md
rg -n "server/files.rs|watcher.rs|template/mod.rs|見出しパースが2回|軽量・高速な Markdown プレビューア|軽量・高速 Markdown プレビューア" README.md CLAUDE.md Cargo.toml docs/todo/BACKLOG.md
```

Expected: first command has no output except unrelated historical backlog headings if present; second command has no output.

- [ ] **Step 6: full verification を実行する**

Run:

```bash
./verify.sh
```

Expected: PASS.

- [ ] **Step 7: BACKLOG 整理をコミットする**

Run:

```bash
git add docs/todo/BACKLOG.md docs/superpowers/README.md
git commit -m "docs: BACKLOG低リスク項目を整理"
```

Expected: commit succeeds.

## Final Verification

- [ ] **Step 1: clean status を確認する**

Run:

```bash
git status --short --branch
```

Expected: branch is `docs/backlog-low-risk-batch` and working tree is clean.

- [ ] **Step 2: 変更ファイル一覧を確認する**

Run:

```bash
git diff --stat develop...HEAD
```

Expected: only the design spec, plan, docs, targeted Rust files, and targeted test file are listed.
