var WS_RECONNECT_BASE = 1000;
var WS_RECONNECT_MAX_DELAY = 30000;
var WS_RECONNECT_MAX_ATTEMPTS = 20;
var WS_UPDATE_COALESCE_MS = 120;
function buildUpdateSignature(data) {
  return JSON.stringify({
    content: data.content !== undefined ? data.content : null,
    toc: data.toc !== undefined ? data.toc : null,
    file: data.file || '',
    refresh: Boolean(data.refresh)
  });
}

function summarizeBufferedUpdateForLog(data) {
  return {
    file: typeof data.file === 'string' ? data.file : '',
    refresh: Boolean(data.refresh),
    contentLength: typeof data.content === 'string' ? data.content.length : null,
    tocLength: typeof data.toc === 'string' ? data.toc.length : null
  };
}

function isObjectWebSocketMessage(data) {
  return data && typeof data === 'object' && !Array.isArray(data);
}

function describeInvalidWebSocketPayload(data) {
  if (data === null) return 'null';
  if (Array.isArray(data)) return 'array';
  return typeof data;
}

// WebSocket の再接続・buffer 状態は closure に閉じ、他機能は controller API 経由で操作する。
function createWebSocketController(ctx, deps) {
  var socket = null;
  var socketReconnectAttempts = 0;
  var pendingWsUpdate = null;
  var pendingWsUpdateSignature = '';
  var pendingWsUpdateTimer = null;
  var lastAppliedUpdateSignature = '';
  var suppressNextReconnect = false;

  function discardBufferedLiveUpdate(reason) {
    if (pendingWsUpdateTimer) {
      window.clearTimeout(pendingWsUpdateTimer);
      pendingWsUpdateTimer = null;
    }
    if (pendingWsUpdate) {
      console.warn('[markdown-view] buffer済み更新を破棄しました。', {
        reason: reason || 'unspecified',
        update: summarizeBufferedUpdateForLog(pendingWsUpdate)
      });
    }
    pendingWsUpdate = null;
    pendingWsUpdateSignature = '';
  }

  function rememberAppliedLiveUpdate(data) {
    lastAppliedUpdateSignature = buildUpdateSignature(data);
  }

  function flushBufferedLiveUpdate() {
    pendingWsUpdateTimer = null;
    var data = pendingWsUpdate;
    pendingWsUpdate = null;
    pendingWsUpdateSignature = '';
    if (!data) return;

    // テキスト選択中はDOM更新を延期して選択破壊を防止
    // 複数回受信した場合は最新の更新のみ保持（最新状態が常に正しいため）
    if (isTextSelected()) {
      if (!ctx.state.pendingUpdate || !ctx.state.pendingUpdate.refresh) {
        ctx.state.pendingUpdate = data;
      }
      hideWsServerErrorBanner();
      hideFileFetchErrorBanner();
      ensurePendingUpdateTimer();
      return;
    }

    deps.updateContent(data);
    hideWsServerErrorBanner();
    hideFileFetchErrorBanner();
    setLiveStatus('live');
  }

  function scheduleBufferedLiveUpdate(data) {
    var signature = buildUpdateSignature(data);
    if (signature === lastAppliedUpdateSignature || signature === pendingWsUpdateSignature) {
      return;
    }

    pendingWsUpdate = data;
    pendingWsUpdateSignature = signature;

    if (pendingWsUpdateTimer) return;
    pendingWsUpdateTimer = window.setTimeout(function() {
      flushBufferedLiveUpdate();
    }, WS_UPDATE_COALESCE_MS);
  }

  function connect() {
    var protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    socket = new WebSocket(protocol + '//' + location.host + '/ws');

    socket.onopen = function() {
      socketReconnectAttempts = 0;
      // 接続成功は WebSocket 経路の一時エラーからの復旧点なので、WS バナーだけを解除する。
      hideWsParseErrorBanner();
      hideWsServerErrorBanner();
      setLiveStatus('live');
    };

    socket.onmessage = function(event) {
      var data;
      try {
        data = JSON.parse(event.data);
      } catch (e) {
        console.error('[markdown-view] JSONパースエラー:', e);
        showWsParseErrorBanner('サーバーから不正なJSONを受信しました。ページを再読み込みしてください。');
        setLiveStatus('error');
        suppressNextReconnect = true;
        socket.close();
        return;
      }
      if (!isObjectWebSocketMessage(data)) {
        console.error('[markdown-view] WebSocketメッセージ形式エラー:', {
          payloadType: describeInvalidWebSocketPayload(data)
        });
        showWsParseErrorBanner('サーバーから形式が不正なWebSocketメッセージを受信しました。ページを再読み込みしてください。');
        setLiveStatus('error');
        suppressNextReconnect = true;
        socket.close();
        return;
      }
      hideWsParseErrorBanner();
      if (data.error) {
        console.error('[markdown-view] サーバーエラー:', data.error);
        showWsServerErrorBanner(data.error);
        setLiveStatus('error');
        return;
      }
      if (isMemoUpdateMessage(data)) {
        if (deps.applyRemoteMemoUpdate(data)) {
          hideWsServerErrorBanner();
          hideFileFetchErrorBanner();
        }
        setLiveStatus('live');
        return;
      }
      if (isMemoRefreshMessage(data) && !(data.refresh && ctx.config.isDirMode)) {
        if (deps.queueRemoteMemoReload(data)) {
          hideWsServerErrorBanner();
          hideFileFetchErrorBanner();
        }
      }
      if (data.refresh && ctx.config.isDirMode) {
        if (!ctx.state.currentFile) {
          console.warn('[markdown-view] 現在ファイルが未設定のため refresh 通知を無視しました。', {
            messageFile: data.file || ''
          });
          return;
        }
        if (!data.file) {
          console.warn('[markdown-view] file を含まない refresh 通知を現在ファイルへ適用します。', {
            currentFile: ctx.state.currentFile
          });
        }
        if (data.file && data.file !== ctx.state.currentFile) {
          // ディレクトリモードでは他ファイルの変更通知も同じWSへ届くため、現在表示中でない refresh は無視する。
          console.warn('[markdown-view] 現在のファイルと異なる refresh 通知を無視しました。', {
            currentFile: ctx.state.currentFile,
            messageFile: data.file
          });
          return;
        }
        if (isTextSelected()) {
          discardBufferedLiveUpdate('refresh通知を優先');
          ctx.state.pendingUpdate = { refresh: true, file: ctx.state.currentFile };
          ensurePendingUpdateTimer();
          return;
        }
        deps.selectFile(ctx.state.currentFile, false);
        return;
      }
      if (ctx.config.isDirMode && data.file) {
        if (data.file !== ctx.state.currentFile) {
          if (ctx.search.currentDocumentQuery) {
            scheduleDirectorySearch(ctx.search.currentDocumentQuery);
          }
          return;
        }
      }
      scheduleBufferedLiveUpdate(data);
    };

    socket.onclose = function() {
      if (suppressNextReconnect) {
        suppressNextReconnect = false;
        return;
      }
      if (!document.getElementById('ws-parse-error-banner') && !document.getElementById('ws-server-error-banner')) {
        setLiveStatus('retry');
      }
      scheduleReconnect();
    };

    socket.onerror = function(event) {
      console.error('[markdown-view] WebSocketエラー:', event);
      setLiveStatus('error');
      socket.close();
    };
  }

  function scheduleReconnect() {
    if (socketReconnectAttempts >= WS_RECONNECT_MAX_ATTEMPTS) {
      console.error('[markdown-view] 再接続上限に達しました。ページをリロードしてください');
      showDisconnectBanner();
      return;
    }
    var delay = Math.min(WS_RECONNECT_BASE * Math.pow(2, socketReconnectAttempts), WS_RECONNECT_MAX_DELAY);
    socketReconnectAttempts++;
    setTimeout(connect, delay);
  }

  return {
    connect: connect,
    discardBufferedLiveUpdate: discardBufferedLiveUpdate,
    rememberAppliedLiveUpdate: rememberAppliedLiveUpdate,
    scheduleBufferedLiveUpdate: scheduleBufferedLiveUpdate
  };
}

