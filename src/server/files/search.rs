use std::ops::Range;
use std::path::Path;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::catalog::list_markdown_files;
use super::content::read_markdown_with_limit;
use super::resolve::resolve_file;
use crate::markdown::{markdown_options, MarkdownProfile};

const MAX_SEARCH_RESULTS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(in crate::server) struct SearchResultItem {
    pub(in crate::server) file: String,
    pub(in crate::server) file_match_index: usize,
    pub(in crate::server) before: String,
    pub(in crate::server) current: String,
    pub(in crate::server) after: String,
}

impl SearchResultItem {
    fn new(
        file: String,
        file_match_index: usize,
        before: String,
        current: String,
        after: String,
    ) -> Self {
        Self {
            file,
            file_match_index,
            before,
            current,
            after,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(in crate::server) struct SearchResponse {
    pub(in crate::server) query: String,
    pub(in crate::server) results: Vec<SearchResultItem>,
    pub(in crate::server) searched_files: usize,
    pub(in crate::server) skipped_files: usize,
}

impl SearchResponse {
    fn empty(query: String) -> Self {
        Self {
            query,
            results: Vec::new(),
            searched_files: 0,
            skipped_files: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct SearchBlockEntry {
    text: String,
    sentences: Vec<Range<usize>>,
}

#[derive(Debug, Clone)]
struct SearchContext {
    before: String,
    current: String,
    after: String,
}

/// ディレクトリ内のMarkdownを横断検索する。
pub(in crate::server) async fn search_directory(
    base_dir: &Path,
    raw_query: &str,
) -> std::io::Result<SearchResponse> {
    let query = raw_query.trim().to_string();
    if query.is_empty() {
        return Ok(SearchResponse::empty(query));
    }

    let files = list_markdown_files(base_dir)?;
    let mut results = Vec::new();
    let mut searched_files = 0;
    let mut skipped_files = 0;

    for relative in files {
        if results.len() >= MAX_SEARCH_RESULTS {
            break;
        }

        let file_path = match resolve_file(base_dir, &relative) {
            Ok(file_path) => file_path,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] 検索対象ファイル解決失敗（スキップ）: {} ({})",
                    relative,
                    error
                );
                skipped_files += 1;
                continue;
            }
        };

        let markdown = match read_markdown_with_limit(&file_path).await {
            Ok(markdown) => markdown,
            Err(error) => {
                tracing::warn!(
                    "[markdown-view] 検索対象ファイル読込失敗（スキップ）: {} ({})",
                    relative,
                    error
                );
                skipped_files += 1;
                continue;
            }
        };

        searched_files += 1;
        let blocks = extract_search_blocks(&markdown);
        let file_results = find_matches_for_file(&relative, &blocks, &query);
        for item in file_results {
            results.push(item);
            if results.len() >= MAX_SEARCH_RESULTS {
                break;
            }
        }
    }

    Ok(SearchResponse {
        query,
        results,
        searched_files,
        skipped_files,
    })
}

fn extract_search_blocks(markdown: &str) -> Vec<SearchBlockEntry> {
    let mut blocks = Vec::new();
    let mut current_block = String::new();
    let mut block_depth = 0usize;
    let mut item_depth = 0usize;
    let mut link_depth = 0usize;
    let mut image_depth = 0usize;
    let mut code_block_depth = 0usize;
    let mut inline_html_depth = 0usize;

    for event in Parser::new_ext(markdown, markdown_options(MarkdownProfile::Search)) {
        match event {
            Event::Start(tag) => {
                if matches!(tag, Tag::Item) {
                    if item_depth == 0 {
                        if block_depth == 0 {
                            current_block.clear();
                        } else if !current_block.ends_with('\n') {
                            current_block.push('\n');
                        }
                        block_depth += 1;
                    }
                    item_depth += 1;
                } else if is_search_block_tag(&tag) {
                    if block_depth == 0 {
                        current_block.clear();
                    } else if !current_block.ends_with('\n') {
                        current_block.push('\n');
                    }
                    block_depth += 1;
                }

                match tag {
                    Tag::Link { .. } => link_depth += 1,
                    Tag::Image { .. } => image_depth += 1,
                    Tag::CodeBlock(_) => code_block_depth += 1,
                    _ => {}
                }
            }
            Event::End(tag) => {
                if matches!(tag, TagEnd::Item) {
                    item_depth = item_depth.saturating_sub(1);
                    if item_depth == 0 {
                        block_depth = block_depth.saturating_sub(1);
                        if block_depth == 0 {
                            finalize_search_block(&mut blocks, &current_block);
                            current_block.clear();
                            inline_html_depth = 0;
                        }
                    }
                } else if is_search_block_end_tag(&tag) {
                    block_depth = block_depth.saturating_sub(1);
                    inline_html_depth = 0;
                    if block_depth == 0 {
                        finalize_search_block(&mut blocks, &current_block);
                        current_block.clear();
                    }
                }

                match tag {
                    TagEnd::Link => link_depth = link_depth.saturating_sub(1),
                    TagEnd::Image => image_depth = image_depth.saturating_sub(1),
                    TagEnd::CodeBlock => code_block_depth = code_block_depth.saturating_sub(1),
                    _ => {}
                }
            }
            Event::Text(text) => {
                if should_capture_text(
                    block_depth,
                    item_depth,
                    link_depth,
                    image_depth,
                    code_block_depth,
                    inline_html_depth,
                ) {
                    current_block.push_str(&text);
                }
            }
            Event::Code(text) => {
                if should_capture_text(
                    block_depth,
                    item_depth,
                    link_depth,
                    image_depth,
                    code_block_depth,
                    inline_html_depth,
                ) {
                    current_block.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if should_capture_text(
                    block_depth,
                    item_depth,
                    link_depth,
                    image_depth,
                    code_block_depth,
                    inline_html_depth,
                ) && !current_block.ends_with('\n')
                {
                    current_block.push('\n');
                }
            }
            Event::Rule => {
                if block_depth > 0 && !current_block.ends_with('\n') {
                    current_block.push('\n');
                }
            }
            Event::InlineHtml(html) => update_inline_html_depth(&mut inline_html_depth, &html),
            Event::Html(_)
            | Event::TaskListMarker(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::FootnoteReference(_) => {}
        }
    }

    if block_depth == 0 {
        finalize_search_block(&mut blocks, &current_block);
    }

    blocks
}

fn should_capture_text(
    block_depth: usize,
    item_depth: usize,
    link_depth: usize,
    image_depth: usize,
    code_block_depth: usize,
    inline_html_depth: usize,
) -> bool {
    block_depth > 0
        && item_depth <= 1
        && link_depth == 0
        && image_depth == 0
        && code_block_depth == 0
        && inline_html_depth == 0
}

fn update_inline_html_depth(depth: &mut usize, html: &str) {
    let trimmed = html.trim();
    if !trimmed.starts_with('<') {
        return;
    }

    if trimmed.starts_with("</") {
        *depth = depth.saturating_sub(1);
        return;
    }

    if trimmed.starts_with("<!") || trimmed.starts_with("<?") || trimmed.ends_with("/>") {
        return;
    }

    if let Some(tag_name) = inline_html_tag_name(trimmed) {
        if !is_void_html_tag(tag_name) {
            *depth += 1;
        }
    }
}

fn inline_html_tag_name(html: &str) -> Option<&str> {
    let without_lt = html.strip_prefix('<')?.trim_start();
    let tag_name_end = without_lt
        .char_indices()
        .find_map(|(index, ch)| (!ch.is_ascii_alphanumeric()).then_some(index))
        .unwrap_or(without_lt.len());

    (tag_name_end > 0).then_some(&without_lt[..tag_name_end])
}

fn is_void_html_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_search_block_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph | Tag::Heading { .. } | Tag::Item | Tag::BlockQuote(_) | Tag::TableCell
    )
}

fn is_search_block_end_tag(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::Item
            | TagEnd::BlockQuote(_)
            | TagEnd::TableCell
    )
}

fn finalize_search_block(blocks: &mut Vec<SearchBlockEntry>, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    blocks.push(SearchBlockEntry {
        text: trimmed.to_string(),
        sentences: split_text_into_sentence_ranges(trimmed),
    });
}

fn find_matches_for_file(
    file: &str,
    blocks: &[SearchBlockEntry],
    query: &str,
) -> Vec<SearchResultItem> {
    let mut results = Vec::new();
    let normalized_query = query.to_lowercase();
    let mut file_match_index = 0usize;

    for (block_index, block) in blocks.iter().enumerate() {
        if block.text.is_empty() {
            continue;
        }

        let normalized = build_case_fold_index(&block.text);
        let mut search_start = 0usize;

        while search_start <= normalized.normalized_text.len() {
            let Some(relative_index) =
                normalized.normalized_text[search_start..].find(&normalized_query)
            else {
                break;
            };
            let normalized_match_start = search_start + relative_index;
            let normalized_match_end = normalized_match_start + normalized_query.len();
            let match_start = normalized.original_offset(normalized_match_start);
            let match_end = normalized.original_offset(normalized_match_end);
            let context = build_search_context(blocks, block_index, match_start, match_end);
            results.push(SearchResultItem::new(
                file.to_string(),
                file_match_index,
                context.before,
                context.current,
                context.after,
            ));
            file_match_index += 1;
            search_start = normalized_match_end;
        }
    }

    results
}

#[derive(Debug, Clone)]
struct CaseFoldIndex {
    normalized_text: String,
    original_offsets: Vec<usize>,
}

impl CaseFoldIndex {
    fn original_offset(&self, normalized_offset: usize) -> usize {
        self.original_offsets
            .get(normalized_offset)
            .copied()
            .unwrap_or_else(|| self.original_offsets.last().copied().unwrap_or(0))
    }
}

fn build_case_fold_index(text: &str) -> CaseFoldIndex {
    let mut normalized_text = String::new();
    let mut original_offsets = vec![0];

    for (char_index, ch) in text.char_indices() {
        let char_end = char_index + ch.len_utf8();
        let folded = ch.to_lowercase().collect::<String>();
        normalized_text.push_str(&folded);
        for _ in 0..folded.len() {
            original_offsets.push(char_end);
        }
    }

    CaseFoldIndex {
        normalized_text,
        original_offsets,
    }
}

fn build_search_context(
    blocks: &[SearchBlockEntry],
    block_index: usize,
    match_start: usize,
    match_end: usize,
) -> SearchContext {
    let block = &blocks[block_index];
    let sentence_index = get_sentence_for_match(&block.sentences, match_start, match_end)
        .unwrap_or(if block.sentences.is_empty() { -1 } else { 0 });
    let current = if sentence_index >= 0 {
        let range = &block.sentences[sentence_index as usize];
        block.text[range.start..range.end].to_string()
    } else {
        block.text.trim().to_string()
    };

    SearchContext {
        before: get_adjacent_sentence(blocks, block_index, sentence_index, -1),
        current,
        after: get_adjacent_sentence(blocks, block_index, sentence_index, 1),
    }
}

fn get_sentence_for_match(
    sentences: &[Range<usize>],
    match_start: usize,
    match_end: usize,
) -> Option<isize> {
    for (index, sentence) in sentences.iter().enumerate() {
        if match_start < sentence.end && match_end > sentence.start {
            return Some(index as isize);
        }
    }
    None
}

fn get_adjacent_sentence(
    blocks: &[SearchBlockEntry],
    block_index: usize,
    sentence_index: isize,
    direction: isize,
) -> String {
    let mut target_block_index = block_index as isize;
    let mut target_sentence_index = sentence_index + direction;

    while target_block_index >= 0 && target_block_index < blocks.len() as isize {
        let entry = &blocks[target_block_index as usize];
        if target_sentence_index >= 0 && (target_sentence_index as usize) < entry.sentences.len() {
            let range = &entry.sentences[target_sentence_index as usize];
            return entry.text[range.start..range.end].to_string();
        }

        target_block_index += direction;
        if target_block_index < 0 || target_block_index >= blocks.len() as isize {
            break;
        }
        target_sentence_index = if direction > 0 {
            0
        } else {
            blocks[target_block_index as usize].sentences.len() as isize - 1
        };
    }

    String::new()
}

fn split_text_into_sentence_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut sentence_start = 0usize;

    for (index, ch) in text.char_indices() {
        if ch != '\n' && !is_sentence_boundary(ch) {
            continue;
        }

        let end = index + ch.len_utf8();
        if let Some(range) = trim_sentence_range(text, sentence_start, end) {
            ranges.push(range);
        }
        sentence_start = end;
    }

    if sentence_start < text.len() {
        if let Some(range) = trim_sentence_range(text, sentence_start, text.len()) {
            ranges.push(range);
        }
    }

    if ranges.is_empty() {
        if let Some(range) = trim_sentence_range(text, 0, text.len()) {
            ranges.push(range);
        }
    }

    ranges
}

fn is_sentence_boundary(ch: char) -> bool {
    matches!(ch, '.' | '!' | '?' | '。' | '！' | '？')
}

fn trim_sentence_range(text: &str, start: usize, end: usize) -> Option<Range<usize>> {
    let mut next_start = start;
    let mut next_end = end;

    while next_start < next_end {
        let ch = text[next_start..next_end].chars().next()?;
        if ch.is_whitespace() {
            next_start += ch.len_utf8();
        } else {
            break;
        }
    }

    while next_end > next_start {
        let ch = text[..next_end].chars().next_back()?;
        if ch.is_whitespace() {
            next_end -= ch.len_utf8();
        } else {
            break;
        }
    }

    (next_end > next_start).then_some(next_start..next_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_search_blocks_リンクとコードブロックを除外し_inline_codeを含める() {
        let blocks = extract_search_blocks(
            "# Title\n\nAlpha [hidden link](https://example.com) visible.\n\n`cargo test`\n\n```rust\nhidden code\n```",
        );

        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].text, "Title");
        assert_eq!(blocks[1].text, "Alpha  visible.");
        assert_eq!(blocks[2].text, "cargo test");
    }

