function setLiveStatus(state) {
  if (!appContext.elements.liveStatusEl) return;
  appContext.elements.liveStatusEl.textContent = appContext.labels.liveStatus[state] || state;
  appContext.elements.liveStatusEl.dataset.state = state;
  if (state === 'live') {
    clearMemoSyncPendingStatus();
  }
}

function updateDocumentStats() {
  if (!appContext.elements.contentRoot) return;
  var headings = appContext.elements.contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6').length;
  var text = (appContext.elements.contentRoot.textContent || '').replace(/\s+/g, '');
  if (appContext.elements.docHeadingCountEl) {
    appContext.elements.docHeadingCountEl.textContent = '見出し ' + headings;
  }
  if (appContext.elements.docCharCountEl) {
    appContext.elements.docCharCountEl.textContent = '文字 ' + text.length;
  }
}

function updateReadingProgress() {
  var scrollTop = window.scrollY || window.pageYOffset;
  var maxScroll = Math.max(document.documentElement.scrollHeight - window.innerHeight, 1);
  var progress = Math.min(100, Math.max(0, (scrollTop / maxScroll) * 100));
  if (appContext.elements.readingProgressBar) {
    appContext.elements.readingProgressBar.style.width = progress + '%';
  }
  if (appContext.elements.backToTop) {
    appContext.elements.backToTop.classList.toggle('visible', scrollTop > 360);
  }
}

function syncDocumentChrome(file) {
  var title = file ? file.split('/').pop() : (appContext.elements.contentRoot ? appContext.elements.contentRoot.getAttribute('data-title') : '');
  if (!title) title = 'markdown-view';
  if (appContext.elements.documentTitleEl) {
    appContext.elements.documentTitleEl.textContent = title;
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

  if (appContext.config.isDirMode) {
    var fileParam = url.searchParams.get('file');
    if (fileParam && /\.md$/i.test(fileParam)) {
      return { file: fileParam, hash: url.hash };
    }
    return null;
  }
  // 単一ファイルモード: 同一path+hash形式のリンクは現在ファイル内ジャンプとして扱う
  if (appContext.state.currentFile) {
    return { file: appContext.state.currentFile, hash: url.hash };
  }
  return null;
}

function resolveMarkdownLinkTarget(href) {
  if (!appContext.config.isDirMode || !href || href.startsWith('#') || href.startsWith('/') || href.startsWith('?')) {
    return null;
  }
  if (href.startsWith('//') || isExternalSchemeHref(href)) {
    return null;
  }

  var currentDir = '';
  var baseUrl;
  var resolvedUrl;
  var relativePath;

  if (appContext.state.currentFile && appContext.state.currentFile.indexOf('/') !== -1) {
    currentDir = appContext.state.currentFile.slice(0, appContext.state.currentFile.lastIndexOf('/') + 1);
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
    console.warn('[markdown-view] hash のデコードに失敗したため raw fragment を使用します。', {
      hash: hash,
      error: error && error.message ? error.message : String(error)
    });
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

/// 旧形式メモ互換: リンク直後の兄弟テキストノードが `L5` / `L5-L7` と空白のみで構成される場合、
/// その行範囲を既存 hash に `:L5-L7` として合成して返す。
/// Why: 新形式（href fragment 内 `:L5-L7`）にフォーマット移行する前に生成された旧形式 citation
/// （`出典: [link](url) L15` の散文配置）を既存資産を書き換えずに救済する。
/// `#memo-preview` 配下のリンクに限定することで、ユーザーが本文に書いた `[spec](spec.md) L5 ...` の
/// ような自然文リンクを誤ジャンプ対象にしない。
/// 新形式（行範囲を既に含む hash）や行範囲情報が無い場合は hash をそのまま返す。
/// renderer がソース行トラッキング用に text を `<span>` でラップするケースに対応するため、
/// TEXT_NODE と ELEMENT_NODE の双方で `textContent` を見る。
function augmentHashWithTrailingLineHint(link, hash) {
  if (!link || !link.closest || !link.closest('#memo-preview')) return hash;
  var sibling = link.nextSibling;
  if (!sibling) return hash;
  if (sibling.nodeType !== Node.TEXT_NODE && sibling.nodeType !== Node.ELEMENT_NODE) return hash;
  if (parseLineHash(hash).lineRange) return hash;
  // 両端アンカー `^\s*...\s*$` で sibling textContent 全体が行番号トークンのみで構成されることを要求。
  // これにより `L10 onwards...` の散文や `L5abc` の別トークン連続を augment 対象から除外する
  var match = sibling.textContent.match(/^\s*L(\d+)(?:-L(\d+))?\s*$/);
  if (!match) return hash;
  var start = parseInt(match[1], 10);
  var end = match[2] ? parseInt(match[2], 10) : start;
  // end < start（逆転）および end == start（単一行）はどちらも start 1 行として扱う
  var suffix = end > start ? 'L' + start + '-L' + end : 'L' + start;
  if (!hash || hash === '#') return '#' + suffix;
  return hash + ':' + suffix;
}

function scrollToLineRange(targetLine, behavior) {
  // 行番号は renderer 側で 1-indexed。0 以下や非数値は無効として早期return
  if (!appContext.elements.contentRoot || typeof targetLine !== 'number' || targetLine < 1) return false;
  var blocks = appContext.elements.contentRoot.querySelectorAll('[data-line-block]');
  // 候補から「最狭マッチ（最深containment）」を選ぶ。
  // <ul>(L5-L20) と <li>(L7-L7) が共に line 7 を含むとき、最狭の <li> を選ぶ。
  // 広いコンテナを選ぶと対象行ではなくコンテナ先頭へスクロールしてしまうため。
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
    if (parsed.headingId) {
      markPendingTocNavigation(parsed.headingId);
    }
    setLocationHash(hash, replace);
    return true;
  }

  // 見出しIDへのフォールバックジャンプ
  if (parsed.headingId) {
    var targetEl = document.getElementById(parsed.headingId);
    if (!targetEl) return false;
    markPendingTocNavigation(parsed.headingId);
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
    clearPendingTocNavigation();
    restoreActiveTocHeading('');
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
  if (!appContext.elements.contentRoot) return;

  var headings = appContext.elements.contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6');
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

  var blocks = appContext.elements.contentRoot.querySelectorAll('pre.code-block');
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

  if (target.file === appContext.state.currentFile) {
    event.preventDefault();
    if (target.hash) {
      if (applyContentAnchorNavigation(target.hash, false)) {
        return;
      }
      console.warn('[markdown-view] 同一ファイル内の見出しが見つかりません:', target.hash);
    }
    setFileParam(appContext.state.currentFile, false, '');
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
  if (!appContext.elements.contentRoot) return;
  appContext.elements.contentRoot.addEventListener('click', handleInternalLinkClick);
}

function setupMemoLinkNavigation() {
  if (!appContext.elements.memoPreviewEl) return;
  appContext.elements.memoPreviewEl.addEventListener('click', handleInternalLinkClick);
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
