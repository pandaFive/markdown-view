# BACKLOG Importance Impact Reprioritization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Run git mutation steps only after the user has explicitly approved committing the reviewed diff.

**Goal:** `docs/todo/TODO.md` と `docs/todo/BACKLOG.md` を、重要度と将来影響度順に再配置し、重要項目を TODO High / Medium へ昇格する。

**Architecture:** docs-only の分類変更として実装する。`TODO.md` は実行優先候補、`BACKLOG.md` は低優先・長期候補という既存構造を維持し、項目本文の由来と判断文脈を失わない。

**Tech Stack:** Markdown, ripgrep, git diff

---

## File Structure

- Modify: `docs/todo/TODO.md`
  - High / Medium の空状態を、昇格項目で置き換える。
  - Done Summary は変更しない。
- Modify: `docs/todo/BACKLOG.md`
  - 昇格した項目を未完了一覧から除外する。
  - 残項目を P1 / P2 / P3 内で重要度と将来影響度順に並べ直す。
  - Done セクションは変更しない。
- Create: `docs/superpowers/specs/2026-05-09-backlog-importance-impact-reprioritization-design.md`
  - 分類基準と検証コマンドの根拠を残す。
- Create: `docs/superpowers/plans/2026-05-09-backlog-importance-impact-reprioritization.md`
  - 実行手順と検証手順を残す。

## Classification Target

### Move To TODO High Priority

1. `watcher の try_send で WatchEvent::Error を FileChanged と同列に破棄しない`
   - 理由: watcher error の破棄は監視不能や異常停止の silent failure に直結する。
2. `panic::catch_unwind の init 経路で init_tx 残存時に ThreadPanic を init 結果として送出`
   - 理由: 初期化 panic の詳細が失われ、監視開始失敗の観測性と復旧判断に直接響く。
3. `Windows メモ原子保存のエラー処理と retry 条件を細分化する`
   - 理由: メモ保存はデータ安全性に関わり、Windows 固有の共有違反や削除保留を潰すと復旧判断が弱くなる。
4. `/api/search のクエリ長ガードを routes.rs 側に追加する`
   - 理由: 極端に長い未信頼入力が tracing と検索処理に流れるため、resource exhaustion とログ観測性の境界に関わる。

### Move To TODO Medium Priority

1. `Host middleware 適用境界を RouteDefinitions marker から security layer helper へ強化する`
   - 理由: Host / CSP / security header の適用順を将来 route 追加時に読み違えにくくする設計負債対応。
2. `Host middleware 化後の低優先 follow-up を整理して追加検証する`
   - 理由: Host security boundary の検証網を厚くするが、主要 middleware 化は実装済みなので Medium に置く。
3. `SanitizedHtml から innerHTML までの信頼境界を設計メモ化する`
   - 理由: XSS 境界の契約明文化は複数機能の前提になるが、今回は設計メモ化であり直接の実装修正ではない。
4. `assets バンドルの sentinel 衝突回避テストを追加`
   - 理由: template 埋め込みの回帰検知基盤で、将来の asset 追加時の守り忘れを防ぐ。
5. `CSP/syntax_theme_css フォールバック CSS の副作用設計判断を doc 化`
   - 理由: CSP と fallback CSS の契約を明示し、将来の renderer/template 変更時の判断材料にする。

### Keep In BACKLOG P1

1. `未知言語コードブロックの silent fallback に警告ログを追加`
2. `BroadcastMessage::Update 系のシリアライズ失敗時の fallback JSON を整備する`
3. `read_route_memo の二重サイズチェックを単一化する`
4. `tokio::select! の cancel-safe 性をコメントで明記する`

### Keep In BACKLOG P2

1. `ディレクトリ検索のキャンセル境界と allocation 削減を検討する`
2. `AppMode 構築時の is_file()/is_dir() 判定の TOCTOU を緩和する`
3. `data-memo-file 属性を None 時にスキップする`
4. `log_path::canonicalize_status の毎回 syscall を削減する`

