function updateFileListActive(file) {
  var items = document.querySelectorAll('.file-tree-file');
  items.forEach(function(li) {
    var link = li.querySelector('a');
    if (link && link.getAttribute('data-file') === file) {
      li.classList.add('active');
      var parent = li.parentElement;
      while (parent && parent.id !== 'sidebar') {
        if (parent.tagName === 'DETAILS') {
          parent.open = true;
        }
        parent = parent.parentElement;
      }
    } else {
      li.classList.remove('active');
    }
  });
}

function setupFileList() {
  var fileLinks = document.querySelectorAll('.file-tree-file a[data-file]');
  fileLinks.forEach(function(link) {
    link.addEventListener('click', function(e) {
      e.preventDefault();
      selectFile(link.getAttribute('data-file'));
    });
  });
}

function setupFileFilter() {
  var summary = document.getElementById('file-filter-summary');
  setupFilterableList({
    inputId: 'file-filter',
    rootId: 'panel-files',
    getItems: function(root) {
      return root.querySelectorAll('.file-tree-file');
    },
    apply: function(items, query, input) {
      var total = items.length;
      var visible = 0;

      items.forEach(function(item) {
        var link = item.querySelector('a[data-file]');
        var matched = !query || (link && link.getAttribute('data-file').toLowerCase().indexOf(query) !== -1);
        item.hidden = !matched;
        if (matched) visible++;
      });

      var dirs = document.querySelectorAll('.file-tree-dir');
      dirs.forEach(function(dir) {
        var descendants = dir.querySelectorAll('.file-tree-file');
        var hasVisibleChild = Array.prototype.some.call(descendants, function(item) {
          return !item.hidden;
        });
        dir.parentElement.hidden = !hasVisibleChild;
        if (query && hasVisibleChild) {
          dir.open = true;
        }
      });

      if (summary) {
        if (input.value.trim()) {
          summary.textContent = visible + ' / ' + total + ' files';
        } else {
          summary.textContent = total + ' files';
        }
      }
    }
  });
}

function setupTabs() {
  var tabs = document.querySelectorAll('.sidebar-tab');
  if (!tabs.length) return;
  tabs.forEach(function(tab) {
    tab.addEventListener('click', function() {
      activateSidebarTab(tab.getAttribute('data-tab'));
    });
  });
}

function activateSidebarTab(target) {
  var tabs = document.querySelectorAll('.sidebar-tab');
  tabs.forEach(function(tab) {
    tab.classList.toggle('active', tab.getAttribute('data-tab') === target);
  });
  var panels = document.querySelectorAll('.sidebar-panel');
  panels.forEach(function(panel) {
    panel.classList.toggle('active', panel.id === 'panel-' + target);
  });
}

// URL エンコード差を吸収して比較するためのヘルパー。location.hash と
// appContext.sidebar.pendingTocNavigationId を両辺 decode して対称に扱うことで、renderer 側の href
// 生成が encoded/raw どちらでもガード条件が一貫して成立する
function tryDecodeHash(value) {
  if (!value) return value || '';
  try { return decodeURIComponent(value); } catch (e) { return value; }
}

function setupDirectoryHistoryNavigation() {
  if (!appContext.config.isDirMode) return;
  window.addEventListener('popstate', function() {
    var file = getFileParam();
    var hash = location.hash || '';

    if (file && file !== appContext.state.currentFile) {
      selectFile(file, false, {
        scrollMode: hash ? 'none' : 'reset',
        anchorHash: hash,
        historyHash: hash
      });
      return;
    }

    // 目次クリック直後の猶予期間（TOC_NAVIGATION_GRACE_MS=400ms）内は
    // appContext.sidebar.pendingTocNavigationId が立ち、ブラウザ既定のアンカースクロールも完了している。
    // このタイミングで pending と同一 hash の popstate が発火すると、restore 経由の
    // scrollIntoView がユーザの明示的 scrollTo を上書きし、getPendingTocNavigationId
    // の帯外判定（同関数内の末尾 clear 経路）が働かず逆方向スクロールで pending が
    // クリアされなくなる。一致 hash の再処理は redundant なのでスキップする。
    // location.hash は日本語など非ASCII文字で URL エンコード済み、appContext.sidebar.pendingTocNavigationId
    // は href.slice(1) で取得する。renderer 側の href 生成が encoded/raw どちらでも
    // 対称に一致判定するため両辺 decode してから比較する。
    // 行範囲形式（例 '#foo:L5'）は appContext.sidebar.pendingTocNavigationId (='foo') と不一致のため
    // ここを通過し、restore 側の lineRange 分岐で処理される。
    // なお clear や scroll 再計算は行わない。pending の解除は grace タイマー失効、
    // または後続 scroll イベント由来の getPendingTocNavigationId の帯外判定に委ねる
    var decodedHash = tryDecodeHash(hash);
    var decodedPendingId = tryDecodeHash(appContext.sidebar.pendingTocNavigationId);
    if (appContext.sidebar.pendingTocNavigationId && decodedHash === '#' + decodedPendingId) {
      return;
    }

    appContext.content.restoreNavigationFromLocation();
  });
}

