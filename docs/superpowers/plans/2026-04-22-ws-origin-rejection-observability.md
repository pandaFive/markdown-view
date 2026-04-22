# WebSocket Origin 拒否経路の観測性強化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `is_allowed_ws_origin` の 8 silent return と `is_trusted_authority` の 3 silent 拒否を監査ログで観測可能化し、HOST 経路と同等の DNS Rebinding 兆候検知を WebSocket 経路でも実現する。

**Architecture:** ハイブリッド構造。`is_allowed_ws_origin` は enum ベースの `check_ws_origin` + `bool` wrapper に分離しログ出力を wrapper に集約。`is_trusted_authority` は `context: &'static str` 引数追加 + inline warn で呼び出し元コンテキストを記録。`tracing-test` 等は未導入で、enum 戻り値を Source of Truth として検証する。

**Tech Stack:** Rust, axum 0.8, tracing 0.1, `pub(super)` 可視性でクレート内限定変更。

**Spec:** `docs/superpowers/specs/2026-04-22-ws-origin-rejection-observability-design.md`

**Branch:** `feat/ws-origin-observability`（spec は commit 済み）

---

## File Structure

- **Modify only:** `src/server/guards.rs`
  - 追加: `WsOriginRejection` enum（~12 行）、`check_ws_origin` 関数（~28 行）
  - 変更: `is_trusted_authority` シグネチャ + warn 3 箇所（~18 行）、`is_allowed_ws_origin` を wrapper 化（~30 行）、`ensure_allowed_request_host` ログフォーマット（1 文字）
  - テスト追加: T1（variant 網羅、~50 行）、T2（context 引数、~10 行）、既存 8 テストへ enum assert（各 ~3 行）
- **Modify at end:** `docs/todo/TODO.md`（該当 High 項目を `- [x]` にマーク）

外部への影響: `src/server/` 内の他モジュールは `guards::is_trusted_authority` を直接呼んでいないため、signature 変更は guards.rs 内で閉じる。

---

## Task 1: `is_trusted_authority` にコンテキスト引数追加 + inline warn

**Files:**
- Modify: `src/server/guards.rs`

このタスクは既存挙動（bool 戻り値の判定結果）を変えず、ログ出力とシグネチャだけを追加するリファクタ。既存テストは context 引数を渡す形に更新するが、判定結果は不変で全て pass し続ける。

- [ ] **Step 1.1: 既存テスト内の `is_trusted_authority(x)` 呼び出しを `is_trusted_authority(x, "host")` / `"origin_authority"` に更新**

`src/server/guards.rs` のテストモジュール内の直接呼び出しは 2 関数：
- `test_trusted_authority_ipv6_port付きを検証する`（L222-250）: 9 個の `is_trusted_authority(...)` 呼び出しすべてに `"host"` を第 2 引数として追加
- `test_trusted_authority_userinfo_を拒否する`（L271-280）: 4 個の `is_trusted_authority(...)` 呼び出しすべてに `"host"` を第 2 引数として追加

例:

```rust
// Before
assert!(is_trusted_authority("[::1]:3000"));
// After
assert!(is_trusted_authority("[::1]:3000", "host"));
```

9 + 4 = 13 箇所。`replace_all` は使わず該当行を1つずつ編集する（他と誤マッチしないよう確認）。

- [ ] **Step 1.2: 本体側 `is_trusted_authority` シグネチャと warn を更新**

`src/server/guards.rs` の L104-124 を以下で置き換え：

```rust
pub(super) fn is_trusted_authority(authority: &str, context: &'static str) -> bool {
    let Ok(parsed) = authority.parse::<Authority>() else {
        tracing::warn!(
            "[markdown-view] authority の parse に失敗し拒否 (context={}): {:?}",
            context,
            authority
        );
        return false;
    };
    // userinfo 付き authority (user@host 形式) は拒否する。
    // 現実の Host / Origin ヘッダーには userinfo は含まれず、
    // 攻撃者が任意 host 文字列を埋め込むバイパス経路になりうるため。
    // （例: "user@[::1]:3000" は http クレートのパーサを通過するが、
    //   host() が "[::1]" を返すため loopback 認定されてしまう）
    if parsed.as_str().contains('@') {
        tracing::warn!(
            "[markdown-view] authority に userinfo を検出し拒否 (context={}): {:?}",
            context,
            authority
        );
        return false;
    }
    // http クレート (1.x) の Authority パーサは非数値port（例: "[::1]:abc"）も受け入れ、
    // この場合 port() / port_u16() はいずれも None を返す（=無port扱い）。
    // DNS Rebinding境界として信頼するには数値portを必須とするため、
    // 元文字列を直接検査してport接尾辞の有無を判定する。
    if has_port_suffix(parsed.as_str()) && parsed.port_u16().is_none() {
        tracing::warn!(
            "[markdown-view] authority に非数値 port を検出し拒否 (context={}): {:?}",
            context,
            authority
        );
        return false;
    }
    is_trusted_host(parsed.host())
}
```

