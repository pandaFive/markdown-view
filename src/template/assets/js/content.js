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

var documentSearchController = createDocumentSearchController(appContext, {
  activateSidebarTab: activateSidebarTab,
  applyPendingDirectorySearchNavigation: function() {
    return applyPendingDirectorySearchNavigation();
  },
  openDirectorySearchResult: function(index) {
    return openDirectorySearchResult(index);
  },
  renderDirectorySearchResults: function() {
    return renderDirectorySearchResults();
  },
  renderDirectorySearchUi: function() {
    return renderDirectorySearchUi();
  },
  scheduleDirectorySearch: function(query) {
    return scheduleDirectorySearch(query);
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

function createDirectorySearchTruncatedState() {
  var item = document.createElement('div');
  item.className = 'document-search-empty';
  item.textContent = '上限により一部のみ表示しています。';
  return item;
}

function renderDirectorySearchResults() {
  var preservedScrollTop = appContext.elements.documentSearchResultsEl.scrollTop;
  appContext.elements.documentSearchResultsEl.innerHTML = '';

  if (!appContext.search.currentDocumentQuery) return;

  if (appContext.search.currentDirectoryLoading) {
    appContext.elements.documentSearchResultsEl.appendChild(createDocumentSearchEmptyState('ディレクトリを検索しています。'));
    return;
  }

  if (appContext.search.currentDirectoryError) {
    appContext.elements.documentSearchResultsEl.appendChild(createDocumentSearchEmptyState(appContext.search.currentDirectoryError));
    return;
  }

  if (appContext.search.currentDirectoryTruncated) {
    appContext.elements.documentSearchResultsEl.appendChild(createDirectorySearchTruncatedState());
  }

  if (!appContext.search.currentDirectoryResults.length) {
    appContext.elements.documentSearchResultsEl.appendChild(createDocumentSearchEmptyState('ディレクトリ内に一致が見つかりません。'));
    return;
  }

  appContext.search.currentDirectoryResults.forEach(function(result, index) {
    var button = document.createElement('button');
    var indexBadge = document.createElement('span');
    var body = document.createElement('span');
    var path = document.createElement('span');

    button.type = 'button';
    button.className = 'document-search-result';
    button.dataset.resultIndex = String(index);
    button.classList.toggle('active', index === appContext.search.currentDirectoryIndex);
    button.setAttribute('aria-current', index === appContext.search.currentDirectoryIndex ? 'true' : 'false');
    button.addEventListener('click', function() {
      openDirectorySearchResult(index);
    });

    indexBadge.className = 'document-search-result-index';
    indexBadge.textContent = String(index + 1).padStart(2, '0');

    body.className = 'document-search-result-body';
    path.className = 'document-search-result-path';
    path.textContent = result.file;
    body.appendChild(path);
    renderDocumentSearchResultContext(body, result.before, appContext.search.currentDocumentQuery, 'before');
    renderDocumentSearchResultContext(body, result.current, appContext.search.currentDocumentQuery, 'current');
    renderDocumentSearchResultContext(body, result.after, appContext.search.currentDocumentQuery, 'after');

    button.appendChild(indexBadge);
    button.appendChild(body);
    appContext.elements.documentSearchResultsEl.appendChild(button);
  });

  appContext.elements.documentSearchResultsEl.scrollTop = preservedScrollTop;
}

function renderDirectorySearchUi() {
  updateDocumentSearchSummary();
  renderDocumentSearchResults();
}

function applyPendingDirectorySearchNavigation() {
  if (!appContext.config.isDirMode || !appContext.search.pendingDirectoryNavigation) return;
  if (appContext.search.pendingDirectoryNavigation.file !== appContext.state.currentFile) return;
  if (appContext.search.pendingDirectoryNavigation.query !== appContext.search.currentDocumentQuery) {
    appContext.search.pendingDirectoryNavigation = null;
    return;
  }
  if (appContext.search.documentMatches.length) {
    setCurrentDocumentSearchMatch(
      Math.min(appContext.search.pendingDirectoryNavigation.fileMatchIndex, appContext.search.documentMatches.length - 1)
    );
  }
  appContext.search.currentDirectoryIndex = appContext.search.pendingDirectoryNavigation.resultIndex;
  appContext.search.pendingDirectoryNavigation = null;
}

function scheduleDirectorySearch(query) {
  if (appContext.search.documentDebounceTimer) {
    clearTimeout(appContext.search.documentDebounceTimer);
  }
  appContext.search.documentDebounceTimer = setTimeout(function() {
    appContext.search.documentDebounceTimer = null;
    runDirectorySearch(query);
  }, 300);
}

function getPreferredDirectorySearchSelection() {
  if (appContext.search.pendingDirectoryNavigation) {
    return {
      file: appContext.search.pendingDirectoryNavigation.file,
      fileMatchIndex: appContext.search.pendingDirectoryNavigation.fileMatchIndex
    };
  }
  if (
    appContext.search.currentDirectoryIndex >= 0 &&
    appContext.search.currentDirectoryIndex < appContext.search.currentDirectoryResults.length
  ) {
    return {
      file: appContext.search.currentDirectoryResults[appContext.search.currentDirectoryIndex].file,
      fileMatchIndex: appContext.search.currentDirectoryResults[appContext.search.currentDirectoryIndex].file_match_index
    };
  }
  if (
    appContext.state.currentFile &&
    appContext.search.currentDocumentIndex >= 0 &&
    appContext.search.currentDocumentIndex < appContext.search.documentMatches.length
  ) {
    return {
      file: appContext.state.currentFile,
      fileMatchIndex: appContext.search.currentDocumentIndex
    };
  }
  return null;
}

function resolveDirectorySearchIndex(results, preferredSelection) {
  var index;
  if (!results.length) return -1;
  if (preferredSelection) {
    index = results.findIndex(function(result) {
      return (
        result.file === preferredSelection.file &&
        result.file_match_index === preferredSelection.fileMatchIndex
      );
    });
    if (index !== -1) return index;
  }
  if (
    appContext.state.currentFile &&
    appContext.search.currentDocumentIndex >= 0 &&
    appContext.search.currentDocumentIndex < appContext.search.documentMatches.length
  ) {
    index = results.findIndex(function(result) {
      return (
        result.file === appContext.state.currentFile &&
        result.file_match_index === appContext.search.currentDocumentIndex
      );
    });
    if (index !== -1) return index;
  }
  return -1;
}

function runDirectorySearch(query) {
  var generation = ++appContext.search.documentFetchGeneration;
  var preferredSelection = getPreferredDirectorySearchSelection();
  appContext.search.currentDirectoryLoading = true;
  appContext.search.currentDirectoryError = '';
  appContext.search.currentDirectoryResults = [];
  appContext.search.currentDirectoryIndex = -1;
  appContext.search.currentDirectorySkippedFiles = 0;
  appContext.search.currentDirectoryTruncated = false;
  appContext.search.currentDirectoryTruncatedReasons = [];
  renderDirectorySearchUi();

  fetch('/api/search?q=' + encodeURIComponent(query), {
    headers: { 'Accept': 'application/json' }
  })
  .then(function(resp) {
    if (!resp.ok) throw createHttpError(resp.status);
    return resp.json().catch(function(err) {
      err.type = 'parse';
      throw err;
    });
  })
  .then(function(data) {
    if (generation !== appContext.search.documentFetchGeneration) return;
    if ((data.query || '') !== appContext.search.currentDocumentQuery) return;
    appContext.search.currentDirectoryLoading = false;
    appContext.search.currentDirectoryError = '';
    appContext.search.currentDirectoryResults = Array.isArray(data.results) ? data.results : [];
    appContext.search.currentDirectorySkippedFiles = Number(data.skipped_files || 0);
    appContext.search.currentDirectoryTruncated = data.truncated === true ||
      (Array.isArray(data.truncated_reasons) && data.truncated_reasons.length > 0);
    appContext.search.currentDirectoryTruncatedReasons = Array.isArray(data.truncated_reasons)
      ? data.truncated_reasons.slice()
      : [];
    appContext.search.currentDirectoryIndex = resolveDirectorySearchIndex(
      appContext.search.currentDirectoryResults,
      preferredSelection
    );
    renderDirectorySearchUi();
  })
  .catch(function(err) {
    if (generation !== appContext.search.documentFetchGeneration) return;
    if (query !== appContext.search.currentDocumentQuery) return;
    appContext.search.currentDirectoryLoading = false;
    appContext.search.currentDirectoryResults = [];
    appContext.search.currentDirectoryIndex = -1;
    appContext.search.currentDirectorySkippedFiles = 0;
    appContext.search.currentDirectoryTruncated = false;
    appContext.search.currentDirectoryTruncatedReasons = [];
    appContext.search.currentDirectoryError = getFileFetchErrorMessage(err);
    console.error('[markdown-view] ディレクトリ検索エラー:', err);
    renderDirectorySearchUi();
  });
}

function openDirectorySearchResult(index) {
  if (!appContext.search.currentDirectoryResults.length) return;
  var normalizedIndex = (index + appContext.search.currentDirectoryResults.length) % appContext.search.currentDirectoryResults.length;
  var result = appContext.search.currentDirectoryResults[normalizedIndex];
  var previousResultIndex = appContext.search.currentDirectoryIndex;
  appContext.search.currentDirectoryIndex = normalizedIndex;
  appContext.search.pendingDirectoryNavigation = {
    file: result.file,
    query: appContext.search.currentDocumentQuery,
    fileMatchIndex: result.file_match_index,
    resultIndex: normalizedIndex,
    previousResultIndex: previousResultIndex
  };
  renderDirectorySearchUi();

  if (result.file === appContext.state.currentFile) {
    applyPendingDirectorySearchNavigation();
    renderDirectorySearchUi();
    return;
  }

  selectFile(result.file, false, {
    scrollMode: 'none',
    requeryDirectorySearch: false
  });
}

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
