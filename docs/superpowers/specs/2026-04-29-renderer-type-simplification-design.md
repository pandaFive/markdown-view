# renderer 型設計整理

## 背景

直近の `render_markdown` 責務分割により、renderer は `src/renderer/render.rs`、`state.rs`、`line.rs`、`security.rs`、`highlight.rs` へ分かれた。公開 API と出力仕様は維持されたが、内部には次の整理余地が残っている。

- `Renderer` struct は `RenderContext` と `RenderState` を所有するだけで、独自の不変条件をほとんど持たない。
- `RenderContext` は `line_lookup`、`syntax_set`、`id_counts` の所有者になっており、実処理の見通しに対して少し重い。
- `RenderState` は `in_code_block: bool` と複数の `Option` を並列に持つため、意味上はあり得ない状態を型で防げない。

今回の整理は、過剰な所有者 struct を減らしながら、意味のある一時状態だけを専用型へ分ける。狙いは「強い状態機械を作る」ことではなく、既存挙動を保ったまま読みやすい内部型へ寄せること。

## ゴール

- `pub fn render_markdown(input: &str) -> SanitizedHtml` の公開契約を維持する。
- Markdown から生成される HTML、行番号属性、heading ID、URL sanitize、syntax highlight fallback を変更しない。
- `Renderer` struct を削除し、`render.rs` を関数中心の構成へ寄せる。
- `RenderContext` をデータ所有型として使わない。残す場合も helper methods の名前空間に限定する。
- `RenderState` の heading / image / code block / table 一時状態を専用 struct へ分け、並列 `Option` による不正状態を減らす。
- 実装は小分けにし、各段階で既存テストにより出力互換を確認できるようにする。

## 非ゴール

- Markdown 出力仕様を変更しない。
- `render_markdown` を `Result` 返却へ変更しない。
- pulldown-cmark 以外の parser へ差し替えない。
- ブラウザ側 JavaScript、memo quote、TOC navigation の仕様を変更しない。
- `RenderState` 全体を巨大なトップレベル状態機械 enum にしない。
- renderer 外部の呼び出し側を変更しない。

## 設計方針

推奨方針は、`Renderer` を削除し、`RenderState` の内部だけを意味のある小さな state 型へ分ける案とする。

`Renderer` は現在 `context` と `state` の所有者であり、実質的には event loop の置き場になっている。これは専用 struct にするほどの不変条件を持たないため、`render(input)` 関数と private helper 関数へ分解する。

一方、`RenderState` の `Option` 群は、単に削るだけでは状態の意味が曖昧になる。ここは `CodeBlockState`、`HeadingState`、`ImageState`、`TableState` に分ける。これにより、例えば `heading_level` と `heading_range` の片方だけが存在する状態や、`image_src` がないのに `image_alt` が残る状態を表しにくくする。

トップレベルの `Normal / InHeading / InImage / InCodeBlock` のような enum 化は行わない。Markdown では heading 内の link / emphasis / image などネストしたイベントがあり、単純な状態機械 enum はかえって分岐と状態遷移を増やすため、今回の KISS / YAGNI の目的に合わない。

## 型構造

`RenderState` は HTML buffer と構文ごとの一時状態を持つ薄い集約にする。

```rust
struct RenderState {
    html_output: String,
    code_block: Option<CodeBlockState>,
    heading: Option<HeadingState>,
    image: Option<ImageState>,
    table: Option<TableState>,
}
```

各 state は関連値をまとめて所有する。

```rust
struct CodeBlockState {
    language: Option<String>,
    content: String,
    start_range: Range<usize>,
}

struct HeadingState {
    level: u8,
    range: Range<usize>,
    plain_text: String,
    html: String,
}

struct ImageState {
    src: String,
    title: Option<String>,
    alt: String,
}

struct TableState {
    in_head: bool,
    alignments: Vec<Alignment>,
    cell_index: usize,
}
```

`Option<CodeBlockState>` や `Option<HeadingState>` は残す。これは「現在その構文の内部にいるか」を表す自然な型であり、既存の `bool + Option` のような重複状態ではない。

## RenderContext とイベント処理

`render.rs` は関数中心にする。

```rust
pub(super) fn render(input: &str) -> SanitizedHtml {
    let line_lookup = LineLookup::new(input);
    let syntax_set = syntax_set();
    let mut id_counts = HashMap::new();
    let mut state = RenderState::new();

    for (event, range) in Parser::new_ext(input, markdown_options()).into_offset_iter() {
        dispatch_event(
            &mut state,
            &line_lookup,
            syntax_set,
            &mut id_counts,
            event,
            range,
        );
    }

    SanitizedHtml::from_sanitized_html(state.into_html())
}
```

