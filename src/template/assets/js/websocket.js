var WS_RECONNECT_BASE = 1000;
var WS_RECONNECT_MAX_DELAY = 30000;
var WS_RECONNECT_MAX_ATTEMPTS = 20;
var WS_UPDATE_COALESCE_MS = 120;
var ws = null;
var reconnectAttempts = 0;
var pendingWsUpdate = null;
var pendingWsUpdateSignature = '';
var pendingWsUpdateTimer = null;
var lastAppliedUpdateSignature = '';

function discardBufferedLiveUpdate() {
  if (pendingWsUpdateTimer) {
    window.clearTimeout(pendingWsUpdateTimer);
    pendingWsUpdateTimer = null;
  }
  pendingWsUpdate = null;
  pendingWsUpdateSignature = '';
}

function buildUpdateSignature(data) {
  return JSON.stringify({
    content: data.content !== undefined ? data.content : null,
    toc: data.toc !== undefined ? data.toc : null,
    file: data.file || '',
    refresh: Boolean(data.refresh)
  });
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
    if (!pendingUpdate || !pendingUpdate.refresh) {
      pendingUpdate = data;
    }
    hideWsServerErrorBanner();
    hideFileFetchErrorBanner();
    ensurePendingUpdateTimer();
    return;
  }

  updateContent(data);
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

function connectWS() {
  var protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  ws = new WebSocket(protocol + '//' + location.host + '/ws');

  ws.onopen = function() {
    reconnectAttempts = 0;
    setLiveStatus('live');
  };

  ws.onmessage = function(event) {
    var data;
    try {
      data = JSON.parse(event.data);
    } catch (e) {
      console.error('[markdown-view] JSONパースエラー:', e);
      showWsParseErrorBanner('サーバーから不正なJSONを受信しました。ページを再読み込みしてください。');
      setLiveStatus('error');
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
      if (applyRemoteMemoUpdate(data)) {
        hideWsServerErrorBanner();
        hideFileFetchErrorBanner();
      }
      setLiveStatus('live');
      return;
    }
    if (data.refresh && isDirMode && currentFile) {
      if (isTextSelected()) {
        discardBufferedLiveUpdate();
        pendingUpdate = { refresh: true, file: currentFile };
        ensurePendingUpdateTimer();
        return;
      }
      selectFile(currentFile, false);
      return;
    }
    if (isDirMode && data.file) {
      if (data.file !== currentFile) {
        if (currentDocumentSearchQuery && typeof scheduleDirectorySearch === 'function') {
          scheduleDirectorySearch(currentDocumentSearchQuery);
        }
        return;
      }
    }
    scheduleBufferedLiveUpdate(data);
  };

  ws.onclose = function() {
    setLiveStatus('retry');
    scheduleReconnect();
  };

  ws.onerror = function(event) {
    console.error('[markdown-view] WebSocketエラー:', event);
    setLiveStatus('error');
    ws.close();
  };
}

function scheduleReconnect() {
  if (reconnectAttempts >= WS_RECONNECT_MAX_ATTEMPTS) {
    console.error('[markdown-view] 再接続上限に達しました。ページをリロードしてください');
    showDisconnectBanner();
    return;
  }
  var delay = Math.min(WS_RECONNECT_BASE * Math.pow(2, reconnectAttempts), WS_RECONNECT_MAX_DELAY);
  reconnectAttempts++;
  setTimeout(connectWS, delay);
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
