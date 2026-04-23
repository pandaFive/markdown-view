# 観測性・防御的改善（CSP fail-fast + パスログ衛生）

**日付:** 2026-04-23
**関連 TODO:** `docs/todo/TODO.md` Medium Priority
- 「CSP フォールバック時の方針整理（fail-fast vs 現状運用）」
- 「エラー経路ログのパス情報を base 相対化」

**対象ファイル（要修正）:**
- `src/server/guards.rs`
- `src/server/routes.rs`
- `src/server/files/resolve.rs`
- `src/server/files/catalog.rs`
- `src/server/files/memo.rs`
- `src/server/files/content.rs`
- `src/watcher/strategy.rs`
- `src/server/log_path.rs`（新規）

## 目的

「異常系で振る舞いが曖昧なまま動き続ける」ふたつのコード経路を、明示的な防御方針で潰す。

1. **CSP フォールバック**: `HeaderValue::from_str` 失敗時に sha256 制約を失った permissive CSP が動き続ける問題を、startup panic（fail-fast）で排除する。
2. **パスログ漏出**: 異常系 `tracing::warn!` が絶対パスをそのまま吐く問題を、base_dir 相対化ヘルパー経由で抑制する。

両者は性質が異なる（前者は構造的バグ防御、後者はログ衛生）が、いずれも「個人使用 CLI における observability 上の防御深化」というテーマで共通する。

## Part 1: CSP fail-fast

### 背景

`src/server/guards.rs:13-38` の `build_csp_header` は `HeaderValue::from_str(&csp)` 失敗時に下記のフォールバック CSP に落ちる:

```text
default-src 'self'; object-src 'none'; frame-ancestors 'none'
```

このフォールバックでは `script-src` が暗黙の `default-src` 値（= `'self'`）に展開され、**通常時に有効だった `script-src 'sha256-...'` 制約が完全に失われる**。同一オリジン内であれば任意スクリプトが実行可能になり、`security.md` の High 違反「sha256 制約消失」相当の degradation を silent に許容している。

**ただし実際には、このフォールバック分岐は構造上ほぼ到達不能である:**
- `csp_hash_sources` の戻り値は base64 sha256（`A-Za-z0-9+/=`）のみで構成される
- format! 文字列は visible-ASCII のみ
- `HeaderValue::from_str` は visible-ASCII を全て受理する
- 失敗するには `csp_hash_sources` の出力契約自体が壊れる必要がある

つまり **「到達した = csp_hash_sources の実装契約破り（バグ）」** という不変条件が成立する。

### 設計方針

到達不能なバグ防御コードでフォールバックするのではなく、契約破りを **startup panic** で表面化させる。

- `create_router` は startup 時に 1 度だけ呼ばれる経路（per-request ではない）
- panic = startup failure と等価で、利用者は CLI 起動失敗として確実に気づく
- silent な permissive CSP よりも startup panic のほうが実害ベースで安全（ブラウザは古いキャッシュ CSP を覚えている可能性があり、後から弱い CSP に置き換わってもタブによっては検知困難）

### 変更内容

**`src/server/guards.rs`:**

```rust
pub(super) fn build_csp_header(syntax_css: &str) -> HeaderValue {
    let (script_src, style_src) = csp_hash_sources(syntax_css);
    let csp = format!(
        "default-src 'self'; script-src {}; style-src {}; img-src 'self'; connect-src 'self' ws: wss:; object-src 'none'; frame-ancestors 'none'",
        script_src, style_src
    );
    // csp_hash_sources は base64 sha256 のみを返す契約のため、
    // visible-ASCII 違反による HeaderValue::from_str 失敗は構造上到達不能。
    // 到達した場合は契約破り（バグ）であり、permissive な fallback CSP で
    // silent に degradation するより startup panic で表面化させる。
    HeaderValue::from_str(&csp).unwrap_or_else(|e| {
        panic!(
            "CSP ヘッダー生成に失敗（csp_hash_sources の出力契約破り）: {} (CSP: {})",
            e, csp
        )
    })
}
```