### Keep In BACKLOG P3

1. `WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する`
2. `サイドバーの "Documents" 文字列を i18n または日本語化`
3. `インラインブラウザJS の TS 化`
4. `catalog.rs のパス構築での Vec アロケーション削減`

## Task 1: Update TODO High / Medium

**Files:**
- Modify: `docs/todo/TODO.md`
- Reference: `docs/todo/BACKLOG.md`

- [ ] **Step 1: Inspect the current TODO headings**

Run:

```bash
sed -n '1,80p' docs/todo/TODO.md
```

Expected: `High Priority` and `Medium Priority` both say no unfinished items.

- [ ] **Step 1.5: Verify promoted claims against current files**

Run read-only checks before treating review-derived text as current fact:

```bash
rg -n "WATCHER_MESSAGE_BUFFER|send_watch_event|try_send|catch_unwind|init_tx|search_directory|Query|MoveFileExW|contentEl\\.innerHTML|memoPreviewEl\\.innerHTML" src tests
rg -n "contentEl\\.innerHTML|memoPreviewEl\\.innerHTML" src/template/assets/js
```

Expected: each promoted item still maps to current source files or the TODO wording explicitly says implementation must re-check the current code before fixing it.

- [ ] **Step 2: Replace the TODO intro and High / Medium empty sections**

Replace the top of `docs/todo/TODO.md` from `# TODO Issues` through the line before `## Done Summary` with this exact content:

