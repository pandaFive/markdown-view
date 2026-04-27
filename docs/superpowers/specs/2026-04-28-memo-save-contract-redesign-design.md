# memo 保存契約の再設計とテスト基盤刷新

- 対象: `src/server/files/memo.rs` / `src/server/files/tests.rs` / `src/server/state.rs`
- 関連 TODO 消化: High-1（memo permission/fallback 契約再定義）/ High-2（`test_save_route_memo_*` 環境依存排除）/ Process-1（memo 保存仕様の設計ノート）/ Process-3（監視系テスト共通ユーティリティ・memo 経路分のみ）
- 関連プラン: `docs/superpowers/plans/2026-04-25-codebase-audit-task-proposals.md`
- 着手前提: `develop` で `./verify.sh` が通過していること（2026-04-27 時点で確認済み）

## 1. 概要

### 1.1 背景

監査時点（2026-04-25）のテスト実行で `save_route_memo` 系ユニットが 6 件失敗しており、原因は「実装と テストどちらが正か」の判断軸が曖昧なまま、`SidecarFallback` enum を中心とする多段 fallback ロジックが膨張していることだった。
さらに `test_save_route_memo_*` 群は `chmod 0o555` で permission denied を誘発する設計で、root 実行下では chmod が効かず偽陽性 pass となる環境依存があった。
PR #93 / コミット `1e2b568` 等で IO エラー透過の統合テストは追加され、現状の `develop` では `./verify.sh` は通過するものの、契約の曖昧さと環境依存テストの構造的問題は残置されている。

### 1.2 目的

1. memo 保存・読み込みの **観察可能な挙動を 1 つの仕様として固定** する
2. 古い `SidecarFallback` 多段分岐を廃止し、契約をシンプルに再設計する
3. テストを `chmod` 依存から脱却させ、root / non-root / コンテナで決定論的に同一結果になるようにする
4. 上記を実現する DI 抽象（`trait MemoFs`）を memo モジュールに導入する
5. 共通テストヘルパーを集約し、Process-3 を memo 経路の範囲で消化する

### 1.3 スコープ

**含む**:
- `src/server/files/memo.rs` の契約再設計（fallback 廃止、暗黙移行への一本化）
- `src/server/files/memo_fs.rs` の新設（`trait MemoFs` ＋ `TokioMemoFs`）
- `src/server/state.rs` への DI フィールド追加
- `src/server/files/test_support.rs` の新設（`MockMemoFs` ＋ `TempWorkspace` ＋ `make_test_app_state`）
- `src/server/files/tests.rs` の memo 関連テスト 16+ 件の移行と新規テスト追加

**含まない（スコープ外、別 spec）**:
- `watcher` / `content` / `search` / `catalog` 系の DI 化（trait MemoFs パターンの横展開）
- TODO Process-2「テスト失敗分類ラベル運用」
- 起動時の一括移行コマンド（`--migrate-memos` 等）
- E2E テストの memo 関連シナリオ（本 spec はバックエンド契約・単体テストのみ）

## 2. 用語と前提

