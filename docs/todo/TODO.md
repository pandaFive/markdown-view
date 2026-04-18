# TODO Issues

## 現在の未完了タスク

### Medium Priority

#### メモ機能: リアルタイム同期

- [x] Step 1. メモ更新用の WebSocket メッセージ仕様を追加
  - ファイル: `src/server/messages.rs`, `src/server/session.rs`, `src/server/files/memo.rs`
  - 内容: メモ保存時に配信できる専用メッセージ型を定義し、既存本文更新メッセージと衝突しない形で直列化する
  - 完了条件: 本文更新メッセージとは別 type を持ち、対象ファイル識別子を必須にする
  - セキュリティ観点: クライアントへ渡す payload は既存の sanitize 境界を崩さない形に固定する。未検証の `raw` を描画責務不明のまま配信しない
  - 理由: リアルタイム同期を段階導入するために、まずサーバー側の通知面を独立させる必要がある

- [x] Step 2. メモ保存時に他クライアントへ更新通知を送る
  - ファイル: `src/server/routes.rs`, `src/server/files/memo.rs`, `src/server/broadcast.rs`
  - 内容: `PUT /api/memo` 成功時に、保存した対象ファイルのメモ更新を broadcast する
  - 前提: Step 1 のメッセージ仕様が確定していること
  - セキュリティ観点: 他ファイルのメモを誤配信しない。path validation と対象ファイル解決の既存制約を維持する
  - 理由: 現状は保存したタブしか最新化されず、複数タブ・複数クライアントで内容がずれる

- [x] Step 3. クライアント側でメモ更新通知を受信して現在表示中のメモへ反映する
  - ファイル: `src/template/assets/js/websocket.js`, `src/template/assets/js/memo.js`
  - 内容: 本文更新と同様に、現在開いているファイルのメモだけを安全に反映し、編集中は上書きを避ける
  - 前提: Step 1-2 完了
  - 完了条件: 現在表示中のファイルにのみ反映し、編集中フラグや dirty state がある場合は自動上書きしない
  - セキュリティ観点: 受信データを DOM へ直接流し込まず、既存の安全な描画経路に限定する
  - 理由: 通知だけ先に入れても、フロント側に競合回避付き反映処理がないと実運用で壊れる

- [x] Step 4. メモのリアルタイム同期競合を検証する E2E テストを追加
  - ファイル: `tests/e2e/`
  - 内容: 複数タブまたは複数ページで同一メモを開き、片方の保存がもう片方へ反映されることを検証する
  - 前提: Step 1-3 完了
  - テスト観点: 反映成功、編集中の上書き回避、別ファイル誤反映なし
  - 理由: 同期機能は race condition を起こしやすく、ユニットテストだけでは不足する

### Low Priority

#### テスト追加

- [ ] `load_initial_socket_update` のエラーパステスト追加
  - ファイル: `src/server/files/tests.rs`
  - 内容: ディレクトリモードで `Ok(None)` を返す、単一ファイル削除時に close_code 1008 を返す、サイズ超過時に close_code 1009 を返す、3 パスのテスト
  - 理由: outcome → `SocketInitError` のワイヤリングが現在未検証。将来の close_code 誤割り当てを回帰テストで検出するため。既存ギャップのため本 PR スコープ外として記録

- [ ] `handle_debounced_events()`のユニットテスト追加
  - ファイル: `src/watcher.rs` L273-327
  - 内容: SingleFile/Directoryの両戦略、非`.md`ファイル、隠しファイル、重複排除のテスト
  - 理由: コアイベント処理ロジックの回帰防止

- [ ] `Watcher::spawn`ディレクトリモードの end-to-end テスト追加
  - ファイル: `src/watcher.rs` テストモジュール
  - 内容: `test_watcher_spawn_単一ファイルモードでイベント受信できる`のディレクトリ版
  - 理由: ディレクトリモード固有のフィルタリングの検証

#### 型・可視性・命名整理

- [ ] `CanonicalPath`を`pub(super)`に降格
  - ファイル: `src/server/state.rs` L12
  - 内容: re-exportされず公開APIにも不使用のため可視性を縮小
  - 理由: 可視性の一貫性向上

- [ ] `WatchStrategy`で`CanonicalPath`型を使用
  - ファイル: `src/watcher.rs` L26-29
  - 内容: `PathBuf`の代わりに既存の`CanonicalPath` newtypeを使い、正規化の不変条件を型で保証
  - 理由: 型安全性の向上

