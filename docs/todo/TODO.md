# TODO Issues

レビュー指摘・コードベース探索で検出した改善項目のうち **High / Medium のみ**を優先度順に掲載。Low 項目は [`BACKLOG.md`](./BACKLOG.md) を参照。

## High Priority

- [ ] メモ書き込みを `tmp + rename` で原子化する
  - ファイル: `src/server/files/memo_fs.rs`, `src/server/files/memo.rs`
  - 現状: `MemoFs::write` が `tokio::fs::write`（内部 truncate+write）で、`memo_fs.rs:41` のコメントが「atomic は要求しない」と明文化している。書き込み途中の電源断・クラッシュでメモが空 or 部分書き込みで破損する。また `ensure_safe_memo_path()` の symlink 検査と実 write の間に TOCTOU window が残る
  - 対応: 同一ディレクトリ内 `.{name}.memo.md.tmp` に `create_new` 相当で書き出してから `rename` で差し替えるパターンに変更。tmp 作成・rename 直前の親ディレクトリ symlink 再検証、tmp 残存時の安全な cleanup、既存 `MockMemoFs` の同セマンティクス模倣をテストで固定する
  - 理由: ユーザーが手書きしたメモを失うクラスのリスクであり、未信頼 workspace や同期ディレクトリでは symlink race のセキュリティ境界にもなる。CLAUDE.md「個人使用前提でも互換性破壊は major」の警戒水準に該当

- [ ] `delete_route_memo` を all-or-nothing 化する
  - ファイル: `src/server/files/memo.rs` L80-100, `src/server/files/tests.rs` L975
  - 現状: primary sidecar 削除→compat sidecar 必須削除→legacy 必須削除を順次実行し、(2)/(3) で `PermissionDenied` 等が出ると 500 を返すが (1) は既に成功している。テスト `test_save_route_memo_空白保存_safe_legacy削除失敗は500を返す` がこの中間状態を意図仕様として固定している
  - 対応: 削除順を「全候補の存在確認 → primary を最後に削除」に変更するか、primary 失敗時のみ 500、compat/legacy 失敗は warn ログ + 200 にする。既存テストの仕様も修正
  - 理由: 「失敗レスポンスを受けたが primary は消えている」状態でクライアントが再試行すると挙動が変わる。原子性の最小担保

- [ ] 見出し ID 生成を単一パス化し render と toc で `HeadingInfo` を共有する
  - ファイル: `src/renderer/render.rs` L250-260, `src/renderer/mod.rs` L150-214, `src/renderer/toc.rs`
  - 現状: `handle_heading_end`（render 側）と `extract_headings`（toc 側）がそれぞれ独自の `id_counts: HashMap<String, usize>` を持ち、pulldown-cmark を 2 回パースする。`extract_headings` は `Tag::Image` を見出しテキストから明示除外するが SoftBreak の `in_heading_image` チェック非対称（`mod.rs:181-189`）。画像 alt + `Event::Code` 混在見出しで TOC ID と本文 `<h{n} id=...>` が乖離し得る
  - 対応: 見出し抽出ループを単一にまとめ、`Vec<HeadingInfo>` を render と toc で共有する。`tests/renderer_test.rs:631` の不変条件テストを境界ケース（画像 alt + code 混在、SoftBreak）まで拡張
  - 理由: 仕様不変条件（render と toc は同じ id を出力する）が型・データフローで担保されておらず、リファクタで silent に乖離する経路が残る。既存 CLAUDE.md「2 回パース」記述を超えて、`search` 経由でも 3 回目が走る点も合わせて整理する

- [ ] `notify_update` の receiver=0 早期 return で `Error` メッセージが silent drop されない経路にする
  - ファイル: `src/server/broadcast.rs` L19-65, `src/server/files/content.rs:70-100`
  - 現状: `tx.receiver_count() == 0` で早期 return する経路と `build_change_broadcast_message` が `Error` も返す設計が衝突。WS 接続より前にファイル削除・読み込み失敗が起きると、`BroadcastMessage::Error` が誰にも届かず tracing にも残らない（`broadcast_error` も `let _ = send(...)` で握りつぶす）
  - 対応: `Update` 系のみ早期 return、`Error` 系は `tx.send(...)` を試みるか少なくとも `tracing::warn!` でローカルに残す。既存テスト `notify_update_読み込み失敗時にエラーをbroadcast` を「receiver=0 のときも warn ログが出る」観点で補強
  - 理由: silent failure。pr-review-toolkit の silent-failure-hunter 観点と整合し、デバッグ可能性を担保

