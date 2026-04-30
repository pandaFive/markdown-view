# BACKLOG Easy Items Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/todo/BACKLOG.md` の実装済み P2 項目を Done へ整理し、`README.md` のアーキテクチャ図を現行構成へ合わせる。

**Architecture:** コード変更なしの docs-only 作業とする。BACKLOG は完了根拠を残して候補リストの精度を上げ、README は `src/server/`, `src/renderer/`, `src/template/` の分割済み構成を読者が追える図へ更新する。

**Tech Stack:** Markdown, ripgrep, existing `./verify.sh`

---

## File Structure

- Modify: `docs/todo/BACKLOG.md`
  - P2 の実装済み 3 項目を未完了欄から外し、`Done` へ移動する。
  - 完了根拠として該当コミットまたは現行テスト名を記録する。
- Modify: `README.md`
  - 古い単一ファイル前提のアーキテクチャ図を、現行 `src/` 配下の分割構成に合わせる。
  - セキュリティ説明は既存方針を維持し、実装より強い保証を書かない。
- Existing: `docs/superpowers/specs/2026-04-30-backlog-easy-items-design.md`
  - 実装判断の根拠として参照する。変更しない。

## Task 1: BACKLOG の P2 完了整理

**Files:**
- Modify: `docs/todo/BACKLOG.md`

- [x] **Step 1: P2 の現状を確認する**

Run:

```bash
rg -n "## P2|## P3|augmentHashWithTrailingLineHint|TOC pending navigation|Codex review ID" docs/todo/BACKLOG.md
```

Expected:

- P2 に 3 件の未完了項目がある。
- 3 件は `tests/e2e/memo_jump.spec.ts` と `tests/e2e/text_selection_defer.spec.ts` の現行内容で対応済みと確認できる。

- [x] **Step 2: 対応済み項目を Done へ移動する**

Edit `docs/todo/BACKLOG.md`:

- `## P2: 保守性・局所回帰検知` の本文を `現時点で未完了項目なし。` にする。
- 移動する 3 件を `## Done` 先頭へ追加する。
- 各項目に `完了根拠` を追加する。

Expected Done entries:

```markdown
- [x] `augmentHashWithTrailingLineHint` ELEMENT_NODE sibling の直接回帰テスト
  - ファイル: `tests/e2e/memo_jump.spec.ts`
  - 内容: `document.createElement('span')` で `L15` を内包したノードを `link.nextSibling` に置き、`augmentHashWithTrailingLineHint(link, '#section-b')` が `#section-b:L15` を返すことを直接検証した
  - 完了根拠: `7185d24 test: ELEMENT_NODE sibling の hash 補完を固定 (#102)`、現行 `tests/e2e/memo_jump.spec.ts` の `augmentHashWithTrailingLineHint は ELEMENT_NODE sibling の textContent から行番号を補完する`
  - 由来: PR #76 レビュー (2026-04-20)

- [x] TOC pending navigation 小揺らしテストの grace 内外分離
  - ファイル: `tests/e2e/text_selection_defer.spec.ts`
  - 内容: slack 内では pending active を維持し、slack 外では通常判定へ戻る境界を分離して検証した
  - 完了根拠: `ff0fbe8 test: TOC pending navigation境界を固定 (#103)`、現行 `tests/e2e/text_selection_defer.spec.ts` の `目次クリック後のslack内スクロールではpending activeを維持し、slack外では通常判定へ戻る`
  - 由来: TOC pending navigation PR 再レビュー (2026-04-28)

- [x] `memo_jump.spec.ts` の Codex review ID コメント削除
  - ファイル: `tests/e2e/memo_jump.spec.ts`
  - 内容: 外部 review system の ID 参照を残さず、回帰保護の対象である false-positive パターンの説明へ置き換え済み
  - 完了根拠: 現行 `tests/e2e/memo_jump.spec.ts` に `Codex review #4136142343` が存在せず、false-positive 系の仕様説明コメントがテスト内に残っている
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20、既存 PR #76 持ち込み課題)
```

- [x] **Step 3: BACKLOG の構造を確認する**

Run:

```bash
rg -n "## P2|現時点で未完了項目なし|augmentHashWithTrailingLineHint|TOC pending navigation|Codex review ID|完了根拠" docs/todo/BACKLOG.md
```

Expected:

- `## P2` 直下に `現時点で未完了項目なし。` がある。
- 移動した 3 件が `Done` に `[x]` として存在する。
- 各項目に `完了根拠` がある。

## Task 2: README アーキテクチャ図更新

**Files:**
- Modify: `README.md`

