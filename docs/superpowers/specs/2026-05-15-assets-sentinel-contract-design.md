# assets sentinel 契約テスト設計

## 目的

`src/template/assets/` の CSS/JS バンドルで使う sentinel 文字列の混入をテストで検知する。

対象 sentinel は次の2つとする。

- `__DARK_THEME_VARS__`
- `__MAX_FILE_SIZE_MB__`

既存の production code の挙動は変更しない。置換処理の helper 化、実行時エラー処理、`MAX_FILE_SIZE` 定数、ユーザー向け文言、CSP hash 計算は非目標とする。

## 背景

`css_bundle.rs` は `TEMPLATE.replace("__DARK_THEME_VARS__", dark_theme_vars)` で CSS テンプレートを置換する。`inline_script.rs` は `TEMPLATE.replace("__MAX_FILE_SIZE_MB__", ...)` で JS テンプレートを置換する。

現在、`__DARK_THEME_VARS__` は `css/base.css` に意図的に存在し、`__MAX_FILE_SIZE_MB__` は `js/bootstrap.js` に意図的に存在する。将来、他の CSS/JS include 元に同じ文字列が混入すると、意図しない置換が発生し、inline asset と CSP hash 対象が予期せず変わる可能性がある。

## 方針

各 sentinel の契約は、置換元に近いモジュールの単体テストで固定する。

- `css_bundle.rs`
  - `css/base.css` だけが `__DARK_THEME_VARS__` を含んでよい。
  - その他の CSS include 元には `__DARK_THEME_VARS__` が含まれてはならない。
  - 結合済み `TEMPLATE` の出現回数は、`css/base.css` の期待出現回数と一致する。
  - 生成済み CSS には `__DARK_THEME_VARS__` が残らない。
- `inline_script.rs`
  - `js/bootstrap.js` だけが `__MAX_FILE_SIZE_MB__` を含んでよい。
  - その他の JS include 元には `__MAX_FILE_SIZE_MB__` が含まれてはならない。
  - 結合済み `TEMPLATE` の `__MAX_FILE_SIZE_MB__` 出現回数は 1 とする。
  - `inline_js(crate::server::MAX_FILE_SIZE)` の生成結果には sentinel が残らず、`maxFileSizeMb: 10` が含まれる。

公開 API は増やさない。テストは private const と private function に近い場所へ追加する。

## 受け入れ条件

- `__DARK_THEME_VARS__` は `base.css` の期待箇所以外に存在しないことがテストで固定される。
- `__MAX_FILE_SIZE_MB__` は `bootstrap.js` の期待箇所以外に存在しないことがテストで固定される。
- `inline_js(MAX_FILE_SIZE)` の生成結果に sentinel が残らず、フロントエンド設定値として `maxFileSizeMb: 10` が含まれる。
- 既存の inline CSS/JS 生成挙動、CSP hash 計算、ユーザー向け文言は変わらない。
- `./verify.sh` が通る。

## 影響範囲

直接の変更対象は次の2ファイルのテストコードに限定する。

- `src/template/assets/css_bundle.rs`
- `src/template/assets/inline_script.rs`

依存として、次の include 元ファイルの sentinel 混入が検査対象になる。

- `src/template/assets/css/*.css`
- `src/template/assets/js/*.js`

`crate::server::MAX_FILE_SIZE` は読み取り依存のみとし、値は変更しない。

## セキュリティ考慮

sentinel 混入は、意図しない CSS/JS 置換を通じて inline asset の内容を変える。CSP hash は生成後の asset に基づいて計算されるため CSP 整合性そのものは壊れにくいが、開発者が意図しない asset 差分を見逃すリスクがある。今回のテストは、許可箇所以外の sentinel 混入を検知し、CSP 対象 asset の予期しない変化をレビュー前に止める。

## テスト計画

1. `cargo test --all-targets --all-features template::assets::css_bundle`
2. `cargo test --all-targets --all-features template::assets::inline_script`
3. `./verify.sh`

最終確認は `./verify.sh` を必須とする。

## ロールバック

追加したテストコードを削除すれば元の状態へ戻せる。production code は変更しないため、実行時挙動のロールバック作業は不要。

## 見積もり

- 人間作業: 20-40 分
- Codex/AI 支援: 10-20 分

検証コマンドの実行時間と、既存テスト失敗が見つかった場合の調査時間は別枠とする。
