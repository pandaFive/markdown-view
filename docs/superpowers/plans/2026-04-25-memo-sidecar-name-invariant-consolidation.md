# Memo Sidecar Name Invariant Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `SidecarMemoName` を sidecar 名生成の Single Source of Truth に格上げし、`memo.rs` の到達不能な検査と分岐を撤去、境界テストを追加する。

**Architecture:** 既に 255 bytes 以下を構築時保証する `SidecarMemoName` 型に doc コメントで不変条件を明記し、その不変条件をユニットテストで固定する。ファイルシステム上限の重複チェック（`sidecar_name_too_long`）と関連 4 か所の分岐は撤去する。runtime behavior は変えない。

**Tech Stack:** Rust 1.85+, cargo test, sha2, tokio (テストでは未使用), `#[cfg(unix)]` で非 UTF-8 パスの cfg-gated テスト

**Spec:** [`docs/superpowers/specs/2026-04-25-memo-sidecar-name-invariant-consolidation-design.md`](../specs/2026-04-25-memo-sidecar-name-invariant-consolidation-design.md)

**Branch:** `refactor/sidecar-name-invariant-consolidation`（既に作成済み、設計書コミット `d91b81d` 済み）

---

## Pre-flight

- [ ] **Step P1: 現在のブランチを確認**

Run: `git branch --show-current`
Expected: `refactor/sidecar-name-invariant-consolidation`

- [ ] **Step P2: ベースラインの全テスト pass を確認**

Run: `./verify.sh`
Expected: 全 pass（fmt / clippy / cargo test / typecheck）

`./verify.sh` が落ちる場合はこのプランを開始しないこと。

---

## Task 1: 境界テストと不変条件テストを追加

**Files:**
- Modify: `src/server/files/tests.rs`（既存の sidecar 名テスト L48-147 の直後、L148 の `test_sidecar_parent_*` 直前に挿入）
- Test: 同ファイル

5 件のテストを追加する。`SidecarMemoName` は既に 255 bytes 以下を構築時保証しているため、追加時点で全 pass する想定。これは「既存の不変条件を documentation テストとして固定する」のが目的で、後続タスク（死んだコード削除）後にも pass し続けることが不変条件の集約の保証となる。

- [ ] **Step 1.1: 個別境界テスト 4 件を追加**

`src/server/files/tests.rs` の `test_sidecar_name_非utf8名はhashで衝突しない` テストの閉じ `}`（L147）の直後、`fn test_sidecar_parent_相対パスはbase_dirへfallbackする` テストの直前に以下を挿入する。間に空行 1 つを挟む（既存の慣習に揃える）:

