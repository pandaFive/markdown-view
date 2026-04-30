use crate::template::MemoResponse;
use crate::template::UpdateMessage;

// 後続タスクでservice配線するまで一時的に未使用になるスケルトン。
#[allow(dead_code)]
pub(super) struct PageRequest<'a> {
    pub file: Option<&'a str>,
}

#[allow(dead_code)]
pub(super) struct ContentRequest<'a> {
    pub file: Option<&'a str>,
}

#[allow(dead_code)]
pub(super) struct MemoRequest<'a> {
    pub file: Option<&'a str>,
}

#[allow(dead_code)]
pub(super) struct SaveMemoRequest<'a> {
    pub file: Option<&'a str>,
    pub raw: String,
}

#[allow(dead_code)]
pub(super) struct PageView {
    pub title: String,
    pub update: UpdateMessage,
    pub memo: MemoResponse,
    pub sidebar: SidebarView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum SidebarView {
    SingleFile,
    Directory {
        directory_name: String,
        file_list: Vec<String>,
        current_file: Option<String>,
    },
}

#[allow(dead_code)]
impl SidebarView {
    fn single_file() -> Self {
        Self::SingleFile
    }

    fn directory(
        directory_name: impl Into<String>,
        file_list: Vec<String>,
        current_file: Option<&str>,
    ) -> Self {
        Self::Directory {
            directory_name: directory_name.into(),
            file_list,
            current_file: current_file.map(ToOwned::to_owned),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidebar_view_directoryは所有データを保持する() {
        let sidebar = SidebarView::directory(
            "docs",
            vec!["README.md".to_string(), "guide/setup.md".to_string()],
            Some("guide/setup.md"),
        );

        assert_eq!(
            sidebar,
            SidebarView::Directory {
                directory_name: "docs".to_string(),
                file_list: vec!["README.md".to_string(), "guide/setup.md".to_string()],
                current_file: Some("guide/setup.md".to_string()),
            }
        );
    }

    #[test]
    fn test_sidebar_view_single_fileを作れる() {
        assert_eq!(SidebarView::single_file(), SidebarView::SingleFile);
    }
}