- **sidecar**: 同階層の `.<filename>.memo.md` 形式のメモファイル。`SidecarMemoName::from_file_name` で 255 byte 以内が保証される。
- **compat_sidecar**: 旧形式 sidecar（`\` 区切り名を含む過去命名）。`SidecarMemoName::compat_from_file_name` で生成される。
- **legacy**: `<base_dir>/.markdown-view/memos/<relative_path>` にある集約ディレクトリ形式メモ。
- **safe**: `ensure_safe_memo_path` を通過したパス。`base_dir` 配下にあり、シンボリックリンク要素を含まない。
- **MAX_FILE_SIZE**: 既存定数（10 MB）。
- **base_dir**: `AppMode::base_dir()` が返す canonicalize 済みパス。

## 3. 設計判断のサマリー

ブレストで合意した方針:

| ID | 判断 | 採用 |
|----|------|------|
| Q1 | 契約の方向性 | **B**: 現挙動を改めて契約を再設計（観察可能な挙動の破壊を許容） |
| Q2 | fallback 構造 | **A**: 完全廃止 / sidecar 一本化 |
| Q3 | 読み込み・移行 | **A**: 読み込み 3 段（sidecar → compat_sidecar → legacy）、保存時に他形式 cleanup（暗黙移行） |
| Q4 | DI 戦略 | **A**: `trait MemoFs` を memo モジュール内に閉じる |
| Q5 | 空保存削除挙動 | **D**: sidecar は厳格（NotFound のみ緩和）、legacy/compat は緩和 |
| Q6 | 既存テスト移行 | **B**: 既存名・構造保持、内部のみ DI 化＋新仕様適合 |
| Q7 | Process-3 範囲 | **B**: 本 spec で完全消化、共通ヘルパー集を整備 |

実装アーキテクチャは「案2: AppState 経由 DI ＋ `async-trait` クレート追加」を採用。

## 4. 契約仕様

### 4.1 保存契約 `save_route_memo`

| 入力 | 期待挙動 |
|------|---------|
| `raw.trim()` が空 | 空保存ルート（4.1.2 へ） |
| `raw.len() > MAX_FILE_SIZE` (10 MB) | `413 PAYLOAD_TOO_LARGE` |
| 通常（非空） | 4.1.1 のフロー |

#### 4.1.1 通常保存フロー（非空）

1. `ensure_safe_memo_path(sidecar)` が失敗 → `403 FORBIDDEN`
2. `MemoFs::create_dir_all(sidecar.parent())` 失敗 → `500 INTERNAL_SERVER_ERROR`
3. `MemoFs::write(sidecar, raw)` 失敗 → `500 INTERNAL_SERVER_ERROR`
4. 書き込み成功後、旧形式 cleanup を best-effort で実行:
   - `compat_sidecar` が存在し safe かつ `sidecar` と別パスなら `MemoFs::remove_file` を試行（失敗は warn のみ、200 を返す）
   - `legacy` が存在し safe なら `MemoFs::remove_file` を試行（失敗は warn のみ、200 を返す）
5. `200 OK` ＋ `MemoResponse::from_raw(raw, ...)`

`PermissionDenied` / `NameTooLong` / `DiskFull` などのエラー種別による分岐は **行わない**。あらゆる IO エラーは 500 として透過する。

#### 4.1.2 空保存削除契約（`raw.trim()` が空）

1. `ensure_safe_memo_path(sidecar)` 失敗 → `403 FORBIDDEN`
2. `MemoFs::remove_file(sidecar)` を試行:
   - `Ok(())` → 続行
   - `Err(NotFound)` → 続行（**冪等性のため warn なし、ログなし**）
   - `Err(他 IO エラー)` → `500 INTERNAL_SERVER_ERROR`（**sidecar 削除は厳格**）
3. `compat_sidecar` が存在し safe かつ `sidecar` と別パスなら `MemoFs::remove_file` を試行:
   - `Ok(())` / `Err(NotFound)` → 続行
   - `Err(他 IO エラー)` → `500 INTERNAL_SERVER_ERROR`
4. `legacy` が存在し safe なら `MemoFs::remove_file` を試行:
   - `Ok(())` / `Err(NotFound)` → 続行
   - `Err(他 IO エラー)` → `500 INTERNAL_SERVER_ERROR`
5. `200 OK` ＋ `MemoResponse::empty(...)`

### 4.2 読み込み契約 `load_route_memo`

優先順位順に 1 段ずつチェックし、最初に見つかったファイルを返す:

1. `sidecar` が safe で `try_exists() == true` → そのファイル
2. `compat_sidecar` が safe で `try_exists() == true` → そのファイル
3. `legacy` が safe で `try_exists() == true` → そのファイル
4. いずれも該当しない → `MemoResponse::empty(...)`

各段階の `try_exists()` で IO エラーが出た場合 → `500 INTERNAL_SERVER_ERROR`。
primary `sidecar` が unsafe な path（シンボリックリンクを含む）の場合は `403 FORBIDDEN`。
移行用候補である `compat_sidecar` / `legacy` が unsafe な場合は warn ログを残してスキップ（次の優先順位へ）。

ファイルが見つかった場合の読み込み処理:
- `MemoFs::metadata` の長さが `MAX_FILE_SIZE` を超える → `413 PAYLOAD_TOO_LARGE`
- `MemoFs::read_with_limit` で実読み取り量を制限し、TOCTOU 超過なら `413 PAYLOAD_TOO_LARGE`
- `MemoFs::read_with_limit` 後、再度長さチェック（防御的な二段階確認）し、超過なら `413 PAYLOAD_TOO_LARGE`
- UTF-8 デコード失敗 → `422 UNPROCESSABLE_ENTITY`
- IO エラー → `500 INTERNAL_SERVER_ERROR`

### 4.3 mode 別差分

| 観点 | single-file | directory |
|------|------------|-----------|
| sidecar 物理パス | 対象ファイル同階層 | 対象ファイル同階層 |
| compat_sidecar 物理パス | 同上 | 同上 |
| legacy 物理パス | `<base>/.markdown-view/memos/<filename>` | `<base>/.markdown-view/memos/<rel>` |
| 保存先 | sidecar **のみ** | sidecar **のみ** |
| 失敗時 fallback | **なし** | **なし** |

mode 別で観察可能な挙動の差分は **保存先・読み込み先の物理パスのみ**。フローや fallback ルールに mode 差分はない。

### 4.4 [Process-1] fallback 優先順位表

| 操作 | sidecar | compat_sidecar | legacy |
|------|---------|----------------|--------|
| 保存（非空） | **書く** | 触らない（保存後 cleanup） | 触らない（保存後 cleanup） |
| 保存（空） | 削除（NotFound 緩和） | 削除（best-effort、warn 緩和） | 削除（best-effort、warn 緩和） |
| 読み込み | 1 番目 | 2 番目 | 3 番目 |
| unsafe path 検出時 | エラー（保存・読み込み共に拒否） | スキップ（次の優先順位へ） | スキップ（次の優先順位へ） |

### 4.5 [Process-1] cleanup 失敗時挙動表

`remove_file` 結果:

| 削除対象 | 結果 | HTTP | ログ |
|---------|------|------|------|
| sidecar | Ok | 200 | なし |
| sidecar | NotFound | 200 | **なし**（冪等性保証） |
| sidecar | PermissionDenied / その他 IO | **500** | warn |
| compat_sidecar | Ok / NotFound | 200 | なし |
| compat_sidecar | PermissionDenied / その他 IO | 200 | warn |
| legacy | Ok / NotFound | 200 | なし |
| legacy | PermissionDenied / その他 IO | 200 | warn |

`try_exists` が IO エラーを返した場合（cleanup 文脈、4.1.1 の旧形式 cleanup および 4.1.2 の compat / legacy 削除前のチェック）:

| 対象 | `try_exists` 結果 | HTTP | ログ | 備考 |
|------|------------------|------|------|------|
| compat_sidecar | IO エラー | 200 | warn | best-effort なので「不在扱い」で続行 |
| legacy | IO エラー | 200 | warn | 同上 |

**通常保存（4.1.1）と空保存（4.1.2）における sidecar 自身の `try_exists` チェックは行わない**（保存 = 直接 write、空保存 = 直接 remove_file で NotFound 緩和するため）。読み込み契約（4.2）の `try_exists` IO エラー扱い（500）はこの cleanup 表とは独立した別経路。

### 4.6 [Process-1] 互換性破壊と移行

#### 観察可能な挙動の破壊

| 旧挙動 | 新挙動 |
|-------|-------|
| 書込不可ディレクトリで保存 → legacy へ自動 fallback、200 を返す | 同条件で 500 |
| 既存 compat が書込不可で sidecar 作成不可 → legacy へ fallback、200 | 同条件で 500 |
| `errno 36 (NameTooLong)` で sidecar 失敗 → legacy へ fallback | `SidecarMemoName` で 255 byte 保証されているので発生しない想定。発生したら 500 として透過 |
| 単一ファイルモードでは permission denied 時 fallback しない | mode 別差分なしに統一（500 で透過） |

#### データ互換性（破壊しない）

- 既存の sidecar / compat_sidecar / legacy ファイルはそのまま読み込み続けられる
- 一度でも保存すると、その瞬間 sidecar に書かれ、旧形式は cleanup される
- cleanup 失敗（permission denied 等）は warn のみで 200 を返す。次回保存時に再試行されるが、再試行も失敗するなら旧ファイルは残る。読み込み時は sidecar が優先されるので UI 上の表示は新しい

#### CLI / 外部 API

CLI オプション・HTTP API のシグネチャは変更しない。

## 5. アーキテクチャ

### 5.1 モジュール構成

```
src/server/files/
├── memo.rs           （既存・縮小）
│   ├── load_route_memo / save_route_memo
│   ├── memo_paths_for_target / sidecar_*_for_target / legacy_*_for_target
│   ├── ensure_safe_memo_path / first_symlink_component
│   └── （削除: SidecarFallback, SaveTarget, choose_save_target,
│              save_memo_to_fallback, save_memo_to_existing_compat*,
│              save_memo_to_legacy, sidecar_fallback_for_error,
│              is_name_too_long_error,
│              cleanup_legacy_memo_if_safe / cleanup_compat_sidecar_if_safe
│              の strict / non-strict 二分岐）
│
├── memo_fs.rs        （NEW）
│   ├── trait MemoFs (Send + Sync + Debug) #[async_trait]
│   └── struct TokioMemoFs (zero-sized, Default)
│
├── test_support.rs   （NEW, pub(crate)）
│   ├── MockMemoFs (IO エラー注入 + 書き込み履歴)
│   ├── make_test_app_state(mode, memo_fs) -> AppState
│   └── TempWorkspace（tempdir + Drop ガード）
│
└── tests.rs          （memo 関連テストを移行・追加）
```

### 5.2 trait `MemoFs` API

```rust
use std::fs::Metadata;
use std::path::Path;

