# syntax_theme_css fallback CSS 非注入設計

## 目的

`syntax_theme_css` がテーマ解決または syntect CSS 生成に失敗したとき、画面上の fallback 通知 CSS を注入せず、構文ハイライトだけを無効化する。

現在の fallback CSS は `body::before` を使うため、`src/template/assets/css/base.css` が既に使っている `body::before` の背景レイヤーを上書きする。失敗検知は既存の `tracing::warn!` に寄せ、UI への副作用を増やさない。

## 非目標

- 画面上の通知 UI は追加しない。
- CSP hash の生成方式は変更しない。
- 通常テーマの CSS 出力は変更しない。
- `base.css` の `body::before` 背景レイヤーは変更しない。
- テーマ名の起動時検証方針は変更しない。

## 背景

`src/renderer/mod.rs` の `syntax_theme_css` は、テーマが見つからない場合または `css_for_theme_with_class_style` が失敗した場合に `highlight_disabled_notice_css()` を返す。この CSS は `body::before` に固定通知を描画する。

一方、`src/template/assets/css/base.css` も `body::before` を背景グリッド用途で使っている。`src/template/assets.rs` の `combined_css` はベース CSS の後ろに `syntax_css` を連結するため、fallback CSS が発生すると後勝ちで背景レイヤーを上書きする。

CSP については `csp_hash_sources` が `combined_css(syntax_css)` の実体から style hash を計算するため、fallback CSS を返す場合も、空文字を返す場合も、CSP 整合性は保たれる。

## 方針

失敗時の戻り値を空文字にする。

- `resolve_theme` 失敗時は warn log を残して `String::new()` を返す。
- `css_for_theme_with_class_style` 失敗時も warn log を残して `String::new()` を返す。
- `highlight_disabled_notice_css()` は削除する。
- `syntax_theme_css` の doc コメントを「空文字は構文ハイライト無効を表す。UI 通知 CSS は注入しない」方針へ更新する。

これにより失敗時は `combined_css("")` の既存契約に合流し、ページへ埋め込まれる CSS はベース CSS のみになる。

## 受け入れ条件

- 無効なテーマ名を指定した `syntax_theme_css` は空文字を返す。
- 無効なテーマ名の失敗時 CSS に `body::before` が含まれない。
- `combined_css("")` はベース CSS のみを返す既存契約を維持する。
- `csp_hash_sources` は実際に埋め込まれる CSS と一致する style hash を返す。
- `./verify.sh` が通る。

## 影響範囲

主な変更対象は次のとおり。

- `src/renderer/mod.rs`
- `tests/renderer_test.rs`
- `docs/todo/TODO.md`

依存として `src/template/assets.rs` の `combined_css` と `csp_hash_sources` の既存契約を利用するが、変更対象にはしない。

## セキュリティ考慮

fallback CSS を削除しても、CSP hash は実際に埋め込まれる style 内容から計算されるため整合性は維持される。むしろグローバルな `body::before` 注入をやめることで、テーマ失敗時に予期しない UI 上書きが起きる面を減らす。

外部入力由来の CSS は追加しない。構文ハイライト失敗は warn log に残すため、silent failure にはしない。

## テスト計画

1. `cargo test --all-targets --all-features renderer_test::test_syntax_theme_css_無効テーマは空文字を返しui_cssを注入しない`
2. `cargo test --all-targets --all-features template::assets`
3. `./verify.sh`

最終確認は `./verify.sh` を必須とする。

## ロールバック

`syntax_theme_css` の失敗時戻り値を現行の fallback CSS に戻し、追加または更新したテストと `docs/todo/TODO.md` の完了整理を戻せば元の状態へ戻せる。

## 見積もり

- 人間作業: 20-35 分
- Codex/AI 支援: 10-20 分

検証コマンドの実行時間と、既存テスト失敗が見つかった場合の調査時間は別枠とする。