```markdown
# TODO Issues

レビュー指摘・コードベース探索で検出した改善項目のうち、次に実行する **High / Medium** のみを優先度順に掲載する。Low 項目は [`BACKLOG.md`](./BACKLOG.md) を参照。

最終整理: 2026-05-09。重要度と将来影響度を基準に、`BACKLOG.md` から実行優先候補を昇格した。完了済みの長文履歴は本ファイル末尾の Done サマリに圧縮し、未完了項目だけを実行候補として残す。
レビュー由来の `現状` は作業候補として扱い、実装前に対象ファイル・行番号・現象を現行コードで再確認する。

## High Priority

放置するとセキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目。

- [ ] watcher の `try_send` で `WatchEvent::Error` を `FileChanged` と同列に破棄しない
  - ファイル: `src/watcher/runtime.rs` L19/L72/L111-127
  - 現状: `mpsc::channel(WATCHER_MESSAGE_BUFFER=32)` が満杯時、`FileChanged` も `Error` も同じ `try_send` 経路で破棄される。`WatchError::Init` / `ThreadPanic` を破棄するとフォアグラウンドが「監視が止まった理由」を失う
  - 対応: イベント種別で優先度を分け、`Error` 系は破棄せず `blocking_send` / 別チャネル / health state への latch などで foreground から取得可能にする。`try_send` 失敗時の `tracing::error!` は補助的な観測性強化として扱い、ログ追加だけでは完了扱いにしない
  - 昇格理由: watcher error の破棄は監視不能や異常停止の silent failure に直結するため High とする
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `panic::catch_unwind` の init 経路で `init_tx` 残存時に `ThreadPanic` を init 結果として送出
  - ファイル: `src/watcher/runtime.rs` L149-215/L243-247
  - 現状: debouncer 構築前に panic が起きた場合 `init_tx` が Some のまま `catch_unwind` を抜け、`await_watcher_init` が `Err(_)` 経路に落ちて「予期せず終了しました」とだけ表示される。`panic_detail` は受信前に終了するため使われない
  - 対応: panic 経路で `init_tx` がまだ Some なら `WatchError::thread_panic(...)` を init 結果として送る
  - 昇格理由: watcher 初期化失敗の詳細が失われる silent failure であり、監視開始可否の判断に直接響くため High とする
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] Windows メモ原子保存のエラー処理と retry 条件を細分化する
  - ファイル: `src/server/files/memo_fs.rs`
  - 現状: Windows の `MoveFileExW` 呼び出しは `spawn_blocking` 経由だが、`JoinError` は `ErrorKind::Other` に潰している。また tmp 作成 retry は `AlreadyExists` のみを対象にしており、Windows の共有違反・削除保留・ウイルス対策ソフトによる一時ロックを retry しない
  - 対応: `JoinError::is_panic()` / `is_cancelled()` を分けて `tracing::error!` に残す。Windows では `raw_os_error()` で sharing violation / delete pending 相当を判定し、短い retry 対象に含める。Windows CI または `cargo check --target x86_64-pc-windows-gnu` が通る環境で検証する
  - 昇格理由: メモ保存はデータ安全性に関わり、失敗理由を潰すと復旧判断が弱くなるため High とする
  - 由来: メモ原子保存 PR 3rd レビュー (2026-04-30)

- [ ] `/api/search` のクエリ長ガードを routes.rs 側に追加する
  - ファイル: `src/server/routes.rs` L298-318, `src/server/files/search.rs`
  - 現状: クエリ `q` を長さチェックせずに `search_directory` に渡す。極端に長い `q`（例: 1MB）が tracing にそのまま流れると無視できないコストになる
  - 対応: 1KB 程度の長さガードを `routes.rs` 側に追加し、超過時は 400 を返す。`search.rs` 内部にも防御を残す（depth in defense）
  - 昇格理由: 極端に長い未信頼入力が tracing と検索処理に流れるため、resource exhaustion とログ観測性の境界に関わる
  - 由来: アーキテクチャレビュー (2026-04-30)

## Medium Priority

すぐ重大事故ではないが、後続改修の前提、設計負債、検証基盤として効く項目。

- [ ] Host middleware 適用境界を `RouteDefinitions` marker から security layer helper へ強化する
  - ファイル: `src/server/routes.rs`, `tests/integration_test.rs`, `docs/superpowers/specs/2026-05-04-host-middleware-structure-observability-design.md`
  - 現状: PR #123 で `build_routes() -> RouteDefinitions` として route 定義と Host middleware 適用の境界を命名・可視性で明示した。ただし `RouteDefinitions(Router<Arc<AppState>>)` の中身は通常の `Router` なので、`build_routes()` 内に共通 `.layer(...)` を混ぜても型エラーにはならない。これは型による強制というより intent marker であり、構造契約の強制力は限定的
  - 対応: `apply_security_layers(routes, csp_header)` のような private helper へ Host middleware / security headers / CSP 適用を集約し、route 定義と layer 適用の呼び出し順をさらに読みやすくする。必要なら許可 Host / 不正 Host の route 横断テストを route 一覧 helper に寄せ、route 追加時にテスト対象へ自然に入る構造へ整理する
  - 昇格理由: Host / CSP / security header の適用順を将来 route 追加時に読み違えにくくする設計負債対応のため Medium とする
  - 由来: PR #123 レビュー follow-up (2026-05-04)

- [ ] Host middleware 化後の低優先 follow-up を整理して追加検証する
  - ファイル: `src/server/routes.rs`, `src/server/guards.rs`, `tests/integration_test.rs`, `docs/superpowers/specs/2026-05-02-host-middleware-guard-design.md`
  - 現状: PR #120 で Host 検証を router middleware へ集約し、主要 route の不正 Host 拒否、security headers、WS Host/Origin 経路の分離、大容量 PUT body の順序を固定した。一方、許可 Host の全 route smoke、malformed/missing/empty Host の middleware 統合テスト、WS Origin 拒否の error message assert、middleware warn ログへの URI path 追加、test helper 内 `axum::serve(...).unwrap()` の panic 観測性、CHANGELOG 相当の運用ドキュメント化は未対応
  - 対応: 追加する価値が高い順に、許可 Host 明示ループ、malformed/missing/empty Host の middleware 経路 403、WS Origin 拒否 message assert、warn ログへの `request.uri().path()` 追加を検討する。`axum::serve(...).unwrap()` は test helper の失敗文脈が分かる `expect(...)` へ寄せる。WS Host 拒否 message 変更は PR 本文には明記済みなので、必要になった時点で README か CHANGELOG 相当へ移す
  - 昇格理由: Host security boundary の検証網を厚くするが、主要 middleware 化は実装済みなので Medium とする
  - 由来: PR #120 再レビュー follow-up (2026-05-02)

- [ ] `SanitizedHtml` から `innerHTML` までの信頼境界を設計メモ化する
  - ファイル: `src/renderer/mod.rs`, `src/template/assets/js/content-renderer.js`, `src/template/assets/js/memo.js`, `README.md`
  - 現状: Rust 側は `SanitizedHtml` newtype、raw HTML 破棄、URL policy、CSP hash で XSS 境界を作っている。一方ブラウザ側は `contentEl.innerHTML = safeData.content` / `memoPreviewEl.innerHTML = data.html` を使うため、境界の正しさは「サーバー生成 HTML だけが入る」という暗黙契約に依存している
  - 対応: renderer の信頼境界、HTTP/WS JSON の `content`/`toc`/`html` フィールド、JS 側の `innerHTML` 使用許可条件を短い設計メモにまとめる。E2E hook やテスト用 expose が production 経路で任意 HTML を流し込まないことも確認項目に含める
  - 昇格理由: XSS 境界の契約明文化は複数機能の前提になるが、今回は設計メモ化であり直接の実装修正ではないため Medium とする
  - 由来: Unix 哲学レビュー (2026-04-30)

- [ ] assets バンドルの sentinel 衝突回避テストを追加
  - ファイル: `src/template/assets/css_bundle.rs` L19, `src/template/assets/inline_script.rs` L17-22
  - 現状: `TEMPLATE.replace("__DARK_THEME_VARS__", ...)` / `replace("__MAX_FILE_SIZE_MB__", ...)` のプレースホルダーは sentinel 衝突に脆弱。`include_str!` した CSS/JS 内に同文字列が無いことを保証するテストが無い
  - 対応: `#[cfg(test)] mod tests` で「include 対象ソースに sentinel 文字列が含まれない」アサートを追加。`MAX_FILE_SIZE / 1024 / 1024` の整数除算で 11MB → 10MB 表示の丸め事故が起きないかも境界テスト
  - 昇格理由: template 埋め込みの回帰検知基盤で、将来の asset 追加時の守り忘れを防ぐため Medium とする
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] CSP/syntax_theme_css フォールバック CSS の副作用設計判断を doc 化
  - ファイル: `src/renderer/mod.rs` L91-108, `src/template/assets.rs` L42-61
  - 現状: `syntax_theme_css` 失敗時に `highlight_disabled_notice_css()`（`body::before` グローバル CSS）を返し、`combined_css` に連結される。CSP ハッシュは fallback ベースで再計算されるため整合性は保たれるが、Markdown 側で `body::before` を期待する CSS が無いという暗黙前提がドキュメントに無い
  - 対応: `body::before` 衝突を許容しない旨を doc コメントに明記。または fallback CSS のセレクタを `.markdown-view-fallback-notice` 等の局所スコープに変更する
  - 昇格理由: CSP と fallback CSS の契約を明示し、将来の renderer/template 変更時の判断材料にするため Medium とする
  - 由来: アーキテクチャレビュー (2026-04-30)