use async_trait::async_trait;

#[async_trait]
pub(crate) trait MemoFs: Send + Sync + std::fmt::Debug {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool>;
    async fn metadata(&self, path: &Path) -> std::io::Result<Metadata>;
    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError>;
    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()>;
    async fn write(&self, path: &Path, content: &[u8]) -> std::io::Result<()>;
    async fn remove_file(&self, path: &Path) -> std::io::Result<()>;
}
```

`MemoReadError`:

| variant | 意味 | HTTP 変換 |
|---------|------|-----------|
| `Open(std::io::Error)` | ファイル open 失敗 | `500 INTERNAL_SERVER_ERROR` |
| `Read(std::io::Error)` | 読み取り中 IO エラー | `500 INTERNAL_SERVER_ERROR` |
| `TooLarge` | 実読み取り量が上限超過 | `413 PAYLOAD_TOO_LARGE` |

設計ポイント:
- `Send + Sync` 必須（`Arc` 共有・tokio タスク間移動のため）
- `Debug` 必須（`AppState` の `#[derive(Debug)]` を保つため）
- 6 メソッドで全 `tokio::fs::*` 呼び出しをカバー
- TOCTOU 対策の最終防衛として `read_with_limit` が実読み取り量を制限する
- 呼び出し側は `metadata` と読み取り後長さ確認で HTTP 契約を決定する
- `NotFound` 等の特殊エラーは呼び出し側で吸収。trait は素直にエラーを透過する

