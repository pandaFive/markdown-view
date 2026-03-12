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

if (isDirMode) {
  window.addEventListener('popstate', function() {
    var file = getFileParam();
    if (file && file !== currentFile) {
      selectFile(file, false);
    }
  });
}

var currentTocTracking = null;
var tocTrackingFrame = null;
var currentActiveTocId = '';
var suppressTocTrackingUntil = 0;
var suppressTocTrackingTimer = null;
var pendingSuppressedTocTrackingUpdate = false;
var TOC_NAVIGATION_GRACE_MS = 400;
var TOC_NAVIGATION_SLACK_PX = 24;
var pendingTocNavigationId = '';
var pendingTocNavigationUntil = 0;
var tocRoot = document.getElementById('toc');

function setActiveTocLink(activeId) {
  if (!currentTocTracking) return;
  if (currentActiveTocId === activeId) return;
  currentActiveTocId = activeId;
  currentTocTracking.links.forEach(function(link, id) {
    link.classList.toggle('active', id === activeId);
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
  if (!currentTocTracking) return;
  setActiveTocLink(getViewportActiveTocId());
}

function clearPendingTocNavigation() {
  pendingTocNavigationId = '';
  pendingTocNavigationUntil = 0;
}

function findTrackedHeading(id) {
  if (!currentTocTracking || !id) return null;
  for (var i = 0; i < currentTocTracking.headings.length; i++) {
    if (currentTocTracking.headings[i].id === id) {
      return currentTocTracking.headings[i];
    }
  }
  return null;
}

function markPendingTocNavigation(id) {
  if (!findTrackedHeading(id)) return;
  pendingTocNavigationId = id;
  pendingTocNavigationUntil = Date.now() + TOC_NAVIGATION_GRACE_MS;
  setActiveTocLink(id);
}

function getPendingTocNavigationId(activationOffset) {
  if (!pendingTocNavigationId) return '';
  var heading = findTrackedHeading(pendingTocNavigationId);
  var navigationTop;
  var maxScrollTop;
  var currentScrollTop;
  if (!heading) {
    clearPendingTocNavigation();
    return '';
  }
  if (Date.now() > pendingTocNavigationUntil) {
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
  if (!currentTocTracking) return '';
  var activationOffset = currentTocTracking.activationOffset;
  var pendingActiveId = getPendingTocNavigationId(activationOffset);
  var activeHeading = null;
  var currentScrollTop = window.scrollY || window.pageYOffset;
  var maxScrollTop = Math.max(document.documentElement.scrollHeight - window.innerHeight, 0);
  var i;

  if (pendingActiveId) {
    return pendingActiveId;
  }

  currentTocTracking.headings.forEach(function(heading) {
    if (heading.getBoundingClientRect().top <= activationOffset) {
      activeHeading = heading;
    }
  });

  if (activeHeading && activeHeading.id !== pendingTocNavigationId) {
    clearPendingTocNavigation();
  }
  if (maxScrollTop > 0 && currentScrollTop >= maxScrollTop - 1) {
    for (i = currentTocTracking.headings.length - 1; i >= 0; i--) {
      if (currentTocTracking.headings[i].getBoundingClientRect().top < window.innerHeight) {
        return currentTocTracking.headings[i].id;
      }
    }
  }
  return activeHeading ? activeHeading.id : '';
}

function hasTrackedHeading(id) {
  if (!currentTocTracking || !id) return false;
  return currentTocTracking.headings.some(function(heading) {
    return heading.id === id;
  });
}

function getCurrentActiveTocId() {
  return currentActiveTocId;
}

function restoreActiveTocHeading(preferredId) {
  if (!currentTocTracking) return;
  var viewportActiveId = getViewportActiveTocId();
  if (hasTrackedHeading(preferredId) && preferredId === viewportActiveId) {
    setActiveTocLink(preferredId);
    return;
  }
  setActiveTocLink(viewportActiveId);
}

function scheduleTocTrackingUpdate() {
  if (!currentTocTracking || tocTrackingFrame !== null) return;
  if (Date.now() < suppressTocTrackingUntil) {
    pendingSuppressedTocTrackingUpdate = true;
    ensureSuppressedTocTrackingResume();
    return;
  }
  tocTrackingFrame = window.requestAnimationFrame(function() {
    tocTrackingFrame = null;
    updateActiveTocHeading();
  });
}

function ensureSuppressedTocTrackingResume() {
  if (suppressTocTrackingTimer !== null) return;
  var delay = Math.max(suppressTocTrackingUntil - Date.now(), 0);
  suppressTocTrackingTimer = window.setTimeout(function() {
    suppressTocTrackingTimer = null;
    if (!pendingSuppressedTocTrackingUpdate) return;
    pendingSuppressedTocTrackingUpdate = false;
    scheduleTocTrackingUpdate();
  }, delay);
}

function suppressTocTrackingFor(ms) {
  suppressTocTrackingUntil = Date.now() + ms;
  pendingSuppressedTocTrackingUpdate = true;
  if (suppressTocTrackingTimer !== null) {
    window.clearTimeout(suppressTocTrackingTimer);
    suppressTocTrackingTimer = null;
  }
  ensureSuppressedTocTrackingResume();
}

function setupTocTracking() {
  if (tocTrackingFrame !== null) {
    window.cancelAnimationFrame(tocTrackingFrame);
    tocTrackingFrame = null;
  }
  if (suppressTocTrackingTimer !== null) {
    window.clearTimeout(suppressTocTrackingTimer);
    suppressTocTrackingTimer = null;
  }
  currentTocTracking = null;
  currentActiveTocId = '';
  pendingSuppressedTocTrackingUpdate = false;

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

  currentTocTracking = {
    headings: trackedHeadings,
    links: tocLinksById,
    activationOffset: getTocActivationOffset(trackedHeadings)
  };
}

var sidebarToggle = document.getElementById('sidebar-toggle');
var sidebarOpen = document.getElementById('sidebar-open');
var sidebar = document.getElementById('sidebar');

if (sidebarToggle) {
  sidebarToggle.addEventListener('click', function() {
    sidebar.classList.remove('open');
  });
}

if (sidebarOpen) {
  sidebarOpen.addEventListener('click', function() {
    sidebar.classList.add('open');
  });
}

if (backToTop) {
  backToTop.addEventListener('click', function() {
    window.scrollTo({ top: 0, behavior: 'smooth' });
  });
}

if (tocRoot) {
  tocRoot.addEventListener('click', function(event) {
    var link = event.target.closest('a[href^="#"]');
    var href;
    if (!link || !tocRoot.contains(link)) return;
    href = link.getAttribute('href') || '';
    if (href.length <= 1) return;
    markPendingTocNavigation(href.slice(1));
  });
}

var themeToggle = document.getElementById('theme-toggle');
if (themeToggle) {
  themeToggle.addEventListener('click', function() {
    var current = htmlEl.getAttribute('data-theme');
    var next;
    if (current === 'dark') {
      next = 'light';
    } else if (current === 'light') {
      next = 'dark';
    } else {
      var prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      next = prefersDark ? 'light' : 'dark';
    }
    htmlEl.setAttribute('data-theme', next);
    try { localStorage.setItem('mdview-theme', next); } catch(e) {
      console.warn('[markdown-view] テーマ設定の保存に失敗:', e.message);
    }
  });

  try {
    var saved = localStorage.getItem('mdview-theme');
    if (saved === 'light' || saved === 'dark') {
      htmlEl.setAttribute('data-theme', saved);
    }
  } catch(e) {
    console.warn('[markdown-view] テーマ設定の読込に失敗:', e.message);
  }
}

connectWS();
setupTocTracking();
restoreActiveTocHeading('');
updateDocumentStats();
updateReadingProgress();
syncDocumentChrome(currentFile);
enhanceContentInteractions();
setupTocFilter();
window.addEventListener('scroll', updateReadingProgress, { passive: true });
window.addEventListener('scroll', scheduleTocTrackingUpdate, { passive: true });
window.addEventListener('resize', updateReadingProgress);
window.addEventListener('resize', scheduleTocTrackingUpdate);
setupTabs();
if (isDirMode) {
  setupFileList();
  setupFileFilter();
}
