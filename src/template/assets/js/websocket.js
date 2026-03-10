var WS_RECONNECT_BASE = 1000;
var WS_RECONNECT_MAX_DELAY = 30000;
var WS_RECONNECT_MAX_ATTEMPTS = 20;
var ws = null;
var reconnectAttempts = 0;

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
    if (data.refresh && isDirMode && currentFile) {
      if (isTextSelected()) {
        pendingUpdate = { refresh: true, file: currentFile };
        ensurePendingUpdateTimer();
        return;
      }
      selectFile(currentFile, false);
      return;
    }
    if (isDirMode && data.file) {
      if (data.file !== currentFile) return;
    }
    // テキスト選択中はDOM更新を延期して選択破壊を防止
    // 複数回受信した場合は最新の更新のみ保持（最新状態が常に正しいため）
    if (isTextSelected()) {
      pendingUpdate = data;
      // 有効な更新を受信した時点でエラーバナーをクリア（DOM反映は延期）
      hideWsServerErrorBanner();
      hideFileFetchErrorBanner();
      // 30秒以上選択が維持される場合のフォールバックタイマー
      // mouseup後にWS受信した場合にもタイマーが確実に起動する
      ensurePendingUpdateTimer();
      return;
    }
    updateContent(data);
    // WebSocket経由の成功更新で各種エラーバナーをクリア
    hideWsServerErrorBanner();
    hideFileFetchErrorBanner();
    setLiveStatus('live');
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
