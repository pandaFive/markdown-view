/// ファイル監視エラーの詳細
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchError {
    kind: WatchErrorKind,
    detail: String,
}

impl WatchError {
    /// 初期化失敗エラーを生成する
    pub fn init(detail: impl Into<String>) -> Self {
        Self {
            kind: WatchErrorKind::Init,
            detail: detail.into(),
        }
    }

    /// notify系エラーを生成する
    pub fn notify(detail: impl Into<String>) -> Self {
        Self {
            kind: WatchErrorKind::Notify,
            detail: detail.into(),
        }
    }

    /// 監視スレッドpanicエラーを生成する
    pub fn thread_panic(detail: impl Into<String>) -> Self {
        Self {
            kind: WatchErrorKind::ThreadPanic,
            detail: detail.into(),
        }
    }

    /// 監視リソース枯渇エラーを生成する
    pub fn resource_exhausted(detail: impl Into<String>) -> Self {
        Self {
            kind: WatchErrorKind::ResourceExhausted,
            detail: detail.into(),
        }
    }

    /// 監視初期化中のnotifyエラーを分類して生成する
    pub fn from_watch_init_error(prefix: &str, error: &notify::Error) -> Self {
        let detail = format!("{prefix}: {error}");
        if is_watch_resource_exhausted(error) {
            Self::resource_exhausted(detail)
        } else {
            Self::init(detail)
        }
    }

    /// 監視登録中のnotifyエラーを分類して生成する
    pub fn from_watch_registration_error(prefix: &str, error: &notify::Error) -> Self {
        let detail = format!("{prefix}: {error}");
        if is_watch_resource_exhausted(error) {
            Self::resource_exhausted(detail)
        } else {
            Self::notify(detail)
        }
    }

    /// エラー種別を返す
    pub fn kind(&self) -> WatchErrorKind {
        self.kind
    }

    /// 補足詳細を返す
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// 利用者向けメッセージを返す
    pub fn user_message(&self) -> String {
        match self.kind {
            WatchErrorKind::Init => format!("監視の初期化に失敗しました: {}", self.detail),
            WatchErrorKind::Notify => {
                format!("通知ライブラリエラーが発生しました: {}", self.detail)
            }
            WatchErrorKind::ThreadPanic => {
                format!("監視スレッドがパニックで停止しました: {}", self.detail)
            }
            WatchErrorKind::ResourceExhausted => {
                format!(
                    "監視対象が多すぎるため監視を開始できません。Linuxではfs.inotify.max_user_watchesの上限を確認してください: {}",
                    self.detail
                )
            }
        }
    }
}

impl std::fmt::Display for WatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.user_message())
    }
}

impl std::error::Error for WatchError {}

/// ファイル監視エラーの分類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchErrorKind {
    /// 監視開始時の初期化失敗
    Init,
    /// notify / debouncer 起因の監視エラー
    Notify,
    /// 監視スレッド内のpanic
    ThreadPanic,
    /// OSのファイル監視リソース枯渇
    ResourceExhausted,
}

fn is_watch_resource_exhausted(error: &notify::Error) -> bool {
    match &error.kind {
        notify::ErrorKind::MaxFilesWatch => true,
        notify::ErrorKind::Io(io_error) => is_enospc_raw_os_error(io_error),
        notify::ErrorKind::Generic(message) => {
            message.contains("ENOSPC") || message.contains("No space left on device")
        }
        _ => {
            let message = error.to_string();
            message.contains("ENOSPC") || message.contains("No space left on device")
        }
    }
}