- [ ] `SocketInitError`の可視性整理
  - ファイル: `src/server/files/content.rs` L24
  - 内容: `pub(in crate::server)`だが`mod.rs`で再エクスポートされず実質`pub(super)`相当。再エクスポート追加か可視性縮小
  - 理由: 現状動作に問題なし。将来の保守性向上

- [ ] `ResolvedTarget::update`メソッド名を`attach_file_info`等に改名
  - ファイル: `src/server/files.rs` L69
  - 内容: `UpdateMessage`との名前衝突を解消
  - 理由: 全呼び出し元に影響し他の修正と同時に行うと差分が大きくなる

- [ ] `RouteTargetRequest`をstruct+enum kindパターンに変更
  - ファイル: `src/server/files/resolve.rs`
  - 内容: 全バリアント同一の`query_file`フィールドをstructに集約
  - 理由: コードの簡素化。バリアント追加時の重複排除

#### 軽微なリファクタ・保守改善

- [ ] `WatchEvent::Error(String)`の構造化エラー化
  - ファイル: `src/watcher.rs` L14-23
  - 内容: 将来コンシューマが増えた場合に`WatchErrorKind`列挙型への移行を検討
  - 理由: 現在は単一コンシューマのため優先度低

- [ ] `file_label`算出ロジックが`ResolvedTarget::new`と`update_broadcast_message`で重複
  - ファイル: `src/server/files.rs` L217-220
  - 内容: 共通関数`file_display_name(path: &Path) -> String`を抽出
  - 理由: 上位のリファクタ後に再評価する

- [ ] `lagged_recovery_message`が単純な委譲関数。直接呼び出しで除去可能
  - ファイル: `src/server/websocket.rs` L27-29
  - 内容: `lagged_recovery_broadcast_message`を直接呼び出しに変更
  - 理由: websocket.rsとfiles.rsの両方を変更する必要がある

- [ ] `handle_socket`内の`if let Some` + `match`のネストを2ステップに分離
  - ファイル: `src/server/websocket.rs` L34-40
  - 内容: 中間変数に束縛してから`if let`で分岐
  - 理由: 可読性改善のみでリスクに見合わない

- [ ] `resolve.rs` L240のコメント詳細化
  - ファイル: `src/server/files/resolve.rs` L240
  - 内容: `build_resolved_target`のgraceful degradationコメントにWebSocketパスの安全性文脈を復元
  - 理由: 旧5行から新1行に簡略化され、保守者向け情報が減少

- [ ] `MemoResponse::new`を`from_raw`に変更してraw/html不整合リスクを排除
  - ファイル: `src/template/message.rs`
  - 内容: `new(raw, html, file)`を`from_raw(raw, file)`に変更し、内部で`render_markdown`を呼ぶ
  - セキュリティ観点: raw と html の責務を一本化し、未整合な HTML 混入経路を減らす
  - 理由: 呼び出し側でraw/htmlの整合性を保証する責務がなくなる

- [ ] `render_markdown("")`の結果をOnceLockでキャッシュ
  - ファイル: `src/template/message.rs`
  - 内容: `MemoResponse::empty`が毎回呼ぶ`render_markdown("")`の結果を静的キャッシュ
  - 注意点: 計測値なしのため、実施前に効果確認を行う
  - 理由: メモ未作成ファイルが多い場合のマイクロ最適化候補

#### メモ機能: 参照導線改善

- [ ] レンダラー出力に行範囲ジャンプ用の安定ターゲットを追加
  - ファイル: `src/renderer/mod.rs`, `src/template/assets/js/content.js`
  - 内容: `Lx-Ly` から本文内の対応ブロックを引けるよう、行範囲単位のターゲット属性またはアンカー生成規約を追加する
  - 理由: 現状の `data-source-*` は参照用で、ジャンプ先として直接使うには粒度が粗い

- [ ] メモプレビュー内の出典クリックで本文へスクロールする処理を追加
  - ファイル: `src/template/assets/js/memo.js`, `src/template/assets/js/sidebar.js`
  - 内容: 出典リンク選択時にメモタブから本文へ戻し、対応ブロックへスクロールする
  - 理由: 行番号が表示されても、実際に本文へ戻れないと参照導線として弱い

