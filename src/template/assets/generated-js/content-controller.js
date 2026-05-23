"use strict";
function createContentController(ctx, deps) {
    var enhancements = createContentEnhancements(ctx, {
        clearMemoSyncPendingStatus: deps.clearMemoSyncPendingStatus
    });
    var navigation = createContentNavigation(ctx, {
        selectFile: deps.selectFile,
        setFileParam: deps.setFileParam,
        markPendingTocNavigation: deps.markPendingTocNavigation,
        clearPendingTocNavigation: deps.clearPendingTocNavigation,
        restoreActiveTocHeading: deps.restoreActiveTocHeading
    });
    var directorySearch = null;
    function requireDirectorySearch() {
        if (!directorySearch) {
            throw new Error('directory search controller is not initialized');
        }
        return directorySearch;
    }
    var documentSearch = createDocumentSearchController(ctx, {
        activateSidebarTab: deps.activateSidebarTab,
        applyPendingDirectorySearchNavigation: function () {
            return requireDirectorySearch().applyPendingDirectorySearchNavigation();
        },
        openDirectorySearchResult: function (index) {
            return requireDirectorySearch().openDirectorySearchResult(index);
        },
        renderDirectorySearchResults: function () {
            return requireDirectorySearch().renderDirectorySearchResults();
        },
        renderDirectorySearchUi: function () {
            return requireDirectorySearch().renderDirectorySearchUi();
        },
        cancelDirectorySearch: function () {
            return requireDirectorySearch().cancelDirectorySearch();
        },
        scheduleDirectorySearch: function (query) {
            return requireDirectorySearch().scheduleDirectorySearch(query);
        }
    });
    directorySearch = createDirectorySearchController(ctx, {
        createHttpError: deps.createHttpError,
        createDocumentSearchEmptyState: function (message) {
            return documentSearch.createDocumentSearchEmptyState(message);
        },
        getFileFetchErrorMessage: deps.getFileFetchErrorMessage,
        openFileSearchResult: function (file, options) {
            return deps.selectFile(file, false, options);
        },
        renderDocumentSearchResultContext: function (container, text, query, variant) {
            return documentSearch.renderDocumentSearchResultContext(container, text, query, variant);
        },
        setCurrentDocumentSearchMatch: function (index, scrollIntoView) {
            return documentSearch.setCurrentDocumentSearchMatch(index, scrollIntoView);
        },
        updateDocumentSearchSummary: function () {
            return documentSearch.updateDocumentSearchSummary();
        }
    });
    function setup() {
        documentSearch.setupDocumentSearch();
        navigation.setupContentLinkNavigation();
        navigation.setupMemoLinkNavigation();
    }
    function applyPendingUpdate() {
        if (!ctx.state.pendingUpdate)
            return;
        if (ctx.state.pendingUpdateTimer) {
            clearTimeout(ctx.state.pendingUpdateTimer);
            ctx.state.pendingUpdateTimer = null;
        }
        if (ctx.state.pendingUpdate.refresh) {
            var refreshFile = ctx.state.pendingUpdate.file;
            ctx.state.pendingUpdate = null;
            if (ctx.config.isDirMode && refreshFile) {
                deps.selectFile(refreshFile, false);
            }
            else {
                console.warn('[markdown-view] refresh 保留更新を適用できませんでした。', {
                    isDirMode: ctx.config.isDirMode,
                    refreshFile: refreshFile || '',
                    currentFile: ctx.state.currentFile || ''
                });
            }
            return;
        }
        if (ctx.config.isDirMode && ctx.state.pendingUpdate.file && ctx.state.pendingUpdate.file !== ctx.state.currentFile) {
            console.warn('[markdown-view] 現在のファイルと異なる保留更新を破棄しました。', {
                currentFile: ctx.state.currentFile,
                messageFile: ctx.state.pendingUpdate.file
            });
            ctx.state.pendingUpdate = null;
            return;
        }
        var data = ctx.state.pendingUpdate;
        ctx.state.pendingUpdate = null;
        var result = updateContent(data);
        if (result.contractViolation) {
            if (deps.showWsServerErrorBanner) {
                deps.showWsServerErrorBanner('サーバーから不正な更新データを受信しました。ページを再読み込みしてください。');
            }
            enhancements.setLiveStatus('error');
            return;
        }
        deps.hideWsServerErrorBanner();
        deps.hideFileFetchErrorBanner();
        enhancements.setLiveStatus('live');
    }
    // サーバーサイドでサニタイズ済みのHTMLを反映する
    // XSS防止: src/renderer/render.rs で raw/inline HTML event を破棄済み
    function updateContent(data, options) {
        options = options || {};
        var validation = validateUpdatePayload(data);
        var safeData = validation.safeData;
        if (validation.hasContractViolation) {
            logUpdatePayloadContractViolation(validation);
            return {
                ok: false,
                contractViolation: true,
                missing: validation.missing.slice()
            };
        }
        if (ctx.state.pendingUpdateTimer) {
            clearTimeout(ctx.state.pendingUpdateTimer);
            ctx.state.pendingUpdateTimer = null;
        }
        ctx.state.pendingUpdate = null;
        var scrollY = window.scrollY;
        var scrollMode = options.scrollMode || 'preserve';
        var preservedActiveTocId = deps.getCurrentActiveTocId();
        var contentEl = document.getElementById('content');
        var tocEl = document.getElementById('toc');
        applyValidatedUpdateHtml(ctx, {
            contentEl: contentEl,
            tocEl: tocEl
        }, validation);
        deps.setupTocTracking();
        deps.suppressTocTrackingFor(120);
        requestAnimationFrame(function () {
            var currentScrollY = window.scrollY || window.pageYOffset;
            var anchorApplied = false;
            if (scrollMode === 'preserve' && Math.abs(currentScrollY - scrollY) <= 1) {
                window.scrollTo(0, scrollY);
            }
            else if (scrollMode === 'reset') {
                window.scrollTo(0, 0);
            }
            if (options.anchorHash) {
                anchorApplied = navigation.applyContentAnchorNavigation(options.anchorHash, true);
                if (!anchorApplied) {
                    console.warn('[markdown-view] リンク先の見出しが見つかりません:', options.anchorHash);
                    window.scrollTo(0, 0);
                    // 既定はhash除去。popstate経路のみ明示的にfalseを渡して、
                    // ユーザーが戻る/進むで辿れるはずの履歴エントリURLの破壊を避ける。
                    if (options.clearHashOnMiss !== false) {
                        navigation.setLocationHash('', true);
                    }
                    deps.clearPendingTocNavigation();
                }
            }
            enhancements.updateReadingProgress();
            deps.restoreActiveTocHeading(preservedActiveTocId);
        });
        enhancements.updateDocumentStats();
        enhancements.syncDocumentChrome(ctx.state.currentFile);
        enhancements.enhanceContentInteractions();
        documentSearch.syncDocumentSearchAfterContentUpdate(options);
        enhancements.setupTocFilter();
        deps.hideQuoteSelectionAction();
        if (!validation.hasContractViolation && ctx.websocket) {
            ctx.websocket.rememberAppliedLiveUpdate(safeData);
        }
        return {
            ok: true,
            contractViolation: false,
            missing: []
        };
    }
    return {
        setup: setup,
        updateContent: updateContent,
        applyPendingUpdate: applyPendingUpdate,
        restoreNavigationFromLocation: navigation.restoreContentNavigationFromLocation,
        openDocumentSearch: documentSearch.openDocumentSearch,
        moveDocumentSearch: documentSearch.moveDocumentSearch,
        applyDocumentSearchQuery: documentSearch.applyDocumentSearchQuery,
        clearDocumentSearchQuery: documentSearch.clearDocumentSearchQuery,
        renderDirectorySearchUi: directorySearch.renderDirectorySearchUi,
        scheduleDirectorySearch: directorySearch.scheduleDirectorySearch,
        augmentHashWithTrailingLineHint: navigation.augmentHashWithTrailingLineHint,
        setLiveStatus: enhancements.setLiveStatus,
        updateDocumentStats: enhancements.updateDocumentStats,
        updateReadingProgress: enhancements.updateReadingProgress,
        syncDocumentChrome: enhancements.syncDocumentChrome,
        enhanceContentInteractions: enhancements.enhanceContentInteractions,
        setupTocFilter: enhancements.setupTocFilter
    };
}