戻り値型を `(HeaderValue, bool)` から `HeaderValue` に変更。フォールバック静的 `HeaderValue` と `tracing::error!` / `tracing::warn!` 呼び出しを削除。

**`src/server/routes.rs:40-78`:**

- `let (csp_header, csp_fallback) = build_csp_header(...)` → `let csp_header = build_csp_header(...)`
- `csp_fallback` 変数とそれに紐づく `security_warning` 変数を削除
- `x-markdown-view-security-warning` ヘッダーを設定する `SetResponseHeaderLayer` を削除（フォールバックがないので運用シグナル自体が不要）

### 互換性

`x-markdown-view-security-warning` ヘッダーは外部公開 API ではなく内部監査用シグナル。CLAUDE.md の「外部公開 API 互換性破壊は major change」基準には該当しない。クライアント JS / E2E テストもこのヘッダーを参照していないことを実装時に grep で確認する。

### テスト

- 既存 `test_csp_hash_sources_*` は不変
- 新規ユニットテストは追加しない。理由:
  - フォールバック分岐削除に伴い「異常路」自体が消える
  - panic を強制発火させるには `csp_hash_sources` を mock する必要があるが、`OnceLock` 初期化済みの static 値を返す関数を mock する仕組みがなく、無理に mock 用境界を切ると本番コードに test-only 経路が混じる
  - 構造的不変条件は doc コメントで明文化する

## Part 2: パスログサニタイザ

### 背景

異常系 `tracing::warn!` が `path.display()` で絶対パスをそのまま出力している箇所が複数存在する（`src/server/files/resolve.rs`, `src/server/files/catalog.rs`, `src/server/files/memo.rs`, `src/server/files/content.rs`, `src/server/routes.rs`, `src/watcher/strategy.rs`）。

個人使用 CLI でも次の経路でログ漏出リスクがある:
- 画面共有・スクリーンショット時の偶発的露出
- バグ報告・issue 投稿時の copy-paste
- ファイル出力にリダイレクトされた状態での共有

絶対パス（例: `/home/propan/personal_dev/secret-project/draft.md`）はディレクトリ構造とプロジェクト命名を漏らす。base_dir 相対化（`drafts/draft.md`）にすればトラブルシュートに必要な情報は残しつつ漏出を抑制できる。

### 設計方針

#### マスキング戦略（ブレインストーミング Q3 結論）

**A) base 配下:** 相対パスで出力（`subdir/file.md`）
**B) base 外:** `<outside-base>/{file_name}` で末尾コンポーネントだけ残す
**C) file_name 取得不可（ルート等）:** `<outside-base>` で完全マスク

file_name を残すのはトラブルシュート上の最低限の手がかりを担保するため。完全マスクは保守時の生産性低下が大きすぎる（Q3 A 推薦理由）。

#### `base_dir` 自体の表示（Q3-i 結論）

`base_dir.display()` 自体は **保持**する。サーバー所有者が自身で指定した値であり、毎回マスクする実益が薄い。逆にマスクするとログから「どのプロジェクトの話か」を特定できなくなり、保守性が大きく落ちる。

例: `resolve.rs:258-260` の `"{} はベース {} の配下ではありません"` は、第1引数（`file_path`）のみ sanitize し、第2引数（`base_dir`）はそのまま `.display()` で出力する。

#### 単一ファイルモードの扱い（Q3-ii 結論）

`AppMode::base_dir()` は単一ファイルモード時に **ファイルの parent ディレクトリ**を返す（`src/server/state.rs:258` テストで確認済み）。これを擬似 base として使うことで、サニタイザは「常に base がある」前提で実装できる。

例: 単一ファイル `~/notes/today.md` を起動した場合、`base_dir = ~/notes`。同ディレクトリ内の sidecar memo (`~/notes/.memo-today.md` 等) は `".memo-today.md"` として relative 化される。

### 新規モジュール: `src/server/log_path.rs`