function showDisconnectBanner() {
  if (document.getElementById('ws-disconnect-banner')) return;
  setLiveStatus('offline');
  var banner = document.createElement('div');
  banner.id = 'ws-disconnect-banner';
  banner.className = 'error-banner disconnect';
  banner.textContent = 'ライブリロード接続が切断されました。ページをリロードしてください。';
  document.body.appendChild(banner);
}

function showWsParseErrorBanner(message) {
  if (document.getElementById('ws-disconnect-banner')) return;
  var banner = document.getElementById('ws-parse-error-banner');
  if (!banner) {
    banner = document.createElement('div');
    banner.id = 'ws-parse-error-banner';
    banner.className = 'error-banner server';
    var closeBtn = document.createElement('span');
    closeBtn.textContent = '\u00d7';
    closeBtn.className = 'error-banner-close';
    closeBtn.onclick = hideWsParseErrorBanner;
    banner.appendChild(closeBtn);
    var msg = document.createElement('span');
    msg.className = 'error-msg';
    banner.appendChild(msg);
    document.body.appendChild(banner);
  }
  banner.querySelector('.error-msg').textContent = message;
}

function hideWsParseErrorBanner() {
  var banner = document.getElementById('ws-parse-error-banner');
  if (banner) {
    banner.remove();
  }
}

function showWsServerErrorBanner(message) {
  if (document.getElementById('ws-disconnect-banner')) return;
  var banner = document.getElementById('ws-server-error-banner');
  if (!banner) {
    banner = document.createElement('div');
    banner.id = 'ws-server-error-banner';
    banner.className = 'error-banner server';
    var closeBtn = document.createElement('span');
    closeBtn.textContent = '\u00d7';
    closeBtn.className = 'error-banner-close';
    closeBtn.onclick = hideWsServerErrorBanner;
    banner.appendChild(closeBtn);
    var msg = document.createElement('span');
    msg.className = 'error-msg';
    banner.appendChild(msg);
    document.body.appendChild(banner);
  }
  banner.querySelector('.error-msg').textContent = message;
}

function hideWsServerErrorBanner() {
  var banner = document.getElementById('ws-server-error-banner');
  if (banner) {
    banner.remove();
  }
}

function showFileFetchErrorBanner(message) {
  if (document.getElementById('ws-disconnect-banner')) return;
  var banner = document.getElementById('file-fetch-error-banner');
  if (!banner) {
    banner = document.createElement('div');
    banner.id = 'file-fetch-error-banner';
    banner.className = 'error-banner fetch';
    var closeBtn = document.createElement('span');
    closeBtn.textContent = '\u00d7';
    closeBtn.className = 'error-banner-close';
    closeBtn.onclick = hideFileFetchErrorBanner;
    banner.appendChild(closeBtn);
    var msg = document.createElement('span');
    msg.className = 'error-msg';
    banner.appendChild(msg);
    document.body.appendChild(banner);
  }
  banner.querySelector('.error-msg').textContent = message;
}

function hideFileFetchErrorBanner() {
  var banner = document.getElementById('file-fetch-error-banner');
  if (banner) {
    banner.remove();
  }
}
