document.addEventListener('mousedown', function(e) {
  // コンテンツ領域でのマウスダウンを追跡
  var contentEl = document.getElementById('content');
  if (contentEl && contentEl.contains(e.target)) {
    isMouseSelecting = true;
  }
});

document.addEventListener('mouseup', function() {
  if (!isMouseSelecting) return;
  isMouseSelecting = false;
  // mouseup後もテキストが選択状態（ハイライト表示）のままなのでDOMを更新しない。
  // selectionchangeで選択が解除された（isCollapsed）時点で適用する。
});

// テキスト選択が完全に解除された時に保留更新を適用
// ドラッグ中もselectionchangeが頻発するため、isMouseSelectingで除外する
document.addEventListener('selectionchange', function() {
  if (isMouseSelecting) return;
  var sel = window.getSelection();
  if (sel && sel.isCollapsed && pendingUpdate) {
    applyPendingUpdate();
  }
});

// テキスト選択中かを判定（ドラッグ操作中 or 選択範囲が存在）
// mousedown直後はgetSelection()がまだ更新されない場合があるため、
// isMouseSelectingフラグで補完する
function isTextSelected() {
  if (isMouseSelecting) return true;
  var sel = window.getSelection();
  return sel && !sel.isCollapsed;
}

function ensurePendingUpdateTimer() {
  if (!pendingUpdateTimer) {
    pendingUpdateTimer = setTimeout(function() {
      pendingUpdateTimer = null;
      applyPendingUpdate();
    }, 30000);
  }
}
