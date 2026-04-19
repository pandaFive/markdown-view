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

function updateLocationHash(url, hash) {
  if (hash === undefined) return;
  if (!hash) {
    url.hash = '';
    return;
  }
  url.hash = hash.charAt(0) === '#' ? hash : '#' + hash;
}

function setLocationHash(hash, replace) {
  var url = new URL(location.href);
  updateLocationHash(url, hash || '');
  if (replace) {
    history.replaceState(null, '', url.toString());
  } else {
    history.pushState(null, '', url.toString());
  }
}

function isModifiedClick(event) {
  return event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey;
}

function isExternalSchemeHref(href) {
  return /^[a-zA-Z][a-zA-Z\d+.-]*:/.test(href);
}

/// `?file=foo.md#hash` 形式や単一ファイルモードの同一path+hash形式を解決する。
/// メモプレビュー内の出典リンクは memo.js の buildQuoteSource() がこの形式で生成する。
/// resolveMarkdownLinkTarget は `?` 開始 href を拒否するため、こちらで補完する。
function resolveFileQueryHref(href) {
  if (!href) return null;
  var url;
  try {
    url = new URL(href, location.href);
  } catch (error) {
    return null;
  }
  if (url.origin !== location.origin) return null;
  if (url.pathname !== location.pathname) return null;
  if (!url.hash) return null;

  if (isDirMode) {
    var fileParam = url.searchParams.get('file');
    if (fileParam && /\.md$/i.test(fileParam)) {
      return { file: fileParam, hash: url.hash };
    }
    return null;
  }
  // 単一ファイルモード: 同一path+hash形式のリンクは現在ファイル内ジャンプとして扱う
  if (currentFile) {
    return { file: currentFile, hash: url.hash };
  }
  return null;
}

function resolveMarkdownLinkTarget(href) {
  if (!isDirMode || !href || href.startsWith('#') || href.startsWith('/') || href.startsWith('?')) {
    return null;
  }
  if (href.startsWith('//') || isExternalSchemeHref(href)) {
    return null;
  }

  var currentDir = '';
  var baseUrl;
  var resolvedUrl;
  var relativePath;

  if (currentFile && currentFile.indexOf('/') !== -1) {
    currentDir = currentFile.slice(0, currentFile.lastIndexOf('/') + 1);
  }

  try {
    baseUrl = new URL(currentDir, 'https://markdown-view.local/');
    resolvedUrl = new URL(href, baseUrl);
  } catch (error) {
    console.warn('[markdown-view] Markdownリンク解決に失敗:', error);
    return null;
  }

  relativePath = resolvedUrl.pathname.replace(/^\/+/, '');
  try {
    relativePath = decodeURIComponent(relativePath);
  } catch (error) {
    console.warn('[markdown-view] リンクパスのデコードに失敗:', relativePath, error);
  }

  if (!/\.md$/i.test(relativePath)) {
    return null;
  }

  return {
    file: relativePath,
    hash: resolvedUrl.hash || ''
  };
}

function parseLineHash(hash) {
  var empty = { headingId: null, lineRange: null };
  if (!hash || hash.charAt(0) !== '#') return empty;
  var raw = hash.slice(1);
  var decoded;
  try {
    decoded = decodeURIComponent(raw);
  } catch (error) {
    decoded = raw;
  }
  if (!decoded) return empty;

  // `heading-id:L5` または `heading-id:L5-L7`
  // `(.*)` は greedy だが、現状 slugify は `:` を除去するため heading_id に `:` は含まれない。
  // slugify 仕様が変わる場合はここの分割戦略を見直すこと
  var combined = decoded.match(/^(.*):L(\d+)(?:-L(\d+))?$/);
  if (combined) {
    var start = parseInt(combined[2], 10);
    var end = combined[3] ? parseInt(combined[3], 10) : start;
    return {
      headingId: combined[1] || null,
      lineRange: { start: start, end: end }
    };
  }

  // `L5` または `L5-L7` 単独
  var lineOnly = decoded.match(/^L(\d+)(?:-L(\d+))?$/);
  if (lineOnly) {
    var s = parseInt(lineOnly[1], 10);
    var e = lineOnly[2] ? parseInt(lineOnly[2], 10) : s;
    return { headingId: null, lineRange: { start: s, end: e } };
  }

  return { headingId: decoded, lineRange: null };
}

