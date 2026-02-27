# TODO Issues

## 包括的PRレビュー Round 3 (レビュー日: 2026-02-27)

### エラー処理

- [ ] WebSocket close frameで`user_message()`を使用
  - ファイル: `src/server.rs` (`handle_socket`)
  - 理由: NotUtf8等のエラー時にclose frameが汎用メッセージ「ファイル読み込みエラー」のまま

- [ ] `notify_update`エラーブロードキャストにファイル名を含める
  - ファイル: `src/server.rs` (`notify_update`)
  - 理由: ディレクトリモードで複数ファイル編集時にどのファイルでエラーが起きたか不明

### 型設計

- [ ] `SanitizedHtml`コンストラクタ可視性の厳格化
  - ファイル: `src/renderer.rs`
  - 理由: `pub(crate)`がrenderer/toc以外からの呼び出しを型レベルで防止できない
