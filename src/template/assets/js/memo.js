function getMemoRequestUrl(file) {
  if (appContext.config.isDirMode && file) {
    return '/api/memo?file=' + encodeURIComponent(file);
  }
  return '/api/memo';
}

function getMemoTargetFile(file) {
  if (appContext.config.isDirMode) {
    return file || appContext.state.currentFile || '';
  }
  return '';
}

function setMemoSaveStatus(state, text) {
  if (!appContext.elements.memoSaveStatusEl) return;
  appContext.elements.memoSaveStatusEl.dataset.state = state;
  appContext.elements.memoSaveStatusEl.textContent = text;
}

function setMemoEditorDisabled(disabled) {
  if (!appContext.elements.memoEditorEl) return;
  appContext.elements.memoEditorEl.disabled = !!disabled;
}

function isMemoEditorDisabled() {
  return !!(appContext.elements.memoEditorEl && appContext.elements.memoEditorEl.disabled);
}

function rememberMemoCaret() {
  if (!appContext.elements.memoEditorEl) return;
  appContext.memo.caretStart = typeof appContext.elements.memoEditorEl.selectionStart === 'number'
    ? appContext.elements.memoEditorEl.selectionStart
    : appContext.elements.memoEditorEl.value.length;
  appContext.memo.caretEnd = typeof appContext.elements.memoEditorEl.selectionEnd === 'number'
    ? appContext.elements.memoEditorEl.selectionEnd
    : appContext.memo.caretStart;
}

function snapshotMemoSelection() {
  if (!appContext.elements.memoEditorEl) return null;
  return {
    start: typeof appContext.elements.memoEditorEl.selectionStart === 'number' ? appContext.elements.memoEditorEl.selectionStart : 0,
    end: typeof appContext.elements.memoEditorEl.selectionEnd === 'number' ? appContext.elements.memoEditorEl.selectionEnd : 0,
    isFocused: document.activeElement === appContext.elements.memoEditorEl
  };
}

function restoreMemoSelection(selection) {
  if (!appContext.elements.memoEditorEl || !selection || !selection.isFocused) return;
  var valueLength = appContext.elements.memoEditorEl.value.length;
  var start = Math.min(selection.start, valueLength);
  var end = Math.min(selection.end, valueLength);
  appContext.elements.memoEditorEl.focus();
  appContext.elements.memoEditorEl.setSelectionRange(start, end);
}

function updateMemoEditor(raw, preserveSelection) {
  if (!appContext.elements.memoEditorEl) return;
  var selection = preserveSelection ? snapshotMemoSelection() : null;
  appContext.elements.memoEditorEl.value = raw || '';
  restoreMemoSelection(selection);
  rememberMemoCaret();
}

function updateMemoPreview(data) {
  if (!appContext.elements.memoPreviewEl || !data) return;
  appContext.elements.memoPreviewEl.innerHTML = data.html || '';
}

function applyMemoData(data, options) {
  if (!appContext.elements.memoEditorEl || !appContext.elements.memoPreviewEl || !data) return true;
  var shouldUpdateEditor = !options || options.updateEditor !== false;
  if (data.load_error) {
    // load_error 時は raw を上書きしない。読み込み失敗応答でユーザ編集中の内容を破壊しないため。
    cancelMemoAutosave();
    updateMemoPreview(data);
    setMemoEditorDisabled(true);
    setMemoSaveStatus('error', data.load_error);
    return false;
  }
  if (shouldUpdateEditor) {
    if (typeof data.raw === 'string') {
      updateMemoEditor(data.raw, !!(options && options.preserveSelection));
    } else {
      console.warn('[markdown-view] raw を含まないメモ応答のためエディタ内容を維持しました。', {
        receivedKeys: Object.keys(data)
      });
    }
  }
  updateMemoPreview(data);
  setMemoEditorDisabled(false);
  return true;
}

function isMemoUpdateMessage(data) {
  return !!data
    && data.type === 'memo_update'
    && typeof data.file === 'string'
    && data.file.length > 0;
}

function isMemoRefreshMessage(data) {
  return !!data && data.memo_refresh === true;
}

function getMemoRemoteUpdateBlockReason() {
  if (!appContext.elements.memoEditorEl) return 'none';
  if (appContext.memo.saveTimer) return 'dirty';
  if (appContext.elements.memoSaveStatusEl) {
    var state = appContext.elements.memoSaveStatusEl.dataset.state;
    if (state === 'dirty' || state === 'saving' || state === 'loading') return state;
  }
  if (document.activeElement === appContext.elements.memoEditorEl) return 'focus';
  return 'none';
}

