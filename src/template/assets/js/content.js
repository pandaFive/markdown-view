var contentEnhancements = createContentEnhancements(appContext, {
  clearMemoSyncPendingStatus: clearMemoSyncPendingStatus
});
var setLiveStatus = contentEnhancements.setLiveStatus;
var updateDocumentStats = contentEnhancements.updateDocumentStats;
var updateReadingProgress = contentEnhancements.updateReadingProgress;
var syncDocumentChrome = contentEnhancements.syncDocumentChrome;
var enhanceContentInteractions = contentEnhancements.enhanceContentInteractions;
var setupFilterableList = contentEnhancements.setupFilterableList;
var setupTocFilter = contentEnhancements.setupTocFilter;

var contentNavigation = createContentNavigation(appContext, {
  selectFile: selectFile,
  setFileParam: setFileParam,
  markPendingTocNavigation: markPendingTocNavigation,
  clearPendingTocNavigation: clearPendingTocNavigation,
  restoreActiveTocHeading: restoreActiveTocHeading
});
var augmentHashWithTrailingLineHint = contentNavigation.augmentHashWithTrailingLineHint;
var applyContentAnchorNavigation = contentNavigation.applyContentAnchorNavigation;
var restoreContentNavigationFromLocation = contentNavigation.restoreContentNavigationFromLocation;
var setupContentLinkNavigation = contentNavigation.setupContentLinkNavigation;
var setupMemoLinkNavigation = contentNavigation.setupMemoLinkNavigation;
var setLocationHash = contentNavigation.setLocationHash;

function createDocumentSearchEmptyState(message) {
  var empty = document.createElement('p');
  empty.className = 'document-search-empty';
  empty.textContent = message;
  return empty;
}

function createDirectorySearchTruncatedState() {
  var item = document.createElement('div');
  item.className = 'document-search-empty';
  item.textContent = '上限により一部のみ表示しています。';
  return item;
}

function formatDirectorySearchSummary() {
  var baseText;
  if (!appContext.search.currentDocumentQuery) {
    baseText = '0 件';
  } else if (appContext.search.currentDirectoryLoading) {
    baseText = '検索中...';
  } else if (appContext.search.currentDirectoryError) {
    baseText = 'エラー';
  } else if (!appContext.search.currentDirectoryResults.length) {
    baseText = '0 件';
  } else if (appContext.search.currentDirectoryIndex >= 0) {
    baseText = (appContext.search.currentDirectoryIndex + 1) + ' / ' + appContext.search.currentDirectoryResults.length + ' 件';
  } else {
    baseText = '0 / ' + appContext.search.currentDirectoryResults.length + ' 件';
  }

  if (appContext.search.currentDirectorySkippedFiles > 0) {
    return baseText + '（' + appContext.search.currentDirectorySkippedFiles + '件スキップ）';
  }
  return baseText;
}

function updateDocumentSearchSummary() {
  if (!appContext.elements.documentSearchSummaryEl) return;
  if (appContext.config.isDirMode) {
    appContext.elements.documentSearchSummaryEl.textContent = formatDirectorySearchSummary();
    return;
  }
  if (!appContext.search.documentMatches.length) {
    appContext.elements.documentSearchSummaryEl.textContent = '0 件';
    return;
  }
  appContext.elements.documentSearchSummaryEl.textContent = (appContext.search.currentDocumentIndex + 1) + ' / ' + appContext.search.documentMatches.length + ' 件';
}

function clearDocumentSearchHighlights() {
  if (!appContext.elements.contentRoot) return;
  appContext.elements.contentRoot.querySelectorAll('mark.document-search-match').forEach(function(mark) {
    var parent = mark.parentNode;
    if (!parent) return;
    parent.replaceChild(document.createTextNode(mark.textContent || ''), mark);
    parent.normalize();
  });
  appContext.search.documentMatches = [];
  appContext.search.currentDocumentIndex = -1;
  updateDocumentSearchSummary();
  renderDocumentSearchResults();
}

var DOCUMENT_SEARCH_BLOCK_SELECTOR = 'p, li, blockquote, th, td, h1, h2, h3, h4, h5, h6';

function shouldSkipDocumentSearchNode(node) {
  var parent = node.parentElement;
  if (!parent) return true;
  return Boolean(parent.closest(
    'a, button, input, textarea, script, style, pre.code-block, mark.document-search-match'
  ));
}

