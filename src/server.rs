//! HTTPルーター、状態管理、ファイル解決、WebSocket通知を束ねる公開ファサード。

mod broadcast;
mod files;
mod guards;
pub(crate) mod log_path;
mod messages;
mod routes;
mod service;
mod session;
mod state;
mod watch;

pub use self::broadcast::notify_update;
pub use self::files::{list_markdown_files, resolve_file, ResolveFileError, MAX_FILE_SIZE};
pub use self::messages::BroadcastMessage;
pub use self::routes::create_router;
pub(crate) use self::state::CanonicalPath;
pub use self::state::{AppMode, AppModeBuildError, AppState, CanonicalPathError};
pub use self::watch::WatchService;
