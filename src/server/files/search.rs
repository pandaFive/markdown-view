use std::collections::VecDeque;
use std::io::Read;
use std::ops::Range;
use std::path::{Component, Path};

use cap_std::fs::Dir;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use tokio::sync::OwnedSemaphorePermit;

use super::catalog::{
    list_markdown_files_from_verified_base_until_cancelled, open_relative_file_nofollow,
    open_verified_base_dir,
};
use super::content::MAX_FILE_SIZE;
use crate::markdown::{markdown_options, MarkdownProfile};
use crate::server::{CanonicalPath, SearchGeneration};
use crate::workspace_exclusion::exclusion_reason_for_relative_path;

const MAX_SEARCH_RESULTS: usize = 100;
const MAX_SEARCH_FILES: usize = 1000;
const MAX_SEARCH_BYTES: usize = 64 * 1024 * 1024;
const MAX_SEARCH_CONTEXT_CHARS: usize = 800;
const SEARCH_CONTEXT_ELLIPSIS: &str = "...";
const CASE_FOLD_CANCEL_CHECK_CHARS: usize = 1024;
const LARGE_SEARCH_BLOCK_BYTES: usize = 64 * 1024;
pub(in crate::server) const MAX_SEARCH_QUERY_CHARS: usize = 256;
const SEARCH_QUERY_TOO_LONG_MESSAGE: &str = "検索クエリが長すぎます";

#[cfg(test)]
type SearchProgressHook = std::sync::Arc<dyn Fn(&str, usize) + Send + Sync + 'static>;

#[cfg(test)]
type SearchListingProgressHook = std::sync::Arc<dyn Fn(&str) + Send + Sync + 'static>;

#[cfg(test)]
static SEARCH_PROGRESS_HOOK: std::sync::OnceLock<std::sync::Mutex<Option<SearchProgressHook>>> =
    std::sync::OnceLock::new();

#[cfg(test)]
static SEARCH_LISTING_PROGRESS_HOOK: std::sync::OnceLock<
    std::sync::Mutex<Option<SearchListingProgressHook>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
pub(in crate::server) struct SearchProgressHookGuard;

#[cfg(test)]
pub(in crate::server) struct SearchListingProgressHookGuard;

