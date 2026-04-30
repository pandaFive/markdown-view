'use strict';
var MAX_FILE_SIZE_MB = __MAX_FILE_SIZE_MB__;

function createAppContext(doc) {
  var html = doc.documentElement;
  var memoEditor = doc.getElementById('memo-editor');
  var memoCaret = memoEditor ? memoEditor.value.length : 0;

  return {
    config: {
      maxFileSizeMb: MAX_FILE_SIZE_MB,
      isDirMode: html.getAttribute('data-dir-mode') === 'true'
    },
    state: {
      currentFile: html.getAttribute('data-current-file') || '',
      lastAppliedContent: null,
      pendingUpdate: null,
      pendingUpdateTimer: null,
      isMouseSelecting: false
    },
    elements: {
      htmlEl: html,
      documentTitleEl: doc.getElementById('document-title'),
      docHeadingCountEl: doc.getElementById('doc-heading-count'),
      docCharCountEl: doc.getElementById('doc-char-count'),
      liveStatusEl: doc.getElementById('live-status'),
      readingProgressBar: doc.getElementById('reading-progress-bar'),
      backToTop: doc.getElementById('back-to-top'),
      contentRoot: doc.getElementById('content'),
      documentSearchInputEl: doc.getElementById('document-search-input'),
      documentSearchSummaryEl: doc.getElementById('document-search-summary'),
      documentSearchResultsEl: doc.getElementById('document-search-results'),
      documentSearchPrevEl: doc.getElementById('document-search-prev'),
      documentSearchNextEl: doc.getElementById('document-search-next'),
      documentSearchClearEl: doc.getElementById('document-search-clear'),
      memoEditorEl: memoEditor,
      memoPreviewEl: doc.getElementById('memo-preview'),
      memoSaveStatusEl: doc.getElementById('memo-save-status'),
      quoteSelectionActionEl: doc.getElementById('quote-selection-action')
    },
    fetch: {
      generation: 0
    },
    memo: {
      loadGeneration: 0,
      saveGeneration: 0,
      saveTimer: null,
      caretStart: memoCaret,
      caretEnd: memoCaret,
      pendingReload: null
    },
    search: {
      documentMatches: [],
      currentDocumentIndex: -1,
      currentDocumentQuery: '',
      currentDirectoryResults: [],
      currentDirectoryIndex: -1,
      currentDirectorySkippedFiles: 0,
      currentDirectoryLoading: false,
      currentDirectoryError: '',
      documentDebounceTimer: null,
      documentFetchGeneration: 0,
      pendingDirectoryNavigation: null
    },
    sidebar: {
      currentTocTracking: null,
      tocTrackingFrame: null,
      currentActiveTocId: '',
      suppressTocTrackingUntil: 0,
      suppressTocTrackingTimer: null,
      pendingSuppressedTocTrackingUpdate: false,
      pendingTocNavigationId: '',
      pendingTocNavigationUntil: 0,
      tocRoot: doc.getElementById('toc')
    },
    labels: {
      liveStatus: {
        live: 'Live',
        retry: 'Reconnecting',
        error: 'Error',
        offline: 'Offline'
      }
    },
    test: {
      markPendingTocNavigationObserver: null
    },
    websocket: null
  };
}

var appContext = createAppContext(document);