function createDocumentSearchMark(text, matchId) {
  var mark = document.createElement('mark');
  mark.className = 'document-search-match';
  mark.dataset.matchId = String(matchId);
  mark.textContent = text;
  return mark;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function trimSentenceRange(text, start, end) {
  var nextStart = start;
  var nextEnd = end;
  while (nextStart < nextEnd && /\s/.test(text.charAt(nextStart))) {
    nextStart += 1;
  }
  while (nextEnd > nextStart && /\s/.test(text.charAt(nextEnd - 1))) {
    nextEnd -= 1;
  }
  return { start: nextStart, end: nextEnd };
}

function splitTextIntoSentenceRanges(text) {
  var ranges = [];
  var sentenceStart = 0;
  var i;

  for (i = 0; i < text.length; i++) {
    var char = text.charAt(i);
    if (char !== '\n' && !/[.!?。！？]/.test(char)) continue;
    var trimmed = trimSentenceRange(text, sentenceStart, i + 1);
    if (trimmed.end > trimmed.start) {
      ranges.push(trimmed);
    }
    sentenceStart = i + 1;
  }

  if (sentenceStart < text.length) {
    var tail = trimSentenceRange(text, sentenceStart, text.length);
    if (tail.end > tail.start) {
      ranges.push(tail);
    }
  }

  if (!ranges.length) {
    var whole = trimSentenceRange(text, 0, text.length);
    if (whole.end > whole.start) {
      ranges.push(whole);
    }
  }

  return ranges;
}

function getSentenceForMatch(sentenceRanges, matchStart, matchEnd) {
  var i;
  for (i = 0; i < sentenceRanges.length; i++) {
    if (matchStart < sentenceRanges[i].end && matchEnd > sentenceRanges[i].start) {
      return i;
    }
  }
  return sentenceRanges.length ? 0 : -1;
}

function getAdjacentSentence(blockEntries, blockIndex, sentenceIndex, direction) {
  var targetBlockIndex = blockIndex;
  var targetSentenceIndex = sentenceIndex + direction;

  while (targetBlockIndex >= 0 && targetBlockIndex < blockEntries.length) {
    var entry = blockEntries[targetBlockIndex];
    if (targetSentenceIndex >= 0 && targetSentenceIndex < entry.sentences.length) {
      return entry.text.slice(entry.sentences[targetSentenceIndex].start, entry.sentences[targetSentenceIndex].end);
    }
    targetBlockIndex += direction;
    if (targetBlockIndex < 0 || targetBlockIndex >= blockEntries.length) break;
    targetSentenceIndex = direction > 0 ? 0 : blockEntries[targetBlockIndex].sentences.length - 1;
  }

  return '';
}

function buildDocumentSearchContext(blockEntries, blockIndex, matchStart, matchEnd) {
  var entry = blockEntries[blockIndex];
  var sentenceIndex = getSentenceForMatch(entry.sentences, matchStart, matchEnd);
  var currentSentenceRange = sentenceIndex >= 0 ? entry.sentences[sentenceIndex] : null;
  var currentSentence = currentSentenceRange
    ? entry.text.slice(currentSentenceRange.start, currentSentenceRange.end)
    : entry.text.trim();

  return {
    before: getAdjacentSentence(blockEntries, blockIndex, sentenceIndex, -1),
    current: currentSentence,
    after: getAdjacentSentence(blockEntries, blockIndex, sentenceIndex, 1)
  };
}

function getDocumentSearchBlocks() {
  if (!appContext.elements.contentRoot) return [];
  return Array.prototype.filter.call(
    appContext.elements.contentRoot.querySelectorAll(DOCUMENT_SEARCH_BLOCK_SELECTOR),
    function(block) {
      return !block.parentElement || !block.parentElement.closest(DOCUMENT_SEARCH_BLOCK_SELECTOR);
    }
  );
}

function collectDocumentSearchTextNodes(block) {
  var walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT, null);
  var textNodes = [];
  var node;
  var offset = 0;

  while ((node = walker.nextNode())) {
    if (!node.nodeValue || !node.nodeValue.trim()) continue;
    if (shouldSkipDocumentSearchNode(node)) continue;
    textNodes.push({
      node: node,
      start: offset,
      end: offset + node.nodeValue.length
    });
    offset += node.nodeValue.length;
  }

  return {
    text: textNodes.map(function(entry) { return entry.node.nodeValue; }).join(''),
    nodes: textNodes
  };
}

