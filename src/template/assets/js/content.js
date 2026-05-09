var contentEnhancements = createContentEnhancements(appContext, {
  clearMemoSyncPendingStatus: clearMemoSyncPendingStatus
});
var setLiveStatus = contentEnhancements.setLiveStatus;
var updateDocumentStats = contentEnhancements.updateDocumentStats;
var updateReadingProgress = contentEnhancements.updateReadingProgress;
var syncDocumentChrome = contentEnhancements.syncDocumentChrome;
var enhanceContentInteractions = contentEnhancements.enhanceContentInteractions;
var setupFilterableList = contentEnhancements.setupFilterableList;
var setupTocFilter = contentEnhancements.setupTocFilter;

var contentNavigation = createContentNavigation(appContext, {
  selectFile: selectFile,
  setFileParam: setFileParam,
  markPendingTocNavigation: markPendingTocNavigation,
  clearPendingTocNavigation: clearPendingTocNavigation,
  restoreActiveTocHeading: restoreActiveTocHeading
});
var augmentHashWithTrailingLineHint = contentNavigation.augmentHashWithTrailingLineHint;
var applyContentAnchorNavigation = contentNavigation.applyContentAnchorNavigation;
var restoreContentNavigationFromLocation = contentNavigation.restoreContentNavigationFromLocation;
var setupContentLinkNavigation = contentNavigation.setupContentLinkNavigation;
var setupMemoLinkNavigation = contentNavigation.setupMemoLinkNavigation;
var setLocationHash = contentNavigation.setLocationHash;

var directorySearchController;
var documentSearchController = createDocumentSearchController(appContext, {
  activateSidebarTab: activateSidebarTab,
  applyPendingDirectorySearchNavigation: function() {
    return directorySearchController.applyPendingDirectorySearchNavigation();
  },
  openDirectorySearchResult: function(index) {
    return directorySearchController.openDirectorySearchResult(index);
  },
  renderDirectorySearchResults: function() {
    return directorySearchController.renderDirectorySearchResults();
  },
  renderDirectorySearchUi: function() {
    return directorySearchController.renderDirectorySearchUi();
  },
  scheduleDirectorySearch: function(query) {
    return directorySearchController.scheduleDirectorySearch(query);
  }
});

directorySearchController = createDirectorySearchController(appContext, {
  createDocumentSearchEmptyState: function(message) {
    return documentSearchController.createDocumentSearchEmptyState(message);
  },
  getFileFetchErrorMessage: getFileFetchErrorMessage,
  openFileSearchResult: function(file, options) {
    return selectFile(file, false, options);
  },
  renderDocumentSearchResultContext: function(container, text, query, variant) {
    return documentSearchController.renderDocumentSearchResultContext(container, text, query, variant);
  },
  setCurrentDocumentSearchMatch: function(index, scrollIntoView) {
    return documentSearchController.setCurrentDocumentSearchMatch(index, scrollIntoView);
  },
  updateDocumentSearchSummary: function() {
    return documentSearchController.updateDocumentSearchSummary();
  }
});

var applyDocumentSearchHighlights = documentSearchController.applyDocumentSearchHighlights;
var applyDocumentSearchQuery = documentSearchController.applyDocumentSearchQuery;
var clearDocumentSearchHighlights = documentSearchController.clearDocumentSearchHighlights;
var clearDocumentSearchQuery = documentSearchController.clearDocumentSearchQuery;
var createDocumentSearchEmptyState = documentSearchController.createDocumentSearchEmptyState;
var moveDocumentSearch = documentSearchController.moveDocumentSearch;
var openDocumentSearch = documentSearchController.openDocumentSearch;
var renderDocumentSearchResultContext = documentSearchController.renderDocumentSearchResultContext;
var renderDocumentSearchResults = documentSearchController.renderDocumentSearchResults;
var setCurrentDocumentSearchMatch = documentSearchController.setCurrentDocumentSearchMatch;
var setupDocumentSearch = documentSearchController.setupDocumentSearch;
var syncDocumentSearchAfterContentUpdate = documentSearchController.syncDocumentSearchAfterContentUpdate;
var updateDocumentSearchSummary = documentSearchController.updateDocumentSearchSummary;