### 5.3 `TokioMemoFs`

```rust
#[derive(Debug, Default)]
pub(crate) struct TokioMemoFs;

#[async_trait]
impl MemoFs for TokioMemoFs {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool> {
        tokio::fs::try_exists(path).await
    }
    async fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        tokio::fs::metadata(path).await
    }
    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(MemoReadError::Open)?;
        read_bytes_with_limit(file).await.map_err(|error| match error {
            ReadMarkdownError::Io(error) => MemoReadError::Read(error),
            ReadMarkdownError::TooLarge => MemoReadError::TooLarge,
            ReadMarkdownError::NotUtf8 => {
                unreachable!("read_bytes_with_limit does not validate UTF-8")
            }
        })
    }
    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        tokio::fs::create_dir_all(path).await
    }
    async fn write(&self, path: &Path, content: &[u8]) -> std::io::Result<()> {
        tokio::fs::write(path, content).await
    }
    async fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        tokio::fs::remove_file(path).await
    }
}
```

zero-sized struct のため `Arc` 化のオーバーヘッドはカウンタ分のみ。

### 5.4 `AppState` 変更

```rust
#[derive(Debug, Clone)]
pub struct AppState {
    mode: AppMode,
    sender: broadcast::Sender<BroadcastMessage>,
    syntax_theme: String,
    dark: bool,
    memo_fs: Arc<dyn MemoFs>,            // ← 追加
}

impl AppState {
    pub fn new(mode: AppMode, sender: ..., theme: String, dark: bool) -> Self {
        Self {
            mode, sender, syntax_theme: theme, dark,
            memo_fs: Arc::new(TokioMemoFs),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_memo_fs(mut self, memo_fs: Arc<dyn MemoFs>) -> Self {
        self.memo_fs = memo_fs;
        self
    }

    pub(crate) fn memo_fs(&self) -> &Arc<dyn MemoFs> {
        &self.memo_fs
    }
}
```

