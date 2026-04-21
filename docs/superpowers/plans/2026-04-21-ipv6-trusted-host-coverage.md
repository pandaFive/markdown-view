# IPv6 Trusted Host Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `src/server/guards.rs` の IPv6 経路を 3 層 (`is_trusted_host` / `is_trusted_authority` / `normalize_authority` / `is_allowed_ws_origin`) の境界テストで覆い、DNS Rebinding 対策のサイレント失敗を予防する。

**Architecture:** 本作業は **characterization test（safety net 追加）**。プロダクションコードは変更せず、現行の期待動作を固定するテストを追加する。各タスクで「テスト追加 → 実行 → PASS 確認 → コミット」の順で進める。万一 FAIL した場合はプロダクション挙動の潜在バグなので一旦停止して報告する。

**Tech Stack:** Rust (cargo test), axum (`http::Uri`, `http::uri::Authority`), 標準ライブラリ `std::net::IpAddr`

**関連 Spec:** `docs/superpowers/specs/2026-04-21-ipv6-trusted-host-coverage-design.md`

---

## File Structure

- Modify: `src/server/guards.rs`（テストモジュール `mod tests` 内のみ）
  - L171-173 の既存 `test_trusted_host_loopback_ipv6` を拡張
  - 新規テスト関数 4 個を論理グループに沿って挿入

プロダクションコード変更なし。新規ファイルなし。

---

## Task 1: `test_trusted_host_loopback_ipv6` を拡張（`::1` 非 bracketed ケース追加）

**Files:**
- Modify: `src/server/guards.rs:170-173`

- [ ] **Step 1: 既存テストを置換する**

`src/server/guards.rs` L170-173 の既存関数を以下に置換:

```rust
    #[test]
    fn test_trusted_host_loopback_ipv6() {
        // bracketed（HTTP authority の正規形式）
        assert!(is_trusted_host("[::1]"));
        // 非 bracketed（is_trusted_host の防御的実装が自前で bracket を trim するケース）
        assert!(is_trusted_host("::1"));
    }
```

- [ ] **Step 2: テストを実行し PASS することを確認**

Run: `cargo test --all-targets --all-features test_trusted_host_loopback_ipv6 -- --nocapture`
Expected: `test result: ok. 1 passed`

**FAIL した場合:** `is_trusted_host` の bracket trim ロジック（L115-117）が期待通りに機能していない可能性。停止して報告。

- [ ] **Step 3: fmt と clippy を実行**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: エラーなし

- [ ] **Step 4: コミット**

```bash
git add src/server/guards.rs
git commit -F - <<'EOF'
test: is_trusted_host に非 bracketed IPv6 loopback ケースを追加

変更内容:
- test_trusted_host_loopback_ipv6 に assert!(is_trusted_host("::1")) を追加

変更理由:
- 既存テストは [::1] のみを検証しており、bracket 剥離後の純粋な IPv6 文字列パス (::1) が未検証だった
- DNS Rebinding 対策の境界として防御的実装が機能することを固定

影響範囲:
- テストのみ（プロダクションコード変更なし）

テスト結果: 1 passed
EOF
```

---

## Task 2: `test_trusted_host_ipv6_非loopbackを拒否する` を追加

**Files:**
- Modify: `src/server/guards.rs`（`test_trusted_host_loopback_ipv6` の直後に挿入）

- [ ] **Step 1: 新規テスト関数を追加**

Task 1 で更新した `test_trusted_host_loopback_ipv6` 関数の **直後** に以下を挿入:

```rust
    #[test]
    fn test_trusted_host_ipv6_非loopbackを拒否する() {
        // link-local：loopback ではない
        assert!(!is_trusted_host("[fe80::1]"));
        // unspecified（::）：0.0.0.0 相当。loopback と紛らわしいため明示
        assert!(!is_trusted_host("[::]"));
        // public IPv6（RFC 3849 ドキュメント用アドレス）
        assert!(!is_trusted_host("[2001:db8::1]"));
        // IPv4-mapped IPv6：Ipv6Addr::is_loopback は ::1 のみ true を返す仕様
        // （IPv4-mapped を loopback 扱いする将来の書き換えを防ぐ固定テスト）
        assert!(!is_trusted_host("[::ffff:127.0.0.1]"));
    }
```