- [ ] **Step 1.3: 本体側呼び出し元 3 箇所を更新**

同じく `src/server/guards.rs` 内：

1. `is_allowed_request_host` L68:
   ```rust
   // Before
   is_trusted_authority(host)
   // After
   is_trusted_authority(host, "host")
   ```

2. `is_allowed_ws_origin` L82:
   ```rust
   // Before
   if !is_trusted_authority(host) {
   // After
   if !is_trusted_authority(host, "host") {
   ```

3. `is_allowed_ws_origin` L97:
   ```rust
   // Before
   if !is_trusted_authority(origin_authority.as_str()) {
   // After
   if !is_trusted_authority(origin_authority.as_str(), "origin_authority") {
   ```

- [ ] **Step 1.4: cargo check で型整合確認**

Run: `cargo check --all-targets --all-features`
Expected: PASS（シグネチャ不一致によるコンパイルエラーなし）

- [ ] **Step 1.5: 既存テスト全 pass を確認**

Run: `cargo test --all-targets --all-features -- --quiet`
Expected: PASS。動作は不変で、追加した warn ログは test harness 上で無害に流れるだけ。

- [ ] **Step 1.6: コミット**

コミットメッセージを `/tmp/commit-msg-task1.txt` に書いてから `git commit -F`:

```text
refactor: is_trusted_authority に context 引数を追加し拒否経路を warn 化

変更内容:
- is_trusted_authority に context: &'static str 引数を追加
- parse 失敗 / userinfo / 非数値 port の 3 拒否分岐に tracing::warn! を追加
- 呼び出し元 3 箇所 (is_allowed_request_host, is_allowed_ws_origin x2) を更新
- 既存テスト 13 箇所の呼び出しに "host" を第2引数として付与

変更理由:
- HOST / Origin authority のどちらから呼ばれたかログで区別可能にし、
  DNS Rebinding 攻撃兆候の追跡性を向上させる

影響範囲:
- src/server/guards.rs のみ。pub(super) 可視性のためクレート外影響なし
- 既存テストの判定結果は不変

テスト結果: Pass (cargo test 全件)
```

Run:

```bash
git add src/server/guards.rs
git commit -F /tmp/commit-msg-task1.txt
rm /tmp/commit-msg-task1.txt
```

---

## Task 2: `WsOriginRejection` enum と `check_ws_origin` を TDD で新設

**Files:**
- Modify: `src/server/guards.rs`（enum + 関数 + T1 テスト追加）

- [ ] **Step 2.1: T1 網羅テストを先に書く**

`src/server/guards.rs` のテストモジュール末尾（`test_allowed_ws_origin_ipv6_loopback許可と境界` の後）に追加：

```rust
#[test]
fn test_check_ws_origin_variants_網羅() {
    // MissingOrigin: Origin ヘッダー不在
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::MissingOrigin)
    );

    // MissingHost: HOST ヘッダー不在
    let mut headers = HeaderMap::new();
    headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::MissingHost)
    );

    // UntrustedHost: HOST が trusted でない
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "evil.example:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::UntrustedHost)
    );

    // OriginParseError: Origin が URI として parse 不可
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "not a uri".parse().unwrap());
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::OriginParseError)
    );

    // UnsupportedScheme: http/https 以外
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "ftp://localhost:3000".parse().unwrap());
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::UnsupportedScheme)
    );

    // UntrustedOriginAuthority: HOST は trusted、Origin authority が trusted でない
    // (AuthorityMismatch ではなく UntrustedOriginAuthority に到達する：
    //  is_trusted_authority("evil.example:3000") が false を返すため、
    //  AuthorityMismatch チェックより前に早期 return される)
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://evil.example:3000".parse().unwrap());
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::UntrustedOriginAuthority)
    );

    // AuthorityMismatch: 両 authority が trusted だが正規化結果が異なる
    // (localhost と 127.0.0.1 はどちらも is_trusted_host で true だが、
    //  normalize_authority の出力文字列が異なるため AuthorityMismatch 発火)
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://127.0.0.1:3000".parse().unwrap());
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::AuthorityMismatch)
    );

    // OriginMissingAuthority: scheme のみで authority が欠落した Origin
    // ("http:" は Uri::parse で成功し scheme_str()==Some("http") & authority()==None を返す)
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "http:".parse().unwrap());
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::OriginMissingAuthority)
    );
}
```