```rust
#[test]
fn test_sidecar_name_255バイト境界はそのまま使う() {
    // .{name}.memo.md = 1 + name + 8 = 255 byte ぴったりに収まる input
    // → name = 246 → "a"*243 + ".md"
    let file_name = format!("{}.md", "a".repeat(243));
    assert_eq!(file_name.len(), 246);
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert_eq!(name.len(), 255, "境界ちょうどはそのまま使う");
    assert_eq!(name, format!(".{}.memo.md", file_name));
    // hash 経路に入っていないことの確認: 元 filename がそのまま含まれる
    assert!(name.contains(&"a".repeat(243)));
}

#[test]
fn test_sidecar_name_256バイト境界はhash経路に入る() {
    // 1 byte 超過で hash truncation 経路
    // .{name}.memo.md = 1 + 247 + 8 = 256 → hash 経路
    let file_name = format!("{}.md", "a".repeat(244));
    assert_eq!(file_name.len(), 247);
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(
        name.len() <= 255,
        "boundary 直上で 255 を超えてはいけない: {}",
        name.len()
    );
    // hash 経路の形式確認: . + prefix + . + 16hex + .memo.md
    // split('.') で ["", prefix, hash, "memo", "md"] の 5 要素
    let parts: Vec<&str> = name.split('.').collect();
    assert_eq!(parts.len(), 5, "hash 経路は 4 dot 区切り: {name}");
    let hash = parts[2];
    assert_eq!(hash.len(), 16, "hash suffix は 16 hex chars: {hash}");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "hash suffix は hex のみ: {hash}"
    );
}

#[test]
fn test_sidecar_name_utf8マルチバイト境界の直前で切断する() {
    // prefix budget = 255 - (1+1+16+8) = 229 bytes
    // budget の境目に 3-byte UTF-8 char (`あ`) を置き、char 境界で切断されることを確認
    // input: "あ"*90 + "tail.md" → 270 + 7 = 277 bytes (hash 経路に確実に入る)
    let file_name = format!("{}tail.md", "あ".repeat(90));
    assert_eq!(file_name.len(), 277);
    let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name));
    let name = sidecar.as_str();
    assert_plain_sidecar_filename(name);
    assert!(name.len() <= 255);
    // 切断点が UTF-8 char 境界にあること（&str[..end] は char boundary を要求するため、
    //   ここまで到達できている時点で UTF-8 として valid。明示的にも検証）
    assert!(name.is_char_boundary(name.len()));
    assert!(std::str::from_utf8(name.as_bytes()).is_ok(), "valid UTF-8: {name}");
}

#[test]
fn test_sidecar_name_パス区切り含む超長名でも255以下_衝突しない() {
    // 区切り種別と位置が異なる 2 つの 300+ byte input で衝突しないこと
    let file_name1 = format!("{}/{}.md", "a".repeat(200), "b".repeat(100));
    let file_name2 = format!("{}\\{}.md", "a".repeat(150), "b".repeat(150));
    assert_eq!(file_name1.len(), 304);
    assert_eq!(file_name2.len(), 304);
    let first = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name1));
    let second = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&file_name2));
    let n1 = first.as_str();
    let n2 = second.as_str();
    assert_plain_sidecar_filename(n1);
    assert_plain_sidecar_filename(n2);
    assert!(n1.len() <= 255);
    assert!(n2.len() <= 255);
    assert!(
        !n1.contains('/') && !n1.contains('\\'),
        "パス区切りが残らない: {n1}"
    );
    assert!(
        !n2.contains('/') && !n2.contains('\\'),
        "パス区切りが残らない: {n2}"
    );
    assert_ne!(n1, n2, "区切り種別 / 位置が異なる場合は衝突しない");
}
```

- [ ] **Step 1.2: 不変条件テーブル駆動テストを追加**

Step 1.1 の最後（`test_sidecar_name_パス区切り含む超長名でも255以下_衝突しない` の閉じ `}` の直後）に以下を続けて挿入:

```rust
#[test]
fn test_sidecar_name_任意入力で常に255バイト以下_不変条件_utf8() {
    // UTF-8 入力の網羅的境界ケース: 出力が常に MAX_FILENAME_BYTES (= 255) 以下
    let cases: Vec<String> = vec![
        String::new(),
        "a".to_string(),
        "a".repeat(254),
        "a".repeat(255),
        "a".repeat(256),
        "a".repeat(1000),
        "あ".repeat(100),
        "../../etc/passwd".to_string(),
        "a/b\\c".to_string(),
        "./relative/path/with/many/segments.md".to_string(),
    ];
    for input in cases {
        let sidecar = SidecarMemoName::from_file_name(std::ffi::OsStr::new(&input));
        let name = sidecar.as_str();
        assert!(
            name.len() <= 255,
            "input {} bytes → output {} bytes (255 超過): {name}",
            input.len(),
            name.len()
        );
        assert!(name.ends_with(".memo.md"), ".memo.md 終端: {name}");
        assert!(name.starts_with('.'), ". 開始: {name}");
        assert!(!name.contains('/'), "/ 含まない: {name}");
        assert!(!name.contains('\\'), "\\ 含まない: {name}");
    }
}

#[cfg(unix)]
#[test]
fn test_sidecar_name_任意入力で常に255バイト以下_不変条件_非utf8() {
    use std::os::unix::ffi::OsStrExt;

    let cases: Vec<Vec<u8>> = vec![
        b"\xff\xfe\xfd".to_vec(),
        vec![0xff; 100],
        vec![0xff; 255],
        vec![0xff; 1000],
    ];
    for raw in cases {
        let os_str = std::ffi::OsStr::from_bytes(&raw);
        let sidecar = SidecarMemoName::from_file_name(os_str);
        let name = sidecar.as_str();
        assert!(
            name.len() <= 255,
            "非UTF-8 input {} bytes → output {} bytes",
            raw.len(),
            name.len()
        );
        assert!(name.starts_with("._bin."), "非UTF-8 fallback 形式: {name}");
        assert!(name.ends_with(".memo.md"));
    }
}
```

