# TODO Issues

## 包括的PRレビュー Round 2 (レビュー日: 2026-02-27)

### Low Priority

- [ ] `notified: HashSet<PathBuf>` → `HashSet<CanonicalPath>` で型安全性維持
  - ファイル: `src/watcher.rs` (`watch_directory`)
  - 理由: `.as_path().to_path_buf()` ラウンドトリップが型レベルのcanonicalize保証を無効化

- [ ] `SanitizedHtml` serde transparent直列化のユニットテスト追加
  - ファイル: `src/renderer.rs` テスト
  - 理由: `#[serde(transparent)]` 削除時にWebSocket/APIのJSON契約が壊れるリスク

- [ ] `WatchHandle` docコメントにタイムアウト秒数（`SHUTDOWN_TIMEOUT_SECS`）を明記
  - ファイル: `src/watcher.rs` (L42-45)
  - 理由: "一定時間待つ" の具体値が不明

- [ ] `FileTreeNode` に `name()` メソッド追加
  - ファイル: `src/template.rs`
  - 理由: 両バリアントに共通の `name` フィールドへのアクセスを簡潔化

- [ ] コードハイライトがクラスベース出力であることのテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: `syn-` クラスプレフィックスの存在と `style=""` 非存在を検証し、インラインstyle回帰を防止

- [ ] `syntax_theme_css(None)` が非空CSSを返すテスト追加
  - ファイル: `tests/renderer_test.rs`
  - 理由: デフォルトテーマ解決の回帰防止

- [ ] `combined_css("")` の空syntax CSSパスのテスト追加
  - ファイル: `src/template.rs` テスト
  - 理由: CSPハッシュ計算に影響するCSS結合ロジックのエッジケース

- [ ] `csp_hash_sources` docコメントにキャッシュなし（毎回計算）の注意追加
  - ファイル: `src/template.rs` (L534-536)
  - 理由: OnceLock廃止後の性能特性が未ドキュメント

- [ ] プロトコル相対URL拒否理由のコメント復元
  - ファイル: `src/renderer.rs` (`is_safe_href`)
  - 理由: `//example.com/...` 拒否のセキュリティ根拠が削除されたコメントに含まれていた

- [ ] CSP fallbackヘッダーの弱化に関するコメント強化
  - ファイル: `src/server.rs` (L307-321)
  - 理由: フォールバックCSPが `script-src`/`style-src` ハッシュ制約を失う点の明示

- [ ] `AppState.theme()` getter のデッドコード削除
  - ファイル: `src/server.rs`
  - 理由: `render_markdown` から `theme` パラメータ削除後、`theme()` accessor は未使用。`theme` フィールドは `syntax_css` 生成時のみ使用されるため getter 不要

- [ ] `ReadMarkdownError::IntoResponse` のユニットテスト追加
  - ファイル: `src/server.rs` (L795-801)
  - 理由: 現在どのハンドラからも直接使用されていない安全ネット実装。`.into_response()` のJSONボディ形式を検証するテストがない
