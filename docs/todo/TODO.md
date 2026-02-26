# TODO Issues

## 包括的PRレビュー (レビュー日: 2026-02-27)

### 巨大な修正（要別途対応）

- [ ] [Medium] `SanitizedHtml` newtypeの導入
  - ファイル: `src/renderer.rs`, `src/toc.rs`, `src/template.rs`, `src/server.rs`
  - 影響範囲: `render_markdown`, `generate_toc`, `UpdateMessage`, `RenderPageParams` の全シグネチャ
  - 修正方針: `SanitizedHtml(String)` newtypeをrenderer/tocのみで構築可能にし、XSS不変条件を型で表現
  - 理由: type-design-analyzer評価 `UpdateMessage` 4.5/10。サニタイズ済みHTMLが`String`型で未保護

- [ ] [Medium] `FileTreeNode` のenum化
  - ファイル: `src/template.rs`, テスト
  - 影響範囲: `build_file_tree`, `render_file_tree_html`, template tests
  - 修正方針: `FileTreeNode::File { name, full_path }` / `FileTreeNode::Directory { name, children }` に分離
  - 理由: type-design-analyzer評価 3.75/10。ファイルにchildrenを持たせる不正状態が型で防げない

- [ ] [Medium] `RenderPageParams` に `SidebarParams` enum導入
  - ファイル: `src/template.rs`, `src/server.rs`
  - 影響範囲: `render_page` の呼び出し箇所すべて
  - 修正方針: `file_list`/`current_file` のcoupled Optionsを `SidebarParams::SingleFile` / `SidebarParams::Directory { file_list, current_file }` に置換
  - 理由: type-design-analyzer評価 3.75/10。coupled Optionsアンチパターン

### Low Priority

- [ ] `slugify`/`generate_unique_id` の直接ユニットテスト追加
  - ファイル: `src/renderer.rs` (L550-581)
  - 理由: renderer/toc間のID不一致リスク。Unicode-only入力、空入力、連続ハイフンのエッジケースが間接テストのみ

- [ ] `read_markdown_with_limit` のTOCTOU第2段階チェックテスト追加
  - ファイル: `src/server.rs` (L1019-1047)
  - 理由: `take()` による第2段階サイズチェックパスがテストで未検証。リファクタ時の削除リスク

- [ ] `is_safe_href("")` のテスト追加
  - ファイル: `src/renderer.rs` (L642)
  - 理由: セキュリティ境界のエッジケース。空文字列で `#` にフォールバックする動作が未テスト

- [ ] `normalize_authority` のエッジケーステスト追加
  - ファイル: `src/server.rs` (L530-532)
  - 理由: trailing dot (`localhost.:3000`) や mixed case のDNS rebinding関連テストが不足

- [ ] `WatchHandle::shutdown`/`Drop` 動作のテスト追加
  - ファイル: `src/watcher.rs` (L53-107)
  - 理由: shutdown完了タイムアウト、Dropフォールバック動作が未テスト

- [ ] `csp_hash_sources` 戻り値タプル順序のdocコメント明記
  - ファイル: `src/template.rs` (L487-498)
  - 理由: `(script-srcハッシュ, style-srcハッシュ)` の順序がドキュメント未記載

- [ ] `WatchHandle` docコメントにgraceful/fallback shutdown動作の違いを明記
  - ファイル: `src/watcher.rs` (L40-45)
  - 理由: `shutdown()` 非同期グレースフル停止 vs `Drop` ブロッキングフォールバックの区別が未記載

- [ ] `WatcherMessage::FileChanged` で `CanonicalPath` を使用
  - ファイル: `src/watcher.rs` (L13-18)
  - 理由: canonicalize済みパスが `PathBuf` で表現されており型安全性が不足

- [ ] HTTPエラーレスポンスにボディ追加
  - ファイル: `src/server.rs` 複数箇所
  - 理由: bare StatusCodeのみでユーザーへの説明がない（403, 404, 500）

- [ ] `img-src *` のプライバシーリスクをドキュメント化
  - ファイル: `src/server.rs` (L298)
  - 理由: 外部画像によるトラッキングピクセル/閲覧追跡のリスクが未ドキュメント

- [ ] syntectの `ClassedHTMLGenerator` 移行で `style-src 'unsafe-inline'` を排除
  - ファイル: `src/renderer.rs`, `src/template.rs`, `src/server.rs`
  - 修正方針: `highlighted_html_for_string` → `ClassedHTMLGenerator` + CSSクラスベースのテーマスタイルシート生成
  - 理由: 現在syntectがインラインstyleを出力するため `style-src 'unsafe-inline'` が必要。クラスベースに移行すればCSPを強化可能