/// 旧形式メモ互換: リンク直後のテキストに `L5` / `L5-L7` が並ぶ場合、
/// その行範囲を既存 hash に `:L5-L7` として合成して返す。
/// 新形式（href fragment 内に `:L5-L7`）や行範囲情報が無い場合は hash をそのまま返す。
/// renderer が text をソース行トラッキング用 `<span>` でラップするケースにも対応するため、
/// TEXT_NODE と ELEMENT_NODE の双方で `textContent` を見る。
function augmentHashWithTrailingLineHint(link, hash) {
  if (!link) return hash;
  var sibling = link.nextSibling;
  if (!sibling) return hash;
  if (sibling.nodeType !== Node.TEXT_NODE && sibling.nodeType !== Node.ELEMENT_NODE) return hash;
  if (parseLineHash(hash).lineRange) return hash;
  // 否定先読みで `L123abc` のような別トークンへの誤マッチを防ぐ
  var match = (sibling.textContent || '').match(/^\s*L(\d+)(?:-L(\d+))?(?![\w])/);
  if (!match) return hash;
  var start = parseInt(match[1], 10);
  var end = match[2] ? parseInt(match[2], 10) : start;
  // 逆転範囲は start のみ採用
  var suffix = end > start ? 'L' + start + '-L' + end : 'L' + start;
  if (!hash || hash === '#') return '#' + suffix;
  return hash + ':' + suffix;
}

function scrollToLineRange(targetLine, behavior) {
  // 行番号は renderer 側で 1-indexed。0 以下や非数値は無効として早期return
  if (!contentRoot || typeof targetLine !== 'number' || targetLine < 1) return false;
  var blocks = contentRoot.querySelectorAll('[data-line-block]');
  // 候補から「最狭マッチ（最深containment）」を選ぶ。
  // <ul>(L5-L20) と <li>(L7-L7) が共に line 7 を含むとき、<li> を選ばないと
  // コンテナ先頭にスクロールしてしまうため (PR #73 codex-bot レビュー指摘)
  // 同値スパン（ネストblockquote内の単独<p>など）では `<=` 比較で DOM 深い側を優先する
  var best = null;
  var bestSpan = Infinity;
  for (var i = 0; i < blocks.length; i++) {
    var block = blocks[i];
    // block コンテナは data-line-block-start/end、heading/code-block は data-source-* から範囲を読む
    var startAttr = block.getAttribute('data-line-block-start');
    if (startAttr === null) startAttr = block.getAttribute('data-source-start-line');
    var endAttr = block.getAttribute('data-line-block-end');
    if (endAttr === null) endAttr = block.getAttribute('data-source-end-line');
    if (startAttr === null || endAttr === null) continue;
    var s = parseInt(startAttr, 10);
    var e = parseInt(endAttr, 10);
    if (isNaN(s) || isNaN(e)) continue;
    if (s <= targetLine && e >= targetLine) {
      var span = e - s;
      if (span <= bestSpan) {
        bestSpan = span;
        best = block;
      }
    }
  }
  if (!best) return false;
  best.scrollIntoView({ block: 'start', behavior: behavior || 'auto' });
  triggerJumpHighlight(best);
  return true;
}

function triggerJumpHighlight(el) {
  if (!el) return;
  el.classList.remove('jump-highlight');
  // CSS animationを再起動するための強制reflow（class再付与前にlayoutをflushする定番技法）
  void el.offsetWidth;
  el.classList.add('jump-highlight');
  // { once: true } でリスナー自動除去。連続クリック時の leak を防ぐ
  el.addEventListener('animationend', function() {
    el.classList.remove('jump-highlight');
  }, { once: true });
}

