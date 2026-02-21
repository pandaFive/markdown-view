use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use tokio::sync::broadcast;

use markdown_view::cli::Args;
use markdown_view::server::{create_router, AppState, MAX_FILE_SIZE};
use markdown_view::watcher::watch_file;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // ファイル存在チェック
    let file_path = args
        .file
        .canonicalize()
        .with_context(|| format!("ファイルが見つかりません: {}", args.file.display()))?;

    if !file_path.is_file() {
        bail!(
            "指定されたパスはファイルではありません: {}",
            file_path.display()
        );
    }

    // ファイルサイズチェック
    let metadata = std::fs::metadata(&file_path).context("ファイルのメタデータ取得に失敗")?;
    if metadata.len() > MAX_FILE_SIZE {
        bail!(
            "ファイルサイズが上限（{}MB）を超えています: {}",
            MAX_FILE_SIZE / 1024 / 1024,
            file_path.display()
        );
    }

    // broadcast チャネル
    let (tx, _rx) = broadcast::channel(16);

    let state = Arc::new(AppState {
        file_path: file_path.clone(),
        dark_mode: args.dark,
        theme: args.theme,
        tx,
    });

    // ファイル監視開始
    watch_file(state.clone())
        .await
        .context("ファイル監視の開始に失敗")?;

    // HTTPサーバー起動（127.0.0.1のみにバインド）
    let bind_addr = format!("127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("ポート {} へのバインドに失敗", args.port))?;

    let local_addr = listener
        .local_addr()
        .context("ローカルアドレスの取得に失敗")?;
    let url = format!("http://{}", local_addr);

    eprintln!("markdown-view: {} をプレビュー中", file_path.display());
    eprintln!("URL: {}", url);
    eprintln!("Ctrl+C で終了");

    // ブラウザ自動起動
    if !args.no_open {
        if let Err(e) = open::that(&url) {
            eprintln!("ブラウザの起動に失敗しました: {}", e);
        }
    }

    let router = create_router(state);
    axum::serve(listener, router).await?;

    Ok(())
}
