export type TestWebSocketHarnessOptions = {
  setE2EFlag?: boolean;
  shortenReconnectDelay?: boolean;
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

  TestWebSocket.prototype = NativeWebSocket.prototype;
  Object.setPrototypeOf(TestWebSocket, NativeWebSocket);
  window.WebSocket = TestWebSocket;

  if (options.shortenReconnectDelay === true) {
    window.setTimeout = ((fn: TimerHandler, delay?: number, ...args: unknown[]) => {
      const effectiveDelay = delay === 30000 ? 50 : delay;
      return nativeSetTimeout(fn, effectiveDelay, ...args);
    }) as typeof window.setTimeout;
  }
}