var TOC_NAVIGATION_GRACE_MS = 400;
var TOC_NAVIGATION_SLACK_PX = 24;

function setActiveTocLink(activeId) {
  if (!appContext.sidebar.currentTocTracking) return;
  if (appContext.sidebar.currentActiveTocId === activeId) return;
  var nextLink = activeId ? appContext.sidebar.currentTocTracking.links.get(activeId) : null;
  var currentLink = appContext.sidebar.currentActiveTocId ? appContext.sidebar.currentTocTracking.links.get(appContext.sidebar.currentActiveTocId) : null;

  if (nextLink) {
    nextLink.classList.add('active');
  }
  if (currentLink && currentLink !== nextLink) {
    currentLink.classList.remove('active');
  }

  appContext.sidebar.currentActiveTocId = activeId;
  appContext.sidebar.currentTocTracking.links.forEach(function(link, id) {
    if (link !== nextLink && link !== currentLink) {
      link.classList.toggle('active', id === activeId);
    }
  });
}

function getTocActivationOffset(headings) {
  if (!headings.length) return 112;
  var scrollMarginTop = parseFloat(window.getComputedStyle(headings[0]).scrollMarginTop);
  if (!Number.isFinite(scrollMarginTop) || scrollMarginTop < 0) {
    return 112;
  }
  return scrollMarginTop;
}

function updateActiveTocHeading() {
  if (!appContext.sidebar.currentTocTracking) return;
  setActiveTocLink(getViewportActiveTocId());
}

function clearPendingTocNavigation() {
  appContext.sidebar.pendingTocNavigationId = '';
  appContext.sidebar.pendingTocNavigationUntil = 0;
}

function findTrackedHeading(id) {
  if (!appContext.sidebar.currentTocTracking || !id) return null;
  for (var i = 0; i < appContext.sidebar.currentTocTracking.headings.length; i++) {
    if (appContext.sidebar.currentTocTracking.headings[i].id === id) {
      return appContext.sidebar.currentTocTracking.headings[i];
    }
  }
  return null;
}

function markPendingTocNavigation(id) {
  if (!findTrackedHeading(id)) return;
  if (appContext.test.markPendingTocNavigationObserver) {
    appContext.test.markPendingTocNavigationObserver(id);
  }
  appContext.sidebar.pendingTocNavigationId = id;
  appContext.sidebar.pendingTocNavigationUntil = Date.now() + TOC_NAVIGATION_GRACE_MS;
  setActiveTocLink(id);
}

function getPendingTocNavigationId(activationOffset) {
  if (!appContext.sidebar.pendingTocNavigationId) return '';
  var heading = findTrackedHeading(appContext.sidebar.pendingTocNavigationId);
  var navigationTop;
  var maxScrollTop;
  var currentScrollTop;
  if (!heading) {
    clearPendingTocNavigation();
    return '';
  }
  if (Date.now() > appContext.sidebar.pendingTocNavigationUntil) {
    clearPendingTocNavigation();
    return '';
  }
  navigationTop = heading.getBoundingClientRect().top;
  currentScrollTop = window.scrollY || window.pageYOffset;
  maxScrollTop = Math.max(document.documentElement.scrollHeight - window.innerHeight, 0);
  if (
    navigationTop <= activationOffset + TOC_NAVIGATION_SLACK_PX &&
    navigationTop >= activationOffset - TOC_NAVIGATION_SLACK_PX
  ) {
    return heading.id;
  }
  if (currentScrollTop >= maxScrollTop - 1 && navigationTop < activationOffset - TOC_NAVIGATION_SLACK_PX) {
    return heading.id;
  }
  clearPendingTocNavigation();
  return '';
}

