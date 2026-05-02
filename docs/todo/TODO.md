# TODO Issues

レビュー指摘・コードベース探索で検出した改善項目のうち **High / Medium のみ**を優先度順に掲載。Low 項目は [`BACKLOG.md`](./BACKLOG.md) を参照。

## High Priority

- [x] 見出し ID 生成を単一パス化し render と toc で `HeadingInfo` を共有する
  - ファイル: `src/renderer/render.rs` L250-260, `src/renderer/mod.rs` L150-214, `src/renderer/toc.rs`
  - 現状: `handle_heading_end`（render 側）と `extract_headings`（toc 側）がそれぞれ独自の `id_counts: HashMap<String, usize>` を持ち、pulldown-cmark を 2 回パースする。`extract_headings` は `Tag::Image` を見出しテキストから明示除外するが SoftBreak の `in_heading_image` チェック非対称（`mod.rs:181-189`）。画像 alt + `Event::Code` 混在見出しで TOC ID と本文 `<h{n} id=...>` が乖離し得る
  - 対応: 見出し抽出ループを単一にまとめ、`Vec<HeadingInfo>` を render と toc で共有する。`tests/renderer_test.rs:631` の不変条件テストを境界ケース（画像 alt + code 混在、SoftBreak）まで拡張
  - 理由: 仕様不変条件（render と toc は同じ id を出力する）が型・データフローで担保されておらず、リファクタで silent に乖離する経路が残る。既存 CLAUDE.md「2 回パース」記述を超えて、`search` 経由でも 3 回目が走る点も合わせて整理する

- [x] Markdown 方言オプションを共通化し、表示・TOC・検索の差分を明示する
  - ファイル: `src/renderer/mod.rs`, `src/server/files/search.rs`, `src/renderer/toc.rs`
  - 現状: renderer/TOC 側の `markdown_options()` は tables/tasklist/strikethrough のみ、検索側は footnotes/heading attributes/GFM も有効にしている。表示対象と検索対象の Markdown 解釈が暗黙に分岐している
  - 対応: 共通の Markdown option profile を導入し、表示・TOC・検索で同じ方言を使うか、用途別に差を残すなら `RenderProfile` / `SearchProfile` のように意図を型・テスト名で明示する。footnote・heading attributes・GFM の検索/表示一致テストを追加
  - 理由: Markdown 機能追加時に検索では見つかるが表示されない、または表示されるが検索されない回帰が起きやすい

- [x] 監視イベント経由の変更ファイルを最終読込前に再検証する
  - ファイル: `src/server/files/resolve.rs`, `src/server/files/content.rs`
  - 現状: HTTP 経路は `resolve_file()` で base 配下・hidden・`.md`・symlink 差し替えを検証する。一方、watcher 経由のディレクトリ更新は `collect_directory_changes()` の事前検証後、`resolve_change_target()` が `changed_file` をそのまま `ResolvedTarget` に包み、`read_and_render_file()` が読み込む
  - 対応: `resolve_change_target()` でもディレクトリモード時は watcher 由来の絶対/字句パスから base 相対を復元し、`resolve_file(base_dir, relative)` 相当の検証を最終読込前に通す。ディレクトリモードのbase 配下の削除済み・一時不在ファイルとwatcher由来の非UTF-8 pathは通知なしでスキップし、単一ファイルモードの監視対象消失は検証エラーとして通知する。既存base外・hidden・非 Markdown・非通常ファイル・base 外 symlinkは error broadcast する境界テストを追加
  - 理由: watcher 側の `is_within_base_dir()` は canonicalize 失敗時に字句パスへフォールバックする。入口の防御に加えて読込直前の防御を置くことで、TOCTOU・symlink・削除競合時のセキュリティ境界を HTTP 経路と揃える

- [ ] Host 検証を router middleware 化して新規 route の守り忘れを防ぐ
  - ファイル: `src/server/routes.rs`, `src/server/guards.rs`
  - 現状: HTTP は各 handler 直下の手動呼び出し、WebSocket は `ws_handler()` 内の専用分岐で Host/Origin を検証している。`create_router()` に route が集約されている一方、Host 検証は opt-in になっている
  - 対応: Host 検証を axum middleware/layer として HTTP route 全体に適用し、WebSocket は Host middleware + Origin 検証の二段構えにする。`/api/files` や `/api/search` と同等の拒否テストに加え、新規 route が middleware を通る構造をテストで固定する
  - 理由: DNS Rebinding 対策はルート横断のセキュリティポリシーであり、handler ごとの呼び忘れを設計上起こりにくくする必要がある

