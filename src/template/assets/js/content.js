function setLiveStatus(state) {
  if (!liveStatusEl) return;
  liveStatusEl.textContent = LIVE_STATUS_LABELS[state] || state;
  liveStatusEl.dataset.state = state;
}

function updateDocumentStats() {
  if (!contentRoot) return;
  var headings = contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6').length;
  var text = (contentRoot.textContent || '').replace(/\s+/g, '');
  if (docHeadingCountEl) {
    docHeadingCountEl.textContent = '見出し ' + headings;
  }
  if (docCharCountEl) {
    docCharCountEl.textContent = '文字 ' + text.length;
  }
}

function updateReadingProgress() {
  var scrollTop = window.scrollY || window.pageYOffset;
  var maxScroll = Math.max(document.documentElement.scrollHeight - window.innerHeight, 1);
  var progress = Math.min(100, Math.max(0, (scrollTop / maxScroll) * 100));
  if (readingProgressBar) {
    readingProgressBar.style.width = progress + '%';
  }
  if (backToTop) {
    backToTop.classList.toggle('visible', scrollTop > 360);
  }
}

function syncDocumentChrome(file) {
  var title = file ? file.split('/').pop() : (contentRoot ? contentRoot.getAttribute('data-title') : '');
  if (!title) title = 'markdown-view';
  if (documentTitleEl) {
    documentTitleEl.textContent = title;
  }
  document.title = title + ' - markdown-view';
}

function copyText(text) {
  if (navigator.clipboard && navigator.clipboard.writeText) {
    return navigator.clipboard.writeText(text);
  }
  return new Promise(function(resolve, reject) {
    try {
      var input = document.createElement('textarea');
      input.value = text;
      input.setAttribute('readonly', 'readonly');
      input.style.position = 'fixed';
      input.style.opacity = '0';
      document.body.appendChild(input);
      input.select();
      var success = document.execCommand('copy');
      input.remove();
      if (success) {
        resolve();
      } else {
        reject(new Error('execCommand("copy") returned false'));
      }
    } catch (error) {
      reject(error);
    }
  });
}

function flashCopiedState(button, copiedLabel, baseLabel) {
  if (!button) return;
  button.classList.add('copied');
  button.textContent = copiedLabel;
  setTimeout(function() {
    button.classList.remove('copied');
    button.textContent = baseLabel;
  }, 1200);
}

function handleCopyClick(button, text, baseLabel) {
  copyText(text).then(function() {
    flashCopiedState(button, 'Copied', baseLabel);
  }).catch(function(err) {
    console.warn('[markdown-view] コピーに失敗:', err);
    flashCopiedState(button, 'Failed', baseLabel);
  });
}

function enhanceContentInteractions() {
  if (!contentRoot) return;

  var headings = contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6');
  headings.forEach(function(heading) {
    if (!heading.id || heading.querySelector('.heading-anchor')) return;
    var button = document.createElement('button');
    button.type = 'button';
    button.className = 'heading-anchor';
    button.textContent = '#';
    button.setAttribute('aria-label', '見出しリンクをコピー');
    button.addEventListener('click', function() {
      var url = new URL(location.href);
      url.hash = heading.id;
      handleCopyClick(button, url.toString(), '#');
    });
    heading.appendChild(button);
  });

  var blocks = contentRoot.querySelectorAll('pre.code-block');
  blocks.forEach(function(block) {
    if (block.querySelector('.code-copy')) return;
    var code = block.querySelector('code');
    if (!code) return;
    var button = document.createElement('button');
    button.type = 'button';
    button.className = 'code-copy';
    button.textContent = 'Copy';
    button.setAttribute('aria-label', 'コードをコピー');
    button.addEventListener('click', function() {
      handleCopyClick(button, code.innerText || code.textContent || '', 'Copy');
    });
    block.appendChild(button);
  });
}

function setupFilterableList(options) {
  var input = document.getElementById(options.inputId);
  var root = document.getElementById(options.rootId);
  if (!input || !root) return;

  var items = options.getItems(root);
  var applyFilter = function() {
    var query = input.value.trim().toLowerCase();
    options.apply(items, query, input);
  };

  input.addEventListener('input', applyFilter);
  applyFilter();
}

function setupTocFilter() {
  setupFilterableList({
    inputId: 'toc-filter',
    rootId: 'toc',
    getItems: function(root) {
      return root.querySelectorAll('li');
    },
    apply: function(items, query) {
      items.forEach(function(item) {
        var link = item.querySelector(':scope > a');
        if (!link) return;
        var matched = !query || link.textContent.toLowerCase().indexOf(query) !== -1;
        item.hidden = !matched;
      });
    }
  });
}