fn is_enospc_raw_os_error(io_error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        io_error.raw_os_error() == Some(28)
    }
    #[cfg(not(unix))]
    {
        let _ = io_error;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{WatchError, WatchErrorKind};

    #[test]
    fn test_watch_error_利用者向けメッセージが種別ごとに生成される() {
        let notify = WatchError::notify("notify詳細");
        let panic = WatchError::thread_panic("panic詳細");

        assert_eq!(
            notify.user_message(),
            "通知ライブラリエラーが発生しました: notify詳細"
        );
        assert_eq!(
            panic.user_message(),
            "監視スレッドがパニックで停止しました: panic詳細"
        );
    }

    #[test]
    fn test_watch_error_initの利用者向けメッセージが生成される() {
        let init = WatchError::init("初期化詳細");

        assert_eq!(
            init.user_message(),
            "監視の初期化に失敗しました: 初期化詳細"
        );
    }

    #[test]
    fn test_watch_error_kindアクセサが正しいwatcherrorkindを返す() {
        assert_eq!(WatchError::init("detail").kind(), WatchErrorKind::Init);
        assert_eq!(WatchError::notify("detail").kind(), WatchErrorKind::Notify);
        assert_eq!(
            WatchError::thread_panic("detail").kind(),
            WatchErrorKind::ThreadPanic
        );
    }

    #[test]
    fn test_watch_error_resource_exhaustedはinotify上限案内を含む() {
        let error = WatchError::resource_exhausted("OS file watch limit reached.");

        let message = error.user_message();

        assert_eq!(error.kind(), WatchErrorKind::ResourceExhausted);
        assert!(message.contains("監視対象が多すぎるため監視を開始できません"));
        assert!(message.contains("fs.inotify.max_user_watches"));
        assert!(message.contains("Linux"));
        assert!(message.contains("OS file watch limit reached."));
    }

    #[test]
    fn test_notify_error_max_files_watchはresource_exhaustedに変換される() {
        let notify_error = notify::Error::new(notify::ErrorKind::MaxFilesWatch);

        let error =
            WatchError::from_watch_init_error("ディレクトリ監視の開始に失敗", &notify_error);

        assert_eq!(error.kind(), WatchErrorKind::ResourceExhausted);
        assert!(error.detail().contains("ディレクトリ監視の開始に失敗"));
        assert!(error.detail().contains("OS file watch limit reached."));
    }

    #[test]
    #[cfg(unix)]
    fn test_notify_error_raw_os_error_28はresource_exhaustedに変換される() {
        let io_error = std::io::Error::from_raw_os_error(28);
        let notify_error = notify::Error::io(io_error);

        let error =
            WatchError::from_watch_init_error("ディレクトリ監視の開始に失敗", &notify_error);

        assert_eq!(error.kind(), WatchErrorKind::ResourceExhausted);
    }

    #[test]
    fn test_notify_error_通常エラーはinitに変換される() {
        let notify_error = notify::Error::generic("generic watch failure");

        let error =
            WatchError::from_watch_init_error("ディレクトリ監視の開始に失敗", &notify_error);

        assert_eq!(error.kind(), WatchErrorKind::Init);
        assert_eq!(
            error.user_message(),
            "監視の初期化に失敗しました: ディレクトリ監視の開始に失敗: generic watch failure"
        );
    }

    #[test]
    fn test_notify_error_動的watch追加のmax_files_watchはresource_exhaustedに変換される() {
        let notify_error = notify::Error::new(notify::ErrorKind::MaxFilesWatch);

        let error = WatchError::from_watch_registration_error(
            "新規ディレクトリの監視追加に失敗",
            &notify_error,
        );

        assert_eq!(error.kind(), WatchErrorKind::ResourceExhausted);
        assert!(error.detail().contains("新規ディレクトリの監視追加に失敗"));
    }

    #[test]
    fn test_notify_error_動的watch追加の通常エラーはnotifyに変換される() {
        let notify_error = notify::Error::generic("dynamic watch failure");

        let error = WatchError::from_watch_registration_error(
            "新規ディレクトリの監視追加に失敗",
            &notify_error,
        );

        assert_eq!(error.kind(), WatchErrorKind::Notify);
        assert_eq!(
            error.user_message(),
            "通知ライブラリエラーが発生しました: 新規ディレクトリの監視追加に失敗: dynamic watch failure"
        );
    }
}