- [ ] ディレクトリ検索の負荷制御をサーバ側に追加する
  - ファイル: `src/server/routes.rs`, `src/server/files/search.rs`, `src/server/files/catalog.rs`
  - 現状: `/api/search` はリクエスト処理内で `list_markdown_files()` による同期ディレクトリ走査後、最大 1000 ファイルを順次 `read_markdown_with_limit()` で読み、Markdown パースして検索する。1 ファイル上限は 10MB だが、検索リクエスト全体の総読込量・時間・キャンセル境界はない。さらに `search_directory` は pulldown-cmark を **検索専用にもう一度パース**（renderer/toc に続く 3 回目）し、各ブロックで `original_offsets: Vec<usize>` をテキスト byte 数 +1 確保する。打ち切りが「ファイル単位」ではなく「結果件数 100」なので巨大 1 ファイルで `MAX_SEARCH_RESULTS` を消費すると他ファイルが silent に無視される
  - 対応: 検索の同期走査/重いパースを `spawn_blocking` または専用検索タスクに逃がす。総読込バイト上限、検索対象ファイル数上限の応答明示、クライアント世代と対応するサーバ側キャンセルまたは古い検索の破棄を検討し、巨大ワークスペースの統合テストを追加。`SearchResultItem` の `before/current/after` を `Cow<str>` 化してマッチごとの `String` 確保を削減
  - 理由: 横断検索は Markdown workspace の中核機能だが、localhost 前提でも巨大ディレクトリや連続検索で Tokio worker を圧迫し、本文表示・メモ・WebSocket の応答性に影響する可能性がある

## Medium Priority

- [ ] async ハンドラ内の同期 I/O を `spawn_blocking` ないし起動時固定化で解消する
  - ファイル: `src/server/files/catalog.rs` L39/60/83/157, `src/server/files/memo.rs` L439, `src/watcher/strategy.rs` L186-210/L262-275
  - 現状: tokio worker thread をブロックしうる経路が 3 箇所に散在する: (a) `list_markdown_files_recursive` が `std::fs::read_dir` / `entry.file_type()` / `path.canonicalize()` を毎再帰呼び出し（さらに `base_dir.canonicalize()` を再帰内で繰り返す）、(b) `ensure_safe_memo_path` から呼ばれる `first_symlink_component` が `std::fs::symlink_metadata` を async 関数の中で実行（他は `tokio::fs::*` で揃えているのに非対称）、(c) debouncer コールバック（notify 内部スレッド）内で `is_within_base_dir` / `try_strip_base` が `path.canonicalize()` を毎イベント呼び出し
  - 対応: `base_dir` の canonicalize は起動時 1 回に固定し、`list_markdown_files_recursive` と `is_within_base_dir` / `try_strip_base` には canonicalized base を引き回す（lexical strip_prefix 中心）。`first_symlink_component` は `tokio::fs::symlink_metadata` に置き換え、必要なら `MemoFs` トレイトに `symlink_metadata` を追加。`list_markdown_files` 自体を `spawn_blocking` ラップする選択肢も検討
  - 理由: 個人ツールでも大ディレクトリ・大量保存時に応答性が落ちる。CLAUDE.md の「ブロッキング I/O が async コンテキストで実行されていないか」という設計規律と整合させる

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

- [ ] `toc.rs` の `build_toc_html` で `level=0` インデックス OOB ガードを入れる
  - ファイル: `src/renderer/toc.rs` L43-46/L53, `src/renderer/mod.rs` L142
  - 現状: `open_li_at_level[(current_level - 1) as usize]` の素のインデックスアクセス。`current_level: u8` を `as usize` してから `-1` する流れは正規化に依存し、`HeadingInfo.level: u8` が 0 を許す型定義のため将来の改修で OOB panic が再導入されやすい
  - 対応: `level: NonZeroU8` への型昇格、または `open_li_at_level.last_mut()` 経由のガードに置換。`#[cfg(test)] mod tests` を toc.rs に追加し level=0 入力で panic しないことを境界テストで固定
  - 理由: panic 経路を型レベルで閉じる。現状到達不能だが「不変条件を型で表現する」原則を満たすため

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
