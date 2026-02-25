use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tokio::sync::mpsc;

use crate::server::{notify_update, AppMode, AppState};

/// デバウンス間隔（ミリ秒）
const DEBOUNCE_MS: u64 = 300;

/// ファイルまたはディレクトリの監視を開始する
///
/// notify + debouncer でファイル変更を検知し、
/// tokioランタイムにブリッジしてbroadcastで通知する
pub async fn watch_path(state: Arc<AppState>) -> Result<()> {
    match &state.mode {
        AppMode::SingleFile(file_path) => {
            let file_path = file_path
                .canonicalize()
                .context("ファイルパスの正規化に失敗")?;
            watch_single_file(state, file_path).await
        }
        AppMode::Directory(dir_path) => {
            let dir_path = dir_path
                .canonicalize()
                .context("ディレクトリパスの正規化に失敗")?;
            watch_directory(state, dir_path).await
        }
    }
}

/// 単一ファイルの監視
async fn watch_single_file(state: Arc<AppState>, file_path: PathBuf) -> Result<()> {
    // 監視対象ディレクトリ（ファイルの親ディレクトリ）
    let watch_dir = file_path
        .parent()
        .context("親ディレクトリが取得できません")?
        .to_path_buf();

    let target_path = file_path.clone();

    // tokio::sync::mpscでnotifyからtokioにブリッジ
    let (tx, mut rx) = mpsc::channel::<PathBuf>(32);

    // 初期化エラーを親タスクに伝播するための oneshot チャネル
    let (init_tx, init_rx) = tokio::sync::oneshot::channel::<std::result::Result<(), String>>();

    // debouncerをstd::threadで起動（notifyはsyncスレッドで動作）
    std::thread::spawn(move || {
        let rt_tx = tx;
        let debouncer = new_debouncer(
            Duration::from_millis(DEBOUNCE_MS),
            move |res: std::result::Result<
                Vec<notify_debouncer_mini::DebouncedEvent>,
                notify::Error,
            >| {
                match res {
                    Ok(events) => {
                        for event in events {
                            if is_content_change_event(&event.kind) {
                                // 対象ファイルの変更のみ通知
                                if is_target_file(&event.path, &target_path) {
                                    let path = event
                                        .path
                                        .canonicalize()
                                        .unwrap_or_else(|_| event.path.clone());
                                    if rt_tx.blocking_send(path).is_err() {
                                        eprintln!("[markdown-view] 通知チャネルが閉じています");
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[markdown-view] ファイル監視エラー: {}", e);
                    }
                }
            },
        );

        let mut debouncer = match debouncer {
            Ok(d) => d,
            Err(e) => {
                if init_tx
                    .send(Err(format!("debouncerの初期化に失敗: {}", e)))
                    .is_err()
                {
                    eprintln!("[markdown-view] 初期化エラーの通知先が既に閉じています");
                }
                return;
            }
        };

        if let Err(e) = debouncer
            .watcher()
            .watch(&watch_dir, notify::RecursiveMode::NonRecursive)
        {
            if init_tx
                .send(Err(format!("ファイル監視の開始に失敗: {}", e)))
                .is_err()
            {
                eprintln!("[markdown-view] 初期化エラーの通知先が既に閉じています");
            }
            return;
        }

        if init_tx.send(Ok(())).is_err() {
            eprintln!("[markdown-view] 初期化成功の通知先が既に閉じています");
        }

        // スレッドを維持（debouncerのlifetimeのため、spurious wakeupで再parkする）
        loop {
            std::thread::park();
        }
    });

    // 初期化結果を待機
    match init_rx.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => anyhow::bail!(e),
        Err(_) => anyhow::bail!("ファイル監視スレッドが予期せず終了しました"),
    }

    // tokioタスクでファイル変更通知を処理
    let notify_handle = tokio::spawn(async move {
        while let Some(changed_path) = rx.recv().await {
            notify_update(&state, &changed_path).await;
        }
        eprintln!("[markdown-view] ファイル変更通知タスクが終了しました。ライブリロードは無効です");
    });
    tokio::spawn(async move {
        if let Err(e) = notify_handle.await {
            eprintln!(
                "[markdown-view] ファイル変更通知タスクがパニックしました: {}",
                e
            );
        }
    });

    Ok(())
}

/// ディレクトリの再帰監視
async fn watch_directory(state: Arc<AppState>, dir_path: PathBuf) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<PathBuf>(32);

    let (init_tx, init_rx) = tokio::sync::oneshot::channel::<std::result::Result<(), String>>();

    let watch_dir = dir_path.clone();
    // イベントコールバック内で相対パスの隠しファイル判定に使用
    let base_for_filter = dir_path.clone();

    std::thread::spawn(move || {
        let rt_tx = tx;
        let debouncer = new_debouncer(
            Duration::from_millis(DEBOUNCE_MS),
            move |res: std::result::Result<
                Vec<notify_debouncer_mini::DebouncedEvent>,
                notify::Error,
            >| {
                match res {
                    Ok(events) => {
                        // 変更された.mdファイルを収集（重複排除）
                        let mut notified = std::collections::HashSet::new();
                        for event in events {
                            if !is_content_change_event(&event.kind) {
                                continue;
                            }
                            // .md拡張子フィルタ
                            let is_md = event
                                .path
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
                            if !is_md {
                                continue;
                            }
                            // 隠しファイル除外（ベースディレクトリからの相対パスで判定）
                            let is_hidden = is_hidden_relative(&event.path, &base_for_filter);
                            if is_hidden {
                                continue;
                            }
                            let path = event
                                .path
                                .canonicalize()
                                .unwrap_or_else(|_| event.path.clone());
                            // canonicalize後のパスがベースディレクトリ内であることを確認
                            // （symlink経由でディレクトリ外のファイルが変更された場合を防止）
                            if !path.starts_with(&base_for_filter) {
                                eprintln!(
                                    "[markdown-view] ベースディレクトリ外のパスを検出（スキップ）: {}",
                                    path.display()
                                );
                                continue;
                            }
                            if notified.insert(path.clone()) && rt_tx.blocking_send(path).is_err() {
                                eprintln!("[markdown-view] 通知チャネルが閉じています");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[markdown-view] ディレクトリ監視エラー: {}", e);
                    }
                }
            },
        );

        let mut debouncer = match debouncer {
            Ok(d) => d,
            Err(e) => {
                if init_tx
                    .send(Err(format!("debouncerの初期化に失敗: {}", e)))
                    .is_err()
                {
                    eprintln!("[markdown-view] 初期化エラーの通知先が既に閉じています");
                }
                return;
            }
        };

        // ディレクトリモードでは再帰監視
        if let Err(e) = debouncer
            .watcher()
            .watch(&watch_dir, notify::RecursiveMode::Recursive)
        {
            if init_tx
                .send(Err(format!("ディレクトリ監視の開始に失敗: {}", e)))
                .is_err()
            {
                eprintln!("[markdown-view] 初期化エラーの通知先が既に閉じています");
            }
            return;
        }

        if init_tx.send(Ok(())).is_err() {
            eprintln!("[markdown-view] 初期化成功の通知先が既に閉じています");
        }

        // スレッドを維持（debouncerのlifetimeのため、spurious wakeupで再parkする）
        loop {
            std::thread::park();
        }
    });

    match init_rx.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => anyhow::bail!(e),
        Err(_) => anyhow::bail!("ディレクトリ監視スレッドが予期せず終了しました"),
    }

    let notify_handle = tokio::spawn(async move {
        while let Some(changed_path) = rx.recv().await {
            notify_update(&state, &changed_path).await;
        }
        eprintln!(
            "[markdown-view] ディレクトリ変更通知タスクが終了しました。ライブリロードは無効です"
        );
    });
    tokio::spawn(async move {
        if let Err(e) = notify_handle.await {
            eprintln!(
                "[markdown-view] ディレクトリ変更通知タスクがパニックしました: {}",
                e
            );
        }
    });

    Ok(())
}

/// レンダリング更新が必要なイベント種別か判定する
fn is_content_change_event(kind: &DebouncedEventKind) -> bool {
    matches!(
        kind,
        DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous
    )
}

/// ベースディレクトリからの相対パスに隠しコンポーネントが含まれるか判定する
///
/// ベースディレクトリ自体が`.`で始まるパスに含まれる場合でも
/// 正しく動作するよう、相対パス部分のみをチェックする。
fn is_hidden_relative(path: &Path, base: &Path) -> bool {
    match path.strip_prefix(base) {
        Ok(relative) => relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
        Err(_) => {
            // strip_prefix失敗時はcanonicalizeして再試行
            let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
            match canonical_path.strip_prefix(&canonical_base) {
                Ok(relative) => relative
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.')),
                Err(_) => false,
            }
        }
    }
}

/// パスが監視対象ファイルと一致するか判定する
///
/// target_pathは起動時にcanonicalize済みの絶対パス。
/// event_pathもcanonicalizeして比較し、失敗時はファイル名で比較する。
fn is_target_file(event_path: &Path, target_path: &Path) -> bool {
    match event_path.canonicalize() {
        Ok(canonical) => canonical == *target_path,
        Err(e) => {
            eprintln!(
                "[markdown-view] パス正規化に失敗（ファイル名比較にフォールバック）: {} ({})",
                event_path.display(),
                e
            );
            // フォールバック: ファイル名と親ディレクトリが一致するかで判定
            event_path.file_name() == target_path.file_name()
                && event_path.parent() == target_path.parent()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_連続更新イベントも更新対象に含まれる() {
        assert!(is_content_change_event(&DebouncedEventKind::Any));
        assert!(is_content_change_event(&DebouncedEventKind::AnyContinuous));
    }

    #[test]
    fn test_隠しファイル判定_相対パスのみチェック() {
        // ベースディレクトリ自体がドットで始まるパスに含まれるケース
        let base = Path::new("/home/user/.config/docs");
        let visible_file = Path::new("/home/user/.config/docs/README.md");
        let hidden_file = Path::new("/home/user/.config/docs/.secret/notes.md");
        let hidden_dotfile = Path::new("/home/user/.config/docs/.hidden.md");

        // ベースディレクトリの.configはチェック対象外
        assert!(!is_hidden_relative(visible_file, base));
        // 相対パス部分の.secretは隠しディレクトリ
        assert!(is_hidden_relative(hidden_file, base));
        // 相対パス部分の.hidden.mdは隠しファイル
        assert!(is_hidden_relative(hidden_dotfile, base));
    }

    #[test]
    fn test_隠しファイル判定_通常のベースディレクトリ() {
        let base = Path::new("/home/user/docs");
        let visible = Path::new("/home/user/docs/guide.md");
        let hidden = Path::new("/home/user/docs/.draft/wip.md");

        assert!(!is_hidden_relative(visible, base));
        assert!(is_hidden_relative(hidden, base));
    }
}