function isMemoRemoteUpdateBlocked() {
  return getMemoRemoteUpdateBlockReason() !== 'none';
}

function flushPendingMemoReloadIfSafe() {
  if (appContext.memo.pendingReload === null) return false;
  if (appContext.config.isDirMode && appContext.memo.pendingReload !== appContext.state.currentFile) {
    appContext.memo.pendingReload = null;
    return false;
  }
  if (getMemoRemoteUpdateBlockReason() !== 'none') {
    return false;
  }

  var file = appContext.memo.pendingReload;
  appContext.memo.pendingReload = null;
  loadMemo(file, appContext.fetch.generation);
  return true;
}

function applyRemoteMemoUpdate(data) {
  if (!isMemoUpdateMessage(data)) return false;
  if (appContext.config.isDirMode && data.file !== appContext.state.currentFile) return false;

  appContext.memo.pendingReload = data.file;
  return flushPendingMemoReloadIfSafe();
}

function queueRemoteMemoReload(data) {
  if (!appContext.elements.memoEditorEl || !isMemoRefreshMessage(data)) return false;
  var file = data.memo_file || data.file || getMemoTargetFile();
  if (appContext.config.isDirMode && file !== appContext.state.currentFile) return false;
  appContext.memo.pendingReload = file;
  return flushPendingMemoReloadIfSafe();
}

function parseJsonResponse(resp) {
  if (!resp.ok) throw createHttpError(resp.status);
  return resp.json().catch(function(err) {
    err.type = 'parse';
    throw err;
  });
}

function getMemoErrorMessage(err) {
  if (err && err.type === 'http') {
    switch (err.status) {
      case 403:
        return 'このメモにはアクセスできません。';
      case 404:
        return 'メモ対象のファイルが見つかりません。';
      case 413:
        return 'メモサイズが上限（' + appContext.config.maxFileSizeMb + 'MB）を超えています。';
      case 422:
        return 'メモはUTF-8テキストで保存してください。';
      case 500:
        return 'メモの保存または取得に失敗しました。';
      default:
        return 'メモ操作に失敗しました（HTTP ' + err.status + '）。';
    }
  }
  if (err && err.type === 'parse') {
    return 'メモAPI応答の解析に失敗しました。';
  }
  return 'メモ通信に失敗しました。';
}

function loadMemo(file, ownerGeneration) {
  if (!appContext.elements.memoEditorEl) return Promise.resolve();
  var requestGeneration = ++appContext.memo.loadGeneration;
  setMemoSaveStatus('loading', '読込中');
  return fetch(getMemoRequestUrl(file), {
    headers: { 'Accept': 'application/json' }
  })
  .then(parseJsonResponse)
  .then(function(data) {
    if (ownerGeneration !== undefined && ownerGeneration !== appContext.fetch.generation) {
      console.warn('[markdown-view] 古いメモ読込レスポンスを破棄しました。', {
        ownerGeneration: ownerGeneration,
        currentGeneration: appContext.fetch.generation
      });
      clearStaleMemoLoadingStatus(requestGeneration);
      return;
    }
    if (requestGeneration !== appContext.memo.loadGeneration) {
      console.warn('[markdown-view] 後続のメモ読込があるため古いレスポンスを破棄しました。', {
        requestGeneration: requestGeneration,
        currentGeneration: appContext.memo.loadGeneration
      });
      return;
    }
    if (applyMemoData(data) !== false) {
      setMemoSaveStatus('saved', '保存済み');
    }
    flushPendingMemoReloadIfSafe();
  })
  .catch(function(err) {
    console.error('[markdown-view] メモ取得エラー:', err);
    if (ownerGeneration !== undefined && ownerGeneration !== appContext.fetch.generation) {
      console.warn('[markdown-view] 古いメモ読込エラーを破棄しました。', {
        ownerGeneration: ownerGeneration,
        currentGeneration: appContext.fetch.generation
      });
      clearStaleMemoLoadingStatus(requestGeneration);
      return;
    }
    if (requestGeneration !== appContext.memo.loadGeneration) {
      console.warn('[markdown-view] 後続のメモ読込があるため古いエラーを破棄しました。', {
        requestGeneration: requestGeneration,
        currentGeneration: appContext.memo.loadGeneration
      });
      return;
    }
    setMemoSaveStatus('error', getMemoErrorMessage(err));
    flushPendingMemoReloadIfSafe();
  });
}

