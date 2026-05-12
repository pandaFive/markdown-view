# TODO Issues

レビュー指摘・コードベース探索で検出した改善項目のうち、次に実行する **High / Medium** のみを優先度順に掲載する。Low 項目は [`BACKLOG.md`](./BACKLOG.md) を参照。

最終整理: 2026-05-09。重要度と将来影響度を基準に、`BACKLOG.md` から実行優先候補を昇格した。完了済みの長文履歴は本ファイル末尾の Done サマリに圧縮し、未完了項目だけを実行候補として残す。
レビュー由来の `現状` は作業候補として扱い、実装前に対象ファイル・行番号・現象を現行コードで再確認する。

## High Priority

放置するとセキュリティ境界、データ安全性、silent failure、監視不能に直接響く項目。

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

## Done Summary

- [x] Windows メモ原子保存のエラー処理と retry 条件を細分化する
  - 完了根拠: `MoveFileExW` の `JoinError` を panic / cancelled / その他 join error に分類してログと `io::Error` message に残す構成にした。Windows の `ERROR_ACCESS_DENIED`、`ERROR_SHARING_VIOLATION`、`ERROR_LOCK_VIOLATION` だけを一時 lock 系として扱い、初回失敗後に `10ms`、`25ms`、`50ms` の最大3回だけ同じ tmp/final path で retry する。retry は `MoveFileExW` 呼び出しだけに限定し、tmp 作成、本文書き込み、rename 前検証、cleanup、親ディレクトリ sync、HTTP API 契約は変更していない。retry 対象判定と JoinError 分類は unit test で固定した。Windows target check は `x86_64-w64-mingw32-gcc` 不足により完走できなかったため、Windows 実機/CI での確認は残リスクとして扱う
- [x] `panic::catch_unwind` の init 経路で `init_tx` 残存時に `ThreadPanic` を init 結果として送出
  - 完了根拠: watcher thread panic handler に `init_tx` を渡し、初期化結果送信前に panic した場合は `WatchError::thread_panic(...)` を `InitResult` の `Err` として返す構成にした。同じ panic detail は既存どおり内部 error channel への `WatchEvent::Error` 補助通知にも流し、health は `Failed(ThreadPanic)` に latch する。init 失敗時は `Watcher::spawn()` が `Err` を返すため、公開 receiver や WebSocket/client への配送は保証しない。`init_tx` が `None` の稼働後 panic と shutdown 中 panic は init result へ触れず、従来どおり health failed と error event で扱う。String payload と `anyhow::Error` payload の detail が init result と error event に保持されることを unit test で固定した。HTTP API、UI、WebSocket payload、shutdown API、watcher event shape は変更していない
- [x] watcher の `try_send` で `WatchEvent::Error` を `FileChanged` と同列に破棄しない
  - 完了根拠: watcher 内部の通常変更通知と異常通知を別 channel に分離し、外部 API は既存の `WatchEvent` receiver に再統合する構成にした。`FileChanged` は満杯時 best-effort で破棄する一方、`Error` は bounded ring queue 経由で file backlog から独立して配送される。専用 queue 自体が満杯の場合は OOM を避けるため最古の error を warn log に残して evict し、最新の error を保持する。内部 forwarder は error を優先して merged receiver へ流し、既存の server broadcast 契約と watcher health latch を維持した。Drop 経路は Tokio worker を同期 join で塞がず、未 shutdown drop は warn で明示する。外部 HTTP API、UI、WebSocket error payload は変更していない。
- [x] `RenderState` を `enum BlockContext` スタックに置き換えて open/close 対応を型化する
  - 完了根拠: `RenderState` は `Vec<BlockContext>` と `RenderStateMismatch` で Heading / CodeBlock / Image / Table の active context を扱う構成になった。`finish_*` は stack top だけを閉じ、wrong-top 時は context を保持して mismatch を返す。`render.rs` は mismatch を `warn!` して malformed context の HTML 確定を skip し、heading ID counter の副作用や code block line attrs の debug panic を回避する。通常 Markdown の見出し、コードブロック、画像、テーブル、複合入力の出力互換を既存・追加テストで固定した。残リスクとして、malformed event stream で table 内に `TableRowEnd` だけが来た場合の row-start 厳密追跡は未実装だが、通常 pulldown-cmark 経路では発生しないため次回 state machine 追加整理候補とする
- [x] `template/mod.rs` のテストをサブモジュールへ分割し、`render_page` の 62 行 `format!` を関数分割する
  - 完了根拠: `template` のテストを `page` / `tree` / `assets` / `message` へ局所化し、`mod.rs` は公開 API smoke test 中心へ戻した。`render_page` は `render_html_document` / `render_head` / `render_workspace_body` helper へ分割し、属性値 escape を `html_attr` に集約した。`SanitizedHtml` の本文 / TOC / memo preview は二重 escape せず、memo degraded、directory mode、message JSON 直列化、CSP hash、tree HTML escape の契約をテストで固定した
- [x] ブラウザ JS の責務境界を小モジュールへ分割する
  - 完了根拠: `content.js` の責務を `content-controller.js`、`content-enhancements.js`、`content-navigation.js`、`document-search.js`、`directory-search.js` に分割し、本文更新、検索、ディレクトリ検索、内部リンク解決、描画後副作用を明示境界へ分けた。`content-renderer.js` の sanitize 済み HTML 反映境界は維持し、検索 query と検索結果は DOM API で描画する構成にした。production `window` への内部 API 露出は増やさず、E2E hook は `window.__MV_E2E__ === true` の場合だけ公開する。検索、Markdown link、memo citation jump、update exposure の E2E で回帰を固定した
