//! サーバー状態とモード判定を管理する。

use std::path::{Path, PathBuf};

use tokio::sync::broadcast;

use super::messages::BroadcastMessage;
use crate::renderer::syntax_theme_css;

/// canonicalize済みの絶対パス
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalPath(PathBuf);

impl CanonicalPath {
    /// パスをcanonicalizeして`CanonicalPath`を生成する
    pub fn try_from_path(path: impl AsRef<Path>) -> Result<Self, CanonicalPathError> {
        let canonical = path
            .as_ref()
            .canonicalize()
            .map_err(CanonicalPathError::Canonicalize)?;
        Ok(Self(canonical))
    }

    /// `Path`として参照する
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for CanonicalPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

/// `CanonicalPath` 生成エラー
#[derive(Debug)]
pub enum CanonicalPathError {
    /// canonicalize失敗
    Canonicalize(std::io::Error),
}

impl std::fmt::Display for CanonicalPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanonicalPathError::Canonicalize(e) => {
                write!(f, "パスの正規化に失敗しました: {}", e)
            }
        }
    }
}

impl std::error::Error for CanonicalPathError {}

/// `AppMode` 構築エラー
#[derive(Debug)]
pub enum AppModeBuildError {
    /// canonicalize済みパスの生成に失敗
    CanonicalPath(CanonicalPathError),
    /// 単一ファイルモードでファイル以外が指定された
    NotFile(PathBuf),
    /// ディレクトリモードでディレクトリ以外が指定された
    NotDirectory(PathBuf),
    /// 単一ファイルモードで.md以外が指定された
    NotMarkdown(PathBuf),
}

impl std::fmt::Display for AppModeBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppModeBuildError::CanonicalPath(e) => write!(f, "{}", e),
            AppModeBuildError::NotFile(path) => {
                write!(
                    f,
                    "単一ファイルモードにはファイルを指定してください: {}",
                    path.display()
                )
            }
            AppModeBuildError::NotDirectory(path) => {
                write!(
                    f,
                    "ディレクトリモードにはディレクトリを指定してください: {}",
                    path.display()
                )
            }
            AppModeBuildError::NotMarkdown(path) => {
                write!(f, ".mdファイルのみ指定可能です: {}", path.display())
            }
        }
    }
}

impl std::error::Error for AppModeBuildError {}

#[derive(Debug, Clone)]
enum AppModeKind {
    SingleFile(CanonicalPath),
    Directory(CanonicalPath),
}

/// アプリケーション動作モード
#[derive(Debug, Clone)]
pub struct AppMode(AppModeKind);

impl AppMode {
    /// 単一ファイルモードを生成する（canonicalize済み・.mdファイルのみ許可）
    pub fn new_single_file(path: impl AsRef<Path>) -> Result<Self, AppModeBuildError> {
        let canonical =
            CanonicalPath::try_from_path(path).map_err(AppModeBuildError::CanonicalPath)?;
        if !canonical.as_path().is_file() {
            return Err(AppModeBuildError::NotFile(
                canonical.as_path().to_path_buf(),
            ));
        }
        match canonical.as_path().extension() {
            Some(ext) if ext.eq_ignore_ascii_case("md") => {}
            _ => {
                return Err(AppModeBuildError::NotMarkdown(
                    canonical.as_path().to_path_buf(),
                ));
            }
        }
        Ok(Self(AppModeKind::SingleFile(canonical)))
    }

    /// ディレクトリモードを生成する（canonicalize済みディレクトリのみ許可）
    pub fn new_directory(path: impl AsRef<Path>) -> Result<Self, AppModeBuildError> {
        let canonical =
            CanonicalPath::try_from_path(path).map_err(AppModeBuildError::CanonicalPath)?;
        if !canonical.as_path().is_dir() {
            return Err(AppModeBuildError::NotDirectory(
                canonical.as_path().to_path_buf(),
            ));
        }
        Ok(Self(AppModeKind::Directory(canonical)))
    }

    /// ベースディレクトリを返す（ファイルモードは親、ディレクトリモードはそのまま）
    pub fn base_dir(&self) -> &Path {
        match &self.0 {
            AppModeKind::SingleFile(p) => p.as_path().parent().unwrap_or(p.as_path()),
            AppModeKind::Directory(p) => p.as_path(),
        }
    }

    /// 単一ファイルモードのパスを返す（ディレクトリモードはNone）
    pub fn single_file(&self) -> Option<&Path> {
        match &self.0 {
            AppModeKind::SingleFile(p) => Some(p.as_path()),
            AppModeKind::Directory(_) => None,
        }
    }

    /// ディレクトリモードのパスを返す（単一ファイルモードはNone）
    pub fn directory(&self) -> Option<&Path> {
        match &self.0 {
            AppModeKind::SingleFile(_) => None,
            AppModeKind::Directory(p) => Some(p.as_path()),
        }
    }

    /// ディレクトリモードかどうか
    pub fn is_directory(&self) -> bool {
        matches!(self.0, AppModeKind::Directory(_))
    }

