# watcher 除外監視計画と ENOSPC 文言改善設計

**作成日**: 2026-05-08
**対象 TODO**: `docs/todo/TODO.md` Medium Priority「watcher 再帰監視の除外パターンと ENOSPC ユーザー文言を追加する」

## 目的

ディレクトリモードの watcher が巨大リポジトリで `.git`、`node_modules`、`target` まで再帰監視し、Linux の `fs.inotify.max_user_watches` 上限に到達して起動失敗する問題を減らす。

起動時に監視登録対象を明示的な `WatchPlan` として作り、機械的に不要なディレクトリを `notify` 登録前に除外する。さらに起動後に作成された許可対象ディレクトリは動的に watch へ追加し、現行の recursive 監視が持つ「新規サブディレクトリにも追従する」性質を維持する。

あわせて ENOSPC 相当の初期化失敗では、notify の生エラーだけでなく、inotify 上限の確認と調整に進める日本語メッセージを出す。

## 非目的

- ユーザー指定の除外パターン CLI / config を追加しない。
- `.gitignore` を解釈しない。
- Markdown ファイル一覧、検索、HTTP/API、WebSocket JSON の外部契約を変更しない。
- watcher の自動再起動機構を追加しない。
- Linux 以外の OS に inotify 固有の対処を押し付けない。
- 除外ディレクトリ配下の変更を検出するオプションは追加しない。

## 採用方針

`WatchStrategy` から `WatchPlan` を生成する。

単一ファイルモードは従来どおり、監視対象ファイルの親ディレクトリを `RecursiveMode::NonRecursive` で登録する。ディレクトリモードは base 配下を起動時に走査し、除外対象を通らないディレクトリだけを `RecursiveMode::NonRecursive` で個別登録する。

起動後のディレクトリ作成に追従するため、watcher thread は debouncer callback から受け取ったイベントを自分の制御ループで処理する。許可対象の新規ディレクトリを検出したら、その subtree を除外規則つきで走査し、追加 watch と既存 Markdown の回復通知を行う。これにより recursive watch から non-recursive 複数 watch へ変えても、通常の Markdown workspace 利用で新規ディレクトリが無視され続ける退行を避ける。

最初に除外するディレクトリ名は `.git`、`node_modules`、`target`、隠しディレクトリである。これは個人向け Markdown workspace の設計思想に合う。生成物、依存パッケージ、VCS 内部、隠し作業領域は Markdown preview / memo / search の主要対象ではなく、監視資源を消費してユーザー体験を悪化させやすい。

## 代替案

### 案A: Recursive watch のまま ENOSPC 文言だけ改善

変更量は最小だが、watch 数を減らせない。巨大 repo の起動失敗は残るため、根本対策にならない。

### 案B: `WatchPlan` と動的追加で事前除外して個別登録する

watch 数を直接減らし、監視境界を型とテストで固定できる。起動時走査と runtime 側の制御ループは増えるが、対象はディレクトリのみで、既存の catalog/search と同じくファイルシステム境界を明示できる。新規ディレクトリ作成にも追従できるため、今回採用する。

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

`WatchStrategy::watch_plan()` は単一ファイルモードでは 1 entry、ディレクトリモードでは除外済みの複数 entry を返す。`watch_dir()` と `recursive_mode()` は plan 化後に runtime の主経路から外して削除する。

`WatchPlan` は登録対象だけでなく診断情報も持つ。

- `registered_candidates`: notify 登録を試みる entry 数。
- `excluded_subtrees`: 除外した subtree 数。
- `excluded_by_reason`: `.git`、`node_modules`、`target`、hidden、symlink、metadata error の理由別件数。

診断情報は `debug!` に出し、通常ログでは騒がせない。

### 除外判定

除外判定は `WatchExcludes` 相当の小さな helper に閉じる。

- component 名が `.git`、`node_modules`、`target` なら除外する。
- component 名が `.` で始まるディレクトリは除外する。
- base directory 自体が隠し名でも、起動時にユーザーが明示指定した root なので除外しない。
- symlink directory は follow しない。watcher の目的は base 配下の実ディレクトリ監視であり、symlink 実体は後段の `resolve_change_target()` 再検証で守る。

