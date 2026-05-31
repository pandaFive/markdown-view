"use strict";
function createDirectorySearchController(ctx, deps) {
    function createDirectorySearchTruncatedState() {
        var item = document.createElement('div');
        item.className = 'document-search-empty';
        item.textContent = '上限により一部のみ表示しています。';
        return item;
    }
    function renderDirectorySearchResults(options) {
        options = options || {};
        var resultsEl = ctx.elements.documentSearchResultsEl;
        var preservedScrollTop = resultsEl.scrollTop;
        var hadRenderedResults = Boolean(resultsEl.querySelector('.document-search-result'));
        if (options.preserveScroll !== false && hadRenderedResults) {
            ctx.search.currentDirectoryResultsScrollTop = preservedScrollTop;
        }
        resultsEl.innerHTML = '';
        if (!ctx.search.currentDocumentQuery)
            return;
        if (ctx.search.currentDirectoryLoading) {
            resultsEl.appendChild(deps.createDocumentSearchEmptyState('ディレクトリを検索しています。'));
            return;
        }
        if (ctx.search.currentDirectoryError) {
            resultsEl.appendChild(deps.createDocumentSearchEmptyState(ctx.search.currentDirectoryError));
            return;
        }
        if (ctx.search.currentDirectoryTruncated) {
            resultsEl.appendChild(createDirectorySearchTruncatedState());
        }
        if (!ctx.search.currentDirectoryResults.length) {
            resultsEl.appendChild(deps.createDocumentSearchEmptyState('ディレクトリ内に一致が見つかりません。'));
            return;
        }
        ctx.search.currentDirectoryResults.forEach(function (result, index) {
            var button = document.createElement('button');
            var indexBadge = document.createElement('span');
            var body = document.createElement('span');
            var path = document.createElement('span');
            button.type = 'button';
            button.className = 'document-search-result';
            button.dataset.resultIndex = String(index);
            button.classList.toggle('active', index === ctx.search.currentDirectoryIndex);
            button.setAttribute('aria-current', index === ctx.search.currentDirectoryIndex ? 'true' : 'false');
            button.addEventListener('click', function () {
                openDirectorySearchResult(index);
            });
            indexBadge.className = 'document-search-result-index';
            indexBadge.textContent = String(index + 1).padStart(2, '0');
            body.className = 'document-search-result-body';
            path.className = 'document-search-result-path';
            path.textContent = result.file == null ? '' : String(result.file);
            body.appendChild(path);
            deps.renderDocumentSearchResultContext(body, result.before == null ? '' : String(result.before), ctx.search.currentDocumentQuery, 'before');
            deps.renderDocumentSearchResultContext(body, result.current == null ? '' : String(result.current), ctx.search.currentDocumentQuery, 'current');
            deps.renderDocumentSearchResultContext(body, result.after == null ? '' : String(result.after), ctx.search.currentDocumentQuery, 'after');
            button.appendChild(indexBadge);
            button.appendChild(body);
            resultsEl.appendChild(button);
        });
        resultsEl.scrollTop = ctx.search.currentDirectoryResultsScrollTop;
    }
    function renderDirectorySearchUi(options) {
        deps.updateDocumentSearchSummary();
        renderDirectorySearchResults(options);
    }
    function applyPendingDirectorySearchNavigation() {
        if (!ctx.config.isDirMode || !ctx.search.pendingDirectoryNavigation)
            return;
        if (ctx.search.pendingDirectoryNavigation.file !== ctx.state.currentFile)
            return;
        if (ctx.search.pendingDirectoryNavigation.query !== ctx.search.currentDocumentQuery) {
            ctx.search.pendingDirectoryNavigation = null;
            return;
        }
        if (ctx.search.documentMatches.length) {
            deps.setCurrentDocumentSearchMatch(Math.min(ctx.search.pendingDirectoryNavigation.fileMatchIndex, ctx.search.documentMatches.length - 1));
        }
        ctx.search.currentDirectoryIndex = ctx.search.pendingDirectoryNavigation.resultIndex;
        ctx.search.pendingDirectoryNavigation = null;
    }
    function scheduleDirectorySearch(query) {
        if (ctx.search.documentDebounceTimer) {
            clearTimeout(ctx.search.documentDebounceTimer);
        }
        ctx.search.documentDebounceTimer = setTimeout(function () {
            ctx.search.documentDebounceTimer = null;
            runDirectorySearch(query);
        }, 300);
    }
    function nextDirectorySearchSequence() {
        ctx.search.directorySearchSequence += 1;
        return ctx.search.directorySearchSequence;
    }
    function directorySearchHeaders(sequence) {
        return {
            'Accept': 'application/json',
            'X-Markdown-View-Search-Client': ctx.search.directorySearchClientId,
            'X-Markdown-View-Search-Sequence': String(sequence)
        };
    }
    function cancelDirectorySearch() {
        if (ctx.search.documentDebounceTimer) {
            clearTimeout(ctx.search.documentDebounceTimer);
            ctx.search.documentDebounceTimer = null;
        }
        ctx.search.documentFetchGeneration += 1;
        var sequence = nextDirectorySearchSequence();
        fetch('/api/search?q=', {
            headers: directorySearchHeaders(sequence)
        }).catch(function (err) {
            console.debug('[markdown-view] ディレクトリ検索キャンセル通知に失敗しました:', err);
        });
    }
    function getPreferredDirectorySearchSelection() {
        if (ctx.search.pendingDirectoryNavigation) {
            return {
                file: ctx.search.pendingDirectoryNavigation.file,
                fileMatchIndex: ctx.search.pendingDirectoryNavigation.fileMatchIndex
            };
        }
        if (ctx.search.currentDirectoryIndex >= 0 &&
            ctx.search.currentDirectoryIndex < ctx.search.currentDirectoryResults.length) {
            var currentResult = ctx.search.currentDirectoryResults[ctx.search.currentDirectoryIndex];
            return {
                file: currentResult.file,
                fileMatchIndex: currentResult.file_match_index
            };
        }
        if (ctx.state.currentFile &&
            ctx.search.currentDocumentIndex >= 0 &&
            ctx.search.currentDocumentIndex < ctx.search.documentMatches.length) {
            return {
                file: ctx.state.currentFile,
                fileMatchIndex: ctx.search.currentDocumentIndex
            };
        }
        return null;
    }
    function resolveDirectorySearchIndex(results, preferredSelection) {
        var index;
        if (!results.length)
            return -1;
        if (preferredSelection) {
            index = results.findIndex(function (result) {
                return (result.file === preferredSelection.file &&
                    result.file_match_index === preferredSelection.fileMatchIndex);
            });
            if (index !== -1)
                return index;
        }
        if (ctx.state.currentFile &&
            ctx.search.currentDocumentIndex >= 0 &&
            ctx.search.currentDocumentIndex < ctx.search.documentMatches.length) {
            index = results.findIndex(function (result) {
                return (result.file === ctx.state.currentFile &&
                    result.file_match_index === ctx.search.currentDocumentIndex);
            });
            if (index !== -1)
                return index;
        }
        return -1;
    }
    function isDirectorySearchResultItem(result) {
        return Boolean(result &&
            typeof result === 'object' &&
            !Array.isArray(result) &&
            typeof result.file === 'string' &&
            result.file.length > 0 &&
            Number.isFinite(result.file_match_index) &&
            result.file_match_index >= 0 &&
            Math.floor(result.file_match_index) === result.file_match_index &&
            typeof result.before === 'string' &&
            typeof result.current === 'string' &&
            typeof result.after === 'string');
    }
    function isSearchTruncationReason(value) {
        return value === 'result_limit' || value === 'file_limit' || value === 'byte_limit';
    }
    function isSearchResponse(data, query) {
        return Boolean(data &&
            typeof data === 'object' &&
            !Array.isArray(data) &&
            data.query === query &&
            Array.isArray(data.results) &&
            data.results.every(isDirectorySearchResultItem) &&
            Number.isFinite(data.searched_files) &&
            Number.isFinite(data.skipped_files) &&
            Number.isFinite(data.searched_bytes) &&
            typeof data.truncated === 'boolean' &&
            Array.isArray(data.truncated_reasons) &&
            data.truncated_reasons.every(isSearchTruncationReason) &&
            Boolean(data.limits &&
                typeof data.limits === 'object' &&
                Number.isFinite(data.limits.max_results) &&
                Number.isFinite(data.limits.max_files) &&
                Number.isFinite(data.limits.max_bytes)));
    }
    function applyDirectorySearchContractViolation(data) {
        ctx.search.currentDirectoryLoading = false;
        ctx.search.currentDirectoryResults = [];
        ctx.search.currentDirectoryIndex = -1;
        ctx.search.currentDirectorySkippedFiles = 0;
        ctx.search.currentDirectoryTruncated = false;
        ctx.search.currentDirectoryTruncatedReasons = [];
        ctx.search.currentDirectoryError = 'サーバー応答の解析に失敗しました。ページを再読み込みしてください。';
        console.warn('[markdown-view] ディレクトリ検索応答の契約違反', {
            expectedQuery: ctx.search.currentDocumentQuery,
            actualQuery: data && typeof data === 'object' && !Array.isArray(data) ? data.query : null,
            hasResults: Boolean(data && typeof data === 'object' && Array.isArray(data.results))
        });
        renderDirectorySearchUi();
    }
    function isApiErrorPayload(value) {
        return Boolean(value && typeof value === 'object' && !Array.isArray(value) &&
            typeof value.error === 'string' && value.error);
    }
    function throwDirectorySearchHttpError(resp) {
        return resp.text().then(function (body) {
            var err = deps.createHttpError(resp.status);
            if (body) {
                try {
                    var payload = JSON.parse(body);
                    if (isApiErrorPayload(payload)) {
                        err.userMessage = payload.error;
                    }
                }
                catch (_parseError) {
                    // JSON でないエラー本文は既存の HTTP status 文言へフォールバックする。
                }
            }
            throw err;
        });
    }
    function runDirectorySearch(query) {
        var generation = ++ctx.search.documentFetchGeneration;
        var sequence = nextDirectorySearchSequence();
        var preferredSelection = getPreferredDirectorySearchSelection();
        ctx.search.currentDirectoryLoading = true;
        ctx.search.currentDirectoryError = '';
        ctx.search.currentDirectoryResults = [];
        ctx.search.currentDirectoryIndex = -1;
        ctx.search.currentDirectorySkippedFiles = 0;
        ctx.search.currentDirectoryTruncated = false;
        ctx.search.currentDirectoryTruncatedReasons = [];
        renderDirectorySearchUi();
        fetch('/api/search?q=' + encodeURIComponent(query), {
            headers: directorySearchHeaders(sequence)
        })
            .then(function (resp) {
            if (!resp.ok)
                return throwDirectorySearchHttpError(resp);
            return resp.json().catch(function (err) {
                err.type = 'parse';
                throw err;
            });
        })
            .then(function (data) {
            if (generation !== ctx.search.documentFetchGeneration)
                return;
            if (query !== ctx.search.currentDocumentQuery)
                return;
            if (!isSearchResponse(data, ctx.search.currentDocumentQuery)) {
                applyDirectorySearchContractViolation(data);
                return;
            }
            var response = data;
            ctx.search.currentDirectoryLoading = false;
            ctx.search.currentDirectoryError = '';
            ctx.search.currentDirectoryResults = response.results;
            ctx.search.currentDirectorySkippedFiles = response.skipped_files;
            ctx.search.currentDirectoryTruncated = response.truncated === true ||
                response.truncated_reasons.length > 0;
            ctx.search.currentDirectoryTruncatedReasons = response.truncated_reasons.slice();
            ctx.search.currentDirectoryIndex = resolveDirectorySearchIndex(ctx.search.currentDirectoryResults, preferredSelection);
            renderDirectorySearchUi();
        })
            .catch(function (err) {
            if (generation !== ctx.search.documentFetchGeneration)
                return;
            if (query !== ctx.search.currentDocumentQuery)
                return;
            ctx.search.currentDirectoryLoading = false;
            ctx.search.currentDirectoryResults = [];
            ctx.search.currentDirectoryIndex = -1;
            ctx.search.currentDirectorySkippedFiles = 0;
            ctx.search.currentDirectoryTruncated = false;
            ctx.search.currentDirectoryTruncatedReasons = [];
            ctx.search.currentDirectoryError = deps.getFileFetchErrorMessage(err);
            console.error('[markdown-view] ディレクトリ検索エラー:', err);
            renderDirectorySearchUi();
        });
    }
    function openDirectorySearchResult(index) {
        if (!ctx.search.currentDirectoryResults.length)
            return;
        var normalizedIndex = (index + ctx.search.currentDirectoryResults.length) % ctx.search.currentDirectoryResults.length;
        var result = ctx.search.currentDirectoryResults[normalizedIndex];
        var previousResultIndex = ctx.search.currentDirectoryIndex;
        ctx.search.currentDirectoryIndex = normalizedIndex;
        ctx.search.pendingDirectoryNavigation = {
            file: result.file,
            query: ctx.search.currentDocumentQuery,
            fileMatchIndex: result.file_match_index,
            resultIndex: normalizedIndex,
            previousResultIndex: previousResultIndex
        };
        renderDirectorySearchUi();
        if (result.file === ctx.state.currentFile) {
            applyPendingDirectorySearchNavigation();
            renderDirectorySearchUi();
            return;
        }
        deps.openFileSearchResult(result.file, {
            scrollMode: 'none',
            requeryDirectorySearch: false
        });
    }
    return {
        applyPendingDirectorySearchNavigation: applyPendingDirectorySearchNavigation,
        cancelDirectorySearch: cancelDirectorySearch,
        openDirectorySearchResult: openDirectorySearchResult,
        renderDirectorySearchResults: renderDirectorySearchResults,
        renderDirectorySearchUi: renderDirectorySearchUi,
        scheduleDirectorySearch: scheduleDirectorySearch
    };
}
