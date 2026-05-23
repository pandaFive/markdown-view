function validateUpdatePayload(data: unknown): ContentUpdateValidation {
  var safeData: ContentUpdatePayload = data && typeof data === 'object' && !Array.isArray(data) ? data : {};
  var missing: string[] = [];

  if (typeof safeData.content !== 'string') missing.push('content');
  if (typeof safeData.toc !== 'string') missing.push('toc');

  return {
    safeData: safeData,
    missing: missing,
    hasContractViolation: missing.length > 0
  };
}

function logUpdatePayloadContractViolation(validation: ContentUpdateValidation): void {
  var safeData = validation.safeData;
  console.warn('[markdown-view] updateContent: ' + validation.missing.join(', ') + ' が欠落または不正 (契約違反)', {
    missing: validation.missing.slice(),
    file: typeof safeData.file === 'string' ? safeData.file : null,
    contentLength: typeof safeData.content === 'string' ? safeData.content.length : null,
    tocLength: typeof safeData.toc === 'string' ? safeData.toc.length : null
  });
}

function normalizeTocHtml(html: string): string {
  return (html || '').replace(/>\s+</g, '><').trim();
}

function requireUpdateTarget(element: HTMLElement | null, selector: string): asserts element is HTMLElement {
  if (!element) {
    throw new Error('updateContent target missing: ' + selector);
  }
}

// サーバーサイドでサニタイズ済みのHTMLだけを #content に反映する境界。
// XSS防止: src/renderer/render.rs で raw/inline HTML event を破棄済み。
function applySanitizedContentHtml(ctx: MarkdownViewAppContext, contentEl: HTMLElement | null, content: unknown): boolean {
  requireUpdateTarget(contentEl, '#content');
  if (typeof content !== 'string') {
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
function applySanitizedTocHtml(tocEl: HTMLElement | null, toc: unknown): boolean {
  requireUpdateTarget(tocEl, '#toc');
  if (typeof toc !== 'string') {
    return false;
  }
  if (normalizeTocHtml(tocEl.innerHTML) === normalizeTocHtml(toc)) {
    return false;
  }
  tocEl.innerHTML = toc;
  return true;
}

function applyValidatedUpdateHtml(ctx: MarkdownViewAppContext, targets: ContentUpdateTargets, validation: ContentUpdateValidation): {
  contentChanged: boolean;
  tocChanged: boolean;
} {
  requireUpdateTarget(targets.contentEl, '#content');
  requireUpdateTarget(targets.tocEl, '#toc');
  var safeData = validation.safeData;
  return {
    contentChanged: applySanitizedContentHtml(ctx, targets.contentEl, safeData.content),
    tocChanged: applySanitizedTocHtml(targets.tocEl, safeData.toc)
  };
}