```rust
//! 監査ログ用のパスサニタイザ。
//!
//! 絶対パスの直接出力（`path.display()`）はディレクトリ構造を漏らすため、
//! base_dir 相対化を経由する。base 外パスは file_name のみ残して
//! `<outside-base>/{file_name}` で出力する。
//!
//! `base_dir` 自体（`base_dir.display()`）の出力はサーバー所有者が指定した
//! 値であり保守性優先で保持する。本ヘルパーの対象は base 配下/外を含む
//! 「path 引数」側のみ。

use std::borrow::Cow;
use std::path::Path;

/// 監査ログ用にパスを base 相対化する。
///
/// - `path` が `base` 配下: 相対パス文字列（例: `"subdir/file.md"`）
/// - `path` が `base` 外: `<outside-base>/{file_name}`
/// - `file_name` 取得不可（ルート等）: `<outside-base>`
pub(crate) fn sanitize_path_for_logging<'a>(path: &'a Path, base: &Path) -> Cow<'a, str> {
    match path.strip_prefix(base) {
        Ok(relative) => Cow::Owned(relative.display().to_string()),
        Err(_) => match path.file_name() {
            Some(name) => Cow::Owned(format!("<outside-base>/{}", name.to_string_lossy())),
            None => Cow::Borrowed("<outside-base>"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_sanitize_base配下を相対パスにする() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/base/subdir/file.md");
        assert_eq!(sanitize_path_for_logging(&path, &base), "subdir/file.md");
    }

    #[test]
    fn test_sanitize_base直下のファイル() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/base/file.md");
        assert_eq!(sanitize_path_for_logging(&path, &base), "file.md");
    }

    #[test]
    fn test_sanitize_base外はfile_nameのみ() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/other/secret.md");
        assert_eq!(
            sanitize_path_for_logging(&path, &base),
            "<outside-base>/secret.md"
        );
    }

    #[test]
    fn test_sanitize_file_nameなしは完全マスク() {
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/");
        assert_eq!(sanitize_path_for_logging(&path, &base), "<outside-base>");
    }

    #[test]
    fn test_sanitize_path自体がbaseの場合() {
        // strip_prefix 成功で空文字（""）になる。許容挙動。
        let base = PathBuf::from("/base");
        let path = PathBuf::from("/base");
        assert_eq!(sanitize_path_for_logging(&path, &base), "");
    }

    #[cfg(unix)]
    #[test]
    fn test_sanitize_非utf8_file_name() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let base = PathBuf::from("/base");
        let mut path = PathBuf::from("/other");
        // 不正な UTF-8 シーケンスを含む file_name
        path.push(OsStr::from_bytes(b"bad\xff.md"));
        let sanitized = sanitize_path_for_logging(&path, &base);
        // to_string_lossy が U+FFFD を挿入しても panic しないこと
        assert!(sanitized.starts_with("<outside-base>/bad"));
        assert!(sanitized.ends_with(".md"));
    }
}
```

### 置換対象一覧（A スコープ確定版）

実コードを `grep '\.display()'` で網羅し、以下の statements を置換対象とする。`base_dir.display()` 側は保持、`path` 側のみ sanitize する。

#### `src/server/files/resolve.rs`

| 箇所 | 対象 | base_dir 取得 |
|---|---|---|
| L258 | `file_path.display()` | `state.mode().base_dir()` |
| L302 | `candidate.display()` | 引数 `base_dir` |
| L349 | `expected_path.display()` | **新規パラメータ追加**（後述） |

L259, L311 の `base_dir.display()` は保持。L417 の `file_display_name` 内 `path.display().to_string()` は表示用 fallback でログではないため対象外。

**シグネチャ変更が必要な関数:**

`revalidate_single_file_target(expected_path: &Path)` →
`revalidate_single_file_target(expected_path: &Path, base_dir: &Path)`

呼び出し側 2 箇所（同ファイル内 `resolve_single_file_target` L162, `resolve_change_target` L176, `resolve_request_target` L193）に `state.mode().base_dir()` を渡す。

#### `src/server/files/catalog.rs`

該当 statements: L30-34, L42-46, L61-65, L83-87, L92-96, L100-104, L135-139, L152-156

各 `tracing::warn!` を見て、`path.display()` / `current_dir.display()` / `resolved.display()` を sanitize 化。`base_dir.display()` は保持。

`list_markdown_files_recursive` は `base_dir` を引数で受けているので追加引数不要。

