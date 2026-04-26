# is_hidden_relative ネスト平坦化リファクタ Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `src/watcher/strategy.rs` の `is_hidden_relative` のネスト深度を 3 段から 1 段に平坦化し、補助ヘルパー `try_strip_base` を抽出する。

**Architecture:** `Option<PathBuf>` を返す `try_strip_base(path, base)` ヘルパーを抽出。canonicalize fallback ロジックをヘルパー側に集約し、`is_hidden_relative` 本体は 1 段 match のみで構成する。warn メッセージ文言は完全保持し、振る舞い互換を維持。

**Tech Stack:** Rust (edition 2021), std::path, tracing, tempfile（既存テスト依存）

**設計書:** `docs/superpowers/specs/2026-04-27-is-hidden-relative-nest-flatten-design.md`

**ブランチ:** `refactor/is-hidden-relative-nest-flatten`（設計書コミット済み）

---

## ファイル構造

| 種別 | パス | 役割 |
|------|------|------|
| Modify | `src/watcher/strategy.rs` | `try_strip_base` 追加、`is_hidden_relative` 平坦化、tests モジュールに 3 本追加 |
| Modify | `docs/todo/TODO.md` | 該当項目の `[ ]` → `[x]` |

すべての変更は単一ファイル `src/watcher/strategy.rs`（プラス完了マーク）に閉じる。新規ファイルなし。

---

### Task 1: `try_strip_base` ヘルパーと単体テスト 3 本を追加（TDD: red → green）

**Files:**
- Modify: `src/watcher/strategy.rs:278`（tests モジュールの `use super::{...}`）
- Modify: `src/watcher/strategy.rs`（tests モジュール末尾に 3 テスト追加）
- Modify: `src/watcher/strategy.rs`（`is_hidden_relative` の閉じ `}` 直後にヘルパー挿入）

- [ ] **Step 1: tests モジュールの use 文に `try_strip_base` を追加**

`src/watcher/strategy.rs:278-281`:
```rust
    use super::{
        is_content_change_event, is_hidden_relative, is_target_file, is_within_base_dir,
        WatchStrategy,
    };
```

を以下に変更:

```rust
    use super::{
        is_content_change_event, is_hidden_relative, is_target_file, is_within_base_dir,
        try_strip_base, WatchStrategy,
    };
```

- [ ] **Step 2: tests モジュール末尾（最後の `}` の直前、L542 付近）に新規テスト 3 本を追加**

`test_is_within_base_dir_ベース外パスはfalse` （L537-542）の閉じ `}` の**直後**、tests モジュール全体の閉じ `}` の**直前**に以下を挿入:

```rust
    #[test]
    fn test_try_strip_base_strip_prefix直接成功() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let sub = canonical_dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("guide.md");
        std::fs::write(&file_path, "# guide").unwrap();

        let result = try_strip_base(&file_path, &canonical_dir);

        assert_eq!(result, Some(PathBuf::from("sub/guide.md")));
    }

    #[test]
    fn test_try_strip_base_canonicalize経由成功() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let sub = canonical_dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("guide.md");
        std::fs::write(&file_path, "# guide").unwrap();

        // base を "sub/.." の非正規化形にして直接 strip_prefix を失敗させ、
        // canonicalize fallback 経路で成功することを確認する
        let non_normalized_base = sub.join("..");

        let result = try_strip_base(&file_path, &non_normalized_base);

        assert_eq!(result, Some(PathBuf::from("sub/guide.md")));
    }

    #[test]
    fn test_try_strip_base_完全失敗でNone() {
        let base = Path::new("/nonexistent/base/dir");
        let unrelated = Path::new("/completely/different/path/file.md");

        let result = try_strip_base(unrelated, base);

        assert!(result.is_none());
    }
```

- [ ] **Step 3: ビルドして失敗を確認（red phase）**

Run:
```bash
cargo build --tests 2>&1 | tail -20
```

Expected: コンパイルエラー — `cannot find function 'try_strip_base' in this scope` または `unresolved import 'super::try_strip_base'`（関数未定義のため）

