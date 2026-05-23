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
  currentFile: string | null;
  isDirectoryMode: boolean;
  maxFileSizeMb: number;
  config: Record<string, unknown>;
  state: Record<string, unknown>;
  elements: MarkdownViewElements;
  fetch: Record<string, unknown>;
  sidebar: Record<string, unknown>;
  memo: Record<string, unknown>;
  search: Record<string, unknown>;
  labels: Record<string, unknown>;
  test: Record<string, unknown>;
  websocket: Record<string, unknown> | null;
  content: Record<string, unknown> | null;
}

interface MarkdownViewTestHooks {
  [key: string]: unknown;
}

interface Window {
  __MV_E2E__?: boolean;
  markdownViewTestHooks?: MarkdownViewTestHooks;
}

declare const __MAX_FILE_SIZE_MB__: number;