function applyContentAnchorNavigation(hash, replace) {
  if (!hash || hash.charAt(0) !== '#') return false;

  var parsed = parseLineHash(hash);
  // ユーザクリック由来 (replace=false) は smooth、履歴復元 (replace=true) は auto で即着地
  var scrollBehavior = replace ? 'auto' : 'smooth';

  // 行範囲があれば優先（より詳細な位置へジャンプ）
  if (parsed.lineRange && scrollToLineRange(parsed.lineRange.start, scrollBehavior)) {
    if (parsed.headingId && typeof markPendingTocNavigation === 'function') {
      markPendingTocNavigation(parsed.headingId);
    }
    setLocationHash(hash, replace);
    return true;
  }

  // 見出しIDへのフォールバックジャンプ
  if (parsed.headingId) {
    var targetEl = document.getElementById(parsed.headingId);
    if (!targetEl) return false;
    if (typeof markPendingTocNavigation === 'function') {
      markPendingTocNavigation(parsed.headingId);
    }
    setLocationHash(hash, replace);
    targetEl.scrollIntoView({ block: 'start', behavior: scrollBehavior });
    return true;
  }

  return false;
}

function restoreContentNavigationFromLocation() {
  var hash = location.hash || '';

  requestAnimationFrame(function() {
    if (hash && applyContentAnchorNavigation(hash, true)) {
      return;
    }
    if (hash) {
      console.warn('[markdown-view] 履歴復元時に見出しが見つかりません:', hash);
    }

    window.scrollTo(0, 0);
    if (typeof clearPendingTocNavigation === 'function') {
      clearPendingTocNavigation();
    }
    if (typeof restoreActiveTocHeading === 'function') {
      restoreActiveTocHeading('');
    }
  });
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

/// 内部リンク（相対 .md / `?file=foo.md#hash` / 同一path+hash）のクリックを処理する共通ハンドラ。
/// `#content` と `#memo-preview` の両方からの delegation で使う。
function handleInternalLinkClick(event) {
  var link = event.target.closest('a[href]');
  if (!link || isModifiedClick(event)) return;
  if (link.hasAttribute('download') || (link.target && link.target !== '_self')) return;

  var href = link.getAttribute('href') || '';
  // 既存relative resolverを優先、ヒットしなければ `?file=` / 同一path系で再試行
  var target = resolveMarkdownLinkTarget(href) || resolveFileQueryHref(href);
  if (!target) return;

  // 旧形式メモ互換（リンク外 `L5-L7` を hash fragment に取り込む）
  target.hash = augmentHashWithTrailingLineHint(link, target.hash);

  if (target.file === currentFile) {
    event.preventDefault();
    if (target.hash) {
      if (applyContentAnchorNavigation(target.hash, false)) {
        return;
      }
      console.warn('[markdown-view] 同一ファイル内のジャンプ先が見つかりません:', target.hash);
    }
    setFileParam(currentFile, false, '');
    restoreContentNavigationFromLocation();
    return;
  }

  event.preventDefault();

  selectFile(target.file, true, {
    scrollMode: target.hash ? 'none' : 'reset',
    anchorHash: target.hash,
    historyHash: target.hash || ''
  });
}

function setupContentLinkNavigation() {
  if (!contentRoot) return;
  contentRoot.addEventListener('click', handleInternalLinkClick);
}

function setupMemoLinkNavigation() {
  if (!memoPreviewEl) return;
  memoPreviewEl.addEventListener('click', handleInternalLinkClick);
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

function createDocumentSearchEmptyState(message) {
  var empty = document.createElement('p');
  empty.className = 'document-search-empty';
  empty.textContent = message;
  return empty;
}

function formatDirectorySearchSummary() {
  var baseText;
  if (!currentDocumentSearchQuery) {
    baseText = '0 件';
  } else if (currentDirectorySearchLoading) {
    baseText = '検索中...';
  } else if (currentDirectorySearchError) {
    baseText = 'エラー';
  } else if (!currentDirectorySearchResults.length) {
    baseText = '0 件';
  } else if (currentDirectorySearchIndex >= 0) {
    baseText = (currentDirectorySearchIndex + 1) + ' / ' + currentDirectorySearchResults.length + ' 件';
  } else {
    baseText = '0 / ' + currentDirectorySearchResults.length + ' 件';
  }

  if (currentDirectorySearchSkippedFiles > 0) {
    return baseText + '（' + currentDirectorySearchSkippedFiles + '件スキップ）';
  }
  return baseText;
}

function updateDocumentSearchSummary() {
  if (!documentSearchSummaryEl) return;
  if (isDirMode) {
    documentSearchSummaryEl.textContent = formatDirectorySearchSummary();
    return;
  }
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
  if (isDirMode) {
    renderDirectorySearchResults();
    return;
  }
  var preservedScrollTop = documentSearchResultsEl.scrollTop;
  documentSearchResultsEl.innerHTML = '';

  if (!currentDocumentSearchQuery) return;

  if (!documentSearchMatches.length) {
    documentSearchResultsEl.appendChild(createDocumentSearchEmptyState('一致する文が見つかりません。'));
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

function renderDirectorySearchResults() {
  var preservedScrollTop = documentSearchResultsEl.scrollTop;
  documentSearchResultsEl.innerHTML = '';

  if (!currentDocumentSearchQuery) return;

  if (currentDirectorySearchLoading) {
    documentSearchResultsEl.appendChild(createDocumentSearchEmptyState('ディレクトリを検索しています。'));
    return;
  }

  if (currentDirectorySearchError) {
    documentSearchResultsEl.appendChild(createDocumentSearchEmptyState(currentDirectorySearchError));
    return;
  }

  if (!currentDirectorySearchResults.length) {
    documentSearchResultsEl.appendChild(createDocumentSearchEmptyState('ディレクトリ内に一致が見つかりません。'));
    return;
  }

  currentDirectorySearchResults.forEach(function(result, index) {
    var button = document.createElement('button');
    var indexBadge = document.createElement('span');
    var body = document.createElement('span');
    var path = document.createElement('span');

    button.type = 'button';
    button.className = 'document-search-result';
    button.dataset.resultIndex = String(index);
    button.classList.toggle('active', index === currentDirectorySearchIndex);
    button.setAttribute('aria-current', index === currentDirectorySearchIndex ? 'true' : 'false');
    button.addEventListener('click', function() {
      openDirectorySearchResult(index);
    });

    indexBadge.className = 'document-search-result-index';
    indexBadge.textContent = String(index + 1).padStart(2, '0');

    body.className = 'document-search-result-body';
    path.className = 'document-search-result-path';
    path.textContent = result.file;
    body.appendChild(path);
    renderDocumentSearchResultContext(body, result.before, currentDocumentSearchQuery, 'before');
    renderDocumentSearchResultContext(body, result.current, currentDocumentSearchQuery, 'current');
    renderDocumentSearchResultContext(body, result.after, currentDocumentSearchQuery, 'after');

    button.appendChild(indexBadge);
    button.appendChild(body);
    documentSearchResultsEl.appendChild(button);
  });

  documentSearchResultsEl.scrollTop = preservedScrollTop;
}

function renderDirectorySearchUi() {
  updateDocumentSearchSummary();
  renderDocumentSearchResults();
}

function applyPendingDirectorySearchNavigation() {
  if (!isDirMode || !pendingDirectorySearchNavigation) return;
  if (pendingDirectorySearchNavigation.file !== currentFile) return;
  if (pendingDirectorySearchNavigation.query !== currentDocumentSearchQuery) {
    pendingDirectorySearchNavigation = null;
    return;
  }
  if (documentSearchMatches.length) {
    setCurrentDocumentSearchMatch(
      Math.min(pendingDirectorySearchNavigation.fileMatchIndex, documentSearchMatches.length - 1)
    );
  }
  currentDirectorySearchIndex = pendingDirectorySearchNavigation.resultIndex;
  pendingDirectorySearchNavigation = null;
}

function scheduleDirectorySearch(query) {
  if (documentSearchDebounceTimer) {
    clearTimeout(documentSearchDebounceTimer);
  }
  documentSearchDebounceTimer = setTimeout(function() {
    documentSearchDebounceTimer = null;
    runDirectorySearch(query);
  }, 300);
}

function getPreferredDirectorySearchSelection() {
  if (pendingDirectorySearchNavigation) {
    return {
      file: pendingDirectorySearchNavigation.file,
      fileMatchIndex: pendingDirectorySearchNavigation.fileMatchIndex
    };
  }
  if (
    currentDirectorySearchIndex >= 0 &&
    currentDirectorySearchIndex < currentDirectorySearchResults.length
  ) {
    return {
      file: currentDirectorySearchResults[currentDirectorySearchIndex].file,
      fileMatchIndex: currentDirectorySearchResults[currentDirectorySearchIndex].file_match_index
    };
  }
  if (
    currentFile &&
    currentDocumentSearchIndex >= 0 &&
    currentDocumentSearchIndex < documentSearchMatches.length
  ) {
    return {
      file: currentFile,
      fileMatchIndex: currentDocumentSearchIndex
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
    currentFile &&
    currentDocumentSearchIndex >= 0 &&
    currentDocumentSearchIndex < documentSearchMatches.length
  ) {
    index = results.findIndex(function(result) {
      return (
        result.file === currentFile &&
        result.file_match_index === currentDocumentSearchIndex
      );
    });
    if (index !== -1) return index;
  }
  return -1;
}

function runDirectorySearch(query) {
  var generation = ++documentSearchFetchGeneration;
  var preferredSelection = getPreferredDirectorySearchSelection();
  currentDirectorySearchLoading = true;
  currentDirectorySearchError = '';
  currentDirectorySearchResults = [];
  currentDirectorySearchIndex = -1;
  currentDirectorySearchSkippedFiles = 0;
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
    if (generation !== documentSearchFetchGeneration) return;
    if ((data.query || '') !== currentDocumentSearchQuery) return;
    currentDirectorySearchLoading = false;
    currentDirectorySearchError = '';
    currentDirectorySearchResults = Array.isArray(data.results) ? data.results : [];
    currentDirectorySearchSkippedFiles = Number(data.skipped_files || 0);
    currentDirectorySearchIndex = resolveDirectorySearchIndex(
      currentDirectorySearchResults,
      preferredSelection
    );
    renderDirectorySearchUi();
  })
  .catch(function(err) {
    if (generation !== documentSearchFetchGeneration) return;
    if (query !== currentDocumentSearchQuery) return;
    currentDirectorySearchLoading = false;
    currentDirectorySearchResults = [];
    currentDirectorySearchIndex = -1;
    currentDirectorySearchSkippedFiles = 0;
    currentDirectorySearchError = getFileFetchErrorMessage(err);
    console.error('[markdown-view] ディレクトリ検索エラー:', err);
    renderDirectorySearchUi();
  });
}

function openDirectorySearchResult(index) {
  if (!currentDirectorySearchResults.length) return;
  var normalizedIndex = (index + currentDirectorySearchResults.length) % currentDirectorySearchResults.length;
  var result = currentDirectorySearchResults[normalizedIndex];
  var previousResultIndex = currentDirectorySearchIndex;
  currentDirectorySearchIndex = normalizedIndex;
  pendingDirectorySearchNavigation = {
    file: result.file,
    query: currentDocumentSearchQuery,
    fileMatchIndex: result.file_match_index,
    resultIndex: normalizedIndex,
    previousResultIndex: previousResultIndex
  };
  renderDirectorySearchUi();

  if (result.file === currentFile) {
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
  if (isDirMode) {
    if (!currentDirectorySearchResults.length) return;
    if (currentDirectorySearchIndex < 0) {
      openDirectorySearchResult(step > 0 ? 0 : currentDirectorySearchResults.length - 1);
      return;
    }
    openDirectorySearchResult(currentDirectorySearchIndex + step);
    return;
  }
  if (!documentSearchMatches.length) return;
  setCurrentDocumentSearchMatch(currentDocumentSearchIndex + step);
}

function applyDocumentSearchQuery(query) {
  currentDocumentSearchQuery = (query || '').trim();
  if (isDirMode) {
    applyDocumentSearchHighlights(currentDocumentSearchQuery);
    pendingDirectorySearchNavigation = null;
    if (!currentDocumentSearchQuery) {
      if (documentSearchDebounceTimer) {
        clearTimeout(documentSearchDebounceTimer);
        documentSearchDebounceTimer = null;
      }
      documentSearchFetchGeneration += 1;
      currentDirectorySearchResults = [];
      currentDirectorySearchIndex = -1;
      currentDirectorySearchSkippedFiles = 0;
      currentDirectorySearchLoading = false;
      currentDirectorySearchError = '';
      renderDirectorySearchUi();
      return;
    }
    currentDirectorySearchResults = [];
    currentDirectorySearchIndex = -1;
    currentDirectorySearchSkippedFiles = 0;
    currentDirectorySearchLoading = true;
    currentDirectorySearchError = '';
    scheduleDirectorySearch(currentDocumentSearchQuery);
    renderDirectorySearchUi();
    return;
  }
  applyDocumentSearchHighlights(currentDocumentSearchQuery);
}

function clearDocumentSearchQuery() {
  if (documentSearchInputEl) {
    documentSearchInputEl.value = '';
  }
  if (documentSearchDebounceTimer) {
    clearTimeout(documentSearchDebounceTimer);
    documentSearchDebounceTimer = null;
  }
  documentSearchFetchGeneration += 1;
  currentDocumentSearchQuery = '';
  pendingDirectorySearchNavigation = null;
  currentDirectorySearchResults = [];
  currentDirectorySearchIndex = -1;
  currentDirectorySearchSkippedFiles = 0;
  currentDirectorySearchLoading = false;
  currentDirectorySearchError = '';
  clearDocumentSearchHighlights();
  if (isDirMode) {
    renderDirectorySearchUi();
  }
}

function syncDocumentSearchAfterContentUpdate(options) {
  options = options || {};
  if (!documentSearchInputEl) return;
  if (isDirMode) {
    currentDocumentSearchQuery = (documentSearchInputEl.value || '').trim();
    applyDocumentSearchHighlights(currentDocumentSearchQuery);
    applyPendingDirectorySearchNavigation();
    if (currentDocumentSearchQuery && options.requeryDirectorySearch !== false) {
      scheduleDirectorySearch(currentDocumentSearchQuery);
    }
    renderDirectorySearchUi();
    return;
  }
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
function updateContent(data, options) {
  options = options || {};
  if (pendingUpdateTimer) {
    clearTimeout(pendingUpdateTimer);
    pendingUpdateTimer = null;
  }
  pendingUpdate = null;
  var scrollY = window.scrollY;
  var scrollMode = options.scrollMode || 'preserve';
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
        if (typeof clearPendingTocNavigation === 'function') {
          clearPendingTocNavigation();
        }
      }
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
    syncDocumentSearchAfterContentUpdate(options);
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
setupContentLinkNavigation();
setupMemoLinkNavigation();
