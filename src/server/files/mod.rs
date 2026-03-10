//! Markdownファイルの探索、検証、読み込み、描画を管理する。

mod catalog;
mod content;
mod resolve;

#[cfg(test)]
mod tests;

pub use self::catalog::list_markdown_files;
pub use self::content::MAX_FILE_SIZE;
pub use self::resolve::{resolve_file, ResolveFileError};

pub(in crate::server) use self::content::{
    build_change_broadcast_message, build_lagged_recovery_message, load_initial_socket_update,
    load_route_update,
};
pub(in crate::server) use self::resolve::{resolve_route_target, RouteTargetRequest};