- [x] `BroadcastMessage::Refresh` の memo_refresh 契約と、メモ読込失敗時の degrade 通知を明示する
  - 完了根拠: `MemoResponse` に `memo_state` を追加し、通常時は `ready`、読込失敗時は `degraded` として直列化する契約にした。初期ページ描画ではメモ読込失敗を degraded response に変換し、メモパネルに専用バナーを表示して textarea と autosave を止める。ブラウザ側の `applyMemoData` も `memo_state: "degraded"` を主条件にし、読込失敗応答で編集中本文を上書きしない。`BroadcastMessage::Refresh` は `refresh: true` と `memo_refresh: true` を含む JSON へ揃え、refresh payload によるメモ再取得を E2E で固定した
- [x] watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する
  - 完了根拠: ディレクトリモードの監視登録を `WatchPlan` 経由にし、`.git`、`node_modules`、`target`、隠しディレクトリ、symlink directory を notify 登録前に除外する構成にした。起動後に作成された通常サブディレクトリは internal event loop で動的に `NonRecursive` watch へ追加し、既存 Markdown の回復通知も送る。watch 登録の部分成功は init failure とし、起動後の追加 watch 失敗は health failure と `WatchEvent::Error` に分類する。ENOSPC 相当は `WatchErrorKind::ResourceExhausted` として Linux inotify 上限の確認へ進める日本語メッセージを返す。レビュー反映で `.git`、`node_modules`、`target`、隠しディレクトリはプレビュー一覧・検索・直接表示からも共通除外し、README に既定除外と inotify 上限を明記した
- [x] shutdown チェーンの観測性を統合する
  - 完了根拠: watcher thread と forwarder task の shutdown timeout 秒数を共通化し、forwarder の最後のイベント種別と WebSocket receiver 数を診断 snapshot として記録する構成にした。forwarder の自然終了ログと timeout / abort 直前ログに `last_event_kind` / `receiver_count` / `elapsed_ms` / `timeout_secs` を含め、停止遅延時にログだけで切り分けられるようにした。HTTP API、WebSocket payload、UI 表示の外部契約は変更していない
- [x] watcher の `WatchEvent::Error` 後の健全性 API と atomic save 耐性 E2E を追加する
  - 完了根拠: `WatcherHealth` を追加し、`WatchService::health()` と `WatchService::is_alive()` から watcher 状態を内部 API として取得できるようにした。`ThreadPanic` と notify error は `Failed(...)` として分類され、`is_alive()` は `Alive` の場合だけ true を返す。単一ファイルモードとディレクトリモードの atomic save 相当の rename シーケンス後に WebSocket update を受け取る統合テストで固定した。HTTP API、UI、WebSocket エラー JSON の外部契約は増やしていない
- [x] `canonicalize` 失敗時の再帰挙動の非対称を解消する
  - 完了根拠: `catalog.rs` の通常ディレクトリとシンボリックリンクディレクトリの再帰可否判定を `resolve_recursable_directory` へ集約し、正規化、base 配下確認、metadata によるディレクトリ判定、visited 登録を同じ経路に揃えた。走査実体は検証済み canonical path を使い、表示用相対パスは symlink 名を維持する。`read_dir` 直前の再検証とログ path の制御文字 escape も追加した。base 外 symlink、隠し target symlink、通常ファイル symlink、symlink cycle、基本列挙、差し替え検出の回帰テストで固定している。filesystem race 全般の完全解消は非目標
- [x] `AppState` の Arc 二重ラップと `with_memo_fs` の API 整合を解消する
  - 完了根拠: `AppState` の共有単位を外側の `Arc<AppState>` に統一し、`with_memo_fs` を削除した。`MemoFs` は生成時注入の constructor へ寄せ、production 経路は Tokio 実装を使う構成になっている
- [x] `WsOriginRejection` ログ分類を完全列挙し、Host bypass 観測性テストを補強する
  - 完了根拠: `WsOriginRejection` のログ分類を wildcard なしの helper に分離し、Host 系 3 variant の traced log test とログ helper 経由の単一出力で固定した
- [x] 見出し ID 生成を単一パス化し render と toc で `HeadingInfo` を共有する
  - 完了根拠: `render_document` が同一 `headings` から本文と TOC を生成する構成になっている
- [x] Markdown 方言オプションを共通化し、表示・TOC・検索の差分を明示する
  - 完了根拠: 現行実装と関連テストで用途別 profile の差分が固定されている
- [x] 監視イベント経由の変更ファイルを最終読込前に再検証する
  - 完了根拠: watcher 経路が canonical base API と読込直前検証へ寄っている
- [x] Host 検証を router middleware 化して新規 route の守り忘れを防ぐ
  - 完了根拠: Host middleware が router 全体へ適用され、WS は Host + Origin の二段検証になっている
- [x] ディレクトリ検索の負荷制御をサーバ側に追加する
  - 完了根拠: 検索結果数・対象ファイル数・総読込 byte 数の打ち切りと blocking 隔離が実装済み
- [x] Host middleware の構造契約と WebSocket bypass 観測性を強化する
  - 完了根拠: PR #123 で `RouteDefinitions` private newtype、WS Host 系 rejection の `error!` ログ化、`/ws` 不正 Host の security headers 統合テスト、MissingHost の traced log test を追加した
- [x] async ハンドラ内の同期 I/O を `spawn_blocking` ないし起動時固定化で解消する
  - 完了根拠: catalog は canonical base API へ分離し、route target 解決・検索候補列挙・ファイル一覧取得は blocking 境界と canonical base API へ寄せた
- [x] `toc.rs` の `build_toc_html` で `level=0` インデックス OOB ガードを入れる
  - 完了根拠: `src/renderer/toc.rs` が `level` を `1..=current_level+1` に正規化し、`level=0` の境界テストを3件持つ
