function getMemoRequestUrl(file) {
  if (isDirMode && file) {
    return '/api/memo?file=' + encodeURIComponent(file);
  }
  return '/api/memo';
}

function getMemoTargetFile(file) {
  if (isDirMode) {
    return file || currentFile || '';
  }
  return '';
}

function setMemoSaveStatus(state, text) {
  if (!memoSaveStatusEl) return;
  memoSaveStatusEl.dataset.state = state;
  memoSaveStatusEl.textContent = text;
}

function rememberMemoCaret() {
  if (!memoEditorEl) return;
  memoCaretStart = typeof memoEditorEl.selectionStart === 'number'
    ? memoEditorEl.selectionStart
    : memoEditorEl.value.length;
  memoCaretEnd = typeof memoEditorEl.selectionEnd === 'number'
    ? memoEditorEl.selectionEnd
    : memoCaretStart;
}

function snapshotMemoSelection() {
  if (!memoEditorEl) return null;
  return {
    start: typeof memoEditorEl.selectionStart === 'number' ? memoEditorEl.selectionStart : 0,
    end: typeof memoEditorEl.selectionEnd === 'number' ? memoEditorEl.selectionEnd : 0,
    isFocused: document.activeElement === memoEditorEl
  };
}

function restoreMemoSelection(selection) {
  if (!memoEditorEl || !selection || !selection.isFocused) return;
  var valueLength = memoEditorEl.value.length;
  var start = Math.min(selection.start, valueLength);
  var end = Math.min(selection.end, valueLength);
  memoEditorEl.focus();
  memoEditorEl.setSelectionRange(start, end);
}

function updateMemoEditor(raw, preserveSelection) {
  if (!memoEditorEl) return;
  var selection = preserveSelection ? snapshotMemoSelection() : null;
  memoEditorEl.value = raw || '';
  restoreMemoSelection(selection);
  rememberMemoCaret();
}

function updateMemoPreview(data) {
  if (!memoPreviewEl || !data) return;
  memoPreviewEl.innerHTML = data.html || '';
}

function applyMemoData(data, options) {
  if (!memoEditorEl || !memoPreviewEl || !data) return;
  var shouldUpdateEditor = !options || options.updateEditor !== false;
  if (shouldUpdateEditor) {
    updateMemoEditor(data.raw || '', !!(options && options.preserveSelection));
  }
  updateMemoPreview(data);
}

function isMemoUpdateMessage(data) {
  return !!data
    && data.type === 'memo_update'
    && typeof data.file === 'string'
    && data.file.length > 0;
}

function getMemoRemoteUpdateBlockReason() {
  if (!memoEditorEl) return 'none';
  if (memoSaveTimer) return 'dirty';
  if (memoSaveStatusEl) {
    var state = memoSaveStatusEl.dataset.state;
    if (state === 'dirty' || state === 'saving' || state === 'loading') return state;
  }
  if (document.activeElement === memoEditorEl) return 'focus';
  return 'none';
}

function isMemoRemoteUpdateBlocked() {
  return getMemoRemoteUpdateBlockReason() !== 'none';
}

function applyResolvedRemoteMemoUpdate(data) {
  cancelMemoAutosave();
  // 古いHTTP応答が直後に戻ってきても上書きしないよう世代を進める。
  memoLoadGeneration++;
  memoSaveGeneration++;
  applyMemoData(data);
  setMemoSaveStatus('saved', '保存済み');
  return true;
}

function flushPendingMemoUpdateIfSafe() {
  if (!pendingMemoUpdate) return false;
  if (isDirMode && pendingMemoUpdate.file !== currentFile) {
    pendingMemoUpdate = null;
    return false;
  }
  if (getMemoRemoteUpdateBlockReason() !== 'none') {
    return false;
  }
  var data = pendingMemoUpdate;
  pendingMemoUpdate = null;
  return applyResolvedRemoteMemoUpdate(data);
}

function flushPendingMemoReloadIfSafe() {
  if (!pendingMemoReload) return false;
  if (isDirMode && pendingMemoReload !== currentFile) {
    pendingMemoReload = null;
    return false;
  }
  if (getMemoRemoteUpdateBlockReason() !== 'none') {
    return false;
  }

  var file = pendingMemoReload;
  pendingMemoReload = null;
  loadMemo(file, fetchGeneration);
  return true;
}

