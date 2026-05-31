"use strict";
function createDocumentSearchController(ctx, deps) {
    var DOCUMENT_SEARCH_BLOCK_SELECTOR = 'p, li, blockquote, th, td, h1, h2, h3, h4, h5, h6';
    function createDocumentSearchEmptyState(message) {
        var empty = document.createElement('p');
        empty.className = 'document-search-empty';
        empty.textContent = message;
        return empty;
    }
    function formatDirectorySearchSummary() {
        var baseText;
        if (!ctx.search.currentDocumentQuery) {
            baseText = '0 件';
        }
        else if (ctx.search.currentDirectoryLoading) {
            baseText = '検索中...';
        }
        else if (ctx.search.currentDirectoryError) {
            baseText = 'エラー';
        }
        else if (!ctx.search.currentDirectoryResults.length) {
            baseText = '0 件';
        }
        else if (ctx.search.currentDirectoryIndex >= 0) {
            baseText = (ctx.search.currentDirectoryIndex + 1) + ' / ' + ctx.search.currentDirectoryResults.length + ' 件';
        }
        else {
            baseText = '0 / ' + ctx.search.currentDirectoryResults.length + ' 件';
        }
        if (ctx.search.currentDirectorySkippedFiles > 0) {
            return baseText + '（' + ctx.search.currentDirectorySkippedFiles + '件スキップ）';
        }
        return baseText;
    }
    function updateDocumentSearchSummary() {
        if (!ctx.elements.documentSearchSummaryEl)
            return;
        if (ctx.config.isDirMode) {
            ctx.elements.documentSearchSummaryEl.textContent = formatDirectorySearchSummary();
            return;
        }
        if (!ctx.search.documentMatches.length) {
            ctx.elements.documentSearchSummaryEl.textContent = '0 件';
            return;
        }
        ctx.elements.documentSearchSummaryEl.textContent = (ctx.search.currentDocumentIndex + 1) + ' / ' + ctx.search.documentMatches.length + ' 件';
    }
    function clearDocumentSearchHighlights() {
        if (!ctx.elements.contentRoot)
            return;
        ctx.elements.contentRoot.querySelectorAll('mark.document-search-match').forEach(function (mark) {
            var parent = mark.parentNode;
            if (!parent)
                return;
            parent.replaceChild(document.createTextNode(mark.textContent || ''), mark);
            parent.normalize();
        });
        ctx.search.documentMatches = [];
        ctx.search.currentDocumentIndex = -1;
        updateDocumentSearchSummary();
        renderDocumentSearchResults();
    }
    function shouldSkipDocumentSearchNode(node) {
        var parent = node.parentElement;
        if (!parent)
            return true;
        return Boolean(parent.closest('a, button, input, textarea, script, style, pre.code-block, mark.document-search-match'));
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
            if (char !== '\n' && !/[.!?。！？]/.test(char))
                continue;
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
            var range = sentenceRanges[i];
            if (matchStart < range.end && matchEnd > range.start) {
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
            if (!entry)
                break;
            if (targetSentenceIndex >= 0 && targetSentenceIndex < entry.sentences.length) {
                var sentence = entry.sentences[targetSentenceIndex];
                return entry.text.slice(sentence.start, sentence.end);
            }
            targetBlockIndex += direction;
            if (targetBlockIndex < 0 || targetBlockIndex >= blockEntries.length)
                break;
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
        if (!ctx.elements.contentRoot)
            return [];
        return Array.prototype.filter.call(ctx.elements.contentRoot.querySelectorAll(DOCUMENT_SEARCH_BLOCK_SELECTOR), function (block) {
            return !block.parentElement || !block.parentElement.closest(DOCUMENT_SEARCH_BLOCK_SELECTOR);
        });
    }
    function collectDocumentSearchTextNodes(block) {
        var walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT, null);
        var textNodes = [];
        var node;
        var offset = 0;
        while ((node = walker.nextNode())) {
            var textNode = node;
            if (!textNode.nodeValue || !textNode.nodeValue.trim())
                continue;
            if (shouldSkipDocumentSearchNode(textNode))
                continue;
            textNodes.push({
                node: textNode,
                start: offset,
                end: offset + textNode.nodeValue.length
            });
            offset += textNode.nodeValue.length;
        }
        return {
            text: textNodes.map(function (entry) { return entry.node.nodeValue || ''; }).join(''),
            nodes: textNodes
        };
    }
    function wrapDocumentSearchSegment(textNode, start, end, matchId) {
        var nodeValue = textNode.nodeValue || '';
        var tail = end < nodeValue.length ? textNode.splitText(end) : null;
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
            if (localStart >= localEnd)
                continue;
            marks.unshift(wrapDocumentSearchSegment(entry.node, localStart, localEnd, matchId).mark);
        }
        return marks;
    }
    function renderDocumentSearchResultContext(container, text, query, variant) {
        if (!text)
            return;
        var span = document.createElement('span');
        var normalizedText = text.replace(/\s+/g, ' ').trim();
        var escapedQuery = escapeRegExp(query);
        var parts = escapedQuery ? normalizedText.split(new RegExp('(' + escapedQuery + ')', 'ig')) : [normalizedText];
        span.className = 'document-search-result-context document-search-result-context-' + variant;
        if (container.childNodes.length > 0) {
            container.appendChild(document.createTextNode(' '));
        }
        parts.forEach(function (part) {
            if (!part)
                return;
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
        if (!ctx.elements.documentSearchResultsEl)
            return;
        var resultsEl = ctx.elements.documentSearchResultsEl;
        if (ctx.config.isDirMode) {
            deps.renderDirectorySearchResults();
            return;
        }
        var preservedScrollTop = resultsEl.scrollTop;
        resultsEl.innerHTML = '';
        if (!ctx.search.currentDocumentQuery)
            return;
        if (!ctx.search.documentMatches.length) {
            resultsEl.appendChild(createDocumentSearchEmptyState('一致する文が見つかりません。'));
            return;
        }
        ctx.search.documentMatches.forEach(function (match, index) {
            var button = document.createElement('button');
            var indexBadge = document.createElement('span');
            var body = document.createElement('span');
            button.type = 'button';
            button.className = 'document-search-result';
            button.dataset.matchIndex = String(index);
            button.classList.toggle('active', index === ctx.search.currentDocumentIndex);
            button.setAttribute('aria-current', index === ctx.search.currentDocumentIndex ? 'true' : 'false');
            button.addEventListener('click', function () {
                setCurrentDocumentSearchMatch(index);
            });
            indexBadge.className = 'document-search-result-index';
            indexBadge.textContent = String(index + 1).padStart(2, '0');
            body.className = 'document-search-result-body';
            renderDocumentSearchResultContext(body, match.context.before, ctx.search.currentDocumentQuery, 'before');
            renderDocumentSearchResultContext(body, match.context.current, ctx.search.currentDocumentQuery, 'current');
            renderDocumentSearchResultContext(body, match.context.after, ctx.search.currentDocumentQuery, 'after');
            button.appendChild(indexBadge);
            button.appendChild(body);
            resultsEl.appendChild(button);
        });
        resultsEl.scrollTop = preservedScrollTop;
    }
    function applyDocumentSearchHighlights(query) {
        if (!ctx.elements.contentRoot)
            return;
        clearDocumentSearchHighlights();
        if (!query)
            return;
        var normalizedQuery = query.toLowerCase();
        var blockEntries = getDocumentSearchBlocks().map(function (block) {
            var blockText = collectDocumentSearchTextNodes(block);
            return {
                text: blockText.text,
                nodes: blockText.nodes,
                sentences: splitTextIntoSentenceRanges(blockText.text)
            };
        });
        blockEntries.forEach(function (blockText, blockIndex) {
            var matchIndex;
            var searchIndex = 0;
            var blockMatches = [];
            if (!blockText.text)
                return;
            matchIndex = blockText.text.toLowerCase().indexOf(normalizedQuery, searchIndex);
            while (matchIndex !== -1) {
                blockMatches.push({
                    matchStart: matchIndex,
                    matchEnd: matchIndex + normalizedQuery.length,
                    matchId: ctx.search.documentMatches.length + blockMatches.length,
                    context: buildDocumentSearchContext(blockEntries, blockIndex, matchIndex, matchIndex + normalizedQuery.length),
                    marks: []
                });
                searchIndex = matchIndex + normalizedQuery.length;
                matchIndex = blockText.text.toLowerCase().indexOf(normalizedQuery, searchIndex);
            }
            blockMatches.slice().reverse().forEach(function (match) {
                var marks = wrapDocumentSearchMatch(blockText.nodes, match.matchStart, match.matchEnd, match.matchId);
                match.marks = marks;
            });
            blockMatches.forEach(function (match) {
                if (!match.marks.length)
                    return;
                ctx.search.documentMatches.push({
                    marks: match.marks,
                    context: match.context
                });
            });
        });
        if (ctx.search.documentMatches.length) {
            setCurrentDocumentSearchMatch(0, false);
        }
        else {
            updateDocumentSearchSummary();
            renderDocumentSearchResults();
        }
    }
    function setCurrentDocumentSearchMatch(index, scrollIntoView) {
        if (!ctx.search.documentMatches.length) {
            ctx.search.currentDocumentIndex = -1;
            updateDocumentSearchSummary();
            return;
        }
        if (ctx.search.currentDocumentIndex >= 0 && ctx.search.documentMatches[ctx.search.currentDocumentIndex]) {
            ctx.search.documentMatches[ctx.search.currentDocumentIndex].marks.forEach(function (mark) {
                mark.classList.remove('current');
            });
        }
        ctx.search.currentDocumentIndex = (index + ctx.search.documentMatches.length) % ctx.search.documentMatches.length;
        var currentMatch = ctx.search.documentMatches[ctx.search.currentDocumentIndex];
        currentMatch.marks.forEach(function (mark) {
            mark.classList.add('current');
        });
        if (scrollIntoView !== false) {
            currentMatch.marks[0].scrollIntoView({
                block: 'center',
                behavior: 'smooth'
            });
        }
        updateDocumentSearchSummary();
        renderDocumentSearchResults();
    }
    function moveDocumentSearch(step) {
        if (ctx.config.isDirMode) {
            if (!ctx.search.currentDirectoryResults.length)
                return;
            if (ctx.search.currentDirectoryIndex < 0) {
                deps.openDirectorySearchResult(step > 0 ? 0 : ctx.search.currentDirectoryResults.length - 1);
                return;
            }
            deps.openDirectorySearchResult(ctx.search.currentDirectoryIndex + step);
            return;
        }
        if (!ctx.search.documentMatches.length)
            return;
        setCurrentDocumentSearchMatch(ctx.search.currentDocumentIndex + step);
    }
    function applyDocumentSearchQuery(query) {
        var previousDocumentQuery = ctx.search.currentDocumentQuery;
        ctx.search.currentDocumentQuery = (query || '').trim();
        if (ctx.search.currentDocumentQuery !== previousDocumentQuery) {
            ctx.search.currentDirectoryResultsScrollTop = 0;
            if (ctx.elements.documentSearchResultsEl) {
                ctx.elements.documentSearchResultsEl.scrollTop = 0;
            }
        }
        if (ctx.config.isDirMode) {
            applyDocumentSearchHighlights(ctx.search.currentDocumentQuery);
            ctx.search.pendingDirectoryNavigation = null;
            if (!ctx.search.currentDocumentQuery) {
                if (ctx.search.documentDebounceTimer) {
                    clearTimeout(ctx.search.documentDebounceTimer);
                    ctx.search.documentDebounceTimer = null;
                }
                if (typeof deps.cancelDirectorySearch === 'function') {
                    deps.cancelDirectorySearch();
                }
                else {
                    ctx.search.documentFetchGeneration += 1;
                }
                ctx.search.currentDirectoryResults = [];
                ctx.search.currentDirectoryIndex = -1;
                ctx.search.currentDirectorySkippedFiles = 0;
                ctx.search.currentDirectoryTruncated = false;
                ctx.search.currentDirectoryTruncatedReasons = [];
                ctx.search.currentDirectoryLoading = false;
                ctx.search.currentDirectoryError = '';
                ctx.search.currentDirectoryResultsScrollTop = 0;
                deps.renderDirectorySearchUi({ preserveScroll: false });
                return;
            }
            ctx.search.currentDirectoryResults = [];
            ctx.search.currentDirectoryIndex = -1;
            ctx.search.currentDirectorySkippedFiles = 0;
            ctx.search.currentDirectoryTruncated = false;
            ctx.search.currentDirectoryTruncatedReasons = [];
            ctx.search.currentDirectoryLoading = true;
            ctx.search.currentDirectoryError = '';
            deps.scheduleDirectorySearch(ctx.search.currentDocumentQuery);
            deps.renderDirectorySearchUi({
                preserveScroll: ctx.search.currentDocumentQuery === previousDocumentQuery
            });
            return;
        }
        applyDocumentSearchHighlights(ctx.search.currentDocumentQuery);
    }
    function clearDocumentSearchQuery() {
        if (ctx.elements.documentSearchInputEl) {
            ctx.elements.documentSearchInputEl.value = '';
        }
        if (ctx.search.documentDebounceTimer) {
            clearTimeout(ctx.search.documentDebounceTimer);
            ctx.search.documentDebounceTimer = null;
        }
        if (ctx.config.isDirMode && typeof deps.cancelDirectorySearch === 'function') {
            deps.cancelDirectorySearch();
        }
        else {
            ctx.search.documentFetchGeneration += 1;
        }
        ctx.search.currentDocumentQuery = '';
        ctx.search.pendingDirectoryNavigation = null;
        ctx.search.currentDirectoryResults = [];
        ctx.search.currentDirectoryIndex = -1;
        ctx.search.currentDirectorySkippedFiles = 0;
        ctx.search.currentDirectoryTruncated = false;
        ctx.search.currentDirectoryTruncatedReasons = [];
        ctx.search.currentDirectoryLoading = false;
        ctx.search.currentDirectoryError = '';
        ctx.search.currentDirectoryResultsScrollTop = 0;
        clearDocumentSearchHighlights();
        if (ctx.config.isDirMode) {
            deps.renderDirectorySearchUi({ preserveScroll: false });
        }
    }
    function syncDocumentSearchAfterContentUpdate(options) {
        options = options || {};
        if (!ctx.elements.documentSearchInputEl)
            return;
        if (ctx.config.isDirMode) {
            ctx.search.currentDocumentQuery = (ctx.elements.documentSearchInputEl.value || '').trim();
            applyDocumentSearchHighlights(ctx.search.currentDocumentQuery);
            deps.applyPendingDirectorySearchNavigation();
            if (ctx.search.currentDocumentQuery && options.requeryDirectorySearch !== false) {
                deps.scheduleDirectorySearch(ctx.search.currentDocumentQuery);
            }
            deps.renderDirectorySearchUi();
            return;
        }
        applyDocumentSearchQuery(ctx.elements.documentSearchInputEl.value);
    }
    function openDocumentSearch() {
        deps.activateSidebarTab('toc');
        var sidebarEl = document.getElementById('sidebar');
        if (sidebarEl) {
            sidebarEl.classList.add('open');
        }
        if (ctx.elements.documentSearchInputEl) {
            ctx.elements.documentSearchInputEl.focus();
            ctx.elements.documentSearchInputEl.select();
        }
    }
    function setupDocumentSearch() {
        if (!ctx.elements.documentSearchInputEl)
            return;
        var inputEl = ctx.elements.documentSearchInputEl;
        inputEl.addEventListener('input', function () {
            applyDocumentSearchQuery(inputEl.value);
        });
        inputEl.addEventListener('keydown', function (event) {
            if (event.key === 'Enter') {
                event.preventDefault();
                moveDocumentSearch(event.shiftKey ? -1 : 1);
                return;
            }
            if (event.key === 'Escape') {
                event.preventDefault();
                if (inputEl.value) {
                    clearDocumentSearchQuery();
                }
                else {
                    inputEl.blur();
                }
            }
        });
        if (ctx.elements.documentSearchPrevEl) {
            ctx.elements.documentSearchPrevEl.addEventListener('click', function () {
                moveDocumentSearch(-1);
            });
        }
        if (ctx.elements.documentSearchNextEl) {
            ctx.elements.documentSearchNextEl.addEventListener('click', function () {
                moveDocumentSearch(1);
            });
        }
        if (ctx.elements.documentSearchClearEl) {
            ctx.elements.documentSearchClearEl.addEventListener('click', function () {
                clearDocumentSearchQuery();
                inputEl.focus();
            });
        }
        document.addEventListener('keydown', function (event) {
            var key = event.key.toLowerCase();
            if ((event.ctrlKey || event.metaKey) && key === 'f') {
                event.preventDefault();
                openDocumentSearch();
            }
        });
        updateDocumentSearchSummary();
    }
    return {
        applyDocumentSearchHighlights: applyDocumentSearchHighlights,
        applyDocumentSearchQuery: applyDocumentSearchQuery,
        clearDocumentSearchHighlights: clearDocumentSearchHighlights,
        clearDocumentSearchQuery: clearDocumentSearchQuery,
        createDocumentSearchEmptyState: createDocumentSearchEmptyState,
        moveDocumentSearch: moveDocumentSearch,
        openDocumentSearch: openDocumentSearch,
        renderDocumentSearchResultContext: renderDocumentSearchResultContext,
        renderDocumentSearchResults: renderDocumentSearchResults,
        setCurrentDocumentSearchMatch: setCurrentDocumentSearchMatch,
        setupDocumentSearch: setupDocumentSearch,
        syncDocumentSearchAfterContentUpdate: syncDocumentSearchAfterContentUpdate,
        updateDocumentSearchSummary: updateDocumentSearchSummary
    };
}
