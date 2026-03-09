# TODO Issues

## PRレビュー: SanitizedHtmlコンストラクタ可視性厳格化 (レビュー日: 2026-03-02)

### Low Priority

- [x] `SanitizedHtml` structのdocコメント表現をより正確にする
  - ファイル: `src/renderer/mod.rs` L15
  - 現状: 「`render_markdown` / `generate_toc` 経由でのみ生成する設計」
  - 提案: 「`render_markdown` / `generate_toc` が主たる生成経路」（テストコードや将来のサブモジュールからも構築可能なため）
  - 理由: 設計意図としては現状で十分理解可能。低リスク

## PRレビュー: テキスト選択中DOM更新延期 (レビュー日: 2026-03-02)

### Low Priority

- [x] `data.refresh`パスがテキスト選択延期機構をバイパスする
  - ファイル: `src/template.rs`（クライアントJS）
  - 現状: `selectFile(currentFile, false)`経由でDOM更新が走り選択が破壊される
  - 理由: refresh発生頻度が低く、ユーザー影響は最小限。対応するにはrefreshパスにも延期チェックが必要

- [x] テキスト選択延期機構のPlaywright E2Eテスト追加
  - ファイル: 新規テストファイル
  - 内容: ドラッグ選択→WS更新→選択維持、ファイル遷移時の保留クリア、30秒フォールバック等
  - 理由: クライアントJS状態マシンのエッジケースを自動テストでカバー

## PRレビュー: UIテーマ刷新 (レビュー日: 2026-03-09)

### Low Priority

- [ ] `mode_label`/`file_count_label` の値をテストで検証する
  - ファイル: `src/template.rs` (mod tests)
  - 内容: SingleFileモードで"Single file"/"1 file"、Directoryモードで"Directory"/"{N} files"が出力されることを検証
  - 理由: 現在はHTML要素の存在のみ確認、値の正確性は未検証

- [ ] JSコメント `// ディレクトリモード判定` を実態に合わせて更新
  - ファイル: `src/template.rs` (JS内 L1198付近)
  - 現状: コメント直後にディレクトリモード以外のDOM参照が多数追加されている
  - 提案: `// グローバルDOM参照の初期化` に変更

- [ ] `.sidebar-open` を `.topbar-btn` クラスと統合してCSS重複を削減
  - ファイル: `src/template.rs` (CSS)
  - 内容: 8+のプロパティが重複。HTML側で `topbar-btn` クラスを付与し `.sidebar-open` 固有スタイルのみ残す

- [ ] コピーハンドラのJS共通化
  - ファイル: `src/template.rs` (JS `enhanceContentInteractions`)
  - 内容: heading-anchorとcode-copyで `copyText().then().catch()` が同一パターンで重複。`handleCopyClick(button, text, baseLabel)` に抽出

- [ ] `setupTocFilter`/`setupFileFilter` のフィルタロジック汎用化
  - ファイル: `src/template.rs` (JS)
  - 内容: 80%同一のフィルタ処理を `setupFilterableList(inputId, itemSelector, ...)` に統合

- [ ] `setLiveStatus` のラベル自動導出
  - ファイル: `src/template.rs` (JS)
  - 内容: `setLiveStatus('live', 'Live')` が5箇所で重複。state→labelマッピングを内部化し `setLiveStatus('live')` で完結させる
