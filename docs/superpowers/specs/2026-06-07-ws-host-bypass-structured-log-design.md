# WS Host bypass structured log 契約設計書

## 目的

`docs/todo/BACKLOG.md` P3 の「WS Host middleware bypass 兆候のメトリクス化を必要性ベースで検討する」を、次に実行する低優先リスク低減項目として扱う。

結論は、恒久メトリクス基盤やカウンタの導入ではなく、既存の `error!` 構造化ログを観測契約として強化して完了させる方針とする。個人向け localhost ツールでは、実運用の継続集計要求がない状態で metrics crate、状態管理、HTTP endpoint を増やすより、Host middleware 後段で Host 系 `WsOriginRejection` に到達した異常を確実にログとテストで固定する方が KISS / YAGNI に合う。

## 非ゴール

- Host / Origin の許可条件を変更すること。
- Router layer 構造や Host middleware 適用境界を変更すること。
- WebSocket API、HTTP response、CSP、security headers を変更すること。
- metrics crate、外部監視基盤、Prometheus endpoint、内部 counter API を追加すること。
- Host / Origin ログへ query string、本文断片、ファイルパス、環境変数を追加すること。
- 検索 RSS plateau 系 BACKLOG 項目を同時に処理すること。

## 方針

推奨方針は、`src/server/guards.rs` の既存 `WsOriginRejection` ログ分類を契約として明示し、unit test で固定すること。

現行コードは、Host middleware 後段で `MissingHost`、`HostMalformed`、`UntrustedHost` に到達した場合に `WS Host 検証異常` として `error!` を出し、`host_recheck_anomaly=true` を structured field に含めている。この分類は「Host middleware bypass、または Host 検証通過後の malformed / untrusted probe」を示す異常兆候であり、通常運用では到達しない。

今回の実装では、この既存構造をメトリクス代替の観測契約として扱う。新しい runtime state は追加しない。外部の拒否可否、HTTP response、WebSocket payload は変えないが、複合拒否時の監査ログ分類は Host middleware 後段の異常兆候を優先するため Host 先行で固定する。

## 受け入れ基準

Host 系 `WsOriginRejection` について、次が unit test で固定される。

- `MissingHost`、`HostMalformed`、`UntrustedHost` は `ERROR` level で記録される。
- `ws_rejection_class` は `"WS Host 検証異常"` である。
- `host_recheck_anomaly` は `true` である。
- `rejection` field が該当 variant を保持する。
- `host` と `origin` は監査ログ用の正規化値であり、欠落や非 ASCII を sentinel として扱う。Origin は parse 可能な場合も scheme + authority までに限定し、path / query / fragment を出さない。userinfo 付き authority は実値を出さず sentinel 化する。

Origin 系 `WsOriginRejection` について、次が維持される。

- `MissingOrigin` は `INFO`、malformed / parse / scheme / authority / mismatch 系は `WARN` で記録される。
- `ws_rejection_class` は `"WS Origin 拒否"` である。
- `host_recheck_anomaly` は `false` である。
- Host 系異常説明文を Origin 系拒否ログに混ぜない。

`docs/todo/BACKLOG.md` では、該当 P3 項目を Done へ移し、メトリクス基盤を導入しない理由、構造化ログで固定した契約、残余リスクを記録する。

## テスト設計

TDD で進める。最初に `src/server/guards.rs` のログ分類 unit test を契約ベースに更新し、必要なら失敗を確認してから実装または整理を行う。

主なテスト対象は次の既存 helper とする。

- `capture_ws_rejection_events()`
- `ws_rejection_log_level()`
- `ws_rejection_log_message()`
- `is_host_middleware_bypass_indicator()`

外部 API を変えないため、統合テストや E2E は増やさない。Host middleware の外部拒否、WS Origin 拒否応答 message、security headers は既存の `tests/integration/security.rs` が担い、structured log message は `src/server/guards.rs` の unit test が担う。

検証コマンドは次を想定する。

```bash
cargo test server::guards --all-targets --all-features
./verify.sh
```

`./verify.sh` が環境要因で失敗した場合は、失敗箇所と残リスクを completion report に残す。

## セキュリティ

Host と Origin は攻撃者制御の未信頼入力として扱う。今回の変更では DNS Rebinding 対策である Host middleware と WebSocket Origin authority 一致検証を緩めない。

ログ値は監査ログ用の正規化 helper を通す。欠落値は `"<absent>"`、非 ASCII header は `"<non-ascii>"` として扱い、raw bytes をログへ出さない。Origin は parse 可能な場合も scheme + authority までを出し、path / query / fragment は落とす。Origin authority に userinfo が含まれる場合は `"<origin-authority-with-userinfo>"` とし、userinfo 実値を出さない。Host / Origin の値は既存契約の範囲でのみ出し、新たに query string、Markdown 本文、ファイルパス、full process args、環境変数を出力しない。

メトリクス基盤を追加しないため、追加 endpoint、長寿命 counter state、外部 scrape surface、依存 crate による攻撃面は増えない。将来、本格運用や継続監視の要求が出た場合だけ、今回固定した `host_recheck_anomaly=true` ログを入力契約として counter 化を再検討する。

## 影響範囲

主な変更対象:

- `src/server/guards.rs`: WS Host / Origin 拒否ログの structured field 契約をテストで固定する。
- `docs/todo/BACKLOG.md`: P3 項目を完了扱いへ移し、メトリクス見送り理由を記録する。

参照対象:

- `src/server/routes.rs`: WS handler が Host middleware 後段で Origin 検証する構造コメント。
- `tests/integration/security.rs`: Host / Origin 外部契約と security headers の既存統合テスト。
- `docs/superpowers/specs/2026-05-16-host-middleware-observability-followup-design.md`: Host middleware 観測性 follow-up の過去設計。

外部 API behavior、HTTP response、WebSocket payload、CSP、security headers、Host / Origin 許可条件は変更しない。監査ログの拒否理由と分類は、Host と Origin の両方に問題がある場合に Host 系異常を優先する。

## ロールバック

この作業コミットを revert すればよい。ログ契約テストと BACKLOG 更新が主対象であり、Host / Origin の許可条件や route 構造は変更しないため、動作面の巻き戻しは小さい。

一部だけ戻す場合は、`src/server/guards.rs` のテスト整理と `docs/todo/BACKLOG.md` の Done 移動を独立して戻せる。

## 見積もり

- 人間作業: 45-90 分。
- Codex / AI 支援: 20-45 分。

`./verify.sh` の実行時間や既存テストの安定性で変動する。

## 実装前提

実装前に、現在の `src/server/guards.rs` が次の前提を満たすことを再確認する。

- Host 系 `WsOriginRejection` は `MissingHost`、`HostMalformed`、`UntrustedHost` の3種である。
- `is_host_middleware_bypass_indicator()` が Host 系だけを `true` にする。
- `emit_ws_rejection_log()` が `ws_rejection_class` と `host_recheck_anomaly` を structured field として記録する。
- Origin 系拒否は `WS Origin 拒否` に分類される。

前提と現行コードがずれている場合は、設計を優先せず現行コードの安全境界を再読解してから計画を更新する。