- [ ] **Step 4: `try_strip_base` ヘルパーを実装**

`src/watcher/strategy.rs` の `is_hidden_relative` 関数の閉じ `}`（現状 L203）の**直後**、空行を1行はさんで `is_target_file`（現状 L205）の**直前**に以下を挿入:

```rust
/// path から base を取り除いた相対 PathBuf を返す。
///
/// `strip_prefix` が直接成功すれば即座に返す。失敗時は path/base を
/// canonicalize して再試行する。canonicalize に失敗した側は元パスを
/// そのまま使い、最終 `strip_prefix` も失敗した場合は `None` を返す。
///
/// 失敗経路では `tracing::warn!` でログを残す。
fn try_strip_base(path: &Path, base: &Path) -> Option<PathBuf> {
    if let Ok(rel) = path.strip_prefix(base) {
        return Some(rel.to_path_buf());
    }
    let canonical_path = path.canonicalize().unwrap_or_else(|e| {
        tracing::warn!(
            "[markdown-view] 隠しファイル判定: パス正規化失敗（元パスで再試行）: {} ({})",
            sanitize_path_for_logging(path, base),
            e
        );
        path.to_path_buf()
    });
    let canonical_base = base.canonicalize().unwrap_or_else(|e| {
        tracing::warn!(
            "[markdown-view] 隠しファイル判定: ベース正規化失敗（元パスで再試行）: {} ({})",
            base.display(),
            e
        );
        base.to_path_buf()
    });
    canonical_path
        .strip_prefix(&canonical_base)
        .ok()
        .map(Path::to_path_buf)
}
```

- [ ] **Step 5: 新規 3 テストが pass することを確認（green phase）**

Run:
```bash
cargo test watcher::strategy::tests::test_try_strip_base 2>&1 | tail -10
```

Expected:
```
running 3 tests
test watcher::strategy::tests::test_try_strip_base_strip_prefix直接成功 ... ok
test watcher::strategy::tests::test_try_strip_base_canonicalize経由成功 ... ok
test watcher::strategy::tests::test_try_strip_base_完全失敗でNone ... ok

test result: ok. 3 passed; 0 failed
```

- [ ] **Step 6: 既存テストへの影響がないことを確認**

Run:
```bash
cargo test watcher::strategy::tests 2>&1 | tail -10
```

Expected: 既存テスト全通過 + 新規 3 テスト pass。`failed` が `0` であること。

- [ ] **Step 7: コミット**

```bash
git add src/watcher/strategy.rs
git commit -m "feat: try_strip_base ヘルパーと単体テスト 3 本を追加" -m "変更内容:
- src/watcher/strategy.rs に try_strip_base(path, base) -> Option<PathBuf> を追加
- 単体テスト 3 本（strip_prefix 直接成功 / canonicalize 経由成功 / 完全失敗で None）を tests モジュールに追加

変更理由:
- is_hidden_relative のネスト平坦化（次タスク）の前段として、相対パス算出ロジックをヘルパーに集約
- 設計書: docs/superpowers/specs/2026-04-27-is-hidden-relative-nest-flatten-design.md

影響範囲:
- 既存呼び出し側（is_hidden_relative）はまだ未使用のため挙動変更なし
- 新規ヘルパー単体の境界が単体テストで明示される

テスト結果: 新規 3 件 pass、既存テスト全通過"
```

---

### Task 2: `is_hidden_relative` を `try_strip_base` 経由に書き換え

**Files:**
- Modify: `src/watcher/strategy.rs:155-203`

- [ ] **Step 1: 既存 3 本のテストが現状で通ることを再確認（baseline）**

Run:
```bash
cargo test watcher::strategy::tests::test_隠しファイル判定 2>&1 | tail -10
```

Expected:
```
running 3 tests
test watcher::strategy::tests::test_隠しファイル判定_相対パスのみチェック ... ok
test watcher::strategy::tests::test_隠しファイル判定_通常のベースディレクトリ ... ok
test watcher::strategy::tests::test_隠しファイル判定_相対パス算出不可時は安全側で除外 ... ok

test result: ok. 3 passed; 0 failed
```