- [ ] **Step 1.3: テストを実行して全 pass を確認**

Run: `cargo test --all-targets --all-features sidecar_name -- --nocapture`
Expected: 6 件の新規テスト（Step 1.1 の 4 件 + Step 1.2 の 2 件）+ 既存 sidecar 名テストが全て pass。

万一どれか 1 件でも fail したら、`SidecarMemoName` の実装が不変条件を満たしていないか、テストの assertion ロジックが間違っている。fail メッセージを読んで原因を特定すること。

- [ ] **Step 1.4: フォーマット・lint チェック**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: 全 pass（warnings なし）。

fmt 違反があれば `cargo fmt --all` を実行して修正。

- [ ] **Step 1.5: コミット**

```bash
git add src/server/files/tests.rs
```

コミットメッセージは以下の内容で `/tmp/commit-msg-task1.txt` を作成し `git commit -F /tmp/commit-msg-task1.txt` で実行（heredoc は project hook の誤検知を回避するため使わない）:

```
test: SidecarMemoName の 255 バイト不変条件と境界テストを追加

変更内容:
- 個別境界テスト 4 件追加: 255 ちょうど / 256 hash 経路 / UTF-8 マルチバイト境界 / パス区切り超長名
- 不変条件テーブル駆動テスト 2 件追加: UTF-8 / 非UTF-8 (cfg unix) で任意入力の出力長 ≤ 255 を網羅

変更理由:
- 設計書 (2026-04-25-memo-sidecar-name-invariant-consolidation-design.md) で型レベル不変条件を
  Single Source of Truth として明文化したため、これを documentation テストとして固定する
- 後続コミットで撤去する sidecar_name_too_long の役割を、テストレベルで肩代わりする

影響範囲:
- src/server/files/tests.rs のみ。runtime コード未変更
- 既存の sidecar 名テストおよび save/load behavior テストには影響なし

テスト結果: cargo test --all-targets --all-features sidecar_name 全 pass
```

その後 `rm /tmp/commit-msg-task1.txt`。

---

## Task 2: SidecarMemoName の doc コメントに不変条件を明記

**Files:**
- Modify: `src/server/files/memo_sidecar.rs:12-58`
- Test: なし（doc 変更のみ）

- [ ] **Step 2.1: 型 doc と各 constructor doc を追加**

`src/server/files/memo_sidecar.rs` の以下を編集する。

**変更前** (`src/server/files/memo_sidecar.rs:12-58`):
```rust
pub(super) struct SidecarMemoName(String);

impl SidecarMemoName {
    pub(super) fn fallback() -> Self {
        Self(MEMO_SUFFIX.to_string())
    }

    pub(super) fn from_file_name(file_name: &OsStr) -> Self {
        if let Some(name) = file_name.to_str() {
            if name.is_empty() {
                return Self::fallback();
            }
            return Self::from_utf8_name(name);
        }

        #[cfg(unix)]
        {
            Self(format!(
                "._bin.{}{}",
                short_hash(file_name.as_bytes()),
                MEMO_SUFFIX
            ))
        }

        #[cfg(not(unix))]
        {
            Self::fallback()
        }
    }

    #[cfg(unix)]
    pub(super) fn compat_from_file_name(file_name: &OsStr) -> Option<Self> {
        let name = file_name.to_str().filter(|name| !name.is_empty())?;
        if !name.contains('\\') || name.contains('/') {
            return None;
        }
        Some(Self(build_legacy_utf8_name(name)))
    }

    #[cfg(not(unix))]
    pub(super) fn compat_from_file_name(_file_name: &OsStr) -> Option<Self> {
        None
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
```