#### `src/server/files/memo.rs`

| 箇所 | 対象 | 備考 |
|---|---|---|
| L443-449 | `memo_path.display()`, `unsafe_component.display()` | `ensure_safe_memo_path` 内で `base_dir = state.mode().base_dir()` を既に取得済み |

他の memo.rs `tracing::warn!`（L314, L382, L396, L413, L495, L526, L544）は `target.file_label()` 経由で出力しており、これは `relative_path.unwrap_or_else(file_display_name)` で **既に relative-safe**。対象外。

#### `src/server/files/content.rs`

| 箇所 | 対象 | base_dir 取得 |
|---|---|---|
| L172-181 | `changed_file.display()` の label fallback | `state.mode().base_dir()` |

`file_label` 構築式を以下に変更:
```rust
let file_label = state
    .mode()
    .single_file()
    .map(file_display_name)
    .unwrap_or_else(|| sanitize_path_for_logging(changed_file, state.mode().base_dir()).into_owned());
```

#### `src/server/routes.rs`

| 箇所 | 対象 | 推奨対応 |
|---|---|---|
| L206-210 | `context.target().file_path().display()` | `context.target().file_label()` への置換が最簡（既に relative-safe） |

L164 は `file_label()` 内部 fallback でログではないため対象外。

#### `src/watcher/strategy.rs`

| 箇所 | 対象 | base 取得 |
|---|---|---|
| L134 | `event.path.display()` | `collect_directory_changes` の `base_dir` 引数 |
| L173 | `path.display()` | `is_hidden_relative` の `base` 引数 |
| L195 | `path.display()` | 同上 |
| L214 | `event_path.display()` | `is_hidden_relative` 経由のため base が手元にある経路で渡す |
| L259 | `path.display()` | 関数引数で base を伝搬 |

L183（`base.display()`）は保持。

### スコープ外（再確認）

- `main.rs:117` 起動ログ — 利用者自身の指定パス、利便性優先で残す
- `src/renderer/mod.rs` の theme/asset パス — base 概念外、syntect リソースで attacker-controlled でない
- `src/server/session.rs` WS エラー — paths を含まない
- `src/server/state.rs:76,83,87` `AppModeBuildError::fmt` — startup 時の利用者向けエラーで監査ログではない
- `src/server/files/search.rs` L94, L107 — 既に `relative` 文字列を出力しており safe

### モジュール公開

`src/server/mod.rs` または `src/server.rs`（公開ファサード）に `mod log_path;` を追加し、`pub(crate)` で `sanitize_path_for_logging` を公開。

## 受け入れ基準

- `./verify.sh` 全 pass
- `cargo clippy --all-targets --all-features -- -D warnings` pass
- `cargo fmt --all -- --check` pass
- `grep -rn '\.display()' src/server/files src/watcher --include='*.rs' | grep -v test` で残っている `.display()` 出力が下記のいずれかであること:
  - `base_dir.display()` / `base.display()`（保持対象）
  - `file_display_name` 内 fallback
  - `AppModeBuildError::fmt`（スコープ外）
- 手動確認:
  - 通常起動: panic せずに起動、CSP ヘッダーに `sha256-` が含まれる、`x-markdown-view-security-warning` ヘッダーが応答に含まれない
  - ディレクトリ外 md を `?file=../outside.md` で要求した場合、ログに `<outside-base>/outside.md` 形式の表記が出る（攻撃者由来パスの絶対パス漏出がない）

## 参考

- ブレインストーミング決定:
  - Q1 (CSP): A (fail-fast)
  - Q2 (path scope): A (異常系 `tracing::warn!` のみ、main.rs / renderer / session 除外)
  - Q3 (mask): A (file_name 残し) + (i-2) (base_dir は保持) + (ii-1) (単一ファイルモードは parent を擬似 base)
- 関連: `~/.claude/rules/security.md` High「平文ログ出力」相当の予防

## 次工程

本 spec を `superpowers:writing-plans` でフェーズ別 PLAN.md に分解する。Part 1 と Part 2 は **独立**して実装・テストできるため並列可。
