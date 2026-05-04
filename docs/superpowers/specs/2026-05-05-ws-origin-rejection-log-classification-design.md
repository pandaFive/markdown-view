# WebSocket Origin 拒否ログ分類の完全列挙設計

**作成日**: 2026-05-05
**対象ファイル**: `src/server/guards.rs`, `src/server/routes.rs`, `docs/todo/TODO.md`

## 目的

`WsOriginRejection` のログ分類を全 variant 明示にし、将来 variant が追加されたときに分類漏れをコンパイル時またはテスト時に検出しやすくする。

現状は Host 系 `WsOriginRejection::{MissingHost, HostMalformed, UntrustedHost}` を bypass 兆候として `error!` に上げている。一方で `is_allowed_ws_origin()` の非 Host 系ログ分類は `_ => warn!()` に残っており、新しい rejection variant が追加された場合に分類方針の確認漏れを検出できない。`is_host_middleware_bypass_indicator()` も `matches!` の false 側へ暗黙に落ちる。

この設計では Host / Origin の許可判定、HTTP status、WebSocket 拒否レスポンス body は変更しない。変更対象はログ分類、テスト、コメント、TODO 整理に限定する。

## 非目的

- Host / Origin の信頼判定仕様を変更しない。
- localhost-only 前提を変更しない。
- WebSocket のメッセージ形式や upgrade 処理を変更しない。
- Host middleware の route 適用構造を再設計しない。
- 新しい tracing/test crate を導入しない。
- UI 表示やユーザー向けエラー文言を変更しない。

## 推奨アプローチ

`src/server/guards.rs` にログ分類専用の小 helper を追加する。

- `is_host_middleware_bypass_indicator(rejection) -> bool`
- `ws_rejection_log_level(rejection) -> tracing::Level`
- `ws_rejection_log_message(rejection) -> &'static str`

各 helper は `WsOriginRejection` の全 variant を wildcard なしの `match` で列挙する。これにより variant 追加時は、分類方針を更新しない限りコンパイルエラーになる。

ログレベルは以下に固定する。

| Rejection | Level | Message class |
| --- | --- | --- |
| `MissingHost` | `ERROR` | Host middleware bypass 兆候 |
| `HostMalformed` | `ERROR` | Host middleware bypass 兆候 |
| `UntrustedHost` | `ERROR` | Host middleware bypass 兆候 |
| `MissingOrigin` | `INFO` | 通常の WS Origin 拒否 |
| `OriginMalformed` | `WARN` | 通常の WS Origin 拒否 |
| `OriginParseError` | `WARN` | 通常の WS Origin 拒否 |
| `UnsupportedScheme` | `WARN` | 通常の WS Origin 拒否 |
| `OriginMissingAuthority` | `WARN` | 通常の WS Origin 拒否 |
| `UntrustedOriginAuthority` | `WARN` | 通常の WS Origin 拒否 |
| `AuthorityMismatch` | `WARN` | 通常の WS Origin 拒否 |

`is_allowed_ws_origin()` は `check_ws_origin(headers)` の `Err(rejection)` で、上記 helper から level と message を取得し、`emit_ws_rejection_log()` へ集約して 1 箇所からログを出す。現行の `tracing` macro 制約に合わせ、helper 内で level ごとに `error!` / `warn!` / `info!` などへ dispatch する。これにより nested `if/match` と `_ => warn!()` を削除する。

Host 系ログメッセージは、Host middleware bypass だけに断定しすぎず、以下の意味が伝わる文言にする。

> Host middleware bypass、または Host 検証通過後の malformed/untrusted probe。通常運用では到達しない。

## 代替案

### `is_allowed_ws_origin()` 内だけを全列挙にする

既存関数内の `_ => warn!()` を全 variant 列挙へ置き換える案。

差分は最小だが、ログレベル判定、メッセージ判定、ログ出力が同じ関数に残る。分類単位の unit test も書きにくいため採用しない。

### `WsRejectionLogClass` enum を追加する

`HostBypass`, `MissingOrigin`, `OriginRejection` のような分類 enum を作り、そこから level/message を導く案。

