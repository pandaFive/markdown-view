//! HTTPルーター、状態管理、ファイル解決、WebSocket通知を束ねる公開ファサード。

mod files;
mod guards;
mod messages;
mod routes;
mod state;
mod websocket;

pub use self::messages::{BroadcastMessage, MAX_FILE_SIZE};
pub use self::routes::create_router;
pub use self::state::{AppMode, AppState};
pub use self::websocket::notify_update;
