export type TestWebSocketHarnessOptions = {
  setE2EFlag?: boolean;
  shorten30sTimeouts?: boolean;
};

export function installTestWebSocketHarness(options: TestWebSocketHarnessOptions = {}) {
  if (options.setE2EFlag === true) {
    window.__MV_E2E__ = true;
  }

  const NativeWebSocket = window.WebSocket;
  const nativeSetTimeout = window.setTimeout.bind(window);

  class TestWebSocket extends NativeWebSocket {
    constructor(...args: ConstructorParameters<typeof WebSocket>) {
      super(...args);
      window.__lastWs = this as MvE2E.TestWebSocketInstance;
    }
  }

  window.WebSocket = TestWebSocket;

  // text_selection_defer の長時間待機を避けるため、30秒タイマーをまとめて短縮する。
  // WebSocket reconnect だけでなく、選択中更新の30秒フォールバックも対象になる。
  if (options.shorten30sTimeouts === true) {
    window.setTimeout = ((fn: TimerHandler, delay?: number, ...args: unknown[]) => {
      const effectiveDelay = delay === 30000 ? 50 : delay;
      return nativeSetTimeout(fn, effectiveDelay, ...args);
    }) as typeof window.setTimeout;
  }
}