#[cfg(test)]
impl Drop for SearchProgressHookGuard {
    fn drop(&mut self) {
        let hook = SEARCH_PROGRESS_HOOK.get_or_init(|| std::sync::Mutex::new(None));
        *hook.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[cfg(test)]
impl Drop for SearchListingProgressHookGuard {
    fn drop(&mut self) {
        let hook = SEARCH_LISTING_PROGRESS_HOOK.get_or_init(|| std::sync::Mutex::new(None));
        *hook.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[cfg(test)]
pub(in crate::server) fn set_search_progress_hook_for_test(
    hook: SearchProgressHook,
) -> SearchProgressHookGuard {
    let slot = SEARCH_PROGRESS_HOOK.get_or_init(|| std::sync::Mutex::new(None));
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    SearchProgressHookGuard
}

#[cfg(test)]
pub(in crate::server) fn set_search_listing_progress_hook_for_test(
    hook: SearchListingProgressHook,
) -> SearchListingProgressHookGuard {
    let slot = SEARCH_LISTING_PROGRESS_HOOK.get_or_init(|| std::sync::Mutex::new(None));
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    SearchListingProgressHookGuard
}

#[cfg(test)]
fn notify_search_progress_for_test(relative: &str, searched_files: usize) {
    let hook = SEARCH_PROGRESS_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(hook) = hook {
        hook(relative, searched_files);
    }
}

#[cfg(not(test))]
fn notify_search_progress_for_test(_relative: &str, _searched_files: usize) {}

#[cfg(test)]
fn notify_search_listing_progress_for_test(relative: &str) {
    let hook = SEARCH_LISTING_PROGRESS_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(hook) = hook {
        hook(relative);
    }
}

#[cfg(not(test))]
fn notify_search_listing_progress_for_test(_relative: &str) {}

#[cfg(test)]
type SearchContextBuildHook = Box<dyn FnMut(usize)>;

#[cfg(test)]
type SearchBlockExtractHook = Box<dyn FnMut(usize)>;

#[cfg(test)]
type SearchBeforeResponseHook = Box<dyn FnMut()>;

#[cfg(test)]
type SearchLargeBlockFindHook = Box<dyn FnMut(usize)>;

#[cfg(test)]
type SearchAfterResolveHook = Box<dyn FnMut(&str)>;

#[cfg(test)]
type SearchAfterCanonicalizeHook = Box<dyn FnMut(&str)>;

#[cfg(test)]
type SearchAfterBaseIdentityValidationHook = Box<dyn FnMut()>;

#[cfg(test)]
type SearchAfterByteLimitHook = Box<dyn FnMut()>;

#[cfg(test)]
type SearchAfterMetadataHook = Box<dyn FnMut(&str)>;

#[cfg(test)]
std::thread_local! {
    static SEARCH_CONTEXT_BUILD_COUNT_FOR_TEST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SEARCH_CONTEXT_BUILD_HOOK_FOR_TEST: std::cell::RefCell<Option<SearchContextBuildHook>> =
        const { std::cell::RefCell::new(None) };
    static SEARCH_BLOCK_EXTRACT_COUNT_FOR_TEST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SEARCH_BLOCK_EXTRACT_HOOK_FOR_TEST: std::cell::RefCell<Option<SearchBlockExtractHook>> =
        const { std::cell::RefCell::new(None) };
    static SEARCH_BEFORE_RESPONSE_HOOK_FOR_TEST: std::cell::RefCell<Option<SearchBeforeResponseHook>> =
        const { std::cell::RefCell::new(None) };
    static SEARCH_CASE_FOLD_CHAR_COUNT_FOR_TEST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SEARCH_LARGE_BLOCK_FIND_HOOK_FOR_TEST: std::cell::RefCell<Option<SearchLargeBlockFindHook>> =
        const { std::cell::RefCell::new(None) };
    static SEARCH_AFTER_RESOLVE_HOOK_FOR_TEST: std::cell::RefCell<Option<SearchAfterResolveHook>> =
        const { std::cell::RefCell::new(None) };
    static SEARCH_AFTER_CANONICALIZE_HOOK_FOR_TEST: std::cell::RefCell<Option<SearchAfterCanonicalizeHook>> =
        const { std::cell::RefCell::new(None) };
    static SEARCH_AFTER_BASE_IDENTITY_VALIDATION_HOOK_FOR_TEST: std::cell::RefCell<Option<SearchAfterBaseIdentityValidationHook>> =
        const { std::cell::RefCell::new(None) };
    static SEARCH_AFTER_BYTE_LIMIT_HOOK_FOR_TEST: std::cell::RefCell<Option<SearchAfterByteLimitHook>> =
        const { std::cell::RefCell::new(None) };
    static SEARCH_AFTER_METADATA_HOOK_FOR_TEST: std::cell::RefCell<Option<SearchAfterMetadataHook>> =
        const { std::cell::RefCell::new(None) };
    static SEARCH_CLIP_SCAN_BYTES_FOR_TEST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SEARCH_MARKDOWN_READ_COUNT_FOR_TEST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_search_context_build_count_for_test() -> usize {
    SEARCH_CONTEXT_BUILD_COUNT_FOR_TEST.with(|count| {
        count.set(0);
        count.get()
    })
}

#[cfg(test)]
fn search_context_build_count_for_test() -> usize {
    SEARCH_CONTEXT_BUILD_COUNT_FOR_TEST.with(std::cell::Cell::get)
}

#[cfg(test)]
fn reset_search_block_extract_count_for_test() -> usize {
    SEARCH_BLOCK_EXTRACT_COUNT_FOR_TEST.with(|count| {
        count.set(0);
        count.get()
    })
}

#[cfg(test)]
fn search_block_extract_count_for_test() -> usize {
    SEARCH_BLOCK_EXTRACT_COUNT_FOR_TEST.with(std::cell::Cell::get)
}

#[cfg(test)]
fn reset_search_markdown_read_count_for_test() -> usize {
    SEARCH_MARKDOWN_READ_COUNT_FOR_TEST.with(|count| {
        count.set(0);
        count.get()
    })
}

#[cfg(test)]
fn search_markdown_read_count_for_test() -> usize {
    SEARCH_MARKDOWN_READ_COUNT_FOR_TEST.with(std::cell::Cell::get)
}

#[cfg(test)]
fn notify_search_markdown_read_for_test() {
    SEARCH_MARKDOWN_READ_COUNT_FOR_TEST.with(|count| count.set(count.get() + 1));
}

#[cfg(not(test))]
fn notify_search_markdown_read_for_test() {}

#[cfg(test)]
struct SearchContextBuildHookGuard;

#[cfg(test)]
impl Drop for SearchContextBuildHookGuard {
    fn drop(&mut self) {
        SEARCH_CONTEXT_BUILD_HOOK_FOR_TEST.with(|hook| {
            *hook.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
fn set_search_context_build_hook_for_test(
    hook: impl FnMut(usize) + 'static,
) -> SearchContextBuildHookGuard {
    SEARCH_CONTEXT_BUILD_HOOK_FOR_TEST.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    SearchContextBuildHookGuard
}

#[cfg(test)]
fn notify_search_context_build_for_test() {
    let next_count = SEARCH_CONTEXT_BUILD_COUNT_FOR_TEST.with(|count| {
        let next_count = count.get() + 1;
        count.set(next_count);
        next_count
    });
    SEARCH_CONTEXT_BUILD_HOOK_FOR_TEST.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(next_count);
        }
    });
}

#[cfg(not(test))]
fn notify_search_context_build_for_test() {}

#[cfg(test)]
struct SearchBlockExtractHookGuard;

#[cfg(test)]
impl Drop for SearchBlockExtractHookGuard {
    fn drop(&mut self) {
        SEARCH_BLOCK_EXTRACT_HOOK_FOR_TEST.with(|hook| {
            *hook.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
fn set_search_block_extract_hook_for_test(
    hook: impl FnMut(usize) + 'static,
) -> SearchBlockExtractHookGuard {
    SEARCH_BLOCK_EXTRACT_COUNT_FOR_TEST.with(|count| count.set(0));
    SEARCH_BLOCK_EXTRACT_HOOK_FOR_TEST.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    SearchBlockExtractHookGuard
}

#[cfg(test)]
fn notify_search_block_extract_for_test() {
    let next_count = SEARCH_BLOCK_EXTRACT_COUNT_FOR_TEST.with(|count| {
        let next_count = count.get() + 1;
        count.set(next_count);
        next_count
    });
    SEARCH_BLOCK_EXTRACT_HOOK_FOR_TEST.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(next_count);
        }
    });
}

#[cfg(not(test))]
fn notify_search_block_extract_for_test() {}

#[cfg(test)]
struct SearchBeforeResponseHookGuard;

#[cfg(test)]
impl Drop for SearchBeforeResponseHookGuard {
    fn drop(&mut self) {
        SEARCH_BEFORE_RESPONSE_HOOK_FOR_TEST.with(|hook| {
            *hook.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
fn set_search_before_response_hook_for_test(
    hook: impl FnMut() + 'static,
) -> SearchBeforeResponseHookGuard {
    SEARCH_BEFORE_RESPONSE_HOOK_FOR_TEST.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    SearchBeforeResponseHookGuard
}

#[cfg(test)]
fn notify_search_before_response_for_test() {
    SEARCH_BEFORE_RESPONSE_HOOK_FOR_TEST.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
}

#[cfg(not(test))]
fn notify_search_before_response_for_test() {}

#[cfg(test)]
struct SearchAfterBaseIdentityValidationHookGuard;

#[cfg(test)]
impl Drop for SearchAfterBaseIdentityValidationHookGuard {
    fn drop(&mut self) {
        SEARCH_AFTER_BASE_IDENTITY_VALIDATION_HOOK_FOR_TEST.with(|hook| {
            *hook.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
fn set_search_after_base_identity_validation_hook_for_test(
    hook: impl FnMut() + 'static,
) -> SearchAfterBaseIdentityValidationHookGuard {
    SEARCH_AFTER_BASE_IDENTITY_VALIDATION_HOOK_FOR_TEST.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    SearchAfterBaseIdentityValidationHookGuard
}

#[cfg(test)]
fn notify_search_after_base_identity_validation_for_test() {
    SEARCH_AFTER_BASE_IDENTITY_VALIDATION_HOOK_FOR_TEST.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
}

#[cfg(not(test))]
fn notify_search_after_base_identity_validation_for_test() {}

#[cfg(test)]
struct SearchAfterByteLimitHookGuard;

#[cfg(test)]
impl Drop for SearchAfterByteLimitHookGuard {
    fn drop(&mut self) {
        SEARCH_AFTER_BYTE_LIMIT_HOOK_FOR_TEST.with(|hook| {
            *hook.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
fn set_search_after_byte_limit_hook_for_test(
    hook: impl FnMut() + 'static,
) -> SearchAfterByteLimitHookGuard {
    SEARCH_AFTER_BYTE_LIMIT_HOOK_FOR_TEST.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    SearchAfterByteLimitHookGuard
}

#[cfg(test)]
fn notify_search_after_byte_limit_for_test() {
    SEARCH_AFTER_BYTE_LIMIT_HOOK_FOR_TEST.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
}

#[cfg(not(test))]
fn notify_search_after_byte_limit_for_test() {}

#[cfg(test)]
struct SearchAfterMetadataHookGuard;

#[cfg(test)]
impl Drop for SearchAfterMetadataHookGuard {
    fn drop(&mut self) {
        SEARCH_AFTER_METADATA_HOOK_FOR_TEST.with(|hook| {
            *hook.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
fn set_search_after_metadata_hook_for_test(
    hook: impl FnMut(&str) + 'static,
) -> SearchAfterMetadataHookGuard {
    SEARCH_AFTER_METADATA_HOOK_FOR_TEST.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    SearchAfterMetadataHookGuard
}

#[cfg(test)]
fn notify_search_after_metadata_for_test(relative: &str) {
    SEARCH_AFTER_METADATA_HOOK_FOR_TEST.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(relative);
        }
    });
}

#[cfg(not(test))]
fn notify_search_after_metadata_for_test(_relative: &str) {}

#[cfg(test)]
struct SearchAfterResolveHookGuard;

#[cfg(test)]
impl Drop for SearchAfterResolveHookGuard {
    fn drop(&mut self) {
        SEARCH_AFTER_RESOLVE_HOOK_FOR_TEST.with(|hook| {
            *hook.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
struct SearchAfterCanonicalizeHookGuard;

#[cfg(test)]
impl Drop for SearchAfterCanonicalizeHookGuard {
    fn drop(&mut self) {
        SEARCH_AFTER_CANONICALIZE_HOOK_FOR_TEST.with(|hook| {
            *hook.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
fn set_search_after_resolve_hook_for_test(
    hook: impl FnMut(&str) + 'static,
) -> SearchAfterResolveHookGuard {
    SEARCH_AFTER_RESOLVE_HOOK_FOR_TEST.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    SearchAfterResolveHookGuard
}

#[cfg(test)]
fn set_search_after_canonicalize_hook_for_test(
    hook: impl FnMut(&str) + 'static,
) -> SearchAfterCanonicalizeHookGuard {
    SEARCH_AFTER_CANONICALIZE_HOOK_FOR_TEST.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    SearchAfterCanonicalizeHookGuard
}

#[cfg(test)]
fn notify_search_after_resolve_for_test(relative: &str) {
    SEARCH_AFTER_RESOLVE_HOOK_FOR_TEST.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(relative);
        }
    });
}

#[cfg(not(test))]
fn notify_search_after_resolve_for_test(_relative: &str) {}

#[cfg(test)]
fn notify_search_after_canonicalize_for_test(relative: &str) {
    SEARCH_AFTER_CANONICALIZE_HOOK_FOR_TEST.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(relative);
        }
    });
}

#[cfg(not(test))]
fn notify_search_after_canonicalize_for_test(_relative: &str) {}

#[cfg(test)]
fn reset_search_case_fold_char_count_for_test() {
    SEARCH_CASE_FOLD_CHAR_COUNT_FOR_TEST.with(|count| count.set(0));
}

#[cfg(test)]
fn search_case_fold_char_count_for_test() -> usize {
    SEARCH_CASE_FOLD_CHAR_COUNT_FOR_TEST.with(std::cell::Cell::get)
}

#[cfg(test)]
fn notify_search_case_fold_char_for_test() {
    SEARCH_CASE_FOLD_CHAR_COUNT_FOR_TEST.with(|count| count.set(count.get() + 1));
}

#[cfg(not(test))]
fn notify_search_case_fold_char_for_test() {}

#[cfg(test)]
struct SearchLargeBlockFindHookGuard;

#[cfg(test)]
impl Drop for SearchLargeBlockFindHookGuard {
    fn drop(&mut self) {
        SEARCH_LARGE_BLOCK_FIND_HOOK_FOR_TEST.with(|hook| {
            *hook.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
fn set_search_large_block_find_hook_for_test(
    hook: impl FnMut(usize) + 'static,
) -> SearchLargeBlockFindHookGuard {
    SEARCH_LARGE_BLOCK_FIND_HOOK_FOR_TEST.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    SearchLargeBlockFindHookGuard
}

#[cfg(test)]
fn notify_search_large_block_find_for_test(search_bytes: usize) {
    SEARCH_LARGE_BLOCK_FIND_HOOK_FOR_TEST.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(search_bytes);
        }
    });
}

#[cfg(not(test))]
fn notify_search_large_block_find_for_test(_search_bytes: usize) {}

#[cfg(test)]
fn reset_search_clip_scan_bytes_for_test() -> usize {
    SEARCH_CLIP_SCAN_BYTES_FOR_TEST.with(|bytes| {
        bytes.set(0);
        bytes.get()
    })
}

#[cfg(test)]
fn search_clip_scan_bytes_for_test() -> usize {
    SEARCH_CLIP_SCAN_BYTES_FOR_TEST.with(std::cell::Cell::get)
}

#[cfg(test)]
fn notify_search_clip_scan_for_test(bytes: usize) {
    SEARCH_CLIP_SCAN_BYTES_FOR_TEST.with(|scanned| {
        scanned.set(scanned.get() + bytes);
    });
}

#[cfg(not(test))]
fn notify_search_clip_scan_for_test(_bytes: usize) {}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(in crate::server) struct SearchLimits {
    pub(in crate::server) max_results: usize,
    pub(in crate::server) max_files: usize,
    pub(in crate::server) max_bytes: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_results: MAX_SEARCH_RESULTS,
            max_files: MAX_SEARCH_FILES,
            max_bytes: MAX_SEARCH_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(in crate::server) enum SearchTruncationReason {
    #[serde(rename = "result_limit")]
    Result,
    #[serde(rename = "file_limit")]
    File,
    #[serde(rename = "byte_limit")]
    Byte,
}

#[derive(Debug, Clone)]
struct SearchStats {
    searched_files: usize,
    skipped_files: usize,
    searched_bytes: usize,
    budgeted_bytes: usize,
    truncated_reasons: Vec<SearchTruncationReason>,
}

impl SearchStats {
    fn new() -> Self {
        Self {
            searched_files: 0,
            skipped_files: 0,
            searched_bytes: 0,
            budgeted_bytes: 0,
            truncated_reasons: Vec::new(),
        }
    }

    fn mark_truncated(&mut self, reason: SearchTruncationReason) {
        if !self.truncated_reasons.contains(&reason) {
            self.truncated_reasons.push(reason);
        }
    }

    fn truncated(&self) -> bool {
        !self.truncated_reasons.is_empty()
    }

    fn clear_truncation(&mut self) {
        self.truncated_reasons.clear();
    }
}

#[derive(Debug, Clone)]
pub(in crate::server) struct SearchCancellation {
    generation: Option<SearchGeneration>,
    #[cfg(test)]
    cancel_after_files_for_test: Option<usize>,
    #[cfg(test)]
    cancel_after_reads_for_test: Option<usize>,
}

impl SearchCancellation {
    pub(in crate::server) fn new(generation: SearchGeneration) -> Self {
        Self {
            generation: Some(generation),
            #[cfg(test)]
            cancel_after_files_for_test: None,
            #[cfg(test)]
            cancel_after_reads_for_test: None,
        }
    }

    pub(in crate::server) fn none() -> Self {
        Self {
            generation: None,
            #[cfg(test)]
            cancel_after_files_for_test: None,
            #[cfg(test)]
            cancel_after_reads_for_test: None,
        }
    }

    fn is_cancelled(&self) -> bool {
        self.generation
            .as_ref()
            .is_some_and(SearchGeneration::is_stale)
    }

    fn is_cancelled_after_files(&self, searched_files: usize) -> bool {
        #[cfg(test)]
        {
            self.cancel_after_files_for_test
                .is_some_and(|limit| searched_files >= limit)
        }

        #[cfg(not(test))]
        {
            let _ = searched_files;
            false
        }
    }

    fn is_cancelled_after_read(&self, read_files: usize) -> bool {
        #[cfg(test)]
        {
            self.cancel_after_reads_for_test
                .is_some_and(|limit| read_files >= limit)
        }

        #[cfg(not(test))]
        {
            let _ = read_files;
            false
        }
    }

    #[cfg(test)]
    fn cancelled_for_test() -> Self {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };

        let current = Arc::new(AtomicU64::new(1));
        let generation = SearchGeneration::new(1, Arc::clone(&current));
        current.store(2, Ordering::Release);
        Self::new(generation)
    }

    #[cfg(test)]
    fn cancel_after_files_for_test(limit: usize) -> Self {
        Self {
            generation: None,
            cancel_after_files_for_test: Some(limit),
            cancel_after_reads_for_test: None,
        }
    }

    #[cfg(test)]
    fn cancel_after_reads_for_test(limit: usize) -> Self {
        Self {
            generation: None,
            cancel_after_files_for_test: None,
            cancel_after_reads_for_test: Some(limit),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(in crate::server) struct SearchResultItem {
    pub(in crate::server) file: String,
    pub(in crate::server) file_match_index: usize,
    pub(in crate::server) before: String,
    pub(in crate::server) current: String,
    pub(in crate::server) after: String,
}

impl SearchResultItem {
    fn new(
        file: String,
        file_match_index: usize,
        before: String,
        current: String,
        after: String,
    ) -> Self {
        Self {
            file,
            file_match_index,
            before,
            current,
            after,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(in crate::server) struct SearchResponse {
    pub(in crate::server) query: String,
    pub(in crate::server) results: Vec<SearchResultItem>,
    pub(in crate::server) searched_files: usize,
    pub(in crate::server) skipped_files: usize,
    pub(in crate::server) truncated: bool,
    pub(in crate::server) truncated_reasons: Vec<SearchTruncationReason>,
    pub(in crate::server) limits: SearchLimits,
    pub(in crate::server) searched_bytes: usize,
}

impl SearchResponse {
    pub(in crate::server) fn empty(query: String) -> Self {
        Self::from_parts(
            query,
            Vec::new(),
            SearchLimits::default(),
            SearchStats::new(),
        )
    }

    fn from_parts(
        query: String,
        results: Vec<SearchResultItem>,
        limits: SearchLimits,
        stats: SearchStats,
    ) -> Self {
        Self {
            query,
            results,
            searched_files: stats.searched_files,
            skipped_files: stats.skipped_files,
            truncated: stats.truncated(),
            truncated_reasons: stats.truncated_reasons,
            limits,
            searched_bytes: stats.searched_bytes,
        }
    }
}

#[derive(Debug, Clone)]
struct SearchBlockEntry {
    text: String,
    sentences: Vec<Range<usize>>,
}

#[derive(Debug)]
enum SearchMarkdownRead {
    Markdown {
        markdown: String,
        bytes_read: usize,
    },
    Skipped {
        error: std::io::Error,
        bytes_read: usize,
    },
    ByteLimit,
}

#[derive(Debug, Clone)]
struct SearchContext {
    before: String,
    current: String,
    after: String,
}

pub(in crate::server) fn normalize_search_query(raw_query: &str) -> std::io::Result<String> {
    let query = raw_query.trim();
    if query.chars().count() > MAX_SEARCH_QUERY_CHARS {
        return Err(search_query_too_long_error());
    }
    Ok(query.to_string())
}

fn search_query_too_long_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        SEARCH_QUERY_TOO_LONG_MESSAGE,
    )
}

/// ディレクトリ内のMarkdownを横断検索する。
pub(in crate::server) async fn search_directory(
    base_dir: &CanonicalPath,
    raw_query: &str,
    cancellation: SearchCancellation,
    permit: Option<OwnedSemaphorePermit>,
) -> std::io::Result<SearchResponse> {
    let query = normalize_search_query(raw_query)?;
    let base_dir = base_dir.clone();

    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        search_directory_blocking(&base_dir, &query, cancellation)
    })
    .await
    .map_err(map_search_join_error)?
}

fn search_directory_blocking(
    base_dir: &CanonicalPath,
    raw_query: &str,
    cancellation: SearchCancellation,
) -> std::io::Result<SearchResponse> {
    search_directory_with_limits_blocking(
        base_dir,
        raw_query,
        SearchLimits::default(),
        cancellation,
    )
}

fn search_directory_with_limits_blocking(
    base_dir: &CanonicalPath,
    raw_query: &str,
    limits: SearchLimits,
    cancellation: SearchCancellation,
) -> std::io::Result<SearchResponse> {
    let query = normalize_search_query(raw_query)?;
    if query.is_empty() {
        return Ok(SearchResponse::empty(query));
    }

    if cancellation.is_cancelled() {
        return Ok(SearchResponse::from_parts(
            query,
            Vec::new(),
            limits,
            SearchStats::new(),
        ));
    }

    let search_base_dir = open_verified_base_dir(base_dir, "検索base directory")?;
    notify_search_after_base_identity_validation_for_test();
    validate_search_base_path_identity(base_dir)?;

    let catalog = list_markdown_files_from_verified_base_until_cancelled(
        &search_base_dir,
        base_dir.as_path(),
        limits.max_files.saturating_add(1),
        &|| cancellation.is_cancelled(),
        &|relative| {
            let relative = path_to_search_relative(relative);
            notify_search_listing_progress_for_test(&relative);
        },
    )?;
    let files = catalog.files;
    let mut results = Vec::new();
    let mut stats = SearchStats::new();
    let mut read_files = 0usize;

    if catalog.truncated || files.len() > limits.max_files {
        stats.mark_truncated(SearchTruncationReason::File);
    }

    for relative in files.into_iter().take(limits.max_files) {
        if cancellation.is_cancelled() {
            results.clear();
            break;
        }
        if cancellation.is_cancelled_after_files(stats.searched_files) {
            break;
        }
        if results.len() >= limits.max_results {
            stats.mark_truncated(SearchTruncationReason::Result);
            break;
        }

        notify_search_after_resolve_for_test(&relative);
        let markdown = match read_search_markdown_with_byte_budget(
            &search_base_dir,
            &relative,
            stats.budgeted_bytes,
            limits.max_bytes,
        ) {
            Ok(SearchMarkdownRead::ByteLimit) => {
                notify_search_after_byte_limit_for_test();
                if cancellation.is_cancelled() {
                    results.clear();
                    break;
                }
                tracing::debug!(
                    "[markdown-view] ディレクトリ検索がbyte-limitに到達しました: file={} searched_bytes={} budgeted_bytes={} max_bytes={} result_count={}",
                    log_safe_search_relative(&relative),
                    stats.searched_bytes,
                    stats.budgeted_bytes,
                    limits.max_bytes,
                    results.len()
                );
                stats.mark_truncated(SearchTruncationReason::Byte);
                break;
            }
            Ok(SearchMarkdownRead::Markdown {
                markdown,
                bytes_read,
            }) => {
                stats.budgeted_bytes = stats.budgeted_bytes.saturating_add(bytes_read);
                markdown
            }
            Ok(SearchMarkdownRead::Skipped { error, bytes_read }) => {
                stats.budgeted_bytes = stats.budgeted_bytes.saturating_add(bytes_read);
                tracing::warn!(
                    "[markdown-view] 検索対象ファイル読込失敗（スキップ）: {} ({})",
                    log_safe_search_relative(&relative),
                    error
                );
                stats.skipped_files += 1;
                continue;
            }
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] 検索対象ファイル読込失敗（スキップ）: {} ({})",
                    log_safe_search_relative(&relative),
                    error
                );
                stats.skipped_files += 1;
                continue;
            }
        };
        read_files += 1;
        if cancellation.is_cancelled() {
            results.clear();
            break;
        }
        if cancellation.is_cancelled_after_read(read_files) {
            break;
        }

        if stats.searched_bytes.saturating_add(markdown.len()) > limits.max_bytes {
            stats.mark_truncated(SearchTruncationReason::Byte);
            break;
        }

        stats.searched_files += 1;
        stats.searched_bytes += markdown.len();
        notify_search_progress_for_test(&relative, stats.searched_files);
        if cancellation.is_cancelled() {
            results.clear();
            break;
        }
        let remaining_results = limits.max_results.saturating_sub(results.len());
        if remaining_results == 0 {
            stats.mark_truncated(SearchTruncationReason::Result);
            break;
        }
        let Some(file_search) = search_file_streaming_blocks(
            &relative,
            &markdown,
            &query,
            remaining_results,
            &|| cancellation.is_cancelled(),
        ) else {
            log_search_cancelled("streaming_find_matches", &stats, results.len());
            results.clear();
            break;
        };
        let file_results = file_search.results;
        if matches!(
            file_search.outcome,
            SearchBlockVisitOutcome::StoppedByResultLimit
        ) {
            stats.mark_truncated(SearchTruncationReason::Result);
        }
        for item in file_results {
            results.push(item);
            if results.len() >= limits.max_results {
                stats.mark_truncated(SearchTruncationReason::Result);
                break;
            }
        }

        if cancellation.is_cancelled_after_files(stats.searched_files) {
            if cancellation.is_cancelled() {
                results.clear();
            }
            break;
        }

        if results.len() >= limits.max_results {
            break;
        }
    }

    notify_search_before_response_for_test();
    if cancellation.is_cancelled() {
        results.clear();
        stats.clear_truncation();
    }

    Ok(SearchResponse::from_parts(query, results, limits, stats))
}

fn validate_search_base_path_identity(base_dir: &CanonicalPath) -> std::io::Result<()> {
    if base_dir.has_current_identity()? {
        return Ok(());
    }

    tracing::warn!("[markdown-view] 検索base directory pathの実体差し替えを検出しました");
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "検索base directory pathが起動時と異なります",
    ))
}

fn map_search_join_error(error: tokio::task::JoinError) -> std::io::Error {
    if error.is_panic() {
        tracing::error!(
            "[markdown-view] ディレクトリ検索blockingタスクがpanicしました: {}",
            error
        );
    } else {
        tracing::warn!(
            "[markdown-view] ディレクトリ検索blockingタスクのjoinエラー: {}",
            error
        );
    }

    std::io::Error::other("ディレクトリ検索タスクの実行に失敗しました")
}

fn log_safe_search_relative(relative: &str) -> String {
    relative.escape_debug().to_string()
}

fn path_to_search_relative(path: &Path) -> String {
    let mut output = String::new();
    for component in path.components() {
        if !output.is_empty() {
            output.push('/');
        }
        output.push_str(&component.as_os_str().to_string_lossy());
    }
    output
}

fn log_search_cancelled(phase: &'static str, stats: &SearchStats, result_count: usize) {
    tracing::debug!(
        "[markdown-view] ディレクトリ検索がstale化したため中断しました: phase={} searched_files={} searched_bytes={} result_count={}",
        phase,
        stats.searched_files,
        stats.searched_bytes,
        result_count
    );
}

fn read_search_markdown_with_byte_budget(
    base_dir: &Dir,
    relative: &str,
    searched_bytes: usize,
    max_bytes: usize,
) -> std::io::Result<SearchMarkdownRead> {
    let canonical_relative = canonical_search_relative(base_dir, relative)?;
    notify_search_after_canonicalize_for_test(relative);
    let file = open_relative_file_nofollow(base_dir, &canonical_relative)?;
    let metadata = file.metadata()?;
    notify_search_after_metadata_for_test(relative);
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "検索対象が通常ファイルではありません",
        ));
    }
    if metadata.len() > MAX_FILE_SIZE {
        return Err(file_too_large_error());
    }

    let file_len = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    let remaining_bytes = max_bytes.saturating_sub(searched_bytes);
    if file_len > remaining_bytes {
        return Ok(SearchMarkdownRead::ByteLimit);
    }

    let read_limit = u64::try_from(remaining_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1)
        .min(MAX_FILE_SIZE + 1);
    read_markdown_from_open_file_with_byte_budget(file, read_limit, remaining_bytes)
}

#[cfg(test)]
fn read_markdown_with_limit_blocking(file_path: &Path) -> std::io::Result<String> {
    let file = std::fs::File::open(file_path)?;
    read_markdown_from_open_file(file)
}

fn canonical_search_relative(
    base_dir: &Dir,
    relative: &str,
) -> std::io::Result<std::path::PathBuf> {
    let canonical_relative = base_dir.canonicalize(Path::new(relative))?;
    if canonical_relative.is_absolute()
        || canonical_relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "検索対象がベースディレクトリ外を指しています",
        ));
    }
    if exclusion_reason_for_relative_path(&canonical_relative).is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "検索対象が除外パスを指しています",
        ));
    }
    match canonical_relative.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("md") => Ok(canonical_relative),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "検索対象がMarkdownファイルではありません",
        )),
    }
}

#[cfg(test)]
fn read_markdown_from_open_file(file: impl Read) -> std::io::Result<String> {
    notify_search_markdown_read_for_test();
    let mut limited_reader = file.take(MAX_FILE_SIZE + 1);
    let mut buffer = Vec::new();
    limited_reader.read_to_end(&mut buffer)?;
    if buffer.len() as u64 > MAX_FILE_SIZE {
        return Err(file_too_large_error());
    }

    String::from_utf8(buffer).map_err(|error| {
        tracing::warn!(
            "[markdown-view] UTF-8デコード失敗: バイトオフセット {} で無効なバイト列",
            error.utf8_error().valid_up_to()
        );
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ファイルがUTF-8テキストではありません",
        )
    })
}

fn read_markdown_from_open_file_with_byte_budget(
    file: impl Read,
    read_limit: u64,
    remaining_bytes: usize,
) -> std::io::Result<SearchMarkdownRead> {
    notify_search_markdown_read_for_test();
    let mut limited_reader = file.take(read_limit);
    let mut buffer = Vec::new();
    limited_reader.read_to_end(&mut buffer)?;
    if buffer.len() as u64 > MAX_FILE_SIZE {
        return Err(file_too_large_error());
    }
    if buffer.len() > remaining_bytes {
        return Ok(SearchMarkdownRead::ByteLimit);
    }

    let bytes_read = buffer.len();
    match String::from_utf8(buffer) {
        Ok(markdown) => Ok(SearchMarkdownRead::Markdown {
            markdown,
            bytes_read,
        }),
        Err(error) => {
            tracing::warn!(
                "[markdown-view] UTF-8デコード失敗: バイトオフセット {} で無効なバイト列",
                error.utf8_error().valid_up_to()
            );
            Ok(SearchMarkdownRead::Skipped {
                error: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "ファイルがUTF-8テキストではありません",
                ),
                bytes_read,
            })
        }
    }
}

fn file_too_large_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "ファイルサイズが上限（10MB）を超えています",
    )
}

#[cfg(test)]
fn extract_search_blocks(markdown: &str) -> Vec<SearchBlockEntry> {
    extract_search_blocks_until_cancelled(markdown, &|| false)
        .expect("キャンセルなしの検索ブロック抽出は常に完了する")
}

#[allow(dead_code)]
fn extract_search_blocks_until_cancelled(
    markdown: &str,
    is_cancelled: &impl Fn() -> bool,
) -> Option<Vec<SearchBlockEntry>> {
    let mut blocks = Vec::new();
    let outcome = visit_search_blocks_until_cancelled(markdown, is_cancelled, |block| {
        blocks.push(block.clone());
        SearchBlockVisit::Continue
    })?;
    debug_assert_eq!(outcome, SearchBlockVisitOutcome::Completed);
    Some(blocks)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchBlockVisit {
    Continue,
    #[allow(dead_code)]
    StopResultLimit,
    #[allow(dead_code)]
    StopCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchBlockVisitOutcome {
    Completed,
    StoppedByResultLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinalizedSearchBlockVisit {
    Continue,
    Stop(SearchBlockVisitOutcome),
    Cancelled,
}

fn visit_search_blocks_until_cancelled(
    markdown: &str,
    is_cancelled: &impl Fn() -> bool,
    mut visitor: impl FnMut(&SearchBlockEntry) -> SearchBlockVisit,
) -> Option<SearchBlockVisitOutcome> {
    let mut current_block = String::new();
    let mut block_depth = 0usize;
    let mut item_depth = 0usize;
    let mut link_depth = 0usize;
    let mut image_depth = 0usize;
    let mut code_block_depth = 0usize;
    let mut inline_html_depth = 0usize;

    for event in Parser::new_ext(markdown, markdown_options(MarkdownProfile::Search)) {
        if is_cancelled() {
            return None;
        }
        notify_search_block_extract_for_test();
        if is_cancelled() {
            return None;
        }

        match event {
            Event::Start(tag) => {
                if matches!(tag, Tag::Item) {
                    if item_depth == 0 {
                        if block_depth == 0 {
                            current_block.clear();
                        } else if !current_block.ends_with('\n') {
                            current_block.push('\n');
                        }
                        block_depth += 1;
                    }
                    item_depth += 1;
                } else if is_search_block_tag(&tag) {
                    if block_depth == 0 {
                        current_block.clear();
                    } else if !current_block.ends_with('\n') {
                        current_block.push('\n');
                    }
                    block_depth += 1;
                }

                match tag {
                    Tag::Link { .. } => link_depth += 1,
                    Tag::Image { .. } => image_depth += 1,
                    Tag::CodeBlock(_) => code_block_depth += 1,
                    _ => {}
                }
            }
            Event::End(tag) => {
                if matches!(tag, TagEnd::Item) {
                    item_depth = item_depth.saturating_sub(1);
                    if item_depth == 0 {
                        block_depth = block_depth.saturating_sub(1);
                        if block_depth == 0 {
                            match visit_finalized_search_block(&current_block, &mut visitor) {
                                FinalizedSearchBlockVisit::Continue => {
                                    current_block.clear();
                                    inline_html_depth = 0;
                                }
                                FinalizedSearchBlockVisit::Stop(outcome) => return Some(outcome),
                                FinalizedSearchBlockVisit::Cancelled => return None,
                            }
                        }
                    }
                } else if is_search_block_end_tag(&tag) {
                    block_depth = block_depth.saturating_sub(1);
                    if !matches!(tag, TagEnd::Paragraph) {
                        debug_assert_eq!(
                            inline_html_depth, 0,
                            "inline HTML depth must be balanced before non-paragraph block end"
                        );
                    }
                    inline_html_depth = 0;
                    if block_depth == 0 {
                        match visit_finalized_search_block(&current_block, &mut visitor) {
                            FinalizedSearchBlockVisit::Continue => current_block.clear(),
                            FinalizedSearchBlockVisit::Stop(outcome) => return Some(outcome),
                            FinalizedSearchBlockVisit::Cancelled => return None,
                        }
                    }
                }

                match tag {
                    TagEnd::Link => link_depth = link_depth.saturating_sub(1),
                    TagEnd::Image => image_depth = image_depth.saturating_sub(1),
                    TagEnd::CodeBlock => code_block_depth = code_block_depth.saturating_sub(1),
                    _ => {}
                }
            }
            Event::Text(text) => {
                if should_capture_text(
                    block_depth,
                    item_depth,
                    link_depth,
                    image_depth,
                    code_block_depth,
                    inline_html_depth,
                ) {
                    current_block.push_str(&text);
                }
            }
            Event::Code(text) => {
                if should_capture_text(
                    block_depth,
                    item_depth,
                    link_depth,
                    image_depth,
                    code_block_depth,
                    inline_html_depth,
                ) {
                    current_block.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if should_capture_text(
                    block_depth,
                    item_depth,
                    link_depth,
                    image_depth,
                    code_block_depth,
                    inline_html_depth,
                ) && !current_block.ends_with('\n')
                {
                    current_block.push('\n');
                }
            }
            Event::Rule => {
                if block_depth > 0 && !current_block.ends_with('\n') {
                    current_block.push('\n');
                }
            }
            Event::InlineHtml(html) => update_inline_html_depth(&mut inline_html_depth, &html),
            Event::Html(_)
            | Event::TaskListMarker(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::FootnoteReference(_) => {}
        }
    }

    if block_depth == 0 {
        match visit_finalized_search_block(&current_block, &mut visitor) {
            FinalizedSearchBlockVisit::Continue => {}
            FinalizedSearchBlockVisit::Stop(outcome) => return Some(outcome),
            FinalizedSearchBlockVisit::Cancelled => return None,
        }
    }

    Some(SearchBlockVisitOutcome::Completed)
}

fn visit_finalized_search_block(
    text: &str,
    visitor: &mut impl FnMut(&SearchBlockEntry) -> SearchBlockVisit,
) -> FinalizedSearchBlockVisit {
    let Some(block) = build_search_block_entry(text) else {
        return FinalizedSearchBlockVisit::Continue;
    };

    match visitor(&block) {
        SearchBlockVisit::Continue => FinalizedSearchBlockVisit::Continue,
        SearchBlockVisit::StopResultLimit => {
            FinalizedSearchBlockVisit::Stop(SearchBlockVisitOutcome::StoppedByResultLimit)
        }
        SearchBlockVisit::StopCancelled => FinalizedSearchBlockVisit::Cancelled,
    }
}

fn should_capture_text(
    block_depth: usize,
    item_depth: usize,
    link_depth: usize,
    image_depth: usize,
    code_block_depth: usize,
    inline_html_depth: usize,
) -> bool {
    block_depth > 0
        && item_depth <= 1
        && link_depth == 0
        && image_depth == 0
        && code_block_depth == 0
        && inline_html_depth == 0
}

fn update_inline_html_depth(depth: &mut usize, html: &str) {
    let trimmed = html.trim();
    if !trimmed.starts_with('<') {
        return;
    }

    if trimmed.starts_with("</") {
        *depth = depth.saturating_sub(1);
        return;
    }

    if trimmed.starts_with("<!") || trimmed.starts_with("<?") || trimmed.ends_with("/>") {
        return;
    }

    if let Some(tag_name) = inline_html_tag_name(trimmed) {
        if !is_void_html_tag(tag_name) {
            *depth += 1;
        }
    }
}

fn inline_html_tag_name(html: &str) -> Option<&str> {
    let without_lt = html.strip_prefix('<')?.trim_start();
    let tag_name_end = without_lt
        .char_indices()
        .find_map(|(index, ch)| (!(ch.is_ascii_alphanumeric() || ch == '-')).then_some(index))
        .unwrap_or(without_lt.len());

    (tag_name_end > 0).then_some(&without_lt[..tag_name_end])
}

fn is_void_html_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_search_block_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph | Tag::Heading { .. } | Tag::Item | Tag::BlockQuote(_) | Tag::TableCell
    )
}

fn is_search_block_end_tag(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::Item
            | TagEnd::BlockQuote(_)
            | TagEnd::TableCell
    )
}

fn build_search_block_entry(text: &str) -> Option<SearchBlockEntry> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(SearchBlockEntry {
        text: trimmed.to_string(),
        sentences: if is_large_search_block(trimmed) {
            Vec::new()
        } else {
            split_text_into_sentence_ranges(trimmed)
        },
    })
}

#[allow(dead_code)]
fn find_matches_for_file(
    file: &str,
    blocks: &[SearchBlockEntry],
    query: &str,
    remaining_results: usize,
    is_cancelled: &impl Fn() -> bool,
) -> Option<Vec<SearchResultItem>> {
    if remaining_results == 0 {
        return Some(Vec::new());
    }

    let mut results = Vec::new();
    let normalized_query = query.to_lowercase();
    let mut file_match_index = 0usize;

    'blocks: for (block_index, block) in blocks.iter().enumerate() {
        if is_cancelled() {
            return None;
        }
        if block.text.is_empty() {
            continue;
        }

        if is_large_search_block(&block.text) {
            let file_results = find_matches_for_large_block(
                file,
                &block.text,
                query,
                file_match_index,
                remaining_results.saturating_sub(results.len()),
                is_cancelled,
            )?;
            file_match_index += file_results.len();
            results.extend(file_results);
            if results.len() >= remaining_results {
                break 'blocks;
            }
            continue;
        }

        let normalized = build_case_fold_index(&block.text, is_cancelled)?;
        let mut search_start = 0usize;

        while search_start <= normalized.normalized_text.len() {
            if is_cancelled() {
                return None;
            }
            let Some(relative_index) =
                normalized.normalized_text[search_start..].find(&normalized_query)
            else {
                break;
            };
            let normalized_match_start = search_start + relative_index;
            let normalized_match_end = normalized_match_start + normalized_query.len();
            let match_start = normalized.original_offset(normalized_match_start);
            let match_end = normalized.original_offset(normalized_match_end);
            let context = build_search_context(blocks, block_index, match_start, match_end);
            results.push(SearchResultItem::new(
                file.to_string(),
                file_match_index,
                context.before,
                context.current,
                context.after,
            ));
            file_match_index += 1;
            if is_cancelled() {
                return None;
            }
            if results.len() >= remaining_results {
                break 'blocks;
            }
            search_start = normalized_match_end;
        }
    }

    Some(results)
}

#[allow(dead_code)]
#[derive(Debug)]
struct StreamingFileSearchResult {
    results: Vec<SearchResultItem>,
    outcome: SearchBlockVisitOutcome,
}

#[derive(Debug, Clone)]
struct PendingSearchBlock {
    previous: Option<SearchBlockEntry>,
    current: SearchBlockEntry,
}

#[allow(dead_code)]
fn search_file_streaming_blocks(
    file: &str,
    markdown: &str,
    query: &str,
    remaining_results: usize,
    is_cancelled: &impl Fn() -> bool,
) -> Option<StreamingFileSearchResult> {
    if remaining_results == 0 {
        return Some(StreamingFileSearchResult {
            results: Vec::new(),
            outcome: SearchBlockVisitOutcome::StoppedByResultLimit,
        });
    }

    let mut results = Vec::new();
    let mut pending_block: Option<PendingSearchBlock> = None;
    let mut previous_context: Option<SearchBlockEntry> = None;
    let normalized_query = query.to_lowercase();
    let mut file_match_index = 0usize;

    let outcome = visit_search_blocks_until_cancelled(markdown, is_cancelled, |block| {
        if results.len() >= remaining_results {
            return SearchBlockVisit::StopResultLimit;
        }

        if is_large_search_block(&block.text) {
            if let Some(pending) = pending_block.take() {
                let previous_current = pending.current.clone();
                let Some(block_results) = flush_pending_search_block(
                    file,
                    pending,
                    Some(block),
                    &normalized_query,
                    file_match_index,
                    remaining_results.saturating_sub(results.len()),
                    is_cancelled,
                ) else {
                    return SearchBlockVisit::StopCancelled;
                };
                file_match_index += block_results.len();
                results.extend(block_results);
                previous_context = Some(previous_current);

                if results.len() >= remaining_results {
                    return SearchBlockVisit::StopResultLimit;
                }
            }

            let Some(block_results) = find_matches_for_large_block(
                file,
                &block.text,
                query,
                file_match_index,
                remaining_results.saturating_sub(results.len()),
                is_cancelled,
            ) else {
                return SearchBlockVisit::StopCancelled;
            };

            file_match_index += block_results.len();
            results.extend(block_results);

            if results.len() >= remaining_results {
                SearchBlockVisit::StopResultLimit
            } else {
                SearchBlockVisit::Continue
            }
        } else {
            let previous_for_new = if let Some(pending) = pending_block.take() {
                let previous_current = pending.current.clone();
                let Some(block_results) = flush_pending_search_block(
                    file,
                    pending,
                    Some(block),
                    &normalized_query,
                    file_match_index,
                    remaining_results.saturating_sub(results.len()),
                    is_cancelled,
                ) else {
                    return SearchBlockVisit::StopCancelled;
                };

                file_match_index += block_results.len();
                results.extend(block_results);

                if results.len() >= remaining_results {
                    return SearchBlockVisit::StopResultLimit;
                }

                Some(previous_current)
            } else {
                previous_context.take()
            };

            pending_block = Some(PendingSearchBlock {
                previous: previous_for_new,
                current: block.clone(),
            });
            SearchBlockVisit::Continue
        }
    })?;

    if matches!(outcome, SearchBlockVisitOutcome::Completed) {
        if let Some(pending) = pending_block.take() {
            let block_results = flush_pending_search_block(
                file,
                pending,
                None,
                &normalized_query,
                file_match_index,
                remaining_results.saturating_sub(results.len()),
                is_cancelled,
            )?;
            results.extend(block_results);
        }
    }

    Some(StreamingFileSearchResult { results, outcome })
}

fn flush_pending_search_block(
    file: &str,
    pending: PendingSearchBlock,
    next: Option<&SearchBlockEntry>,
    normalized_query: &str,
    file_match_index_start: usize,
    remaining_results: usize,
    is_cancelled: &impl Fn() -> bool,
) -> Option<Vec<SearchResultItem>> {
    let mut blocks = Vec::with_capacity(3);
    if let Some(previous) = pending.previous {
        blocks.push(previous);
    }
    let block_index = blocks.len();
    blocks.push(pending.current);
    if let Some(next) = next {
        blocks.push(next.clone());
    }

    find_matches_for_block_with_context(
        file,
        &blocks,
        block_index,
        normalized_query,
        file_match_index_start,
        remaining_results,
        is_cancelled,
    )
}

fn find_matches_for_block_with_context(
    file: &str,
    blocks: &[SearchBlockEntry],
    block_index: usize,
    normalized_query: &str,
    file_match_index_start: usize,
    remaining_results: usize,
    is_cancelled: &impl Fn() -> bool,
) -> Option<Vec<SearchResultItem>> {
    if remaining_results == 0 {
        return Some(Vec::new());
    }

    let block = &blocks[block_index];
    if block.text.is_empty() {
        return Some(Vec::new());
    }

    let normalized = build_case_fold_index(&block.text, is_cancelled)?;
    let mut search_start = 0usize;
    let mut results = Vec::new();

    while search_start <= normalized.normalized_text.len() {
        if is_cancelled() {
            return None;
        }
        let Some(relative_index) =
            normalized.normalized_text[search_start..].find(normalized_query)
        else {
            break;
        };
        let normalized_match_start = search_start + relative_index;
        let normalized_match_end = normalized_match_start + normalized_query.len();
        let match_start = normalized.original_offset(normalized_match_start);
        let match_end = normalized.original_offset(normalized_match_end);
        let context = build_search_context(blocks, block_index, match_start, match_end);
        results.push(SearchResultItem::new(
            file.to_string(),
            file_match_index_start + results.len(),
            context.before,
            context.current,
            context.after,
        ));
        if is_cancelled() {
            return None;
        }
        if results.len() >= remaining_results {
            break;
        }
        search_start = normalized_match_end;
    }

    Some(results)
}

fn is_large_search_block(text: &str) -> bool {
    text.len() > LARGE_SEARCH_BLOCK_BYTES
}

fn find_matches_for_large_block(
    file: &str,
    text: &str,
    query: &str,
    file_match_index_start: usize,
    remaining_results: usize,
    is_cancelled: &impl Fn() -> bool,
) -> Option<Vec<SearchResultItem>> {
    if remaining_results == 0 {
        return Some(Vec::new());
    }

    let normalized_query = query.to_lowercase();
    if normalized_query.is_empty() {
        return Some(Vec::new());
    }

    let pattern = normalized_query.as_bytes();
    let prefix_table = build_kmp_prefix_table(pattern);
    let mut matched_bytes = 0usize;
    let mut normalized_boundary = 0usize;
    let mut recent_offsets = VecDeque::from([(0usize, 0usize)]);
    let mut results = Vec::new();

    for (chars_seen, (char_index, ch)) in text.char_indices().enumerate() {
        if chars_seen % CASE_FOLD_CANCEL_CHECK_CHARS == 0 && is_cancelled() {
            return None;
        }
        notify_search_case_fold_char_for_test();
        let char_end = char_index + ch.len_utf8();
        let folded = ch.to_lowercase().collect::<String>();

        for byte in folded.bytes() {
            notify_search_large_block_find_for_test(1);
            normalized_boundary += 1;
            recent_offsets.push_back((normalized_boundary, char_end));
            while recent_offsets.len() > pattern.len() + 1 {
                recent_offsets.pop_front();
            }

            while matched_bytes > 0 && byte != pattern[matched_bytes] {
                matched_bytes = prefix_table[matched_bytes - 1];
            }
            if byte == pattern[matched_bytes] {
                matched_bytes += 1;
            }
            if matched_bytes < pattern.len() {
                continue;
            }

            let normalized_match_end = normalized_boundary;
            let normalized_match_start = normalized_match_end.saturating_sub(pattern.len());
            let match_start = recent_offset(&recent_offsets, normalized_match_start).unwrap_or(0);
            let match_end =
                recent_offset(&recent_offsets, normalized_match_end).unwrap_or(char_end);
            let current = clip_text_around_match(text, match_start, match_end);
            results.push(SearchResultItem::new(
                file.to_string(),
                file_match_index_start + results.len(),
                String::new(),
                current,
                String::new(),
            ));
            if is_cancelled() {
                return None;
            }
            if results.len() >= remaining_results {
                return Some(results);
            }
            matched_bytes = 0;
        }
    }

    Some(results)
}

fn build_kmp_prefix_table(pattern: &[u8]) -> Vec<usize> {
    let mut table = vec![0usize; pattern.len()];
    let mut matched = 0usize;

    for index in 1..pattern.len() {
        while matched > 0 && pattern[index] != pattern[matched] {
            matched = table[matched - 1];
        }
        if pattern[index] == pattern[matched] {
            matched += 1;
            table[index] = matched;
        }
    }

    table
}

fn recent_offset(offsets: &VecDeque<(usize, usize)>, boundary: usize) -> Option<usize> {
    offsets
        .iter()
        .find_map(|(candidate, offset)| (*candidate == boundary).then_some(*offset))
}

#[derive(Debug, Clone)]
struct CaseFoldIndex {
    normalized_text: String,
    original_offsets: Vec<usize>,
}

impl CaseFoldIndex {
    fn original_offset(&self, normalized_offset: usize) -> usize {
        self.original_offsets
            .get(normalized_offset)
            .copied()
            .unwrap_or_else(|| self.original_offsets.last().copied().unwrap_or(0))
    }
}

fn build_case_fold_index(text: &str, is_cancelled: &impl Fn() -> bool) -> Option<CaseFoldIndex> {
    let mut normalized_text = String::new();
    let mut original_offsets = vec![0];

    for (chars_seen, (char_index, ch)) in text.char_indices().enumerate() {
        if chars_seen % CASE_FOLD_CANCEL_CHECK_CHARS == 0 && is_cancelled() {
            return None;
        }
        notify_search_case_fold_char_for_test();
        let char_end = char_index + ch.len_utf8();
        let folded = ch.to_lowercase().collect::<String>();
        normalized_text.push_str(&folded);
        for _ in 0..folded.len() {
            original_offsets.push(char_end);
        }
    }

    Some(CaseFoldIndex {
        normalized_text,
        original_offsets,
    })
}

fn build_search_context(
    blocks: &[SearchBlockEntry],
    block_index: usize,
    match_start: usize,
    match_end: usize,
) -> SearchContext {
    notify_search_context_build_for_test();
    let block = &blocks[block_index];
    let sentence_index = get_sentence_for_match(&block.sentences, match_start, match_end)
        .unwrap_or(if block.sentences.is_empty() { -1 } else { 0 });
    let current = if sentence_index >= 0 {
        let range = &block.sentences[sentence_index as usize];
        clip_match_context(&block.text, range, match_start, match_end)
    } else {
        clip_text_around_match(&block.text, match_start, match_end)
    };

    SearchContext {
        before: get_adjacent_sentence(blocks, block_index, sentence_index, -1),
        current,
        after: get_adjacent_sentence(blocks, block_index, sentence_index, 1),
    }
}

fn get_sentence_for_match(
    sentences: &[Range<usize>],
    match_start: usize,
    match_end: usize,
) -> Option<isize> {
    for (index, sentence) in sentences.iter().enumerate() {
        if match_start < sentence.end && match_end > sentence.start {
            return Some(index as isize);
        }
    }
    None
}

fn get_adjacent_sentence(
    blocks: &[SearchBlockEntry],
    block_index: usize,
    sentence_index: isize,
    direction: isize,
) -> String {
    let mut target_block_index = block_index as isize;
    let mut target_sentence_index = sentence_index + direction;

    while target_block_index >= 0 && target_block_index < blocks.len() as isize {
        let entry = &blocks[target_block_index as usize];
        if target_sentence_index >= 0 && (target_sentence_index as usize) < entry.sentences.len() {
            let range = &entry.sentences[target_sentence_index as usize];
            let sentence = &entry.text[range.start..range.end];
            return if direction < 0 {
                clip_text_tail(sentence)
            } else {
                clip_text_head(sentence)
            };
        }

        target_block_index += direction;
        if target_block_index < 0 || target_block_index >= blocks.len() as isize {
            break;
        }
        target_sentence_index = if direction > 0 {
            0
        } else {
            blocks[target_block_index as usize].sentences.len() as isize - 1
        };
    }

    String::new()
}

fn clip_match_context(
    text: &str,
    sentence_range: &Range<usize>,
    match_start: usize,
    match_end: usize,
) -> String {
    let sentence = &text[sentence_range.start..sentence_range.end];
    let relative_match_start = match_start.saturating_sub(sentence_range.start);
    let relative_match_end = match_end.saturating_sub(sentence_range.start);
    clip_text_around_match(sentence, relative_match_start, relative_match_end)
}

fn clip_text_around_match(text: &str, match_start: usize, match_end: usize) -> String {
    if !has_more_than_context_chars(text) {
        return text.to_string();
    }

    let match_start = previous_char_boundary(text, match_start.min(text.len()));
    let match_end = previous_char_boundary(text, match_end.min(text.len()).max(match_start));
    let match_chars = counted_chars_in_range(text, match_start, match_end);
    let target_match_chars = match_chars.min(MAX_SEARCH_CONTEXT_CHARS);
    let before_budget = (MAX_SEARCH_CONTEXT_CHARS - target_match_chars) / 2;
    let after_budget = MAX_SEARCH_CONTEXT_CHARS - target_match_chars - before_budget;

    let (mut start_byte, before_chars) = start_byte_before_chars(text, match_start, before_budget);
    let after_target = after_budget + before_budget.saturating_sub(before_chars);
    let (end_byte, after_chars) = end_byte_after_chars(text, match_end, after_target);
    let after_shortage = after_target.saturating_sub(after_chars);
    if after_shortage > 0 {
        start_byte = start_byte_before_chars(text, match_start, before_budget + after_shortage).0;
    }

    clip_text_bytes(text, start_byte, end_byte)
}

fn has_more_than_context_chars(text: &str) -> bool {
    let mut chars_seen = 0usize;
    for (_, ch) in text.char_indices() {
        notify_search_clip_scan_for_test(ch.len_utf8());
        chars_seen += 1;
        if chars_seen > MAX_SEARCH_CONTEXT_CHARS {
            return true;
        }
    }
    false
}

fn previous_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn counted_chars_in_range(text: &str, start_byte: usize, end_byte: usize) -> usize {
    let mut count = 0usize;
    for ch in text[start_byte..end_byte].chars() {
        notify_search_clip_scan_for_test(ch.len_utf8());
        count += 1;
    }
    count
}

fn start_byte_before_chars(text: &str, end_byte: usize, char_budget: usize) -> (usize, usize) {
    if char_budget == 0 {
        return (end_byte, 0);
    }

    let mut start_byte = end_byte;
    let mut chars_seen = 0usize;
    for (byte_index, ch) in text[..end_byte].char_indices().rev() {
        notify_search_clip_scan_for_test(ch.len_utf8());
        start_byte = byte_index;
        chars_seen += 1;
        if chars_seen >= char_budget {
            break;
        }
    }
    (start_byte, chars_seen)
}

fn end_byte_after_chars(text: &str, start_byte: usize, char_budget: usize) -> (usize, usize) {
    if char_budget == 0 {
        return (start_byte, 0);
    }

    let mut end_byte = start_byte;
    let mut chars_seen = 0usize;
    for (relative_byte, ch) in text[start_byte..].char_indices() {
        if chars_seen >= char_budget {
            break;
        }
        notify_search_clip_scan_for_test(ch.len_utf8());
        end_byte = start_byte + relative_byte + ch.len_utf8();
        chars_seen += 1;
    }
    (end_byte, chars_seen)
}

fn clip_text_head(text: &str) -> String {
    let total_chars = text.chars().count();
    if total_chars <= MAX_SEARCH_CONTEXT_CHARS {
        return text.to_string();
    }

    let mut snippet = char_slice_to_string(text, 0, MAX_SEARCH_CONTEXT_CHARS);
    snippet.push_str(SEARCH_CONTEXT_ELLIPSIS);
    snippet
}

fn clip_text_tail(text: &str) -> String {
    let total_chars = text.chars().count();
    if total_chars <= MAX_SEARCH_CONTEXT_CHARS {
        return text.to_string();
    }

    let mut snippet = String::from(SEARCH_CONTEXT_ELLIPSIS);
    snippet.push_str(&char_slice_to_string(
        text,
        total_chars - MAX_SEARCH_CONTEXT_CHARS,
        total_chars,
    ));
    snippet
}

fn char_slice_to_string(text: &str, start_char: usize, end_char: usize) -> String {
    let start_byte = char_index_to_byte(text, start_char);
    let end_byte = char_index_to_byte(text, end_char);
    text[start_byte..end_byte].to_string()
}

fn clip_text_bytes(text: &str, start_byte: usize, end_byte: usize) -> String {
    let mut snippet = String::new();
    if start_byte > 0 {
        snippet.push_str(SEARCH_CONTEXT_ELLIPSIS);
    }
    snippet.push_str(&text[start_byte..end_byte]);
    if end_byte < text.len() {
        snippet.push_str(SEARCH_CONTEXT_ELLIPSIS);
    }
    snippet
}

fn char_index_to_byte(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .map(|(byte_index, _)| byte_index)
        .nth(char_index)
        .unwrap_or(text.len())
}

fn split_text_into_sentence_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut sentence_start = 0usize;

    for (index, ch) in text.char_indices() {
        if ch != '\n' && !is_sentence_boundary(ch) {
            continue;
        }

        let end = index + ch.len_utf8();
        if let Some(range) = trim_sentence_range(text, sentence_start, end) {
            ranges.push(range);
        }
        sentence_start = end;
    }

    if sentence_start < text.len() {
        if let Some(range) = trim_sentence_range(text, sentence_start, text.len()) {
            ranges.push(range);
        }
    }

    if ranges.is_empty() {
        if let Some(range) = trim_sentence_range(text, 0, text.len()) {
            ranges.push(range);
        }
    }

    ranges
}

fn is_sentence_boundary(ch: char) -> bool {
    matches!(ch, '.' | '!' | '?' | '。' | '！' | '？')
}

fn trim_sentence_range(text: &str, start: usize, end: usize) -> Option<Range<usize>> {
    let mut next_start = start;
    let mut next_end = end;

    while next_start < next_end {
        let ch = text[next_start..next_end].chars().next()?;
        if ch.is_whitespace() {
            next_start += ch.len_utf8();
        } else {
            break;
        }
    }

    while next_end > next_start {
        let ch = text[..next_end].chars().next_back()?;
        if ch.is_whitespace() {
            next_end -= ch.len_utf8();
        } else {
            break;
        }
    }

    (next_end > next_start).then_some(next_start..next_end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use tracing_test::traced_test;

    #[test]
    fn test_read_markdown_with_limit_blocking_utf8本文を読む() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "見出し\n\nneedle").unwrap();

        let markdown = read_markdown_with_limit_blocking(&path).unwrap();

        assert_eq!(markdown, "見出し\n\nneedle");
    }

    #[test]
    fn test_read_markdown_with_limit_blocking_utf8以外はinvalid_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.md");
        std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();

        let error = read_markdown_with_limit_blocking(&path).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_extract_search_blocks_リンクとコードブロックを除外し_inline_codeを含める() {
        let blocks = extract_search_blocks(
            "# Title\n\nAlpha [hidden link](https://example.com) visible.\n\n`cargo test`\n\n```rust\nhidden code\n```",
        );

        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].text, "Title");
        assert_eq!(blocks[1].text, "Alpha  visible.");
        assert_eq!(blocks[2].text, "cargo test");
    }