**変更後**:
```rust
/// メモ sidecar ファイル名生成の Single Source of Truth。
///
/// **不変条件**: 任意の `OsStr` 入力に対して、`as_str().len() <= MAX_FILENAME_BYTES (= 255)`
/// を構築時に保証する。UTF-8 経路の hash truncation、Unix 非 UTF-8 経路の `._bin.{hash}.memo.md`、
/// fallback の `.memo.md` がいずれも 255 bytes 以下に収まる。
///
/// この不変条件は `src/server/files/tests.rs` の
/// `test_sidecar_name_任意入力で常に255バイト以下_不変条件_*` で固定される。
/// 呼び出し側でファイルシステム上限の重複チェックを行う必要はない。
pub(super) struct SidecarMemoName(String);

impl SidecarMemoName {
    /// 入力なし / 不明なファイル名向けの fallback 名 (`.memo.md`)。
    /// 出力は常に 8 bytes で、`MAX_FILENAME_BYTES` を超えない。
    pub(super) fn fallback() -> Self {
        Self(MEMO_SUFFIX.to_string())
    }

    /// `OsStr` ファイル名から sidecar 名を構築する。
    /// 出力は任意の入力に対して `MAX_FILENAME_BYTES (= 255)` bytes 以下を保証する。
    pub(super) fn from_file_name(file_name: &OsStr) -> Self {
        if let Some(name) = file_name.to_str() {
            if name.is_empty() {
                return Self::fallback();
            }
            return Self::from_utf8_name(name);
        }

        #[cfg(unix)]
        {
            Self(format!(
                "._bin.{}{}",
                short_hash(file_name.as_bytes()),
                MEMO_SUFFIX
            ))
        }

        #[cfg(not(unix))]
        {
            Self::fallback()
        }
    }

    /// 旧形式 (backslash を区切り正規化せずそのまま含む) の sidecar 名を再構築する。
    /// 出力は `MAX_FILENAME_BYTES (= 255)` bytes 以下を保証する。
    /// 入力に区切り正規化対象 (`\`) が含まれない場合は `None` を返す。
    #[cfg(unix)]
    pub(super) fn compat_from_file_name(file_name: &OsStr) -> Option<Self> {
        let name = file_name.to_str().filter(|name| !name.is_empty())?;
        if !name.contains('\\') || name.contains('/') {
            return None;
        }
        Some(Self(build_legacy_utf8_name(name)))
    }

    #[cfg(not(unix))]
    pub(super) fn compat_from_file_name(_file_name: &OsStr) -> Option<Self> {
        None
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
```

- [ ] **Step 2.2: テストとビルドを確認**

Run: `cargo test --all-targets --all-features memo_sidecar`
Expected: 既存テスト全 pass。

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: 全 pass。

- [ ] **Step 2.3: コミット**

```bash
git add src/server/files/memo_sidecar.rs
```

コミットメッセージを `/tmp/commit-msg-task2.txt` に書いて `git commit -F` で実行:

```
docs: SidecarMemoName の不変条件を doc コメントに明記

変更内容:
- 型 doc に「任意の OsStr 入力に対して as_str().len() <= 255 を構築時保証」を明記
- fallback / from_file_name / compat_from_file_name の各 constructor doc に
  255 bytes 以下保証を 1 行で追加
- 不変条件を固定するテストの参照を doc に記載

変更理由:
- 直前のタスクで追加した境界テストおよび不変条件テーブル駆動テストの意図を、
  型側からも明示的に示し、Single Source of Truth であることを doc レベルで担保する
- 後続コミットで撤去する sidecar_name_too_long の役割を、型 doc レベルで肩代わりする

影響範囲:
- src/server/files/memo_sidecar.rs の doc コメントのみ。実装ロジックおよび runtime 挙動は不変

テスト結果: cargo test memo_sidecar 全 pass、fmt / clippy 全 pass
```

その後 `rm /tmp/commit-msg-task2.txt`。

---

## Task 3: sidecar_name_too_long と 4 か所の死んだ分岐を撤去

**Files:**
- Modify: `src/server/files/memo.rs:6, 53-56, 221-248, 257-271, 286-288, 590-594`
- Modify: `src/server/files/memo_sidecar.rs:9`（`MAX_FILENAME_BYTES` の visibility 縮小）
- Test: 既存 + Task 1 で追加した全テスト

`SidecarMemoName` が型として ≤ 255 bytes を保証するため、呼び出し側の重複チェックを撤去する。

- [ ] **Step 3.1: `memo.rs` の `save_route_memo` 内分岐を撤去 (L51-63 周辺)**

`src/server/files/memo.rs` の以下を編集。

