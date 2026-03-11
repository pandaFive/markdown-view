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
}
