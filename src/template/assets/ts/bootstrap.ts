'use strict';

var DIRECTORY_SEARCH_CLIENT_ID_STORAGE_KEY = 'markdown-view.directorySearchClientId';

function isValidDirectorySearchClientId(clientId: unknown): clientId is string {
  return typeof clientId === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(clientId);
}

function createDirectorySearchClientId(): string {
  if (window.crypto && typeof window.crypto.getRandomValues === 'function') {
    var randomParts = new Uint32Array(2);
    var randomPartA: number;
    var randomPartB: number;
    window.crypto.getRandomValues(randomParts);
    randomPartA = randomParts[0] || 0;
    randomPartB = randomParts[1] || 0;
    return 'tab-' + Date.now().toString(36) + '-' +
      randomPartA.toString(36) + randomPartB.toString(36);
  }
  return 'tab-' + Date.now().toString(36) + '-' +
    Math.random().toString(36).slice(2, 12);
}

function isReloadNavigation(): boolean {
  var entries: PerformanceEntryList;
  var navigation: PerformanceEntry | undefined;

  if (!window.performance || typeof window.performance.getEntriesByType !== 'function') {
    return false;
  }

  entries = window.performance.getEntriesByType('navigation');
  navigation = entries[0];
  return typeof navigation !== 'undefined' &&
    'type' in navigation &&
    (navigation as PerformanceNavigationTiming).type === 'reload';
}

function getDirectorySearchClientId(): string {
  var storage: Storage | null;
  var storedClientId: string | null;
  var generatedClientId: string;

  try {
    storage = window.sessionStorage;
    storedClientId = storage && isReloadNavigation()
      ? storage.getItem(DIRECTORY_SEARCH_CLIENT_ID_STORAGE_KEY)
      : null;
    if (isValidDirectorySearchClientId(storedClientId)) {
      return storedClientId;
    }
  } catch (_storageReadError) {
    storage = null;
  }

  generatedClientId = createDirectorySearchClientId();
  if (storage) {
    try {
      storage.setItem(DIRECTORY_SEARCH_CLIENT_ID_STORAGE_KEY, generatedClientId);
    } catch (_storageWriteError) {
      // sessionStorage が使えない環境では今回生成したIDだけを使う。
    }
  }

  return generatedClientId;
}

function createAppContext(doc: Document): MarkdownViewAppContext {
  var html = doc.documentElement;
  var memoEditor = doc.getElementById('memo-editor') as HTMLTextAreaElement | null;
  var contentEl = doc.getElementById('content');
  var tocEl = doc.getElementById('toc');
  var memoCaret = memoEditor ? memoEditor.value.length : 0;

  return {
    config: {
      maxFileSizeMb: __MAX_FILE_SIZE_MB__,
      isDirMode: html.getAttribute('data-dir-mode') === 'true'
    },
    state: {
      currentFile: html.getAttribute('data-current-file') || '',
      // updateContent の no-op 判定キャッシュ。
      // 初期化部が enhanceContentInteractions() で heading-anchor / code-copy ボタンを
      // #content に追記するため、SSR 時点の contentRoot.innerHTML は WS 経由 data.content
      // と必ず乖離する（副次的にブラウザの HTML 正規化差もある）。DOM ではなく最後に
      // 適用した data.content 文字列を比較対象にすることで、初回 broadcast 以降の
      // .jump-highlight 等の一時 DOM 状態を不要に壊さない。
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
      contentEl: contentEl,
      contentRoot: contentEl,
      tocEl: tocEl,
      tocRoot: tocEl,
      documentSearchInputEl: doc.getElementById('document-search-input') as HTMLInputElement | null,
      documentSearchSummaryEl: doc.getElementById('document-search-summary'),
      documentSearchResultsEl: doc.getElementById('document-search-results'),
      documentSearchPrevEl: doc.getElementById('document-search-prev') as HTMLButtonElement | null,
      documentSearchNextEl: doc.getElementById('document-search-next') as HTMLButtonElement | null,
      documentSearchClearEl: doc.getElementById('document-search-clear') as HTMLButtonElement | null,
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
      pendingSaveGenerations: [],
      saveTimer: null,
      caretStart: memoCaret,
      caretEnd: memoCaret,
      previousLoadStatus: null,
      pendingReload: null
    },
    search: {
      documentMatches: [],
      currentDocumentIndex: -1,
      currentDocumentQuery: '',
      currentDirectoryResults: [],
      currentDirectoryIndex: -1,
      currentDirectorySkippedFiles: 0,
      currentDirectoryTruncated: false,
      currentDirectoryTruncatedReasons: [],
      currentDirectoryLoading: false,
      currentDirectoryError: '',
      documentDebounceTimer: null,
      documentFetchGeneration: 0,
      directorySearchClientId: getDirectorySearchClientId(),
      directorySearchSequence: 0,
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
      tocRoot: tocEl
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
    websocket: null,
    content: null
  };
}

var appContext: MarkdownViewAppContext = createAppContext(document);