**変更前** (`src/server/files/memo.rs:51-63`):
```rust
    if trimmed.is_empty() {
        delete_legacy_memo_if_safe_strict(state, target, request, &memo_paths.legacy).await?;
        if !sidecar_name_too_long(&memo_paths.sidecar) {
            ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
            delete_memo_file_if_exists(&memo_paths.sidecar, target, request).await?;
        }
        if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
            delete_compat_sidecar_if_safe(state, target, request, compat_sidecar).await?;
        }
        return Ok(MemoResponse::empty(
            target.relative_path().map(ToOwned::to_owned),
        ));
    }
```

**変更後**:
```rust
    if trimmed.is_empty() {
        delete_legacy_memo_if_safe_strict(state, target, request, &memo_paths.legacy).await?;
        ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
        delete_memo_file_if_exists(&memo_paths.sidecar, target, request).await?;
        if let Some(compat_sidecar) = &memo_paths.compat_sidecar {
            delete_compat_sidecar_if_safe(state, target, request, compat_sidecar).await?;
        }
        return Ok(MemoResponse::empty(
            target.relative_path().map(ToOwned::to_owned),
        ));
    }
```

- [ ] **Step 3.2: `memo.rs` の `resolve_active_memo_path` 内分岐を撤去 (L221-248 周辺)**

**変更前** (`src/server/files/memo.rs:215-249`):
```rust
async fn resolve_active_memo_path(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
) -> Result<PathBuf, ApiError> {
    let sidecar_usable = !sidecar_name_too_long(&memo_paths.sidecar);
    if sidecar_usable {
        ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
    }
    if sidecar_usable
        && tokio::fs::try_exists(&memo_paths.sidecar)
            .await
            .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(memo_paths.sidecar.clone());
    }
    if compat_sidecar_exists(state, target, request, memo_paths).await? {
        return Ok(memo_paths
            .compat_sidecar
            .clone()
            .expect("compat path exists"));
    }

    match inspect_legacy_memo(state, target, request, &memo_paths.legacy).await? {
        LegacyMemoState::SafeExists => Ok(memo_paths.legacy.clone()),
        LegacyMemoState::SafeMissing | LegacyMemoState::Unsafe => {
            if sidecar_usable {
                Ok(memo_paths.sidecar.clone())
            } else {
                Ok(memo_paths.legacy.clone())
            }
        }
    }
}
```

**変更後**:
```rust
async fn resolve_active_memo_path(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
) -> Result<PathBuf, ApiError> {
    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
    if tokio::fs::try_exists(&memo_paths.sidecar)
        .await
        .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(memo_paths.sidecar.clone());
    }
    if compat_sidecar_exists(state, target, request, memo_paths).await? {
        return Ok(memo_paths
            .compat_sidecar
            .clone()
            .expect("compat path exists"));
    }

    match inspect_legacy_memo(state, target, request, &memo_paths.legacy).await? {
        LegacyMemoState::SafeExists => Ok(memo_paths.legacy.clone()),
        LegacyMemoState::SafeMissing | LegacyMemoState::Unsafe => Ok(memo_paths.sidecar.clone()),
    }
}
```

- [ ] **Step 3.3: `memo.rs` の `choose_save_target` 内分岐を撤去 (L251-310 周辺、2 か所)**

**変更前** (`src/server/files/memo.rs:251-310`):
```rust
async fn choose_save_target(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
) -> Result<SaveTarget, ApiError> {
    let sidecar_too_long = sidecar_name_too_long(&memo_paths.sidecar);
    if !sidecar_too_long {
        ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
    }
    if !sidecar_too_long
        && tokio::fs::try_exists(&memo_paths.sidecar)
            .await
            .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(SaveTarget::Sidecar {
            fallback: SidecarFallback::None,
            delete_legacy_after_save: false,
            delete_compat_after_save: false,
        });
    }
    let compat_sidecar_exists = compat_sidecar_exists(state, target, request, memo_paths).await?;
    let legacy_state = inspect_legacy_memo(state, target, request, &memo_paths.legacy).await?;
    if compat_sidecar_exists {
        return Ok(SaveTarget::Sidecar {
            fallback: if legacy_state == LegacyMemoState::SafeExists {
                SidecarFallback::CompatThenLegacy
            } else {
                SidecarFallback::Compat
            },
            delete_legacy_after_save: false,
            delete_compat_after_save: true,
        });
    }

    if sidecar_too_long {
        return Ok(SaveTarget::Legacy);
    }

    if matches!(
        legacy_state,
        LegacyMemoState::SafeMissing | LegacyMemoState::Unsafe
    ) {
        return Ok(SaveTarget::Sidecar {
            fallback: if state.mode().is_directory() {
                SidecarFallback::Legacy
            } else {
                SidecarFallback::None
            },
            delete_legacy_after_save: false,
            delete_compat_after_save: false,
        });
    }

    Ok(SaveTarget::Sidecar {
        fallback: SidecarFallback::Legacy,
        delete_legacy_after_save: true,
        delete_compat_after_save: false,
    })
}
```

