# watcher 除外監視計画と ENOSPC 文言改善設計

**作成日**: 2026-05-08
**対象 TODO**: `docs/todo/TODO.md` Medium Priority「watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する」

## 目的

ディレクトリモードの watcher が巨大リポジトリで `.git`、`node_modules`、`target` まで再帰監視し、Linux の `fs.inotify.max_user_watches` 上限に到達して起動失敗する問題を減らす。

起動時に監視登録対象を明示的な `WatchPlan` として作り、機械的に不要なディレクトリを `notify` 登録前に除外する。あわせて ENOSPC 相当の初期化失敗では、notify の生エラーだけでなく、inotify 上限の確認と調整に進める日本語メッセージを出す。

## 非目的

- ユーザー指定の除外パターン CLI / config を追加しない。
- `.gitignore` を解釈しない。
- Markdown ファイル一覧、検索、HTTP/API、WebSocket JSON の外部契約を変更しない。
- watcher の自動再起動機構を追加しない。
- Linux 以外の OS に inotify 固有の対処を押し付けない。

## 採用方針

`WatchStrategy` から `WatchPlan` を生成する。

単一ファイルモードは従来どおり、監視対象ファイルの親ディレクトリを `RecursiveMode::NonRecursive` で登録する。ディレクトリモードは base 配下を起動時に走査し、除外対象を通らないディレクトリだけを `RecursiveMode::NonRecursive` で個別登録する。

最初に除外するディレクトリ名は `.git`、`node_modules`、`target`、隠しディレクトリである。これは個人向け Markdown workspace の設計思想に合う。生成物、依存パッケージ、VCS 内部、隠し作業領域は Markdown preview / memo / search の主要対象ではなく、監視資源を消費してユーザー体験を悪化させやすい。

## 代替案

### 案A: Recursive watch のまま ENOSPC 文言だけ改善

変更量は最小だが、watch 数を減らせない。巨大 repo の起動失敗は残るため、根本対策にならない。

### 案B: `WatchPlan` で事前除外して個別登録する

watch 数を直接減らし、監視境界を型とテストで固定できる。起動時走査は増えるが、対象はディレクトリのみで、既存の catalog/search と同じくファイルシステム境界を明示できる。今回採用する。

### 案C: CLI / config で除外パターンをユーザー指定可能にする

将来拡張としては自然だが、今は設定面が先行する。まずは既定除外を型化し、後から同じ `WatchPlan` に user excludes を注入できる余地を残す。

## アーキテクチャ

### `WatchPlan`

`src/watcher/strategy.rs` に内部型を追加する。

```rust
pub(super) struct WatchPlan {
    entries: Vec<WatchPlanEntry>,
}

pub(super) struct WatchPlanEntry {
    path: PathBuf,
    recursive_mode: RecursiveMode,
}
```

`WatchStrategy::watch_plan()` は単一ファイルモードでは 1 entry、ディレクトリモードでは除外済みの複数 entry を返す。`watch_dir()` と `recursive_mode()` は plan 化後に runtime の主経路から外し、テスト互換や段階的移行が不要なら削除する。

### 除外判定

除外判定は `WatchExcludes` 相当の小さな helper に閉じる。

- component 名が `.git`、`node_modules`、`target` なら除外する。
- component 名が `.` で始まるディレクトリは除外する。
- base directory 自体が隠し名でも、起動時にユーザーが明示指定した root なので除外しない。
- symlink directory は follow しない。watcher の目的は base 配下の実ディレクトリ監視であり、symlink 実体は後段の `resolve_change_target()` 再検証で守る。

ディレクトリ走査中の `read_dir` / metadata 失敗は warn ログに sanitize 済み path と error を残し、その subtree を登録しない。起動全体は失敗させない。明示指定された base 自体の plan 生成に失敗する場合だけ init failure にする。

### Runtime 登録

`src/watcher/runtime.rs` は `WatchPlan` の entries を順に `debouncer.watcher().watch(&entry.path, entry.recursive_mode)` へ登録する。

いずれかの登録で失敗した場合は init failure とする。複数 watch の一部成功後に失敗した場合も、watcher thread は初期化失敗として終了する。部分監視で起動すると「一部ファイルだけ更新されない」silent failure になるため避ける。

登録成功数、除外 subtree 数、除外理由別件数は `tracing::debug!` に出す。通常利用で騒がしくしないため、詳細な excluded path 一覧は出さない。

### ENOSPC 文言

`src/watcher/error.rs` に ENOSPC 相当を表す `WatchErrorKind::ResourceExhausted` を追加する。

既存 `WatchErrorKind::Init` は通常の初期化失敗として維持する。watch 登録失敗が ENOSPC 相当なら `WatchError::resource_exhausted(detail)` を生成し、`user_message()` が次を満たす。

