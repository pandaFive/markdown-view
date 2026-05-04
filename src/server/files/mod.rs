//! Markdownファイルの探索、検証、読み込み、描画を管理する。

mod catalog;
mod content;
mod memo;
mod memo_fs;
mod memo_sidecar;
mod resolve;
mod search;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;

use crate::server::CanonicalPath;

pub use self::catalog::list_markdown_files;
pub use self::content::MAX_FILE_SIZE;
pub use self::resolve::{resolve_file, ResolveFileError};

pub(in crate::server) async fn list_markdown_files_blocking(
    base_dir: &CanonicalPath,
) -> std::io::Result<Vec<String>> {
    let base_dir = base_dir.clone();
    tokio::task::spawn_blocking(move || {
        self::catalog::list_markdown_files_from_canonical_base(&base_dir)
    })
    .await
    .map_err(|error| {
        if error.is_panic() {
            tracing::error!(
                "[markdown-view] ファイル一覧取得タスクがpanicしました: {}",
                error
            );
        } else {
            tracing::warn!(
                "[markdown-view] ファイル一覧取得タスクのjoinエラー: {}",
                error
            );
        }
        std::io::Error::other(format!("ファイル一覧取得タスクのjoinエラー: {error}"))
    })?
}

pub(in crate::server) use self::content::{
    build_change_broadcast_message, build_change_error_log_message_without_receivers,
    build_lagged_recovery_message, load_initial_socket_update, load_route_update,
};
pub(in crate::server) use self::memo::{load_route_memo, save_route_memo};
pub(in crate::server) use self::memo_fs::{MemoFs, TokioMemoFs};
pub(in crate::server) use self::resolve::{
    resolve_route_target, ResolvedTarget, RouteTargetRequest,
};
pub(in crate::server) use self::search::{search_directory, SearchResponse};

#[cfg(test)]
pub(in crate::server) use self::resolve::RouteTargetKind;
#[cfg(test)]
pub(in crate::server) use self::test_support::{MockMemoFs, Op};
