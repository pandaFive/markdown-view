function setupSelectionDeferral() {
  document.addEventListener('mousedown', function(e) {
    // コンテンツ領域でのマウスダウンを追跡
    var contentEl = appContext.elements.contentRoot;
    if (contentEl && contentEl.contains(e.target)) {
      appContext.state.isMouseSelecting = true;
    }
  });

  document.addEventListener('mouseup', function() {
    if (!appContext.state.isMouseSelecting) return;
    appContext.state.isMouseSelecting = false;
    // mouseup後もテキストが選択状態（ハイライト表示）のままなのでDOMを更新しない。
    // selectionchangeで選択が解除された（isCollapsed）時点で適用する。
  });

  // テキスト選択が完全に解除された時に保留更新を適用
  // ドラッグ中もselectionchangeが頻発するため、appContext.state.isMouseSelectingで除外する
  document.addEventListener('selectionchange', function() {
    if (appContext.state.isMouseSelecting) return;
    var sel = window.getSelection();
    if (sel && sel.isCollapsed && appContext.state.pendingUpdate) {
      applyPendingUpdate();
    }
  });
}

// テキスト選択中かを判定（ドラッグ操作中 or 選択範囲が存在）
// mousedown直後はgetSelection()がまだ更新されない場合があるため、
// appContext.state.isMouseSelectingフラグで補完する
function isTextSelected() {
  if (appContext.state.isMouseSelecting) return true;
  var sel = window.getSelection();
  return sel && !sel.isCollapsed;
}

function ensurePendingUpdateTimer() {
  if (!appContext.state.pendingUpdateTimer) {
    // 選択状態が長時間維持されると live update が永久に止まるため、30秒で強制適用する。
    // 主目的は、マウスドラッグ中の innerHTML 更新でブラウザ選択が破壊されるのを避けつつ、
    // 選択解除イベントを取りこぼした場合でも live 表示へ戻すこと。
    appContext.state.pendingUpdateTimer = setTimeout(function() {
      appContext.state.pendingUpdateTimer = null;
      applyPendingUpdate();
    }, 30000);
  }
}
