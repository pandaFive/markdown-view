use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tokio::sync::mpsc;

use crate::server::{notify_update, AppState};

/// デバウンス間隔（ミリ秒）
const DEBOUNCE_MS: u64 = 300;

/// ファイル監視を開始する
///
/// notify + debouncer でファイル変更を検知し、
/// tokioランタイムにブリッジしてbroadcastで通知する
pub async fn watch_file(state: Arc<AppState>) -> Result<()> {
    let file_path = state
        .file_path
        .canonicalize()
        .context("ファイルパスの正規化に失敗")?;

    // 監視対象ディレクトリ（ファイルの親ディレクトリ）
    let watch_dir = file_path
        .parent()
        .context("親ディレクトリが取得できません")?
        .to_path_buf();

    let target_path = file_path.clone();

    // tokio::sync::mpscでnotifyからtokioにブリッジ
    let (tx, mut rx) = mpsc::channel(32);

    // debouncerをstd::threadで起動（notifyはsyncスレッドで動作）
    std::thread::spawn(move || {
        let rt_tx = tx;
        let mut debouncer = new_debouncer(
            Duration::from_millis(DEBOUNCE_MS),
            move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
                if let Ok(events) = res {
                    for event in events {
                        if event.kind == DebouncedEventKind::Any {
                            // 対象ファイルの変更のみ通知
                            if is_target_file(&event.path, &target_path) {
                                let _ = rt_tx.blocking_send(());
                                break;
                            }
                        }
                    }
                }
            },
        )
        .expect("debouncerの初期化に失敗");

        debouncer
            .watcher()
            .watch(&watch_dir, notify::RecursiveMode::NonRecursive)
            .expect("ファイル監視の開始に失敗");

        // スレッドを維持（debouncerのlifetimeのため）
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    });

    // tokioタスクでファイル変更通知を処理
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            notify_update(&state).await;
        }
    });

    Ok(())
}

/// パスが監視対象ファイルと一致するか判定する
fn is_target_file(event_path: &Path, target_path: &Path) -> bool {
    // canonicalizeで比較（シンボリックリンク対応）
    match event_path.canonicalize() {
        Ok(canonical) => canonical == *target_path,
        Err(_) => event_path == target_path,
    }
}