ディレクトリ走査中の `read_dir` / metadata 失敗は warn ログに sanitize 済み path と error を残し、その subtree を登録しない。起動全体は失敗させない。明示指定された base 自体の plan 生成に失敗する場合だけ init failure にする。

### Runtime 制御ループ

`src/watcher/runtime.rs` は `WatchPlan` の entries を順に `debouncer.watcher().watch(&entry.path, entry.recursive_mode)` へ登録する。

いずれかの登録で失敗した場合は init failure とする。複数 watch の一部成功後に失敗した場合も、watcher thread は初期化失敗として終了する。部分監視で起動すると「一部ファイルだけ更新されない」silent failure になるため避ける。

debouncer callback は `notify` イベントを直接 `WatchEvent` に変換しない。callback は `std::sync::mpsc::Sender<InternalWatchResult>` へ debounced result を渡すだけにする。watcher thread 本体は shutdown flag を見ながら `recv_timeout` で internal result を処理し、次を同じスレッドで行う。

1. notify error を `WatchEvent::Error` と health failure に変換する。
2. 既存の `collect_changed_paths()` で Markdown 変更通知を作る。
3. ディレクトリモードではイベント path から新規ディレクトリ候補を抽出する。
4. 候補 subtree を `WatchPlan::for_new_subtree()` で除外規則つきに展開する。
5. 未登録 path だけを `watch(..., NonRecursive)` へ追加する。
6. 追加 subtree 内に既存 `.md` があれば、作成直後の取りこぼし回復として `WatchEvent::FileChanged` を送る。

callback から `debouncer.watcher()` を直接触らない。`notify-debouncer-mini` の `Debouncer::watcher()` は `&mut dyn Watcher` を返すため、watcher を mutate する処理は debouncer を所有する watcher thread 本体に閉じる。

登録済み path は `HashSet<PathBuf>` で保持し、同じディレクトリを二重登録しない。追加登録に失敗した場合は `WatchEvent::Error(WatchError::notify(...))` と health `Failed(Notify)` にし、既存 watcher は継続する。起動後の追加失敗は init failure ではないが、監視品質の劣化として扱う。

登録成功数、除外 subtree 数、除外理由別件数は `tracing::debug!` に出す。通常利用で騒がしくしないため、詳細な excluded path 一覧は出さない。

### ENOSPC 文言

`src/watcher/error.rs` に ENOSPC 相当を表す `WatchErrorKind::ResourceExhausted` を追加する。

既存 `WatchErrorKind::Init` は通常の初期化失敗として維持する。watch 登録失敗が ENOSPC 相当なら `WatchError::resource_exhausted(detail)` を生成し、`user_message()` が次を満たす。

- Linux の inotify watch 上限に到達した可能性を明記する。
- `fs.inotify.max_user_watches` を確認する対象として示す。
- 一時変更と永続化は「例」として示し、自動実行しない。
- notify error detail はログや補足に残すが、ユーザーに実行すべきコマンドとして解釈させない。

ENOSPC 判定は `notify::ErrorKind::MaxFilesWatch` を主経路にする。補助として `notify::ErrorKind::Io` の `raw_os_error() == Some(28)`、`No space left on device`、`ENOSPC` 文字列を helper に集約する。誤検知しても対処案は安全な説明に留まる。

## データフロー

1. `WatchService::start()` が `Watcher::spawn(mode)` を呼ぶ。
2. `Watcher::spawn()` が `WatchStrategy::from_mode(&mode)` を作る。
3. `WatchStrategy::watch_plan()` が監視登録 entries を返す。
4. watcher thread が debouncer を作る。
5. runtime が `WatchPlanEntry` を順に `notify` へ登録する。
6. 登録失敗時は `WatchError` を init result として返す。ENOSPC 相当なら user message に inotify advice を含める。
7. 登録成功後、debouncer callback は internal channel に結果を渡す。
8. watcher thread 本体が internal result を処理し、Markdown 変更通知と新規ディレクトリの動的 watch 追加を行う。
9. `WatchEvent::FileChanged` は現行どおり後段の `resolve_change_target()` 再検証を通る。

