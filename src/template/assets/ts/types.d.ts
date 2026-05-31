type MemoState = 'ready' | 'degraded';
type LiveStatusState = 'connected' | 'disconnected' | 'error' | 'live' | 'retry' | 'offline';

type SearchTruncationReason = 'result_limit' | 'file_limit' | 'byte_limit';
type MemoSaveState = 'dirty' | 'error' | 'loading' | 'saved' | 'saving' | string;
type MemoRemoteUpdateBlockReason = 'none' | 'dirty' | 'focus' | 'loading' | 'saving' | string;
type SearchContextVariant = 'before' | 'current' | 'after';

interface MarkdownViewHttpError extends Error {
  type?: 'http' | 'parse' | 'contract';
  status?: number;
  userMessage?: string;
}

interface ApiErrorPayload {
  error: string;
}

interface LineRange {
  start: number;
  end: number;
}

interface ParsedLineHash {
  headingId: string | null;
  lineRange: LineRange | null;
}

interface MarkdownLinkTarget {
  file: string;
  hash: string;
}

interface SentenceRange {
  start: number;
  end: number;
}

interface DocumentSearchTextNodeEntry {
  node: Text;
  start: number;
  end: number;
}

interface DocumentSearchTextBlock {
  text: string;
  nodes: DocumentSearchTextNodeEntry[];
}

interface DocumentSearchBlockEntry extends DocumentSearchTextBlock {
  sentences: SentenceRange[];
}

interface DocumentSearchContext {
  before: string;
  current: string;
  after: string;
}

interface DocumentSearchMatch {
  marks: HTMLElement[];
  context: DocumentSearchContext;
}

interface WrappedDocumentSearchSegment {
  mark: HTMLElement;
  tail: Text | null;
}

interface DirectorySearchSelection {
  file: string;
  fileMatchIndex: number | undefined;
}

interface PendingDirectoryNavigation {
  file: string;
  query: string;
  fileMatchIndex: number;
  resultIndex: number;
  previousResultIndex: number;
}

interface TocTracking {
  headings: HTMLElement[];
  links: Map<string, HTMLAnchorElement>;
  activationOffset: number;
}

interface MemoSelectionSnapshot {
  start: number;
  end: number;
  isFocused: boolean;
}

interface QuoteSource {
  label: string;
  href: string;
  lines: string;
}

interface MemoApplyOptions {
  updateEditor?: boolean;
  preserveSelection?: boolean;
  preserveDegradedEditor?: boolean;
}

interface SelectFileOptions extends ContentUpdateOptions {}

interface FilterableListOptions<TItem extends HTMLElement> {
  inputId: string;
  rootId: string;
  getItems(root: HTMLElement): Iterable<TItem> | ArrayLike<TItem>;
  apply(items: TItem[], query: string, input: HTMLInputElement): void;
}

interface SearchResult {
  file: string;
  file_match_index: number;
  before: string;
  current: string;
  after: string;
}