function wrapDocumentSearchSegment(textNode, start, end, matchId) {
  var tail = end < textNode.nodeValue.length ? textNode.splitText(end) : null;
  var matchNode = start > 0 ? textNode.splitText(start) : textNode;
  var mark = createDocumentSearchMark(matchNode.nodeValue, matchId);
  matchNode.parentNode.replaceChild(mark, matchNode);
  return { mark: mark, tail: tail };
}

function wrapDocumentSearchMatch(nodes, matchStart, matchEnd, matchId) {
  var marks = [];
  var i;

  for (i = nodes.length - 1; i >= 0; i--) {
    var entry = nodes[i];
    var localStart = Math.max(0, matchStart - entry.start);
    var localEnd = Math.min(entry.end - entry.start, matchEnd - entry.start);
    if (localStart >= localEnd) continue;
    marks.unshift(wrapDocumentSearchSegment(entry.node, localStart, localEnd, matchId).mark);
  }

  return marks;
}

function renderDocumentSearchResultContext(container, text, query, variant) {
  if (!text) return;
  var span = document.createElement('span');
  var normalizedText = text.replace(/\s+/g, ' ').trim();
  var escapedQuery = escapeRegExp(query);
  var parts = escapedQuery ? normalizedText.split(new RegExp('(' + escapedQuery + ')', 'ig')) : [normalizedText];

  span.className = 'document-search-result-context document-search-result-context-' + variant;
  if (container.childNodes.length > 0) {
    container.appendChild(document.createTextNode(' '));
  }
  parts.forEach(function(part) {
    if (!part) return;
    if (escapedQuery && new RegExp('^' + escapedQuery + '$', 'i').test(part)) {
      var mark = document.createElement('mark');
      mark.className = 'document-search-result-mark';
      mark.textContent = part;
      span.appendChild(mark);
      return;
    }
    span.appendChild(document.createTextNode(part));
  });
  container.appendChild(span);
}

function renderDocumentSearchResults() {
  if (!appContext.elements.documentSearchResultsEl) return;
  if (appContext.config.isDirMode) {
    renderDirectorySearchResults();
    return;
  }
  var preservedScrollTop = appContext.elements.documentSearchResultsEl.scrollTop;
  appContext.elements.documentSearchResultsEl.innerHTML = '';

  if (!appContext.search.currentDocumentQuery) return;

  if (!appContext.search.documentMatches.length) {
    appContext.elements.documentSearchResultsEl.appendChild(createDocumentSearchEmptyState('一致する文が見つかりません。'));
    return;
  }

  appContext.search.documentMatches.forEach(function(match, index) {
    var button = document.createElement('button');
    var indexBadge = document.createElement('span');
    var body = document.createElement('span');

    button.type = 'button';
    button.className = 'document-search-result';
    button.dataset.matchIndex = String(index);
    button.classList.toggle('active', index === appContext.search.currentDocumentIndex);
    button.setAttribute('aria-current', index === appContext.search.currentDocumentIndex ? 'true' : 'false');
    button.addEventListener('click', function() {
      setCurrentDocumentSearchMatch(index);
    });

    indexBadge.className = 'document-search-result-index';
    indexBadge.textContent = String(index + 1).padStart(2, '0');

    body.className = 'document-search-result-body';
    renderDocumentSearchResultContext(body, match.context.before, appContext.search.currentDocumentQuery, 'before');
    renderDocumentSearchResultContext(body, match.context.current, appContext.search.currentDocumentQuery, 'current');
    renderDocumentSearchResultContext(body, match.context.after, appContext.search.currentDocumentQuery, 'after');

    button.appendChild(indexBadge);
    button.appendChild(body);
    appContext.elements.documentSearchResultsEl.appendChild(button);
  });

  appContext.elements.documentSearchResultsEl.scrollTop = preservedScrollTop;
}