## エラー処理

- base plan 生成失敗: init failure。
- subtree 走査失敗: warn log、該当 subtree は登録しない。
- watch 登録失敗: init failure。部分監視では起動しない。
- 起動後の追加 watch 失敗: notify failure。health を `Failed(Notify)` にし、`WatchEvent::Error` を送って既存 watch は継続する。
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
- 新規 subtree 用 plan が既存 plan と同じ除外規則を使う。
- 新規 subtree 用 plan が既存登録済み path を二重登録しない。

`src/watcher/error.rs`:

- ENOSPC 相当の detail で user message に inotify advice が含まれる。
- `notify::ErrorKind::MaxFilesWatch` が resource exhausted と判定される。
- `notify::ErrorKind::Io` の raw os error 28 が resource exhausted と判定される。
- 通常 init / notify / panic の既存文言が壊れない。

`src/watcher/runtime.rs`:

- 複数 entry 登録のうち1件が失敗した場合、init failure になる helper 境界をユニットテストで固定する。
- debouncer callback は internal channel へ渡すだけで、watcher mutation は owner loop で行う。
- internal event 処理が Markdown 変更通知と新規ディレクトリ追加を同じ batch から行う。
- 起動後の追加 watch 失敗は health `Failed(Notify)` と `WatchEvent::Error` になり、thread は継続する。

統合テスト:

- ディレクトリモードで通常 `.md` 更新通知が届く既存テストを維持する。
- 新規サブディレクトリ作成後、その配下の `.md` 更新通知が届くことを watcher spawn 経路で確認する。
- 除外ディレクトリ配下 `.md` の変更が通知されないことは、watcher spawn 経路ではなく plan unit test と runtime の登録 helper test で保証する。ファイルシステム通知の非発生を待つ統合テストは flaky になりやすいため追加しない。

## 受け入れ条件

- ディレクトリモードで `.git`、`node_modules`、`target`、隠しディレクトリ配下が `notify` 登録対象にならない。
- 通常ディレクトリ配下の `.md` 更新通知は維持される。
- 起動後に作成された通常サブディレクトリ配下の `.md` 更新通知も維持される。
- watch 登録の部分成功では起動しない。
- 起動後の追加 watch 失敗は silent failure ではなく health failure と `WatchEvent::Error` になる。
- ENOSPC 相当の初期化失敗が、Linux inotify 上限に関する操作可能な日本語メッセージになる。
- README に Linux inotify 上限と既定除外方針を短く追記する。
- `./verify.sh` が通る。

## セキュリティ考慮

notify event、filesystem path、error detail は未信頼入力として扱う。ログには既存方針どおり sanitize 済み path を出す。

除外 plan は監視資源の節約であり、読み込み許可の境界ではない。実際の Markdown 読み込みは引き続き `resolve_change_target()`、`resolve_file()`、canonical base 検証、HTML sanitize、CSP で守る。

symlink directory を起動時 plan で辿らないことで、base 外 subtree を大量監視する事故を避ける。symlink file や race は後段の再検証で扱う。

動的 watch 追加でも同じ除外規則を使う。新規ディレクトリ名、symlink、metadata は未信頼入力として扱い、base 外や hidden subtree を登録しない。作成直後の race で消えた path は warn / skip に留め、読み込み前再検証を維持する。

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

問題が plan 生成や動的追加だけに限られる場合は、`WatchStrategy::watch_plan()` のディレクトリモードを base 1件 `RecursiveMode::Recursive` に戻し、runtime 制御ループを旧 callback 直送へ戻す一時 rollback も可能。ただし ENOSPC 回避効果は失われる。

## 工数見積もり

人間の作業見積もり: 6-10 時間。watch plan、動的 watch 追加、internal event loop、OS 差分を考慮したテスト、README 更新、全検証を含む。

Codex / AI 支援込み見積もり: 3-5 時間。既存 watcher tests は厚いが、callback 直送から owner loop へ変えるため、段階的なテスト追加と慎重な統合確認が必要。
