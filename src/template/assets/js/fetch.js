function getFileParam() {
  var params = new URLSearchParams(location.search);
  return params.get('file') || '';
}

function setFileParam(file, replace, hash) {
  var url = new URL(location.href);
  if (file) {
    url.searchParams.set('file', file);
  } else {
    url.searchParams.delete('file');
  }
  if (hash !== undefined) {
    if (hash) {
      url.hash = hash.charAt(0) === '#' ? hash : '#' + hash;
    } else {
      url.hash = '';
    }
  }
  if (replace) {
    history.replaceState(null, '', url.toString());
  } else {
    history.pushState(null, '', url.toString());
  }
}

function setupHistoryUrlSync() {
  if (!appContext.config.isDirMode) return;
  if (appContext.state.currentFile) {
    setFileParam(appContext.state.currentFile, true);
  }
}

function createHttpError(status) {
  var err = new Error('HTTP ' + status);
  err.type = 'http';
  err.status = status;
  return err;
}

function getFileFetchErrorMessage(err) {
  if (err && err.type === 'http') {
    switch (err.status) {
      case 403:
        return 'このファイルにはアクセスできません。';
      case 404:
        return '指定したファイルが見つかりません。';
      case 413:
        return 'ファイルサイズが上限（' + appContext.config.maxFileSizeMb + 'MB）を超えています。';
      case 500:
        return 'サーバー内部エラーが発生しました。';
      default:
        return 'ファイルの読み込みに失敗しました（HTTP ' + err.status + '）。';
    }
  }
  if (err && err.type === 'parse') {
    return 'サーバー応答の解析に失敗しました。ページを再読み込みしてください。';
  }
  return 'ネットワークエラーが発生しました。接続を確認して再度お試しください。';
}

function selectFile(file, pushHistory, options) {
  if (pushHistory === undefined) pushHistory = true;
  options = options || {};
  var previousFile = appContext.state.currentFile;
  // historyHash指定時のみ失敗時に元hashへ戻す。未指定経路（サイドバークリック等）では
  // 呼び出し側がhash操作を意図していないため、fetch失敗でcurrent hashを上書きしない。
  var shouldRestoreHash = options.historyHash !== undefined;
  var previousHash = shouldRestoreHash ? location.hash : undefined;
  var gen = ++appContext.fetch.generation;
  if (appContext.websocket) {
    appContext.websocket.discardBufferedLiveUpdate('ファイル切替を優先');
  }
  appContext.state.pendingUpdate = null;
  if (appContext.state.pendingUpdateTimer) {
    clearTimeout(appContext.state.pendingUpdateTimer);
    appContext.state.pendingUpdateTimer = null;
  }
  flushPendingMemoSave();
  prepareMemoFileLoad();
  appContext.state.currentFile = file;
  if (pushHistory) setFileParam(file, false, options.historyHash);
  updateFileListActive(file);

  fetch('/api/content?file=' + encodeURIComponent(file), {
    headers: { 'Accept': 'application/json' }
  })
  .then(function(resp) {
    if (!resp.ok) throw createHttpError(resp.status);
    return resp.json().catch(function(err) {
      err.type = 'parse';
      throw err;
    });
  })
  .then(function(data) {
    hideFileFetchErrorBanner();
    if (gen !== appContext.fetch.generation) return;
    if (!appContext.config.isDirMode && previousFile && previousFile !== file) {
      appContext.content.clearDocumentSearchQuery();
    }
    var scrollMode = options.scrollMode || (previousFile === file ? 'preserve' : 'reset');
    var updateResult = appContext.content.updateContent(data, {
      scrollMode: scrollMode,
      requeryDirectorySearch: options.requeryDirectorySearch !== false,
      anchorHash: options.anchorHash || '',
      clearHashOnMiss: pushHistory
    });
    if (updateResult && updateResult.contractViolation) {
      throw new Error('content response contract violation');
    }
    if (appContext.config.isDirMode && !pushHistory) {
      setFileParam(appContext.state.currentFile, true, options.historyHash);
    }
    if (data.file && data.file !== appContext.state.currentFile) {
      appContext.state.currentFile = data.file;
      setFileParam(appContext.state.currentFile, true, options.historyHash);
      updateFileListActive(appContext.state.currentFile);
    }
    appContext.content.syncDocumentChrome(appContext.state.currentFile);
    loadMemo(appContext.state.currentFile, gen);
    appContext.content.setLiveStatus('live');
  })
  .catch(function(err) {
    console.error('[markdown-view] ファイル取得エラー:', err);
    if (gen !== appContext.fetch.generation) return;
    if (
      appContext.config.isDirMode &&
      appContext.search.pendingDirectoryNavigation &&
      appContext.search.pendingDirectoryNavigation.file === file
    ) {
      appContext.search.currentDirectoryIndex = appContext.search.pendingDirectoryNavigation.previousResultIndex;
      appContext.search.pendingDirectoryNavigation = null;
      appContext.content.renderDirectorySearchUi();
    }
    appContext.state.currentFile = previousFile;
    updateFileListActive(previousFile);
    setFileParam(previousFile, !pushHistory, previousHash);
    // 失敗した遷移の generation で旧ファイルのメモを読み直し、後続遷移があれば loadMemo 側で破棄する。
    loadMemo(previousFile, gen);
    showFileFetchErrorBanner(getFileFetchErrorMessage(err));
    appContext.content.setLiveStatus('error');
  });
}