```

- [ ] **Step 3: Verify TODO contains the expected number of unfinished items**

Run:

```bash
rg -n "^- \\[ \\]" docs/todo/TODO.md
```

Expected: 9 matches, with 4 under High Priority and 5 under Medium Priority.

## Task 2: Update BACKLOG Remaining Items

**Files:**
- Modify: `docs/todo/BACKLOG.md`
- Reference: `docs/todo/TODO.md`

- [ ] **Step 1: Remove items moved to TODO**

Delete these incomplete item blocks from `docs/todo/BACKLOG.md`:

```text
Host middleware 適用境界を `RouteDefinitions` marker から security layer helper へ強化する
Host middleware 化後の低優先 follow-up を整理して追加検証する
Windows メモ原子保存のエラー処理と retry 条件を細分化する
assets バンドルの sentinel 衝突回避テストを追加
watcher の `try_send` で `WatchEvent::Error` を `FileChanged` と同列に破棄しない
`SanitizedHtml` から `innerHTML` までの信頼境界を設計メモ化する
`/api/search` のクエリ長ガードを routes.rs 側に追加する
`panic::catch_unwind` の init 経路で `init_tx` 残存時に `ThreadPanic` を init 結果として送出
CSP/syntax_theme_css フォールバック CSS の副作用設計判断を doc 化
```

- [ ] **Step 2: Replace the BACKLOG introduction and remaining P1/P2/P3 sections**

Replace the `docs/todo/BACKLOG.md` content from `# Backlog (Low Priority)` through the line before `## Done` with this exact content:

