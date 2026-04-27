//! Markdownファイルの探索、検証、読み込み、描画を管理する。

mod catalog;
mod content;
mod memo;
mod memo_fs;
mod memo_sidecar;
mod resolve;
mod search;

#[cfg(test)]
mod tests;

pub use self::catalog::list_markdown_files;
pub use self::content::MAX_FILE_SIZE;
pub use self::resolve::{resolve_file, ResolveFileError};

pub(in crate::server) use self::content::{
    build_change_broadcast_message, build_lagged_recovery_message, load_initial_socket_update,
    load_route_update,
};
pub(in crate::server) use self::memo::{load_route_memo, save_route_memo};
pub(in crate::server) use self::resolve::{
    resolve_route_target, ResolvedTarget, RouteTargetRequest,
};
pub(in crate::server) use self::search::{search_directory, SearchResponse};
pub(in crate::server) use self::memo_fs::{MemoFs, TokioMemoFs};

#[cfg(test)]
pub(in crate::server) use self::resolve::RouteTargetKind;
