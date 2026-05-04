# TODO Issues

レビュー指摘・コードベース探索で検出した改善項目のうち、次に実行する **High / Medium** のみを優先度順に掲載する。Low 項目は [`BACKLOG.md`](./BACKLOG.md) を参照。

最終整理: 2026-05-05。完了済みの長文履歴は本ファイル末尾の Done サマリに圧縮し、未完了項目だけを実行候補として残す。

## High Priority

現時点で未完了の High Priority はなし。

## Medium Priority

- [ ] `WsOriginRejection` ログ分類を完全列挙し、Host bypass 観測性テストを補強する
  - ファイル: `src/server/guards.rs`, `docs/todo/TODO.md`
  - 現状: PR #123 で Host 系 `WsOriginRejection::{MissingHost, HostMalformed, UntrustedHost}` を bypass 兆候として `error!` に上げ、MissingHost の traced log test を追加した。一方、`is_allowed_ws_origin()` の非 Host 系ログ分類は `_ => warn!()` に残っており、新しい rejection variant が追加された場合にコンパイラで分類漏れを検出できない。`is_host_middleware_bypass_indicator()` も `matches!` の false 側へ暗黙に落ちるため、variant 追加時の意図確認が弱い。HostMalformed / UntrustedHost の実ログ出力は helper 分類テストで間接的に守られているが、traced log test では直接固定していない
  - 対応: `WsOriginRejection` 全 variant を match で明示列挙し、Host 系 / MissingOrigin / その他 Origin 系の分類を compiler-enforced にする。可能なら `level_for_ws_rejection(rejection) -> tracing::Level` と `message_for_ws_rejection(rejection)` 相当の小 helper へ分け、`tracing::event!` でログ分岐を平坦化する。HostMalformed / UntrustedHost の traced log test も追加し、コメントは「middleware bypass、または Host 検証通過後の malformed/untrusted probe。通常運用では到達しない」に更新する
  - 理由: DNS Rebinding 防御の判定自体は変えずに、将来 variant 追加時の silent fallback とログ分類漏れをコンパイル時・テスト時に検出しやすくする

- [ ] watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する
  - ファイル: `src/watcher/strategy.rs` L43-48, `src/watcher/runtime.rs` L192-198
  - 現状: ディレクトリモードは `RecursiveMode::Recursive` を無条件で適用し、`is_hidden_relative` でイベント受信後にフィルタする。Linux 既定の `fs.inotify.max_user_watches` (8192) を `node_modules`/`target`/`.git` を含む大規模ツリーで枯渇させ、`ENOSPC` 時に `WatchError::init` がそのまま漏れる
  - 対応: `notify` に渡す前段で `.git`/`node_modules`/`target` を最低限除外する。`debouncer.watcher().watch(...)` のエラーが `ENOSPC` 相当のときは `inotify` 上限引き上げ手順を含む user_message に置き換える。CLI/README にも上限の存在を明記
  - 理由: 巨大リポジトリで再現性のある起動失敗を、原因不明の panic ではなく操作可能なメッセージで案内する

- [ ] shutdown チェーンの観測性を統合する
  - ファイル: `src/watcher/runtime.rs` L21/L47-57, `src/server/watch.rs` L18/L42-72, `src/server/broadcast.rs` L36-65
  - 現状: 2 段階のタイムアウトが連鎖（`SHUTDOWN_TIMEOUT_SECS=2` と `WATCH_FORWARDER_SHUTDOWN_TIMEOUT_SECS=2`）し、`abort()` 前のログは「abort された」だけで「watcher 側が close しないのか forwarder 側が drop しないのか」が判別不能
  - 対応: 2 つのタイムアウト定数を共通化し、`abort` 直前に `(elapsed_ms, last_event_kind, receiver_count)` を含む warn ログを 1 行追加。`spawn_watch_event_forwarder` 終了時のログにも `state.tx().receiver_count()` と最後のイベント種別を含めて、シャットダウン時に dropped events があった場合に検知できるようにする
  - 理由: HTTP サーバー再起動経路でのファイルハンドルリークを再現性のあるログで切り分けられるようにする

- [ ] `AppState` の Arc 二重ラップと `with_memo_fs` の API 整合を解消する
  - ファイル: `src/server/state.rs` L202-258, `src/main.rs` L105
  - 現状: `AppState: Clone` でフィールド単位に `Arc` を持つにも関わらず、`main.rs:105` が `Arc::new(AppState::new(...))` で外側でも Arc 化している。`with_memo_fs` は `cfg(test)` で `mut self` を取るが、`Arc<AppState>` をテスト本体で扱う API（`api_files_handler` 等）と相互運用できないハーフサポート状態
  - 対応: `AppState: !Clone` にして `Arc<AppState>` 一本に統一。`MemoFs` を `AppState::new` の引数にする builder パターンに変更し、`with_memo_fs` を削除。テストは `Arc::clone` に書き換え
  - 理由: 「Clone とフィールド Arc の二重投資」を解消し、ライフサイクルを単一化する

- [ ] `BroadcastMessage::Refresh` の memo_refresh 契約と、メモ読込失敗時の degrade 通知を明示する
  - ファイル: `src/server/messages.rs` L38-40, `src/server/routes.rs` L188-220
  - 現状: `Refresh` variant は `serde_json::json!({ "refresh": true })` を返すだけで、ディレクトリモードの遅延回復経路（`content.rs:133`）が memo の再取得指示を欠く。一方 `index_handler` のメモ読込失敗は `MemoResponse::empty` に silent fallback し、ユーザーには「メモが消えた」ように見える
  - 対応: `Refresh` variant に `memo_refresh: bool` を追加するか、仕様コメントを `messages.rs` に明記。`MemoResponse` に degrade flag を追加し、HTML 側でバナー表示できるよう情報を渡す
  - 理由: 仕様契約を型・コメントに固定し、UI が「メモ機能の一時的な機能低下」を区別できるようにする

