# Issue 145: Host/security layer 適用境界 helper 化設計

## 背景

Issue 145 は、Host validation、security headers、CSP layer が `create_router()` 側でまとめて適用されている一方で、その適用順と適用範囲がコメントと private newtype に依存している問題を扱う。

現状の `src/server/routes.rs` には `RouteDefinitions(Router<Arc<AppState>>)` と `build_routes()` があり、route 定義と共通 layer 適用はある程度分離されている。ただし `create_router()` 内に security layer の詳細が直書きされており、将来 route 追加時に layer 適用後へ `.route(...)` を足すと Host middleware を bypass できる構造は残っている。

## ゴール

- Host middleware、`X-Content-Type-Options`、`X-Frame-Options`、CSP の適用を `apply_security_layers(routes, csp_header)` に集約する。
- `create_router()` を「route 定義を作る」「security layer を適用する」「state を注入する」の流れに整理する。
- Host/security smoke test に使える route 一覧 helper を整備し、新規 route 追加時の検証対象更新を容易にする。
- Host/Origin/CSP の既存ポリシー、拒否時レスポンス、監査ログを維持する。

## 非ゴール

- Host、Origin、CSP、security header の許可条件変更。
- axum router の公開 API 変更。
- WebSocket Origin 検証の責務移動。Host は middleware、Origin は `ws_handler` 内の既存検証に残す。
- `SecuredRouter` のような強い型 wrapper 導入。今回の規模では route 一覧 helper と security layer helper で十分とする。
- UI、renderer、file service、watcher の挙動変更。

## アーキテクチャ

`src/server/routes.rs` に private helper `apply_security_layers(routes, csp_header)` を追加する。この helper は `RouteDefinitions` を受け取り、Host middleware、`nosniff`、`DENY`、CSP を既存と同じ順序で適用した `Router<Arc<AppState>>` を返す。

`create_router(state)` は次の責務だけを持つ。

```text
let csp_header = build_csp_header(state.syntax_css());
let routes = build_routes();
apply_security_layers(routes, csp_header).with_state(state)
```

これにより、security layer の順序と理由を helper に閉じ込め、`create_router()` に新規 route を直接足しにくい形へ寄せる。`RouteDefinitions` は引き続き private のままにし、裸の `Router` と route 定義中の router を区別する。

## Route 一覧 helper

Host/security smoke test 用に、検証対象 route を列挙する helper を追加する。helper は production request handling には使わず、テストから route coverage を保つための一覧として扱う。

想定する検証対象は次の通り。

- `GET /`
- `GET /api/content`
- `GET /api/search`
- `GET /api/memo`
- `PUT /api/memo`
- `GET /api/files`
- `GET /ws`

helper の公開範囲は最小にする。integration test から参照しにくい場合は、production module に公開 helper を増やすより、`tests/integration/security.rs` 側で小さな case table を持たせる。ただし route 定義とテスト対象の二重管理が増えすぎる場合は、`pub(crate)` helper として server module 内に閉じる。

## テスト方針

既存の `tests/integration/security.rs` を中心に更新する。現在の個別 Host 拒否テストを、route 一覧に基づく case-driven test へ寄せる。

GET 系 route は単純な request で検証する。`PUT /api/memo` は最小 JSON body を付け、不正 Host が body limit や handler 処理より前に拒否されることを確認する。`GET /ws` は HTTP upgrade request として投げ、不正 Host が Host middleware で `403` になることを確認する。

Host 拒否レスポンスでは、既存の `assert_forbidden_with_security_headers` を使って `X-Content-Type-Options: nosniff`、`X-Frame-Options: DENY`、CSP が残ることを確認する。これにより layer 順序の退行を検知する。

## セキュリティ考慮

`Host` ヘッダーは攻撃者が制御し得る未信頼入力として扱う。今回の変更では `require_allowed_request_host` と `ensure_allowed_request_host` を再利用し、非 ASCII、userinfo、非数値 port、非 loopback host の扱いを変えない。

WebSocket は Host middleware と Origin 検証の二段構えを維持する。Host が許可されても、Origin authority と Host の一致検証を通らなければ接続できない。逆に不正 Host は Origin 検証に到達する前に middleware で拒否される。

拒否レスポンスに security headers が付くことも security boundary の一部として扱う。`apply_security_layers()` の layer 順序を変える場合は、不正 Host 応答でも `nosniff`、`DENY`、CSP が付くことを必ず確認する。

## 受け入れ条件

- `create_router()` から security layer の詳細が `apply_security_layers()` に移っている。
- 新規 route 追加時に Host/security smoke 対象を更新しやすい route 一覧または case table がある。
- 不正 Host は主要 HTTP route と `/ws` で `403` になる。
- Host 拒否レスポンスに security headers が付く。
- Host/Origin/CSP のポリシーを緩めていない。
- ユーザー可視の正常系挙動が変わらない。
- `./verify.sh` が通る。

## 検証計画

実装時は TDD で進める。先に case-driven Host/security smoke test を追加または整理し、現状の不足を確認してから router helper を実装する。

想定コマンド:

```text
cargo test --test integration_test security
cargo test --all-targets --all-features host
./verify.sh
```

docs-only の本設計書作成では、Markdown の placeholder、矛盾、scope、曖昧さを自己レビューし、`git diff --check` を validation として実行する。

## 影響範囲

主な変更対象は次の通り。

- `src/server/routes.rs`: `apply_security_layers()` と route smoke helper の追加、`create_router()` の整理。
- `tests/integration/security.rs`: Host/security smoke test の case-driven 化。

必要に応じて、test helper の visibility 調整に限って周辺 module を触る。

## Rollback path

実装コミットを revert すれば、現在の `create_router()` 内に security layer を直接並べる構成へ戻せる。挙動変更を伴わない構造整理として実装するため、rollback 時の data migration や設定変更は不要。

## 見積もり

- Human effort: 45〜75分
- Codex/AI-assisted effort: 20〜35分

route 一覧 helper の visibility を integration test からどう扱うかで上下する。public API を増やさずに済む場合は短く収まる。

## 残留リスク

- route 定義と smoke test case table を完全に単一ソース化できない場合、新規 route 追加時にテスト対象更新漏れが残る。
- axum の `Router::layer` 適用順は読み間違えやすいため、helper 化後も security header 付き拒否レスポンスの regression test が必要。
- `GET /ws` の HTTP upgrade rejection は client 実装差が出やすいため、既存 helper と同じ reqwest ベースの検証に揃える。