- [ ] **Step 2: テストを実行し PASS することを確認**

Run: `cargo test --all-targets --all-features test_trusted_host_ipv6_非loopbackを拒否する -- --nocapture`
Expected: `test result: ok. 1 passed`

**FAIL した場合:**
- `[::ffff:127.0.0.1]` が unexpected に true を返した場合、IPv4-mapped を loopback として許可してしまっており DNS Rebinding の潜在リスクがあるため即報告。
- 他ケースが true を返した場合は`is_trusted_host` の実装見直しが必要。

- [ ] **Step 3: fmt と clippy を実行**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: エラーなし

- [ ] **Step 4: コミット**

```bash
git add src/server/guards.rs
git commit -F - <<'EOF'
test: is_trusted_host の IPv6 非 loopback 拒否境界を追加

変更内容:
- test_trusted_host_ipv6_非loopbackを拒否する を新規追加
- link-local / unspecified / public / IPv4-mapped の 4 パターンを検証

変更理由:
- DNS Rebinding 対策として loopback 以外の IPv6 が漏れなく拒否されることを固定
- 特に IPv4-mapped (::ffff:127.0.0.1) は Rust の Ipv6Addr::is_loopback 仕様の境界で、
  将来の「親切な」挙動変更を CI で検知するための safety net

影響範囲:
- テストのみ（プロダクションコード変更なし）

テスト結果: 1 passed
EOF
```

---

## Task 3: `test_trusted_authority_ipv6_port付きを検証する` を追加

**Files:**
- Modify: `src/server/guards.rs`（Task 2 のテストの直後に挿入）

- [ ] **Step 1: 新規テスト関数を追加**

Task 2 の `test_trusted_host_ipv6_非loopbackを拒否する` の **直後** に以下を挿入:

```rust
    #[test]
    fn test_trusted_authority_ipv6_port付きを検証する() {
        // 正常系：port 付き IPv6 loopback authority
        assert!(is_trusted_authority("[::1]:3000"));

        // 非数値 port：Authority parser が拒否する → false
        assert!(!is_trusted_authority("[::1]:abc"));

        // 非 loopback IPv6 + port：is_trusted_host 側で拒否
        assert!(!is_trusted_authority("[fe80::1]:3000"));
    }
```

- [ ] **Step 2: テストを実行し PASS することを確認**

Run: `cargo test --all-targets --all-features test_trusted_authority_ipv6_port付きを検証する -- --nocapture`
Expected: `test result: ok. 1 passed`

**FAIL した場合:** Authority parser の挙動が想定と異なる可能性（axum のバージョン差など）。stop して `authority.parse::<Authority>()` の具体的な戻り値を調べる。

- [ ] **Step 3: fmt と clippy を実行**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: エラーなし

- [ ] **Step 4: コミット**

```bash
git add src/server/guards.rs
git commit -F - <<'EOF'
test: is_trusted_authority の IPv6 port 付き境界を追加

変更内容:
- test_trusted_authority_ipv6_port付きを検証する を新規追加
- [::1]:3000 許可 / [::1]:abc 拒否 / [fe80::1]:3000 拒否 の 3 ケース

変更理由:
- is_trusted_host レベルでは port 付き authority は検証できないため、
  is_trusted_authority レイヤで「正当な IPv6 authority のみ通過」を固定
- 非数値 port を介したバイパス攻撃への防御境界を明示化

影響範囲:
- テストのみ（プロダクションコード変更なし）

テスト結果: 1 passed
EOF
```

---

## Task 4: `test_normalize_authority_ipv6_等価性` を追加

**Files:**
- Modify: `src/server/guards.rs`（既存 `test_normalize_authority_末尾ドットと大文字小文字を正規化する` の直後に挿入）

- [ ] **Step 1: 新規テスト関数を追加**

既存の `test_normalize_authority_末尾ドットと大文字小文字を正規化する`（L249-258 付近）の **直後** に以下を挿入:

