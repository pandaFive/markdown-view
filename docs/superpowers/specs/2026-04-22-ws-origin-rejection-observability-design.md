# WebSocket Origin 拒否経路の観測性強化

**日付:** 2026-04-22
**関連 TODO:** `docs/todo/TODO.md` High Priority（`is_allowed_ws_origin` 拒否経路 warn ログ追加）
**対象ファイル:** `src/server/guards.rs`

## 目的

`is_allowed_ws_origin` の silent な `false` return と `is_trusted_authority` 内の userinfo / 非数値 port 拒否を監査ログで観測可能にし、HOST 経路 (`ensure_allowed_request_host`) と同等の DNS Rebinding 兆候検知を WebSocket 経路でも実現する。

## 背景

- `ensure_allowed_request_host` は拒否時に raw HOST 値を `tracing::warn!` で記録し、監査経路を持つ（L53-56）。
- 対して `is_allowed_ws_origin` は **8 箇所以上**で silent に `false` を返す（Origin なし / HOST なし / trusted でない HOST / URI parse 失敗 / 非 http(s) scheme / authority なし / trusted でない Origin authority / authority 不一致）。
- 加えて `is_trusted_authority` は **3 箇所**で silent 拒否（parse 失敗 / userinfo `@` 含む / 非数値 port）。これらは DNS Rebinding 兆候が強いにもかかわらず、HOST/Origin どちらから呼ばれて発火したかも含めて痕跡が残らない。
- pr-review-toolkit の silent-failure-hunter による指摘: 攻撃者が WebSocket 経路で DNS Rebinding を試行した場合、HOST 経路では検知できるが Origin 経路では完全に silent で「ブラウザが Origin を送っていない」と区別できない。

## 設計方針

### 構造方針：ハイブリッド（ブレインストーミング結論）

- **`is_allowed_ws_origin`**: 拒否理由を enum `WsOriginRejection` で返す内部関数 `check_ws_origin` を新設。public wrapper `is_allowed_ws_origin` は `check_ws_origin` の結果を `bool` に変換すると同時にログ出力を担当する。
- **`is_trusted_authority`**: `bool` 戻り値は維持し、`context: &'static str` 引数を追加。拒否分岐（parse 失敗 / userinfo / 非数値 port）で `tracing::warn!` を直書きし、ログ内に呼び出し元（`"host"` or `"origin_authority"`）を埋め込む。

enum + wrapper パターンはテスト可能性と呼び出し元との分離を両立し、inline warn は variant を新設するほどの構造価値がない細部に適用する。

### ログレベルの段階化

攻撃兆候の強弱で `warn` / `info` を振り分ける：

| 拒否理由 | レベル | 理由 |
|---|---|---|
| `MissingOrigin` | info | curl / 非ブラウザクライアントで通常起きる |
| `MissingHost` | info | 通常ブラウザで欠落しないが、特別な攻撃兆候ではない |
| `UntrustedHost` | warn | HOST 経路の警告と同格の兆候 |
| `OriginParseError` | warn | malformed Origin は通常発生しない |
| `UnsupportedScheme` | warn | 中程度の兆候（誤設定の可能性もあるが可視化する） |
| `OriginMissingAuthority` | warn | 異常形式の Origin |
| `UntrustedOriginAuthority` | warn | 外部 Origin からの接続試行 |
| `AuthorityMismatch` | warn | **DNS Rebinding の典型兆候** |

`is_trusted_authority` 内の warn（parse 失敗 / userinfo / 非数値 port）は全て **warn**（いずれも攻撃兆候が強い）。

### ログフォーマット

既存 HOST 経路を Debug (`{:?}`) に揃えることで、制御文字を含む値によるログインジェクションを一括で防ぐ。

```text
warn: [markdown-view] WS Origin 拒否 (AuthorityMismatch): host="localhost:3000" origin="http://evil.example:3000"
info: [markdown-view] WS Origin 拒否 (MissingOrigin): host="localhost:3000" origin="<missing>"
warn: [markdown-view] authority に userinfo を検出し拒否 (context=origin_authority): "user@[::1]:3000"
warn: [markdown-view] authority に非数値 port を検出し拒否 (context=host): "[::1]:abc"
warn: [markdown-view] authority の parse に失敗し拒否 (context=host): "bad\\n\\r"
```