```markdown
# Backlog (Low Priority)

低優先度で蓄積している項目。High/Medium は [`TODO.md`](./TODO.md) に置き、ここには低優先・長期改善・完了済みの履歴を置く。
未完了項目は重要度と将来影響度を基準に P1/P2/P3 へ分類する。各項目末尾の「由来」は TODO.md 再編時（2026-04-21）以降の発見コンテキスト。

最終整理: 2026-05-09。セキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目は `TODO.md` へ昇格した。ここには昇格しないが文脈を残すべき候補を置く。
レビュー由来の `現状` は作業候補として扱い、実装前に対象ファイル・行番号・現象を現行コードで再確認する。

## P1: リスク低減・契約明文化

- [ ] 未知言語コードブロックの silent fallback に警告ログを追加
  - ファイル: `src/renderer/highlight.rs` L14-50, `tests/renderer_test.rs` L346
  - 現状: `find_syntax_by_token().or_else(find_syntax_by_extension())?` が None を返すと `plain_code_block_html` で `class="language-{lang}"` だけ付与する fallback が走るが、ユーザーに「ハイライトが効いていない」ことを知らせる経路がない。`tests/renderer_test.rs:346` `test_未知言語コードブロックはフォールバック描画される` で仕様固定済み
  - 対応: 初回フォールバック時に `tracing::debug!` 程度のログを 1 回だけ出す（同じ言語名の繰り返しは抑制）。CLI 起動時に「対応シンタックス一覧」コマンドで利用可能言語を確認できるドキュメント追加も検討
  - 判断: silent fallback の観測性改善だが、描画安全性は既存 fallback で保たれているため BACKLOG P1 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `BroadcastMessage::Update` 系のシリアライズ失敗時の fallback JSON を整備する
  - ファイル: `src/server/messages.rs` L46-56, `src/server/session.rs` L80-93
  - 現状: `serde_json::to_string(update)` の失敗は実質不可能だが、`session.rs` 側でエラー処理を持つ。Update メッセージ用の最小サイズ fallback (`{"content":"","toc":""}` 等) を返す `to_json_or_empty` 経路が無い
  - 対応: `BroadcastMessage::Update` の `to_json` に明示 fallback を追加。観測性として `tracing::error!` を残す
  - 判断: 実質不可能な失敗経路の契約整理であり、直接の実行時リスクは限定的なため BACKLOG P1 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `read_route_memo` の二重サイズチェックを単一化する
  - ファイル: `src/server/files/memo.rs` L455-481, `src/server/files/content.rs` L298-311
  - 現状: `fs.read_with_limit` が `MAX_FILE_SIZE+1` で `take` し超過時に `MemoReadError::TooLarge` を返すのに、`memo.rs:476-481` が読み込み完了後に `bytes.len() as u64 > MAX_FILE_SIZE` を再度チェックしている
  - 対応: `read_with_limit` の契約を doc コメントで明示し、呼び出し側の重複チェックを削除
  - 判断: メモ読込の契約明文化として価値は高いが、現状は防御が重複している状態なので BACKLOG P1 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `tokio::select!` の cancel-safe 性をコメントで明記する
  - ファイル: `src/server/session.rs` L54-132
  - 現状: `socket.recv()` と `rx.recv()` を `tokio::select!` で競わせているが、両者が cancel safe である根拠コメントが無い。将来の改修で cancel-unsafe な future を入れる事故リスク
  - 対応: 各 branch の future が cancel safe であることを doc コメントで明記し、新規 branch 追加時のチェックリストを残す
  - 判断: 将来の WebSocket 改修時の守りとして重要だが、現行 branch は cancel-safe なため BACKLOG P1 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

## P2: 保守性・局所回帰検知

- [ ] ディレクトリ検索のキャンセル境界と allocation 削減を検討する
  - ファイル: `src/server/files/search.rs`, `src/template/assets/js/directory-search.js`
  - 現状: ディレクトリ検索は `spawn_blocking` に隔離され、結果数・ファイル数・総読込 byte 数の打ち切りも明示されている。一方、連続検索時に古い検索処理をキャンセルする仕組みはなく、`SearchResultItem` の `before/current/after` はマッチごとに `String` を確保する
  - 対応: クライアント検索世代とサーバ側処理の対応、古い検索結果の破棄、`Cow<str>` 化や検索ブロック処理の allocation 削減を、計測結果に基づいて検討する
  - 判断: 検索負荷制御は実装済みで、残件は効率化と古い結果の扱いなので BACKLOG P2 に残す
  - 由来: ディレクトリ検索 blocking 隔離の残余リスク (2026-05-04)

- [ ] `AppMode` 構築時の `is_file()`/`is_dir()` 判定の TOCTOU を緩和する
  - ファイル: `src/server/state.rs` L18-24/L112/L132
  - 現状: `CanonicalPath::try_from_path` で `canonicalize` した直後に `is_file()`/`is_dir()` で判定するが、両者の間に rename/unlink される race window がある。実害は起動時の `AppMode::new_*` のみで影響は小さい
  - 対応: `metadata` を一度取得してから `is_file`/`is_dir` を判定し、race window を縮める。`AppModeBuildError` のメッセージも metadata 起点に整理
  - 判断: path safety に関係するが起動時限定で影響が小さいため BACKLOG P2 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `data-memo-file` 属性を None 時にスキップする
  - ファイル: `src/template/page.rs` L43/L47, `src/template/message.rs` L9-11
  - 現状: `params.memo.file().unwrap_or_default()` で常に `data-memo-file=""`（空文字）を出力する。`UpdateMessage` の `#[serde(skip_serializing_if = "Option::is_none")]` と非対称
  - 対応: `data-memo-file` も None 時に属性ごとスキップする経路に変更し、bootstrap.js 側を「属性無し ⇒ memo 無し」と扱うよう揃える
  - 判断: template/message 契約の整合性改善であり、データ安全性への直接影響は限定的なため BACKLOG P2 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] `log_path::canonicalize_status` の毎回 syscall を削減する
  - ファイル: `src/server/log_path.rs` L52-65
  - 現状: ログ出力ごとに `path` と `base` を canonicalize する。warn/error 時のみ呼ばれるが、ログ storm 状況下では I/O が増える
  - 対応: `base` の canonicalize 結果を起動時に一度だけ算出してキャッシュし、ログ経路では path 側のみ canonicalize する。または `OnceLock` で base を保持
  - 判断: ログ storm 時の効率化であり、現行の安全性を弱めていないため BACKLOG P2 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

