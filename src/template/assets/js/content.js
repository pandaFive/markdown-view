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

  if (marks.length) {
    documentSearchMatches.push({ marks: marks });
  }
}

function applyDocumentSearchHighlights(query) {
  if (!contentRoot) return;
  clearDocumentSearchHighlights();
  if (!query) return;

  var normalizedQuery = query.toLowerCase();

  getDocumentSearchBlocks().forEach(function(block) {
    var blockText = collectDocumentSearchTextNodes(block);
    var matchIndex;
    var searchIndex = 0;

    if (!blockText.text) return;

    matchIndex = blockText.text.toLowerCase().indexOf(normalizedQuery, searchIndex);
    while (matchIndex !== -1) {
      wrapDocumentSearchMatch(
        blockText.nodes,
        matchIndex,
        matchIndex + normalizedQuery.length,
        documentSearchMatches.length
      );
      searchIndex = matchIndex + normalizedQuery.length;
      matchIndex = blockText.text.toLowerCase().indexOf(normalizedQuery, searchIndex);
    }
  });

  if (documentSearchMatches.length) {
    setCurrentDocumentSearchMatch(0, false);
  } else {
    updateDocumentSearchSummary();
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
}

function moveDocumentSearch(step) {
  if (!documentSearchMatches.length) return;
  setCurrentDocumentSearchMatch(currentDocumentSearchIndex + step);
}

function applyDocumentSearchQuery(query) {
  applyDocumentSearchHighlights((query || '').trim());
}

function clearDocumentSearchQuery() {
  if (documentSearchInputEl) {
    documentSearchInputEl.value = '';
  }
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
    window.scrollTo(0, scrollY);
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
