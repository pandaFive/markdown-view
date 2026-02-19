# TODO Issues (レビュー日: 2026-02-18)

## Low Priority

- [ ] `SyntaxSet::load_defaults_newlines()` を `LazyLock` でキャッシュ
  - ファイル: `src/renderer.rs:17`
  - 理由: 毎回ロードは非効率。初回ロードのみにすることでレンダリング性能が向上する
  - 備考: `std::sync::LazyLock`（Rust 1.80+）を使用

- [ ] AppState にコンストラクタを追加
  - ファイル: `src/server.rs:16-21`
  - 理由: 構造体の全フィールドが`pub`でバリデーションなしに生成可能。コンストラクタでポート範囲チェック等を追加

- [ ] テンプレートの単体テスト追加
  - ファイル: `src/template.rs`
  - 理由: `render_page` のXSSエスケープ、ダークモード切替等のテストが未整備

- [ ] 未知言語コードブロックのテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: syntectが認識しない言語指定時のフォールバック動作のテストが未整備

- [ ] テーブルalignment対応
  - ファイル: `src/renderer.rs:338-341`
  - 理由: テーブルのセル揃え（left/center/right）が未実装

- [ ] ファイルサイズ上限のテスト追加
  - ファイル: `tests/integration_test.rs`
  - 理由: `MAX_FILE_SIZE` 超過時の動作テストが未整備