## P3: 長期改善・低緊急

- [ ] WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する
  - ファイル: `src/server/guards.rs`
  - 現状: PR #123 で Host middleware 後段に到達した Host 系 `WsOriginRejection` を `error!` ログとして観測できるようにした。個人向け localhost ツールとしてはログで十分だが、本格運用や継続監視を想定するなら、発生回数をメトリクスやカウンタとして扱う余地がある
  - 対応: 実運用で bypass 兆候を集計する必要が出た場合のみ、軽量なカウンタや structured logging 連携を検討する。現時点では依存追加やメトリクス基盤導入は YAGNI とする
  - 判断: 既に error ログがあり、メトリクス基盤は実運用要求が出てからでよいため BACKLOG P3 に残す
  - 由来: PR #123 レビュー follow-up (2026-05-04)

- [ ] サイドバーの "Documents" 文字列を i18n または日本語化
  - ファイル: `src/server/routes.rs` L33-41 (`sidebar_directory_name`)
  - 現状: `unwrap_or("Documents")` で英語固定。日本語 UI でも同名が出る
  - 対応: 日本語デフォルト（"ドキュメント"）にするか、ディレクトリ名取得失敗時のフォールバック挙動をコメントで明示
  - 判断: UI 文言の局所改善であり、安全性や後続設計への影響は小さいため BACKLOG P3 に残す
  - 由来: アーキテクチャレビュー (2026-04-30)