interface SearchResponse {
  query: string;
  results: SearchResult[];
  searched_files: number;
  skipped_files: number;
  searched_bytes: number;
  truncated: boolean;
  truncated_reasons: SearchTruncationReason[];
  limits: {
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
  scrollMode?: 'preserve' | 'reset' | 'none';
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
  documentMatches: DocumentSearchMatch[];
  currentDocumentIndex: number;
  currentDocumentQuery: string;
  currentDirectoryResults: SearchResult[];
  currentDirectoryIndex: number;
  currentDirectorySkippedFiles: number;
  currentDirectoryTruncated: boolean;
  currentDirectoryTruncatedReasons: SearchTruncationReason[];
  currentDirectoryLoading: boolean;
  currentDirectoryError: string;
  currentDirectoryResultsScrollTop: number;
  documentDebounceTimer: number | null;
  documentFetchGeneration: number;
  directorySearchClientId: string;
  directorySearchSequence: number;
  pendingDirectoryNavigation: PendingDirectoryNavigation | null;
}

interface MarkdownViewSidebarState {
  currentTocTracking: TocTracking | null;
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
  markPendingTocNavigationObserver: ((id: string) => void) | null;
}

interface MarkdownViewContentController {
  setup(): void;
  updateContent(data: unknown, options?: ContentUpdateOptions): ContentUpdateResult;
  applyPendingUpdate(): void;
  restoreNavigationFromLocation(): void;
  openDocumentSearch(): void;
  moveDocumentSearch(step: number): void;
  applyDocumentSearchQuery(query: string): void;
  clearDocumentSearchQuery(): void;
  renderDirectorySearchUi(options?: DirectorySearchRenderOptions): void;
  scheduleDirectorySearch(query: string): void;
  augmentHashWithTrailingLineHint(link: Element, hash: string): string;
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

interface MarkdownViewContentEnhancementsController {
  setLiveStatus(state: LiveStatusState): void;
  updateDocumentStats(): void;
  updateReadingProgress(): void;
  syncDocumentChrome(file: string): void;
  enhanceContentInteractions(): void;
  setupTocFilter(): void;
}

interface MarkdownViewContentNavigationController {
  augmentHashWithTrailingLineHint(link: Element, hash: string): string;
  applyContentAnchorNavigation(hash: string, replace: boolean): boolean;
  restoreContentNavigationFromLocation(): void;
  setupContentLinkNavigation(): void;
  setupMemoLinkNavigation(): void;
  setLocationHash(hash: string, replace: boolean): void;
}

interface MarkdownViewDocumentSearchController {
  applyDocumentSearchHighlights(query: string): void;
  applyDocumentSearchQuery(query: string): void;
  clearDocumentSearchHighlights(): void;
  clearDocumentSearchQuery(): void;
  createDocumentSearchEmptyState(message: string): HTMLElement;
  moveDocumentSearch(step: number): void;
  openDocumentSearch(): void;
  renderDocumentSearchResultContext(
    container: HTMLElement,
    text: string,
    query: string,
    variant: SearchContextVariant
  ): void;
  renderDocumentSearchResults(): void;
  setCurrentDocumentSearchMatch(index: number, scrollIntoView?: boolean): void;
  setupDocumentSearch(): void;
  syncDocumentSearchAfterContentUpdate(options?: ContentUpdateOptions): void;
  updateDocumentSearchSummary(): void;
}

interface DirectorySearchRenderOptions {
  preserveScroll?: boolean;
}

interface MarkdownViewDirectorySearchController {
  applyPendingDirectorySearchNavigation(): void;
  cancelDirectorySearch(): void;
  openDirectorySearchResult(index: number): void;
  renderDirectorySearchResults(options?: DirectorySearchRenderOptions): void;
  renderDirectorySearchUi(options?: DirectorySearchRenderOptions): void;
  scheduleDirectorySearch(query: string): void;
}

interface ContentEnhancementsDeps {
  clearMemoSyncPendingStatus(): void;
}

interface ContentNavigationDeps {
  selectFile(file: string, pushHistory?: boolean, options?: SelectFileOptions): void;
  setFileParam(file: string, replace: boolean, hash?: string): void;
  markPendingTocNavigation(id: string): void;
  clearPendingTocNavigation(): void;
  restoreActiveTocHeading(preferredId: string): void;
}

interface DocumentSearchDeps {
  activateSidebarTab(target: string): void;
  applyPendingDirectorySearchNavigation(): void;
  openDirectorySearchResult(index: number): void;
  renderDirectorySearchResults(options?: DirectorySearchRenderOptions): void;
  renderDirectorySearchUi(options?: DirectorySearchRenderOptions): void;
  cancelDirectorySearch(): void;
  scheduleDirectorySearch(query: string): void;
}

interface DirectorySearchDeps {
  createHttpError(status: number): MarkdownViewHttpError;
  createDocumentSearchEmptyState(message: string): HTMLElement;
  getFileFetchErrorMessage(err: unknown): string;
  openFileSearchResult(file: string, options?: SelectFileOptions): void;
  renderDocumentSearchResultContext(
    container: HTMLElement,
    text: string,
    query: string,
    variant: SearchContextVariant
  ): void;
  setCurrentDocumentSearchMatch(index: number, scrollIntoView?: boolean): void;
  updateDocumentSearchSummary(): void;
}

interface ContentControllerDeps extends ContentEnhancementsDeps, ContentNavigationDeps {
  activateSidebarTab(target: string): void;
  createHttpError(status: number): MarkdownViewHttpError;
  getCurrentActiveTocId(): string;
  getFileFetchErrorMessage(err: unknown): string;
  hideFileFetchErrorBanner(): void;
  hideQuoteSelectionAction(): void;
  hideWsServerErrorBanner(): void;
  showWsServerErrorBanner(message: string): void;
  setupTocTracking(): void;
  suppressTocTrackingFor(ms: number): void;
}

interface WebSocketDeps {
  updateContent(data: unknown, options?: ContentUpdateOptions): ContentUpdateResult;
  scheduleDirectorySearch(query: string): void;
  setLiveStatus(state: LiveStatusState): void;
  selectFile(file: string, pushHistory?: boolean, options?: SelectFileOptions): void;
  applyRemoteMemoUpdate(data: ContentUpdatePayload): boolean;
  queueRemoteMemoReload(data: ContentUpdatePayload): boolean;
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
  activateSidebarTab(target: string): void;
  applyDocumentSearchQuery(query: string): void;
  augmentHashWithTrailingLineHint(link: Element, hash: string): string;
  markPendingTocNavigation(id: string): void;
  setMarkPendingTocNavigationObserverForTest(callback: ((id: string) => void) | null): void;
  moveDocumentSearch(direction: number): void;
  scheduleBufferedLiveUpdate(data: ContentUpdatePayload): void;
  selectFile(file: string, pushHistory?: boolean, options?: SelectFileOptions): void;
  setCurrentFileForTest(file: string): void;
  setDirModeForTest(value: boolean): void;
  updateContent(data: unknown, options?: ContentUpdateOptions): ContentUpdateResult;
  readonly isDirMode: boolean;
  readonly currentFile: string;
  readonly lastAppliedContent: string | null;
}

interface Window {
  __MV_E2E__?: boolean;
  markdownViewTestHooks?: MarkdownViewTestHooks;
}

declare const __MAX_FILE_SIZE_MB__: number;