ヘッダーが存在しない場合は `<missing>` 固定文字列を使用する。

### 既存 HOST 経路のフォーマット統一

`ensure_allowed_request_host` L53-56 の warn は現在 `{}` で raw 値を出力している。本コミット内で `{:?}` に統一し、Origin 経路との対称性とログインジェクション耐性を確保する。

## 変更仕様

### 1. `WsOriginRejection` 定義

`src/server/guards.rs` に `pub(super)` 可視性で追加：

```rust
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
```

`Debug` は log 出力（`{:?}`）用。`PartialEq` はテストでの比較用。

### 2. `check_ws_origin` 新設

```rust
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

### 3. `is_allowed_ws_origin` を wrapper に書き換え

```rust
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

### 4. `is_trusted_authority` にコンテキスト引数追加

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
    if parsed.as_str().contains('@') {
        tracing::warn!(
            "[markdown-view] authority に userinfo を検出し拒否 (context={}): {:?}",
            context,
            authority
        );
        return false;
    }
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

`is_trusted_host` 側の「trusted でない host 名」の拒否は呼び出し元（`is_allowed_request_host` / `check_ws_origin`）で拾われるためここでは warn を出さない（重複ログ防止）。

### 5. 呼び出し元更新

- `is_allowed_request_host` L68: `is_trusted_authority(host)` → `is_trusted_authority(host, "host")`
- `check_ws_origin` 内 2 箇所: 上記実装に記載済み

### 6. 既存 HOST 経路の `{:?}` 統一

`ensure_allowed_request_host` L53-56:

```rust
tracing::warn!(
    "[markdown-view] 許可されていないHostヘッダーを拒否: {:?}",
    host
);
```

`host` は既に `unwrap_or("<missing-or-invalid>")` 済みの `&str`。`{}` → `{:?}` のみ変更。

## テスト設計

### 既存テストの変更

**`test_allowed_ws_origin_*` 8 個**を `check_ws_origin` の返り値検証に拡張（既存 `is_allowed_ws_origin` 経由の bool アサートは残しつつ、enum variant も assert）。

具体例（既存 `test_allowed_ws_origin_rejects_different_host`）：

```rust
#[test]
fn test_allowed_ws_origin_rejects_different_host() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "localhost:3000".parse().unwrap());
    headers.insert(ORIGIN, "http://evil.example:3000".parse().unwrap());
    assert!(!is_allowed_ws_origin(&headers));
    // HOST は trusted だが Origin authority が untrusted のため
    // AuthorityMismatch ではなく UntrustedOriginAuthority に到達する
    assert_eq!(
        check_ws_origin(&headers),
        Err(WsOriginRejection::UntrustedOriginAuthority)
    );
}
```

既存 `test_allowed_ws_origin_rejects_different_port`（HOST=localhost:3000 + Origin=http://localhost:4000）は両 authority が trusted で正規化結果が異なるため `AuthorityMismatch` にマップされる。

### 追加テスト

**T1 `test_check_ws_origin_variants_網羅`**
8 variant すべてを 1 ケースずつ返り値で検証：

| variant | HOST | Origin | 到達理由 |
|---|---|---|---|
| `MissingOrigin` | `localhost:3000` | （省略） | Origin ヘッダー不在 |
| `MissingHost` | （省略） | `http://localhost:3000` | HOST ヘッダー不在 |
| `UntrustedHost` | `evil.example:3000` | `http://localhost:3000` | HOST が trusted でない |
| `OriginParseError` | `localhost:3000` | `not a uri` | Origin が URI として parse 不可 |
| `UnsupportedScheme` | `localhost:3000` | `ftp://localhost:3000` | http/https 以外 |
| `OriginMissingAuthority` | `localhost:3000` | `http:///` | Origin に authority なし |
| `UntrustedOriginAuthority` | `localhost:3000` | `http://evil.example:3000` | HOST は trusted、Origin authority が trusted でない |
| `AuthorityMismatch` | `localhost:3000` | `http://127.0.0.1:3000` | 両 authority trusted だが正規化結果が異なる |

**variant 到達順の注意点**: `check_ws_origin` は早期 return の連鎖でフローを決定するため、テストケースは「検証したい variant の分岐に到達するまで、前段チェックを全て pass させる」設計になっている必要がある。特に:

- `UntrustedOriginAuthority` には HOST が trusted である必要がある（さもないと `UntrustedHost` で早期 return される）
- `AuthorityMismatch` には両 authority が trusted である必要がある（さもないと `UntrustedOriginAuthority` で早期 return される）。`127.0.0.1` と `localhost` はどちらも trusted だが `normalize_authority` 出力が異なるため、この variant を確実に起こせる

**T2 `test_is_trusted_authority_context_引数を受け取る`**
`is_trusted_authority("user@localhost:3000", "host")` と `is_trusted_authority("[::1]:abc", "origin_authority")` がいずれも `false` を返すことを確認。ログ内容のアサートは行わない（後述「テスト範囲外」）。

### テスト範囲外（YAGNI）

- **ログ出力内容のアサート**: `tracing-test` 等の導入コスト > ベネフィット。enum の返り値と分岐の存在で構造的には検証済み。手動検証は `RUST_LOG=markdown_view=info cargo run` と不正リクエスト送出で一度確認する。
- **rate limit / 重複抑制**: 個人使用前提のツールで、非ブラウザ接続の info ログがログを埋める運用懸念は低い。必要になった時点で別 TODO に切り出す。

### テスト件数サマリ

- 既存拡張: 8 テスト（既存 `test_allowed_ws_origin_*` に enum assert を追加）
- 新規: 2 テスト（T1 網羅、T2 context 引数）
- 合計アサーション: 既存 8 + 新規約 10 = 約 18 件

## 非目標

- `ensure_allowed_request_host` を `check_request_host` に構造化すること（HOST 経路は既に warn ログを持つため、観測性ギャップの解消という目的から外れる）。
- `tracing::event!` with fields への置き換え（Debug 文字列フォーマットで目的を達成可能、依存増加なし）。
- Origin 検証ロジック自体の強化（本タスクは観測性強化のみ、判定ロジックは不変）。

## リスクと緩和

| リスク | 緩和策 |
|---|---|
| `is_trusted_authority` のシグネチャ変更が呼び出し元 3 箇所に波及 | 本 spec の §2.5 で呼び出し元を全列挙。`pub(super)` 可視性のためクレート外影響なし。cargo build でコンパイルエラーとして検出可能 |
| 非ブラウザクライアント（curl, wscat）接続時の info ログ増加 | `info` レベルのため運用環境のデフォルト（warn）では出ない。意図的に観測する際のみ `RUST_LOG=markdown_view=info` で有効化 |
| ログインジェクション（raw Origin に `\r\n` 制御文字） | `{:?}` フォーマッタが制御文字を自動エスケープ。同コミット内で HOST 経路も `{:?}` に統一 |
| `AuthorityMismatch` と `UntrustedOriginAuthority` の発火順序が直感と異なる | テスト T1 に明示コメント。コード内のフロー順序も spec §2.2 と一致 |

## 変更ファイル一覧

- `src/server/guards.rs` — 本体 + テスト。追加約 120 行、変更約 10 行。
- `docs/todo/TODO.md` — 該当 High 項目の完了マーク（実装 PR 完了時）。

## 受け入れ基準

1. `cargo test --all-targets --all-features` 全 Pass
2. `cargo clippy --all-targets --all-features -- -D warnings` warning なし
3. `cargo fmt --all -- --check` 差分なし
4. `npm run typecheck` 影響なし（E2E には波及しない）
5. 手動確認: `RUST_LOG=markdown_view=info cargo run -- README.md` 起動後、curl で HOST なし WebSocket upgrade 要求を送出し info ログが出ることを確認
6. 既存 8 テスト (`test_allowed_ws_origin_*`) と新規 2 テストがすべて Pass

## 実装順序（次段階の writing-plans 用メモ）

1. `WsOriginRejection` enum 定義追加
2. `is_trusted_authority` に `context` 引数を追加し、warn 3 箇所を挿入
3. 呼び出し元 (`is_allowed_request_host`) を更新
4. `check_ws_origin` 新設
5. `is_allowed_ws_origin` を wrapper に書き換え（log_rejection ロジック内包）
6. 既存 8 テストに enum assert 追加
7. 新規 2 テスト追加
8. `ensure_allowed_request_host` のログを `{:?}` に統一
9. verify.sh で全検証

各ステップを小コミットに分割するかは writing-plans で議論する。
