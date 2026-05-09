# template / RenderState 境界整理 設計

作成日: 2026-05-09

## 背景

`docs/todo/TODO.md` には Medium Priority として、`src/template/mod.rs` のテスト集中と `render_page` の巨大 `format!`、および `src/renderer/state.rs` の暗黙的な状態管理が残っている。どちらも外部機能追加ではなく、既存の HTML 生成と Markdown render の保守性・安全性を上げるための構造改善である。

今回の設計は、2件を同じ実装計画で連続実行できる粒度にまとめる。実行順は `template` 整理を先、`renderer` 型化を後にする。

## ゴール

- `src/template/mod.rs` を再エクスポートと薄い公開契約テスト中心へ戻す。
- `template` のテストを `page` / `tree` / `assets` / `message` の責務に合わせて局所化する。
- `render_page` の大きな HTML `format!` を小さな helper に分割する。
- HTML 属性値の escape 経路を helper に集約し、XSS レビュー面積を下げる。
- `RenderState` の open/close 対応を `BlockContext` stack で表現する。
- 通常 Markdown 入力の HTML 出力互換を既存・追加テストで確認する。
- `./verify.sh` を通す。

## 非ゴール

- UI の見た目や文言を意図的に変更しない。
- Markdown 方言や sanitize 方針を変更しない。
- renderer 全体の parser abstraction を作らない。
- Host/Origin 検証、path validation、CSP の既存境界を変更しない。
- 異常な parser event 順序に対する既存 fallback 互換を完全には保証しない。

## 受け入れ条件

- `src/template/mod.rs` の大半を占めるテストが各サブモジュールへ移動している。
- `mod.rs` には公開 API の結合 smoke test と CSP などの公開契約テストだけが残っている。
- `render_page` は `render_head`、`render_workspace_body`、`render_topbar` などの内部 helper へ分割されている。
- `data-memo-file`、`data-current-file` など、外部入力を含む属性値は共通 helper 経由で escape されている。
- `RenderState` は `Vec<BlockContext>` を持ち、少なくとも Heading / CodeBlock / Image / Table 関連の active context を stack 操作で扱っている。
- `finish_*` 系処理は期待 context の不一致を `RenderStateMismatch` 相当で検出できる。
- 通常 Markdown 入力の代表ケースで、見出し、コードブロック、画像、テーブルの HTML 契約が維持されている。
- `./verify.sh` が成功している。

## 全体アーキテクチャ

作業は同じ plan 内で2段階に分ける。

第1段階では `src/template/` を整理する。既存の `assets`、`message`、`page`、`tree` のモジュール構成は維持し、新しい公開 API は原則増やさない。`mod.rs` は入口としての再エクスポートと公開契約テストに限定する。

第2段階では `src/renderer/state.rs` を型化する。現在の `heading`、`code_block`、`image`、`table` の独立した `Option` 群を、block-level の open/close 対応を表す `Vec<BlockContext>` に寄せる。`RenderState` の外部操作窓口は `pub(super)` メソッドとして維持し、`BlockContext` は private enum にする。

通常入力の HTML 互換は守る。一方で、通常 Markdown 入力では発生しない不整合 event については、暗黙 fallback より検出と型化を優先する。

## コンポーネント設計

### template

`src/template/page.rs` はページ全体の組み立てに集中する。外部入口として `render_page(RenderPageParams)` を残し、内部で以下の helper へ分割する。

- `render_html_document`
- `render_head`
- `render_workspace_body`
- `render_topbar`
- `render_sidebar`
- `html_attr`

`SanitizedHtml` の本文と TOC は sanitize 済み HTML として挿入し、二重 escape しない。title、directory name、current file、memo file などの未信頼文字列は、テキスト文脈または属性文脈に合わせて escape する。

`src/template/tree.rs` には file tree 構築と tree HTML のテストを置く。`src/template/assets.rs` には inline asset と CSP hash の整合テストを置く。`src/template/message.rs` には memo/update JSON の契約テストを置く。`src/template/mod.rs` には、公開 API として組み合わせた時に壊れていないことを確認する smoke test だけを残す。

### renderer

`RenderState` は以下のような private enum を内部に持つ。

```rust
enum BlockContext {
    Heading(HeadingState),
    CodeBlock(CodeBlockState),
    Image(ImageState),
    Table(TableState),
    TableCell(TableCellState),
}
```

Table は `BlockContext::Table(TableState)` を stack に積み、cell の active 状態は `TableState` 内で管理する。`TableCell` を独立 context にする案は採らない。これにより、table 全体の alignments と cell index を同じ状態に閉じ込める。

`start_heading`、`finish_heading`、`start_code_block`、`finish_code_block` などは stack top を確認する。期待 context がない場合は `RenderStateMismatch` を返す。`dispatch_event` 側は戻り値を match し、通常経路と mismatch recover を明示する。

`LineLookup`、`HeadingInfo`、syntax highlight、`html_escape`、`sanitize_image_src`、`sanitize_link_href` は既存 helper を使い続ける。

