use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use tokio::sync::broadcast;

use markdown_view::cli::Args;
use markdown_view::renderer::validate_theme;
use markdown_view::server::{create_router, AppMode, AppState, MAX_FILE_SIZE};
use markdown_view::watcher::watch_path;

fn init_logging() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(env_filter)
        .try_init();
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
    let watcher_handle = watch_path(state.clone())
        .await
        .context("監視の開始に失敗")?;

    // HTTPサーバー起動（127.0.0.1のみにバインド）
    let bind_addr = format!("127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("ポート {} へのバインドに失敗", args.port))?;

    let local_addr = listener
        .local_addr()
        .context("ローカルアドレスの取得に失敗")?;
    let url = format!("http://{}", local_addr);

    if let Some(p) = mode.single_file() {
        tracing::info!("markdown-view: {} をプレビュー中", p.display());
    } else if let Some(p) = mode.directory() {
        tracing::info!(
            "markdown-view: {} 内のMarkdownファイルをプレビュー中",
            p.display()
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
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;

    watcher_handle.shutdown().await;
    server_result?;

    Ok(())
}
