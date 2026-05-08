use std::ffi::OsStr;
use std::path::Path;

/// workspace内で生成物または内部管理領域として扱うパスの除外理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WorkspaceExclusionReason {
    Git,
    Hidden,
    NodeModules,
    Target,
}

/// ベースディレクトリからの相対パスが表示・検索・監視対象外か判定する。
pub(crate) fn exclusion_reason_for_relative_path(
    relative: &Path,
) -> Option<WorkspaceExclusionReason> {
    relative
        .components()
        .find_map(|component| exclusion_reason_for_name(component.as_os_str()))
}

/// 単一のファイル名またはディレクトリ名が除外対象か判定する。
pub(crate) fn exclusion_reason_for_name(name: &OsStr) -> Option<WorkspaceExclusionReason> {
    let name = name.to_string_lossy();
    match name.as_ref() {
        ".git" => Some(WorkspaceExclusionReason::Git),
        "node_modules" => Some(WorkspaceExclusionReason::NodeModules),
        "target" => Some(WorkspaceExclusionReason::Target),
        _ if name.starts_with('.') => Some(WorkspaceExclusionReason::Hidden),
        _ => None,
    }
}