**変更後**:
```rust
async fn choose_save_target(
    state: &AppState,
    target: &ResolvedTarget,
    request: RouteTargetRequest<'_>,
    memo_paths: &MemoPaths,
) -> Result<SaveTarget, ApiError> {
    ensure_safe_memo_path(&memo_paths.sidecar, state, target, request)?;
    if tokio::fs::try_exists(&memo_paths.sidecar)
        .await
        .map_err(|error| io_api_error(target, request, "存在確認", error))?
    {
        return Ok(SaveTarget::Sidecar {
            fallback: SidecarFallback::None,
            delete_legacy_after_save: false,
            delete_compat_after_save: false,
        });
    }
    let compat_sidecar_exists = compat_sidecar_exists(state, target, request, memo_paths).await?;
    let legacy_state = inspect_legacy_memo(state, target, request, &memo_paths.legacy).await?;
    if compat_sidecar_exists {
        return Ok(SaveTarget::Sidecar {
            fallback: if legacy_state == LegacyMemoState::SafeExists {
                SidecarFallback::CompatThenLegacy
            } else {
                SidecarFallback::Compat
            },
            delete_legacy_after_save: false,
            delete_compat_after_save: true,
        });
    }

    if matches!(
        legacy_state,
        LegacyMemoState::SafeMissing | LegacyMemoState::Unsafe
    ) {
        return Ok(SaveTarget::Sidecar {
            fallback: if state.mode().is_directory() {
                SidecarFallback::Legacy
            } else {
                SidecarFallback::None
            },
            delete_legacy_after_save: false,
            delete_compat_after_save: false,
        });
    }

    Ok(SaveTarget::Sidecar {
        fallback: SidecarFallback::Legacy,
        delete_legacy_after_save: true,
        delete_compat_after_save: false,
    })
}
```

- [ ] **Step 3.4: `memo.rs` の `sidecar_name_too_long` 関数定義を削除 (L590-594 周辺)**

**変更前** (`src/server/files/memo.rs:590-594` の前後):
```rust
    delete_memo_file_if_exists(legacy_path, target, request).await
}

fn sidecar_name_too_long(sidecar_path: &Path) -> bool {
    sidecar_path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().len() > MAX_FILENAME_BYTES)
}

fn ensure_safe_memo_path(
```

**変更後**:
```rust
    delete_memo_file_if_exists(legacy_path, target, request).await
}

fn ensure_safe_memo_path(
```

- [ ] **Step 3.5: `memo.rs` の import から `MAX_FILENAME_BYTES` を削除 (L6)**

**変更前** (`src/server/files/memo.rs:6`):
```rust
use super::memo_sidecar::{SidecarMemoName, MAX_FILENAME_BYTES};
```

**変更後**:
```rust
use super::memo_sidecar::SidecarMemoName;
```

- [ ] **Step 3.6: `memo_sidecar.rs` の `MAX_FILENAME_BYTES` を private に縮小**

`memo.rs` から参照されなくなったため、visibility を `pub(super)` から private に縮小する。

**変更前** (`src/server/files/memo_sidecar.rs:9`):
```rust
pub(super) const MAX_FILENAME_BYTES: usize = 255;
```

**変更後**:
```rust
const MAX_FILENAME_BYTES: usize = 255;
```

- [ ] **Step 3.7: 全テスト実行（既存 + Task 1 追加分の全 pass を確認）**

Run: `cargo test --all-targets --all-features`
Expected: 全 pass。特に以下が pass し続けること:
- Task 1 で追加した 6 件の境界・不変条件テスト
- 既存の `test_sidecar_name_*`（L48-147）
- 既存の memo save/load behavior テスト（L1080+ 周辺）
- `tests/integration_test.rs` の memo 関連テスト

