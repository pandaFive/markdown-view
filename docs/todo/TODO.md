# TODO Issues

## 現在の未完了タスク

### Low Priority

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