function applyRemoteMemoUpdate(data) {
  if (!isMemoUpdateMessage(data)) return false;
  if (isDirMode && data.file !== currentFile) return false;

  var blockReason = getMemoRemoteUpdateBlockReason();
  if (blockReason === 'focus') {
    pendingMemoUpdate = data;
    return false;
  }
  if (blockReason !== 'none') {
    pendingMemoUpdate = null;
    pendingMemoReload = data.file;
    return false;
  }

  pendingMemoReload = null;
  pendingMemoUpdate = null;
  return applyResolvedRemoteMemoUpdate(data);
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
        return 'メモサイズが上限（' + MAX_FILE_SIZE_MB + 'MB）を超えています。';
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
  if (!memoEditorEl) return Promise.resolve();
  var requestGeneration = ++memoLoadGeneration;
  setMemoSaveStatus('loading', '読込中');
  return fetch(getMemoRequestUrl(file), {
    headers: { 'Accept': 'application/json' }
  })
  .then(parseJsonResponse)
  .then(function(data) {
    if (ownerGeneration !== undefined && ownerGeneration !== fetchGeneration) return;
    if (requestGeneration !== memoLoadGeneration) return;
    applyMemoData(data);
    setMemoSaveStatus('saved', '保存済み');
    flushPendingMemoUpdateIfSafe();
    flushPendingMemoReloadIfSafe();
  })
  .catch(function(err) {
    console.error('[markdown-view] メモ取得エラー:', err);
    if (ownerGeneration !== undefined && ownerGeneration !== fetchGeneration) return;
    if (requestGeneration !== memoLoadGeneration) return;
    setMemoSaveStatus('error', getMemoErrorMessage(err));
    flushPendingMemoUpdateIfSafe();
    flushPendingMemoReloadIfSafe();
  });
}

function cancelMemoAutosave() {
  if (memoSaveTimer) {
    clearTimeout(memoSaveTimer);
    memoSaveTimer = null;
  }
}

function scheduleMemoSave(immediate) {
  if (!memoEditorEl) return;
  cancelMemoAutosave();
  // ローカル編集開始後は、focus中に保留した古いリモート更新を破棄する。
  pendingMemoUpdate = null;
  setMemoSaveStatus('dirty', '未保存');
  if (immediate) {
    saveMemoNow();
    return;
  }
  memoSaveTimer = setTimeout(saveMemoNow, 500);
}

function saveMemoNow(targetFileOverride, rawOverride) {
  if (!memoEditorEl) return;
  cancelMemoAutosave();
  var raw = rawOverride !== undefined ? rawOverride : memoEditorEl.value;
  var requestGeneration = ++memoSaveGeneration;
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
    if (requestGeneration !== memoSaveGeneration) return;
    if (!memoEditorEl) return;
    if (targetFileOverride === undefined && memoEditorEl.value !== raw) {
      setMemoSaveStatus('dirty', '未保存');
      return;
    }
    if (targetFileOverride === undefined) {
      if ((data.raw || '') === raw) {
        updateMemoPreview(data);
      } else {
        applyMemoData(data, { preserveSelection: true });
      }
    }
    setMemoSaveStatus('saved', '保存済み');
    flushPendingMemoUpdateIfSafe();
    flushPendingMemoReloadIfSafe();
  })
  .catch(function(err) {
    console.error('[markdown-view] メモ保存エラー:', err);
    if (requestGeneration !== memoSaveGeneration) return;
    setMemoSaveStatus('error', getMemoErrorMessage(err));
    flushPendingMemoUpdateIfSafe();
    flushPendingMemoReloadIfSafe();
  });
}

