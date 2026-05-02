# 見出し ID 生成単一パス化 設計

## 目的

本文 HTML の見出し `id` と TOC の `href` を、同じ `HeadingInfo` から生成する。これにより、画像 alt、inline code、soft/hard break、重複見出しを含む境界ケースでも、本文と TOC の ID が乖離しないことをデータフローで担保する。

## 非目標

- 検索処理の Markdown パース統合は行わない。
- Markdown 方言オプションの表示・TOC・検索間の整理は行わない。
- `HeadingInfo.level` の `NonZeroU8` 化や TOC level=0 ガードは別 TODO として扱う。
- WebSocket、template、JSON レスポンスの外部契約は変更しない。

## 現状

`render_markdown` は `src/renderer/render.rs` で pulldown-cmark を走査し、`handle_heading_end` で `id_counts` から見出し ID を生成する。`toc::generate_toc` は `extract_headings` を通じて別の parser と別の `id_counts` を使い、`HeadingInfo` を作る。

この構造では、render 側と TOC 側で見出しテキスト抽出ルールが少しでもずれると、本文 `<h*> id` と TOC `href` が silent に乖離する。特に画像 alt、inline code、soft/hard break の扱いは両実装にまたがるため、将来の変更時に不変条件が崩れやすい。

## 設計

`renderer` に統合結果型を追加する。

```rust
pub struct RenderedDocument {
    pub content: SanitizedHtml,
    pub toc: SanitizedHtml,
    pub headings: Vec<HeadingInfo>,
}
```

`render_document(input: &str) -> RenderedDocument` を追加し、1 回の `Parser::new_ext(input, markdown_options()).into_offset_iter()` で HTML と `Vec<HeadingInfo>` を同時に生成する。render 側の見出し終了処理で確定した level、plain text、id を `HeadingInfo` として記録し、その `headings` を `toc::build_toc_html` に渡して TOC を生成する。

既存 API は互換維持する。

- `render_markdown(input)` は `render_document(input).content` を返す。
- `toc::generate_toc(input)` は `render_document(input).toc` を返す。
- `extract_headings(input)` は互換用 wrapper として残し、`render_document(input).headings` を返す。

サーバー経路では `src/server/files/content.rs` の `read_and_render_file()` を `render_document(&markdown)` に切り替え、`UpdateMessage::new(document.content, document.toc, ...)` の形で使う。これにより、通常の表示更新では HTML と TOC が常に同じ parser 走査結果から作られる。

## コンポーネント境界

- `src/renderer/render.rs`: HTML 生成と見出し収集を同じ走査で行う。`RenderState` に見出し終了時の情報を外へ返すか、render loop 側で `Vec<HeadingInfo>` に push する。
- `src/renderer/mod.rs`: `RenderedDocument` と `render_document` を公開し、既存 wrapper を維持する。
- `src/renderer/toc.rs`: `HeadingInfo` から TOC HTML を作る責務に限定する。必要に応じて `build_toc_html` の可視性を `pub(in crate::renderer)` に広げる。
- `src/server/files/content.rs`: HTML と TOC の別々の呼び出しを統合 API へ置き換える。

## データフロー

1. Markdown 入力を `render_document` に渡す。
2. parser を 1 回だけ走査する。
3. 見出し開始時に level と source range を `RenderState` に記録する。
4. 見出し内の text/code/break は render 用 HTML fragment と plain text に同時反映する。画像 alt は既存どおり見出し ID 用 plain text から除外する。
5. 見出し終了時に plain text から slug を作り、共有 `id_counts` で一意 ID を確定する。
6. 同じ ID を HTML の `<h*> id` と `HeadingInfo.id` に使う。
7. 全走査後、`HeadingInfo` から TOC を生成する。

## エラー処理とセキュリティ

Markdown parser の既存挙動に合わせ、未対応イベントは debug log で無視する。raw HTML / inline HTML は引き続き出力しない。リンク URL と画像 URL の sanitize、本文テキストと属性値の escape、TOC の text/id escape は維持する。

`SanitizedHtml` の構築権限は renderer モジュール内に閉じる。統合 API は raw HTML 文字列ではなく `SanitizedHtml` を返すため、server/template 側の信頼境界は広げない。

## テスト計画

- `tests/renderer_test.rs` の既存一致テストを拡張し、画像 alt + inline code + soft break を含む見出しで本文 ID と TOC href が一致することを確認する。
- hard break を含む見出しでも本文 ID と TOC href が一致することを確認する。
- 重複見出しの連番が本文と TOC で同じ順序になることを確認する。
- raw HTML を含む入力で raw HTML が本文に出力されず、TOC text/id が escape されることを確認する。
- `cargo test --test renderer_test` と `cargo test --test toc_test` を先に実行し、最後に `./verify.sh` を実行する。

## 受け入れ条件

- サーバーの通常表示更新経路で、Markdown 本文の parser 走査が render/toc 用に分離されていない。
- `render_markdown`、`toc::generate_toc`、`extract_headings` の既存呼び出しはコンパイル互換を維持する。
- 本文 HTML の見出し ID と TOC href が、境界ケースを含め同じ `HeadingInfo` 由来になる。
- raw HTML 破棄、URL sanitize、escape の既存セキュリティ性質が維持される。
- 必要なテストと `./verify.sh` が成功する。

## 影響範囲

主な変更対象は `src/renderer/mod.rs`、`src/renderer/render.rs`、`src/renderer/toc.rs`、`src/server/files/content.rs`、`tests/renderer_test.rs`。外部レスポンス形状、HTML template、ブラウザ JS の変更は想定しない。

## ロールバック

問題が出た場合は、`read_and_render_file()` を `render_markdown(&markdown)` と `generate_toc(&markdown)` の組み合わせへ戻し、追加した `render_document` と関連テストを戻す。既存公開 API を維持するため、ロールバック時の利用側変更は限定的に済む。
