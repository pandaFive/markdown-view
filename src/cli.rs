use std::path::PathBuf;

use clap::Parser;

/// 軽量・高速 Markdown プレビューア
#[derive(Parser, Debug)]
#[command(name = "markdown-view", version, about)]
pub struct Args {
    /// プレビューするMarkdownファイルまたはディレクトリのパス
    pub path: PathBuf,

    /// HTTPサーバーのポート番号
    #[arg(short, long, default_value_t = 3000)]
    pub port: u16,

    /// ブラウザの自動起動を無効にする
    #[arg(long)]
    pub no_open: bool,

    /// ダークモードを強制する
    #[arg(long)]
    pub dark: bool,

    /// シンタックスハイライトのテーマ名
    #[arg(long)]
    pub theme: Option<String>,
}