## データフロー

`template` の入口は `RenderPageParams` のままにする。`render_page` は必要な文字列を文脈別に escape し、各 helper に渡す。ユーザー入力を含む属性値は `html_attr` を通す。属性名は外部入力から作らず、コード内の静的な名前だけを使う。

`renderer` の入口は現行どおり `Parser::new_ext(input, markdown_options(MarkdownProfile::Render)).into_offset_iter()` から `dispatch_event` へ流れる。`Start` event は context を push し、`End` event は stack top を検証して HTML を確定する。`Text`、`Code`、`SoftBreak` などの inline event は、現在の context に応じて HTML 出力または context 内の buffer へ蓄積する。

## エラー処理

`template` では、新しい fallible API は導入しない。既存と同様に `String` を返すが、escape の責務を helper に寄せることで、危険な挿入箇所を局所化する。

`renderer` では、`finish_heading` の `Option` fallback と `finish_code_block` の `unreachable!` のような混在を避ける。内部エラー型 `RenderStateMismatch` を導入し、test/debug では不整合を panic または test failure として検出する。release では外部 API の `SanitizedHtml` 返却契約を維持するため、mismatch を `tracing::warn!` で記録し、該当する end event の HTML 確定だけを skip する。未完了 context が残った場合も、未検証の閉じタグを生成せず、その context の buffered HTML は破棄する。

この recover は壊れた HTML を作らないことを優先する。異常系の既存 fallback 互換は非ゴールとする。

## セキュリティ考慮

今回の変更は HTML 文字列組み立てと Markdown event state に触れるため、XSS と unsafe HTML 挿入が主なリスクである。

`template` 側では、属性値 escape を helper に集約する。`SanitizedHtml` ではない外部文字列を HTML として挿入しない。`SanitizedHtml` の content/toc は既存の sanitize 境界を尊重し、二重 escape しない。

`renderer` 側では、`push_html` 相当の経路に入る文字列が、静的タグ、escape 済みテキスト、sanitize 済み URL、syntax highlighter 出力のいずれかであることを維持する。画像 src は `sanitize_image_src`、リンク href は `sanitize_link_href` を使い続ける。

CSP、Host/Origin validation、path traversal 防止、localhost-only 前提は変更しない。

## テスト方針

TDD は2段で行う。

まず `template` 側では、移動先モジュールで既存振る舞いを固定するテストを作る。対象は tree 構築、escape、active path 展開、memo/update JSON、CSP hash、single file / directory page、degraded memo、属性 escape である。巨大 HTML snapshot は避け、構造と契約を小さく確認する。

次に `renderer` 側では、通常入力の互換を既存テストで確認し、追加で見出し、コードブロック、画像 alt、テーブルセルの代表ケースを置く。`RenderStateMismatch` は private API なので、`cfg(test)` の単体テストで state 操作の不整合検出を確認する。

最終検証は `./verify.sh` とする。実装中は必要に応じて targeted test を使う。

## 実行順

1. `template` の既存テストを責務別に分類する。
2. 移動先モジュールに振る舞い固定テストを置く。
3. `mod.rs` のテストを移動し、公開契約 smoke test だけを残す。
4. `render_page` を helper に分割する。
5. 属性 escape helper を導入し、属性値挿入を集約する。
6. `template` の targeted test を通す。
7. `RenderState` に `BlockContext` stack と `RenderStateMismatch` を導入する。
8. Heading / CodeBlock から stack 操作へ移す。
9. Image / Table 関連を stack 操作 API へ寄せる。
10. `dispatch_event` / `handle_*` 側の mismatch 処理を明示する。
11. renderer の通常入力・異常系テストを通す。
12. `./verify.sh` を通す。

## 影響範囲

主な変更対象:

- `src/template/mod.rs`
- `src/template/page.rs`
- `src/template/tree.rs`
- `src/template/assets.rs`
- `src/template/message.rs`
- `src/renderer/state.rs`
- `src/renderer/render.rs`
- `tests/renderer_test.rs`
- 必要に応じた template 関連の module-local tests

依存影響:

- `src/server.rs` や route 層は `render_page` の public contract 経由で影響を受けるが、API を変えないため直接編集しない想定。
- `src/renderer/security.rs`、`src/renderer/highlight.rs`、`src/renderer/line.rs` は依存される側で、原則編集しない。

## ロールバック方針

変更は段階単位で戻せるようにする。`template` 整理で問題が出た場合は `src/template/*` の変更だけを戻す。`renderer` 型化で問題が出た場合は `src/renderer/state.rs`、`src/renderer/render.rs`、追加テストを戻す。

通常入力の HTML 互換が壊れた場合は、型化範囲を Heading / CodeBlock だけに縮小して再計画する。Table / Image の型化が絡みすぎる場合も同じ撤退線を使う。

## 見積もり

人間作業の見積もりは 1.5-2.5 日。Codex/AI 支援では 4-8 時間程度。

renderer 型化で Table / Image の既存契約が想定より絡む場合は、追加で 2-4 時間を見込む。