    /// ディレクトリモード時にファイルの相対パスを計算する
    ///
    /// `file_path` はcanonicalize済みの絶対パスであること。
    /// strip_prefix失敗時はエラーログを出力してNoneを返す。
    /// 単一ファイルモードでは常にNoneを返す。
    pub fn relative_path_of(&self, file_path: &Path) -> Option<String> {
        match &self.0 {
            AppModeKind::Directory(base) => match file_path.strip_prefix(base.as_path()) {
                Ok(relative) => Some(relative.to_string_lossy().replace('\\', "/")),
                Err(_) => {
                    tracing::warn!(
                        "[markdown-view] 相対パス算出失敗: {} はベース {} の配下ではありません",
                        file_path.display(),
                        base.as_path().display()
                    );
                    None
                }
            },
            AppModeKind::SingleFile(_) => None,
        }
    }
}

/// サーバー共有状態
pub struct AppState {
    mode: AppMode,
    dark_mode: bool,
    syntax_css: String,
    tx: broadcast::Sender<BroadcastMessage>,
}

impl AppState {
    /// `AppState` を生成する
    pub fn new(
        mode: AppMode,
        dark_mode: bool,
        theme: Option<String>,
        tx: broadcast::Sender<BroadcastMessage>,
    ) -> Self {
        Self {
            syntax_css: syntax_theme_css(theme.as_deref()),
            mode,
            dark_mode,
            tx,
        }
    }

    /// 動作モードを返す
    pub fn mode(&self) -> &AppMode {
        &self.mode
    }

    /// ダークモード設定を返す
    pub fn dark_mode(&self) -> bool {
        self.dark_mode
    }

    /// 構文ハイライト用CSSを返す
    pub fn syntax_css(&self) -> &str {
        &self.syntax_css
    }

    /// broadcast送信チャネルを返す
    pub fn tx(&self) -> &broadcast::Sender<BroadcastMessage> {
        &self.tx
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use tokio::sync::broadcast;

    use super::*;

    #[test]
    fn test_app_mode_new_single_file() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");

        let mode = AppMode::new_single_file(&file_path).unwrap();
        let canonical = file_path.canonicalize().unwrap();

        assert_eq!(mode.base_dir(), canonical.parent().unwrap());
        assert_eq!(mode.single_file(), Some(canonical.as_path()));
        assert!(mode.directory().is_none());
    }

    #[test]
    fn test_app_mode_new_directory() {
        let dir = tempfile::tempdir().unwrap();

        let mode = AppMode::new_directory(dir.path()).unwrap();
        let canonical = dir.path().canonicalize().unwrap();

        assert_eq!(mode.base_dir(), canonical.as_path());
        assert!(mode.single_file().is_none());
        assert_eq!(mode.directory(), Some(canonical.as_path()));
    }

    #[test]
    fn test_app_mode_relative_path_of_ディレクトリモード() {
        let dir = create_test_dir();
        let canonical = dir.path().canonicalize().unwrap();
        let mode = AppMode::new_directory(dir.path()).unwrap();
        let file_path = canonical.join("docs/api.md");
        assert_eq!(
            mode.relative_path_of(&file_path),
            Some("docs/api.md".to_string())
        );
    }

    #[test]
    fn test_app_mode_relative_path_of_単一ファイルモードはnone() {
        let (_dir, file_path) = create_markdown_fixture("test.md", "# test");

        let mode = AppMode::new_single_file(&file_path).unwrap();
        let canonical = file_path.canonicalize().unwrap();
        assert_eq!(mode.relative_path_of(&canonical), None);
    }

    #[test]
    fn test_app_mode_new_directory_存在しないパスは拒否() {
        let result = AppMode::new_directory("/nonexistent/path/that/does/not/exist");
        assert!(matches!(result, Err(AppModeBuildError::CanonicalPath(_))));
    }

    #[test]
    fn test_app_mode_new_single_file_非mdは拒否() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "test").unwrap();

        let result = AppMode::new_single_file(&file_path);
        assert!(matches!(result, Err(AppModeBuildError::NotMarkdown(_))));
    }

    #[test]
    fn test_app_mode_new_single_file_ディレクトリ指定は拒否() {
        let dir = tempfile::tempdir().unwrap();
        let result = AppMode::new_single_file(dir.path());
        assert!(matches!(result, Err(AppModeBuildError::NotFile(_))));
    }

    #[test]
    fn test_app_mode_new_directory_ファイル指定は拒否() {
        let (_dir, file_path) = create_markdown_fixture("note.md", "# note");
        let result = AppMode::new_directory(&file_path);
        assert!(matches!(result, Err(AppModeBuildError::NotDirectory(_))));
    }

    fn create_markdown_fixture(name: &str, content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    fn create_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# README").unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("docs/api.md"), "# API").unwrap();
        dir
    }

    #[allow(dead_code)]
    fn create_single_file_state(file_path: &Path) -> AppState {
        let (tx, _rx) = broadcast::channel(16);
        AppState::new(
            AppMode::new_single_file(file_path).unwrap(),
            false,
            None,
            tx,
        )
    }
}
