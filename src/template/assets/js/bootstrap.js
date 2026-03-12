'use strict';
var MAX_FILE_SIZE_MB = __MAX_FILE_SIZE_MB__;

// グローバルDOM参照の初期化
var htmlEl = document.documentElement;
var isDirMode = htmlEl.getAttribute('data-dir-mode') === 'true';
var currentFile = htmlEl.getAttribute('data-current-file') || '';
var documentTitleEl = document.getElementById('document-title');
var docHeadingCountEl = document.getElementById('doc-heading-count');
var docCharCountEl = document.getElementById('doc-char-count');
var liveStatusEl = document.getElementById('live-status');
var readingProgressBar = document.getElementById('reading-progress-bar');
var backToTop = document.getElementById('back-to-top');
var contentRoot = document.getElementById('content');
var documentSearchInputEl = document.getElementById('document-search-input');
var documentSearchSummaryEl = document.getElementById('document-search-summary');
var documentSearchResultsEl = document.getElementById('document-search-results');
var documentSearchPrevEl = document.getElementById('document-search-prev');
var documentSearchNextEl = document.getElementById('document-search-next');
var documentSearchClearEl = document.getElementById('document-search-clear');
var memoEditorEl = document.getElementById('memo-editor');
var memoPreviewEl = document.getElementById('memo-preview');
var memoSaveStatusEl = document.getElementById('memo-save-status');
var quoteSelectionActionEl = document.getElementById('quote-selection-action');

// テキスト選択中のDOM更新延期機構
// マウスドラッグ中にWebSocket経由のinnerHTML更新が走ると選択が破壊されるため、
// 選択操作中は更新を保留し、選択解除（selectionchange + isCollapsed）後に適用する。
// 選択が長時間維持される場合は30秒タイムアウトでフォールバック適用する。
var pendingUpdate = null;
var pendingUpdateTimer = null;
var isMouseSelecting = false;
var memoLoadGeneration = 0;
var memoSaveGeneration = 0;
var memoSaveTimer = null;
var memoCaretStart = memoEditorEl ? memoEditorEl.value.length : 0;
var memoCaretEnd = memoCaretStart;
var documentSearchMatches = [];
var currentDocumentSearchIndex = -1;
var currentDocumentSearchQuery = '';
var LIVE_STATUS_LABELS = {
  live: 'Live',
  retry: 'Reconnecting',
  error: 'Error',
  offline: 'Offline'
};
