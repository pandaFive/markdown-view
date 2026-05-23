function setupFilterableList<TItem extends HTMLElement>(options: FilterableListOptions<TItem>): void {
  var input = document.getElementById(options.inputId) as HTMLInputElement | null;
  var root = document.getElementById(options.rootId);
  if (!input || !root) return;

  var inputEl = input;
  var items = Array.from(options.getItems(root));
  var applyFilter = function(): void {
    var query = inputEl.value.trim().toLowerCase();
    options.apply(items, query, inputEl);
  };

  inputEl.addEventListener('input', applyFilter);
  applyFilter();
}

function createContentEnhancements(
  ctx: MarkdownViewAppContext,
  deps: ContentEnhancementsDeps
): MarkdownViewContentEnhancementsController {
  function setLiveStatus(state: LiveStatusState): void {
    if (!ctx.elements.liveStatusEl) return;
    ctx.elements.liveStatusEl.textContent = ctx.labels.liveStatus[state as keyof MarkdownViewLabels['liveStatus']] || state;
    ctx.elements.liveStatusEl.dataset.state = state;
    if (state === 'live') {
      deps.clearMemoSyncPendingStatus();
    }
  }

  function updateDocumentStats(): void {
    if (!ctx.elements.contentRoot) return;
    var headings = ctx.elements.contentRoot.querySelectorAll('h1, h2, h3, h4, h5, h6').length;
    var text = (ctx.elements.contentRoot.textContent || '').replace(/\s+/g, '');
    if (ctx.elements.docHeadingCountEl) {
      ctx.elements.docHeadingCountEl.textContent = '見出し ' + headings;
    }
    if (ctx.elements.docCharCountEl) {
      ctx.elements.docCharCountEl.textContent = '文字 ' + text.length;
    }
  }

  function updateReadingProgress(): void {
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

  function syncDocumentChrome(file: string): void {
    var title = file ? file.split('/').pop() : (ctx.elements.contentRoot ? ctx.elements.contentRoot.getAttribute('data-title') : '');
    if (!title) title = 'markdown-view';
    if (ctx.elements.documentTitleEl) {
      ctx.elements.documentTitleEl.textContent = title;
    }
    document.title = title + ' - markdown-view';
  }

  function copyText(text: string): Promise<void> {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(text);
    }
    return new Promise<void>(function(resolve, reject) {
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

  function flashCopiedState(button: HTMLElement | null, copiedLabel: string, baseLabel: string): void {
    if (!button) return;
    button.classList.add('copied');
    button.textContent = copiedLabel;
    setTimeout(function() {
      button.classList.remove('copied');
      button.textContent = baseLabel;
    }, 1200);
  }

  function handleCopyClick(button: HTMLElement, text: string, baseLabel: string): void {
    copyText(text).then(function() {
      flashCopiedState(button, 'Copied', baseLabel);
    }).catch(function(err) {
      console.warn('[markdown-view] コピーに失敗:', err);
      flashCopiedState(button, 'Failed', baseLabel);
    });
  }

  function enhanceContentInteractions(): void {
    if (!ctx.elements.contentRoot) return;

    var headings = ctx.elements.contentRoot.querySelectorAll<HTMLElement>('h1, h2, h3, h4, h5, h6');
    headings.forEach(function(heading: HTMLElement): void {
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

    var blocks = ctx.elements.contentRoot.querySelectorAll<HTMLElement>('pre.code-block');
    blocks.forEach(function(block: HTMLElement): void {
      if (block.querySelector('.code-copy')) return;
      var code = block.querySelector<HTMLElement>('code');
      if (!code) return;
      var codeEl = code;
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'code-copy';
      button.textContent = 'Copy';
      button.setAttribute('aria-label', 'コードをコピー');
      button.addEventListener('click', function() {
        handleCopyClick(button, codeEl.innerText || codeEl.textContent || '', 'Copy');
      });
      block.appendChild(button);
    });
  }

  function setupTocFilter(): void {
    setupFilterableList({
      inputId: 'toc-filter',
      rootId: 'toc',
      getItems: function(root: HTMLElement): NodeListOf<HTMLElement> {
        return root.querySelectorAll<HTMLElement>('li');
      },
      apply: function(items: HTMLElement[], query: string): void {
        items.forEach(function(item: HTMLElement): void {
          var link = item.querySelector<HTMLAnchorElement>(':scope > a');
          if (!link) return;
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
