# render_markdown silent failure 観測性強化設計

## 背景

直近の `render_markdown` 責務分割と renderer 型整理により、renderer は `src/renderer/render.rs`、`state.rs`、`line.rs`、`security.rs`、`highlight.rs` に分かれた。公開 API と HTML 出力互換は維持されたが、互換性を優先して残した silent fallback がある。

`docs/todo/BACKLOG.md` では、この課題を「`render_markdown` 責務分割後の silent failure 観測性強化」として管理している。対象は `heading_line_attrs`、`code_block_line_attrs`、`finish_heading` の `None` 経路、未処理 Markdown event/tag のログ、コードハイライト fallback の観測性である。

今回の設計は、通常ユーザー向けの出力仕様を変えず、開発時に内部契約違反や観測経路の消失を検知しやすくする。

## ゴール

- `pub fn render_markdown(input: &str) -> SanitizedHtml` の公開契約を維持する。
- Markdown から生成される HTML、行番号属性、heading ID、URL sanitize、syntax highlight fallback を変更しない。
- `heading_line_attrs` と `code_block_line_attrs` の active state 前提を debug/test で検知できるようにする。
- `finish_heading` が active heading なしで呼ばれた場合を debug/test で検知できるようにする。
- 未処理 Markdown event/tag の `tracing::debug!` 経路が消えないように固定する。
- コードハイライト parse 失敗時の `tracing::warn!` 経路と escaped fallback を固定する。
- 既存の security invariant を維持する。

## 非ゴール

- `render_markdown` を `Result` 返却へ変更しない。
- release build で新しい panic を増やさない。
- 未知言語コードブロックを warning 扱いにしない。
- fallback marker や診断用属性を HTML 出力へ追加しない。
- raw HTML / inline HTML の破棄方針を変えない。
- link / image の URL sanitize 方針を変えない。
- E2E やブラウザ側 JavaScript の仕様を変更しない。

## 推奨方針

推奨方針は、debug/test で内部契約を固定し、release の HTML 出力互換を維持する案とする。

`heading_line_attrs` と `code_block_line_attrs` は、pulldown-cmark の `Start(Heading)` / `End(Heading)`、`Start(CodeBlock)` / `End(CodeBlock)` の対応が保たれる前提で呼ばれる。ここで active state がない場合は、通常入力のエラーではなく renderer 内部の契約違反として扱う。ただし今回の目的は互換性維持なので、release では現行通り空属性 fallback を残す。

`finish_heading` は現在 `Option<String>` を返すため、呼び出し側が `None` を無視すると heading 終了イベントが silent に消える。これも debug/test では契約違反として検知し、release では現行互換の `None` fallback を維持する。

未処理 Markdown event/tag は `debug!` を維持する。raw HTML 破棄や parser の将来拡張を考えると、未処理分岐を `warn!` や panic に昇格すると通常入力で過剰なノイズや停止を招く。

コードハイライトは、未知言語を正常な escaped fallback として扱う。観測対象は syntect の行解析が失敗した場合に限定し、既存の `warn!` と escaped fallback を固定する。

## アーキテクチャ

変更は既存の renderer 内部境界に沿って入れる。

- `src/renderer/render.rs`: event dispatch と、状態から行番号属性を組み立てる helper の責務を維持する。`heading_line_attrs` と `code_block_line_attrs` に debug/test 向けの契約検知を追加する。
- `src/renderer/state.rs`: transient state の所有者として扱う。`finish_heading` の active heading 前提を debug/test で明示する。
- `src/renderer/highlight.rs`: code block HTML 生成の責務を維持する。未知言語 fallback は正常系のまま、parse 失敗時の warn fallback を固定する。
- `tests/renderer_test.rs` または renderer module tests: 公開出力互換を壊さず、内部契約と観測経路の回帰を狭く固定する。

新しい公開型や公開 API は追加しない。必要な helper は module-private または `pub(super)` に留める。

## データフロー

通常の render フローは変えない。

1. `render(input)` が `LineLookup`、`SyntaxSet`、heading ID 用 `HashMap`、`RenderState` を作る。
2. `Parser::new_ext(input, markdown_options()).into_offset_iter()` から `(Event, Range<usize>)` を受け取る。
3. `dispatch_event` が `handle_start`、`handle_end`、text/code/break 系 handler へ分配する。
4. `RenderState` が HTML バッファと heading / image / code block / table の一時状態を管理する。
5. 最後に `SanitizedHtml::from_sanitized_html(state.into_html())` を返す。

見出し終了時は、`state.heading_plain_text()` から slug と unique ID を作り、`heading_line_attrs(line_lookup, state)` で行番号属性を作る。その後 `state.finish_heading(id, heading_attrs)` が heading HTML を返し、呼び出し側が HTML バッファへ push する。

コードブロック終了時は、`code_block_line_attrs(end_range, line_lookup, state)` で start/end range を合わせた行番号属性を作り、`state.finish_code_block(syntax_set, line_attrs)` が `highlight.rs` 経由で HTML を push する。

この流れ自体は変更しない。追加するのは、前提が崩れた場合に debug/test で検知する契約チェックである。

## エラー処理と観測性

### 内部契約違反