var applyPendingDirectorySearchNavigation = directorySearchController.applyPendingDirectorySearchNavigation;
var openDirectorySearchResult = directorySearchController.openDirectorySearchResult;
var renderDirectorySearchResults = directorySearchController.renderDirectorySearchResults;
var renderDirectorySearchUi = directorySearchController.renderDirectorySearchUi;
var scheduleDirectorySearch = directorySearchController.scheduleDirectorySearch;

function applyPendingUpdate() {
  if (!appContext.state.pendingUpdate) return;
  if (appContext.state.pendingUpdateTimer) {
    clearTimeout(appContext.state.pendingUpdateTimer);
    appContext.state.pendingUpdateTimer = null;
  }
  if (appContext.state.pendingUpdate.refresh) {
    var refreshFile = appContext.state.pendingUpdate.file;
    appContext.state.pendingUpdate = null;
    if (appContext.config.isDirMode && refreshFile) {
      selectFile(refreshFile, false);
    } else {
      console.warn('[markdown-view] refresh 保留更新を適用できませんでした。', {
        isDirMode: appContext.config.isDirMode,
        refreshFile: refreshFile || '',
        currentFile: appContext.state.currentFile || ''
      });
    }
    return;
  }
  if (appContext.config.isDirMode && appContext.state.pendingUpdate.file && appContext.state.pendingUpdate.file !== appContext.state.currentFile) {
    console.warn('[markdown-view] 現在のファイルと異なる保留更新を破棄しました。', {
      currentFile: appContext.state.currentFile,
      messageFile: appContext.state.pendingUpdate.file
    });
    appContext.state.pendingUpdate = null;
    return;
  }
  var data = appContext.state.pendingUpdate;
  appContext.state.pendingUpdate = null;
  updateContent(data);
  hideWsServerErrorBanner();
  hideFileFetchErrorBanner();
  setLiveStatus('live');
}

// サーバーサイドでサニタイズ済みのHTMLを反映する
// XSS防止: src/renderer/render.rs で raw/inline HTML event を破棄済み
let updateContent = function updateContent(data, options) {
  options = options || {};
  var validation = validateUpdatePayload(data);
  var safeData = validation.safeData;

  if (validation.hasContractViolation) {
    logUpdatePayloadContractViolation(validation);
  }

  if (appContext.state.pendingUpdateTimer) {
    clearTimeout(appContext.state.pendingUpdateTimer);
    appContext.state.pendingUpdateTimer = null;
  }
  appContext.state.pendingUpdate = null;
  var scrollY = window.scrollY;
  var scrollMode = options.scrollMode || 'preserve';
  var preservedActiveTocId = getCurrentActiveTocId();
  var contentEl = document.getElementById('content');
  var tocEl = document.getElementById('toc');

  applyValidatedUpdateHtml(appContext, {
    contentEl: contentEl,
    tocEl: tocEl
  }, validation);

  setupTocTracking();
  suppressTocTrackingFor(120);

  requestAnimationFrame(function() {
    var currentScrollY = window.scrollY || window.pageYOffset;
    var anchorApplied = false;
    if (scrollMode === 'preserve' && Math.abs(currentScrollY - scrollY) <= 1) {
      window.scrollTo(0, scrollY);
    } else if (scrollMode === 'reset') {
      window.scrollTo(0, 0);
    }
    if (options.anchorHash) {
      anchorApplied = applyContentAnchorNavigation(options.anchorHash, true);
      if (!anchorApplied) {
        console.warn('[markdown-view] リンク先の見出しが見つかりません:', options.anchorHash);
        window.scrollTo(0, 0);
        // 既定はhash除去。popstate経路のみ明示的にfalseを渡して、
        // ユーザーが戻る/進むで辿れるはずの履歴エントリURLの破壊を避ける。
        if (options.clearHashOnMiss !== false) {
          setLocationHash('', true);
        }
        clearPendingTocNavigation();
      }
    }
    updateReadingProgress();
    restoreActiveTocHeading(preservedActiveTocId);
  });

  updateDocumentStats();
  syncDocumentChrome(appContext.state.currentFile);
  enhanceContentInteractions();
  syncDocumentSearchAfterContentUpdate(options);
  setupTocFilter();
  hideQuoteSelectionAction();
  if (!validation.hasContractViolation && appContext.websocket) {
    appContext.websocket.rememberAppliedLiveUpdate(safeData);
  }
};