function renderDirectorySearchResults() {
  var preservedScrollTop = appContext.elements.documentSearchResultsEl.scrollTop;
  appContext.elements.documentSearchResultsEl.innerHTML = '';

  if (!appContext.search.currentDocumentQuery) return;

  if (appContext.search.currentDirectoryLoading) {
    appContext.elements.documentSearchResultsEl.appendChild(createDocumentSearchEmptyState('ディレクトリを検索しています。'));
    return;
  }

  if (appContext.search.currentDirectoryError) {
    appContext.elements.documentSearchResultsEl.appendChild(createDocumentSearchEmptyState(appContext.search.currentDirectoryError));
    return;
  }

  if (appContext.search.currentDirectoryTruncated) {
    appContext.elements.documentSearchResultsEl.appendChild(createDirectorySearchTruncatedState());
  }

  if (!appContext.search.currentDirectoryResults.length) {
    appContext.elements.documentSearchResultsEl.appendChild(createDocumentSearchEmptyState('ディレクトリ内に一致が見つかりません。'));
    return;
  }

  appContext.search.currentDirectoryResults.forEach(function(result, index) {
    var button = document.createElement('button');
    var indexBadge = document.createElement('span');
    var body = document.createElement('span');
    var path = document.createElement('span');

    button.type = 'button';
    button.className = 'document-search-result';
    button.dataset.resultIndex = String(index);
    button.classList.toggle('active', index === appContext.search.currentDirectoryIndex);
    button.setAttribute('aria-current', index === appContext.search.currentDirectoryIndex ? 'true' : 'false');
    button.addEventListener('click', function() {
      openDirectorySearchResult(index);
    });

    indexBadge.className = 'document-search-result-index';
    indexBadge.textContent = String(index + 1).padStart(2, '0');

    body.className = 'document-search-result-body';
    path.className = 'document-search-result-path';
    path.textContent = result.file;
    body.appendChild(path);
    renderDocumentSearchResultContext(body, result.before, appContext.search.currentDocumentQuery, 'before');
    renderDocumentSearchResultContext(body, result.current, appContext.search.currentDocumentQuery, 'current');
    renderDocumentSearchResultContext(body, result.after, appContext.search.currentDocumentQuery, 'after');

    button.appendChild(indexBadge);
    button.appendChild(body);
    appContext.elements.documentSearchResultsEl.appendChild(button);
  });

  appContext.elements.documentSearchResultsEl.scrollTop = preservedScrollTop;
}

function renderDirectorySearchUi() {
  updateDocumentSearchSummary();
  renderDocumentSearchResults();
}

function applyPendingDirectorySearchNavigation() {
  if (!appContext.config.isDirMode || !appContext.search.pendingDirectoryNavigation) return;
  if (appContext.search.pendingDirectoryNavigation.file !== appContext.state.currentFile) return;
  if (appContext.search.pendingDirectoryNavigation.query !== appContext.search.currentDocumentQuery) {
    appContext.search.pendingDirectoryNavigation = null;
    return;
  }
  if (appContext.search.documentMatches.length) {
    setCurrentDocumentSearchMatch(
      Math.min(appContext.search.pendingDirectoryNavigation.fileMatchIndex, appContext.search.documentMatches.length - 1)
    );
  }
  appContext.search.currentDirectoryIndex = appContext.search.pendingDirectoryNavigation.resultIndex;
  appContext.search.pendingDirectoryNavigation = null;
}

function scheduleDirectorySearch(query) {
  if (appContext.search.documentDebounceTimer) {
    clearTimeout(appContext.search.documentDebounceTimer);
  }
  appContext.search.documentDebounceTimer = setTimeout(function() {
    appContext.search.documentDebounceTimer = null;
    runDirectorySearch(query);
  }, 300);
}

function getPreferredDirectorySearchSelection() {
  if (appContext.search.pendingDirectoryNavigation) {
    return {
      file: appContext.search.pendingDirectoryNavigation.file,
      fileMatchIndex: appContext.search.pendingDirectoryNavigation.fileMatchIndex
    };
  }
  if (
    appContext.search.currentDirectoryIndex >= 0 &&
    appContext.search.currentDirectoryIndex < appContext.search.currentDirectoryResults.length
  ) {
    return {
      file: appContext.search.currentDirectoryResults[appContext.search.currentDirectoryIndex].file,
      fileMatchIndex: appContext.search.currentDirectoryResults[appContext.search.currentDirectoryIndex].file_match_index
    };
  }
  if (
    appContext.state.currentFile &&
    appContext.search.currentDocumentIndex >= 0 &&
    appContext.search.currentDocumentIndex < appContext.search.documentMatches.length
  ) {
    return {
      file: appContext.state.currentFile,
      fileMatchIndex: appContext.search.currentDocumentIndex
    };
  }
  return null;
}