**`OriginMissingAuthority` テスト入力の検証手順**: Step 2.4 で T1 を pass させる際に、`"http:"` が `OriginMissingAuthority` に到達しない場合（`Uri::parse` 挙動が想定と異なる場合）は以下の代替候補を順に試す：
1. `"http:/"`（scheme + 単一スラッシュ）
2. `"http:?query"`（scheme + クエリのみ）
3. `"http:path-only"`（scheme + 相対パス）

いずれも `Uri::parse` で成功し `scheme_str() == Some("http")` かつ `authority() == None` を返せばよい。3 候補すべてが到達失敗した場合に限り、variant 自体が unreachable と判断し、`WsOriginRejection::OriginMissingAuthority` を enum から削除し、`check_ws_origin` の該当分岐も削除する（YAGNI）。その場合は Step 2.1 の該当アサート 4 行も削除してコミット。

- [ ] **Step 2.2: コンパイルエラーを確認**

Run: `cargo test --all-targets --all-features 2>&1 | head -30`
Expected: FAIL with "cannot find type `WsOriginRejection`" および "cannot find function `check_ws_origin`"

- [ ] **Step 2.3: enum と check_ws_origin を実装**

`src/server/guards.rs` で、`is_allowed_ws_origin` 関数の**直前**に追加：

```rust
/// WebSocket Origin 検証の拒否理由
///
/// `is_allowed_ws_origin` の silent return を観測可能にするため、
/// 各拒否分岐を variant として表現する。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum WsOriginRejection {
    MissingOrigin,
    MissingHost,
    UntrustedHost,
    OriginParseError,
    UnsupportedScheme,
    OriginMissingAuthority,
    UntrustedOriginAuthority,
    AuthorityMismatch,
}

/// WebSocket Origin 検証を行い、許可時は `Ok(())`、拒否時は理由を返す
pub(super) fn check_ws_origin(headers: &HeaderMap) -> Result<(), WsOriginRejection> {
    let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) else {
        return Err(WsOriginRejection::MissingOrigin);
    };
    let Some(host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
        return Err(WsOriginRejection::MissingHost);
    };
    if !is_trusted_authority(host, "host") {
        return Err(WsOriginRejection::UntrustedHost);
    }
    let Ok(origin_uri) = origin.parse::<Uri>() else {
        return Err(WsOriginRejection::OriginParseError);
    };
    match origin_uri.scheme_str() {
        Some("http") | Some("https") => {}
        _ => return Err(WsOriginRejection::UnsupportedScheme),
    }
    let Some(origin_authority) = origin_uri.authority() else {
        return Err(WsOriginRejection::OriginMissingAuthority);
    };
    if !is_trusted_authority(origin_authority.as_str(), "origin_authority") {
        return Err(WsOriginRejection::UntrustedOriginAuthority);
    }
    if normalize_authority(origin_authority.as_str()) != normalize_authority(host) {
        return Err(WsOriginRejection::AuthorityMismatch);
    }
    Ok(())
}
```

- [ ] **Step 2.4: T1 テストを実行して pass を確認**

Run: `cargo test --all-targets --all-features test_check_ws_origin_variants_網羅 -- --nocapture`
Expected: PASS（全 assert_eq!）

もしコンパイルエラーがあれば修正し、再度実行。

- [ ] **Step 2.5: 既存テスト含め全体で回帰がないことを確認**

Run: `cargo test --all-targets --all-features -- --quiet`
Expected: 全 Pass

- [ ] **Step 2.6: コミット**

`/tmp/commit-msg-task2.txt`:

```text
feat: WsOriginRejection enum と check_ws_origin を追加

変更内容:
- WsOriginRejection enum を追加（8 variants）
- check_ws_origin 関数を新設：Origin 検証の拒否理由を構造化
- T1 test_check_ws_origin_variants_網羅 テストを追加（8 variant を検証）

変更理由:
- is_allowed_ws_origin の silent return を構造化し、後続タスクで
  呼び出し側からログ出力できる基盤を整える

影響範囲:
- src/server/guards.rs のみ。is_allowed_ws_origin の挙動は未変更
  (Task 3 で wrapper 化する)

テスト結果: Pass (cargo test 全件)
```

Run:

```bash
git add src/server/guards.rs
git commit -F /tmp/commit-msg-task2.txt
rm /tmp/commit-msg-task2.txt
```

---

## Task 3: `is_allowed_ws_origin` を `check_ws_origin` の wrapper に書き換え（ログ出力追加）

**Files:**
- Modify: `src/server/guards.rs`

- [ ] **Step 3.1: `is_allowed_ws_origin` を書き換え**

既存の `is_allowed_ws_origin` 関数（L75-102）を以下で置き換える：

```rust
/// WebSocket接続時のOriginヘッダーを検証する
///
/// DNS Rebinding対策として、Host検証に加えてOriginのauthority一致も要求する。
/// Originスキームは`http`/`https`のみ許可する。
/// 拒否時は `check_ws_origin` の返す `WsOriginRejection` を使って
/// info / warn の監査ログを出力する。
pub(super) fn is_allowed_ws_origin(headers: &HeaderMap) -> bool {
    match check_ws_origin(headers) {
        Ok(()) => true,
        Err(rejection) => {
            let host = headers
                .get(HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<missing>");
            let origin = headers
                .get(ORIGIN)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<missing>");
            match rejection {
                WsOriginRejection::MissingOrigin | WsOriginRejection::MissingHost => {
                    tracing::info!(
                        "[markdown-view] WS Origin 拒否 ({:?}): host={:?} origin={:?}",
                        rejection,
                        host,
                        origin
                    );
                }
                _ => {
                    tracing::warn!(
                        "[markdown-view] WS Origin 拒否 ({:?}): host={:?} origin={:?}",
                        rejection,
                        host,
                        origin
                    );
                }
            }
            false
        }
    }
}
```

- [ ] **Step 3.2: cargo test で既存 8 テストが通ることを確認**

Run: `cargo test --all-targets --all-features test_allowed_ws_origin -- --nocapture`
Expected: 既存 8 テスト（`test_allowed_ws_origin_valid`, `_rejects_different_port`, `_rejects_ftp_scheme`, `_rejects_different_host`, `_missing_origin`, `_missing_host`, `_trailing_dot...`, `_ipv6_loopback...`）すべて Pass。

- [ ] **Step 3.3: 全テスト回帰確認**

Run: `cargo test --all-targets --all-features -- --quiet`
Expected: 全 Pass

- [ ] **Step 3.4: コミット**

`/tmp/commit-msg-task3.txt`:

```text
feat: is_allowed_ws_origin を check_ws_origin wrapper 化し監査ログを追加

変更内容:
- is_allowed_ws_origin を check_ws_origin(...).is_ok() ベースの wrapper に書き換え
- 拒否時に WsOriginRejection variant に応じて tracing::info! / tracing::warn! を出力
- raw HOST/Origin 値は Debug フォーマット ({:?}) で出力しログインジェクション耐性を確保

変更理由:
- HOST 経路 (ensure_allowed_request_host) と同等の DNS Rebinding 兆候検知を
  WebSocket 経路でも実現する
- MissingOrigin/MissingHost は curl 等で通常起きるため info、
  他は攻撃兆候が強いため warn で段階化する

影響範囲:
- src/server/guards.rs のみ。bool 戻り値としての挙動は変わらず、
  既存 8 テストは引き続き pass

テスト結果: Pass (cargo test 全件)
```

Run:

```bash
git add src/server/guards.rs
git commit -F /tmp/commit-msg-task3.txt
rm /tmp/commit-msg-task3.txt
```

---

## Task 4: 既存 `test_allowed_ws_origin_*` 8 テストに enum assert を追加

**Files:**
- Modify: `src/server/guards.rs`

各テストで `assert!(!is_allowed_ws_origin(...))` の直後に `assert_eq!(check_ws_origin(...), Err(...))` を追加し、どの variant に到達したかを固定化する。

