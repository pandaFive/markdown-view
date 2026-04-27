//! メモ保存・読み込みで使用するファイルシステム抽象。
//!
//! 本番では [`TokioMemoFs`] が `tokio::fs::*` を呼び出す薄いラッパーとして動作する。
//! テストでは `MockMemoFs`（`test_support` モジュール）を注入し、
//! 特定パスの I/O エラーを決定論的に再現する。

use std::fs::Metadata;
use std::path::Path;

use async_trait::async_trait;

use super::content::{read_bytes_with_limit, ReadMarkdownError};

#[derive(Debug)]
pub(crate) enum MemoReadError {
    Open(std::io::Error),
    Read(std::io::Error),
    TooLarge,
}

/// メモ保存先ファイルシステムの抽象。
///
/// サイズ上限の事前チェック等のドメイン責務は呼び出し側で行い、
/// 実読み取り量の上限は `read_with_limit` 側でも保証する。
/// `NotFound` 等の特殊エラー処理も呼び出し側で吸収する。
#[async_trait]
pub(crate) trait MemoFs: Send + Sync + std::fmt::Debug {
    /// パス存在確認。シンボリックリンク要素は呼び出し側で別途検査済み想定。
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool>;

    /// メタデータ取得（サイズ制限の一段目チェック用）
    async fn metadata(&self, path: &Path) -> std::io::Result<Metadata>;

    /// バイト列読み込み。TOCTOU 対策として実読み取り量を上限以下に制限する。
    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError>;

    /// 親ディレクトリを再帰的に作成（既存ならエラーを返さない）
    async fn create_dir_all(&self, path: &Path) -> std::io::Result<()>;

    /// バイト列書き込み（atomic は要求しない）
    async fn write(&self, path: &Path, content: &[u8]) -> std::io::Result<()>;

    /// ファイル削除。`NotFound` を含むエラーは透過する（呼び出し側で吸収）。
    async fn remove_file(&self, path: &Path) -> std::io::Result<()>;
}

/// 本番用 [`MemoFs`] 実装。`tokio::fs::*` を直接呼び出す。
#[derive(Debug, Default)]
pub(crate) struct TokioMemoFs;

#[async_trait]
impl MemoFs for TokioMemoFs {
    async fn try_exists(&self, path: &Path) -> std::io::Result<bool> {
        tokio::fs::try_exists(path).await
    }

    async fn metadata(&self, path: &Path) -> std::io::Result<Metadata> {
        tokio::fs::metadata(path).await
    }

    async fn read_with_limit(&self, path: &Path) -> Result<Vec<u8>, MemoReadError> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(MemoReadError::Open)?;
        read_bytes_with_limit(file)
            .await
            .map_err(|error| match error {
                ReadMarkdownError::Io(error) => MemoReadError::Read(error),
                ReadMarkdownError::TooLarge => MemoReadError::TooLarge,
                ReadMarkdownError::NotUtf8 => MemoReadError::Read(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "bounded memo read returned UTF-8 validation error",
                )),
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