function resolveDirectorySearchIndex(results, preferredSelection) {
  var index;
  if (!results.length) return -1;
  if (preferredSelection) {
    index = results.findIndex(function(result) {
      return (
        result.file === preferredSelection.file &&
        result.file_match_index === preferredSelection.fileMatchIndex
      );
    });
    if (index !== -1) return index;
  }
  if (
    appContext.state.currentFile &&
    appContext.search.currentDocumentIndex >= 0 &&
    appContext.search.currentDocumentIndex < appContext.search.documentMatches.length
  ) {
    index = results.findIndex(function(result) {
      return (
        result.file === appContext.state.currentFile &&
        result.file_match_index === appContext.search.currentDocumentIndex
      );
    });
    if (index !== -1) return index;
  }
  return -1;
}

function runDirectorySearch(query) {
  var generation = ++appContext.search.documentFetchGeneration;
  var preferredSelection = getPreferredDirectorySearchSelection();
  appContext.search.currentDirectoryLoading = true;
  appContext.search.currentDirectoryError = '';
  appContext.search.currentDirectoryResults = [];
  appContext.search.currentDirectoryIndex = -1;
  appContext.search.currentDirectorySkippedFiles = 0;
  appContext.search.currentDirectoryTruncated = false;
  appContext.search.currentDirectoryTruncatedReasons = [];
  renderDirectorySearchUi();

  fetch('/api/search?q=' + encodeURIComponent(query), {
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
    if (generation !== appContext.search.documentFetchGeneration) return;
    if ((data.query || '') !== appContext.search.currentDocumentQuery) return;
    appContext.search.currentDirectoryLoading = false;
    appContext.search.currentDirectoryError = '';
    appContext.search.currentDirectoryResults = Array.isArray(data.results) ? data.results : [];
    appContext.search.currentDirectorySkippedFiles = Number(data.skipped_files || 0);
    appContext.search.currentDirectoryTruncated = data.truncated === true ||
      (Array.isArray(data.truncated_reasons) && data.truncated_reasons.length > 0);
    appContext.search.currentDirectoryTruncatedReasons = Array.isArray(data.truncated_reasons)
      ? data.truncated_reasons.slice()
      : [];
    appContext.search.currentDirectoryIndex = resolveDirectorySearchIndex(
      appContext.search.currentDirectoryResults,
      preferredSelection
    );
    renderDirectorySearchUi();
  })
  .catch(function(err) {
    if (generation !== appContext.search.documentFetchGeneration) return;
    if (query !== appContext.search.currentDocumentQuery) return;
    appContext.search.currentDirectoryLoading = false;
    appContext.search.currentDirectoryResults = [];
    appContext.search.currentDirectoryIndex = -1;
    appContext.search.currentDirectorySkippedFiles = 0;
    appContext.search.currentDirectoryTruncated = false;
    appContext.search.currentDirectoryTruncatedReasons = [];
    appContext.search.currentDirectoryError = getFileFetchErrorMessage(err);
    console.error('[markdown-view] ディレクトリ検索エラー:', err);
    renderDirectorySearchUi();
  });
}

function openDirectorySearchResult(index) {
  if (!appContext.search.currentDirectoryResults.length) return;
  var normalizedIndex = (index + appContext.search.currentDirectoryResults.length) % appContext.search.currentDirectoryResults.length;
  var result = appContext.search.currentDirectoryResults[normalizedIndex];
  var previousResultIndex = appContext.search.currentDirectoryIndex;
  appContext.search.currentDirectoryIndex = normalizedIndex;
  appContext.search.pendingDirectoryNavigation = {
    file: result.file,
    query: appContext.search.currentDocumentQuery,
    fileMatchIndex: result.file_match_index,
    resultIndex: normalizedIndex,
    previousResultIndex: previousResultIndex
  };
  renderDirectorySearchUi();

  if (result.file === appContext.state.currentFile) {
    applyPendingDirectorySearchNavigation();
    renderDirectorySearchUi();
    return;
  }

  selectFile(result.file, false, {
    scrollMode: 'none',
    requeryDirectorySearch: false
  });
}