function clearStaleMemoLoadingStatus(requestGeneration) {
  // ファイル遷移などで現在の文書世代だけが変わった場合、loadGeneration はまだこのリクエストを指す。
  // その状態で古い読込を破棄すると後続の読込完了が来ないため、表示だけを安全な既定状態へ戻す。
  if (requestGeneration !== appContext.memo.loadGeneration) return;
  if (!appContext.elements.memoSaveStatusEl || appContext.elements.memoSaveStatusEl.dataset.state !== 'loading') return;
  setMemoSaveStatus('saved', '保存済み');
}

function rememberPendingMemoSave(requestGeneration) {
  appContext.memo.pendingSaveGenerations.push(requestGeneration);
}

function finishPendingMemoSave(requestGeneration) {
  appContext.memo.pendingSaveGenerations = appContext.memo.pendingSaveGenerations.filter(function(generation) {
    return generation !== requestGeneration;
  });
}

function clearStaleMemoSavingStatus() {
  if (appContext.memo.pendingSaveGenerations.length > 0) return;
  if (!appContext.elements.memoSaveStatusEl || appContext.elements.memoSaveStatusEl.dataset.state !== 'saving') return;
  setMemoSaveStatus('saved', '保存済み');
}

function cancelMemoAutosave() {
  if (appContext.memo.saveTimer) {
    clearTimeout(appContext.memo.saveTimer);
    appContext.memo.saveTimer = null;
  }
}

function scheduleMemoSave(immediate) {
  if (!appContext.elements.memoEditorEl || isMemoEditorDisabled()) return;
  cancelMemoAutosave();
  setMemoSaveStatus('dirty', '未保存');
  if (immediate) {
    saveMemoNow();
    return;
  }
  appContext.memo.saveTimer = setTimeout(saveMemoNow, 500);
}

function saveMemoNow(targetFileOverride, rawOverride) {
  if (!appContext.elements.memoEditorEl || isMemoEditorDisabled()) {
    cancelMemoAutosave();
    return;
  }
  cancelMemoAutosave();
  var raw = rawOverride !== undefined ? rawOverride : appContext.elements.memoEditorEl.value;
  var requestGeneration = ++appContext.memo.saveGeneration;
  rememberPendingMemoSave(requestGeneration);
  var targetFile = targetFileOverride !== undefined
    ? targetFileOverride
    : getMemoTargetFile();
  setMemoSaveStatus('saving', '保存中');

  fetch('/api/memo', {
    method: 'PUT',
    headers: {
      'Accept': 'application/json',
      'Content-Type': 'application/json'
    },
    body: JSON.stringify({
      file: targetFile || null,
      raw: raw
    })
  })
  .then(parseJsonResponse)
  .then(function(data) {
    finishPendingMemoSave(requestGeneration);
    if (requestGeneration !== appContext.memo.saveGeneration) {
      console.warn('[markdown-view] 後続のメモ保存があるため古いレスポンスを破棄しました。', {
        requestGeneration: requestGeneration,
        currentGeneration: appContext.memo.saveGeneration
      });
      clearStaleMemoSavingStatus();
      return;
    }
    if (!appContext.elements.memoEditorEl) {
      console.warn('[markdown-view] メモエディタが見つからないため保存レスポンスを反映できません。');
      clearStaleMemoSavingStatus();
      return;
    }
    if (targetFileOverride === undefined && appContext.elements.memoEditorEl.value !== raw) {
      setMemoSaveStatus('dirty', '未保存');
      return;
    }
    if (targetFileOverride === undefined) {
      if ((data.raw || '') === raw) {
        updateMemoPreview(data);
      } else {
        if (applyMemoData(data, { preserveSelection: true }) === false) {
          return;
        }
      }
    }
    setMemoSaveStatus('saved', '保存済み');
    flushPendingMemoReloadIfSafe();
  })
  .catch(function(err) {
    console.error('[markdown-view] メモ保存エラー:', err);
    finishPendingMemoSave(requestGeneration);
    if (requestGeneration !== appContext.memo.saveGeneration) {
      console.warn('[markdown-view] 後続のメモ保存があるため古いエラーを破棄しました。', {
        requestGeneration: requestGeneration,
        currentGeneration: appContext.memo.saveGeneration
      });
      clearStaleMemoSavingStatus();
      return;
    }
    setMemoSaveStatus('error', getMemoErrorMessage(err));
    flushPendingMemoReloadIfSafe();
  });
}

