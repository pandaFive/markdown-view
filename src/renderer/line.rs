use std::ops::Range;

pub(super) struct LineLookup {
    line_starts: Vec<usize>,
}

impl LineLookup {
    pub(super) fn new(input: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in input.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts }
    }

    pub(super) fn line_for_offset(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(index) => index + 1,
            Err(index) => index,
        }
    }

    pub(super) fn line_range(&self, range: &Range<usize>) -> (usize, usize) {
        if range.is_empty() {
            let line = self.line_for_offset(range.start);
            return (line, line);
        }
        let start_line = self.line_for_offset(range.start);
        let end_line = self.line_for_offset(range.end.saturating_sub(1));
        (start_line, end_line)
    }
}

pub(super) fn source_line_attrs(line_lookup: &LineLookup, range: &Range<usize>) -> String {
    let (start_line, end_line) = line_lookup.line_range(range);
    format!(
        " data-source-start-line=\"{}\" data-source-end-line=\"{}\"",
        start_line, end_line
    )
}

/// block-level コンテナ（<p>, <ul>, <ol>, <li>, <table>, <blockquote>）向けの行範囲属性。
///
/// `data-source-*` は memo quote の集計対象なので、祖先コンテナの範囲を混ぜないため付与しない。
pub(super) fn block_line_attrs(line_lookup: &LineLookup, range: &Range<usize>) -> String {
    let (start_line, end_line) = line_lookup.line_range(range);
    format!(
        " data-line-block data-line-block-start=\"{}\" data-line-block-end=\"{}\"",
        start_line, end_line
    )
}

/// heading / code-block 用: 既存の `source_line_attrs` に `data-line-block` マーカーを前置。
pub(super) fn line_block_marker_with(source_attrs: String) -> String {
    format!(" data-line-block{}", source_attrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_lookup_line_for_offsetは改行境界を正しく返す() {
        let lookup = LineLookup::new("alpha\nbeta\ncharlie");

        assert_eq!(lookup.line_for_offset(0), 1);
        assert_eq!(lookup.line_for_offset(5), 1);
        assert_eq!(lookup.line_for_offset(6), 2);
        assert_eq!(lookup.line_for_offset(10), 2);
        assert_eq!(lookup.line_for_offset(11), 3);
    }

    #[test]
    fn test_line_lookup_line_rangeは複数行範囲を正しく返す() {
        let lookup = LineLookup::new("alpha\nbeta\ncharlie");

        assert_eq!(lookup.line_range(&(0..5)), (1, 1));
        assert_eq!(lookup.line_range(&(0..10)), (1, 2));
        assert_eq!(lookup.line_range(&(6..18)), (2, 3));
        assert_eq!(lookup.line_range(&(6..6)), (2, 2));
    }
}