function applyDocumentSearchHighlights(query) {
  if (!appContext.elements.contentRoot) return;
  clearDocumentSearchHighlights();
  if (!query) return;

  var normalizedQuery = query.toLowerCase();
  var blockEntries = getDocumentSearchBlocks().map(function(block) {
    var blockText = collectDocumentSearchTextNodes(block);
    return {
      text: blockText.text,
      nodes: blockText.nodes,
      sentences: splitTextIntoSentenceRanges(blockText.text)
    };
  });

  blockEntries.forEach(function(blockText, blockIndex) {
    var matchIndex;
    var searchIndex = 0;

    if (!blockText.text) return;

    matchIndex = blockText.text.toLowerCase().indexOf(normalizedQuery, searchIndex);
    while (matchIndex !== -1) {
      var marks = wrapDocumentSearchMatch(
        blockText.nodes,
        matchIndex,
        matchIndex + normalizedQuery.length,
        appContext.search.documentMatches.length
      );
      if (marks.length) {
        appContext.search.documentMatches.push({
          marks: marks,
          context: buildDocumentSearchContext(
            blockEntries,
            blockIndex,
            matchIndex,
            matchIndex + normalizedQuery.length
          )
        });
      }
      searchIndex = matchIndex + normalizedQuery.length;
      matchIndex = blockText.text.toLowerCase().indexOf(normalizedQuery, searchIndex);
    }
  });

  if (appContext.search.documentMatches.length) {
    setCurrentDocumentSearchMatch(0, false);
  } else {
    updateDocumentSearchSummary();
    renderDocumentSearchResults();
  }
}

function setCurrentDocumentSearchMatch(index, scrollIntoView) {
  if (!appContext.search.documentMatches.length) {
    appContext.search.currentDocumentIndex = -1;
    updateDocumentSearchSummary();
    return;
  }
  if (appContext.search.currentDocumentIndex >= 0 && appContext.search.documentMatches[appContext.search.currentDocumentIndex]) {
    appContext.search.documentMatches[appContext.search.currentDocumentIndex].marks.forEach(function(mark) {
      mark.classList.remove('current');
    });
  }
  appContext.search.currentDocumentIndex = (index + appContext.search.documentMatches.length) % appContext.search.documentMatches.length;
  appContext.search.documentMatches[appContext.search.currentDocumentIndex].marks.forEach(function(mark) {
    mark.classList.add('current');
  });
  if (scrollIntoView !== false) {
    appContext.search.documentMatches[appContext.search.currentDocumentIndex].marks[0].scrollIntoView({
      block: 'center',
      behavior: 'smooth'
    });
  }
  updateDocumentSearchSummary();
  renderDocumentSearchResults();
}

function moveDocumentSearch(step) {
  if (appContext.config.isDirMode) {
    if (!appContext.search.currentDirectoryResults.length) return;
    if (appContext.search.currentDirectoryIndex < 0) {
      openDirectorySearchResult(step > 0 ? 0 : appContext.search.currentDirectoryResults.length - 1);
      return;
    }
    openDirectorySearchResult(appContext.search.currentDirectoryIndex + step);
    return;
  }
  if (!appContext.search.documentMatches.length) return;
  setCurrentDocumentSearchMatch(appContext.search.currentDocumentIndex + step);
}