function flushPendingMemoSave() {
  if (!appContext.elements.memoEditorEl || isMemoEditorDisabled() || !appContext.memo.saveTimer) return;
  saveMemoNow(getMemoTargetFile(), appContext.elements.memoEditorEl.value);
}

function escapeMarkdownLinkLabel(text) {
  return String(text || '')
    .replace(/\\/g, '\\\\')
    .replace(/\[/g, '\\[')
    .replace(/\]/g, '\\]');
}

function toBlockQuote(text) {
  return String(text || '')
    .trim()
    .split(/\r?\n/)
    .map(function(line) { return '> ' + line; })
    .join('\n');
}

function formatLineLabel(range) {
  if (!range) return '';
  if (range.start === range.end) {
    return 'L' + range.start;
  }
  return 'L' + range.start + '-L' + range.end;
}

function getHeadingForRange(range) {
  if (!appContext.elements.contentRoot || !range) return null;
  var startNode = range.startContainer.nodeType === Node.ELEMENT_NODE
    ? range.startContainer
    : range.startContainer.parentElement;
  if (!startNode) return null;
  var headings = appContext.elements.contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6');
  var lastHeading = null;
  headings.forEach(function(heading) {
    if (!heading.id) return;
    if (heading === startNode || heading.contains(startNode)) {
      lastHeading = heading;
      return;
    }
    var position = heading.compareDocumentPosition(startNode);
    if (position & Node.DOCUMENT_POSITION_FOLLOWING) {
      lastHeading = heading;
    }
  });
  return lastHeading;
}

function getSelectionLineRange(range) {
  if (!appContext.elements.contentRoot || !range) return null;
  var nodes = appContext.elements.contentRoot.querySelectorAll('[data-source-start-line]');
  var start = null;
  var end = null;
  nodes.forEach(function(node) {
    try {
      if (!range.intersectsNode(node)) return;
    } catch (err) {
      return;
    }
    var nodeStart = parseInt(node.getAttribute('data-source-start-line'), 10);
    var nodeEnd = parseInt(node.getAttribute('data-source-end-line'), 10);
    if (!isNaN(nodeStart) && (start === null || nodeStart < start)) {
      start = nodeStart;
    }
    if (!isNaN(nodeEnd) && (end === null || nodeEnd > end)) {
      end = nodeEnd;
    }
  });
  if (start === null || end === null) return null;
  return { start: start, end: end };
}

function buildQuoteSource(range) {
  var heading = getHeadingForRange(range);
  var lineRange = getSelectionLineRange(range);
  var fileLabel = appContext.state.currentFile || (appContext.elements.contentRoot ? appContext.elements.contentRoot.getAttribute('data-title') : 'document');
  var sourceLabel = fileLabel;
  var href = appContext.config.isDirMode && appContext.state.currentFile
    ? '?file=' + encodeURIComponent(appContext.state.currentFile)
    : location.pathname;

  if (heading) {
    sourceLabel += ' > ' + getHeadingLabel(heading);
    href += '#' + heading.id;
  }

  var lineLabel = formatLineLabel(lineRange);
  if (lineLabel) {
    sourceLabel += ' (' + lineLabel + ')';
    // コロン区切りで見出しIDと衝突を避けつつfragmentに行範囲を埋め込む。
    // 見出しなし時は `#L5-L7` 形式。クライアント側 parseLineHash で復元される。
    href += (heading ? ':' : '#') + lineLabel;
  }

  return {
    label: escapeMarkdownLinkLabel(sourceLabel),
    href: href,
    lines: lineLabel
  };
}

function getHeadingLabel(heading) {
  if (!heading) return '';
  var clone = heading.cloneNode(true);
  var anchorButton = clone.querySelector('.heading-anchor');
  if (anchorButton) {
    anchorButton.remove();
  }
  return clone.textContent.trim().replace(/\s+/g, ' ');
}

function buildQuoteMarkdownFromSelection() {
  var selection = window.getSelection();
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) return '';
  var range = selection.getRangeAt(0);
  var rawText = selection.toString().trim();
  if (!rawText || !isSelectionInsideContent(selection)) return '';

  var source = buildQuoteSource(range);
  var quote = toBlockQuote(rawText);
  // 行番号は source.label 内に `(L5-L7)` として埋め込み済み。
  // hrefにもfragmentとして行範囲が含まれ、クリック時の本文ジャンプに使われる。
  var citation = '出典: [' + source.label + '](' + source.href + ')';
  return quote + '\n\n' + citation + '\n';
}

