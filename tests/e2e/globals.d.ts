export {};

declare global {
  namespace MvE2E {
    type UpdateContentPayload = {
      content: string;
      toc: string;
    };
    type UpdateMessage = Partial<UpdateContentPayload> & {
      file?: string;
      refresh?: boolean;
      memo_refresh?: boolean;
      memo_file?: string;
      type?: string;
      error?: string;
      raw?: string;
      html?: string;
      load_error?: string;
      memo_state?: 'ready' | 'degraded';
    };
    type UpdateContentOptions = {
      scrollMode?: 'preserve' | 'reset' | 'none';
      anchorHash?: string;
      historyHash?: string;
      clearHashOnMiss?: boolean;
      requeryDirectorySearch?: boolean;
    };
    type UpdateContentResult = {
      ok: boolean;
      contractViolation: boolean;
      missing: string[];
    };
    type ClickObservation = {
      defaultPrevented: boolean;
    };
    type TestWebSocketInstance = WebSocket & {
      onmessage: ((ev: MessageEvent) => void) | null;
    };
  }

  interface Window {
    __MV_E2E__?: boolean;
    __lastWs?: MvE2E.TestWebSocketInstance;
    __bridgedWs?: MvE2E.TestWebSocketInstance;
    __parseErrorWs?: MvE2E.TestWebSocketInstance;
    __serverErrorWs?: MvE2E.TestWebSocketInstance;
    __errorWs?: MvE2E.TestWebSocketInstance;
    __realWsOnmessage?: (ev: { data: string }) => void;
    __dispatchWsMessage?: (payload: unknown) => void;
    __markPendingCalls?: number;
    __wsCloseCalls?: number;
    __tocActiveChanges?: string[];
    __stopTocObserver?: () => void;
    __clickObservations?: Record<string, MvE2E.ClickObservation>;
    markdownViewTestHooks: {
      activateSidebarTab(target: string): void;
      applyDocumentSearchQuery(query: string): void;
      augmentHashWithTrailingLineHint(link: HTMLAnchorElement, hash: string): string;
      markPendingTocNavigation(id: string): void;
      moveDocumentSearch(direction: number): void;
      scheduleBufferedLiveUpdate(data: MvE2E.UpdateMessage): void;
      selectFile(file: string, pushHistory?: boolean, options?: MvE2E.UpdateContentOptions): void;
      setCurrentFileForTest(file: string): void;
      setDirModeForTest(value: boolean): void;
      setMarkPendingTocNavigationObserverForTest(callback: ((id: string) => void) | null): void;
      updateContent(data: MvE2E.UpdateMessage, opts?: MvE2E.UpdateContentOptions): MvE2E.UpdateContentResult;
      readonly isDirMode: boolean;
      readonly currentFile: string;
      readonly lastAppliedContent: string | null;
    };
  }
}