function getViewportActiveTocId() {
  if (!appContext.sidebar.currentTocTracking) return '';
  var activationOffset = appContext.sidebar.currentTocTracking.activationOffset;
  var pendingActiveId = getPendingTocNavigationId(activationOffset);
  var activeHeading = null;
  var currentScrollTop = window.scrollY || window.pageYOffset;
  var maxScrollTop = Math.max(document.documentElement.scrollHeight - window.innerHeight, 0);
  var i;

  if (pendingActiveId) {
    return pendingActiveId;
  }

  appContext.sidebar.currentTocTracking.headings.forEach(function(heading) {
    if (heading.getBoundingClientRect().top <= activationOffset) {
      activeHeading = heading;
    }
  });

  if (activeHeading && activeHeading.id !== appContext.sidebar.pendingTocNavigationId) {
    clearPendingTocNavigation();
  }
  if (maxScrollTop > 0 && currentScrollTop >= maxScrollTop - 1) {
    for (i = appContext.sidebar.currentTocTracking.headings.length - 1; i >= 0; i--) {
      if (appContext.sidebar.currentTocTracking.headings[i].getBoundingClientRect().top < window.innerHeight) {
        return appContext.sidebar.currentTocTracking.headings[i].id;
      }
    }
  }
  return activeHeading ? activeHeading.id : '';
}

function hasTrackedHeading(id) {
  if (!appContext.sidebar.currentTocTracking || !id) return false;
  return appContext.sidebar.currentTocTracking.headings.some(function(heading) {
    return heading.id === id;
  });
}

function getCurrentActiveTocId() {
  return appContext.sidebar.currentActiveTocId;
}

function restoreActiveTocHeading(preferredId) {
  if (!appContext.sidebar.currentTocTracking) return;
  var viewportActiveId = getViewportActiveTocId();
  if (hasTrackedHeading(preferredId) && preferredId === viewportActiveId) {
    setActiveTocLink(preferredId);
    return;
  }
  setActiveTocLink(viewportActiveId);
}

function scheduleTocTrackingUpdate() {
  if (!appContext.sidebar.currentTocTracking || appContext.sidebar.tocTrackingFrame !== null) return;
  if (Date.now() < appContext.sidebar.suppressTocTrackingUntil) {
    appContext.sidebar.pendingSuppressedTocTrackingUpdate = true;
    ensureSuppressedTocTrackingResume();
    return;
  }
  appContext.sidebar.tocTrackingFrame = window.requestAnimationFrame(function() {
    appContext.sidebar.tocTrackingFrame = null;
    updateActiveTocHeading();
  });
}

function ensureSuppressedTocTrackingResume() {
  if (appContext.sidebar.suppressTocTrackingTimer !== null) return;
  var delay = Math.max(appContext.sidebar.suppressTocTrackingUntil - Date.now(), 0);
  appContext.sidebar.suppressTocTrackingTimer = window.setTimeout(function() {
    appContext.sidebar.suppressTocTrackingTimer = null;
    if (!appContext.sidebar.pendingSuppressedTocTrackingUpdate) return;
    appContext.sidebar.pendingSuppressedTocTrackingUpdate = false;
    scheduleTocTrackingUpdate();
  }, delay);
}

function suppressTocTrackingFor(ms) {
  appContext.sidebar.suppressTocTrackingUntil = Date.now() + ms;
  appContext.sidebar.pendingSuppressedTocTrackingUpdate = true;
  if (appContext.sidebar.suppressTocTrackingTimer !== null) {
    window.clearTimeout(appContext.sidebar.suppressTocTrackingTimer);
    appContext.sidebar.suppressTocTrackingTimer = null;
  }
  ensureSuppressedTocTrackingResume();
}