function applyDocumentSearchQuery(query) {
  appContext.search.currentDocumentQuery = (query || '').trim();
  if (appContext.config.isDirMode) {
    applyDocumentSearchHighlights(appContext.search.currentDocumentQuery);
    appContext.search.pendingDirectoryNavigation = null;
    if (!appContext.search.currentDocumentQuery) {
      if (appContext.search.documentDebounceTimer) {
        clearTimeout(appContext.search.documentDebounceTimer);
        appContext.search.documentDebounceTimer = null;
      }
      appContext.search.documentFetchGeneration += 1;
      appContext.search.currentDirectoryResults = [];
      appContext.search.currentDirectoryIndex = -1;
      appContext.search.currentDirectorySkippedFiles = 0;
      appContext.search.currentDirectoryTruncated = false;
      appContext.search.currentDirectoryTruncatedReasons = [];
      appContext.search.currentDirectoryLoading = false;
      appContext.search.currentDirectoryError = '';
      renderDirectorySearchUi();
      return;
    }
    appContext.search.currentDirectoryResults = [];
    appContext.search.currentDirectoryIndex = -1;
    appContext.search.currentDirectorySkippedFiles = 0;
    appContext.search.currentDirectoryTruncated = false;
    appContext.search.currentDirectoryTruncatedReasons = [];
    appContext.search.currentDirectoryLoading = true;
    appContext.search.currentDirectoryError = '';
    scheduleDirectorySearch(appContext.search.currentDocumentQuery);
    renderDirectorySearchUi();
    return;
  }
  applyDocumentSearchHighlights(appContext.search.currentDocumentQuery);
}

function clearDocumentSearchQuery() {
  if (appContext.elements.documentSearchInputEl) {
    appContext.elements.documentSearchInputEl.value = '';
  }
  if (appContext.search.documentDebounceTimer) {
    clearTimeout(appContext.search.documentDebounceTimer);
    appContext.search.documentDebounceTimer = null;
  }
  appContext.search.documentFetchGeneration += 1;
  appContext.search.currentDocumentQuery = '';
  appContext.search.pendingDirectoryNavigation = null;
  appContext.search.currentDirectoryResults = [];
  appContext.search.currentDirectoryIndex = -1;
  appContext.search.currentDirectorySkippedFiles = 0;
  appContext.search.currentDirectoryTruncated = false;
  appContext.search.currentDirectoryTruncatedReasons = [];
  appContext.search.currentDirectoryLoading = false;
  appContext.search.currentDirectoryError = '';
  clearDocumentSearchHighlights();
  if (appContext.config.isDirMode) {
    renderDirectorySearchUi();
  }
}

function syncDocumentSearchAfterContentUpdate(options) {
  options = options || {};
  if (!appContext.elements.documentSearchInputEl) return;
  if (appContext.config.isDirMode) {
    appContext.search.currentDocumentQuery = (appContext.elements.documentSearchInputEl.value || '').trim();
    applyDocumentSearchHighlights(appContext.search.currentDocumentQuery);
    applyPendingDirectorySearchNavigation();
    if (appContext.search.currentDocumentQuery && options.requeryDirectorySearch !== false) {
      scheduleDirectorySearch(appContext.search.currentDocumentQuery);
    }
    renderDirectorySearchUi();
    return;
  }
  applyDocumentSearchQuery(appContext.elements.documentSearchInputEl.value);
}

function openDocumentSearch() {
  activateSidebarTab('toc');
  var sidebarEl = document.getElementById('sidebar');
  if (sidebarEl) {
    sidebarEl.classList.add('open');
  }
  if (appContext.elements.documentSearchInputEl) {
    appContext.elements.documentSearchInputEl.focus();
    appContext.elements.documentSearchInputEl.select();
  }
}

function setupDocumentSearch() {
  if (!appContext.elements.documentSearchInputEl) return;

  appContext.elements.documentSearchInputEl.addEventListener('input', function() {
    applyDocumentSearchQuery(appContext.elements.documentSearchInputEl.value);
  });

  appContext.elements.documentSearchInputEl.addEventListener('keydown', function(event) {
    if (event.key === 'Enter') {
      event.preventDefault();
      moveDocumentSearch(event.shiftKey ? -1 : 1);
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      if (appContext.elements.documentSearchInputEl.value) {
        clearDocumentSearchQuery();
      } else {
        appContext.elements.documentSearchInputEl.blur();
      }
    }
  });

  if (appContext.elements.documentSearchPrevEl) {
    appContext.elements.documentSearchPrevEl.addEventListener('click', function() {
      moveDocumentSearch(-1);
    });
  }
  if (appContext.elements.documentSearchNextEl) {
    appContext.elements.documentSearchNextEl.addEventListener('click', function() {
      moveDocumentSearch(1);
    });
  }
  if (appContext.elements.documentSearchClearEl) {
    appContext.elements.documentSearchClearEl.addEventListener('click', function() {
      clearDocumentSearchQuery();
      appContext.elements.documentSearchInputEl.focus();
    });
  }

  document.addEventListener('keydown', function(event) {
    var key = event.key.toLowerCase();
    if ((event.ctrlKey || event.metaKey) && key === 'f') {
      event.preventDefault();
      openDocumentSearch();
    }
  });

  updateDocumentSearchSummary();
}