- [x] `is_trusted_host` / `normalize_authority` の IPv6 網羅テストを追加
  - ファイル: `src/server/guards.rs`
  - 現状: L171-173 の `test_trusted_host_loopback_ipv6` が `[::1]` のみを検証
  - 追加観点: `[::1]:3000`（port 付き bracketed）、`::1`（非 bracketed）、`[fe80::1]`（非 loopback）、`[::1]:abc`（非数値 port）の 4 パターン
  - 理由: DNS Rebinding 対策の核。正規化エッジケースで想定外に通過するとセキュリティ境界が崩れる

- [x] メモ sidecar fallback 経路の超長ファイル名＋特殊文字テストを追加
  - ファイル: `src/server/files/memo.rs`, `src/server/files/tests.rs`
  - 現状: `ensure_safe_memo_path` / `truncate_to_bytes` の基本テストと非utf8/拡張子大小テストはあるが、255 バイト超のファイル名と `../` や `\..\` の組み合わせが未検証
  - 追加観点: (a) 超長名＋特殊文字で sidecar 名が隔離され破損しないこと、(b) 異なる長い名前が同一 sidecar 名に衝突しないこと
  - 理由: パストラバーサル境界の回帰テスト

- [x] メモ API のボディ制限値を意図明文化し、境界テストを追加
  - ファイル: `src/server/routes.rs` L27
  - 現状: `MEMO_JSON_BODY_LIMIT = (MAX_FILE_SIZE * 2) + 4096` が無説明で定義
  - 対応: JSON エスケープで最悪 2 倍になる前提を doc コメントで明示。`MAX_FILE_SIZE + 小さなマージン` に引き締める可否を再検討。境界テスト（10MB + 1 バイト、20MB 付近）を追加
  - 理由: 将来の保守時に「なぜ 2 倍か」が読めないと制限緩和や強化判断を誤る

- [x] WebSocket close_code マッピングの統合テストを追加
  - ファイル: `tests/integration_test.rs`
  - 現状: `ReadMarkdownError::close_code()` のユニットテストは存在、`load_initial_socket_update` のエラー arm も Low 側で TODO 化済み。だが実際の WebSocket フレームまで透過確認する E2E はない
  - 追加観点: IO → 1011、TooLarge → 1009、NotUtf8 → 1003 の 3 シナリオを実サーバー + WebSocket クライアントで検証
  - 理由: WebSocket プロトコル境界。クライアント側の再接続ロジックが close_code に依存するため、中間層のどこかで書き換わると下流が壊れる

- [x] `is_allowed_ws_origin` の拒否経路に warn ログを追加（HOST 経路との観測性を揃える）
  - ファイル: `src/server/guards.rs` L75-102
  - 現状: `is_allowed_ws_origin` は 6 箇所以上で silent な `false` return（Origin なし / HOST なし / 非 http(s) / authority 不一致 / 非数値 port / userinfo 付き等）。対して `ensure_allowed_request_host` は拒否時に raw HOST 値を warn ログする監査経路を持つ
  - 対応: 各拒否分岐に `tracing::warn!` を追加し、どの理由で弾かれたかと原始 HOST/Origin を記録。`is_trusted_authority` の non-numeric port / userinfo 拒否も同様に観測可能にする
  - 理由: 攻撃者が WebSocket 経路で DNS Rebinding を試行した際、HOST 経路では検知できるが Origin 経路では完全に silent で「ブラウザが Origin を送っていない」と区別できない。pr-review-toolkit の silent-failure-hunter が指摘

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
  - 現状: 2 段階のタイムアウトが連鎖（`SHUTDOWN_TIMEOUT_SECS=2` と `WATCH_FORWARDER_SHUTDOWN_TIMEOUT_SECS=2`）し、`abort()` 前のログは「abort された」だけで「watcher 側が close しないのか forwarder 側が drop しないのか」が判別不能。`broadcast_error` も受信者ゼロ時に `let _ = send(...)` で握りつぶす
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

- [ ] 監視イベント経由の変更ファイルを最終読込前に再検証する
  - ファイル: `src/server/files/resolve.rs`, `src/server/files/content.rs`, `src/watcher/strategy.rs`
  - 現状: HTTP 経路は `resolve_file()` で base 配下・hidden・`.md`・symlink 差し替えを検証する。一方、watcher 経由のディレクトリ更新は `collect_directory_changes()` の事前検証後、`resolve_change_target()` が `changed_file` をそのまま `ResolvedTarget` に包み、`read_and_render_file()` が読み込む
  - 対応: `resolve_change_target()` でもディレクトリモード時は watcher 由来の絶対/字句パスから base 相対を復元し、`resolve_file(base_dir, relative)` 相当の検証を最終読込前に通す。削除済みファイルや canonicalize 失敗時の扱いは「安全側で error broadcast」になるよう統合テストを追加
  - 理由: watcher 側の `is_within_base_dir()` は canonicalize 失敗時に字句パスへフォールバックする。入口の防御に加えて読込直前の防御を置くことで、TOCTOU・symlink・削除競合時のセキュリティ境界を HTTP 経路と揃える

- [ ] ディレクトリ検索の負荷制御をサーバ側に追加する
  - ファイル: `src/server/routes.rs`, `src/server/files/search.rs`, `src/server/files/catalog.rs`
  - 現状: `/api/search` はリクエスト処理内で `list_markdown_files()` による同期ディレクトリ走査後、最大 1000 ファイルを順次 `read_markdown_with_limit()` で読み、Markdown パースして検索する。1 ファイル上限は 10MB だが、検索リクエスト全体の総読込量・時間・キャンセル境界はない。さらに `search_directory` は pulldown-cmark を **検索専用にもう一度パース**（renderer/toc に続く 3 回目）し、各ブロックで `original_offsets: Vec<usize>` をテキスト byte 数 +1 確保する。打ち切りが「ファイル単位」ではなく「結果件数 100」なので巨大 1 ファイルで `MAX_SEARCH_RESULTS` を消費すると他ファイルが silent に無視される
  - 対応: 検索の同期走査/重いパースを `spawn_blocking` または専用検索タスクに逃がす。総読込バイト上限、検索対象ファイル数上限の応答明示、クライアント世代と対応するサーバ側キャンセルまたは古い検索の破棄を検討し、巨大ワークスペースの統合テストを追加。`SearchResultItem` の `before/current/after` を `Cow<str>` 化してマッチごとの `String` 確保を削減
  - 理由: 横断検索は Markdown workspace の中核機能だが、localhost 前提でも巨大ディレクトリや連続検索で Tokio worker を圧迫し、本文表示・メモ・WebSocket の応答性に影響する可能性がある

- [ ] Host 検証を router middleware 化して新規 route の守り忘れを防ぐ
  - ファイル: `src/server/routes.rs`, `src/server/guards.rs`
  - 現状: HTTP は `RouteContext::ensure_allowed()` または handler 直下の手動呼び出し、WebSocket は `ws_handler()` 内の専用分岐で Host/Origin を検証している。`create_router()` に route が集約されている一方、Host 検証は opt-in になっている
  - 対応: Host 検証を axum middleware/layer として HTTP route 全体に適用し、WebSocket は Host middleware + Origin 検証の二段構えにする。`/api/files` や `/api/search` と同等の拒否テストに加え、新規 route が middleware を通る構造をテストで固定する
  - 理由: DNS Rebinding 対策はルート横断のセキュリティポリシーであり、handler ごとの呼び忘れを設計上起こりにくくする必要がある

- [ ] `RouteContext` を HTTP adapter と application service に分割する
  - ファイル: `src/server/routes.rs`, `src/server/files/{resolve,content,memo}.rs`
  - 現状: `RouteContext` が Host 検証、対象解決、本文ロード、メモロード/保存、サイドバー構築、memo broadcast 用 file label 生成まで抱えている。ルート層が HTTP 変換だけでなくアプリケーション手順の調停役にもなっており、新規 API 追加時に責務の置き場所が曖昧になる
  - 対応: Host 検証は middleware 化し、`RouteContext` は request DTO から service input を作る薄い adapter へ縮小する。本文/メモ/サイドバー/broadcast label は application service 側の小さな関数に分離し、HTTP handler は `Result<Json<_>, ApiError>` への変換に集中させる
  - 理由: Unix 哲学の「一つのことをうまくやる」に反し始めている境界。セキュリティ検証の守り忘れと、ルート層の肥大化による変更局所性低下を同時に下げる

- [ ] Markdown 方言オプションを共通化し、表示・TOC・検索の差分を明示する
  - ファイル: `src/renderer/mod.rs`, `src/server/files/search.rs`, `src/renderer/toc.rs`
  - 現状: renderer/TOC 側の `markdown_options()` は tables/tasklist/strikethrough のみ、検索側は footnotes/heading attributes/GFM も有効にしている。表示対象と検索対象の Markdown 解釈が暗黙に分岐している
  - 対応: 共通の Markdown option profile を導入し、表示・TOC・検索で同じ方言を使うか、用途別に差を残すなら `RenderProfile` / `SearchProfile` のように意図を型・テスト名で明示する。footnote・heading attributes・GFM の検索/表示一致テストを追加
  - 理由: Markdown 機能追加時に検索では見つかるが表示されない、または表示されるが検索されない回帰が起きやすい

- [ ] ブラウザ JS の巨大グローバル状態を小モジュールへ分割する
  - ファイル: `src/template/assets/js/{bootstrap,content,fetch,memo,selection,sidebar,websocket}.js`, `src/template/assets/inline_script.rs`
  - 現状: `bootstrap.js` が多数の DOM 参照・状態変数をグローバルに初期化し、`content.js` は 1300 行超で検索、リンク解決、履歴、描画反映、スクロール、引用ジャンプをまとめて扱っている。検索・メモ・引用は Markdown workspace の中核機能だが、実装上は単一スクリプトに集中している。`innerHTML` はサーバー生成の `SanitizedHtml` を信頼する設計だが、信頼境界が JS 側の構造では表現されていない
  - 対応: `content-renderer` / `document-search` / `navigation` / `live-update-buffer` / `memo-citation` のように責務単位で分割し、共有状態は明示的な state object 経由に寄せる。`updateContent` の入力型・`SanitizedHtml` 前提・`innerHTML` 使用箇所を契約テストで固定する。メモや検索を削る、または純プレビューモードへ戻すことは非目標
  - 理由: 問題は「機能が多いこと」ではなく、workspace として成長した中核機能群の境界がブラウザ JS 内で十分に表現されていないこと。現状の CSP/HTML sanitize 境界は強いが、グローバル関数・暗黙 state・巨大ファイルのままだと将来の入力経路追加で XSS 境界を壊しやすい

- [x] CSP フォールバック時の方針整理（fail-fast vs 現状運用）
  - ファイル: `src/server/guards.rs` L30-36
  - 現状: `HeaderValue::from_str(&csp)` 失敗時のフォールバック CSP は `default-src 'self'; object-src 'none'; frame-ancestors 'none'`。inline は暗黙拒否されるが、sha256 ハッシュベースの厳格制約は失われる
  - 対応候補: (a) CSP 生成失敗をサーバー起動中止扱いにする、(b) フォールバック CSP に `script-src 'none'; style-src 'none'` を明示する、(c) 現状維持で運用ログ監視に任せる
  - 理由: フォールバック発生時の動作セマンティクスが不明瞭。個人使用前提だが、意図ある設計として明文化したい

- [x] エラー経路ログのパス情報を base 相対化
  - ファイル: `src/server/files/resolve.rs` ほか `tracing::warn!` でパスを出す箇所
  - 現状: パス正規化失敗時にユーザー指定パス・サーバー実ディレクトリ構造をそのまま warn ログに出力
  - 対応: base_dir 基準での相対化ヘルパー `sanitize_path_for_logging(path, base)` を抽出し、絶対パスや base 外パスを丸めて出力
  - 理由: 個人使用前提でもディレクトリ構造の漏出は望ましくない

- [x] `is_hidden_relative` のネスト深度を 3 → 2 階層に削減
  - ファイル: `src/watcher/strategy.rs` L162-195
  - 現状: `match strip_prefix → match canonicalize(path) → match canonicalize(base)` の 3 段ネストで、canonicalize 失敗時のフォールバックログが 2 回重複
  - 対応: `try_relative_components(path, base) -> Option<impl Iterator<Component>>` 風のヘルパーを抽出し、呼び出し側は 1 回 match
  - 理由: 直前の watcher リファクタで隠し判定のロジックだけが旧形状のまま残っている

- [x] `updateContent` の inverse case (file-switch / data.content 変更時) の再描画検証
  - ファイル: `tests/e2e/memo_jump.spec.js` (回帰テスト L429 周辺に Step 4 追加 or 別テスト)
  - 内容: 現状の回帰テストは「同一 data.content での 2 回目 no-op」のみ検証。**逆方向**である「data.content が変わったら必ず再描画される」を直接検証するテストが欠落
  - 想定実装: 既存 prime → highlight → 同一 no-op の後に Step 4 として、別の `data.content` 文字列 (例: ダミー HTML) を渡して `window.updateContent` を呼び、(a) `.jump-highlight` が消えている (= 再描画された) (b) その後同一の changed content で再度呼ぶと no-op (= cache が新値で更新された) の 2 点を検証
  - 理由: cache invariant が逆転した regression (条件が常に false 化する書き換え等) を現状の suite では検出できない

- [x] `updateContent` で `data.content === undefined` を契約違反として明示ログ
  - ファイル: `src/template/assets/js/content.js` L1239 周辺
  - 内容: `UpdateMessage` (`src/template/message.rs`) は `content` / `toc` に `skip_serializing_if` を付けていないため `data.content` は **必ず** 存在するはずだが、現状は `undefined` を no-op で黙殺している。サーバ契約変更や中継プロキシ改変で content が欠落した場合「ファイル編集してもプレビュー更新されない」サイレント失敗になる
  - 想定実装: `data.content === undefined` の場合 `console.warn('[markdown-view] updateContent: data.content が欠落 (契約違反)', data);` を出し、TOC 更新等の副作用は継続
  - 理由: WS フレームを直接覗かないとデバッグ不能なサイレント失敗の予防

- [x] HTTP `/api/content` の IO エラー経路 (500) の統合テストを追加
  - ファイル: `tests/integration_test.rs`
  - 現状: `ReadMarkdownError::into_response()` が `Io(_) → 500 INTERNAL_SERVER_ERROR` にマップされる (`src/server/files/content.rs:236-240`) が、HTTP 境界の統合テストは存在しない (grep で `INTERNAL_SERVER_ERROR` は L376 のディレクトリ delete 経路のみ)
  - 対応: WebSocket 1011 テストと同じ手法 (`chmod 0o000` で EACCES 誘発) を `/api/content` の reqwest 呼び出しに適用し、status 500 と JSON `error` フィールドが `"ファイルの読み込みに失敗しました"` であることを検証。既存 `assert_json_error_for_paths` (L1707) と同じ構造で実装可能
  - 理由: WebSocket 経路の 1011 透過確認と対称。IO エラーが `NotUtf8`/`TooLarge` の HTTP ステータスに誤分類されても現状は検知できない

- [x] `build_change_broadcast_message` の IO エラー経路を統合テストでカバー
  - ファイル: `tests/integration_test.rs`
  - 対応: ファイル更新を watcher に拾わせて chmod 0o000 → notify のシーケンスで change 経路を刺激
  - 理由: 初期化 / 変更経路で同じ `ReadMarkdownError` の扱いが分かれており、一つの経路のリファクタで他経路が silent に壊れる可能性がある

- [x] `build_lagged_recovery_message` の IO エラー透過を統合テストでカバー
  - ファイル: `tests/integration_test.rs`
  - 対応: `broadcast::channel(1)` の飽和などで lag recovery を意図的に発生させる必要があり、再現性が低いため将来の宿題
  - 理由: 遅延回復経路でも `ReadMarkdownError` が `BroadcastMessage::Error(format!("..."))` に畳み込まれるため、リファクタ時の回帰を検知したい

- [x] メモ sidecar 名生成の不変条件を `SidecarMemoName` に集約し、境界テストと受容リスクを補強
  - ファイル: `src/server/files/memo.rs`, `src/server/files/memo_sidecar.rs`, `src/server/files/tests.rs`, `docs/superpowers/specs/2026-04-24-memo-sidecar-name-hardening-design.md`
  - 現状: `SidecarMemoName` が 255 bytes 以下を保証するため `sidecar_name_too_long` 分岐は実質到達不能になっている。255 bytes ちょうど / 256 bytes 超過、UTF-8 境界直前、正規化済み超長名の組み合わせテストも薄い。64 bit hash 衝突と Windows 非 UTF-8 名の fallback 集約は受容リスクとして設計書に残っていない
  - 対応: `sidecar_name_too_long` を削除または型内部へ統合し、呼び出し側の legacy fallback 分岐を現実の契約に合わせる。境界テストを追加し、hash 衝突・非 UTF-8 fallback 集約を設計書の既知リスクとして明記する
  - 理由: sidecar 名生成の single source of truth を明確にし、将来のリファクタで長名・正規化・非 UTF-8 のセキュリティ境界が silent に変わることを防ぐ