`RenderContext` は `line_lookup`、`syntax_set`、`id_counts` の所有者にしない。必要なら `struct RenderContext;` に近い helper 名前空間として残すが、private helper 関数群で十分なら削除する。

判断基準は次の通り。

- 複数 handler から同じ意味で呼ばれる処理は helper 関数にする。
- helper がデータを所有し始めるなら `RenderContext` へ戻さない。ローカル変数を明示的に渡す。
- 空 struct が単なる名前空間にしかならないなら、Rust らしく module-private 関数へ寄せる。

想定 helper は次の範囲に留める。

- `source_line_attrs(line_lookup, range)`
- `block_line_attrs(line_lookup, range)`
- `heading_line_attrs(state, line_lookup)`
- `code_block_line_attrs(state, line_lookup, end_range)`
- `next_heading_id(id_counts, state)`

## 実装分割

実装は一括で行わない。外部出力を固定しながら、次の順で進める。

1. 代表的な renderer 出力テストを確認し、不足があれば `tests/renderer_test.rs` に境界テストを追加する。
2. `RenderState` の内部を `CodeBlockState`、`HeadingState`、`ImageState`、`TableState` へ分ける。
3. `Renderer` struct を削除し、`render.rs` の event loop を `render(input)` と private helper 関数へ移す。
4. `RenderContext` のデータ所有をやめる。空の名前空間型として価値がなければ削除する。
5. 不要になった accessor や helper を削り、公開範囲を最小化する。

各段階は、HTML 出力を変えない refactor として扱う。変更中に出力差分が出た場合は、仕様変更として進めず、原因を切り分けて戻す。

## セキュリティ不変条件

renderer モジュールツリーは `SanitizedHtml` 構築権を持つ信頼境界である。この境界は維持する。

- raw HTML / inline HTML は出力しない。
- 全テキストと属性値は `html_escape` 経由で出力する。
- link と image は別の URL policy で sanitize する。
- protocol-relative URL は拒否する。
- image の remote URL は拒否する。
- `SanitizedHtml::from_sanitized_html` は renderer モジュールツリー外へ公開しない。
- 新しい helper はサニタイズ済み HTML 断片だけを `html_output` へ渡す。

特に `RenderState::push_html` と heading 用の HTML 断片追加は、生文字列混入の入口になりやすい。今回の整理では、これらの呼び出し元を増やす場合、静的タグ、`html_escape` 済み文字列、URL sanitize 済み属性値、または `highlight` helper の出力に限定する。

## テスト方針

既存の `tests/renderer_test.rs` を主な回帰網として使う。特に次の観点を acceptance criteria として確認する。

- raw HTML / inline HTML が破棄される。
- 危険 URL が `#` に置換される。
- link と image の URL policy 差分が維持される。
- heading ID と重複 ID の生成が変わらない。
- `data-source-*` / `data-line-block*` 属性が変わらない。
- code block の syntax highlight と unknown language fallback が変わらない。
- table、task list、blockquote、list の出力が変わらない。
- full pipeline XSS 系の既存テストが通る。

不足がある場合は、実装前に代表ケースを追加する。テスト名は既存方針に合わせて日本語で書く。

検証コマンドは次の通り。

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
./verify.sh
```

Rust renderer 内部のリファクタなので、E2E は必須にしない。ただし行番号属性や memo quote への影響が疑わしい差分が出た場合は、関連 E2E を opt-in で実行する。

## 受け入れ条件

- `render_markdown` の公開 API と呼び出し側を変更せずにビルドできる。
- 既存 renderer / toc / integration テストが通る。
- 追加した境界テストがある場合、それらが通る。
- `./verify.sh` が通る。
- `Renderer` struct が削除されている。
- `RenderContext` がデータ所有者ではなくなっている、または削除されている。
- `RenderState` の heading / image / code block / table 一時状態が専用型へ分かれている。
- `bool + Option` のような重複状態が減っている。

## 影響範囲

主な変更対象は次のファイル。

- `src/renderer/render.rs`
- `src/renderer/state.rs`
- `tests/renderer_test.rs`

確認対象は次のファイル。

- `src/renderer/mod.rs`
- `src/renderer/line.rs`
- `src/renderer/security.rs`
- `src/renderer/highlight.rs`
- `tests/toc_test.rs`
- `tests/integration_test.rs`

原則として、`src/server/`、`src/template/`、ブラウザ側 JavaScript は変更しない。

## ロールバック

公開 API と呼び出し側を変えないため、問題が出た場合はこのリファクタ commit 群を revert する。もし実装を複数 commit に分ける場合は、テスト追加 commit と内部 refactor commit を分け、出力差分が出た refactor commit だけを戻せるようにする。

ロールバック後は `cargo test --all-targets --all-features` と `./verify.sh` を再実行し、renderer のセキュリティ不変条件が戻っていることを確認する。
