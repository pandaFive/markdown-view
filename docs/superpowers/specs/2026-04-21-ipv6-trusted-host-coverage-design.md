# IPv6 網羅テストによる DNS Rebinding 境界の拡充

**日付:** 2026-04-21
**関連 TODO:** `docs/todo/TODO.md` High Priority H1
**対象ファイル:** `src/server/guards.rs`

## 目的

`is_trusted_host` / `is_trusted_authority` / `normalize_authority` / `is_allowed_ws_origin` の IPv6 経路を境界テストで覆い、DNS Rebinding 対策のサイレント失敗を予防する。

現状の `test_trusted_host_loopback_ipv6` は `[::1]` 1 ケースしか検証しておらず、IPv6 正規化のエッジケースで想定外に通過した場合に気づけない。

## 背景

- `src/server/guards.rs` の `is_trusted_host` は自前で brackets を trim する防御的実装を持つが、その分岐は `[::1]` しかテストされていない。
- axum (`http` クレート) の `Authority::host()` は IPv6 ホストから brackets を剥がして返す仕様のため、`normalize_authority` の出力は `::1:3000` 形式になる。この挙動の固定化テストが欠落。
- Rust 標準の `Ipv6Addr::is_loopback()` は `::1` のみ true を返し、IPv4-mapped な `::ffff:127.0.0.1` は false。この挙動が将来「親切心」で緩和されるとセキュリティ境界が崩れる。

## 設計方針

### レイヤ戦略：3 層すべてを網羅

```
[HOST header: "[::1]:3000"]
    │
    ├─ layer 1  is_trusted_host("::1")            ← テスト #1, #2
    ├─ layer 2  is_trusted_authority("[::1]:3000") ← テスト #3
    ├─ layer 3  normalize_authority(...)           ← テスト #4
    └─ layer 4  is_allowed_ws_origin(...)          ← テスト #5
```

深層防御として、どのレイヤで正規化が破綻しても検出できる構造を作る。

### 追加テスト構成

| # | テスト関数名 | 対象レイヤ | 検証内容 |
|---|---|---|---|
| 1 | `test_trusted_host_loopback_ipv6`（**既存拡張**） | `is_trusted_host` | `[::1]` と `::1`（bracket 有無）両方を loopback として許可 |
| 2 | `test_trusted_host_ipv6_非loopbackを拒否する`（新規） | `is_trusted_host` | `[fe80::1]` / `[::]` / `[2001:db8::1]` / `[::ffff:127.0.0.1]` を拒否 |
| 3 | `test_trusted_authority_ipv6_port付きを検証する`（新規） | `is_trusted_authority` | `[::1]:3000` 許可、`[::1]:abc` 拒否、`[fe80::1]:3000` 拒否 |
| 4 | `test_normalize_authority_ipv6_等価性`（新規） | `normalize_authority` | `[::1]:3000` の冪等性、HOST 風と Origin-authority 風が一致 |
| 5 | `test_allowed_ws_origin_ipv6_loopback許可と境界`（新規） | `is_allowed_ws_origin` | HOST `[::1]:3000` + Origin `http://[::1]:3000` 許可、port 不一致と `[fe80::1]` を拒否 |

**合計:** テスト関数 5 個（うち 1 個は既存拡張）、アサーション合計 約 11 件。

### テストケース詳細

#### #1 `test_trusted_host_loopback_ipv6`（拡張）
- `assert!(is_trusted_host("[::1]"));` ← 既存維持
- `assert!(is_trusted_host("::1"));` ← 追加（非 bracketed でも許可）

#### #2 `test_trusted_host_ipv6_非loopbackを拒否する`（新規）
- `assert!(!is_trusted_host("[fe80::1]"));` ← link-local
- `assert!(!is_trusted_host("[::]"));` ← unspecified
- `assert!(!is_trusted_host("[2001:db8::1]"));` ← public
- `assert!(!is_trusted_host("[::ffff:127.0.0.1]"));` ← IPv4-mapped（現状動作固定）

#### #3 `test_trusted_authority_ipv6_port付きを検証する`（新規）
- `assert!(is_trusted_authority("[::1]:3000"));`
- `assert!(!is_trusted_authority("[::1]:abc"));` ← 非数値 port → Authority parse 失敗
- `assert!(!is_trusted_authority("[fe80::1]:3000"));`

#### #4 `test_normalize_authority_ipv6_等価性`（新規）
- **HOST 風 vs Origin-authority 風の一致検証**（`is_allowed_ws_origin` 内部で実行される比較を直接再現）:
  ```rust
  let host_normalized = normalize_authority("[::1]:3000");
  let origin_uri: Uri = "http://[::1]:3000".parse().unwrap();
  let origin_authority = origin_uri.authority().unwrap().as_str();
  assert_eq!(host_normalized, normalize_authority(origin_authority));
  ```
- axum の `Authority::host()` が IPv6 から brackets を剥がす挙動を前提として、`normalize_authority` の出力が非空・かつ `::1` と `3000` を含むことを部分アサート（具体的な文字列リテラルは axum 挙動依存のため、内容ベースで検証）

#### #5 `test_allowed_ws_origin_ipv6_loopback許可と境界`（新規）
- 成功ケース: HOST `[::1]:3000` + Origin `http://[::1]:3000`
- 失敗ケース: HOST `[::1]:3000` + Origin `http://[::1]:4000`（port 不一致）
- 失敗ケース: HOST `[fe80::1]:3000` + Origin `http://[fe80::1]:3000`（両者 loopback でない）

## 重要な設計判断

1. **IPv4-mapped IPv6 は「拒否」を明示**
   現仕様（Rust 標準の `Ipv6Addr::is_loopback()` 準拠）を固定するテスト。将来この挙動が変更されたら CI で検知する。

2. **`[::]`（unspecified）拒否**
   `0.0.0.0` 相当。`[::1]` と視覚的に紛らわしいため明示的にカバー。

3. **`normalize_authority` の brackets 剥離挙動を固定**
   axum 側の挙動変更があった場合に等価比較が破綻しないよう、現在の出力形式を assertion で縛る。

4. **プロダクションコードは変更しない**
   テスト拡充のみ。現仕様が正しく実装されていることを検証する純粋な safety net。

5. **命名規約**
   新規テストは CLAUDE.md の「テスト名は日本語で記述する」に従う。既存の英語名テスト（#1）はアサーション追加のみで名前を維持（最小差分原則）。

## 影響範囲

- 変更ファイル: `src/server/guards.rs`（テストモジュール内のみ）
- プロダクションコード変更: **なし**
- 外部公開 API 変更: なし
- 依存関係追加: なし

## 検証

- `cargo test --all-targets --all-features` ですべての既存テスト + 追加 5 関数が Pass
- `./verify.sh` がフル Pass（fmt / clippy / cargo test / E2E typecheck）

## スコープ外（将来の別 TODO 候補）

- Zone ID 付き IPv6（`[fe80::1%25eth0]` 等、RFC 6874）
  → axum の Authority parser が対応しないため自動的に拒否される。現時点で追加テストの価値は薄い。
- `normalize_authority` の fallback 分岐（Authority parse 失敗経路）を IPv6 で突くケース
  → 実ブラウザの HOST ヘッダーでは発生しない経路のため対象外。

## 成功基準

- 上記 5 テストが Pass し、`src/server/guards.rs` の IPv6 経路が 3 層以上で検証される。
- 将来のリファクタリング・依存アップグレードで IPv6 経路の挙動が変化した場合、最低 1 件の assertion が失敗する。
