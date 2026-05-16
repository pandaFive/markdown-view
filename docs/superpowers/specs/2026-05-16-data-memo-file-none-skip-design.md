# data-memo-file None 時省略設計

## 目的

初期 HTML の `data-memo-file` 表現を、`MemoResponse` / `UpdateMessage` の JSON 契約と揃える。

現状の `src/template/page.rs` は `MemoResponse.file()` が `None` の場合でも `data-memo-file=""` を `<html>` に出力する。一方、`src/template/message.rs` の `MemoResponse` と `UpdateMessage` は `#[serde(skip_serializing_if = "Option::is_none")]` により、`None` の `file` を JSON から省略する。

今回の変更では、`None` を空文字値ではなく「属性なし」として表す。これにより、将来のブラウザ側コードが空文字を有効なメモ対象ファイルとして解釈する余地を減らす。

## 非目的

- ブラウザ JS に新しい `data-memo-file` 読み取り経路は追加しない。
- メモ保存 API、メモ読込 API、WebSocket payload は変更しない。
- `MemoResponse` / `UpdateMessage` の JSON 直列化契約は変更しない。
- ディレクトリモードの file routing は変更しない。
- `data-dir-mode`、`data-current-file`、`data-title` など他の HTML 属性契約は変更しない。

## 背景

`src/template/page.rs` の `render_page` は、次の経路で `<html>` の属性を組み立てている。

- `params.memo.file() == Some(file)`: `data-memo-file="{file}"` を出力する。
- `params.memo.file() == None`: `data-memo-file=""` を出力する。

`src/template/assets/js/` の現行コードは `data-memo-file` を参照していない。そのため、今回の対象は初期 HTML の契約整合に限定できる。

## 方針

`render_page` の `memo_file_attr` 生成を、`Some(file)` のときだけ `html_attr("data-memo-file", file)` を返す形にする。

`None` の場合は空文字を返し、`render_html_document` に渡す `<html lang="ja" data-theme="{theme}"{dir_mode_attr}{memo_file_attr}>` の差し込み結果として属性を出さない。

`Some(file)` 側は既存の `html_attr` をそのまま使う。これにより、ファイル名を HTML 属性へ入れる際の `html_escape` 経路は維持する。

## 受け入れ条件

- `MemoResponse::empty(None)` など `file == None` のページ HTML に `data-memo-file` が含まれない。
- `file == Some("README.md")` のページ HTML には、従来どおり `data-memo-file="README.md"` が含まれる。
- 既存のメモ UI、degraded 表示、`data-dir-mode`、`data-current-file`、`data-title` の出力に影響しない。
- `MemoResponse` / `UpdateMessage` の JSON 直列化契約を変更しない。
- `./verify.sh` が通る。

## 影響範囲

主な変更対象は次のとおり。

- `src/template/page.rs`

テストは `src/template/page.rs` の template unit test に追加する。既存の `test_メモuiが描画される` は `Some("README.md")` の属性出力を固定しているため、これを維持する。

依存として `src/template/message.rs` の `MemoResponse.file()` 契約を参照するが、変更対象にはしない。

## セキュリティ考慮

`Some(file)` の属性出力は既存どおり `html_attr` / `html_escape` を通すため、HTML 属性 escape の安全性は維持する。

`None` 時に空属性を出さないことで、空文字を実在する memo file 識別子として扱う将来実装の余地を減らす。外部入力を新たに評価、実行、または DOM sink へ渡す変更はない。

Host / Origin 検証、CSP、HTML sanitization、path validation、ファイルサイズ制限には影響しない。

## テスト計画

1. `src/template/page.rs` に `file == None` では `data-memo-file` を出力しない unit test を追加する。
2. 既存の `file == Some("README.md")` の属性出力テストを維持する。
3. 実装後に次を実行する。

```bash
cargo test template::page
cargo test --all-targets --all-features
./verify.sh
```

docs-only の設計書作成時点では、文書検証として次を確認する。

```bash
rg -n "目的|非目的|受け入れ条件|セキュリティ|ロールバック|見積もり" docs/superpowers/specs/2026-05-16-data-memo-file-none-skip-design.md
rg -n "TB[D]|TO[DO]|未[定]|あ[と]で" docs/superpowers/specs/2026-05-16-data-memo-file-none-skip-design.md
```

## ロールバック

`src/template/page.rs` の `None` 時処理を `data-memo-file=""` 出力へ戻し、追加した unit test を戻せば従来の HTML 初期出力へ戻せる。

API、WebSocket payload、保存データ形式は変更しないため、ロールバック時のデータ移行は不要。

## 見積もり

- 人間作業: 20-30 分
- Codex/AI 支援: 10-15 分

検証コマンドの実行時間と、既存テスト失敗が見つかった場合の調査時間は別枠とする。
