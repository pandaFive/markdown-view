# render_markdown の責務分割設計

## 背景

`src/renderer/mod.rs` の `render_markdown` は、pulldown-cmark のイベントループ、見出しID生成、行番号属性、コードブロック、画像、リンク、テーブル、HTMLエスケープ、URL sanitize を一つの関数周辺に抱えている。既存テストは厚いが、今後 Markdown 表現やセキュリティポリシーを変更する際に、影響範囲を読み取りにくい。

`docs/todo/BACKLOG.md` では、この課題を「`render_markdown` の責務分割」として管理している。今回の設計は、外部挙動を維持しながら内部境界を整理し、将来の拡張口を作る。

## ゴール

- `pub fn render_markdown(input: &str) -> SanitizedHtml` の公開契約を維持する。
- `render_markdown` 本体をイベントディスパッチ中心に寄せる。
- heading / code block / table / image / link などのフェーズ別処理を `Renderer` / `RenderState` の責務へ分ける。
- `RenderOptions` と `RenderContext` を内部 API として導入し、将来の拡張口を用意する。
- セキュリティ境界を明確にし、raw HTML 破棄、HTMLエスケープ、URL policy、`SanitizedHtml` 構築権限を維持する。

## 非ゴール

- Markdown 出力仕様を変更しない。
- `render_markdown` を `Result` 返却に変更しない。
- `RenderOptions` を公開 API として安定化しない。
- E2E 挙動やブラウザ側 JavaScript の仕様を変更しない。
- pulldown-cmark 以外の Markdown parser へ差し替えない。

## アーキテクチャ

`render_markdown(input)` は空入力なら現行通り空の `SanitizedHtml` を返す。それ以外は `RenderOptions::default()` と `Renderer::new(input, options)` を作り、`renderer.render()` に委譲する。

`src/renderer/` は次の粒度へ分ける。

- `mod.rs`: 公開 API の再エクスポート、`SanitizedHtml`、`render_markdown`、`markdown_options`、syntax theme 系。
- `render.rs`: `Renderer` / `RenderOptions` / `RenderContext` とイベントディスパッチ。
- `state.rs`: `RenderState` と HTML バッファ、heading / code block / image / table の一時状態。
- `line.rs`: `LineLookup`、`source_line_attrs`、`block_line_attrs`。
- `security.rs`: `html_escape`、URL sanitize、`UrlPolicy`。
- `highlight.rs`: `SyntaxSet` を使う code block HTML 生成。

`Renderer` は pulldown-cmark の `(Event, Range<usize>)` を受け取り、`handle_start_tag`、`handle_end_tag`、`handle_text`、`handle_break` などへ分配する。内部では heading / image / table / code block の専用メソッドへ流す。

## コンポーネント

`RenderOptions` は内部向け設定として始める。初期値は現行挙動と同じで、`track_source_lines: true`、`syntax_highlighting: true` を想定する。公開範囲は非公開または `pub(crate)` に留める。

`RenderContext` は入力から導ける共有依存を持つ。具体的には `LineLookup`、`SyntaxSet` 参照、heading ID 用の `id_counts` を持つ。

`RenderState` は HTML バッファと transient state の所有者とする。heading の plain text / HTML、code block の言語・本文・range、image の src/title/alt、table の alignments/head/cell index を集約する。ただし URL sanitize や syntax highlight の判断は持たせず、必要な値を受けて安全な HTML 断片を push する役割に寄せる。

`security.rs` は link と image の URL policy を分ける。link は `http`、`https`、`mailto`、`tel` とローカル参照を許可し、image はローカル参照のみ許可する。protocol-relative URL は引き続き拒否する。

## データフロー

`Renderer::render()` は `Parser::new_ext(input, markdown_options()).into_offset_iter()` を生成し、イベントと byte range を `dispatch_event(event, range)` に渡す。

各イベントでは `RenderContext` から必要な行番号属性を作る。inline text/code は `data-source-*` を持つ `<span>` / `<code>` として出力し、block 要素は `data-line-block-*` を使う。heading と code block は現行仕様通り `data-line-block` と `data-source-*` の両方を持つ。

heading 内では text/code/link/emphasis/strong/del/br/image のうち、現行と同じものだけ `heading_html` に蓄積する。heading ID は `heading_plain_text` から `slugify` と `generate_unique_id` で生成する。画像 alt は text/code/soft break を集め、画像 URL は `sanitize_image_src` を通して `<img>` にする。

code block は start range と end range を合わせて line attrs を作る。syntax highlighting が成功すれば classed HTML を出し、失敗または無効なら escaped plain text を出す。

table は alignments と cell index を state に保持し、head/body の `<th>` / `<td>` と alignment class を現行通り出力する。

## エラー処理

今回の変更はリファクタなので、エラーの表面化は増やさない。`render_markdown` は `Result` を返さない。

syntax highlight の行解析エラーは warn を出して escaped plain text にフォールバックする。テーマ CSS 生成の失敗は現行通り notice CSS へフォールバックする。

未処理の Markdown イベントは現行通り debug ログで無視する。

## セキュリティ不変条件

- raw HTML / inline HTML は出力しない。
- 全テキストと属性値は `html_escape` 経由で出力する。
- link と image は別の URL policy で sanitize する。
- protocol-relative URL は拒否する。
- image の remote URL は拒否する。
- `SanitizedHtml` の構築権限は renderer モジュールツリー内に閉じる。
- 新しい内部 API は公開範囲を最小にする。

## テスト方針

既存の `tests/renderer_test.rs` を主な回帰網として使う。特に raw HTML 破棄、危険 URL の `#` 置換、画像 remote URL 拒否、heading ID、line attrs、code block、table、task list、full pipeline XSS の既存テストを acceptance criteria にする。

追加テストは分割で壊れやすい境界に絞る。

- 代表的な複合 Markdown で `render_markdown` の公開 API 出力を固定する。
- heading 内の link / inline code / emphasis と ID 生成が分割後も一致することを固定する。
- code block の未知言語時に escaped fallback することを固定する。
- link と image の URL policy 差分を同じ入力内で固定する。

検証は `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features`、最終的に `./verify.sh` とする。E2E は Rust renderer の内部リファクタでは必須にしないが、行番号属性や memo quote に不安が残る場合だけ opt-in で対象 spec を走らせる。

## 受け入れ条件

- `render_markdown` の既存呼び出し側を変更せずビルドできる。
- 既存 renderer / toc / integration テストが通る。
- 新規境界テストが追加され、責務分割後の代表ケースを固定している。
- `./verify.sh` が通る。
- `docs/todo/BACKLOG.md` の対象項目を完了扱いにできる実装計画が立てられる。

## 影響範囲

主な変更対象は `src/renderer/` と `tests/renderer_test.rs`。呼び出し側の `src/server/files/content.rs`、`src/template/message.rs`、`src/template/mod.rs`、`tests/integration_test.rs` は外部契約維持の確認対象だが、原則として変更しない。

## ロールバック

外部 API と出力仕様を変えないため、問題が出た場合はこのリファクタ commit を revert する。呼び出し側に変更を入れない設計なので、ロールバック時の依存修正は最小で済む。