function setupTocTracking() {
  var previousActiveTocId = appContext.sidebar.currentActiveTocId;
  if (appContext.sidebar.tocTrackingFrame !== null) {
    window.cancelAnimationFrame(appContext.sidebar.tocTrackingFrame);
    appContext.sidebar.tocTrackingFrame = null;
  }
  if (appContext.sidebar.suppressTocTrackingTimer !== null) {
    window.clearTimeout(appContext.sidebar.suppressTocTrackingTimer);
    appContext.sidebar.suppressTocTrackingTimer = null;
  }
  appContext.sidebar.currentTocTracking = null;
  appContext.sidebar.currentActiveTocId = '';
  appContext.sidebar.pendingSuppressedTocTrackingUpdate = false;

  var headings = document.querySelectorAll('#content h1, #content h2, #content h3, #content h4, #content h5, #content h6');
  var tocLinks = document.querySelectorAll('#toc a');

  if (headings.length === 0 || tocLinks.length === 0) return;

  var tocLinksById = new Map();
  tocLinks.forEach(function(link) {
    var href = link.getAttribute('href') || '';
    if (href.startsWith('#') && href.length > 1) {
      tocLinksById.set(href.slice(1), link);
    }
  });

  var trackedHeadings = Array.prototype.filter.call(headings, function(heading) {
    return heading.id && tocLinksById.has(heading.id);
  });

  if (trackedHeadings.length === 0) return;

  appContext.sidebar.currentTocTracking = {
    headings: trackedHeadings,
    links: tocLinksById,
    activationOffset: getTocActivationOffset(trackedHeadings)
  };

  if (previousActiveTocId && tocLinksById.has(previousActiveTocId)) {
    setActiveTocLink(previousActiveTocId);
  }
}

function setupSidebarInteractions() {
  var sidebarToggle = document.getElementById('sidebar-toggle');
  var sidebarOpen = document.getElementById('sidebar-open');
  var sidebar = document.getElementById('sidebar');

  if (sidebarToggle && sidebar) {
    sidebarToggle.addEventListener('click', function() {
      sidebar.classList.remove('open');
    });
  }

  if (sidebarOpen && sidebar) {
    sidebarOpen.addEventListener('click', function() {
      sidebar.classList.add('open');
    });
  }

  if (appContext.elements.backToTop) {
    appContext.elements.backToTop.addEventListener('click', function() {
      window.scrollTo({ top: 0, behavior: 'smooth' });
    });
  }

  if (appContext.sidebar.tocRoot) {
    appContext.sidebar.tocRoot.addEventListener('click', function(event) {
      var link = event.target.closest('a[href^="#"]');
      var href;
      if (!link || !appContext.sidebar.tocRoot.contains(link)) return;
      href = link.getAttribute('href') || '';
      if (href.length <= 1) return;
      markPendingTocNavigation(href.slice(1));
    });
  }

  window.addEventListener('scroll', function() {
    appContext.content.updateReadingProgress();
  }, { passive: true });
  window.addEventListener('scroll', scheduleTocTrackingUpdate, { passive: true });
  window.addEventListener('resize', function() {
    appContext.content.updateReadingProgress();
  });
  window.addEventListener('resize', scheduleTocTrackingUpdate);
}

function setupThemeToggle() {
  var themeToggle = document.getElementById('theme-toggle');
  if (!themeToggle) return;

  themeToggle.addEventListener('click', function() {
    var current = appContext.elements.htmlEl.getAttribute('data-theme');
    var next;
    if (current === 'dark') {
      next = 'light';
    } else if (current === 'light') {
      next = 'dark';
    } else {
      var prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      next = prefersDark ? 'light' : 'dark';
    }
    appContext.elements.htmlEl.setAttribute('data-theme', next);
    try { localStorage.setItem('mdview-theme', next); } catch(e) {
      console.warn('[markdown-view] テーマ設定の保存に失敗:', e.message);
    }
  });

  try {
    var saved = localStorage.getItem('mdview-theme');
    if (saved === 'light' || saved === 'dark') {
      appContext.elements.htmlEl.setAttribute('data-theme', saved);
    }
  } catch(e) {
    console.warn('[markdown-view] テーマ設定の読込に失敗:', e.message);
  }
}

