# 未知言語コードブロック fallback ログ設計

**作成日**: 2026-05-16
**対象ファイル**: `src/renderer/highlight.rs`, `tests/renderer_test.rs`

## 目的

未知のコードブロック言語が指定されたときに、現在の安全な plain fallback 描画は維持したまま、ハイライト未適用をログで観測できるようにする。

現状は `find_syntax_by_token()` と `find_syntax_by_extension()` がどちらも失敗すると、`plain_code_block_html` へ落ちて `class="language-{lang}"` 付きの安全な HTML を返す。この描画契約は `tests/renderer_test.rs` の未知言語 fallback テストで固定されている。一方で、ハイライトが効かなかった事実をログから追えないため、silent fallback として残っている。

## 非目的

- ユーザー向け UI 通知は追加しない。
- 対応言語一覧コマンドは追加しない。
- HTML 出力は変更しない。
- ハイライトエンジンや syntect 設定は変更しない。
- 未知言語名の推測や alias 追加は行わない。
- parse error の既存 `warn!` 経路は変更しない。

## 方針

`highlighted_code_html` の syntax lookup 失敗時だけ、`tracing::debug!` で fallback 発生を記録する。

`parse_html_for_line_which_includes_newline` の失敗は既に `tracing::warn!` で観測できるため、今回の対象に含めない。これにより、未知言語と syntect parse error のログ分類を混ぜない。

ログは同一プロセス内で同じ language につき 1 回だけ出す。繰り返しレンダリングやライブ更新で同じ未知言語が何度も出てもログを増やさず、異なる未知言語はそれぞれ初回だけ記録する。

依存は追加しない。標準ライブラリの `OnceLock` と `Mutex<HashSet<String>>`、または同等の小さな内部 helper で重複抑制を行う。

## コンポーネント

### `render_code_block_html`

公開済みの renderer 内部 API として、HTML 出力契約を維持する。

言語指定があり、syntax lookup が失敗した場合も、従来どおり `plain_code_block_html(Some(lang), code, line_attrs)` を返す。

### `highlighted_code_html`

syntax lookup の失敗を検知し、未知言語 fallback の debug ログを出す。

parse error の `warn!` は既存どおり残す。

### 重複抑制 helper

未知言語名をキーにして、初回だけ true を返す小さな helper を追加する。テストではこの helper を直接検証し、tracing subscriber の状態に依存しない安定した unit test にする。

テスト間の独立性が必要なため、helper はテストで状態をリセットできる形にするか、状態を注入できる小さな純粋関数へ寄せる。

## ログとセキュリティ

language 名は Markdown 入力由来の未信頼文字列として扱う。

ログ出力時は制御文字をそのまま混入させない。`escape_debug()` 相当の表現を使い、改行や制御文字がログ行を壊さない形にする。

HTML 出力側は既存どおり `html_escape(lang)` を使い、`class="language-{...}"` の属性 escape 契約を維持する。

この変更は Host / Origin 検証、CSP、HTML sanitization、path validation、ファイル読込制限には影響しない。

## テスト

既存の `test_未知言語コードブロックはフォールバック描画される` は維持し、HTML fallback 出力が変わらないことを確認する。

追加または更新する unit test は次を確認する。

- 同じ未知言語は初回だけログ対象になる。
- 異なる未知言語はそれぞれ初回だけログ対象になる。
- 制御文字を含む language 名がログ安全な表現へ変換される。
- parse error の既存 `warn!` 経路を変更しない。

## 受け入れ条件

- 未知言語コードブロックの HTML fallback 出力が変わらない。
- 未知言語 fallback を `tracing::debug!` で観測できる。
- 同じ未知言語の repeated render で debug ログ対象が 1 回に抑制される。
- 異なる未知言語はそれぞれ 1 回ずつ debug ログ対象になる。
- ログに出す language は制御文字をそのまま混入させない。
- 依存を追加しない。

## 検証

実装後は次を実行する。

```bash
cargo test --test renderer_test
cargo test --all-targets --all-features
./verify.sh
```

docs-only の設計書作成時点では、文書検証として次を確認する。

```bash
rg -n "未知言語|fallback|tracing::debug|escape_debug|受け入れ条件|セキュリティ" docs/superpowers/specs/2026-05-16-unknown-code-language-fallback-log-design.md
rg -n "TB[D]|TO[DO]|未[定]|あ[と]で" docs/superpowers/specs/2026-05-16-unknown-code-language-fallback-log-design.md
```

## 影響範囲

- 変更対象: `src/renderer/highlight.rs`
- テスト対象: `tests/renderer_test.rs` または `src/renderer/highlight.rs` の module test
- 実行時影響: 未知言語コードブロック初回 fallback 時の debug ログのみ
- HTML / WebSocket / HTTP API への影響: なし

## ロールバック

`src/renderer/highlight.rs` と追加・更新したテストを revert すれば元に戻せる。

HTML 出力契約を変えないため、ロールバック時にユーザー表示や既存 Markdown の互換性へ追加影響はない。