`Arc<dyn MemoFs>` を `Clone` させるため、`AppState` の `Clone` は浅いコピーで済む（既存の `broadcast::Sender` と同じ扱い）。

### 5.5 呼び出し側変更

`save_route_memo` / `load_route_memo` のシグネチャは **変えない**。
内部で `state.memo_fs()` を取得し、既存の `tokio::fs::*` を全て trait メソッド呼び出しに置換する。

`memo.rs` 内の `tokio::fs::*` 直呼び箇所は **約 12 箇所**（`try_exists` x 数箇所、`metadata`、`create_dir_all`、`write`、`remove_file`）。これらを順次 `fs.try_exists(path).await` 形式に置換する。

### 5.6 依存追加

`Cargo.toml` の `[dependencies]` に以下を追加:

```toml
async-trait = "0.1"
```

## 6. テスト戦略

### 6.1 `MockMemoFs` API

```rust
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::sync::Mutex as AsyncMutex;
use async_trait::async_trait;
use super::memo_fs::{MemoFs, TokioMemoFs};

#[derive(Debug, Default)]
pub(crate) struct MockMemoFs {
    inner: TokioMemoFs,
    failures: Mutex<HashMap<(Op, PathBuf), io::ErrorKind>>,
    write_observer: AsyncMutex<Vec<(PathBuf, Vec<u8>)>>,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum Op {
    TryExists, Metadata, Read, CreateDirAll, Write, RemoveFile,
}

impl MockMemoFs {
    pub fn new() -> Self { Self::default() }
    pub fn fail_at(&self, op: Op, path: impl Into<PathBuf>, kind: io::ErrorKind) -> &Self;
    pub fn clear_failures(&self) -> &Self;
    pub async fn writes(&self) -> Vec<(PathBuf, Vec<u8>)>;
}
```

設計ポイント:
- `inner: TokioMemoFs` への delegate により、大半のテストは実 tempdir 上で動く
- `(Op, PathBuf)` キーで失敗注入。`chmod` に依存せず、Rust 側で「このパスへのこの操作は `PermissionDenied` を返す」と決定論的に制御
- `write_observer` で「どのパスに何が書かれたか」を検証可能（暗黙移行や cleanup の挙動検証に必要）
- `clear_failures()` でテスト間の汚染を防ぐ

### 6.2 共通ヘルパー

```rust
pub(crate) struct TempWorkspace {
    dir: tempfile::TempDir,
    permission_resets: Mutex<Vec<(PathBuf, std::fs::Permissions)>>,
}

impl TempWorkspace {
    pub fn new() -> io::Result<Self>;
    pub fn path(&self) -> &Path;
    pub fn write_file(&self, rel: &Path, content: &str) -> io::Result<PathBuf>;
    pub fn write_md(&self, rel: &Path, content: &str) -> io::Result<PathBuf>;
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        // permission_resets を逆順で復元してから tempdir を削除
        // chmod 0o555 等の状態が残っていてもクリーンアップ可能にする
        // （MockMemoFs 経由でエラー注入する設計なので chmod は本来使われないが、
        //  念のため安全策として保持）
    }
}

pub(crate) fn make_test_app_state(
    mode: AppMode,
    memo_fs: Arc<dyn MemoFs>,
) -> AppState {
    let (sender, _) = broadcast::channel(16);
    AppState::new(mode, sender, "InspiredGitHub".into(), false)
        .with_memo_fs(memo_fs)
}
```

