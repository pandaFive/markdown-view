function validateUpdatePayload(data) {
  var safeData = data && typeof data === 'object' && !Array.isArray(data) ? data : {};
  var missing = [];

  if (typeof safeData.content !== 'string') missing.push('content');
  if (typeof safeData.toc !== 'string') missing.push('toc');

  return {
    safeData: safeData,
    missing: missing,
    hasContractViolation: missing.length > 0
  };
}

function logUpdatePayloadContractViolation(validation) {
  var safeData = validation.safeData;
  console.warn('[markdown-view] updateContent: ' + validation.missing.join(', ') + ' が欠落または不正 (契約違反)', {
    missing: validation.missing.slice(),
    file: typeof safeData.file === 'string' ? safeData.file : null,
    contentLength: typeof safeData.content === 'string' ? safeData.content.length : null,
    tocLength: typeof safeData.toc === 'string' ? safeData.toc.length : null
  });
}

function normalizeTocHtml(html) {
  return (html || '').replace(/>\s+</g, '><').trim();
}

// サーバーサイドでサニタイズ済みのHTMLだけを #content に反映する境界。
// XSS防止: pulldown-cmarkでraw HTML無効化済み（renderer.rs参照）。
function applySanitizedContentHtml(ctx, contentEl, content) {
  if (!contentEl || typeof content !== 'string') {
    return false;
  }
  if (content === ctx.state.lastAppliedContent) {
    return false;
  }
  contentEl.innerHTML = content;
  ctx.state.lastAppliedContent = content;
  return true;
}

// サーバー生成済みTOC HTMLだけを #toc に反映する境界。
function applySanitizedTocHtml(tocEl, toc) {
  if (!tocEl || typeof toc !== 'string') {
    return false;
  }
  if (normalizeTocHtml(tocEl.innerHTML) === normalizeTocHtml(toc)) {
    return false;
  }
  tocEl.innerHTML = toc;
  return true;
}

function applyValidatedUpdateHtml(ctx, targets, validation) {
  var safeData = validation.safeData;
  return {
    contentChanged: applySanitizedContentHtml(ctx, targets.contentEl, safeData.content),
    tocChanged: applySanitizedTocHtml(targets.tocEl, safeData.toc)
  };
}