- [x] **Step 1: 現行ファイル構成を確認する**

Run:

```bash
rg --files src | sort
```

Expected:

- `src/server/`, `src/server/files/`, `src/renderer/`, `src/template/`, `src/template/assets/js/`, `src/template/assets/css/` が存在する。
- README の旧図にある `websocket.rs`, `renderer.rs`, `template.rs`, `files.rs` 単一ファイル前提は現行構成と一致しない。

- [x] **Step 2: README のアーキテクチャ図を更新する**

Edit `README.md` の `### アーキテクチャ` コードブロックを、次の構成へ置き換える。

````markdown
```
src/
  main.rs            CLI起動、サーバー初期化、ブラウザ起動
  lib.rs             ライブラリ公開境界
  cli.rs             clap による CLI オプション定義
  watcher/           notify + debouncer から tokio へ変更通知を橋渡し
  server.rs          server モジュールの公開ファサード
  server/
    state.rs         AppState / AppMode / CanonicalPath
    routes.rs        axum ルーター、HTTP API、WebSocket upgrade
    session.rs       WebSocket セッションと close code 送信
    broadcast.rs     ファイル変更通知の broadcast message 構築
    watch.rs         watcher からの変更イベント処理
    guards.rs        Host / Origin 検証、CSP / セキュリティヘッダー
    messages.rs      API / WebSocket メッセージ型、ファイルサイズ上限
    log_path.rs      ログ出力用パスの相対化
    files/
      catalog.rs     ディレクトリ内 Markdown 一覧
      content.rs     Markdown 読み込みと HTML / TOC 生成
      resolve.rs     パス解決と traversal 防止
      search.rs      ファイル検索
      memo*.rs       メモ sidecar の保存、名前生成、ファイル I/O
  renderer/          Markdown -> HTML 変換
    render.rs        pulldown-cmark event の描画
    state.rs         レンダリング状態
    line.rs          ソース行属性
    security.rs      URL / HTML sanitize
    highlight.rs     syntect によるコードハイライト
    toc.rs           Markdown -> 目次 HTML
  template/          HTML ページ、UpdateMessage、ファイルツリー、埋め込み assets
    assets/js/       ブラウザ側の更新、選択、メモ、サイドバー、WebSocket
    assets/css/      ページ / サイドバー / メモ / オーバーレイの CSS
```
````

- [x] **Step 3: README の関連説明を確認する**

Run:

```bash
rg -n "websocket.rs|renderer.rs|template.rs|files.rs|src/server/|src/renderer/|src/template/" README.md
```

Expected:

- 古い単一ファイル前提の `websocket.rs`, `renderer.rs`, `template.rs`, `files.rs` が残っていない。
- `src/server/`, `src/renderer/`, `src/template/` の現行構成が確認できる。

## Task 3: 検証とコミット

**Files:**
- Verify: `docs/todo/BACKLOG.md`
- Verify: `README.md`
- Verify: `docs/superpowers/plans/2026-04-30-backlog-easy-items.md`

- [x] **Step 1: プレースホルダーと対象語を確認する**

Run:

```bash
rg -n "TBD|TODO|未定|要確認" docs/todo/BACKLOG.md README.md docs/superpowers/plans/2026-04-30-backlog-easy-items.md
rg -n "augmentHashWithTrailingLineHint|TOC pending navigation|Codex review ID|src/server/|src/renderer/|src/template/" docs/todo/BACKLOG.md README.md
```

Expected:

- 1つ目のコマンドは、この実装で追加した未確定プレースホルダーを出さない。
- 2つ目のコマンドは、BACKLOG の完了根拠と README の現行構成を確認できる。

- [x] **Step 2: フル検証を実行する**

Run:

```bash
./verify.sh
```

Expected:

- format, clippy, Rust tests, TypeScript typecheck が pass する。

- [x] **Step 3: 差分を確認する**

Run:

```bash
git diff -- docs/todo/BACKLOG.md README.md docs/superpowers/plans/2026-04-30-backlog-easy-items.md
git status --short
```

Expected:

- 変更は `docs/todo/BACKLOG.md`, `README.md`, `docs/superpowers/plans/2026-04-30-backlog-easy-items.md` に限定される。
- プロダクションコード、テストコード、設定ファイルの差分がない。

- [x] **Step 4: コミットする**

Run:

```bash
git add docs/todo/BACKLOG.md README.md docs/superpowers/plans/2026-04-30-backlog-easy-items.md
git commit -m "docs: BACKLOG簡易項目を整理"
```

Expected:

- docs-only コミットが作成される。
