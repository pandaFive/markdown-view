# Host middleware 境界テスト追加設計

**作成日**: 2026-05-15
**対象 TODO**: `docs/todo/TODO.md` の「Host middleware 化後の低優先 follow-up を整理して追加検証する」
**対象ファイル**: `tests/integration/security.rs`

## 目的

Host middleware 化後の境界を統合テストで固定し、新規 route 追加や layer 順変更で DNS Rebinding 対策が外れる回帰を検知しやすくする。

今回の焦点は production code の変更ではなく、既存の `create_router()` 配下にある主要 route が許可 Host では通り、異常 Host では middleware 経路で拒否されることを確認する検証網の追加である。

## 非目的

- Host の許可条件は変更しない。
- WebSocket Origin 検証ロジックは変更しない。
- warn ログへ `request.uri().path()` を追加しない。
- WS Origin 拒否 message assert は追加しない。
- test helper の `unwrap()` / `expect()` 整理は広範には扱わない。
- `docs/todo/TODO.md` の項目完了整理はこの作業では行わない。

## 方針

既存の `tests/integration/security.rs` にある `HOST_SMOKE_CASES` を主要 route の列挙元として維持する。新規 route を追加した場合はこの配列へ追加するという現在の意図をそのまま使い、許可 Host と拒否 Host の両方で同じ route 群を確認する。

追加する検証は次の 2 系統に分ける。

1. 許可 Host で主要 route が Host middleware に拒否されず、既存の security headers も維持されること。
2. 空 Host と不正 Host が HTTP 経由で `403` JSON と security headers を返し、欠落 Host と非 ASCII Host は request Host guard 境界で `403` JSON を返すこと。

HTTP クライアントが実ネットワーク経由で送れない Host 異常値は、無理に reqwest 経由へ載せない。`missing Host` や非 ASCII Host のようにクライアントや HTTP 実装が補正・拒否し得るケースは、request Host guard 境界を直接呼ぶテストへ寄せる。

## テスト設計

### 許可 Host smoke

`HOST_SMOKE_CASES` を使い、`127.0.0.1:<port>` または `localhost:<port>` の許可 Host で主要 route へリクエストする。HTTP route は成功 status または既存仕様上の正常な client error を許容し、Host middleware 由来の `403` ではないことを確認する。security headers は既存の Host 拒否テストと同じ観点で確認する。

`/ws` は許可 Host かつ許可 Origin で接続できることを確認する。WebSocket は HTTP response headers を通常の `reqwest::Response` と同じ形で確認しにくいため、Host middleware に拒否されないことを主眼にする。

### 異常 Host rejection

既存の不正 Host smoke は維持し、`evil.example:<port>` が主要 route で `403` になることを確認する。

追加で Host ヘッダー異常値の境界テストを置く。実ネットワーク経由で送れる値は HTTP 経由で検証し、送れない値は `require_allowed_request_host` または `ensure_allowed_request_host` の境界で検証する。

対象ケースは以下を優先する。

- Host 欠落: request Host guard 境界で `403`
- Host 空文字: HTTP 経由または guard 境界で `403`
- Host 非 ASCII: request Host guard 境界で `403`
- Host 不正 authority: 既存 smoke と同じ HTTP 経由で `403`

テスト名は日本語にする。

## 受け入れ条件

- 許可 Host で `/`, `/api/content`, `/api/memo`, `/api/files`, `/api/search`, `/api/memo PUT`, `/ws` が Host middleware に拒否されない。
- 不正 Host は既存同様 `403` JSON と security headers を返す。
- 欠落、空、malformed Host の少なくとも guard または middleware 境界が `403` を返す。
- Host / Origin の許可条件を緩めていない。
- production code 変更が不要な場合は `tests/integration/security.rs` への変更に閉じる。
- `cargo test --test integration_test --all-features` が通る。
- 最終確認として `./verify.sh` が通る。

## セキュリティ考慮

Host ヘッダーは攻撃者が制御し得る未信頼入力として扱う。今回の追加テストは DNS Rebinding 対策の適用漏れを検知するためのものであり、Host 許可条件や Origin 一致検証を緩めてはならない。

拒否レスポンスでは `nosniff`、`DENY`、CSP が維持されることを確認する。これは middleware の layer 順が変わった場合に、Host 拒否だけは動くが security headers が落ちる回帰を検知するためである。

非 ASCII Host や欠落 Host は通常のブラウザ経路では発生しにくいが、未信頼クライアントやプロキシ経由では probe として現れ得る。テストでは HTTP クライアントの制約とサーバ境界の責務を分け、送信不能な値を無理に外部経路で再現しない。

## 影響範囲

- 主な変更対象: `tests/integration/security.rs`
- 参照対象: `src/server/routes.rs`, `src/server/guards.rs`, `tests/integration/support.rs`
- 実行時コードへの想定影響: なし
- セキュリティ境界への想定影響: なし。既存境界の回帰検知を強化する。
- 間接影響: 新規 route 追加時に Host smoke case へ追加する運用がより明確になる。

## ロールバック

追加したテストコミットを revert すれば元に戻せる。production code の変更を含めない方針のため、ロールバック時の実行時リスクはない。

実装中に production code 変更が必要だと判明した場合は、その理由を実装計画で明記し、テスト追加とは分けて確認する。
