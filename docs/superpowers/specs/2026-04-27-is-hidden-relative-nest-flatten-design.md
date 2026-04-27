# `is_hidden_relative` ネスト平坦化リファクタ 設計書

- 作成日: 2026-04-27
- スコープ: `src/watcher/strategy.rs` の `is_hidden_relative` のネスト深度削減
- 元 TODO: `docs/todo/TODO.md` Medium Priority 「`is_hidden_relative` のネスト深度を 3 → 2 階層に削減」
- 関連 PR: 直前の watcher リファクタ（隠し判定ロジックだけが旧形状で残った）

## 背景と問題

`src/watcher/strategy.rs` の `is_hidden_relative`（L163-203, 41行）は、
ベースディレクトリからの相対パスに隠しコンポーネント（先頭`.`）が含まれるかを判定する関数。

```text
fn is_hidden_relative(path, base) -> bool {
  match path.strip_prefix(base) {
    Ok(relative) => check_dotfile(relative),
    Err(_) => {
      let canonical_path = match path.canonicalize() { ... };  // 失敗時 warn
      let canonical_base = match base.canonicalize() { ... };  // 失敗時 warn
      match canonical_path.strip_prefix(&canonical_base) {
        Ok(relative) => check_dotfile(relative),
        Err(_) => { warn!; true }
      }
    }
  }
}
```

### 問題点

1. **3段ネスト match** で読みづらい
2. **canonicalize 失敗時のフォールバック warn が 2 回（path / base）に分散** している
3. **直前の watcher リファクタで隠し判定だけが旧形状のまま残っている** と TODO が指摘

### スコープ外

- `is_within_base_dir`（L255-269）も類似構造（canonicalize fallback 持ち）だが、
  失敗時に `normalize_lexical_path` を使う点で挙動が異なるため、同じヘルパーへの集約は行わない。
  TODO のスコープも `is_hidden_relative` のみ。

## 設計

### 方針

`Option<PathBuf>` を返すヘルパー `try_strip_base(path, base)` を抽出し、
呼び出し側 `is_hidden_relative` を 1段 match に平坦化する。

戻り値型として TODO 提案の `Option<impl Iterator<Component>>` ではなく
`Option<PathBuf>` を採用する理由：

- Iterator 案は自己参照（canonicalize 結果を関数内で所有しつつ Iterator を返す形）になり、
  ライフタイム/借用が複雑
- PathBuf 案は所有権がシンプル、warn 集約も容易、コストは fail-back 経路のみで無視可能
- `.components()` の呼び出しは呼び出し側で行えば良く、抽象化の必要性は低い

### 新ヘルパー関数

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

### 呼び出し側（`is_hidden_relative`）

```rust
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

### Doc コメント保持

`is_hidden_relative` の既存 doc コメント（L155-162）は方針説明として有用なので保持する。
内容を新構造に合わせて軽く調整する：

```rust
/// ベースディレクトリからの相対パスに隠しコンポーネントが含まれるか判定する
///
/// ベースディレクトリ自体が`.`で始まるパスに含まれる場合でも
/// 正しく動作するよう、相対パス部分のみをチェックする。
///
/// ## Fail-safe動作
/// `try_strip_base` が `None` を返した場合は `true` を返し、
/// 安全側に倒す（隠しファイルとして扱い処理をスキップする）。
```

## ビフォー/アフター比較

| 項目 | Before | After |
|------|--------|-------|
| `is_hidden_relative` 行数 | 41 | 約13 |
| ネスト深度 | 3段 | 1段 |
| warn ログ種類 | 3 | 3（同一文言保持） |
| 抽出ヘルパー | なし | `try_strip_base` |
| 振る舞い | — | 完全互換 |

## warn メッセージ方針

3つの warn メッセージはすべて文言完全保持する。

- `"隠しファイル判定: パス正規化失敗（元パスで再試行）: {} ({})"` （path 側 canonicalize 失敗）
- `"隠しファイル判定: ベース正規化失敗（元パスで再試行）: {} ({})"` （base 側 canonicalize 失敗）
- `"隠しファイル判定: 相対パス算出不可（安全側で除外）: {}"` （最終 strip_prefix 失敗）

理由：
- 個人使用前提でもログ監視している場合の互換性
- canonicalize 失敗の主体（path 側 / base 側）はデバッグ情報として有意義

「2回重複」と TODO が指摘するのは「path も base も両方失敗した場合」だが、
これは異なる主体の失敗なので情報量は分離されているべき。文言は変えない。

## テスト方針

### 既存テスト（保持）

`src/watcher/strategy.rs` 内の以下3本はそのまま通過する想定：

- `test_隠しファイル判定_相対パスのみチェック`（L501-510）
- `test_隠しファイル判定_通常のベースディレクトリ`（L512-520）
- `test_隠しファイル判定_相対パス算出不可時は安全側で除外`（L522-528）

### 新規追加テスト（`try_strip_base` 単体）

ヘルパー単体の境界を明確にするため、以下3本を追加：

| テスト名 | シナリオ | 期待 |
|---------|----------|------|
| `test_try_strip_base_strip_prefix直接成功` | base 配下の通常パス | `Some(相対 PathBuf)` |
| `test_try_strip_base_canonicalize経由成功` | base が symlink 経由など非正規化済み | `Some(相対 PathBuf)` |
| `test_try_strip_base_完全失敗でNone` | 無関係パス | `None` |

`canonicalize 経由成功` の作り方：tempdir 内に subdir を作り、`subdir/../subdir/file.md` のような
非正規化パスでアクセスする、または symlink を作成する（プラットフォーム依存に注意し、
Unix のみで symlink テストを書くか、`subdir/../` 形式を使う）。

## リスクと受容

- **挙動変更リスク**: なし。ヘルパー抽出のみで分岐セマンティクスは完全保持。
- **観測性リスク**: warn メッセージ文言完全保持で観測性も保持。
- **テスト coverage リスク**: 既存3本に加え、ヘルパー単体3本で境界を明示。

## 完了条件（Done Criteria）

- [ ] `try_strip_base` ヘルパーが `src/watcher/strategy.rs` に追加されている
- [ ] `is_hidden_relative` のネスト深度が 1 段になっている（外側 match のみ）
- [ ] 既存テスト3本が通過する
- [ ] 新規テスト3本が追加され通過する
- [ ] `cargo fmt --all -- --check` が通る
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` が通る
- [ ] `cargo test --all-targets --all-features` が全通過する

## スペック自己レビュー結果

- プレースホルダ: なし
- 内部矛盾: なし（warn 文言・テスト方針・スコープが整合）
- スコープ: 単一実装計画で完結する規模（約30行のリファクタ + テスト3本）
- 曖昧さ: `canonicalize 経由成功` テストの作り方を 2 案（`../` 形式 / symlink）併記したが、
  実装時に Unix 限定 symlink を避ける方針で「`subdir/../subdir/file.md` 形式」を採用する