function installMarkdownViewTestHooks() {
  if (window.__MV_E2E__ !== true) return;
  window.markdownViewTestHooks = {
    activateSidebarTab: function(target) {
      return activateSidebarTab(target);
    },
    applyDocumentSearchQuery: function(query) {
      return appContext.content.applyDocumentSearchQuery(query);
    },
    augmentHashWithTrailingLineHint: function(link, hash) {
      return appContext.content.augmentHashWithTrailingLineHint(link, hash);
    },
    markPendingTocNavigation: function(id) {
      return markPendingTocNavigation(id);
    },
    setMarkPendingTocNavigationObserverForTest: function(callback) {
      appContext.test.markPendingTocNavigationObserver = typeof callback === 'function' ? callback : null;
    },
    moveDocumentSearch: function(direction) {
      return appContext.content.moveDocumentSearch(direction);
    },
    scheduleBufferedLiveUpdate: function(data) {
      if (!appContext.websocket) {
        throw new Error('WebSocket controller is not initialized');
      }
      return appContext.websocket.scheduleBufferedLiveUpdate(data);
    },
    selectFile: function(file, pushHistory, options) {
      return selectFile(file, pushHistory, options);
    },
    setCurrentFileForTest: function(file) {
      appContext.state.currentFile = file || '';
    },
    setDirModeForTest: function(value) {
      appContext.config.isDirMode = !!value;
    },
    updateContent: function(data, options) {
      return appContext.content.updateContent(data, options);
    },
    get isDirMode() {
      return appContext.config.isDirMode;
    },
    get currentFile() {
      return appContext.state.currentFile;
    },
    get lastAppliedContent() {
      return appContext.state.lastAppliedContent;
    }
  };
}

function startMarkdownViewApp() {
  appContext.content = createContentController(appContext, {
    activateSidebarTab: activateSidebarTab,
    clearMemoSyncPendingStatus: clearMemoSyncPendingStatus,
    clearPendingTocNavigation: clearPendingTocNavigation,
    createHttpError: createHttpError,
    getCurrentActiveTocId: getCurrentActiveTocId,
    getFileFetchErrorMessage: getFileFetchErrorMessage,
    hideFileFetchErrorBanner: hideFileFetchErrorBanner,
    hideQuoteSelectionAction: hideQuoteSelectionAction,
    hideWsServerErrorBanner: hideWsServerErrorBanner,
    markPendingTocNavigation: markPendingTocNavigation,
    restoreActiveTocHeading: restoreActiveTocHeading,
    selectFile: selectFile,
    setFileParam: setFileParam,
    showWsServerErrorBanner: showWsServerErrorBanner,
    setupTocTracking: setupTocTracking,
    suppressTocTrackingFor: suppressTocTrackingFor
  });
  setupSelectionDeferral();
  setupHistoryUrlSync();
  setupDirectoryHistoryNavigation();
  appContext.content.setup();
  setupMemoInteractions();
  setupSidebarInteractions();
  setupThemeToggle();

  appContext.websocket = createWebSocketController(appContext, {
    updateContent: appContext.content.updateContent,
    scheduleDirectorySearch: appContext.content.scheduleDirectorySearch,
    setLiveStatus: appContext.content.setLiveStatus,
    selectFile: selectFile,
    applyRemoteMemoUpdate: applyRemoteMemoUpdate,
    queueRemoteMemoReload: queueRemoteMemoReload
  });
  appContext.websocket.connect();

  setupTocTracking();
  restoreActiveTocHeading('');
  appContext.content.updateDocumentStats();
  appContext.content.updateReadingProgress();
  appContext.content.syncDocumentChrome(appContext.state.currentFile);
  appContext.content.enhanceContentInteractions();
  appContext.content.setupTocFilter();
  setupTabs();
  if (appContext.config.isDirMode) {
    setupFileList();
    setupFileFilter();
  }
  installMarkdownViewTestHooks();
}