function flushPendingMemoSave() {
  if (!memoEditorEl || !memoSaveTimer) return;
  saveMemoNow(getMemoTargetFile(), memoEditorEl.value);
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
  if (!contentRoot || !range) return null;
  var startNode = range.startContainer.nodeType === Node.ELEMENT_NODE
    ? range.startContainer
    : range.startContainer.parentElement;
  if (!startNode) return null;
  var headings = contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6');
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
  if (!contentRoot || !range) return null;
  var nodes = contentRoot.querySelectorAll('[data-source-start-line]');
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
  var fileLabel = currentFile || (contentRoot ? contentRoot.getAttribute('data-title') : 'document');
  var sourceLabel = fileLabel;
  var href = isDirMode && currentFile
    ? '?file=' + encodeURIComponent(currentFile)
    : location.pathname;

  if (heading) {
    sourceLabel += ' > ' + getHeadingLabel(heading);
    href += '#' + heading.id;
  }

  return {
    label: escapeMarkdownLinkLabel(sourceLabel),
    href: href,
    lines: formatLineLabel(lineRange)
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
  var citation = '出典: [' + source.label + '](' + source.href + ')';
  if (source.lines) {
    citation += ' ' + source.lines;
  }
  return quote + '\n\n' + citation + '\n';
}

function insertTextIntoMemo(text) {
  if (!memoEditorEl) return;
  var currentValue = memoEditorEl.value;
  var start = typeof memoCaretStart === 'number' ? memoCaretStart : currentValue.length;
  var end = typeof memoCaretEnd === 'number' ? memoCaretEnd : start;
  var prefix = start > 0 && !/\n\n$/.test(currentValue.slice(0, start)) ? '\n\n' : '';
  var suffix = end < currentValue.length && !/^\n/.test(currentValue.slice(end)) ? '\n' : '';
  var insertion = prefix + text + suffix;
  memoEditorEl.value = currentValue.slice(0, start) + insertion + currentValue.slice(end);
  memoCaretStart = start + insertion.length;
  memoCaretEnd = memoCaretStart;
  memoEditorEl.focus();
  memoEditorEl.setSelectionRange(memoCaretStart, memoCaretEnd);
}

function isSelectionInsideContent(selection) {
  if (!selection || selection.rangeCount === 0 || !contentRoot) return false;
  var range = selection.getRangeAt(0);
  var ancestor = range.commonAncestorContainer.nodeType === Node.ELEMENT_NODE
    ? range.commonAncestorContainer
    : range.commonAncestorContainer.parentElement;
  return !!ancestor && contentRoot.contains(ancestor);
}

function positionQuoteSelectionAction(range) {
  if (!quoteSelectionActionEl || !range) return;
  var rect = range.getBoundingClientRect();
  if (!rect || (!rect.width && !rect.height)) {
    hideQuoteSelectionAction();
    return;
  }
  quoteSelectionActionEl.hidden = false;
  quoteSelectionActionEl.style.top = (window.scrollY + rect.bottom + 10) + 'px';
  quoteSelectionActionEl.style.left = (window.scrollX + rect.left + Math.max(rect.width / 2, 16)) + 'px';
}

function hideQuoteSelectionAction() {
  if (!quoteSelectionActionEl) return;
  quoteSelectionActionEl.hidden = true;
}

function refreshQuoteSelectionAction() {
  if (!quoteSelectionActionEl) return;
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

if (memoEditorEl) {
  ['click', 'keyup', 'select'].forEach(function(eventName) {
    memoEditorEl.addEventListener(eventName, rememberMemoCaret);
  });
  memoEditorEl.addEventListener('blur', function() {
    flushPendingMemoUpdateIfSafe();
    flushPendingMemoReloadIfSafe();
  });
  memoEditorEl.addEventListener('input', function() {
    rememberMemoCaret();
    scheduleMemoSave(false);
  });
}

if (quoteSelectionActionEl) {
  quoteSelectionActionEl.addEventListener('mousedown', function(event) {
    event.preventDefault();
  });
  quoteSelectionActionEl.addEventListener('click', function() {
    var markdown = buildQuoteMarkdownFromSelection();
    if (!markdown) {
      hideQuoteSelectionAction();
      return;
    }
    activateSidebarTab('memo');
    insertTextIntoMemo(markdown);
    hideQuoteSelectionAction();
    scheduleMemoSave(true);
  });
}

document.addEventListener('selectionchange', function() {
  requestAnimationFrame(refreshQuoteSelectionAction);
});
window.addEventListener('scroll', hideQuoteSelectionAction, { passive: true });
window.addEventListener('resize', hideQuoteSelectionAction);
