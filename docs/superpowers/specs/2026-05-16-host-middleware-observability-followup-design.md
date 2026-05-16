# Host middleware 観測性 follow-up 完了設計

**作成日**: 2026-05-16
**対象 TODO**: `docs/todo/TODO.md` の「Host middleware 化後の低優先 follow-up を整理して追加検証する」
**対象ファイル**: `src/server/guards.rs`, `tests/integration/security.rs`, `tests/integration/support.rs`, `docs/todo/TODO.md`

## 目的

Host middleware 化後に残っている低優先 follow-up のうち、観測性と回帰検知に直接効く項目を完了させる。

既存実装では Host middleware の主要 route smoke、許可 Host smoke、空 Host 拒否、巨大 body 付き memo PUT の body limit 前拒否が統合テストで固定されている。今回の焦点は、残っている warn ログの文脈不足、WS Origin 拒否メッセージの外部契約、test helper の server 起動失敗文脈、TODO 完了整理である。

## 非目的

- Host の許可条件は変更しない。
- WebSocket Origin の許可条件や authority 一致検証は変更しない。
- Host middleware の layer 順や route 登録構造は変更しない。
- security headers、CSP、body limit の外部契約は変更しない。
- 全テストの `unwrap()` / `expect()` を横断的に整理しない。
- 新しい監視基盤、メトリクス、依存 crate は追加しない。

## 方針

推奨方針は、観測性と完了整理に絞った小さな仕上げである。

`src/server/guards.rs` では Host middleware 拒否ログに request URI path を含める。`require_allowed_request_host` は `Request` を受け取っているため、拒否前に `request.uri().path()` を取得し、拒否ログへ渡す。query string はログへ含めない。これにより `/api/search?q=...` の検索語などを追加で露出せず、どの route で拒否されたかだけを確認できる。

`tests/integration/security.rs` では、許可 Host かつ不正 Origin の `/ws` が `403` と Origin 用エラーメッセージを返すことを固定する。Host 拒否メッセージは既存テストで固定済みなので、今回の追加テストは Host 拒否と Origin 拒否が外部応答上も混ざらないことに集中する。

`tests/integration/support.rs` では、共通 `spawn_test_server` の `axum::serve(...).unwrap()` を文脈付き `expect(...)` に置き換える。対象は TODO に明記された server 起動 helper に限定し、周辺テストの一般的な `unwrap()` 整理には広げない。

最後に `docs/todo/TODO.md` の該当 Medium 項目を Done Summary へ移し、今回の実施内容を完了根拠として残す。

## データフロー

Host 拒否の処理経路は現在のまま維持する。

1. `create_router()` が route 定義を作る。
2. `apply_security_layers()` が route 群全体へ Host middleware と security header layer を適用する。
3. `require_allowed_request_host()` が request headers を検証する。
4. 拒否時は `403` JSON を返し、security headers は既存 layer により維持される。

変更点は 3 の拒否ログだけである。`require_allowed_request_host()` が `request.uri().path()` を読み、Host 拒否ログへ `path` を含める。path はログ用途に限定し、許可判定やレスポンス生成には使わない。

WebSocket Origin 拒否は、Host middleware 通過後の `ws_handler` で引き続き `is_allowed_ws_origin(&headers)` が担う。許可 Host と不正 Origin の組み合わせを統合テストで使い、Host middleware 由来ではなく Origin 検証由来の拒否メッセージが返ることを確認する。

## テスト設計

TDD で進める。最初に失敗する統合テストまたは unit test を追加し、その後実装する。

### WS Origin 拒否メッセージ

`tests/integration/security.rs` に、許可 Host と不正 Origin で `/ws` upgrade request を送るテストを追加する。

期待値:

- status は `403`
- security headers は既存 helper で確認する
- JSON `error` は `WebSocket接続元が許可されていません`

このテストは、Host 拒否の `許可されていないHostヘッダーです` と Origin 拒否の文言が分離され続けることを固定する。

### Host 拒否ログの path

`src/server/guards.rs` の既存 tracing capture test に合わせ、Host middleware 拒否ログに path が含まれることを固定する。期待値は structured field としての `path`、またはログ本文の `path="/"` のどちらかで、既存のログ capture helper で安定して検証できる形にする。

query string は期待値に含めない。`/api/search?q=secret` のような入力を使う場合でも、ログ上は `/api/search` だけを確認する。

### Test server helper

`tests/integration/support.rs` の `spawn_test_server` で `axum::serve(listener, router).await.expect(...)` を使う。テスト追加は不要で、文言が失敗文脈を説明していることをコードレビューで確認する。

## セキュリティ考慮

Host と Origin は攻撃者が制御し得る未信頼入力として扱う。今回の変更では Host 許可条件、Origin authority 一致、loopback 判定、CSP、security headers を緩めない。

ログ追加では query string を出さない。検索語、ファイル指定、その他のユーザー入力が URL query に含まれる可能性があるため、route 特定に必要な path のみを出す。Host 値は既存どおり `log_value_for_header` を経由し、非 ASCII や欠落 Host の扱いを変えない。

WS Origin 拒否テストは DNS Rebinding 対策の二段検証を守るためのものである。Host が許可されても Origin authority が一致しない場合は拒否される、という外部契約を固定する。

## 受け入れ条件

- Host middleware の拒否ログで、拒否された request path が分かる。
- Host 拒否ログに query string を新規出力しない。
- WS の不正 Origin は `403` と `WebSocket接続元が許可されていません` を返す。
- Host 拒否メッセージと Origin 拒否メッセージがテスト上分離される。
- `spawn_test_server` の `axum::serve` 失敗が文脈付き `expect` になる。
- `docs/todo/TODO.md` の該当項目が Done Summary へ移る。
- `cargo test --test integration_test --all-features` が通る。
- 最終確認として `./verify.sh` が通る。

## 影響範囲

- 主な変更対象: `src/server/guards.rs`, `tests/integration/security.rs`, `tests/integration/support.rs`, `docs/todo/TODO.md`
- 参照対象: `src/server/routes.rs`, `docs/superpowers/specs/2026-05-02-host-middleware-guard-design.md`, `docs/superpowers/specs/2026-05-15-host-middleware-boundary-tests-design.md`
- 実行時の挙動差分: Host 拒否ログの情報量が増える
- 外部 API への意図した影響: なし
- セキュリティ境界への意図した影響: なし。既存境界の観測性と回帰検知を強化する。

## ロールバック

この作業コミットを revert すれば元に戻せる。Host/Origin の許可条件、route 構造、security headers は変更しないため、ロールバック時の実行時リスクは小さい。

一部だけ戻す場合は、Host 拒否ログの path 追加、WS Origin 拒否 message assert、test helper の `expect`、`TODO.md` の Done Summary 移動をそれぞれ独立して戻せる。