### 6.3 既存テスト 16+ 件の移行マッピング

| 既存テスト名 | 新仕様での扱い |
|------------|------------|
| `test_save_route_memo_新メモファイルがシンボリックリンクなら拒否する` | 維持（403 検証、変更なし） |
| `test_save_route_memo_単一ファイルモードで同階層sidecarへ保存する` | 維持 |
| `test_save_route_memo_旧パスのみ存在する場合は新sidecarへ移行して保存する` | 維持（暗黙移行の核心テスト。`MockMemoFs::writes()` で書き込み履歴を検証） |
| `test_save_route_memo_旧symlinkが残っていてもsidecar保存を継続できる` | 維持（unsafe legacy がスキップされる挙動） |
| `test_save_route_memo_空白保存はunsafeなlegacyがあってもsidecar削除を優先する` | 維持（空保存削除挙動） |
| `test_save_route_memo_空白保存でsafe_legacy削除失敗ならエラーにする` | **挙動変更**: テスト名を `test_save_route_memo_空白保存_safe_legacy削除失敗は警告のみで200を返す` に変更し、新仕様（warn＋200）を検証 |
| `test_save_route_memo_保存成功後のlegacy削除失敗は成功扱いにする` | 維持（同じ挙動。`chmod` を `MockMemoFs::fail_at` に置換） |
| `test_save_route_memo_書込不可サブディレクトリではlegacyへfallbackする` | **削除**（fallback 廃止）。代わりに 6.4 の新規テストでカバー |
| `test_save_route_memo_長いファイル名でも短縮sidecarへ保存できる` | 維持 |
| `test_save_route_memo_長いファイル名のlegacyメモは空白保存で削除できる` | 維持 |
| `test_save_route_memo_非utf8ファイル名でもsidecarが衝突しない` | 維持 |
| `test_save_route_memo_正規化される短いファイル名でもsidecarが衝突しない` | 維持 |
| `test_save_route_memo_拡張子の大文字小文字が異なるファイルでもsidecarが衝突しない` | 維持 |
| `test_save_route_memo_旧形式backslash_sidecarを新形式へ移行する` | 維持（compat_sidecar の暗黙移行） |
| `test_save_route_memo_旧形式backslash_sidecarは新形式作成不可なら既存compatへfallbackする` | **削除**（fallback 廃止） |
| `test_save_route_memo_既存compatが書込不可なら既存legacyへfallbackする` | **削除**（fallback 廃止） |
| `test_save_route_memo_単一ファイルモードではpermission_deniedでもlegacyへfallbackしない` | **転用**: テスト名を `test_save_route_memo_単一ファイルモードでpermission_deniedなら500を返す` に変更し、500 を確認 |
| `test_save_route_memo_単一ファイルモードでも既存legacyがあればpermission_denied時にfallbackする` | **削除**（fallback 廃止） |
| `test_load_route_memo_*`（5 件） | すべて維持。実装が単純化されるだけで、観察可能な挙動は不変 |

### 6.4 新規追加テスト一覧

| テスト名 | 検証内容 |
|---------|---------|
| `test_save_route_memo_sidecar書込不可で500を返す` | `MockMemoFs::fail_at(Op::Write, sidecar, PermissionDenied)` → 500 |
| `test_save_route_memo_create_dir_all失敗で500を返す` | `fail_at(Op::CreateDirAll, sidecar.parent(), PermissionDenied)` → 500 |
| `test_save_route_memo_disk_full系IO失敗で500を返す` | `fail_at(Op::Write, sidecar, Other)` → 500（fallback しない確認） |
| `test_save_route_memo_保存成功後のcompat削除失敗は200を返す` | compat 既存 + `fail_at(Op::RemoveFile, compat)` → 200, warn ログ |
| `test_save_route_memo_空保存_sidecarが既にない場合は冪等的に200を返す` | sidecar 不在 → 200（NotFound 緩和） |
| `test_save_route_memo_空保存_sidecar削除失敗は500を返す` | `fail_at(Op::RemoveFile, sidecar, PermissionDenied)` → 500 |
| `test_load_route_memo_sidecar優先_compat_legacy両方存在しても新sidecarを返す` | 3 形式並存時の優先順位 |
| `test_load_route_memo_compat優先_legacy存在でも新compatを返す` | sidecar 不在時の compat 優先 |
| `test_load_route_memo_全て不在なら空メモ` | 全形式不在 → empty |

