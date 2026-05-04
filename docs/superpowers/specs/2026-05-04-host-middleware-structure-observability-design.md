# Host middleware 構造契約と WebSocket bypass 観測性強化設計

**作成日**: 2026-05-04
**対象ファイル**: `src/server/routes.rs`, `src/server/guards.rs`, `tests/integration_test.rs`

## 目的

Host middleware 化済みの DNS Rebinding 防御を、将来の route/layer 追加で壊れにくく、壊れた場合に気付きやすい形へ強化する。

現状は `create_router()` が `build_routes()` の戻り値へ Host middleware と security header layer を適用している。コメントで「新規 route は `build_routes()` 内へ追加する」と明記されているが、`build_routes()` の戻り値は通常の `Router<Arc<AppState>>` であり、route 定義専用という契約は型では表現されていない。

また、WebSocket 経路では Host middleware 後段の `check_ws_origin()` に Host 再検証を defense-in-depth として残している。ここで Host 系拒否に到達した場合は middleware bypass の兆候だが、現状は通常の WebSocket Origin 拒否と同じ分類に見えやすい。

## 非目的

- 許可 Host / Origin の判定仕様を変更しない。
- localhost-only 前提を変更しない。
- 外部公開、認証、認可、TLS 終端、proxy 対応を追加しない。
- CSP や security header の内容を再設計しない。
- WebSocket の機能やメッセージ形式を変更しない。
- UI 表示を変更しない。

## 推奨アプローチ

`build_routes()` の戻り値を裸の `Router<Arc<AppState>>` から private newtype に包む。

例:

```rust
struct RouteDefinitions(Router<Arc<AppState>>);
```

`build_routes() -> RouteDefinitions` とし、route 登録だけを行う関数であることを型名で表現する。`create_router()` だけが `RouteDefinitions` から中身を取り出し、Host middleware、security header layer、CSP、`with_state` を適用する。

`RouteDefinitions` には共通 `.layer(...)` を生やさない。共通 layer 適用は `create_router()` 側、または `apply_security_layers` 相当の private helper に閉じ込める。これにより、route 定義と security layer 適用の境界を名前と型で分ける。

WebSocket の Host 系拒否は、既存の `WsOriginRejection::{MissingHost, HostMalformed, UntrustedHost}` を bypass indicator として扱う。Host middleware 後段でこれらに到達した場合は `tracing::error!` で記録し、通常の Origin 欠落や authority mismatch と区別する。

## 代替案

### route 一覧の統合テストのみを追加する

既存の `test_host_middlewareは全http_routeの不正hostを拒否する` を拡張し、route 漏れ検知をテストに寄せる案。

実装は小さいが、`build_routes()` が route 定義専用である契約は引き続きコメント依存になる。将来 `build_routes()` 内へ共通 layer を混ぜる変更も型では検出できない。

### Host 検証を handler ごとに戻す

各 handler で `ensure_allowed_request_host` を明示呼び出しする案。

局所的には分かりやすいが、route 追加時の守り忘れが再発する。PR #120 で middleware 化した目的と逆行するため採用しない。

## データフロー

`create_router(state)` が `build_routes()` から `RouteDefinitions` を受け取る。

その後、Host middleware、security header layer、CSP header layer、`with_state(state)` を一括適用する。HTTP handler と `ws_handler` は Host 検証済みの前提で動作する。

`ws_handler` は WebSocket 固有の Origin authority 一致だけを検証する。`check_ws_origin()` 内の Host 再検証は middleware bypass への defense-in-depth として残す。

## エラー処理

Host middleware で拒否された場合は、既存通り `403` と JSON body を返す。

```json
{"error":"許可されていないHostヘッダーです"}
```

security header layer は Host 拒否レスポンスにも適用される順序を維持する。

`check_ws_origin()` が Host 系拒否を返した場合は、Host middleware bypass、または今後の直接呼び出し経路の兆候として `error!` で記録する。

通常の WebSocket Origin 拒否は現状の段階化を維持する。

- `MissingOrigin`: `info!`
- `OriginMalformed`, `OriginParseError`, `UnsupportedScheme`, `UntrustedOriginAuthority`, `AuthorityMismatch`: `warn!`
- `MissingHost`, `HostMalformed`, `UntrustedHost`: `error!`

## テスト方針

TDD で進める。

最初に、WebSocket の不正 Host 拒否レスポンスにも `assert_forbidden_with_security_headers` を適用するテストへ更新する。これにより Host middleware が `/ws` にも security header layer と同じ順序で適用されることを固定する。

次に、Host 系 `WsOriginRejection` を bypass indicator として分類する小さな helper を追加する場合は、その unit test を `guards.rs` に追加する。既存の `check_ws_origin()` の拒否理由テストは維持し、Host/Origin の判定仕様を変えていないことを確認する。

最後に以下を実行する。

```bash
cargo test --all-targets --all-features
./verify.sh
```

## 受け入れ条件

- `create_router()` で Host middleware が全 route に適用される構造が、コメントだけでなく型またはテストで固定されている。
- `build_routes()` が route 定義専用である契約を破りにくい形になっている。
- WebSocket 経路で Host middleware bypass 相当が起きた場合、通常の Origin 拒否と区別できるログレベルまたは拒否理由になる。
- 不正 Host は HTTP と WebSocket upgrade の両方で `403` になり、security headers の既存期待を壊さない。
- Host / Origin の信頼判定仕様は変わらない。
- `cargo test --all-targets --all-features` と `./verify.sh` が通る。

## セキュリティ考慮

この設計は DNS Rebinding 対策の構造的な回帰防止を目的とする。Host / Origin の許可条件は緩めない。

Host middleware は route 横断のセキュリティ境界であり、handler ごとの opt-in へ戻さない。新規 route が Host middleware を bypass しないよう、route 定義と security layer 適用の境界を private newtype で明示する。

WebSocket の Host 系拒否は通常運用では Host middleware によって先に拒否されるため、後段で到達した場合は bypass 兆候として扱う。ログには Host / Origin 値が含まれるが、既存の `log_value_for_header` による `"<absent>"` / `"<non-ascii>"` sentinel 化を維持し、欠落と malformed probe を区別する。

外部から届く Header 値は未信頼入力として扱う。ログ分類を追加しても、Header 値を shell、SQL、policy、HTML として実行・解釈しない。

## 影響範囲

- 変更対象: `src/server/routes.rs`, `src/server/guards.rs`, `tests/integration_test.rs`
- 影響する依存: `markdown_view::server::create_router` を使う統合テスト、WebSocket upgrade 経路、Host 拒否時の JSON error response
- UI 影響: なし
- データ永続化影響: なし

## ロールバック

実装コミットを revert すれば元に戻せる。

この変更は routing とログ分類の構造変更に閉じており、データ移行は不要。問題が出た場合は `RouteDefinitions` newtype と Host 系 rejection のログ分類変更を戻し、既存の `create_router()` / `build_routes()` 構造へ戻す。
