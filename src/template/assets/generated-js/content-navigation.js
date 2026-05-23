"use strict";
function createContentNavigation(ctx, deps) {
    function updateLocationHash(url, hash) {
        if (hash === undefined)
            return;
        if (!hash) {
            url.hash = '';
            return;
        }
        url.hash = hash.charAt(0) === '#' ? hash : '#' + hash;
    }
    function setLocationHash(hash, replace) {
        var url = new URL(location.href);
        updateLocationHash(url, hash || '');
        if (replace) {
            history.replaceState(null, '', url.toString());
        }
        else {
            history.pushState(null, '', url.toString());
        }
    }
    function isModifiedClick(event) {
        return event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey;
    }
    function isExternalSchemeHref(href) {
        return /^[a-zA-Z][a-zA-Z\d+.-]*:/.test(href);
    }
    /// `?file=foo.md#hash` 形式や単一ファイルモードの同一path+hash形式を解決する。
    /// メモプレビュー内の出典リンクは memo.js の buildQuoteSource() がこの形式で生成する。
    /// resolveMarkdownLinkTarget は `?` 開始 href を拒否するため、こちらで補完する。
    function resolveFileQueryHref(href) {
        if (!href)
            return null;
        var url;
        try {
            url = new URL(href, location.href);
        }
        catch (error) {
            return null;
        }
        if (url.origin !== location.origin)
            return null;
        if (url.pathname !== location.pathname)
            return null;
        if (!url.hash)
            return null;
        if (ctx.config.isDirMode) {
            var fileParam = url.searchParams.get('file');
            if (fileParam && /\.md$/i.test(fileParam)) {
                return { file: fileParam, hash: url.hash };
            }
            return null;
        }
        // 単一ファイルモード: 同一path+hash形式のリンクは現在ファイル内ジャンプとして扱う
        if (ctx.state.currentFile) {
            return { file: ctx.state.currentFile, hash: url.hash };
        }
        return null;
    }
    function resolveMarkdownLinkTarget(href) {
        if (!ctx.config.isDirMode || !href || href.startsWith('#') || href.startsWith('/') || href.startsWith('?')) {
            return null;
        }
        if (href.startsWith('//') || isExternalSchemeHref(href)) {
            return null;
        }
        var currentDir = '';
        var baseUrl;
        var resolvedUrl;
        var relativePath;
        if (ctx.state.currentFile && ctx.state.currentFile.indexOf('/') !== -1) {
            currentDir = ctx.state.currentFile.slice(0, ctx.state.currentFile.lastIndexOf('/') + 1);
        }
        try {
            baseUrl = new URL(currentDir, 'https://markdown-view.local/');
            resolvedUrl = new URL(href, baseUrl);
        }
        catch (error) {
            console.warn('[markdown-view] Markdownリンク解決に失敗:', error);
            return null;
        }
        relativePath = resolvedUrl.pathname.replace(/^\/+/, '');
        try {
            relativePath = decodeURIComponent(relativePath);
        }
        catch (error) {
            console.warn('[markdown-view] リンクパスのデコードに失敗:', relativePath, error);
        }
        if (!/\.md$/i.test(relativePath)) {
            return null;
        }
        return {
            file: relativePath,
            hash: resolvedUrl.hash || ''
        };
    }
    function parseLineHash(hash) {
        var empty = { headingId: null, lineRange: null };
        if (!hash || hash.charAt(0) !== '#')
            return empty;
        var raw = hash.slice(1);
        var decoded;
        try {
            decoded = decodeURIComponent(raw);
        }
        catch (error) {
            console.warn('[markdown-view] hash のデコードに失敗したため raw fragment を使用します。', {
                hash: hash,
                error: error instanceof Error ? error.message : String(error)
            });
            decoded = raw;
        }
        if (!decoded)
            return empty;
        // `heading-id:L5` または `heading-id:L5-L7`
        // `(.*)` は greedy だが、現状 slugify は `:` を除去するため heading_id に `:` は含まれない。
        // slugify 仕様が変わる場合はここの分割戦略を見直すこと
        var combined = decoded.match(/^(.*):L(\d+)(?:-L(\d+))?$/);
        if (combined) {
            var start = parseInt(combined[2], 10);
            var end = combined[3] ? parseInt(combined[3], 10) : start;
            return {
                headingId: combined[1] || null,
                lineRange: { start: start, end: end }
            };
        }
        // `L5` または `L5-L7` 単独
        var lineOnly = decoded.match(/^L(\d+)(?:-L(\d+))?$/);
        if (lineOnly) {
            var s = parseInt(lineOnly[1], 10);
            var e = lineOnly[2] ? parseInt(lineOnly[2], 10) : s;
            return { headingId: null, lineRange: { start: s, end: e } };
        }
        return { headingId: decoded, lineRange: null };
    }
    /// 旧形式メモ互換: リンク直後の兄弟テキストノードが `L5` / `L5-L7` と空白のみで構成される場合、
    /// その行範囲を既存 hash に `:L5-L7` として合成して返す。
    /// Why: 新形式（href fragment 内 `:L5-L7`）にフォーマット移行する前に生成された旧形式 citation
    /// （`出典: [link](url) L15` の散文配置）を既存資産を書き換えずに救済する。
    /// `#memo-preview` 配下のリンクに限定することで、ユーザーが本文に書いた `[spec](spec.md) L5 ...` の
    /// ような自然文リンクを誤ジャンプ対象にしない。
    /// 新形式（行範囲を既に含む hash）や行範囲情報が無い場合は hash をそのまま返す。
    /// renderer がソース行トラッキング用に text を `<span>` でラップするケースに対応するため、
    /// TEXT_NODE と ELEMENT_NODE の双方で `textContent` を見る。
    function augmentHashWithTrailingLineHint(link, hash) {
        if (!link || !link.closest || !link.closest('#memo-preview'))
            return hash;
        var sibling = link.nextSibling;
        if (!sibling)
            return hash;
        if (sibling.nodeType !== Node.TEXT_NODE && sibling.nodeType !== Node.ELEMENT_NODE)
            return hash;
        if (parseLineHash(hash).lineRange)
            return hash;
        // 両端アンカー `^\s*...\s*$` で sibling textContent 全体が行番号トークンのみで構成されることを要求。
        // これにより `L10 onwards...` の散文や `L5abc` の別トークン連続を augment 対象から除外する
        var match = (sibling.textContent || '').match(/^\s*L(\d+)(?:-L(\d+))?\s*$/);
        if (!match)
            return hash;
        var start = parseInt(match[1], 10);
        var end = match[2] ? parseInt(match[2], 10) : start;
        // end < start（逆転）および end == start（単一行）はどちらも start 1 行として扱う
        var suffix = end > start ? 'L' + start + '-L' + end : 'L' + start;
        if (!hash || hash === '#')
            return '#' + suffix;
        return hash + ':' + suffix;
    }
    function scrollToLineRange(targetLine, behavior) {
        // 行番号は renderer 側で 1-indexed。0 以下や非数値は無効として早期return
        if (!ctx.elements.contentRoot || typeof targetLine !== 'number' || targetLine < 1)
            return false;
        var blocks = ctx.elements.contentRoot.querySelectorAll('[data-line-block]');
        // 候補から「最狭マッチ（最深containment）」を選ぶ。
        // <ul>(L5-L20) と <li>(L7-L7) が共に line 7 を含むとき、最狭の <li> を選ぶ。
        // 広いコンテナを選ぶと対象行ではなくコンテナ先頭へスクロールしてしまうため。
        // 同値スパン（ネストblockquote内の単独<p>など）では `<=` 比較で DOM 深い側を優先する
        var best = null;
        var bestSpan = Infinity;
        for (var i = 0; i < blocks.length; i++) {
            var block = blocks[i];
            if (!block)
                continue;
            // block コンテナは data-line-block-start/end、heading/code-block は data-source-* から範囲を読む
            var startAttr = block.getAttribute('data-line-block-start');
            if (startAttr === null)
                startAttr = block.getAttribute('data-source-start-line');
            var endAttr = block.getAttribute('data-line-block-end');
            if (endAttr === null)
                endAttr = block.getAttribute('data-source-end-line');
            if (startAttr === null || endAttr === null)
                continue;
            var s = parseInt(startAttr, 10);
            var e = parseInt(endAttr, 10);
            if (isNaN(s) || isNaN(e))
                continue;
            if (s <= targetLine && e >= targetLine) {
                var span = e - s;
                if (span <= bestSpan) {
                    bestSpan = span;
                    best = block;
                }
            }
        }
        if (!best)
            return false;
        best.scrollIntoView({ block: 'start', behavior: behavior || 'auto' });
        triggerJumpHighlight(best);
        return true;
    }
    function triggerJumpHighlight(el) {
        if (!el)
            return;
        el.classList.remove('jump-highlight');
        // CSS animationを再起動するための強制reflow（class再付与前にlayoutをflushする定番技法）
        void el.offsetWidth;
        el.classList.add('jump-highlight');
        // { once: true } でリスナー自動除去。連続クリック時の leak を防ぐ
        el.addEventListener('animationend', function () {
            el.classList.remove('jump-highlight');
        }, { once: true });
    }
    function applyContentAnchorNavigation(hash, replace) {
        if (!hash || hash.charAt(0) !== '#')
            return false;
        var parsed = parseLineHash(hash);
        // ユーザクリック由来 (replace=false) は smooth、履歴復元 (replace=true) は auto で即着地
        var scrollBehavior = replace ? 'auto' : 'smooth';
        // 行範囲があれば優先（より詳細な位置へジャンプ）
        if (parsed.lineRange && scrollToLineRange(parsed.lineRange.start, scrollBehavior)) {
            if (parsed.headingId) {
                deps.markPendingTocNavigation(parsed.headingId);
            }
            setLocationHash(hash, replace);
            return true;
        }
        // 見出しIDへのフォールバックジャンプ
        if (parsed.headingId) {
            var targetEl = document.getElementById(parsed.headingId);
            if (!targetEl)
                return false;
            deps.markPendingTocNavigation(parsed.headingId);
            setLocationHash(hash, replace);
            targetEl.scrollIntoView({ block: 'start', behavior: scrollBehavior });
            return true;
        }
        return false;
    }
    function restoreContentNavigationFromLocation() {
        var hash = location.hash || '';
        requestAnimationFrame(function () {
            if (hash && applyContentAnchorNavigation(hash, true)) {
                return;
            }
            if (hash) {
                console.warn('[markdown-view] 履歴復元時に見出しが見つかりません:', hash);
            }
            window.scrollTo(0, 0);
            deps.clearPendingTocNavigation();
            deps.restoreActiveTocHeading('');
        });
    }
    /// 内部リンク（相対 .md / `?file=foo.md#hash` / 同一path+hash）のクリックを処理する共通ハンドラ。
    /// `#content` と `#memo-preview` の両方からの delegation で使う。
    function handleInternalLinkClick(event) {
        var eventTarget = event.target instanceof Element ? event.target : null;
        var link = eventTarget ? eventTarget.closest('a[href]') : null;
        if (!link || isModifiedClick(event))
            return;
        if (link.hasAttribute('download') || (link.target && link.target !== '_self'))
            return;
        var href = link.getAttribute('href') || '';
        // 既存relative resolverを優先、ヒットしなければ `?file=` / 同一path系で再試行
        var linkTarget = resolveMarkdownLinkTarget(href) || resolveFileQueryHref(href);
        if (!linkTarget)
            return;
        // 旧形式メモ互換（リンク外 `L5-L7` を hash fragment に取り込む）
        linkTarget.hash = augmentHashWithTrailingLineHint(link, linkTarget.hash);
        if (linkTarget.file === ctx.state.currentFile) {
            event.preventDefault();
            if (linkTarget.hash) {
                if (applyContentAnchorNavigation(linkTarget.hash, false)) {
                    return;
                }
                console.warn('[markdown-view] 同一ファイル内の見出しが見つかりません:', linkTarget.hash);
            }
            deps.setFileParam(ctx.state.currentFile, false, '');
            restoreContentNavigationFromLocation();
            return;
        }
        event.preventDefault();
        deps.selectFile(linkTarget.file, true, {
            scrollMode: linkTarget.hash ? 'none' : 'reset',
            anchorHash: linkTarget.hash,
            historyHash: linkTarget.hash || ''
        });
    }
    function setupContentLinkNavigation() {
        if (!ctx.elements.contentRoot)
            return;
        ctx.elements.contentRoot.addEventListener('click', handleInternalLinkClick);
    }
    function setupMemoLinkNavigation() {
        if (!ctx.elements.memoPreviewEl)
            return;
        ctx.elements.memoPreviewEl.addEventListener('click', handleInternalLinkClick);
    }
    return {
        augmentHashWithTrailingLineHint: augmentHashWithTrailingLineHint,
        applyContentAnchorNavigation: applyContentAnchorNavigation,
        restoreContentNavigationFromLocation: restoreContentNavigationFromLocation,
        setupContentLinkNavigation: setupContentLinkNavigation,
        setupMemoLinkNavigation: setupMemoLinkNavigation,
        setLocationHash: setLocationHash
    };
}