### 6.5 chmod 依存テストの全廃

既存の `chmod 0o555` / `0o444` を使った 7 箇所はすべて `MockMemoFs::fail_at(...)` に置換する。

副次効果:
- root 実行下でも結果が決定論的
- macOS / Linux / コンテナで同一結果
- `TempWorkspace::Drop` で権限戻し漏れの心配がなくなる（chmod を本来使わなくなるため）

## 7. スコープ外（再掲）

- watcher / content / search / catalog 系の DI 化
- TODO Process-2「テスト失敗分類ラベル運用」
- 起動時の一括移行コマンド
- E2E テスト

## 8. 実装順序のヒント

実装プラン作成時の参考。順序は writing-plans スキル側で最終決定する。

- **Phase A**: `Cargo.toml` に `async-trait` 追加。`src/server/files/memo_fs.rs` 新設、`trait MemoFs` ＋ `TokioMemoFs` 実装。この段階で `cargo build` は通り、既存テストは全て pass のまま（trait 自体はまだ誰も使わない）。
- **Phase B**: `AppState` に `memo_fs: Arc<dyn MemoFs>` フィールド追加、`new` / `with_memo_fs` / `memo_fs()` API 整備。同時に `memo.rs` 内の `tokio::fs::*` 直呼び 12 箇所をすべて `state.memo_fs()` 経由に置換（既存の fallback ロジックは保持）。この段階で `cargo test` は通り、本番経路は DI 化されているが契約はまだ旧仕様のまま。
- **Phase C**: `src/server/files/test_support.rs` 新設、`MockMemoFs` ＋ `TempWorkspace` ＋ `make_test_app_state` 整備。tests.rs はまだ既存内容のまま（Phase E で移行）。
- **Phase D**: `memo.rs` を新契約に書き直し（`SidecarFallback` enum / `SaveTarget` struct / `choose_save_target` / `save_memo_to_fallback` 系 / `sidecar_fallback_for_error` / `is_name_too_long_error` を削除、cleanup ロジックを 4.1.1 / 4.1.2 / 4.5 表通りにシンプル化）。この段階で `cargo build` は通るが、既存テストは旧契約前提のため大量に失敗する想定。
- **Phase E**: 既存テスト 16+ 件を 6.3 マッピング表のとおり移行（`MockMemoFs` ベース、削除分は削除、転用分はテスト名変更）。`chmod` 依存を全廃。
- **Phase F**: 6.4 の新規テストを追加。
- **Phase G**: `cargo test --all-targets --all-features` ＋ `./verify.sh` を root / non-root で実行し決定論的に同一結果になることを確認。`TODO.md` の High-1 / High-2 / Process-1 / Process-3（memo 経路分）を完了マーク。`docs/superpowers/plans/2026-04-25-codebase-audit-task-proposals.md` の High-1 / High-2 を完了に更新。

## 9. 完了条件

- [ ] `cargo test --all-targets --all-features` が non-root / root 両方で同一結果（chmod 依存テスト排除済み）
- [ ] `./verify.sh` 通過
- [ ] 6.3 マッピング表のとおりテストが書き換わっている（維持・転用・削除）
- [ ] 6.4 新規追加テスト一覧の全件が pass
- [ ] `SidecarFallback` enum / `SaveTarget` struct / `choose_save_target` / `save_memo_to_fallback` / `save_memo_to_existing_compat*` / `save_memo_to_legacy` / `sidecar_fallback_for_error` / `is_name_too_long_error` がコードベースから消えている
- [ ] `TODO.md` の以下が完了マーク:
  - High-1（memo permission/fallback 契約再定義）
  - High-2（`test_save_route_memo_*` 環境依存排除）
  - Process-1（memo 保存仕様の設計ノート → 本 spec 自体で消化）
  - Process-3 のうち memo 経路分（残りは別 spec として TODO に残置・注記追加）
