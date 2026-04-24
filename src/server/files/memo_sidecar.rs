use std::ffi::OsStr;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use sha2::{Digest, Sha256};

const MEMO_SUFFIX: &str = ".memo.md";
pub(super) const MAX_FILENAME_BYTES: usize = 255;
const SIDECAR_HASH_LEN: usize = 16;

pub(super) struct SidecarMemoName(String);

impl SidecarMemoName {
    pub(super) fn fallback() -> Self {
        Self(MEMO_SUFFIX.to_string())
    }

    pub(super) fn from_file_name(file_name: &OsStr) -> Self {
        if let Some(name) = file_name.to_str() {
            return Self::from_utf8_name(name);
        }

        #[cfg(unix)]
        {
            return Self(format!(
                "._bin.{}{}",
                short_hash(file_name.as_bytes()),
                MEMO_SUFFIX
            ));
        }

        #[cfg(not(unix))]
        {
            Self::fallback()
        }
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