- [ ] **Step 4.1: `test_allowed_ws_origin_valid` に enum assert 追加（成功ケース）**

```rust
#[test]
fn test_allowed_ws_origin_valid() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
    assert!(is_allowed_ws_origin(&headers));
    assert_eq!(check_ws_origin(&headers), Ok(()));
}
```

- [ ] **Step 4.2: `test_allowed_ws_origin_rejects_different_port` に enum assert 追加**

```rust
#[test]
fn test_allowed_ws_origin_rejects_different_port() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://localhost:4000".parse().unwrap());
    assert!(!is_allowed_ws_origin(&headers));
    // 両 authority が trusted かつ normalize 結果が異なるため AuthorityMismatch
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::AuthorityMismatch)
    );
}
```

- [ ] **Step 4.3: `test_allowed_ws_origin_rejects_ftp_scheme` に enum assert 追加**

```rust
#[test]
fn test_allowed_ws_origin_rejects_ftp_scheme() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "ftp://localhost:3000".parse().unwrap());
    assert!(!is_allowed_ws_origin(&headers));
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::UnsupportedScheme)
    );
}
```

- [ ] **Step 4.4: `test_allowed_ws_origin_rejects_different_host` に enum assert 追加**

```rust
#[test]
fn test_allowed_ws_origin_rejects_different_host() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://evil.example:3000".parse().unwrap());
    assert!(!is_allowed_ws_origin(&headers));
    // HOST は trusted だが Origin authority が trusted でないため
    // AuthorityMismatch ではなく UntrustedOriginAuthority に到達する
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::UntrustedOriginAuthority)
    );
}
```

- [ ] **Step 4.5: `test_allowed_ws_origin_missing_origin` に enum assert 追加**

```rust
#[test]
fn test_allowed_ws_origin_missing_origin() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    assert!(!is_allowed_ws_origin(&headers));
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::MissingOrigin)
    );
}
```

- [ ] **Step 4.6: `test_allowed_ws_origin_missing_host` に enum assert 追加**

```rust
#[test]
fn test_allowed_ws_origin_missing_host() {
    let mut headers = HeaderMap::new();
    headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
    assert!(!is_allowed_ws_origin(&headers));
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::MissingHost)
    );
}
```

- [ ] **Step 4.7: `test_allowed_ws_origin_trailing_dotとmixed_caseを許可する` に enum assert 追加**

```rust
#[test]
fn test_allowed_ws_origin_trailing_dotとmixed_caseを許可する() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "LOCALHOST.:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
    assert!(is_allowed_ws_origin(&headers));
    assert_eq!(check_ws_origin(&headers), Ok(()));
}
```

- [ ] **Step 4.8: `test_allowed_ws_origin_ipv6_loopback許可と境界` に enum assert 追加**

成功ケースと 2 つの失敗ケースすべてに追加：

```rust
#[test]
fn test_allowed_ws_origin_ipv6_loopback許可と境界() {
    // 成功ケース：HOST と Origin が同一 IPv6 loopback authority
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "[::1]:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://[::1]:3000".parse().unwrap());
    assert!(is_allowed_ws_origin(&headers));
    assert_eq!(check_ws_origin(&headers), Ok(()));

    // 失敗ケース：port 不一致
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "[::1]:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://[::1]:4000".parse().unwrap());
    assert!(!is_allowed_ws_origin(&headers));
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::AuthorityMismatch)
    );

    // 失敗ケース：非 loopback IPv6（link-local）は
    // HOST/Origin が一致していても拒否される
    // (HOST が trusted でない時点で UntrustedHost に到達)
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "[fe80::1]:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://[fe80::1]:3000".parse().unwrap());
    assert!(!is_allowed_ws_origin(&headers));
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::UntrustedHost)
    );
}
```

- [ ] **Step 4.9: 8 テスト全 pass 確認**

Run: `cargo test --all-targets --all-features test_allowed_ws_origin -- --quiet`
Expected: 8 テスト全 Pass

- [ ] **Step 4.10: コミット**

`/tmp/commit-msg-task4.txt`:

```text
test: 既存 test_allowed_ws_origin_* 8件に WsOriginRejection variant 検証を追加

変更内容:
- 既存 8 テストに assert_eq!(check_ws_origin(...), Ok(())/Err(variant)) を追加
- コメントで variant 到達順の注意点を明記
  (特に UntrustedOriginAuthority vs AuthorityMismatch の差異)

変更理由:
- bool 戻り値だけでは「どの拒否経路で落ちたか」が検証されていなかった
- variant を固定化することで、将来ロジック順を入れ替えた際の
  意図しない経路変化を回帰テストで検出可能にする

影響範囲:
- src/server/guards.rs のテストのみ。本体ロジックは未変更

テスト結果: Pass (cargo test 全件)
```

Run:

```bash
git add src/server/guards.rs
git commit -F /tmp/commit-msg-task4.txt
rm /tmp/commit-msg-task4.txt
```

---

## Task 5: T2 context 引数テスト追加 + HOST 経路 `{:?}` 統一

**Files:**
- Modify: `src/server/guards.rs`

- [ ] **Step 5.1: T2 `test_is_trusted_authority_context_引数を受け取る` を追加**

テストモジュール末尾に追加：

```rust
#[test]
fn test_is_trusted_authority_context_引数を受け取る() {
    // userinfo 経由バイパスは "host" コンテキストで拒否される
    assert!(!is_trusted_authority("user@localhost:3000", "host"));

    // 非数値 port は "origin_authority" コンテキストで拒否される
    // (warn ログには context=origin_authority が記録される)
    assert!(!is_trusted_authority("[::1]:abc", "origin_authority"));

    // 正常系: context 値に関わらず判定結果は不変
    assert!(is_trusted_authority("[::1]:3000", "host"));
    assert!(is_trusted_authority("localhost:3000", "origin_authority"));
}
```

- [ ] **Step 5.2: `ensure_allowed_request_host` のログを `{:?}` に統一**

`src/server/guards.rs` の L53-56:

```rust
// Before
tracing::warn!(
    "[markdown-view] 許可されていないHostヘッダーを拒否: {}",
    host
);
// After
tracing::warn!(
    "[markdown-view] 許可されていないHostヘッダーを拒否: {:?}",
    host
);
```

- [ ] **Step 5.3: `normalize_authority` 内の warn も `{:?}` に統一されているか確認**

`src/server/guards.rs` L171-174:

既存は既に `{:?}` を使用しているので変更不要。念のため目視確認する：

```rust
tracing::warn!(
    "[markdown-view] authority解析に失敗（簡易正規化にフォールバック）: {:?}",
    authority
);
```

もし `{}` だった場合は `{:?}` に変更（現状は `{:?}` のはず）。

- [ ] **Step 5.4: 既存 `test_allowed_request_host_invalid` テストが引き続き pass することを確認**

Run: `cargo test --all-targets --all-features test_allowed_request_host -- --quiet`
Expected: Pass（ログ出力フォーマット変更のみで、bool 判定は不変）

- [ ] **Step 5.5: 全テスト回帰確認**

Run: `cargo test --all-targets --all-features -- --quiet`
Expected: 全 Pass

- [ ] **Step 5.6: コミット**

`/tmp/commit-msg-task5.txt`:

```text
test: is_trusted_authority context 引数テスト追加 + HOST 経路ログを Debug 統一

変更内容:
- T2 test_is_trusted_authority_context_引数を受け取る テストを追加
  (userinfo 拒否, 非数値 port 拒否, 正常系を各 context で検証)
- ensure_allowed_request_host の warn ログフォーマットを {} から {:?} に変更
  (Origin 経路の {:?} と揃える)

変更理由:
- context 引数による判定結果の不変性を明示的に検証する
- 制御文字 (\r\n 等) を含む raw Host 値によるログインジェクション耐性を
  Origin 経路と一致させる

影響範囲:
- src/server/guards.rs のみ
- ログ出力文字列の見た目が変わるが、内容は同じ

テスト結果: Pass (cargo test 全件)
```

Run:

```bash
git add src/server/guards.rs
git commit -F /tmp/commit-msg-task5.txt
rm /tmp/commit-msg-task5.txt
```

---

## Task 6: 全検証と TODO.md 更新

**Files:**
- Modify: `docs/todo/TODO.md`

- [ ] **Step 6.1: `./verify.sh` で全検証**

Run: `./verify.sh`
Expected: PASS（cargo fmt --check, cargo clippy -D warnings, cargo test, npm run typecheck すべて成功）

もし失敗した場合は該当 Task に戻って修正。多くは fmt や clippy の軽微な指摘。

- [ ] **Step 6.2: 手動確認: info ログが出ることを確認**