    #[test]
    fn test_extract_search_blocks_脚注参照ラベルを検索対象に含めない() {
        let blocks = extract_search_blocks(
            "Paragraph with footnote.[^note]\n\n[^note]: hidden footnote body",
        );

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "Paragraph with footnote.");
        assert_eq!(blocks[1].text, "hidden footnote body");
    }

    #[test]
    fn test_search_profileは脚注定義本文を検索ブロック化する() {
        let blocks = extract_search_blocks("本文です。[^note]\n\n[^note]: 検索専用の脚注本文");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "本文です。");
        assert_eq!(blocks[1].text, "検索専用の脚注本文");
    }

    #[test]
    fn test_search_profileはheading_attributes付き見出しの本文を検索対象にする() {
        let blocks = extract_search_blocks("# 表示見出し {#custom-id}");

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "表示見出し");
    }

    #[test]
    fn test_search_profileでもraw_htmlとinline_htmlは検索対象にしない() {
        let blocks = extract_search_blocks(
            "<section>hidden html</section>\n\n本文 visible <span>hidden inline</span>",
        );

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "本文 visible");
    }

    #[test]
    fn test_search_profileはネストしたinline_html内テキストを検索対象にしない() {
        let blocks = extract_search_blocks("before <span>a <span>b</span> c</span> tail");

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "before  tail");
    }

    #[test]
    fn test_search_profileは非paragraphブロック内inline_htmlを検索対象にしない() {
        let blocks = extract_search_blocks(
            "# 見出し <span>secret</span> tail\n\n> 引用 <span>hidden</span> tail\n\n| col |\n| --- |\n| セル <span>private</span> tail |",
        );

        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0].text, "見出し  tail");
        assert_eq!(blocks[1].text, "引用  tail");
        assert_eq!(blocks[2].text, "col");
        assert_eq!(blocks[3].text, "セル  tail");
    }

    #[test]
    fn test_search_profileはvoidタグ後の同段落テキストを検索対象に含める() {
        let blocks = extract_search_blocks("before <br> after\n\nimage <img src=\"x\"> tail");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "before  after");
        assert_eq!(blocks[1].text, "image  tail");
    }

    #[test]
    fn test_search_profileはハイフン付きcustom_element内テキストを検索対象にしない() {
        let blocks = extract_search_blocks(
            "before <img-card>hidden</img-card> after\n\nhead <wbr-widget>secret</wbr-widget> tail",
        );

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "before  after");
        assert_eq!(blocks[1].text, "head  tail");
    }

    #[test]
    fn test_search_profileは自閉じタグ後の段落内テキストを検索対象に含める() {
        let blocks = extract_search_blocks("before <custom/> after");

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "before  after");
    }

    #[test]
    fn test_search_profileはブロックhtml後の段落を検索対象にする() {
        let blocks = extract_search_blocks("<section>hidden html</section>\n\n次の段落 visible");

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "次の段落 visible");
    }

    #[test]
    fn test_search_profileは未閉鎖inline_html後の次段落を検索対象にする() {
        let blocks = extract_search_blocks("本文 <span>hidden\n\n次の段落 visible");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "本文");
        assert_eq!(blocks[1].text, "次の段落 visible");
    }

    #[test]
    fn test_search_profileはlist_item内の未閉鎖inline_html後の段落を検索対象にする() {
        let blocks = extract_search_blocks("- first <span>hidden\n\n  second visible");

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "first \nsecond visible");
    }

    #[test]
    fn test_search_profileはlist_item内の未閉鎖inline_html後にlist外段落を検索対象にする() {
        let blocks = extract_search_blocks("- first <span>hidden\n\noutside visible");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "first");
        assert_eq!(blocks[1].text, "outside visible");
    }

    #[test]
    fn test_extract_search_blocks_ネストしたリスト項目は親項目へ混ぜない() {
        let blocks = extract_search_blocks("- parent\n  - child alpha\n- sibling beta");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "parent");
        assert_eq!(blocks[1].text, "sibling beta");
    }

    #[test]
    fn test_visit_search_blocks_抽出契約を維持する() {
        let mut visited = Vec::new();

        let outcome = visit_search_blocks_until_cancelled(
            "# 表示見出し {#custom-id}\n\n本文 [link](https://example.com) visible\n\n```sh\nignored\n```\n\n| col |\n| --- |\n| セル |\n",
            &|| false,
            |block| {
                visited.push(block.clone());
                SearchBlockVisit::Continue
            },
        )
        .expect("キャンセルなしのvisitorは完了する");

        assert_eq!(outcome, SearchBlockVisitOutcome::Completed);
        assert_eq!(visited.len(), 4);
        assert_eq!(visited[0].text, "表示見出し");
        assert_eq!(visited[1].text, "本文  visible");
        assert_eq!(visited[2].text, "col");
        assert_eq!(visited[3].text, "セル");
    }

    #[test]
    fn test_visit_search_blocks_visitor停止後は後続blockを抽出しない() {
        let mut visited = Vec::new();
        let before_extracts = reset_search_block_extract_count_for_test();

        let outcome = visit_search_blocks_until_cancelled(
            "needle first.\n\nneedle second.\n\nneedle third.",
            &|| false,
            |block| {
                visited.push(block.text.clone());
                SearchBlockVisit::StopResultLimit
            },
        )
        .expect("visitor停止はキャンセルではない");
        let extracts = search_block_extract_count_for_test() - before_extracts;

        assert_eq!(outcome, SearchBlockVisitOutcome::StoppedByResultLimit);
        assert_eq!(visited, vec!["needle first.".to_string()]);
        assert!(
            extracts < 6,
            "visitor停止後に後続blockのevent streamを読み進めすぎている: {extracts}"
        );
    }

    #[test]
    fn test_visit_search_blocks_visitor_cancelledならnoneを返す() {
        let mut visited = Vec::new();

        let outcome = visit_search_blocks_until_cancelled(
            "needle first.\n\nneedle second.\n\nneedle third.",
            &|| false,
            |block| {
                visited.push(block.text.clone());
                SearchBlockVisit::StopCancelled
            },
        );

        assert!(outcome.is_none());
        assert_eq!(visited, vec!["needle first.".to_string()]);
    }

    #[test]
    fn test_visit_search_blocks_途中staleならnoneを返す() {
        let checks = Cell::new(0usize);
        let mut visited = Vec::new();

        let outcome = visit_search_blocks_until_cancelled(
            "# title\n\nfirst paragraph\n\nsecond paragraph",
            &|| {
                let next = checks.get() + 1;
                checks.set(next);
                next >= 3
            },
            |block| {
                visited.push(block.text.clone());
                SearchBlockVisit::Continue
            },
        );

        assert!(outcome.is_none());
        assert!(
            visited.len() <= 1,
            "stale後にblock visitorが処理を続けている: {visited:?}"
        );
    }

    #[test]
    fn test_extract_search_blocks_until_cancelled_途中staleならnoneを返す() {
        let checks = Cell::new(0usize);

        let blocks = extract_search_blocks_until_cancelled(
            "# title\n\nfirst paragraph\n\nsecond paragraph",
            &|| {
                let next = checks.get() + 1;
                checks.set(next);
                next >= 3
            },
        );

        assert!(blocks.is_none());
    }

    #[test]
    fn test_search_file_streaming_blocks_残り件数で後続block抽出を停止する() {
        let markdown = (0..120)
            .map(|index| format!("needle sentence {index}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let before_extracts = reset_search_block_extract_count_for_test();
        let before_context_builds = reset_search_context_build_count_for_test();

        let result =
            search_file_streaming_blocks("many.md", &markdown, "needle", 3, &|| false).unwrap();
        let extracts = search_block_extract_count_for_test() - before_extracts;
        let context_builds = search_context_build_count_for_test() - before_context_builds;

        assert_eq!(
            result.outcome,
            SearchBlockVisitOutcome::StoppedByResultLimit
        );
        assert_eq!(result.results.len(), 3);
        assert_eq!(context_builds, 3);
        assert!(
            extracts < 30,
            "result-limit後も後続block抽出が進んでいる: {extracts}"
        );
        assert_eq!(result.results[0].file_match_index, 0);
        assert_eq!(result.results[1].file_match_index, 1);
        assert_eq!(result.results[2].file_match_index, 2);
    }

    #[test]
    fn test_search_file_streaming_blocks_unicode小文字化でバイト長が変わっても安全に一致する() {
        let markdown = "İstanbul is here. Another line.";

        let result =
            search_file_streaming_blocks("README.md", markdown, "i̇stanbul", 100, &|| false)
                .unwrap();

        assert_eq!(result.outcome, SearchBlockVisitOutcome::Completed);
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].file_match_index, 0);
        assert_eq!(result.results[0].current, "İstanbul is here.");
    }

    #[test]
    fn test_search_file_streaming_blocks_前後contextは既存検索と一致する() {
        let markdown = "before sentence.\n\nneedle here.\n\nafter sentence.";
        let blocks = extract_search_blocks(markdown);

        let expected =
            find_matches_for_file("README.md", &blocks, "needle", 100, &|| false).unwrap();
        let result =
            search_file_streaming_blocks("README.md", markdown, "needle", 100, &|| false).unwrap();

        assert_eq!(result.results.len(), 1);
        assert_eq!(expected.len(), 1);
        assert_eq!(result.results[0].before, expected[0].before);
        assert_eq!(result.results[0].current, expected[0].current);
        assert_eq!(result.results[0].after, expected[0].after);
        assert_eq!(result.results[0].after, "after sentence.");
    }

    #[test]
    fn test_search_file_streaming_blocks_staleならnoneを返す() {
        let checks = Cell::new(0usize);
        let markdown = "# title\n\nneedle first.\n\nneedle second.";

        let result = search_file_streaming_blocks("README.md", markdown, "needle", 100, &|| {
            let next = checks.get() + 1;
            checks.set(next);
            next >= 4
        });

        assert!(result.is_none());
    }

    #[test]
    fn test_find_matches_for_file_ローカル一致番号を維持する() {
        let blocks = vec![
            SearchBlockEntry {
                text: "Alpha note here. Alpha note again.".to_string(),
                sentences: split_text_into_sentence_ranges("Alpha note here. Alpha note again."),
            },
            SearchBlockEntry {
                text: "Alpha note third.".to_string(),
                sentences: split_text_into_sentence_ranges("Alpha note third."),
            },
        ];

        let results =
            find_matches_for_file("README.md", &blocks, "alpha note", usize::MAX, &|| false)
                .unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].file_match_index, 0);
        assert_eq!(results[1].file_match_index, 1);
        assert_eq!(results[2].file_match_index, 2);
    }

    #[test]
    fn test_find_matches_for_file_unicode小文字化でバイト長が変わっても安全に一致する() {
        let text = "İstanbul is here. Another line.";
        let blocks = vec![SearchBlockEntry {
            text: text.to_string(),
            sentences: split_text_into_sentence_ranges(text),
        }];

        let results =
            find_matches_for_file("README.md", &blocks, "i̇stanbul", usize::MAX, &|| false).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_match_index, 0);
        assert_eq!(results[0].current, "İstanbul is here.");
    }

    #[test]
    fn test_find_matches_for_file_巨大ブロックunicode小文字化でバイト長が変わっても安全に一致する()
    {
        let block_text = format!(
            "{}İstanbul is here.",
            "a".repeat(LARGE_SEARCH_BLOCK_BYTES + 1)
        );
        let blocks = vec![SearchBlockEntry {
            text: block_text,
            sentences: Vec::new(),
        }];

        let results =
            find_matches_for_file("README.md", &blocks, "i̇stanbul", usize::MAX, &|| false).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_match_index, 0);
        assert!(results[0].current.contains("İstanbul is here."));
    }

    #[test]
    fn test_find_matches_for_file_巨大ブロックno_matchはprefix全体を繰り返し検索しない() {
        use std::cell::Cell;
        use std::rc::Rc;

        let block_text = "a".repeat(LARGE_SEARCH_BLOCK_BYTES + 20_000);
        let blocks = vec![SearchBlockEntry {
            text: block_text.clone(),
            sentences: Vec::new(),
        }];
        let scanned_bytes = Rc::new(Cell::new(0usize));
        let scanned_bytes_for_hook = Rc::clone(&scanned_bytes);
        let hook_calls = Rc::new(Cell::new(0usize));
        let hook_calls_for_hook = Rc::clone(&hook_calls);
        let max_scanned_bytes = block_text.len() * 32;
        let _guard = set_search_large_block_find_hook_for_test(move |search_bytes| {
            hook_calls_for_hook.set(hook_calls_for_hook.get() + 1);
            let next = scanned_bytes_for_hook.get() + search_bytes;
            assert!(
                next <= max_scanned_bytes,
                "巨大ブロックno-matchで検索範囲を再走査しすぎている: {next} bytes"
            );
            scanned_bytes_for_hook.set(next);
        });

        let results =
            find_matches_for_file("many.md", &blocks, "needle", usize::MAX, &|| false).unwrap();

        assert!(results.is_empty());
        assert!(hook_calls.get() > 0);
        assert!(scanned_bytes.get() > 0);
        assert!(scanned_bytes.get() <= max_scanned_bytes);
    }

    #[test]
    fn test_find_matches_for_file_巨大ブロックlate_matchはprefix全体を繰り返し検索しない() {
        use std::cell::Cell;
        use std::rc::Rc;

        let block_text = format!("{}needle", "a".repeat(LARGE_SEARCH_BLOCK_BYTES + 20_000));
        let blocks = vec![SearchBlockEntry {
            text: block_text.clone(),
            sentences: Vec::new(),
        }];
        let scanned_bytes = Rc::new(Cell::new(0usize));
        let scanned_bytes_for_hook = Rc::clone(&scanned_bytes);
        let hook_calls = Rc::new(Cell::new(0usize));
        let hook_calls_for_hook = Rc::clone(&hook_calls);
        let max_scanned_bytes = block_text.len() * 32;
        let _guard = set_search_large_block_find_hook_for_test(move |search_bytes| {
            hook_calls_for_hook.set(hook_calls_for_hook.get() + 1);
            let next = scanned_bytes_for_hook.get() + search_bytes;
            assert!(
                next <= max_scanned_bytes,
                "巨大ブロックlate-matchで検索範囲を再走査しすぎている: {next} bytes"
            );
            scanned_bytes_for_hook.set(next);
        });

        let results =
            find_matches_for_file("many.md", &blocks, "needle", usize::MAX, &|| false).unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].current.contains("needle"));
        assert!(hook_calls.get() > 0);
        assert!(scanned_bytes.get() > 0);
        assert!(scanned_bytes.get() <= max_scanned_bytes);
    }

    #[test]
    fn test_find_matches_for_file_巨大ブロック長いquery_no_matchはquery長ぶん再走査しない() {
        use std::cell::Cell;
        use std::rc::Rc;

        let block_text = "a".repeat(LARGE_SEARCH_BLOCK_BYTES + 20_000);
        let blocks = vec![SearchBlockEntry {
            text: block_text.clone(),
            sentences: Vec::new(),
        }];
        let query = "b".repeat(MAX_SEARCH_QUERY_CHARS);
        let scanned_bytes = Rc::new(Cell::new(0usize));
        let scanned_bytes_for_hook = Rc::clone(&scanned_bytes);
        let max_scanned_bytes = block_text.len() * 8;
        let _guard = set_search_large_block_find_hook_for_test(move |search_bytes| {
            let next = scanned_bytes_for_hook.get() + search_bytes;
            assert!(
                next <= max_scanned_bytes,
                "巨大ブロック長いquery no-matchで検索範囲を再走査しすぎている: {next} bytes"
            );
            scanned_bytes_for_hook.set(next);
        });

        let results =
            find_matches_for_file("many.md", &blocks, &query, usize::MAX, &|| false).unwrap();

        assert!(results.is_empty());
        assert!(scanned_bytes.get() > 0);
        assert!(scanned_bytes.get() <= max_scanned_bytes);
    }

    #[test]
    fn test_find_matches_for_file_巨大ブロック長いquery_late_matchはquery長ぶん再走査しない() {
        use std::cell::Cell;
        use std::rc::Rc;

        let query = "b".repeat(MAX_SEARCH_QUERY_CHARS);
        let block_text = format!("{}{}", "a".repeat(LARGE_SEARCH_BLOCK_BYTES + 20_000), query);
        let blocks = vec![SearchBlockEntry {
            text: block_text.clone(),
            sentences: Vec::new(),
        }];
        let scanned_bytes = Rc::new(Cell::new(0usize));
        let scanned_bytes_for_hook = Rc::clone(&scanned_bytes);
        let max_scanned_bytes = block_text.len() * 8;
        let _guard = set_search_large_block_find_hook_for_test(move |search_bytes| {
            let next = scanned_bytes_for_hook.get() + search_bytes;
            assert!(
                next <= max_scanned_bytes,
                "巨大ブロック長いquery late-matchで検索範囲を再走査しすぎている: {next} bytes"
            );
            scanned_bytes_for_hook.set(next);
        });

        let results =
            find_matches_for_file("many.md", &blocks, &query, usize::MAX, &|| false).unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].current.contains(&query));
        assert!(scanned_bytes.get() > 0);
        assert!(scanned_bytes.get() <= max_scanned_bytes);
    }

    #[test]
    fn test_find_matches_for_file_巨大ブロックsnippet生成は全文を結果件数分走査しない() {
        let block_text = format!(
            "{}{}",
            "a".repeat(LARGE_SEARCH_BLOCK_BYTES + 20_000),
            (0..10).map(|_| " needle").collect::<Vec<_>>().join("")
        );
        let blocks = vec![SearchBlockEntry {
            text: block_text.clone(),
            sentences: Vec::new(),
        }];

        reset_search_clip_scan_bytes_for_test();
        let results = find_matches_for_file("many.md", &blocks, "needle", 3, &|| false).unwrap();
        let scanned_bytes = search_clip_scan_bytes_for_test();

        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|item| item.current.contains("needle")));
        assert!(scanned_bytes > 0);
        assert!(
            scanned_bytes < block_text.len(),
            "巨大ブロックsnippet生成で本文全体を繰り返し走査している: {scanned_bytes} bytes"
        );
    }

    #[test]
    fn test_find_matches_for_file_残り件数で同一ファイル内探索を停止する() {
        let block_text = (0..120)
            .map(|index| format!("needle sentence {index}."))
            .collect::<Vec<_>>()
            .join(" ");
        let blocks = vec![SearchBlockEntry {
            text: block_text.clone(),
            sentences: split_text_into_sentence_ranges(&block_text),
        }];

        let before_context_builds = reset_search_context_build_count_for_test();
        let results = find_matches_for_file("many.md", &blocks, "needle", 3, &|| false).unwrap();
        let context_builds = search_context_build_count_for_test() - before_context_builds;

        assert_eq!(results.len(), 3);
        assert_eq!(context_builds, 3);
        assert_eq!(results[0].file_match_index, 0);
        assert_eq!(results[1].file_match_index, 1);
        assert_eq!(results[2].file_match_index, 2);
        assert!(results.iter().all(|item| item.file == "many.md"));
    }

    #[test]
    fn test_find_matches_for_file_巨大ブロック正規化中にstaleならcontextを生成しない() {
        use std::cell::Cell;

        let block_text = format!("{}needle", "a".repeat(LARGE_SEARCH_BLOCK_BYTES + 1));
        let blocks = vec![SearchBlockEntry {
            text: block_text.clone(),
            sentences: Vec::new(),
        }];
        let cancel_checks = Cell::new(0usize);
        reset_search_context_build_count_for_test();

        let results = find_matches_for_file("many.md", &blocks, "needle", 1, &|| {
            let next = cancel_checks.get() + 1;
            cancel_checks.set(next);
            next >= 3
        });

        assert!(results.is_none());
        assert_eq!(search_context_build_count_for_test(), 0);
    }

    #[test]
    fn test_find_matches_for_file_残り件数0なら結果を生成しない() {
        let block_text = "needle first. needle second.";
        let blocks = vec![SearchBlockEntry {
            text: block_text.to_string(),
            sentences: split_text_into_sentence_ranges(block_text),
        }];

        let results = find_matches_for_file("many.md", &blocks, "needle", 0, &|| false).unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn test_find_matches_for_file_巨大currentはmatch周辺へ切り詰める() {
        let block_text = format!("{}needle{}", "a".repeat(2_000), "b".repeat(2_000));
        let blocks = vec![SearchBlockEntry {
            text: block_text.clone(),
            sentences: split_text_into_sentence_ranges(&block_text),
        }];

        let results = find_matches_for_file("many.md", &blocks, "needle", 1, &|| false).unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].current.len() < block_text.len());
        assert!(results[0].current.len() <= MAX_SEARCH_CONTEXT_CHARS + 6);
        assert!(results[0].current.contains("needle"));
        assert!(results[0].current.starts_with("..."));
        assert!(results[0].current.ends_with("..."));
    }

    #[test]
    fn test_find_matches_for_file_巨大before_afterを切り詰める() {
        let before = "a".repeat(2_000);
        let after = "b".repeat(2_000);
        let block_text = format!("{before}.\nneedle.\n{after}.");
        let blocks = vec![SearchBlockEntry {
            text: block_text,
            sentences: vec![0..2001, 2002..2009, 2010..4011],
        }];

        let results = find_matches_for_file("many.md", &blocks, "needle", 1, &|| false).unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].before.len() <= MAX_SEARCH_CONTEXT_CHARS + 3);
        assert!(results[0].after.len() <= MAX_SEARCH_CONTEXT_CHARS + 3);
        assert!(results[0].before.starts_with("..."));
        assert!(results[0].before.ends_with('.'));
        assert!(results[0].after.starts_with('b'));
        assert!(results[0].after.ends_with("..."));
    }

    fn repeated_needles(count: usize) -> String {
        (0..count)
            .map(|index| format!("needle sentence {index}."))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn canonical_of(path: &Path) -> CanonicalPath {
        CanonicalPath::try_from_path(path).unwrap()
    }

    #[test]
    fn test_log_safe_search_relativeは制御文字をescapeする() {
        assert_eq!(
            log_safe_search_relative("bad\n\u{1b}.md"),
            "bad\\n\\u{1b}.md"
        );
    }

    #[test]
    fn test_search_cancellationは新しい世代を検知する() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };

        let current = Arc::new(AtomicU64::new(1));
        let generation = SearchGeneration::new(1, Arc::clone(&current));
        let cancellation = SearchCancellation::new(generation);

        assert!(!cancellation.is_cancelled());

        current.store(2, Ordering::Release);

        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn test_search_directory_キャンセル済みならファイル処理へ進まない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        let canonical = canonical_of(dir.path());
        let cancellation = SearchCancellation::cancelled_for_test();

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(response.query, "needle");
        assert_eq!(response.searched_files, 0);
        assert_eq!(response.skipped_files, 0);
        assert!(!response.truncated);
        assert!(response.results.is_empty());
    }

    #[test]
    fn test_search_directory_キャンセル済みならbase列挙前に空結果を返す() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = canonical_of(dir.path());
        std::fs::remove_dir_all(dir.path()).unwrap();
        let cancellation = SearchCancellation::cancelled_for_test();

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(response.query, "needle");
        assert_eq!(response.searched_files, 0);
        assert_eq!(response.skipped_files, 0);
        assert!(!response.truncated);
        assert!(response.results.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_search_directory_base実体差し替えを拒否する() {
        let parent = tempfile::tempdir().unwrap();
        let base = parent.path().join("workspace");
        let replacement = parent.path().join("replacement");
        std::fs::create_dir(&base).unwrap();
        std::fs::write(base.join("a.md"), "old needle").unwrap();
        let canonical = canonical_of(&base);

        std::fs::create_dir(&replacement).unwrap();
        std::fs::write(replacement.join("a.md"), "new needle").unwrap();
        std::fs::remove_dir_all(&base).unwrap();
        std::fs::rename(&replacement, &base).unwrap();

        let error = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .expect_err("base directoryの実体差し替えは拒否する");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn test_search_directory_base検証後の実体差し替えを拒否する() {
        let parent = tempfile::tempdir().unwrap();
        let base = parent.path().join("workspace");
        let replacement = parent.path().join("replacement");
        std::fs::create_dir(&base).unwrap();
        std::fs::write(base.join("a.md"), "old needle").unwrap();
        let canonical = canonical_of(&base);

        std::fs::create_dir(&replacement).unwrap();
        std::fs::write(replacement.join("a.md"), "new needle").unwrap();
        let base_for_hook = base.clone();
        let replacement_for_hook = replacement.clone();
        let _guard = set_search_after_base_identity_validation_hook_for_test(move || {
            std::fs::remove_dir_all(&base_for_hook).unwrap();
            std::fs::rename(&replacement_for_hook, &base_for_hook).unwrap();
        });

        let error = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .expect_err("base directory検証後の実体差し替えも拒否する");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn test_search_directory_base_identity不明なら拒否する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle").unwrap();
        let canonical = CanonicalPath::unknown_identity_for_test(dir.path());

        let error = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .expect_err("identityを取得できないbase directory検索は拒否する");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn test_search_directory_test用ファイル数limitは結果を破棄せず後続ファイルへ進まない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle first").unwrap();
        std::fs::write(dir.path().join("b.md"), "needle second").unwrap();
        let canonical = canonical_of(dir.path());
        let cancellation = SearchCancellation::cancel_after_files_for_test(1);

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(response.searched_files, 1);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].file, "a.md");
        assert!(!response.truncated);
        assert!(response.truncated_reasons.is_empty());
    }

    #[test]
    fn test_search_directory_読込直後にキャンセルされたら解析へ進まない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle first").unwrap();
        std::fs::write(dir.path().join("b.md"), "needle second").unwrap();
        let canonical = canonical_of(dir.path());
        let cancellation = SearchCancellation::cancel_after_reads_for_test(1);

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(response.searched_files, 0);
        assert!(response.results.is_empty());
    }

    #[test]
    fn test_search_directory_応答構築直前にstaleなら結果を返さない() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle first").unwrap();
        let canonical = canonical_of(dir.path());
        let current = Arc::new(AtomicU64::new(1));
        let generation = SearchGeneration::new(1, Arc::clone(&current));
        let cancellation = SearchCancellation::new(generation);
        let _guard = set_search_before_response_hook_for_test(move || {
            current.store(2, Ordering::Release);
        });

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(response.searched_files, 1);
        assert!(response.results.is_empty());
        assert!(!response.truncated);
    }

    #[traced_test]
    #[test]
    fn test_search_directory_ファイル内探索中にstale化したら部分結果を返さない() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("many.md"), repeated_needles(120)).unwrap();
        let canonical = canonical_of(dir.path());
        let current = Arc::new(AtomicU64::new(1));
        let generation = SearchGeneration::new(1, Arc::clone(&current));
        let cancellation = SearchCancellation::new(generation);
        reset_search_context_build_count_for_test();
        let _guard = set_search_context_build_hook_for_test(move |count| {
            if count == 3 {
                current.store(2, Ordering::Release);
            }
        });

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(search_context_build_count_for_test(), 3);
        assert_eq!(response.searched_files, 1);
        assert!(response.results.is_empty());
        assert!(!response.truncated);
        assert!(logs_contain(
            "ディレクトリ検索がstale化したため中断しました"
        ));
        assert!(logs_contain("phase=streaming_find_matches"));
        assert!(logs_contain("searched_files=1"));
        assert!(!logs_contain("needle"));
        assert!(!logs_contain("many.md"));
    }

    #[traced_test]
    #[test]
    fn test_search_directory_前ファイル結果があってもファイル内探索中staleなら結果を返さない() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle first").unwrap();
        std::fs::write(dir.path().join("b.md"), repeated_needles(120)).unwrap();
        let canonical = canonical_of(dir.path());
        let current = Arc::new(AtomicU64::new(1));
        let generation = SearchGeneration::new(1, Arc::clone(&current));
        let cancellation = SearchCancellation::new(generation);
        reset_search_context_build_count_for_test();
        let _guard = set_search_context_build_hook_for_test(move |count| {
            if count == 3 {
                current.store(2, Ordering::Release);
            }
        });

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(search_context_build_count_for_test(), 3);
        assert_eq!(response.searched_files, 2);
        assert!(response.results.is_empty());
        assert!(!response.truncated);
        assert!(logs_contain("phase=streaming_find_matches"));
        assert!(!logs_contain("needle"));
        assert!(!logs_contain("a.md"));
        assert!(!logs_contain("b.md"));
    }

    #[traced_test]
    #[test]
    fn test_search_directory_ブロック抽出中staleなら蓄積済み結果も返さない() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle first").unwrap();
        std::fs::write(dir.path().join("b.md"), repeated_needles(120)).unwrap();
        let canonical = canonical_of(dir.path());
        let current = Arc::new(AtomicU64::new(1));
        let generation = SearchGeneration::new(1, Arc::clone(&current));
        let cancellation = SearchCancellation::new(generation);
        let _block_guard = set_search_block_extract_hook_for_test(move |count| {
            if count > 20 {
                current.store(2, Ordering::Release);
            }
        });

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            cancellation,
        )
        .unwrap();

        assert_eq!(response.searched_files, 2);
        assert!(response.results.is_empty());
        assert!(!response.truncated);
        assert!(logs_contain("phase=streaming_find_matches"));
        assert!(!logs_contain("needle"));
        assert!(!logs_contain("a.md"));
        assert!(!logs_contain("b.md"));
    }

    #[test]
    fn test_search_directory_巨大単一文many_matchのjson応答サイズを抑える() {
        let dir = tempfile::tempdir().unwrap();
        let block_text = format!("{}{}", "needle".repeat(120), "a".repeat(20_000));
        std::fs::write(dir.path().join("many.md"), block_text).unwrap();
        let canonical = canonical_of(dir.path());

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();
        let json = serde_json::to_string(&response).unwrap();

        assert_eq!(response.results.len(), 100);
        assert!(json.len() < 120_000);
        assert!(response
            .results
            .iter()
            .all(|item| item.current.len() <= MAX_SEARCH_CONTEXT_CHARS + 6));
    }

    #[test]
    fn test_search_directory_巨大単一ブロックmany_matchはresult_limit後に全文正規化しない() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = (0..120)
            .map(|index| format!("needle {index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let block_text = format!("{prefix} {}", "a".repeat(200_000));
        std::fs::write(dir.path().join("many.md"), block_text).unwrap();
        let canonical = canonical_of(dir.path());
        reset_search_case_fold_char_count_for_test();

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert_eq!(response.results.len(), 100);
        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Result]
        );
        assert!(
            search_case_fold_char_count_for_test() < 64 * 1024,
            "巨大tailまでcase-foldしている: {} chars",
            search_case_fold_char_count_for_test()
        );
    }

    #[test]
    fn test_search_directory_result_limit到達後に同一ファイルの後続blockを抽出しない() {
        let dir = tempfile::tempdir().unwrap();
        let markdown = (0..120)
            .map(|index| format!("needle sentence {index}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        std::fs::write(dir.path().join("many.md"), markdown).unwrap();
        let canonical = canonical_of(dir.path());
        let before_extracts = reset_search_block_extract_count_for_test();

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 3,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();
        let extracts = search_block_extract_count_for_test() - before_extracts;

        assert_eq!(response.results.len(), 3);
        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Result]
        );
        assert_eq!(response.searched_files, 1);
        assert!(
            extracts < 30,
            "directory searchがresult-limit後もblock抽出を続けている: {extracts}"
        );
    }

    #[test]
    fn test_search_directory_マルチバイト巨大単一文many_matchのjson応答サイズを抑える() {
        let dir = tempfile::tempdir().unwrap();
        let block_text = format!("{}{}", "針".repeat(120), "語😀".repeat(10_000));
        std::fs::write(dir.path().join("many.md"), block_text).unwrap();
        let canonical = canonical_of(dir.path());

        let response = search_directory_with_limits_blocking(
            &canonical,
            "針",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();
        let json = serde_json::to_string(&response).unwrap();

        assert_eq!(response.results.len(), 100);
        assert!(json.len() < 360_000);
        assert!(response
            .results
            .iter()
            .all(|item| item.current.chars().count() <= MAX_SEARCH_CONTEXT_CHARS + 6));
    }

    #[tokio::test]
    async fn test_search_directory_通常検索は打ち切りなしの統計を返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        std::fs::write(dir.path().join("other.md"), "# Other").unwrap();

        let canonical = canonical_of(dir.path());
        let response = search_directory(&canonical, "needle", SearchCancellation::none(), None)
            .await
            .unwrap();

        assert_eq!(response.query, "needle");
        assert!(!response.truncated);
        assert!(response.truncated_reasons.is_empty());
        assert_eq!(response.limits.max_results, 100);
        assert_eq!(response.limits.max_files, 1000);
        assert_eq!(response.limits.max_bytes, 64 * 1024 * 1024);
        assert_eq!(response.searched_files, 2);
        assert_eq!(response.skipped_files, 0);
        assert_eq!(
            response.searched_bytes,
            "# Home\n\nneedle".len() + "# Other".len()
        );
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].file, "README.md");
    }

    #[tokio::test]
    async fn test_search_directory_読込失敗ファイルをスキップして検索を継続する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle is here").unwrap();
        std::fs::write(
            dir.path().join("b.md"),
            [0xff, 0xfe, b'n', b'e', b'e', b'd', b'l', b'e'],
        )
        .unwrap();

        let canonical = canonical_of(dir.path());
        let response = search_directory(&canonical, "needle", SearchCancellation::none(), None)
            .await
            .unwrap();

        assert!(!response.truncated);
        assert!(response.truncated_reasons.is_empty());
        assert_eq!(response.searched_files, 1);
        assert_eq!(response.skipped_files, 1);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].file, "a.md");
    }

    #[tokio::test]
    async fn test_search_directory_結果数上限到達を明示する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("many.md"), repeated_needles(120)).unwrap();

        let canonical = canonical_of(dir.path());
        let response = search_directory(&canonical, "needle", SearchCancellation::none(), None)
            .await
            .unwrap();

        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Result]
        );
        assert_eq!(response.results.len(), 100);
        assert_eq!(response.searched_files, 1);
    }

    #[test]
    fn test_search_directory_残り結果件数だけ次ファイルのcontextを生成する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), repeated_needles(95)).unwrap();
        std::fs::write(dir.path().join("b.md"), repeated_needles(120)).unwrap();
        let canonical = canonical_of(dir.path());
        reset_search_context_build_count_for_test();

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Result]
        );
        assert_eq!(response.results.len(), 100);
        assert_eq!(search_context_build_count_for_test(), 100);
        assert_eq!(response.results[94].file, "a.md");
        assert_eq!(response.results[95].file, "b.md");
    }

    #[test]
    fn test_search_directory_結果上限0なら読込と解析を行わず打ち切る() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("many.md"), repeated_needles(120)).unwrap();
        let canonical = canonical_of(dir.path());
        reset_search_context_build_count_for_test();

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 0,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Result]
        );
        assert_eq!(response.searched_files, 0);
        assert_eq!(response.searched_bytes, 0);
        assert!(response.results.is_empty());
        assert_eq!(search_context_build_count_for_test(), 0);
    }

    #[tokio::test]
    async fn test_search_directory_ファイル数上限到達を明示する() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..3 {
            std::fs::write(
                dir.path().join(format!("note-{index}.md")),
                format!("needle {index}"),
            )
            .unwrap();
        }

        let canonical = canonical_of(dir.path());
        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 2,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::File]
        );
        assert_eq!(response.searched_files, 2);
        assert_eq!(response.results.len(), 2);
    }

    #[tokio::test]
    async fn test_search_directory_ファイル数が上限ちょうどなら打ち切り扱いにしない() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..2 {
            std::fs::write(
                dir.path().join(format!("note-{index}.md")),
                format!("needle {index}"),
            )
            .unwrap();
        }

        let canonical = canonical_of(dir.path());
        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 2,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert!(!response.truncated);
        assert!(response.truncated_reasons.is_empty());
        assert_eq!(response.searched_files, 2);
        assert_eq!(response.results.len(), 2);
    }

    #[test]
    fn test_read_search_markdown_with_byte_budget_予算内なら本文を読む() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        std::fs::write(&path, "needle").unwrap();
        let base_dir = Dir::open_ambient_dir(dir.path(), cap_std::ambient_authority()).unwrap();

        let result =
            read_search_markdown_with_byte_budget(&base_dir, "a.md", 0, "needle".len()).unwrap();

        match result {
            SearchMarkdownRead::Markdown {
                markdown,
                bytes_read,
            } => {
                assert_eq!(markdown, "needle");
                assert_eq!(bytes_read, "needle".len());
            }
            SearchMarkdownRead::Skipped { .. } => panic!("予算内UTF-8ファイルはskipしない"),
            SearchMarkdownRead::ByteLimit => panic!("予算内ファイルは本文を読む必要がある"),
        }
    }

    #[test]
    fn test_read_search_markdown_with_byte_budget_予算超過なら本文構築前に停止する() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.md");
        std::fs::write(&path, "needle should not be searched").unwrap();
        let base_dir = Dir::open_ambient_dir(dir.path(), cap_std::ambient_authority()).unwrap();

        let result = read_search_markdown_with_byte_budget(
            &base_dir,
            "b.md",
            "needle".len(),
            "needle".len(),
        )
        .unwrap();

        assert!(matches!(result, SearchMarkdownRead::ByteLimit));
    }

    #[test]
    fn test_read_search_markdown_with_byte_budget_metadata後に増えた本文も残り予算で打ち切る() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("growing.md");
        std::fs::write(&path, "needle").unwrap();
        let path_for_hook = path.clone();
        let _guard = set_search_after_metadata_hook_for_test(move |relative| {
            if relative == "growing.md" {
                std::fs::write(&path_for_hook, "needle should not be fully read").unwrap();
            }
        });
        let base_dir = Dir::open_ambient_dir(dir.path(), cap_std::ambient_authority()).unwrap();
        reset_search_markdown_read_count_for_test();

        let result =
            read_search_markdown_with_byte_budget(&base_dir, "growing.md", 0, "needle".len())
                .unwrap();

        assert!(matches!(result, SearchMarkdownRead::ByteLimit));
        assert_eq!(
            search_markdown_read_count_for_test(),
            1,
            "残り予算+1の限定読込でbyte-limitを検出する"
        );
    }

    #[test]
    fn test_read_search_markdown_with_byte_budget_metadata失敗はerrを返す() {
        let dir = tempfile::tempdir().unwrap();
        let base_dir = Dir::open_ambient_dir(dir.path(), cap_std::ambient_authority()).unwrap();

        let error =
            read_search_markdown_with_byte_budget(&base_dir, "missing.md", 0, "needle".len())
                .expect_err("metadata失敗は呼び出し側のskip経路へ渡す");

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn test_read_search_markdown_with_byte_budget_単体ファイル上限超過はerrを返す() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oversized.md");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_FILE_SIZE + 1)
            .unwrap();
        let base_dir = Dir::open_ambient_dir(dir.path(), cap_std::ambient_authority()).unwrap();

        let error =
            read_search_markdown_with_byte_budget(&base_dir, "oversized.md", 0, "needle".len())
                .expect_err("単体ファイル上限超過はskip経路へ渡す");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[cfg(unix)]
    #[test]
    fn test_search_directory_resolve後にbase外symlinkへ差し替わっても本文を返さない() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.md");
        std::fs::write(&target, "harmless").unwrap();
        std::fs::write(outside.path().join("secret.md"), "outside-secret").unwrap();
        let canonical = canonical_of(dir.path());
        let target_for_hook = target.clone();
        let outside_target = outside.path().join("secret.md");
        let _guard = set_search_after_resolve_hook_for_test(move |relative| {
            if relative == "a.md" {
                std::fs::remove_file(&target_for_hook).unwrap();
                std::os::unix::fs::symlink(&outside_target, &target_for_hook).unwrap();
            }
        });

        let response = search_directory_with_limits_blocking(
            &canonical,
            "outside-secret",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert_eq!(response.searched_files, 0);
        assert_eq!(response.skipped_files, 1);
        assert!(response.results.is_empty());
        assert!(!response.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn test_search_directory_canonicalize後に中間symlinkを除外dirへ差し替えても本文を返さない() {
        let dir = tempfile::tempdir().unwrap();
        let visible_dir = dir.path().join("visible");
        std::fs::create_dir_all(&visible_dir).unwrap();
        std::fs::write(visible_dir.join("a.md"), "harmless").unwrap();
        let excluded_dir = dir.path().join(".git");
        std::fs::create_dir_all(&excluded_dir).unwrap();
        std::fs::write(excluded_dir.join("a.md"), "git-secret").unwrap();
        let canonical = canonical_of(dir.path());
        let visible_for_hook = visible_dir.clone();
        let excluded_for_hook = excluded_dir.clone();
        let _guard = set_search_after_canonicalize_hook_for_test(move |relative| {
            if relative == "visible/a.md" {
                std::fs::remove_dir_all(&visible_for_hook).unwrap();
                std::os::unix::fs::symlink(&excluded_for_hook, &visible_for_hook).unwrap();
            }
        });

        let response = search_directory_with_limits_blocking(
            &canonical,
            "git-secret",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert_eq!(response.searched_files, 0);
        assert_eq!(response.skipped_files, 1);
        assert!(response.results.is_empty());
        assert!(!response.truncated);
    }

    #[tokio::test]
    async fn test_search_directory_総読込バイト上限到達を明示し超過ファイルは検索しない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle").unwrap();
        std::fs::write(dir.path().join("b.md"), "needle should not be searched").unwrap();

        let canonical = canonical_of(dir.path());
        reset_search_markdown_read_count_for_test();
        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: "needle".len(),
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Byte]
        );
        assert_eq!(response.searched_files, 1);
        assert_eq!(response.searched_bytes, "needle".len());
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].file, "a.md");
        assert_eq!(
            search_markdown_read_count_for_test(),
            1,
            "byte-limit超過候補ファイルは本文読込前に打ち切る"
        );
    }

    #[tokio::test]
    async fn test_search_directory_byte予算超過候補は本文string構築前に打ち切る() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle").unwrap();
        std::fs::write(
            dir.path().join("b.md"),
            [0xff, 0xfe, b'n', b'e', b'e', b'd', b'l', b'e'],
        )
        .unwrap();
        std::fs::write(dir.path().join("c.md"), "needle after invalid").unwrap();

        let canonical = canonical_of(dir.path());
        reset_search_markdown_read_count_for_test();
        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: "needle".len(),
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Byte]
        );
        assert_eq!(response.searched_files, 1);
        assert_eq!(response.skipped_files, 0);
        assert_eq!(response.searched_bytes, "needle".len());
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].file, "a.md");
        assert_eq!(
            search_markdown_read_count_for_test(),
            1,
            "byte-limit超過候補は検索用本文Stringの構築前に打ち切る"
        );
    }

    #[tokio::test]
    async fn test_search_directory_invalid_utf8も読込予算を消費しbyte_limitで停止する() {
        let dir = tempfile::tempdir().unwrap();
        let invalid_bytes = [0xff, 0xfe, b'n', b'e', b'e', b'd', b'l', b'e'];
        std::fs::write(dir.path().join("a.md"), invalid_bytes).unwrap();
        std::fs::write(dir.path().join("b.md"), "needle").unwrap();

        let canonical = canonical_of(dir.path());
        reset_search_markdown_read_count_for_test();
        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: invalid_bytes.len(),
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Byte]
        );
        assert_eq!(response.searched_files, 0);
        assert_eq!(response.skipped_files, 1);
        assert_eq!(response.searched_bytes, 0);
        assert!(response.results.is_empty());
        assert_eq!(
            search_markdown_read_count_for_test(),
            1,
            "invalid UTF-8で消費した読込予算により次候補は本文読込前に打ち切る"
        );
    }

    #[traced_test]
    #[test]
    fn test_search_directory_byte_limit到達ログは本文とqueryを含めない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "x").unwrap();
        std::fs::write(dir.path().join("b.md"), "needle").unwrap();

        let canonical = canonical_of(dir.path());
        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 1,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert!(response.truncated);
        assert_eq!(
            response.truncated_reasons,
            vec![SearchTruncationReason::Byte]
        );
        assert!(logs_contain("ディレクトリ検索がbyte-limitに到達しました"));
        assert!(logs_contain("searched_bytes=1"));
        assert!(logs_contain("budgeted_bytes=1"));
        assert!(logs_contain("max_bytes=1"));
        assert!(!logs_contain("needle"));
    }

    #[test]
    fn test_search_directory_byte予算超過判定直後にstaleならtruncationを返さない() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "needle").unwrap();
        std::fs::write(dir.path().join("b.md"), "needle should not be searched").unwrap();
        let canonical = canonical_of(dir.path());
        let current = Arc::new(AtomicU64::new(1));
        let generation = SearchGeneration::new(1, Arc::clone(&current));
        let cancellation = SearchCancellation::new(generation);
        let _guard = set_search_after_byte_limit_hook_for_test(move || {
            current.store(2, Ordering::Release);
        });

        let response = search_directory_with_limits_blocking(
            &canonical,
            "needle",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: "needle".len(),
            },
            cancellation,
        )
        .unwrap();

        assert!(response.results.is_empty());
        assert!(!response.truncated);
        assert!(response.truncated_reasons.is_empty());
    }

    #[test]
    fn test_search_directory_queryが上限を超えるとinvalid_inputを返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        let canonical = canonical_of(dir.path());
        let query = "あ".repeat(MAX_SEARCH_QUERY_CHARS + 1);

        let error = search_directory_with_limits_blocking(
            &canonical,
            &query,
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn test_search_directory_async入口は長すぎるqueryを列挙前に拒否する() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        let canonical = canonical_of(dir.path());
        std::fs::remove_dir_all(dir.path()).unwrap();
        let query = "あ".repeat(MAX_SEARCH_QUERY_CHARS + 1);

        let error = search_directory(&canonical, &query, SearchCancellation::none(), None)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_search_directory_queryはtrim後256文字まで許可する() {
        let dir = tempfile::tempdir().unwrap();
        let query = "あ".repeat(MAX_SEARCH_QUERY_CHARS);
        std::fs::write(dir.path().join("README.md"), format!("{query} found")).unwrap();
        let canonical = canonical_of(dir.path());

        let response = search_directory_with_limits_blocking(
            &canonical,
            &query,
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert_eq!(response.query, query);
        assert_eq!(response.results.len(), 1);
    }

    #[test]
    fn test_search_directory_queryはtrim後空なら空結果を返す() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Home\n\nneedle").unwrap();
        let canonical = canonical_of(dir.path());

        let response = search_directory_with_limits_blocking(
            &canonical,
            "   ",
            SearchLimits {
                max_results: 100,
                max_files: 1000,
                max_bytes: 64 * 1024 * 1024,
            },
            SearchCancellation::none(),
        )
        .unwrap();

        assert_eq!(response.query, "");
        assert_eq!(response.searched_files, 0);
        assert!(response.results.is_empty());
    }
}