- [ ] **Step 2: `is_hidden_relative` を平坦化版に置換**

`src/watcher/strategy.rs:155-203` の以下のブロック:

```rust
/// ベースディレクトリからの相対パスに隠しコンポーネントが含まれるか判定する
///
/// ベースディレクトリ自体が`.`で始まるパスに含まれる場合でも
/// 正しく動作するよう、相対パス部分のみをチェックする。
///
/// ## Fail-safe動作
/// `strip_prefix`とcanonicalizeの両方に失敗した場合は`true`を返し、
/// 安全側に倒す（隠しファイルとして扱い処理をスキップする）。
fn is_hidden_relative(path: &Path, base: &Path) -> bool {
    match path.strip_prefix(base) {
        Ok(relative) => relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
        Err(_) => {
            let canonical_path = match path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: パス正規化失敗（元パスで再試行）: {} ({})",
                        sanitize_path_for_logging(path, base), e
                    );
                    path.to_path_buf()
                }
            };
            let canonical_base = match base.canonicalize() {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: ベース正規化失敗（元パスで再試行）: {} ({})",
                        base.display(), e
                    );
                    base.to_path_buf()
                }
            };
            match canonical_path.strip_prefix(&canonical_base) {
                Ok(relative) => relative
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
                Err(_) => {
                    tracing::warn!(
                        "[markdown-view] 隠しファイル判定: 相対パス算出不可（安全側で除外）: {}",
                        sanitize_path_for_logging(path, base)
                    );
                    true
                }
            }
        }
    }
}
```

を以下に置換:

```rust
/// ベースディレクトリからの相対パスに隠しコンポーネントが含まれるか判定する
///
/// ベースディレクトリ自体が`.`で始まるパスに含まれる場合でも
/// 正しく動作するよう、相対パス部分のみをチェックする。
///
/// ## Fail-safe動作
/// `try_strip_base` が `None` を返した場合は `true` を返し、
/// 安全側に倒す（隠しファイルとして扱い処理をスキップする）。
fn is_hidden_relative(path: &Path, base: &Path) -> bool {
    match try_strip_base(path, base) {
        Some(relative) => relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
        None => {
            tracing::warn!(
                "[markdown-view] 隠しファイル判定: 相対パス算出不可（安全側で除外）: {}",
                sanitize_path_for_logging(path, base)
            );
            true
        }
    }
}
```

- [ ] **Step 3: 既存 3 本のテストが引き続き通ることを確認（互換性確認）**

Run:
```bash
cargo test watcher::strategy::tests::test_隠しファイル判定 2>&1 | tail -10
```

Expected: Step 1 と同じ — 3 件 pass。**1 件でも fail した場合は振る舞い互換が崩れているのでロールバックして調査**。

- [ ] **Step 4: watcher::strategy 配下の全テストを通す**

Run:
```bash
cargo test watcher::strategy::tests 2>&1 | tail -15
```

Expected: 全 pass（既存 + 新規 try_strip_base 3 本）。`failed` が `0` であること。

- [ ] **Step 5: コミット**

```bash
git add src/watcher/strategy.rs
git commit -m "refactor: is_hidden_relative のネスト深度を 3 段から 1 段へ平坦化" -m "変更内容:
- src/watcher/strategy.rs:163-203 を try_strip_base 経由の実装に置換
- doc コメントの Fail-safe セクションを新フローに合わせて更新

変更理由:
- docs/todo/TODO.md Medium「is_hidden_relative のネスト深度を 3 → 2 階層に削減」
- 直前 watcher リファクタで隠し判定だけが旧形状で残っていたものを整理
- 設計書: docs/superpowers/specs/2026-04-27-is-hidden-relative-nest-flatten-design.md

影響範囲:
- 振る舞い完全互換（既存 3 本のテスト全通過）
- warn メッセージ文言完全保持
- ネスト深度 3 段 → 1 段、行数 41 → 約 13

テスト結果: 既存 3 件 pass、try_strip_base 単体 3 件 pass"
```

---

### Task 3: 全体検証と TODO 完了マーク

