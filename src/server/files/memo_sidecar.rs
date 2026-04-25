use std::ffi::OsStr;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use sha2::{Digest, Sha256};

const MEMO_SUFFIX: &str = ".memo.md";
const MAX_FILENAME_BYTES: usize = 255;
const SIDECAR_HASH_LEN: usize = 16;

/// メモ sidecar ファイル名生成の Single Source of Truth。
///
/// **不変条件**: 任意の `OsStr` 入力に対して、`as_str().len() <= MAX_FILENAME_BYTES (= 255)`
/// を構築時に保証する。UTF-8 経路の hash truncation、Unix 非 UTF-8 経路の `._bin.{hash}.memo.md`、
/// fallback の `.memo.md` がいずれも 255 bytes 以下に収まる。
/// 255 bytes は POSIX `NAME_MAX` と ext4 / xfs など主要なローカルファイルシステムの
/// 一般的なファイル名上限に合わせた保守的な上限値。
///
/// この不変条件は `src/server/files/tests.rs` の
/// `test_sidecar_name_任意入力で常に255バイト以下_不変条件_*` で固定される。
/// 呼び出し側でファイルシステム上限の重複チェックを行う必要はない。
pub(super) struct SidecarMemoName(String);

impl SidecarMemoName {
    /// 入力なし / 不明なファイル名向けの fallback 名 (`.memo.md`)。
    /// 出力は常に 8 bytes で、`MAX_FILENAME_BYTES` を超えない。
    pub(super) fn fallback() -> Self {
        Self(MEMO_SUFFIX.to_string())
    }

    /// `OsStr` ファイル名から sidecar 名を構築する。
    /// 出力は任意の入力に対して `MAX_FILENAME_BYTES (= 255)` bytes 以下を保証する。
    pub(super) fn from_file_name(file_name: &OsStr) -> Self {
        if let Some(name) = file_name.to_str() {
            if name.is_empty() {
                return Self::fallback();
            }
            return Self::from_utf8_name(name);
        }

        #[cfg(unix)]
        {
            Self(format!(
                "._bin.{}{}",
                short_hash(file_name.as_bytes()),
                MEMO_SUFFIX
            ))
        }

        #[cfg(not(unix))]
        {
            Self::fallback()
        }
    }

    /// 旧形式 (backslash を区切り正規化せずそのまま含む) の sidecar 名を再構築する。
    /// 出力は `MAX_FILENAME_BYTES (= 255)` bytes 以下を保証する。
    /// 入力に区切り正規化対象 (`\`) が含まれない場合、または `/` が含まれる場合は `None` を返す。
    #[cfg(unix)]
    pub(super) fn compat_from_file_name(file_name: &OsStr) -> Option<Self> {
        let name = file_name.to_str().filter(|name| !name.is_empty())?;
        if !name.contains('\\') || name.contains('/') {
            return None;
        }
        Some(Self(build_legacy_utf8_name(name)))
    }

    #[cfg(not(unix))]
    pub(super) fn compat_from_file_name(_file_name: &OsStr) -> Option<Self> {
        None
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    fn from_utf8_name(file_name: &str) -> Self {
        let normalized = normalize_visible_separators(file_name);
        let full = format!(".{normalized}{MEMO_SUFFIX}");
        if normalized == file_name && full.len() <= MAX_FILENAME_BYTES {
            return Self(full);
        }

        let hash = short_hash(file_name.as_bytes());
        let reserved = 1 + 1 + SIDECAR_HASH_LEN + MEMO_SUFFIX.len();
        let prefix_budget = MAX_FILENAME_BYTES.saturating_sub(reserved);
        let prefix = truncate_to_bytes(&normalized, prefix_budget);
        Self(format!(".{prefix}.{hash}{MEMO_SUFFIX}"))
    }
}

fn build_legacy_utf8_name(file_name: &str) -> String {
    let full = format!(".{file_name}{MEMO_SUFFIX}");
    if full.len() <= MAX_FILENAME_BYTES {
        return full;
    }

    let hash = short_hash(file_name.as_bytes());
    let reserved = 1 + 1 + SIDECAR_HASH_LEN + MEMO_SUFFIX.len();
    let prefix_budget = MAX_FILENAME_BYTES.saturating_sub(reserved);
    let prefix = truncate_to_bytes(file_name, prefix_budget);
    format!(".{prefix}.{hash}{MEMO_SUFFIX}")
}

fn normalize_visible_separators(file_name: &str) -> String {
    file_name.replace(['/', '\\'], "_")
}

fn short_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("{:x}", digest)[..SIDECAR_HASH_LEN].to_string()
}

fn truncate_to_bytes(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }

    let mut end = 0;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    &input[..end]
}
