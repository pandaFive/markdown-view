# Host 検証 middleware 化 設計

**作成日**: 2026-05-02
**対象 TODO**: `docs/todo/TODO.md` の「Host 検証を router middleware 化して新規 route の守り忘れを防ぐ」
**対象ファイル**: `src/server/routes.rs`, `src/server/guards.rs`, `tests/integration_test.rs`

## 目的

`Host` 検証を handler ごとの手動呼び出しから `create_router()` 配下の共通 middleware に移し、新規 HTTP route 追加時の検証漏れを構造的に防ぐ。

この変更は DNS Rebinding 対策の適用漏れを減らすためのセキュリティ境界整理である。既存の `ensure_allowed_request_host` が持つ許可条件、拒否時の `403` JSON 形状、監査ログ方針は維持する。

WebSocket の不正 Host 拒否は Host middleware 経由になるため、エラーメッセージは従来の汎用的な `WebSocket接続元が許可されていません` ではなく、Host 拒否を示す `許可されていないHostヘッダーです` になる。これは Host 拒否経路と Origin 拒否経路を区別するための互換性上の変更として扱う。HTTP status と JSON shape は維持するが、WS Host 拒否の `error` 文字列は外部観測可能な互換性変更であるため、PR 本文の Compatibility / Breaking Change にも明記する。

## 非目的

- `Host` の許可条件は変更しない。
- WebSocket `Origin` 検証ロジックを全面 middleware 化しない。
- CSP、`X-Content-Type-Options`、`X-Frame-Options` は変更しない。
- localhost-only 前提を変更しない。
- `/api/search` の負荷制御やキャンセル境界は扱わない。
- route 構成や公開 API の追加は行わない。

## 方針

`src/server/guards.rs` に axum middleware 用の Host 検証関数を追加する。middleware は request headers を使って既存の `ensure_allowed_request_host(&headers)` を呼び、許可なら `next.run(request).await` へ進め、拒否なら既存と同じ `ApiError` 応答を返す。

`src/server/routes.rs` では route 登録を内部ヘルパーへ閉じ込め、その route 群へ Host middleware を layer として適用する。これにより `/`, `/api/content`, `/api/memo`, `/api/files`, `/api/search`, `/ws` が同じ Host 境界を通る。新規 route は Host middleware の後ろへ直接追加せず、必ず route 登録ヘルパー側へ追加する。route 登録ヘルパーは route 定義のみを持ち、共通 `.layer(...)` は `create_router()` 側で route 群全体へ適用する。

middleware 化後、HTTP handler から手動 `ensure_allowed_request_host` 呼び出しを削除する。`ws_handler` では Host 検証を middleware に任せ、handler 内には既存の `Origin` 検証だけを残す。WebSocket は Host middleware と Origin 検証の二段構えにする。

既存のレスポンスヘッダー layer は維持する。Host middleware の layer 順は、拒否時の JSON 応答にも必要なセキュリティヘッダーを付与できる順序にする。`Router::layer` は呼び出し時点で存在する route にだけ適用されるため、Host middleware 後に route を追加すると、その route は Host 検証を完全に bypass する。

## テスト方針

TDD で進める。最初に Host middleware 経由で拒否されることを期待する統合テストを追加または更新し、その後実装する。

既存の不正 Host テストは維持する。追加確認では、少なくとも `/`, `/api/files`, `/api/search`, `/api/memo`, `/ws` のように page/API/WebSocket upgrade の複数 route 種別で `Host: evil.example:<port>` が `403` になることを固定する。body 付き PUT `/api/memo` でも、Host middleware が body limit より前に拒否し、拒否レスポンスに security headers が付くことを確認する。

`/ws` については、不正 Host が Host middleware で拒否されることと、許可 Host かつ不正 Origin は既存の Origin 検証で拒否されることを分けて確認する。

テスト名は日本語にする。

## 受け入れ条件

- 全 HTTP route と `/ws` が `create_router()` の Host middleware を通る。
- handler ごとの Host 手動検証が不要になっている。
- 不正 Host は `/`, `/api/content`, `/api/memo`, `/api/files`, `/api/search` で `403` になる。
- `/ws` は不正 Host を Host 固有メッセージで拒否し、許可 Host かつ不正 Origin も引き続き拒否する。
- `Host` と `Origin` の許可条件を緩めていない。
- Host 拒否時の warn 監査ログを維持している。
- `cargo test --all-targets --all-features` が通る。
- 可能なら `./verify.sh` が通る。

## セキュリティ考慮

この作業は DNS Rebinding 対策の適用漏れを減らすための変更であり、セキュリティ境界を緩めてはならない。

`Host` ヘッダーは攻撃者が制御し得る未信頼入力として扱う。middleware でも既存の `ensure_allowed_request_host` を再利用し、非 ASCII、userinfo、非数値 port、非 loopback host の扱いを変えない。

WebSocket では `Origin` も未信頼入力として扱う。Host middleware 化後も `Origin` authority と `Host` の一致検証を維持し、Host だけ通れば接続できる状態にしない。

拒否レスポンスに付くセキュリティヘッダーも確認対象とする。middleware の layer 順で `nosniff`、`DENY`、CSP が意図せず外れないよう検証する。

## 影響範囲

- 変更対象: `src/server/guards.rs`, `src/server/routes.rs`, `tests/integration_test.rs`
- 影響する route: `/`, `/ws`, `/api/content`, `/api/memo`, `/api/files`, `/api/search`
- 実行時の意図した影響: Host 検証の適用位置が handler から router middleware に移る
- 実行時の意図しない影響として注意する点: 拒否レスポンスの header、status、JSON body、WebSocket upgrade 失敗時の挙動

## ロールバック

Host middleware 化コミットを revert すれば、handler ごとの手動 Host 検証方式に戻せる。

セキュリティ境界変更なので、部分 revert ではなくコミット単位の revert を前提にする。実装時は middleware 追加、handler 呼び出し削除、テスト更新を 1 つの焦点にまとめ、ロールバックしやすい差分にする。