将来拡張には強いが、現状は分類が 3 種類だけであり、抽象化が先行する。今回は helper 関数で十分に明示性とテスト容易性を得られるため採用しない。

## データフロー

`ws_handler()` はこれまで通り `is_allowed_ws_origin(&headers)` を呼ぶ。

`is_allowed_ws_origin()` は `check_ws_origin()` で Origin / Host の検証を行う。許可時は `true` を返す。拒否時は `WsOriginRejection` を受け取り、`log_value_for_header()` で Host / Origin を audit log 用の安全な文字列へ変換する。

その後、`ws_rejection_log_level()` と `ws_rejection_log_message()` で分類し、`emit_ws_rejection_log()` 経由でログを出して `false` を返す。ログには人間向け本文に加えて `rejection`, `host`, `origin`, `ws_rejection_class`, `host_recheck_anomaly` の structured fields を含める。

## エラー処理

ユーザー向けのエラー応答は変更しない。WebSocket 接続元拒否時は既存通り `403` と以下の JSON error を返す。

```json
{"error":"WebSocket接続元が許可されていません"}
```

ログ出力は Host 系と Origin 系で分ける。

- Host 系: `MissingHost`, `HostMalformed`, `UntrustedHost` は通常 middleware 後段では到達しないため `ERROR`
- `MissingOrigin`: curl や非ブラウザ client で起こり得るため `INFO`
- その他 Origin 系: malformed / parse error / scheme / authority mismatch の監査信号として `WARN`

## テスト方針

`src/server/guards.rs` の既存 unit test を拡張する。

まず `is_host_middleware_bypass_indicator()` のテストを、全 variant を列挙する分類テストとして維持する。次に `ws_rejection_log_level()` と `ws_rejection_log_message()` の分類も全 variant で固定する。

既存の `test_check_ws_origin_variants_網羅` は維持し、検証結果の variant が変わっていないことを確認する。

`#[traced_test]` は既存の `MissingHost` に加え、`HostMalformed` と `UntrustedHost` を追加する。各 test で Host 系専用メッセージ、対象 variant 名、通常の `WS Origin 拒否` メッセージが出ないことを確認する。

最後に以下を実行する。

```bash
cargo test --all-targets --all-features
./verify.sh
```

## 受け入れ条件

- `WsOriginRejection` のログ分類 helper が wildcard なしで全 variant を列挙している。
- `is_allowed_ws_origin()` に `_ => warn!()` が残っていない。
- Host 系 3 variant の実ログ出力が `#[traced_test]` で固定されている。
- Host / Origin の許可判定仕様が変わっていない。
- `src/server/routes.rs` の `ws_handler` コメントが、Host 再検証の到達条件を正確に説明している。
- `docs/todo/TODO.md` の該当項目が完了扱いに整理されている。
- `cargo test --all-targets --all-features` と `./verify.sh` が通る。

## セキュリティ考慮

この変更は DNS Rebinding 防御の判定を緩めない。Host / Origin の拒否条件は既存の `check_ws_origin()` と `is_trusted_authority()` のまま維持する。

Header 値は外部入力として扱い、ログ出力では既存の `log_value_for_header()` による `"<absent>"` / `"<non-ascii>"` sentinel 化を維持する。ログ分類を追加しても、Header 値を shell、SQL、policy、HTML として実行・解釈しない。

Host 系 rejection が `is_allowed_ws_origin()` へ到達する状況は、Host middleware bypass、または Host 検証通過後に Host header が malformed/untrusted として観測される異常系である。通常運用では到達しないため、`ERROR` として audit signal を強める。

## 影響範囲

- 変更対象: `src/server/guards.rs`, `src/server/routes.rs`, `docs/todo/TODO.md`
- 影響する依存: WebSocket upgrade 前の Origin 検証、Host/Origin 拒否時の監査ログ、`tracing_test` を使う guards unit test
- UI 影響: なし
- API response 影響: なし
- データ永続化影響: なし

## ロールバック

実装コミットを revert すれば元に戻せる。

この変更はログ分類とテスト整理に閉じており、データ移行は不要。問題が出た場合は helper 追加と `is_allowed_ws_origin()` のログ出力変更、TODO 更新を戻せば既存構造へ復帰できる。