- [ ] インラインブラウザJS の TS 化
  - ファイル: `src/template/assets/js/{bootstrap,content,fetch,memo,selection,sidebar,websocket}.js`
  - 内容: Rust の `include_str!` でコンパイル時に埋め込まれる JS を TS で記述し、事前 tsc でビルドして `.js` 出力を `include_str!` 対象にする
  - 理由: ブラウザ側 JS は現在無型。ただし Rust ビルドパイプラインへの Node 依存追加が必要で、「Rust 単体ビルド」の明快さが崩れる
  - 判断: 型安全性の長期改善だが、Node 依存追加の設計判断が必要なため BACKLOG P3 に残す
  - 由来: E2E TypeScript 移行 PR レビュー (2026-04-20)

- [ ] `catalog.rs` のパス構築での Vec アロケーション削減
  - ファイル: `src/server/files/catalog.rs`
  - 現状: 相対パス構築で `components().map(...).collect::<Vec<_>>().join("/")` を使っている。上限 1000 件だが呼出あたり Vec アロケーションが発生する
  - 対応: 計測または必要性確認のうえ、イテレータ駆動で直接 String を構築する（`itertools::Itertools::join()` もしくは手書き fold）
  - 判断: マイクロ最適化であり、実装前に効果確認が必要なため BACKLOG P3 に残す
  - 由来: PR #59 探索 (2026-04-18)

```

- [ ] **Step 3: Verify moved items are not duplicated**

Run:

```bash
rg -n '^- \[ \].*(watcher の `try_send`|panic::catch_unwind|Windows メモ原子保存|/api/search|Host middleware 適用境界|Host middleware 化後|SanitizedHtml|assets バンドル|CSP/syntax_theme_css)' docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected: each moved item title appears only as an unfinished item in `docs/todo/TODO.md`; no moved item title appears as an unfinished item in `docs/todo/BACKLOG.md`.

## Task 3: Validate Documentation Structure

**Files:**
- Verify: `docs/todo/TODO.md`
- Verify: `docs/todo/BACKLOG.md`
- Verify: `docs/superpowers/specs/2026-05-09-backlog-importance-impact-reprioritization-design.md`
- Verify: `docs/superpowers/plans/2026-05-09-backlog-importance-impact-reprioritization.md`

- [ ] **Step 1: Count unfinished items by file**

Run:

```bash
rg -c "^- \\[ \\]" docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected:

```text
docs/todo/TODO.md:9
docs/todo/BACKLOG.md:12
```

Output order may vary; pass if `TODO.md` reports 9 and `BACKLOG.md` reports 12.

- [ ] **Step 2: Check priority headings**

Run:

```bash
rg -n "^## " docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected headings:

```text
docs/todo/TODO.md:## High Priority
docs/todo/TODO.md:## Medium Priority
docs/todo/TODO.md:## Done Summary
docs/todo/BACKLOG.md:## P1: リスク低減・契約明文化
docs/todo/BACKLOG.md:## P2: 保守性・局所回帰検知
docs/todo/BACKLOG.md:## P3: 長期改善・低緊急
docs/todo/BACKLOG.md:## Done
```

- [ ] **Step 3: Check security and silent-failure terms are represented**

Run:

```bash
rg -n "Host|Origin|CSP|sanitize|path|silent|watcher|atomic|memo|innerHTML|未信頼|データ安全性|監視不能" docs/todo/TODO.md docs/todo/BACKLOG.md
```

Expected: matches exist in both files, with the highest-impact watcher, memo, query-length, and Host/security-boundary items represented in `TODO.md`.

- [ ] **Step 4: Check unresolved placeholder wording**

Run:

```bash
rg -n "未[定]|要[確]認|あ[と]で|完了済みだが未[完]了" docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/specs/2026-05-09-backlog-importance-impact-reprioritization-design.md docs/superpowers/plans/2026-05-09-backlog-importance-impact-reprioritization.md
```

Expected: no matches.

- [ ] **Step 5: Check whitespace and reviewed-claim guard**

Run:

```bash
git diff --check -- docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/specs/2026-05-09-backlog-importance-impact-reprioritization-design.md docs/superpowers/plans/2026-05-09-backlog-importance-impact-reprioritization.md
rg -n 'レビュー由来の `現状`' docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/plans/2026-05-09-backlog-importance-impact-reprioritization.md
```

Expected: `git diff --check` has no output. The reviewed-claim guard exists in `TODO.md`, `BACKLOG.md`, and the exact replacement blocks in this plan.

- [ ] **Step 6: Review the diff**

Run:

```bash
git diff -- docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/specs/2026-05-09-backlog-importance-impact-reprioritization-design.md docs/superpowers/plans/2026-05-09-backlog-importance-impact-reprioritization.md
```

Expected: diff only changes priority placement and explanatory text in TODO/BACKLOG plus the supporting Superpowers spec/plan docs. It does not modify source code, tests, config, or Done Summary content.

## Task 4: Commit The Reprioritization After User Approval

**Files:**
- Modify: `docs/todo/TODO.md`
- Modify: `docs/todo/BACKLOG.md`
- Create: `docs/superpowers/specs/2026-05-09-backlog-importance-impact-reprioritization-design.md`
- Create: `docs/superpowers/plans/2026-05-09-backlog-importance-impact-reprioritization.md`

Run this task only after the user explicitly approves committing the reviewed docs-only diff.

- [ ] **Step 1: Confirm worktree status**

Run:

```bash
git status --short --branch
```

Expected: only the TODO/BACKLOG changes plus the new Superpowers spec/plan files are present, unless the spec/plan files have already been committed in earlier approved commits.

- [ ] **Step 2: Stage the docs**

Run:

```bash
git add docs/todo/TODO.md docs/todo/BACKLOG.md docs/superpowers/specs/2026-05-09-backlog-importance-impact-reprioritization-design.md docs/superpowers/plans/2026-05-09-backlog-importance-impact-reprioritization.md
```

Expected: command succeeds with no output.

- [ ] **Step 3: Check staged diff**

Run:

```bash
git diff --cached --check
git diff --cached --stat
```

Expected: `git diff --cached --check` has no output. `git diff --cached --stat` shows only the TODO/BACKLOG docs and the Superpowers spec/plan files, unless the spec/plan files were committed earlier.

- [ ] **Step 4: Commit**

Run:

```bash
git commit -m "docs: TODOとBACKLOGを重要度順に再整理"
```

Expected: commit succeeds and reports changes to `docs/todo/TODO.md` and `docs/todo/BACKLOG.md`. If spec/plan files were not committed earlier, the commit also reports those two files.

- [ ] **Step 5: Confirm clean status**

Run:

```bash
git status --short --branch
```

Expected: clean worktree on `docs/backlog-importance-impact`.
