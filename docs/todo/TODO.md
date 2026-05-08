# TODO Issues

レビュー指摘・コードベース探索で検出した改善項目のうち、次に実行する **High / Medium** のみを優先度順に掲載する。Low 項目は [`BACKLOG.md`](./BACKLOG.md) を参照。

最終整理: 2026-05-05。完了済みの長文履歴は本ファイル末尾の Done サマリに圧縮し、未完了項目だけを実行候補として残す。

## High Priority

現時点で未完了の High Priority はなし。

## Medium Priority

以下はリスク低減順に実行する。起動不能や silent failure に近い項目を先に扱い、変更範囲が大きい構造変更は後ろへ置く。

- [ ] `BroadcastMessage::Refresh` の memo_refresh 契約と、メモ読込失敗時の degrade 通知を明示する
  - ファイル: `src/server/messages.rs` L38-40, `src/server/routes.rs` L188-220
  - 現状: `Refresh` variant は `serde_json::json!({ "refresh": true })` を返すだけで、ディレクトリモードの遅延回復経路（`content.rs:133`）が memo の再取得指示を欠く。一方 `index_handler` のメモ読込失敗は `MemoResponse::empty` に silent fallback し、ユーザーには「メモが消えた」ように見える
  - 対応: `Refresh` variant に `memo_refresh: bool` を追加するか、仕様コメントを `messages.rs` に明記。`MemoResponse` に degrade flag を追加し、HTML 側でバナー表示できるよう情報を渡す
  - 理由: 仕様契約を型・コメントに固定し、UI が「メモ機能の一時的な機能低下」を区別できるようにする

- [ ] ブラウザ JS の責務境界を小モジュールへ分割する
  - ファイル: `src/template/assets/js/{bootstrap,content,content-renderer,fetch,memo,selection,sidebar,websocket}.js`, `src/template/assets/inline_script.rs`
  - 現状: `docs/superpowers/plans/2026-04-30-browser-js-deglobalization.md` の実行で production の `window` 露出は IIFE と `appContext` 集約により解消済み。E2E用内部操作も `window.__MV_E2E__ === true` 時の `markdownViewTestHooks` に限定した。さらに `content-renderer.js` で `updateContent` の payload 契約、契約違反 warn、`#content` / `#toc` への sanitize 済み HTML 反映、TOC HTML 正規化を明示境界へ切り出した。一方、`content.js` は検索、リンク解決、履歴、スクロール、引用ジャンプ、描画後副作用をまだまとめて扱う巨大ファイルのままで、controller API と依存境界は未整理
  - 対応: 次の分割単位を `document-search` / `directory-search`、`navigation` / `link-resolution`、`createContentController(ctx, deps)` の順で切る。`innerHTML` 使用箇所は引き続き信頼境界を明示し、検索やメモを削る、または純プレビューモードへ戻すことは非目標
  - 理由: 問題は「機能が多いこと」ではなく、workspace として成長した中核機能群の境界がブラウザ JS 内で十分に表現されていないこと。`content-renderer` により最重要の XSS 信頼境界は狭まったが、巨大ファイルと暗黙の `appContext` 依存が残ると将来の入力経路追加で状態遷移を壊しやすい

- [ ] `template/mod.rs` のテストをサブモジュールへ分割し、`render_page` の 62 行 `format!` を関数分割する
  - ファイル: `src/template/mod.rs` (740 行), `src/template/page.rs` L45-104
  - 現状: `template/mod.rs` の 740 行のうち約 720 行が `cfg(test) mod tests` で、page/tree/assets/message にまたがる horizontal integration test を吸収。`render_page` は `<!DOCTYPE html>` から `</html>` までを 62 行の単一 `format!` で組み、`html_escape(memo_file_attr)` などの属性挿入が場当たり的
  - 対応: テストを各サブモジュール（page/tree/assets/message）の `#[cfg(test)] mod tests` に局所化し、`mod.rs` には公開 API 契約テスト（CSP 整合性など）のみ残す。`render_page` は `render_head` / `render_body` / `attr(name, value)` ヘルパーへ分割
  - 理由: CLAUDE.md「300 行を超えたファイルは分割を提案」に該当。エスケープ漏れの一発リスクを集約しないために属性挿入をヘルパー化する

- [ ] `RenderState` を `enum BlockContext` スタックに置き換えて open/close 対応を型化する
  - ファイル: `src/renderer/state.rs` L42-336, `src/renderer/render.rs` L33-55
  - 現状: 14 個の `pub(super)` メソッド（`push_html`/`push_soft_break`/`finish_heading`/`finish_code_block` 等）で State Machine が implicit。`finish_heading` は `debug_assert! + take().?` で release fallback、`finish_code_block` は release でも `unreachable!`、と契約強制が混在
  - 対応: `enum BlockContext { Heading(HeadingState), CodeBlock(CodeBlockState), Image(ImageState), TableCell(TableCellState), ... }` のスタックを `RenderState` に持たせ、`finish_*` を `Result<_, RenderStateMismatch>` 化。`render::dispatch_event` 側は match で完全列挙
  - 理由: 状態機械の不変条件をコメントではなく型で表現し、open/close 不整合を型エラーで弾く

## Done Summary

- [x] watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する
  - 完了根拠: ディレクトリモードの監視登録を `WatchPlan` 経由にし、`.git`、`node_modules`、`target`、隠しディレクトリ、symlink directory を notify 登録前に除外する構成にした。起動後に作成された通常サブディレクトリは internal event loop で動的に `NonRecursive` watch へ追加し、既存 Markdown の回復通知も送る。watch 登録の部分成功は init failure とし、起動後の追加 watch 失敗は health failure と `WatchEvent::Error` に分類する。ENOSPC 相当は `WatchErrorKind::ResourceExhausted` として Linux inotify 上限の確認へ進める日本語メッセージを返す。README に既定除外と inotify 上限を明記した
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
