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

var currentObserver = null;

function setupTocTracking() {
  if (currentObserver) {
    currentObserver.disconnect();
    currentObserver = null;
  }

  var headings = document.querySelectorAll('#content h1, #content h2, #content h3, #content h4, #content h5, #content h6');
  var tocLinks = document.querySelectorAll('#toc a');

  if (headings.length === 0 || tocLinks.length === 0) return;

  currentObserver = new IntersectionObserver(function(entries) {
    entries.forEach(function(entry) {
      if (entry.isIntersecting) {
        var id = entry.target.getAttribute('id');
        tocLinks.forEach(function(link) {
          link.classList.toggle('active', link.getAttribute('href') === '#' + id);
        });
      }
    });
  }, { rootMargin: '-10% 0% -80% 0%' });

  headings.forEach(function(heading) {
    if (heading.id) currentObserver.observe(heading);
  });
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
updateDocumentStats();
updateReadingProgress();
syncDocumentChrome(currentFile);
enhanceContentInteractions();
setupTocFilter();
window.addEventListener('scroll', updateReadingProgress, { passive: true });
window.addEventListener('resize', updateReadingProgress);
setupTabs();
if (isDirMode) {
  setupFileList();
  setupFileFilter();
}