function updateDocumentSearchSummary() {
  if (!documentSearchSummaryEl) return;
  if (!documentSearchMatches.length) {
    documentSearchSummaryEl.textContent = '0 件';
    return;
  }
  documentSearchSummaryEl.textContent = (currentDocumentSearchIndex + 1) + ' / ' + documentSearchMatches.length + ' 件';
}

function clearDocumentSearchHighlights() {
  if (!contentRoot) return;
  contentRoot.querySelectorAll('mark.document-search-match').forEach(function(mark) {
    var parent = mark.parentNode;
    if (!parent) return;
    parent.replaceChild(document.createTextNode(mark.textContent || ''), mark);
    parent.normalize();
  });
  documentSearchMatches = [];
  currentDocumentSearchIndex = -1;
  updateDocumentSearchSummary();
  renderDocumentSearchResults();
}

var DOCUMENT_SEARCH_BLOCK_SELECTOR = 'p, li, blockquote, th, td, h1, h2, h3, h4, h5, h6';

function shouldSkipDocumentSearchNode(node) {
  var parent = node.parentElement;
  if (!parent) return true;
  return Boolean(parent.closest(
    'button, input, textarea, script, style, pre.code-block, mark.document-search-match'
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
  if (!contentRoot) return [];
  return Array.prototype.filter.call(
    contentRoot.querySelectorAll(DOCUMENT_SEARCH_BLOCK_SELECTOR),
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
  if (!documentSearchResultsEl) return;
  var preservedScrollTop = documentSearchResultsEl.scrollTop;
  documentSearchResultsEl.innerHTML = '';

  if (!currentDocumentSearchQuery) return;

  if (!documentSearchMatches.length) {
    var empty = document.createElement('p');
    empty.className = 'document-search-empty';
    empty.textContent = '一致する文が見つかりません。';
    documentSearchResultsEl.appendChild(empty);
    return;
  }

  documentSearchMatches.forEach(function(match, index) {
    var button = document.createElement('button');
    var indexBadge = document.createElement('span');
    var body = document.createElement('span');

    button.type = 'button';
    button.className = 'document-search-result';
    button.dataset.matchIndex = String(index);
    button.classList.toggle('active', index === currentDocumentSearchIndex);
    button.setAttribute('aria-current', index === currentDocumentSearchIndex ? 'true' : 'false');
    button.addEventListener('click', function() {
      setCurrentDocumentSearchMatch(index);
    });

    indexBadge.className = 'document-search-result-index';
    indexBadge.textContent = String(index + 1).padStart(2, '0');

    body.className = 'document-search-result-body';
    renderDocumentSearchResultContext(body, match.context.before, currentDocumentSearchQuery, 'before');
    renderDocumentSearchResultContext(body, match.context.current, currentDocumentSearchQuery, 'current');
    renderDocumentSearchResultContext(body, match.context.after, currentDocumentSearchQuery, 'after');

    button.appendChild(indexBadge);
    button.appendChild(body);
    documentSearchResultsEl.appendChild(button);
  });

  documentSearchResultsEl.scrollTop = preservedScrollTop;
}

function applyDocumentSearchHighlights(query) {
  if (!contentRoot) return;
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
        documentSearchMatches.length
      );
      if (marks.length) {
        documentSearchMatches.push({
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

  if (documentSearchMatches.length) {
    setCurrentDocumentSearchMatch(0, false);
  } else {
    updateDocumentSearchSummary();
    renderDocumentSearchResults();
  }
}

function setCurrentDocumentSearchMatch(index, scrollIntoView) {
  if (!documentSearchMatches.length) {
    currentDocumentSearchIndex = -1;
    updateDocumentSearchSummary();
    return;
  }
  if (currentDocumentSearchIndex >= 0 && documentSearchMatches[currentDocumentSearchIndex]) {
    documentSearchMatches[currentDocumentSearchIndex].marks.forEach(function(mark) {
      mark.classList.remove('current');
    });
  }
  currentDocumentSearchIndex = (index + documentSearchMatches.length) % documentSearchMatches.length;
  documentSearchMatches[currentDocumentSearchIndex].marks.forEach(function(mark) {
    mark.classList.add('current');
  });
  if (scrollIntoView !== false) {
    documentSearchMatches[currentDocumentSearchIndex].marks[0].scrollIntoView({
      block: 'center',
      behavior: 'smooth'
    });
  }
  updateDocumentSearchSummary();
  renderDocumentSearchResults();
}

function moveDocumentSearch(step) {
  if (!documentSearchMatches.length) return;
  setCurrentDocumentSearchMatch(currentDocumentSearchIndex + step);
}

function applyDocumentSearchQuery(query) {
  currentDocumentSearchQuery = (query || '').trim();
  applyDocumentSearchHighlights(currentDocumentSearchQuery);
}

function clearDocumentSearchQuery() {
  if (documentSearchInputEl) {
    documentSearchInputEl.value = '';
  }
  currentDocumentSearchQuery = '';
  clearDocumentSearchHighlights();
}

function syncDocumentSearchAfterContentUpdate() {
  if (!documentSearchInputEl) return;
  applyDocumentSearchQuery(documentSearchInputEl.value);
}

function openDocumentSearch() {
  if (typeof activateSidebarTab === 'function') {
    activateSidebarTab('toc');
  }
  var sidebarEl = document.getElementById('sidebar');
  if (sidebarEl) {
    sidebarEl.classList.add('open');
  }
  if (documentSearchInputEl) {
    documentSearchInputEl.focus();
    documentSearchInputEl.select();
  }
}

function setupDocumentSearch() {
  if (!documentSearchInputEl) return;

  documentSearchInputEl.addEventListener('input', function() {
    applyDocumentSearchQuery(documentSearchInputEl.value);
  });

  documentSearchInputEl.addEventListener('keydown', function(event) {
    if (event.key === 'Enter') {
      event.preventDefault();
      moveDocumentSearch(event.shiftKey ? -1 : 1);
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      if (documentSearchInputEl.value) {
        clearDocumentSearchQuery();
      } else {
        documentSearchInputEl.blur();
      }
    }
  });

  if (documentSearchPrevEl) {
    documentSearchPrevEl.addEventListener('click', function() {
      moveDocumentSearch(-1);
    });
  }
  if (documentSearchNextEl) {
    documentSearchNextEl.addEventListener('click', function() {
      moveDocumentSearch(1);
    });
  }
  if (documentSearchClearEl) {
    documentSearchClearEl.addEventListener('click', function() {
      clearDocumentSearchQuery();
      documentSearchInputEl.focus();
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
  if (!pendingUpdate) return;
  if (pendingUpdateTimer) {
    clearTimeout(pendingUpdateTimer);
    pendingUpdateTimer = null;
  }
  if (pendingUpdate.refresh) {
    var refreshFile = pendingUpdate.file;
    pendingUpdate = null;
    if (isDirMode && refreshFile) {
      selectFile(refreshFile, false);
    }
    return;
  }
  if (isDirMode && pendingUpdate.file && pendingUpdate.file !== currentFile) {
    pendingUpdate = null;
    return;
  }
  var data = pendingUpdate;
  pendingUpdate = null;
  updateContent(data);
  hideWsServerErrorBanner();
  hideFileFetchErrorBanner();
  setLiveStatus('live');
}

function normalizeTocHtml(html) {
  return (html || '').replace(/>\s+</g, '><').trim();
}

// サーバーサイドでサニタイズ済みのHTMLを反映する
// XSS防止: pulldown-cmarkでraw HTML無効化済み（renderer.rs参照）
function updateContent(data) {
  if (pendingUpdateTimer) {
    clearTimeout(pendingUpdateTimer);
    pendingUpdateTimer = null;
  }
  pendingUpdate = null;
  var scrollY = window.scrollY;
  var preservedActiveTocId = typeof getCurrentActiveTocId === 'function' ? getCurrentActiveTocId() : '';
  var contentEl = document.getElementById('content');
  var tocEl = document.getElementById('toc');

  if (data.content !== undefined && contentEl.innerHTML !== data.content) {
    contentEl.innerHTML = data.content;
  }
  if (data.toc !== undefined && normalizeTocHtml(tocEl.innerHTML) !== normalizeTocHtml(data.toc)) {
    tocEl.innerHTML = data.toc;
  }

  if (typeof setupTocTracking === 'function') {
    setupTocTracking();
  }
  if (typeof suppressTocTrackingFor === 'function') {
    suppressTocTrackingFor(120);
  }

  requestAnimationFrame(function() {
    var currentScrollY = window.scrollY || window.pageYOffset;
    if (Math.abs(currentScrollY - scrollY) <= 1) {
      window.scrollTo(0, scrollY);
    }
    updateReadingProgress();
    if (typeof restoreActiveTocHeading === 'function') {
      restoreActiveTocHeading(preservedActiveTocId);
    }
  });

  updateDocumentStats();
  syncDocumentChrome(currentFile);
  enhanceContentInteractions();
  if (typeof syncDocumentSearchAfterContentUpdate === 'function') {
    syncDocumentSearchAfterContentUpdate();
  }
  setupTocFilter();
  if (typeof hideQuoteSelectionAction === 'function') {
    hideQuoteSelectionAction();
  }
  if (typeof rememberAppliedLiveUpdate === 'function') {
    rememberAppliedLiveUpdate(data);
  }
}

setupDocumentSearch();
