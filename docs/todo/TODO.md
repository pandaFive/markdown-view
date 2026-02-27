# TODO Issues

## 包括的PRレビュー Round 3 (レビュー日: 2026-02-27)

### エラー処理

- [x] ~~WebSocket close frameで`user_message()`を使用~~ (完了: 2026-02-28)

- [ ] `notify_update`エラーブロードキャストにファイル名を含める
  - ファイル: `src/server.rs` (`notify_update`)
  - 理由: ディレクトリモードで複数ファイル編集時にどのファイルでエラーが起きたか不明

### 型設計

- [ ] `SanitizedHtml`コンストラクタ可視性の厳格化
  - ファイル: `src/renderer.rs`
  - 理由: `pub(crate)`がrenderer/toc以外からの呼び出しを型レベルで防止できない

## PRレビュー: WebSocket close frame修正 (レビュー日: 2026-02-28)

### Low Priority

- [ ] WebSocket close codeをエラーバリアント別に精緻化
  - ファイル: `src/server.rs` L668-672
  - 現状: 全バリアントで`1011`（Internal Error）を使用
  - 推奨: `Io`→1011、`TooLarge`→1009（Message Too Big）、`NotUtf8`→1003（Unsupported Data）
  - 理由: RFC 6455準拠のセマンティクス改善。ローカルツールのため実影響は最小限
