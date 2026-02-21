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
                                    if rt_tx.blocking_send(()).is_err() {
                                        // 受信側が閉じた場合はログ出力のみ
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
                let _ = init_tx.send(Err(format!("debouncerの初期化に失敗: {}", e)));
                return;
            }
        };

        if let Err(e) = debouncer
            .watcher()
            .watch(&watch_dir, notify::RecursiveMode::NonRecursive)
        {
            let _ = init_tx.send(Err(format!("ファイル監視の開始に失敗: {}", e)));
            return;
        }

        // 初期化成功を通知
        let _ = init_tx.send(Ok(()));

        // スレッドを維持（debouncerのlifetimeのため）
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    });

    // 初期化結果を待機
    match init_rx.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => anyhow::bail!(e),
        Err(_) => anyhow::bail!("ファイル監視スレッドが予期せず終了しました"),
    }

    // tokioタスクでファイル変更通知を処理
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            notify_update(&state).await;
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

/// パスが監視対象ファイルと一致するか判定する
fn is_target_file(event_path: &Path, target_path: &Path) -> bool {
    // canonicalizeで比較（シンボリックリンク対応）
    match event_path.canonicalize() {
        Ok(canonical) => canonical == *target_path,
        Err(_) => event_path == target_path,
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
}