```rust
    #[test]
    fn test_normalize_authority_ipv6_等価性() {
        // is_allowed_ws_origin 内部で実行される比較を直接再現：
        // HOST ヘッダー文字列と Origin URI から取得した authority 文字列が
        // 同じ正規化結果になることを保証する
        let host_normalized = normalize_authority("[::1]:3000");
        let origin_uri: Uri = "http://[::1]:3000".parse().expect("有効な URI");
        let origin_authority = origin_uri
            .authority()
            .expect("authority が存在する")
            .as_str();
        assert_eq!(host_normalized, normalize_authority(origin_authority));

        // 非空かつ IPv6 情報と port が含まれていることを確認
        // （axum の Authority::host() が brackets を剥がすため具体的な文字列形式は
        // 内容ベースで検証：brackets 有無を決め打ちしない）
        assert!(!host_normalized.is_empty());
        assert!(host_normalized.contains("::1"));
        assert!(host_normalized.contains("3000"));
    }
```

- [ ] **Step 2: テストを実行し PASS することを確認**

Run: `cargo test --all-targets --all-features test_normalize_authority_ipv6_等価性 -- --nocapture`
Expected: `test result: ok. 1 passed`

**FAIL した場合:** axum の `Authority::host()` や `Uri::authority()` の挙動が想定と異なる可能性。`println!` 等で実際の文字列を確認する。

- [ ] **Step 3: fmt と clippy を実行**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: エラーなし

- [ ] **Step 4: コミット**

```bash
git add src/server/guards.rs
git commit -F - <<'EOF'
test: normalize_authority の IPv6 等価性検証を追加

変更内容:
- test_normalize_authority_ipv6_等価性 を新規追加
- HOST 風文字列と Origin-URI-authority 風文字列の正規化結果一致を検証
- 非空 / ::1 / 3000 を含むことを内容ベースで assert

変更理由:
- is_allowed_ws_origin 内の normalize_authority(host) == normalize_authority(origin)
  比較が IPv6 経路で確実に成立することを固定
- axum 依存の挙動（brackets 剥離）を内容ベースで固定し、
  軽微なバージョン差では壊れず、意味的な破綻時に検知できる粒度にする

影響範囲:
- テストのみ（プロダクションコード変更なし）

テスト結果: 1 passed
EOF
```

---

## Task 5: `test_allowed_ws_origin_ipv6_loopback許可と境界` を追加

**Files:**
- Modify: `src/server/guards.rs`（既存 `test_allowed_ws_origin_trailing_dotとmixed_caseを許可する` の直後、`mod tests` の末尾付近に挿入）

- [ ] **Step 1: 新規テスト関数を追加**

既存の `test_allowed_ws_origin_trailing_dotとmixed_caseを許可する`（L261-266 付近）の **直後** に以下を挿入:

```rust
    #[test]
    fn test_allowed_ws_origin_ipv6_loopback許可と境界() {
        // 成功ケース：HOST と Origin が同一 IPv6 loopback authority
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "[::1]:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://[::1]:3000".parse().unwrap());
        assert!(is_allowed_ws_origin(&headers));

        // 失敗ケース：port 不一致
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "[::1]:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://[::1]:4000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));

        // 失敗ケース：非 loopback IPv6（link-local）は
        // HOST/Origin が一致していても拒否される
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "[fe80::1]:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://[fe80::1]:3000".parse().unwrap());
        assert!(!is_allowed_ws_origin(&headers));
    }
```

- [ ] **Step 2: テストを実行し PASS することを確認**

Run: `cargo test --all-targets --all-features test_allowed_ws_origin_ipv6_loopback許可と境界 -- --nocapture`
Expected: `test result: ok. 1 passed`

**FAIL した場合:**
- 成功ケースが FAIL: `is_allowed_ws_origin` の IPv6 経路で正規化比較が破綻している可能性。これは DNS Rebinding 対策の実質的な欠陥。
- 失敗ケース（非 loopback）が PASS: loopback 判定が IPv6 側で機能していない可能性。即停止して報告。

- [ ] **Step 3: fmt と clippy を実行**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: エラーなし

- [ ] **Step 4: コミット**