- Linux の inotify watch 上限に到達した可能性を明記する。
- `fs.inotify.max_user_watches` を確認する対象として示す。
- 一時変更と永続化は「例」として示し、自動実行しない。
- notify error detail はログや補足に残すが、ユーザーに実行すべきコマンドとして解釈させない。

ENOSPC 判定は `notify::Error` の文字列表現に依存せざるを得ない場合があるため、`No space left on device`、`ENOSPC`、Linux raw os error 28 を小さな helper に集約する。誤検知しても対処案は安全な説明に留まる。

## データフロー

1. `WatchService::start()` が `Watcher::spawn(mode)` を呼ぶ。
2. `Watcher::spawn()` が `WatchStrategy::from_mode(&mode)` を作る。
3. `WatchStrategy::watch_plan()` が監視登録 entries を返す。
4. watcher thread が debouncer を作る。
5. runtime が `WatchPlanEntry` を順に `notify` へ登録する。
6. 登録失敗時は `WatchError` を init result として返す。ENOSPC 相当なら user message に inotify advice を含める。
7. 登録成功後のイベント処理は現行どおり `collect_changed_paths()` と後段の `resolve_change_target()` 再検証を通る。

## エラー処理

- base plan 生成失敗: init failure。
- subtree 走査失敗: warn log、該当 subtree は登録しない。
- watch 登録失敗: init failure。部分監視では起動しない。
- ENOSPC 相当: resource exhausted として操作可能な user message。
- notify callback error: 既存どおり `WatchEvent::Error` と health `Failed(Notify)`。

## テスト方針

TDD で進める。

`src/watcher/strategy.rs`:

- 単一ファイルモードの plan が親ディレクトリ `NonRecursive` 1件になる。
- ディレクトリモードの plan が通常サブディレクトリを含む。
- `.git`、`node_modules`、`target`、隠しディレクトリ配下を plan に含めない。
- base 自体が隠し名でも root entry は含める。
- symlink directory を辿らない。

`src/watcher/error.rs`:

- ENOSPC 相当の detail で user message に inotify advice が含まれる。
- 通常 init / notify / panic の既存文言が壊れない。

`src/watcher/runtime.rs`:

- 複数 entry 登録のうち1件が失敗した場合、init failure になる helper 境界をユニットテストで固定する。

統合テスト:

- ディレクトリモードで通常 `.md` 更新通知が届く既存テストを維持する。
- 除外ディレクトリ配下 `.md` の変更が通知されないことは、watcher spawn 経路ではなく plan unit test と runtime の登録 helper test で保証する。ファイルシステム通知の非発生を待つ統合テストは flaky になりやすいため追加しない。

## 受け入れ条件

- ディレクトリモードで `.git`、`node_modules`、`target`、隠しディレクトリ配下が `notify` 登録対象にならない。
- 通常ディレクトリ配下の `.md` 更新通知は維持される。
- watch 登録の部分成功では起動しない。
- ENOSPC 相当の初期化失敗が、Linux inotify 上限に関する操作可能な日本語メッセージになる。
- README に Linux inotify 上限と既定除外方針を短く追記する。
- `./verify.sh` が通る。

## セキュリティ考慮

notify event、filesystem path、error detail は未信頼入力として扱う。ログには既存方針どおり sanitize 済み path を出す。

除外 plan は監視資源の節約であり、読み込み許可の境界ではない。実際の Markdown 読み込みは引き続き `resolve_change_target()`、`resolve_file()`、canonical base 検証、HTML sanitize、CSP で守る。

symlink directory を起動時 plan で辿らないことで、base 外 subtree を大量監視する事故を避ける。symlink file や race は後段の再検証で扱う。

ENOSPC advice は説明として表示するだけで、コマンドを自動実行しない。外部由来の error detail を shell、SQL、HTML、ポリシーとして解釈しない。

## 影響範囲

- `src/watcher/strategy.rs`: watch plan と除外判定。
- `src/watcher/runtime.rs`: 複数 watch entry 登録、登録失敗処理。
- `src/watcher/error.rs`: ENOSPC 相当の user message。
- `README.md`: Linux inotify 上限と既定除外方針。
- `docs/todo/TODO.md`: 実装完了時に対象 TODO を Done Summary へ移す。

HTTP route、renderer、template、memo、search の外部契約は変更しない。

## ロールバック

実装コミットを revert すれば、従来のディレクトリ `RecursiveMode::Recursive` 1件登録へ戻せる。

問題が plan 生成だけに限られる場合は、`WatchStrategy::watch_plan()` のディレクトリモードを base 1件 `RecursiveMode::Recursive` に戻す一時 rollback も可能。ただし ENOSPC 回避効果は失われる。

## 工数見積もり

人間の作業見積もり: 4-6 時間。watch plan の境界設計、OS 差分を考慮したテスト、README 更新、全検証を含む。

Codex / AI 支援込み見積もり: 1.5-3 時間。既存 watcher tests が厚いため、設計どおりに小さく分ければ短縮できる。