万一どれかが fail したら、削除した分岐に runtime 上の意味があった（SidecarMemoName の不変条件が実際には満たされていなかった等）可能性。直ちに `git diff` を見直し、原因を特定すること。

- [ ] **Step 3.8: フォーマット・lint 全 pass**

Run: `./verify.sh`
Expected: fmt / clippy / cargo test / typecheck の全 pass。

特に `cargo clippy` で `MAX_FILENAME_BYTES` の visibility 縮小に伴う warnings (`dead_code` 等) が出ていないことを確認。

- [ ] **Step 3.9: コミット**

```bash
git add src/server/files/memo.rs src/server/files/memo_sidecar.rs
```

コミットメッセージを `/tmp/commit-msg-task3.txt` に書いて `git commit -F` で実行:

```
refactor: sidecar_name_too_long と到達不能な分岐を撤去

変更内容:
- src/server/files/memo.rs から sidecar_name_too_long 関数を削除
- save_route_memo / resolve_active_memo_path / choose_save_target の合計 4 か所の
  sidecar_usable / sidecar_too_long 分岐を撤去
- import から MAX_FILENAME_BYTES を削除
- src/server/files/memo_sidecar.rs の MAX_FILENAME_BYTES の visibility を pub(super) → private に縮小

変更理由:
- 2026-04-24 hardening 以降、SidecarMemoName が型として ≤ 255 bytes を構築時保証するため、
  これらの呼び出し側チェックは到達不能になっていた
- 設計書 (2026-04-25) で型を Single Source of Truth に格上げする方針を確定
- 死んだコードは読む人を確実に混乱させるため撤去する

影響範囲:
- src/server/files/memo.rs 約 30 行削減
- src/server/files/memo_sidecar.rs visibility 1 行のみ
- runtime behavior は変えない
- 既存テスト + Task 1 追加分の不変条件テストが全て pass し続けることで保証

テスト結果: ./verify.sh 全 pass（fmt / clippy / cargo test / typecheck）
```

その後 `rm /tmp/commit-msg-task3.txt`。

---

## Task 4: TODO.md の該当項目を完了マーク

**Files:**
- Modify: `docs/todo/TODO.md:81-85`
- Test: なし

- [ ] **Step 4.1: TODO.md の該当項目チェックボックスを完了に変更**

`docs/todo/TODO.md` の L81-85 を編集。

**変更前** (`docs/todo/TODO.md:81-85`):
```
- [ ] メモ sidecar 名生成の不変条件を `SidecarMemoName` に集約し、境界テストと受容リスクを補強
  - ファイル: `src/server/files/memo.rs`, `src/server/files/memo_sidecar.rs`, `src/server/files/tests.rs`, `docs/superpowers/specs/2026-04-24-memo-sidecar-name-hardening-design.md`
  - 現状: `SidecarMemoName` が 255 bytes 以下を保証するため `sidecar_name_too_long` 分岐は実質到達不能になっている。255 bytes ちょうど / 256 bytes 超過、UTF-8 境界直前、正規化済み超長名の組み合わせテストも薄い。64 bit hash 衝突と Windows 非 UTF-8 名の fallback 集約は受容リスクとして設計書に残っていない
  - 対応: `sidecar_name_too_long` を削除または型内部へ統合し、呼び出し側の legacy fallback 分岐を現実の契約に合わせる。境界テストを追加し、hash 衝突・非 UTF-8 fallback 集約を設計書の既知リスクとして明記する
  - 理由: sidecar 名生成の single source of truth を明確にし、将来のリファクタで長名・正規化・非 UTF-8 のセキュリティ境界が silent に変わることを防ぐ
```

**変更後**:
```
- [x] メモ sidecar 名生成の不変条件を `SidecarMemoName` に集約し、境界テストと受容リスクを補強
  - ファイル: `src/server/files/memo.rs`, `src/server/files/memo_sidecar.rs`, `src/server/files/tests.rs`, `docs/superpowers/specs/2026-04-24-memo-sidecar-name-hardening-design.md`
  - 現状: `SidecarMemoName` が 255 bytes 以下を保証するため `sidecar_name_too_long` 分岐は実質到達不能になっている。255 bytes ちょうど / 256 bytes 超過、UTF-8 境界直前、正規化済み超長名の組み合わせテストも薄い。64 bit hash 衝突と Windows 非 UTF-8 名の fallback 集約は受容リスクとして設計書に残っていない
  - 対応: `sidecar_name_too_long` を削除または型内部へ統合し、呼び出し側の legacy fallback 分岐を現実の契約に合わせる。境界テストを追加し、hash 衝突・非 UTF-8 fallback 集約を設計書の既知リスクとして明記する
  - 理由: sidecar 名生成の single source of truth を明確にし、将来のリファクタで長名・正規化・非 UTF-8 のセキュリティ境界が silent に変わることを防ぐ
```