- [ ] watcher の `WatchEvent::Error` 後の健全性 API と atomic save 耐性 E2E を追加する
  - ファイル: `src/watcher/runtime.rs` L165-178, `src/watcher/strategy.rs` L109-116/L216-246, `tests/integration_test.rs`
  - 現状: notify 由来エラーをブロードキャストした後も watcher は停止しないが、`ThreadPanic` 時は実際にはスレッドが死んでいる。HTTP 側からは無音と区別できない。エディタの atomic save（rename / 削除→作成）は inode 切り替えで notify が古い inode を見失う可能性があるが、tempfile で再現する E2E が無い
  - 対応: `WatchService` に `is_alive()` 相当の健全性フラグを追加し、`ThreadPanic` 受信で立てる。tempfile で「`target.md.swp` → `target.md~` → rename」のシーケンスを E2E に追加し、保存後の WS update 受信を確認
  - 理由: 監視機能が silent に死ぬパターンを観測可能にする

- [ ] `RenderState` を `enum BlockContext` スタックに置き換えて open/close 対応を型化する
  - ファイル: `src/renderer/state.rs` L42-336, `src/renderer/render.rs` L33-55
  - 現状: 14 個の `pub(super)` メソッド（`push_html`/`push_soft_break`/`finish_heading`/`finish_code_block` 等）で State Machine が implicit。`finish_heading` は `debug_assert! + take().?` で release fallback、`finish_code_block` は release でも `unreachable!`、と契約強制が混在
  - 対応: `enum BlockContext { Heading(HeadingState), CodeBlock(CodeBlockState), Image(ImageState), TableCell(TableCellState), ... }` のスタックを `RenderState` に持たせ、`finish_*` を `Result<_, RenderStateMismatch>` 化。`render::dispatch_event` 側は match で完全列挙
  - 理由: 状態機械の不変条件をコメントではなく型で表現し、open/close 不整合を型エラーで弾く

- [ ] `template/mod.rs` のテストをサブモジュールへ分割し、`render_page` の 62 行 `format!` を関数分割する
  - ファイル: `src/template/mod.rs` (740 行), `src/template/page.rs` L45-104
  - 現状: `template/mod.rs` の 740 行のうち約 720 行が `cfg(test) mod tests` で、page/tree/assets/message にまたがる horizontal integration test を吸収。`render_page` は `<!DOCTYPE html>` から `</html>` までを 62 行の単一 `format!` で組み、`html_escape(memo_file_attr)` などの属性挿入が場当たり的
  - 対応: テストを各サブモジュール（page/tree/assets/message）の `#[cfg(test)] mod tests` に局所化し、`mod.rs` には公開 API 契約テスト（CSP 整合性など）のみ残す。`render_page` は `render_head` / `render_body` / `attr(name, value)` ヘルパーへ分割
  - 理由: CLAUDE.md「300 行を超えたファイルは分割を提案」に該当。エスケープ漏れの一発リスクを集約しないために属性挿入をヘルパー化する

- [ ] `canonicalize` 失敗時の再帰挙動の非対称を解消する
  - ファイル: `src/server/files/catalog.rs` L72/L83/L109-118/L152-169
  - 現状: 通常ディレクトリ枝とシンボリックリンク枝で `canonicalize_dir_for_cycle` が `None` を返した時の扱いは揃っているが、`(file_type.is_symlink() && path.is_dir())` 判定で同期 `is_dir()` syscall を呼び、TOCTOU と性能の双方で穴が残る。テスト名 `canonicalize失敗時はスキップ扱い` が「失敗＝素通り」を仕様化している
  - 対応: visited 集合経由のループ防止を symlink/通常ディレクトリで完全対称にし、`is_dir()` の同期呼び出しは canonicalize の結果から導く。「canonicalize 失敗時に visited を経由せず再帰しない」ことをテストで固定
  - 理由: ループ防止のセキュリティ境界が「素通り経路」で破綻しないことを担保する

- [ ] ブラウザ JS の責務境界を小モジュールへ分割する
  - ファイル: `src/template/assets/js/{bootstrap,content,fetch,memo,selection,sidebar,websocket}.js`, `src/template/assets/inline_script.rs`
  - 現状: `docs/superpowers/plans/2026-04-30-browser-js-deglobalization.md` の実行で production の `window` 露出は IIFE と `appContext` 集約により解消済み。E2E用内部操作も `window.__MV_E2E__ === true` 時の `markdownViewTestHooks` に限定した。一方、`content.js` は検索、リンク解決、履歴、描画反映、スクロール、引用ジャンプをまとめて扱う巨大ファイルのままで、`appContext` 直接参照も多い。`innerHTML` はサーバー生成の `SanitizedHtml` を信頼する設計だが、信頼境界は型やモジュール境界としてはまだ表現されていない
  - 対応: `content-renderer` / `document-search` / `navigation` / `live-update-buffer` / `memo-citation` のように責務単位で分割し、分割後の境界では `ctx` 注入や小さな controller API で依存を明示する。`updateContent` の入力型・`SanitizedHtml` 前提・`innerHTML` 使用箇所を契約テストで固定する。メモや検索を削る、または純プレビューモードへ戻すことは非目標
  - 理由: 問題は「機能が多いこと」ではなく、workspace として成長した中核機能群の境界がブラウザ JS 内で十分に表現されていないこと。production グローバル露出は解消したが、巨大ファイルと暗黙の `appContext` 依存が残ると将来の入力経路追加で XSS 境界や状態遷移を壊しやすい

## Done Summary

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
