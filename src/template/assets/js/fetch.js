function getFileParam() {
  var params = new URLSearchParams(location.search);
  return params.get('file') || '';
}

function setFileParam(file, replace) {
  var url = new URL(location.href);
  if (file) {
    url.searchParams.set('file', file);
  } else {
    url.searchParams.delete('file');
  }
  if (replace) {
    history.replaceState(null, '', url.toString());
  } else {
    history.pushState(null, '', url.toString());
  }
}

if (isDirMode) {
  if (currentFile) {
    setFileParam(currentFile, true);
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
        return 'ファイルサイズが上限（' + MAX_FILE_SIZE_MB + 'MB）を超えています。';
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

var fetchGeneration = 0;
function selectFile(file, pushHistory, options) {
  if (pushHistory === undefined) pushHistory = true;
  options = options || {};
  var previousFile = currentFile;
  var gen = ++fetchGeneration;
  if (typeof discardBufferedLiveUpdate === 'function') {
    discardBufferedLiveUpdate();
  }
  pendingUpdate = null;
  if (pendingUpdateTimer) {
    clearTimeout(pendingUpdateTimer);
    pendingUpdateTimer = null;
  }
  if (typeof flushPendingMemoSave === 'function') {
    flushPendingMemoSave();
  }
  currentFile = file;
  if (pushHistory) setFileParam(file);
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
    if (gen !== fetchGeneration) return;
    if (!isDirMode && previousFile && previousFile !== file && typeof clearDocumentSearchQuery === 'function') {
      clearDocumentSearchQuery();
    }
    var scrollMode = options.scrollMode || (previousFile === file ? 'preserve' : 'reset');
    updateContent(data, {
      scrollMode: scrollMode,
      requeryDirectorySearch: options.requeryDirectorySearch !== false
    });
    if (isDirMode && !pushHistory) {
      setFileParam(currentFile, true);
    }
    if (data.file && data.file !== currentFile) {
      currentFile = data.file;
      setFileParam(currentFile, true);
      updateFileListActive(currentFile);
    }
    syncDocumentChrome(currentFile);
    if (typeof loadMemo === 'function') {
      loadMemo(currentFile, gen);
    }
    setLiveStatus('live');
  })
  .catch(function(err) {
    console.error('[markdown-view] ファイル取得エラー:', err);
    if (gen !== fetchGeneration) return;
    if (
      isDirMode &&
      pendingDirectorySearchNavigation &&
      pendingDirectorySearchNavigation.file === file
    ) {
      currentDirectorySearchIndex = pendingDirectorySearchNavigation.previousResultIndex;
      pendingDirectorySearchNavigation = null;
      if (typeof renderDirectorySearchUi === 'function') {
        renderDirectorySearchUi();
      }
    }
    currentFile = previousFile;
    updateFileListActive(previousFile);
    setFileParam(previousFile, !pushHistory);
    if (typeof loadMemo === 'function') {
      loadMemo(previousFile, gen);
    }
    showFileFetchErrorBanner(getFileFetchErrorMessage(err));
    setLiveStatus('error');
  });
}