（変更箇所は `- [ ]` → `- [x]` のみ）

- [ ] **Step 4.2: コミット**

```bash
git add docs/todo/TODO.md
```

コミットメッセージを `/tmp/commit-msg-task4.txt` に書いて `git commit -F` で実行:

```
docs: メモ sidecar 名生成不変条件集約 TODO を完了マーク

変更内容:
- docs/todo/TODO.md の該当 Medium Priority 項目のチェックボックスを完了に変更

変更理由:
- 設計書 2026-04-25-memo-sidecar-name-invariant-consolidation-design.md と
  実装コミット (test 追加 / doc 補強 / sidecar_name_too_long 撤去) で TODO の
  Acceptance Criteria を全て満たしたため

影響範囲:
- docs/todo/TODO.md のみ。runtime コード未変更

テスト結果: ドキュメント更新のため未実行
```

その後 `rm /tmp/commit-msg-task4.txt`。

---

## Final Verification

- [ ] **Step F1: 最終的に `./verify.sh` を全 pass で確認**

Run: `./verify.sh`
Expected: 全 pass。

- [ ] **Step F2: ブランチのコミットログを確認**

Run: `git log --oneline develop..HEAD`
Expected: 5 コミット（古い順に下）:
1. `docs: メモ sidecar 名生成不変条件集約の設計書を追加`（Pre-flight 前に既にコミット済み）
2. `test: SidecarMemoName の 255 バイト不変条件と境界テストを追加`
3. `docs: SidecarMemoName の不変条件を doc コメントに明記`
4. `refactor: sidecar_name_too_long と到達不能な分岐を撤去`
5. `docs: メモ sidecar 名生成不変条件集約 TODO を完了マーク`

各コミットが atomic で、それぞれ `./verify.sh` が pass する状態にあることが望ましい（途中コミットでの pass は Task 内 step で確認済み）。

- [ ] **Step F3: 最終 diff レビュー**

Run: `git diff develop..HEAD --stat`
Expected: 変更ファイル一覧:
- `docs/superpowers/specs/2026-04-25-memo-sidecar-name-invariant-consolidation-design.md` (new, ~129 lines)
- `docs/superpowers/plans/2026-04-25-memo-sidecar-name-invariant-consolidation.md` (new, このプラン)
- `docs/todo/TODO.md` (1 line changed)
- `src/server/files/memo.rs` (約 30 行削減)
- `src/server/files/memo_sidecar.rs` (約 15 行追加: doc コメント、visibility 縮小 1 行)
- `src/server/files/tests.rs` (約 100 行追加: 境界テスト + 不変条件テスト)

差分が想定外に大きい場合は何かが間違っている。`git diff` で該当箇所を確認すること。

---

## Acceptance Criteria（spec から転記）

実装完了の判定基準は spec の Acceptance Criteria に従う:

- [x] sidecar_name_too_long と関連する 3 か所の分岐が memo.rs から削除されている
  - 実際は 4 か所: save_route_memo / resolve_active_memo_path / choose_save_target (2 か所)
- [x] SidecarMemoName の型 doc と各 constructor doc に 255 bytes 以下保証が明記されている
- [x] 個別境界テスト 4 件 + 不変条件テーブル駆動テスト 1 件（実装上は UTF-8 / 非 UTF-8 で 2 件分割）が tests.rs に追加されている
- [x] 既存の test_sidecar_name_* および save / load behavior テストが全て pass する
- [x] ./verify.sh が pass する
- [x] Known Risks セクションが本設計書に含まれ、64 bit hash 衝突と Windows 非 UTF-8 fallback 集約が明文化されている
  - spec で完了済み (Pre-flight 前のコミット d91b81d)

実装担当者は Final Verification の Step F1-F3 を完了した時点で、spec の全 Acceptance Criteria が満たされていることを最終確認する。