function applyPendingUpdate() {
  if (!appContext.state.pendingUpdate) return;
  if (appContext.state.pendingUpdateTimer) {
    clearTimeout(appContext.state.pendingUpdateTimer);
    appContext.state.pendingUpdateTimer = null;
  }
  if (appContext.state.pendingUpdate.refresh) {
    var refreshFile = appContext.state.pendingUpdate.file;
    appContext.state.pendingUpdate = null;
    if (appContext.config.isDirMode && refreshFile) {
      selectFile(refreshFile, false);
    } else {
      console.warn('[markdown-view] refresh 保留更新を適用できませんでした。', {
        isDirMode: appContext.config.isDirMode,
        refreshFile: refreshFile || '',
        currentFile: appContext.state.currentFile || ''
      });
    }
    return;
  }
  if (appContext.config.isDirMode && appContext.state.pendingUpdate.file && appContext.state.pendingUpdate.file !== appContext.state.currentFile) {
    console.warn('[markdown-view] 現在のファイルと異なる保留更新を破棄しました。', {
      currentFile: appContext.state.currentFile,
      messageFile: appContext.state.pendingUpdate.file
    });
    appContext.state.pendingUpdate = null;
    return;
  }
  var data = appContext.state.pendingUpdate;
  appContext.state.pendingUpdate = null;
  updateContent(data);
  hideWsServerErrorBanner();
  hideFileFetchErrorBanner();
  setLiveStatus('live');
}

// サーバーサイドでサニタイズ済みのHTMLを反映する
// XSS防止: src/renderer/render.rs で raw/inline HTML event を破棄済み
let updateContent = function updateContent(data, options) {
  options = options || {};
  var validation = validateUpdatePayload(data);
  var safeData = validation.safeData;

  if (validation.hasContractViolation) {
    logUpdatePayloadContractViolation(validation);
  }

  if (appContext.state.pendingUpdateTimer) {
    clearTimeout(appContext.state.pendingUpdateTimer);
    appContext.state.pendingUpdateTimer = null;
  }
  appContext.state.pendingUpdate = null;
  var scrollY = window.scrollY;
  var scrollMode = options.scrollMode || 'preserve';
  var preservedActiveTocId = getCurrentActiveTocId();
  var contentEl = document.getElementById('content');
  var tocEl = document.getElementById('toc');

  applyValidatedUpdateHtml(appContext, {
    contentEl: contentEl,
    tocEl: tocEl
  }, validation);

  setupTocTracking();
  suppressTocTrackingFor(120);

  requestAnimationFrame(function() {
    var currentScrollY = window.scrollY || window.pageYOffset;
    var anchorApplied = false;
    if (scrollMode === 'preserve' && Math.abs(currentScrollY - scrollY) <= 1) {
      window.scrollTo(0, scrollY);
    } else if (scrollMode === 'reset') {
      window.scrollTo(0, 0);
    }
    if (options.anchorHash) {
      anchorApplied = applyContentAnchorNavigation(options.anchorHash, true);
      if (!anchorApplied) {
        console.warn('[markdown-view] リンク先の見出しが見つかりません:', options.anchorHash);
        window.scrollTo(0, 0);
        // 既定はhash除去。popstate経路のみ明示的にfalseを渡して、
        // ユーザーが戻る/進むで辿れるはずの履歴エントリURLの破壊を避ける。
        if (options.clearHashOnMiss !== false) {
          setLocationHash('', true);
        }
        clearPendingTocNavigation();
      }
    }
    updateReadingProgress();
    restoreActiveTocHeading(preservedActiveTocId);
  });

  updateDocumentStats();
  syncDocumentChrome(appContext.state.currentFile);
  enhanceContentInteractions();
  syncDocumentSearchAfterContentUpdate(options);
  setupTocFilter();
  hideQuoteSelectionAction();
  if (!validation.hasContractViolation && appContext.websocket) {
    appContext.websocket.rememberAppliedLiveUpdate(safeData);
  }
};