ターミナル A:

```bash
RUST_LOG=markdown_view=info cargo run --quiet -- README.md --port 17422
```

ターミナル B（サーバー起動を待って）:

```bash
# MissingOrigin を誘発：HOST ヘッダーのみで WebSocket upgrade を試みる
curl -i -N \
  -H "Host: localhost:17422" \
  -H "Connection: Upgrade" \
  -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Version: 13" \
  -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
  http://localhost:17422/ws 2>&1 | head -5
```

ターミナル A で以下のログが出ることを確認：

```text
INFO ... [markdown-view] WS Origin 拒否 (MissingOrigin): host="localhost:17422" origin="<missing>"
```

ターミナル A を Ctrl+C で停止。

- [ ] **Step 6.3: 手動確認: warn ログが出ることを確認**

ターミナル A:

```bash
RUST_LOG=markdown_view=info cargo run --quiet -- README.md --port 17422
```

ターミナル B:

```bash
# UntrustedOriginAuthority を誘発：Origin を外部に設定
curl -i -N \
  -H "Host: localhost:17422" \
  -H "Origin: http://evil.example:17422" \
  -H "Connection: Upgrade" \
  -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Version: 13" \
  -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
  http://localhost:17422/ws 2>&1 | head -5
```

ターミナル A で以下のログが出ることを確認：

```text
WARN ... [markdown-view] WS Origin 拒否 (UntrustedOriginAuthority): host="localhost:17422" origin="http://evil.example:17422"
```

- [ ] **Step 6.4: TODO.md の該当 High 項目を完了マーク**

`docs/todo/TODO.md` L31-35 を以下のように変更：

```markdown
- [x] `is_allowed_ws_origin` の拒否経路に warn ログを追加（HOST 経路との観測性を揃える）
  - ファイル: `src/server/guards.rs` L75-102
  - 現状: `is_allowed_ws_origin` は 6 箇所以上で silent な `false` return（Origin なし / HOST なし / 非 http(s) / authority 不一致 / 非数値 port / userinfo 付き等）。対して `ensure_allowed_request_host` は拒否時に raw HOST 値を warn ログする監査経路を持つ
  - 対応: 各拒否分岐に `tracing::warn!` を追加し、どの理由で弾かれたかと原始 HOST/Origin を記録。`is_trusted_authority` の non-numeric port / userinfo 拒否も同様に観測可能にする
  - 理由: 攻撃者が WebSocket 経路で DNS Rebinding を試行した際、HOST 経路では検知できるが Origin 経路では完全に silent で「ブラウザが Origin を送っていない」と区別できない。pr-review-toolkit の silent-failure-hunter が指摘
```

変更箇所: L31 の `- [ ]` → `- [x]`

- [ ] **Step 6.5: コミット**

`/tmp/commit-msg-task6.txt`:

```text
docs: TODO.md の WS Origin 拒否経路観測性タスクを完了マーク

変更内容:
- docs/todo/TODO.md の該当 High 項目を [x] に更新

変更理由:
- 本 PR (feat/ws-origin-observability) で実装完了

影響範囲:
- docs のみ

テスト結果: 該当なし
```

Run:

```bash
git add docs/todo/TODO.md
git commit -F /tmp/commit-msg-task6.txt
rm /tmp/commit-msg-task6.txt
```

- [ ] **Step 6.6: コミット履歴確認**

Run: `git log --oneline main..HEAD`
Expected: 以下 8 コミットが順に並ぶ
1. `docs: WebSocket Origin 拒否経路の観測性強化 spec 追加`
2. `docs: WebSocket Origin 拒否経路の観測性強化 implementation plan 追加`
3. `refactor: is_trusted_authority に context 引数を追加し拒否経路を warn 化`
4. `feat: WsOriginRejection enum と check_ws_origin を追加`
5. `feat: is_allowed_ws_origin を check_ws_origin wrapper 化し監査ログを追加`
6. `test: 既存 test_allowed_ws_origin_* 8件に WsOriginRejection variant 検証を追加`
7. `test: is_trusted_authority context 引数テスト追加 + HOST 経路ログを Debug 統一`
8. `docs: TODO.md の WS Origin 拒否経路観測性タスクを完了マーク`

---

## 完了後のステップ（plan 外）

PR 作成は `/commit-push-pr` または `/pr-merge` で行う。develop に squash merge する。