- [ ] 本文ジャンプ時の一時ハイライト表示を追加
  - ファイル: `src/template/assets/js/content.js`, `src/template/assets/css/memo.css`
  - 内容: スクロール後に対象箇所を数秒ハイライトし、どこへ移動したか分かるようにする
  - 理由: 長文ドキュメントではスクロールだけだと着地点が視認しづらい

- [ ] 出典ジャンプ導線の E2E テストを追加
  - ファイル: `tests/e2e/`
  - 内容: メモプレビューの出典クリックで、対応見出しや行範囲付近へ遷移・ハイライトされることを検証する
  - 理由: UI の回帰が起きやすく、DOM 属性変更時の破壊を検知したい

## 完了済みレビュー記録

### PRレビュー: セキュリティ強化とtemplate分割 (レビュー日: 2026-03-09)

- [x] `TargetResolveContext`の抽象度評価 → `&'static str`パラメータに簡素化済み

### PRレビュー: server/templateモジュール分割 (レビュー日: 2026-03-10)

- [x] `consume_initial_ws_message`のエラー無視を修正済み

### PRレビュー: serverファサード化とサブモジュール分割 (レビュー日: 2026-03-10)

- [x] `MAX_FILE_SIZE`と`file_size_limit_error_message()`を`files.rs`に移動済み
- [x] `file_size_limit_error_message()`を`&'static str`定数に変換済み
- [x] `relative_path_of`内の`tracing::warn!`を呼び出し側に移動済み
  - ファイル: `src/server/state.rs` L172-187
  - 内容: データ型メソッドから副作用（ログ出力）を分離し、呼び出し側で処理
  - 理由: データ型と副作用の分離
- [x] `state.rs`の未使用テストヘルパー`create_single_file_state`は既に削除済み
  - ファイル: `src/server/state.rs` L333
  - 内容: `#[allow(dead_code)]`付きの未使用ヘルパーを削除
  - 理由: デッドコードの除去
- [x] `AppState`に`#[derive(Debug)]`を追加済み
  - ファイル: `src/server/state.rs` L191
  - 内容: 診断性向上のためDebug traitを導出
  - 理由: サーバー状態のログ出力・デバッグ支援

### PRレビュー: server files責務分割 (レビュー日: 2026-03-10)

- [x] `build_lagged_recovery_message`ディレクトリモードのテスト追加済み
  - ファイル: `src/server/files/content.rs` L92
  - 内容: ディレクトリモードで`BroadcastMessage::Refresh`を返すパスのテスト
  - 理由: リファクタ前から存在する既存ギャップ
- [x] `load_route_update`エラーマッピングのテスト追加済み
  - ファイル: `src/server/files/content.rs` L47
  - 内容: `ReadMarkdownError` → `ApiError`変換のユニットテスト
  - 理由: リファクタ前から存在する既存ギャップ
- [x] `resolve_request_target`デフォルトファイル選択のテスト追加済み
  - ファイル: `src/server/files/resolve.rs` L167
  - 内容: `query_file`なし時のREADME優先選択ロジックのテスト
  - 理由: リファクタ前から存在する既存ギャップ

### PRレビュー: メモ機能 (レビュー日: 2026-03-12)

- [x] 空/whitespaceメモ保存でファイル削除される動作の統合テスト追加済み
  - ファイル: `tests/integration_test.rs`
  - 内容: PUT `/api/memo` に `{"raw": "  \n  "}` を送り、既存メモが削除され後続GETが空を返すことを検証
  - 理由: 削除は破壊的操作であり回帰テストが必要
- [x] 10MB超メモの413拒否テスト追加済み
  - ファイル: `tests/integration_test.rs`
  - 内容: PUT `/api/memo` に10MB超のbodyを送り413が返ることを検証
  - 理由: サイズ制限の回帰防止
- [x] メモAPIへのパストラバーサルテスト追加済み
  - ファイル: `tests/integration_test.rs`
  - 内容: PUT `/api/memo?file=../../etc/passwd` が404/400で拒否されることを検証
  - 理由: セキュリティ境界の明示的テスト
- [x] `data-source-start-line`/`data-source-end-line`の値の正確性テスト追加済み
  - ファイル: `tests/renderer_test.rs`
  - 内容: `LineLookup::line_for_offset`と`line_range`のユニットテスト、複数行入力での行番号正確性を検証
  - 理由: 既存テストは属性の存在のみ確認し値を検証していない