    #[test]
    fn test_extract_search_blocks_脚注参照ラベルを検索対象に含めない() {
        let blocks = extract_search_blocks(
            "Paragraph with footnote.[^note]\n\n[^note]: hidden footnote body",
        );

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "Paragraph with footnote.");
        assert_eq!(blocks[1].text, "hidden footnote body");
    }

    #[test]
    fn test_search_profileは脚注定義本文を検索ブロック化する() {
        let blocks = extract_search_blocks("本文です。[^note]\n\n[^note]: 検索専用の脚注本文");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "本文です。");
        assert_eq!(blocks[1].text, "検索専用の脚注本文");
    }

    #[test]
    fn test_search_profileはheading_attributes付き見出しの本文を検索対象にする() {
        let blocks = extract_search_blocks("# 表示見出し {#custom-id}");

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "表示見出し");
    }

    #[test]
    fn test_search_profileでもraw_htmlとinline_htmlは検索対象にしない() {
        let blocks = extract_search_blocks(
            "<section>hidden html</section>\n\n本文 visible <span>hidden inline</span>",
        );

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "本文 visible");
    }

    #[test]
    fn test_search_profileはvoidタグ後の同段落テキストを検索対象に含める() {
        let blocks = extract_search_blocks("before <br> after\n\nimage <img src=\"x\"> tail");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "before  after");
        assert_eq!(blocks[1].text, "image  tail");
    }

    #[test]
    fn test_search_profileは自閉じタグ後の段落内テキストを検索対象に含める() {
        let blocks = extract_search_blocks("before <custom/> after");

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "before  after");
    }

    #[test]
    fn test_search_profileはブロックhtml後の段落を検索対象にする() {
        let blocks = extract_search_blocks("<section>hidden html</section>\n\n次の段落 visible");

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "次の段落 visible");
    }

    #[test]
    fn test_search_profileは未閉鎖inline_html後の次段落を検索対象にする() {
        let blocks = extract_search_blocks("本文 <span>hidden\n\n次の段落 visible");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "本文");
        assert_eq!(blocks[1].text, "次の段落 visible");
    }

    #[test]
    fn test_search_profileはlist_item内の未閉鎖inline_html後の段落を検索対象にする() {
        let blocks = extract_search_blocks("- first <span>hidden\n\n  second visible");

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "first \nsecond visible");
    }

    #[test]
    fn test_search_profileはlist_item内の未閉鎖inline_html後にlist外段落を検索対象にする() {
        let blocks = extract_search_blocks("- first <span>hidden\n\noutside visible");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "first");
        assert_eq!(blocks[1].text, "outside visible");
    }

    #[test]
    fn test_extract_search_blocks_ネストしたリスト項目は親項目へ混ぜない() {
        let blocks = extract_search_blocks("- parent\n  - child alpha\n- sibling beta");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "parent");
        assert_eq!(blocks[1].text, "sibling beta");
    }

    #[test]
    fn test_find_matches_for_file_ローカル一致番号を維持する() {
        let blocks = vec![
            SearchBlockEntry {
                text: "Alpha note here. Alpha note again.".to_string(),
                sentences: split_text_into_sentence_ranges("Alpha note here. Alpha note again."),
            },
            SearchBlockEntry {
                text: "Alpha note third.".to_string(),
                sentences: split_text_into_sentence_ranges("Alpha note third."),
            },
        ];

        let results = find_matches_for_file("README.md", &blocks, "alpha note");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].file_match_index, 0);
        assert_eq!(results[1].file_match_index, 1);
        assert_eq!(results[2].file_match_index, 2);
    }

    #[test]
    fn test_find_matches_for_file_unicode小文字化でバイト長が変わっても安全に一致する() {
        let text = "İstanbul is here. Another line.";
        let blocks = vec![SearchBlockEntry {
            text: text.to_string(),
            sentences: split_text_into_sentence_ranges(text),
        }];

        let results = find_matches_for_file("README.md", &blocks, "i̇stanbul");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_match_index, 0);
        assert_eq!(results[0].current, "İstanbul is here.");
    }
}
