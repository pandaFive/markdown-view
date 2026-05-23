function setupSelectionDeferral(): void {
  document.addEventListener('mousedown', function(e: MouseEvent): void {
    // コンテンツ領域でのマウスダウンを追跡
    var contentEl = appContext.elements.contentRoot;
    if (contentEl && contentEl.contains(e.target as Node)) {
      appContext.state.isMouseSelecting = true;
    }
  });

  document.addEventListener('mouseup', function(): void {
    if (!appContext.state.isMouseSelecting) return;
    appContext.state.isMouseSelecting = false;
    // mouseup後もテキストが選択状態（ハイライト表示）のままなのでDOMを更新しない。
    // selectionchangeで選択が解除された（isCollapsed）時点で適用する。
  });

  // テキスト選択が完全に解除された時に保留更新を適用
  // ドラッグ中もselectionchangeが頻発するため、appContext.state.isMouseSelectingで除外する
  document.addEventListener('selectionchange', function(): void {
    if (appContext.state.isMouseSelecting) return;
    var sel = window.getSelection();
    if (sel && sel.isCollapsed && appContext.state.pendingUpdate) {
      appContext.content!.applyPendingUpdate();
    }
  });
}

// テキスト選択中かを判定（ドラッグ操作中 or 選択範囲が存在）
// mousedown直後はgetSelection()がまだ更新されない場合があるため、
// appContext.state.isMouseSelectingフラグで補完する
function isTextSelected(): boolean {
  if (appContext.state.isMouseSelecting) return true;
  var sel = window.getSelection();
  return !!sel && !sel.isCollapsed;
}

function ensurePendingUpdateTimer(): void {
  if (!appContext.state.pendingUpdateTimer) {
    // 通常は selectionchange で選択解除を検知して保留更新を適用する。
    // 30秒はイベント取りこぼしや選択状態の長時間維持で live update が永久停止しないための
    // 最終フォールバックであり、ドラッグ中の選択保護を短時間で破らないため短縮しない。
    appContext.state.pendingUpdateTimer = setTimeout(function(): void {
      appContext.state.pendingUpdateTimer = null;
      appContext.content!.applyPendingUpdate();
    }, 30000);
  }
}