function insertTextIntoMemo(text) {
  if (!appContext.elements.memoEditorEl || isMemoEditorDisabled()) return false;
  var currentValue = appContext.elements.memoEditorEl.value;
  var start = typeof appContext.memo.caretStart === 'number' ? appContext.memo.caretStart : currentValue.length;
  var end = typeof appContext.memo.caretEnd === 'number' ? appContext.memo.caretEnd : start;
  var prefix = start > 0 && !/\n\n$/.test(currentValue.slice(0, start)) ? '\n\n' : '';
  var suffix = end < currentValue.length && !/^\n/.test(currentValue.slice(end)) ? '\n' : '';
  var insertion = prefix + text + suffix;
  appContext.elements.memoEditorEl.value = currentValue.slice(0, start) + insertion + currentValue.slice(end);
  appContext.memo.caretStart = start + insertion.length;
  appContext.memo.caretEnd = appContext.memo.caretStart;
  appContext.elements.memoEditorEl.focus();
  appContext.elements.memoEditorEl.setSelectionRange(appContext.memo.caretStart, appContext.memo.caretEnd);
  return true;
}

function isSelectionInsideContent(selection) {
  if (!selection || selection.rangeCount === 0 || !appContext.elements.contentRoot) return false;
  var range = selection.getRangeAt(0);
  var ancestor = range.commonAncestorContainer.nodeType === Node.ELEMENT_NODE
    ? range.commonAncestorContainer
    : range.commonAncestorContainer.parentElement;
  return !!ancestor && appContext.elements.contentRoot.contains(ancestor);
}

function positionQuoteSelectionAction(range) {
  if (!appContext.elements.quoteSelectionActionEl || !range) return;
  var rect = range.getBoundingClientRect();
  if (!rect || (!rect.width && !rect.height)) {
    hideQuoteSelectionAction();
    return;
  }
  appContext.elements.quoteSelectionActionEl.hidden = false;
  appContext.elements.quoteSelectionActionEl.style.top = (window.scrollY + rect.bottom + 10) + 'px';
  appContext.elements.quoteSelectionActionEl.style.left = (window.scrollX + rect.left + Math.max(rect.width / 2, 16)) + 'px';
}

function hideQuoteSelectionAction() {
  if (!appContext.elements.quoteSelectionActionEl) return;
  appContext.elements.quoteSelectionActionEl.hidden = true;
}

function refreshQuoteSelectionAction() {
  if (!appContext.elements.quoteSelectionActionEl) return;
  var selection = window.getSelection();
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed || !isSelectionInsideContent(selection)) {
    hideQuoteSelectionAction();
    return;
  }
  if (!selection.toString().trim()) {
    hideQuoteSelectionAction();
    return;
  }
  positionQuoteSelectionAction(selection.getRangeAt(0));
}

function setupMemoInteractions() {
  if (appContext.elements.memoEditorEl) {
    ['click', 'keyup', 'select'].forEach(function(eventName) {
      appContext.elements.memoEditorEl.addEventListener(eventName, rememberMemoCaret);
    });
    appContext.elements.memoEditorEl.addEventListener('blur', function() {
      flushPendingMemoReloadIfSafe();
    });
    appContext.elements.memoEditorEl.addEventListener('input', function() {
      rememberMemoCaret();
      scheduleMemoSave(false);
    });
  }

  if (appContext.elements.quoteSelectionActionEl) {
    appContext.elements.quoteSelectionActionEl.addEventListener('mousedown', function(event) {
      event.preventDefault();
    });
    appContext.elements.quoteSelectionActionEl.addEventListener('click', function() {
      var markdown = buildQuoteMarkdownFromSelection();
      if (!markdown) {
        hideQuoteSelectionAction();
        return;
      }
      activateSidebarTab('memo');
      if (!insertTextIntoMemo(markdown)) {
        hideQuoteSelectionAction();
        return;
      }
      hideQuoteSelectionAction();
      scheduleMemoSave(true);
    });
  }

  document.addEventListener('selectionchange', function() {
    requestAnimationFrame(refreshQuoteSelectionAction);
  });
  window.addEventListener('scroll', hideQuoteSelectionAction, { passive: true });
  window.addEventListener('resize', hideQuoteSelectionAction);
}
