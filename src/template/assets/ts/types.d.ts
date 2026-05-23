type MemoState = 'ready' | 'degraded';
type LiveStatusState = 'connected' | 'disconnected' | 'error' | 'live' | 'retry' | 'offline';

type SearchTruncationReason = 'result_limit' | 'file_limit' | 'byte_limit';

interface SearchResult {
  file: string;
  line?: number;
  snippet?: string;
  file_match_index?: number;
  before?: string;
  current?: string;
  after?: string;
}

interface SearchResponse {
  query?: string;
  results: SearchResult[];
  searched_files: number;
  skipped_files?: number;
  searched_bytes: number;
  truncated: boolean;
  truncated_reasons: SearchTruncationReason[];
  limits?: {
    max_results: number;
    max_files: number;
    max_bytes: number;
  };
}

interface ContentUpdatePayload {
  content?: string;
  toc?: string;
  file?: string | null;
  refresh?: boolean;
  memo_refresh?: boolean;
  memo_file?: string;
  type?: string;
  error?: string;
  raw?: string;
  html?: string;
  load_error?: string;
  memo_state?: MemoState;
}

interface ContentUpdateValidation {
  safeData: ContentUpdatePayload;
  missing: string[];
  hasContractViolation: boolean;
}

interface ContentUpdateTargets {
  contentEl: HTMLElement | null;
  tocEl: HTMLElement | null;
}

interface ContentUpdateResult {
  ok: boolean;
  contractViolation: boolean;
  missing: string[];
}

interface ContentUpdateOptions {
  scrollMode?: 'preserve' | 'reset';
  anchorHash?: string;
  clearHashOnMiss?: boolean;
  requeryDirectorySearch?: boolean;
  historyHash?: string;
}

interface MarkdownViewConfig {
  maxFileSizeMb: number;
  isDirMode: boolean;
}

interface MarkdownViewState {
  currentFile: string;
  lastAppliedContent: string | null;
  pendingUpdate: ContentUpdatePayload | null;
  pendingUpdateTimer: number | null;
  isMouseSelecting: boolean;
}

interface MarkdownViewFetchState {
  generation: number;
}

interface MarkdownViewMemoState {
  loadGeneration: number;
  saveGeneration: number;
  pendingSaveGenerations: number[];
  saveTimer: number | null;
  caretStart: number;
  caretEnd: number;
  previousLoadStatus: {
    state: string;
    text: string;
  } | null;
  pendingReload: string | null;
}

interface MarkdownViewSearchState {
  documentMatches: unknown[];
  currentDocumentIndex: number;
  currentDocumentQuery: string;
  currentDirectoryResults: SearchResult[];
  currentDirectoryIndex: number;
  currentDirectorySkippedFiles: number;
  currentDirectoryTruncated: boolean;
  currentDirectoryTruncatedReasons: SearchTruncationReason[];
  currentDirectoryLoading: boolean;
  currentDirectoryError: string;
  documentDebounceTimer: number | null;
  documentFetchGeneration: number;
  directorySearchClientId: string;
  directorySearchSequence: number;
  pendingDirectoryNavigation: {
    file: string;
    previousResultIndex: number;
  } | null;
}

interface MarkdownViewSidebarState {
  currentTocTracking: any | null;
  tocTrackingFrame: number | null;
  currentActiveTocId: string;
  suppressTocTrackingUntil: number;
  suppressTocTrackingTimer: number | null;
  pendingSuppressedTocTrackingUpdate: boolean;
  pendingTocNavigationId: string;
  pendingTocNavigationUntil: number;
  tocRoot: HTMLElement | null;
}

interface MarkdownViewLabels {
  liveStatus: {
    live: string;
    retry: string;
    error: string;
    offline: string;
  };
}

interface MarkdownViewTestState {
  markPendingTocNavigationObserver: unknown | null;
}

interface MarkdownViewContentController {
  setup(): void;
  updateContent(data: ContentUpdatePayload, options?: ContentUpdateOptions): ContentUpdateResult;
  applyPendingUpdate(): void;
  restoreNavigationFromLocation(): void;
  openDocumentSearch(): void;
  moveDocumentSearch(step: number): void;
  applyDocumentSearchQuery(query: string): void;
  clearDocumentSearchQuery(): void;
  renderDirectorySearchUi(): void;
  scheduleDirectorySearch(query: string): void;
  augmentHashWithTrailingLineHint(link: string, hash: string): string;
  setLiveStatus(state: LiveStatusState): void;
  updateDocumentStats(): void;
  updateReadingProgress(): void;
  syncDocumentChrome(file: string): void;
  enhanceContentInteractions(): void;
  setupTocFilter(): void;
}

interface MarkdownViewWebSocketController {
  connect(): void;
  discardBufferedLiveUpdate(reason: string): void;
  rememberAppliedLiveUpdate(data: ContentUpdatePayload): void;
  scheduleBufferedLiveUpdate(data: ContentUpdatePayload): void;
}

interface MemoResponse {
  raw?: string;
  content?: string;
  html: string;
  file?: string | null;
  load_error?: string;
  memo_state?: MemoState;
}

interface MarkdownViewElements {
  htmlEl: HTMLElement;
  contentEl: HTMLElement | null;
  contentRoot: HTMLElement | null;
  tocEl: HTMLElement | null;
  tocRoot: HTMLElement | null;
  documentTitleEl: HTMLElement | null;
  docHeadingCountEl: HTMLElement | null;
  docCharCountEl: HTMLElement | null;
  liveStatusEl: HTMLElement | null;
  readingProgressBar: HTMLElement | null;
  backToTop: HTMLElement | null;
  memoEditorEl: HTMLTextAreaElement | null;
  memoPreviewEl: HTMLElement | null;
  memoSaveStatusEl: HTMLElement | null;
  quoteSelectionActionEl: HTMLElement | null;
  documentSearchInputEl: HTMLInputElement | null;
  documentSearchSummaryEl: HTMLElement | null;
  documentSearchResultsEl: HTMLElement | null;
  documentSearchPrevEl: HTMLButtonElement | null;
  documentSearchNextEl: HTMLButtonElement | null;
  documentSearchClearEl: HTMLButtonElement | null;
}

interface MarkdownViewAppContext {
  config: MarkdownViewConfig;
  state: MarkdownViewState;
  elements: MarkdownViewElements;
  fetch: MarkdownViewFetchState;
  sidebar: MarkdownViewSidebarState;
  memo: MarkdownViewMemoState;
  search: MarkdownViewSearchState;
  labels: MarkdownViewLabels;
  test: MarkdownViewTestState;
  websocket: MarkdownViewWebSocketController | null;
  content: MarkdownViewContentController | null;
}

interface MarkdownViewTestHooks {
  [key: string]: unknown;
}

interface Window {
  __MV_E2E__?: boolean;
  markdownViewTestHooks?: MarkdownViewTestHooks;
}

declare const __MAX_FILE_SIZE_MB__: number;
