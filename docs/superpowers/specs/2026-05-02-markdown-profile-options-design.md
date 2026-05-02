# Markdown 方言オプション共通化 設計

## 目的

Markdown parser options を共通 API に集約し、表示・TOC・検索がどの Markdown 方言を使うかを明示する。検索だけ広めの方言を使う現行方針は維持しつつ、差分を型、関数名、テスト名で読めるようにする。

これにより、将来 Markdown 拡張を追加するときに、表示・TOC・検索のどこへ反映する変更なのかを意識して編集できるようにする。

## 非目標

- footnotes や heading attributes の表示対応は行わない。
- 検索抽出仕様の大幅な変更は行わない。
- 検索負荷制御、parser 走査回数削減、検索結果上限の見直しは別課題として扱う。
- TOC の生成経路は再分割せず、既存どおり `render_document()` 由来にする。

## 現状

表示側は `src/renderer/mod.rs` の `markdown_options()` で `ENABLE_TABLES`、`ENABLE_TASKLISTS`、`ENABLE_STRIKETHROUGH` だけを有効化している。検索側は `src/server/files/search.rs` の別関数で同じ 3 つに加え、`ENABLE_FOOTNOTES`、`ENABLE_HEADING_ATTRIBUTES`、`ENABLE_GFM` も有効化している。

表示と TOC は `render_document()` に統合済みなので、TOC は表示側の Markdown 方言に従う。一方、検索は独立 parser を使うため、表示・TOCより広い方言でテキスト抽出する。この差分自体は検索 UX として許容するが、現在は同名の private 関数が別々に option を組み立てており、意図的な差分か偶発的な差分かがコードから読み取りにくい。

## 設計

共通モジュールを追加する。

```rust
pub enum MarkdownProfile {
    Render,
    Search,
}

pub fn markdown_options(profile: MarkdownProfile) -> pulldown_cmark::Options
```

`MarkdownProfile::Render` は表示 HTML と TOC の profile とし、現在の表示側と同じ `ENABLE_TABLES`、`ENABLE_TASKLISTS`、`ENABLE_STRIKETHROUGH` を有効にする。

`MarkdownProfile::Search` は検索抽出用の profile とし、Render profile の option を土台に `ENABLE_FOOTNOTES`、`ENABLE_HEADING_ATTRIBUTES`、`ENABLE_GFM` を追加する。`ENABLE_GFM` が一部 option を内包する場合でも、Search は Render に差分を足す形で実装し、「検索は表示の上位互換 profile」という意図をコードで表す。

モジュール名は `src/markdown.rs` とする。現時点では options だけを持つ小モジュールだが、Markdown 方言に関する共通契約を置く場所として `renderer` と `server` のどちらにも寄せない。

## コンポーネント境界

- `src/markdown.rs`: `MarkdownProfile` と `markdown_options()` を定義する。profile の差分テストもここへ置く。
- `src/lib.rs`: `pub(crate) mod markdown;` として crate 内に公開する。外部 API としては固定しない。
- `src/renderer/render.rs`: `markdown_options(MarkdownProfile::Render)` を使う。
- `src/renderer/mod.rs`: ローカルの `markdown_options()` を削除する。
- `src/server/files/search.rs`: `markdown_options(MarkdownProfile::Search)` を使い、ローカルの `markdown_options()` を削除する。
- `src/renderer/toc.rs`: 直接 parser options を持たず、`render_document()` 由来の TOC を返す現行構造を維持する。

## データフロー

表示経路では、Markdown 入力が `render_document()` に渡され、`render.rs` が `MarkdownProfile::Render` の options で parser を走査する。本文 HTML と `HeadingInfo` は同じ走査から生成され、TOC はその `HeadingInfo` から作られる。

検索経路では、`search_directory()` が Markdown ファイルを読み、`extract_search_blocks()` が `MarkdownProfile::Search` の options で parser を走査する。検索は footnote 定義本文などを現行どおり検索ブロック化できる一方、raw HTML、inline HTML、リンク本文、画像 alt、コードブロックは既存の除外方針を維持する。

## エラー処理とセキュリティ

`pulldown_cmark::Options` の構築は失敗しないため、新しい fallible API は追加しない。

表示 profile は広げない。これにより、HTML 出力の信頼境界、raw HTML 破棄、URL sanitize、属性値 escape の攻撃面を今回の変更で増やさない。検索 profile は表示より広いが、検索結果は HTML として直接出力されず、既存の検索レスポンスとフロントエンド描画経路で扱う。検索抽出では raw HTML と inline HTML を引き続き無視し、外部文書由来の Markdown を信頼しない。

heading attributes を検索 profile に残しても、表示側の見出し ID は既存の slug 生成に従う。ユーザー入力から任意 ID を表示 HTML に流し込まない方針を維持する。

## テスト計画

TDD として、先に profile の差分テストを追加する。

- `MarkdownProfile::Render` は tables、tasklists、strikethrough を含む。
- `MarkdownProfile::Search` は Render profile の option をすべて含む。
- `MarkdownProfile::Search` だけ footnotes、heading attributes、GFM を含む。
- render/TOC は footnote 定義本文を表示・TOC対象として扱わない。
- search は現行どおり footnote 定義本文を検索ブロック化する。
- heading attributes 付き見出しは、表示 ID と TOC href が既存 slug に従い、検索では見出し本文を拾う。
- raw HTML、危険 URL、リンク本文、画像 alt、コードブロック除外の既存セキュリティテストを維持する。

検証は `cargo test --all-targets --all-features` を実行し、最後に `./verify.sh` を実行する。

## 受け入れ条件

- Markdown option 定義が 1 つの共通 API に集約されている。
- 表示・TOC は `MarkdownProfile::Render`、検索は `MarkdownProfile::Search` を使うことがコードから読める。
- Search profile は Render profile の上位互換として組み立てられている。
- footnotes、heading attributes、GFM の差分がテスト名で明示されている。
- 表示 renderer のセキュリティ境界を広げない。
- 既存の検索挙動を基本維持し、検索抽出の安全側除外を壊さない。
- 必要なテストと `./verify.sh` が成功する。

## 影響範囲

主な変更対象は `src/markdown.rs`、`src/lib.rs`、`src/renderer/mod.rs`、`src/renderer/render.rs`、`src/server/files/search.rs`、関連テスト。外部 JSON レスポンス形状、HTML template、ブラウザ JS、CLI オプションは変更しない。

## ロールバック

問題が出た場合は、呼び出し元を既存のローカル `markdown_options()` に戻し、`src/markdown.rs` と追加テストを削除する。挙動変更を最小にする設計なので、ロールバックは parser option の取得元差し戻しに限定できる。
