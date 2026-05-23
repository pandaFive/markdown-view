"use strict";
function setupFilterableList(options) {
    var input = document.getElementById(options.inputId);
    var root = document.getElementById(options.rootId);
    if (!input || !root)
        return;
    var inputEl = input;
    var items = Array.from(options.getItems(root));
    var applyFilter = function () {
        var query = inputEl.value.trim().toLowerCase();
        options.apply(items, query, inputEl);
    };
    inputEl.addEventListener('input', applyFilter);
    applyFilter();
}
function createContentEnhancements(ctx, deps) {
    function setLiveStatus(state) {
        if (!ctx.elements.liveStatusEl)
            return;
        ctx.elements.liveStatusEl.textContent = ctx.labels.liveStatus[state] || state;
        ctx.elements.liveStatusEl.dataset.state = state;
        if (state === 'live') {
            deps.clearMemoSyncPendingStatus();
        }
    }
    function updateDocumentStats() {
        if (!ctx.elements.contentRoot)
            return;
        var headings = ctx.elements.contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6').length;
        var text = (ctx.elements.contentRoot.textContent || '').replace(/\s+/g, '');
        if (ctx.elements.docHeadingCountEl) {
            ctx.elements.docHeadingCountEl.textContent = '見出し ' + headings;
        }
        if (ctx.elements.docCharCountEl) {
            ctx.elements.docCharCountEl.textContent = '文字 ' + text.length;
        }
    }
    function updateReadingProgress() {
        var scrollTop = window.scrollY || window.pageYOffset;
        var maxScroll = Math.max(document.documentElement.scrollHeight - window.innerHeight, 1);
        var progress = Math.min(100, Math.max(0, (scrollTop / maxScroll) * 100));
        if (ctx.elements.readingProgressBar) {
            ctx.elements.readingProgressBar.style.width = progress + '%';
        }
        if (ctx.elements.backToTop) {
            ctx.elements.backToTop.classList.toggle('visible', scrollTop > 360);
        }
    }
    function syncDocumentChrome(file) {
        var title = file ? file.split('/').pop() : (ctx.elements.contentRoot ? ctx.elements.contentRoot.getAttribute('data-title') : '');
        if (!title)
            title = 'markdown-view';
        if (ctx.elements.documentTitleEl) {
            ctx.elements.documentTitleEl.textContent = title;
        }
        document.title = title + ' - markdown-view';
    }
    function copyText(text) {
        if (navigator.clipboard && navigator.clipboard.writeText) {
            return navigator.clipboard.writeText(text);
        }
        return new Promise(function (resolve, reject) {
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
                }
                else {
                    reject(new Error('execCommand("copy") returned false'));
                }
            }
            catch (error) {
                reject(error);
            }
        });
    }
    function flashCopiedState(button, copiedLabel, baseLabel) {
        if (!button)
            return;
        button.classList.add('copied');
        button.textContent = copiedLabel;
        setTimeout(function () {
            button.classList.remove('copied');
            button.textContent = baseLabel;
        }, 1200);
    }
    function handleCopyClick(button, text, baseLabel) {
        copyText(text).then(function () {
            flashCopiedState(button, 'Copied', baseLabel);
        }).catch(function (err) {
            console.warn('[markdown-view] コピーに失敗:', err);
            flashCopiedState(button, 'Failed', baseLabel);
        });
    }
    function enhanceContentInteractions() {
        if (!ctx.elements.contentRoot)
            return;
        var headings = ctx.elements.contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6');
        headings.forEach(function (heading) {
            if (!heading.id || heading.querySelector('.heading-anchor'))
                return;
            var button = document.createElement('button');
            button.type = 'button';
            button.className = 'heading-anchor';
            button.textContent = '#';
            button.setAttribute('aria-label', '見出しリンクをコピー');
            button.addEventListener('click', function () {
                var url = new URL(location.href);
                url.hash = heading.id;
                handleCopyClick(button, url.toString(), '#');
            });
            heading.appendChild(button);
        });
        var blocks = ctx.elements.contentRoot.querySelectorAll('pre.code-block');
        blocks.forEach(function (block) {
            if (block.querySelector('.code-copy'))
                return;
            var code = block.querySelector('code');
            if (!code)
                return;
            var codeEl = code;
            var button = document.createElement('button');
            button.type = 'button';
            button.className = 'code-copy';
            button.textContent = 'Copy';
            button.setAttribute('aria-label', 'コードをコピー');
            button.addEventListener('click', function () {
                handleCopyClick(button, codeEl.innerText || codeEl.textContent || '', 'Copy');
            });
            block.appendChild(button);
        });
    }
    function setupTocFilter() {
        setupFilterableList({
            inputId: 'toc-filter',
            rootId: 'toc',
            getItems: function (root) {
                return root.querySelectorAll('li');
            },
            apply: function (items, query) {
                items.forEach(function (item) {
                    var link = item.querySelector(':scope > a');
                    if (!link)
                        return;
                    var matched = !query || (link.textContent || '').toLowerCase().indexOf(query) !== -1;
                    item.hidden = !matched;
                });
            }
        });
    }
    return {
        setLiveStatus: setLiveStatus,
        updateDocumentStats: updateDocumentStats,
        updateReadingProgress: updateReadingProgress,
        syncDocumentChrome: syncDocumentChrome,
        enhanceContentInteractions: enhanceContentInteractions,
        setupTocFilter: setupTocFilter
    };
}