**Files:**
- Modify: `docs/todo/TODO.md:51`

- [ ] **Step 1: cargo fmt 確認**

Run:
```bash
cargo fmt --all -- --check
```

Expected: 終了コード 0、無出力。

差分が出た場合のみ:
```bash
cargo fmt --all
git add -u
git commit -m "chore: cargo fmt 適用"
```

- [ ] **Step 2: cargo clippy 確認**

Run:
```bash
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -20
```

Expected: 終了コード 0、warning なし、`error` 行なし。

- [ ] **Step 3: 全テスト実行**

Run:
```bash
cargo test --all-targets --all-features 2>&1 | tail -30
```

Expected: 全テスト pass。すべての `test result: ok` が `0 failed` で終わる。

- [ ] **Step 4: E2E 型チェック（CLAUDE.md verify.sh と整合させる）**

Run:
```bash
npm run typecheck 2>&1 | tail -10
```

Expected: 終了コード 0、型エラーなし。

> 注: 今回のリファクタは Rust のみで TypeScript への影響はないが、`./verify.sh` 相当の最低限ゲートを通す。

- [ ] **Step 5: TODO.md の該当項目を完了マーク**

`docs/todo/TODO.md:51` の以下の行:

```markdown
- [ ] `is_hidden_relative` のネスト深度を 3 → 2 階層に削減
```

を以下に変更:

```markdown
- [x] `is_hidden_relative` のネスト深度を 3 → 2 階層に削減
```

- [ ] **Step 6: 完了マークをコミット**

```bash
git add docs/todo/TODO.md
git commit -m "chore: TODO.md の is_hidden_relative ネスト平坦化項目を完了に更新" -m "変更内容:
- docs/todo/TODO.md L51 の該当項目を [ ] → [x]

変更理由:
- リファクタ実装完了（refactor/is-hidden-relative-nest-flatten ブランチで完了）

影響範囲:
- ドキュメントのみ

テスト結果: 該当なし"
```

---

## Self-Review

### Spec coverage

| 設計書要件 | 対応タスク |
|------------|----------|
| `try_strip_base` ヘルパー追加 | Task 1 Step 4 |
| 戻り値型 `Option<PathBuf>` | Task 1 Step 4（実装） |
| `is_hidden_relative` 1 段 match 化 | Task 2 Step 2 |
| doc コメント Fail-safe セクション更新 | Task 2 Step 2 |
| 既存 3 本テスト保持確認 | Task 2 Step 1, 3 |
| `try_strip_base` 単体テスト strip_prefix 直接成功 | Task 1 Step 2 |
| `try_strip_base` 単体テスト canonicalize 経由成功 | Task 1 Step 2 |
| `try_strip_base` 単体テスト 完全失敗で None | Task 1 Step 2 |
| warn メッセージ文言完全保持 | Task 1 Step 4 + Task 2 Step 2（既存メッセージそのまま） |
| スコープ外 `is_within_base_dir` 不変 | （触らない、タスクなし） |
| `cargo fmt` / `clippy` / `test` ゲート | Task 3 Step 1-3 |
| TODO 項目完了マーク | Task 3 Step 5 |

カバレッジ漏れなし。

### Placeholder scan

- TBD / TODO / implement later: なし
- 「edge case を追加」「適切なエラーハンドリング」など曖昧な指示: なし
- すべての Step がコードブロック・実行コマンド・ファイル位置のいずれかを具体的に提示: ✓
- 「Task N と同様」のような自己参照: なし（コード再掲）

### Type consistency

- `try_strip_base(path: &Path, base: &Path) -> Option<PathBuf>` のシグネチャが Task 1 Step 2（テスト）/ Task 1 Step 4（実装）/ Task 2 Step 2（呼び出し側）で完全一致 ✓
- warn メッセージ文言（3 種）が Task 1 Step 4（新規）と既存コード（Task 2 Step 2 で保持される側）で完全一致 ✓
- doc コメントの Fail-safe セクションは旧言及（`strip_prefix`とcanonicalize）から新言及（`try_strip_base`）へ更新 — 整合 ✓

問題なし。
