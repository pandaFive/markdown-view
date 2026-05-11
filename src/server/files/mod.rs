//! Markdownファイルの探索、検証、読み込み、描画を管理する。

use axum::http::StatusCode;

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

pub use self::catalog::list_markdown_files;
pub use self::content::MAX_FILE_SIZE;
pub use self::resolve::{resolve_file, ResolveFileError};

pub(in crate::server) use self::catalog::{list_markdown_files_from_canonical_base, MAX_FILE_LIST};
pub(in crate::server) use self::content::{
    build_change_broadcast_message, build_change_error_log_message_without_receivers,
    build_lagged_recovery_message, load_initial_socket_update, load_route_update,
};
pub(in crate::server) use self::memo::{load_route_memo, save_route_memo};
pub(in crate::server) use self::memo_fs::{MemoFs, TokioMemoFs};
pub(in crate::server) use self::resolve::{
    resolve_route_target, ResolvedTarget, RouteTargetRequest,
};
#[allow(unused_imports)]
pub(in crate::server) use self::search::{
    normalize_search_query, search_directory, SearchResponse, MAX_SEARCH_QUERY_CHARS,
};

pub(in crate::server) async fn run_blocking_file_task<T, F>(
    task_label: &'static str,
    task: F,
) -> Result<T, StatusCode>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(task).await.map_err(|error| {
        if error.is_panic() {
            tracing::error!(
                "[markdown-view] {}タスクがpanicしました: {}",
                task_label,
                error
            );
        } else {
            tracing::warn!(
                "[markdown-view] {}タスクのjoinエラー: {}",
                task_label,
                error
            );
        }
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

#[cfg(test)]
pub(in crate::server) use self::resolve::RouteTargetKind;
#[cfg(test)]
pub(in crate::server) use self::test_support::{MockMemoFs, Op};