```bash
git add src/server/guards.rs
git commit -F - <<'EOF'
test: is_allowed_ws_origin の IPv6 エンドツーエンド境界を追加

変更内容:
- test_allowed_ws_origin_ipv6_loopback許可と境界 を新規追加
- 成功: HOST [::1]:3000 + Origin http://[::1]:3000
- 失敗: port 不一致 ([::1]:3000 vs [::1]:4000)
- 失敗: 非 loopback ([fe80::1]:3000 同士)

変更理由:
- WebSocket 経路の DNS Rebinding 対策を IPv6 でエンドツーエンドに検証
- 3 層 (trusted_host / trusted_authority / normalize_authority) の
  組み合わせ挙動を最上位から固定する safety net として機能

影響範囲:
- テストのみ（プロダクションコード変更なし）

テスト結果: 1 passed
EOF
```

---

## Task 6: 全体検証（verify.sh 完走確認）

**Files:** なし（検証のみ）

- [ ] **Step 1: verify.sh を実行してフルパス確認**

Run: `./verify.sh`
Expected: すべての段階（fmt / clippy / cargo test / typecheck）が Pass し、最後に "All checks passed" 相当のメッセージ

**FAIL した場合:** 失敗した段階に応じて原因を切り分け。`cargo test` が失敗した場合は追加したテストのどれが問題かを `--test-threads=1` で個別確認。

- [ ] **Step 2: guards モジュールに関連する全テストが通ることを確認**

Run: `cargo test --all-targets --all-features server::guards -- --nocapture`
Expected: 既存テスト + 追加 4 件（Task 2-5）+ 拡張 1 件（Task 1）が全 PASS

- [ ] **Step 3: TODO.md の H1 項目にチェックを入れる**

`docs/todo/TODO.md` L7-11 の H1 項目を以下に更新（タスク完了マーク）:

```markdown
- [x] `is_trusted_host` / `normalize_authority` の IPv6 網羅テストを追加
  - ファイル: `src/server/guards.rs`
  - 現状: L171-173 の `test_trusted_host_loopback_ipv6` が `[::1]` のみを検証
  - 追加観点: `[::1]:3000`（port 付き bracketed）、`::1`（非 bracketed）、`[fe80::1]`（非 loopback）、`[::1]:abc`（非数値 port）の 4 パターン
  - 理由: DNS Rebinding 対策の核。正規化エッジケースで想定外に通過するとセキュリティ境界が崩れる
```

- [ ] **Step 4: TODO.md 更新をコミット**

```bash
git add docs/todo/TODO.md
git commit -F - <<'EOF'
docs: TODO.md H1 (IPv6 trusted host テスト) を完了マーク

変更内容:
- docs/todo/TODO.md H1 項目のチェックボックスを [x] に更新

変更理由:
- 5 テスト関数 (拡張 1 + 新規 4) の追加および verify.sh 完走により、
  High Priority H1 の完了条件を満たしたため

影響範囲:
- ドキュメントのみ

テスト結果: N/A（docs only）
EOF
```

---

## Self-Review Result

**Spec coverage:**
- Spec セクション「追加テスト構成」の #1〜#5 → Task 1〜5 で 1:1 対応 ✓
- Spec「重要な設計判断」1〜5 → Task 2（IPv4-mapped）、Task 2（unspecified）、Task 4（brackets 剥離の内容ベース検証）、全タスク（プロダクション変更なし）、命名規約（全タスク日本語名）で網羅 ✓
- Spec「検証」 → Task 6 で `./verify.sh` と `cargo test` を実行 ✓
- Spec「成功基準」 → Task 6 Step 2 で 5 テストすべての PASS を確認 ✓

**Placeholder scan:** TBD/TODO/「後で実装」なし。全ステップに具体的なコード・コマンド・期待結果を記述 ✓

**Type consistency:** 関数名（`is_trusted_host` / `is_trusted_authority` / `normalize_authority` / `is_allowed_ws_origin`）と import（`Uri` / `HeaderMap` / `HOST` / `ORIGIN`）は spec・既存コード・全タスクで一貫 ✓

**FAIL 時の挙動指針：** 全テストタスクに「FAIL した場合」の調査方針を明記。Characterization test として production の想定挙動と合致しない場合を潜在バグ発見の機会として扱う ✓
