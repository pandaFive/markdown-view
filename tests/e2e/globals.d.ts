export {};

declare global {
  namespace MvE2E {
    type UpdateContentPayload = {
      content: string;
      toc: string;
    };
    type UpdateContentOptions = {
      scrollMode?: 'preserve' | 'reset' | 'none';
      anchorHash?: string;
      historyHash?: string;
      clearHashOnMiss?: boolean;
      requeryDirectorySearch?: boolean;
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
    __realWsOnmessage?: (ev: { data: string }) => void;
    __dispatchWsMessage?: (payload: unknown) => void;
    __markPendingCalls?: number;
    __tocActiveChanges?: string[];
    __stopTocObserver?: () => void;
    __clickObservations?: Record<string, MvE2E.ClickObservation>;
    markPendingTocNavigation?: (id: string) => void;
    // E2E フラグ有効時だけ expose されるテスト hook であり production API ではない。
    updateContent?: (data: MvE2E.UpdateContentPayload, opts?: MvE2E.UpdateContentOptions) => void;
    // E2E から参照している browser bundle 内部 hook であり production API ではない。
    scheduleBufferedLiveUpdate: (data: MvE2E.UpdateContentPayload) => void;
  }

  var isDirMode: boolean;
  var currentFile: string;
  function activateSidebarTab(tab: string): void;
  function applyDocumentSearchQuery(value: string): void;
  function moveDocumentSearch(direction: number): void;
  function selectFile(file: string, pushHistory?: boolean, options?: MvE2E.UpdateContentOptions): void;
  function augmentHashWithTrailingLineHint(link: HTMLAnchorElement, hash: string): string;
}
