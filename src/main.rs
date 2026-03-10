use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use tokio::sync::broadcast;

use markdown_view::cli::Args;
use markdown_view::renderer::validate_theme;
use markdown_view::server::{
    create_router, spawn_watch_event_forwarder, AppMode, AppState, MAX_FILE_SIZE,
};
use markdown_view::watcher::Watcher;

fn init_logging() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    if let Err(e) = tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(env_filter)
        .try_init()
    {
        eprintln!("[markdown-view] ログシステムの初期化に失敗: {}", e);
    }
}

async fn bind_preview_listener(
    preferred_port: u16,
) -> Result<(tokio::net::TcpListener, std::net::SocketAddr, bool)> {
    if preferred_port == 0 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("ポート自動割り当てに失敗")?;
        let local_addr = listener
            .local_addr()
            .context("ローカルアドレスの取得に失敗")?;
        return Ok((listener, local_addr, false));
    }

    for port in preferred_port..=u16::MAX {
        let bind_addr = format!("127.0.0.1:{}", port);
        match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(listener) => {
                let local_addr = listener
                    .local_addr()
                    .context("ローカルアドレスの取得に失敗")?;
                return Ok((listener, local_addr, port != preferred_port));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("ポート {} へのバインドに失敗", port));
            }
        }
    }

    bail!(
        "ポート {} 以上で利用可能なポートが見つかりませんでした",
        preferred_port
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let args = Args::parse();

    // パス存在チェック
    let path = args
        .path
        .canonicalize()
        .with_context(|| format!("パスが見つかりません: {}", args.path.display()))?;

    // ファイルかディレクトリかを判定してモードを決定
    let mode = if path.is_file() {
        // 単一ファイルモード: 起動時にサイズチェック
        let metadata = std::fs::metadata(&path).context("ファイルのメタデータ取得に失敗")?;
        if metadata.len() > MAX_FILE_SIZE {
            bail!(
                "ファイルサイズが上限（{}MB）を超えています: {}",
                MAX_FILE_SIZE / 1024 / 1024,
                path.display()
            );
        }
        AppMode::new_single_file(&path).context("単一ファイルモードの初期化に失敗")?
    } else if path.is_dir() {
        AppMode::new_directory(&path).context("ディレクトリモードの初期化に失敗")?
    } else {
        bail!(
            "指定されたパスはファイルでもディレクトリでもありません: {}",
            path.display()
        );
    };

    // テーマ名の起動時検証（存在しない場合は即座にエラー）
    if let Some(ref theme_name) = args.theme {
        if let Err(available) = validate_theme(theme_name) {
            bail!(
                "テーマ '{}' が見つかりません。利用可能なテーマ: {:?}",
                theme_name,
                available
            );
        }
    }

    // broadcast チャネル
    let (tx, _rx) = broadcast::channel(16);

    let state = Arc::new(AppState::new(mode.clone(), args.dark, args.theme, tx));

    // ファイル/ディレクトリ監視開始
    let (watcher, watch_events) = Watcher::spawn(mode.clone())
        .await
        .context("監視の開始に失敗")?;
    let watch_forwarder = spawn_watch_event_forwarder(state.clone(), watch_events);

    // HTTPサーバー起動（127.0.0.1のみにバインド）
    let (listener, local_addr, port_fallback) = bind_preview_listener(args.port).await?;
    let url = format!("http://{}", local_addr);

    if let Some(p) = mode.single_file() {
        tracing::info!("markdown-view: {} をプレビュー中", p.display());
    } else if let Some(p) = mode.directory() {
        tracing::info!(
            "markdown-view: {} 内のMarkdownファイルをプレビュー中",
            p.display()
        );
    }
    if port_fallback {
        tracing::warn!(
            "[markdown-view] ポート {} は使用中のため、空きポート {} を使用します",
            args.port,
            local_addr.port()
        );
    }
    tracing::info!("URL: {}", url);
    tracing::info!("Ctrl+C で終了");

    // ブラウザ自動起動
    if !args.no_open {
        if let Err(e) = open::that(&url) {
            tracing::warn!("ブラウザの起動に失敗しました: {}", e);
        }
    }

    let router = create_router(state);
    let server_result = axum::serve(listener, router)
        .with_graceful_shutdown(async {
            // Ctrl+C受信時にHTTPサーバーをグレースフル停止する
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    tracing::info!("[markdown-view] Ctrl+C を受信。終了します...");
                }
                Err(e) => {
                    tracing::error!(
                        "[markdown-view] シグナルハンドラの登録に失敗: {}。手動で終了してください",
                        e
                    );
                    // シグナルを待てないため永遠に待機する（別手段でプロセスを終了させる）
                    std::future::pending::<()>().await;
                }
            }
        })
        .await;

    watcher.shutdown();
    if let Err(e) = watch_forwarder.await {
        tracing::warn!(
            "[markdown-view] 監視イベント転送タスクの終了待機に失敗: {}",
            e
        );
    }
    server_result?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bind_preview_listener_空きポートなら指定ポートを使う() {
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").expect("空きポート確保");
        let port = reserved.local_addr().expect("ローカルアドレス取得").port();
        drop(reserved);

        let (listener, addr, port_fallback) =
            bind_preview_listener(port).await.expect("リスナー起動");
        assert_eq!(addr.port(), port);
        assert!(!port_fallback);
        drop(listener);
    }

    #[tokio::test]
    async fn test_bind_preview_listener_指定ポート使用中なら次の空きポートを使う() {
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").expect("空きポート確保");
        let port = reserved.local_addr().expect("ローカルアドレス取得").port();

        let (listener, addr, port_fallback) = bind_preview_listener(port)
            .await
            .expect("フォールバック起動");
        assert_ne!(addr.port(), port);
        assert!(addr.port() > port);
        assert!(port_fallback);
        drop(listener);
        drop(reserved);
    }
}