`heading_line_attrs`、`code_block_line_attrs`、`finish_heading` は、対応する active state がある前提で呼ばれる。active state がない場合は debug/test で検知する。

実装候補は `debug_assert!` を中心にする。release では既存の `unwrap_or_default()` や `Option<String>` fallback を維持し、HTML 出力差分や新しい runtime panic を作らない。

### 意図した無視

`Event::Html` と `Event::InlineHtml` は XSS 防止のため出力せず破棄する。これは silent failure ではなく安全仕様なので、警告化しない。

未処理 Markdown event/tag は、現行通り `tracing::debug!` で記録して無視する。将来 pulldown-cmark が event/tag を増やした場合に、通常利用を止めず開発時に追跡できる状態を維持する。

### ハイライト fallback

未知言語は warning ではなく正常 fallback とする。Markdown では任意の info string が許され、未知言語はユーザー入力として普通に起きるためである。

syntect の `parse_html_for_line_which_includes_newline` が失敗した場合は、既存通り `tracing::warn!` を出し、escaped plain text へ fallback する。この経路が消えないようにテストで固定する。

## セキュリティ不変条件

- raw HTML / inline HTML は出力しない。
- 全テキストと属性値は `html_escape` 経由で出力する。
- link と image は別の URL policy で sanitize する。
- protocol-relative URL は拒否する。
- image の remote URL は拒否する。
- `SanitizedHtml` の構築権限は renderer モジュールツリー内に閉じる。
- 診断用 marker や内部状態を HTML に追加しない。
- ログへ未サニタイズ HTML 本文や巨大な入力全体を出さない。

今回の観測性強化は、開発時検知と既存ログ経路の固定に限定する。ユーザー入力を追加でログ出力する場合は、必要最小限の種別や言語名に留め、Markdown 本文や危険 URL の原文を安易に出さない。

## テスト方針

テストは renderer の単体テスト中心にする。目的は挙動変更ではなく、観測経路と内部契約検知が消えないことの固定である。

追加候補は次の通り。

- `state.rs` module test: `finish_heading` が active heading なしで呼ばれたとき、debug/test では契約違反として検知されることを固定する。
- `render.rs` module test: `heading_line_attrs` が active heading なしなら debug/test で検知されることを固定する。
- `render.rs` module test: `code_block_line_attrs` が active code block なしなら debug/test で検知されることを固定する。
- renderer public test: 見出しとコードブロックの通常経路で `data-line-block` と `data-source-*` が引き続き付くことを確認する。既存テストで十分なら追加しない。
- 未処理 event/tag: 依存追加なしでログ捕捉が難しい場合は、小さな helper へ分け、未処理分岐が `IgnoredMarkdownEvent` のような観測対象を返すことを module test で固定する。依存追加は最終手段にする。
- `highlight.rs` module test: parse 失敗時 fallback を安定して発火できない場合は、fallback 分岐を小さな helper に分け、warn 対象の失敗が escaped HTML に落ちることを固定する。

テスト名は既存方針に合わせて日本語で書く。新しい dev-dependency は YAGNI として原則追加しない。

## 検証

実装後は次を実行する。

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
./verify.sh
```

Rust renderer 内部の変更なので E2E は必須にしない。ただし行番号属性の HTML 出力に差分が出た場合は、memo quote や markdown link の関連 E2E を opt-in で実行する。

## 受け入れ条件

- `render_markdown` の公開 API と呼び出し側を変更せずにビルドできる。
- 既存 HTML 出力互換テストが通る。
- `heading_line_attrs`、`code_block_line_attrs`、`finish_heading` の内部契約違反が debug/test で検知できる。
- 未処理 Markdown event/tag の `debug!` 観測経路が消えないことを固定できている。
- コードハイライト parse 失敗時の `warn!` 経路または warn 対象 fallback が固定されている。
- 未知言語コードブロックは warning にならず、escaped fallback を維持する。
- raw HTML 破棄、URL sanitize、HTML escape の security invariant が維持されている。
- `./verify.sh` が通る。
- `docs/todo/BACKLOG.md` の対象項目を完了扱いにできる実装計画が立てられる。

## 影響範囲

主な変更対象は次のファイル。

- `src/renderer/render.rs`
- `src/renderer/state.rs`
- `src/renderer/highlight.rs`
- `tests/renderer_test.rs`

確認対象は次のファイル。

- `src/renderer/mod.rs`
- `src/renderer/line.rs`
- `src/renderer/security.rs`
- `tests/toc_test.rs`
- `tests/integration_test.rs`
- `src/server/files/content.rs`
- `src/template/message.rs`

原則として `src/server/`、`src/template/`、ブラウザ側 JavaScript は変更しない。

## ロールバック

公開 API と通常 HTML 出力を変えないため、問題が出た場合はこの観測性強化の実装 commit 群を revert する。仕様書 commit はそのまま残して再計画してもよいが、設計自体が不適切だった場合は仕様書 commit も revert する。

実装を複数 commit に分ける場合は、テスト追加、内部契約チェック、fallback helper 整理を分け、問題のある commit だけを戻せるようにする。ロールバック後は `cargo test --all-targets --all-features` と `./verify.sh` を再実行する。
