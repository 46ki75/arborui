use unicode_segmentation::{GraphemeIndices, UnicodeSegmentation};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The policy used to estimate a grapheme's terminal display width.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WidthPolicy {
    /// Use sequence-aware Unicode width with ambiguous characters kept narrow.
    #[default]
    Unicode,
    /// Use sequence-aware Unicode width with ambiguous characters made wide.
    Cjk,
    /// Sum code point widths for compatibility with traditional `wcwidth` behavior.
    WcWidth,
}

/// A borrowed extended grapheme cluster and its measured width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Grapheme<'a> {
    /// UTF-8 byte offset in the source string.
    pub byte_offset: usize,
    /// Borrowed grapheme text.
    pub text: &'a str,
    /// Estimated terminal width in cells.
    pub width: usize,
}

/// An iterator over extended grapheme clusters and their measured widths.
#[derive(Clone, Debug)]
pub struct Graphemes<'a> {
    inner: GraphemeIndices<'a>,
    policy: WidthPolicy,
}

impl<'a> Iterator for Graphemes<'a> {
    type Item = Grapheme<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(byte_offset, text)| Grapheme {
            byte_offset,
            text,
            width: grapheme_width(text, self.policy),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl DoubleEndedIterator for Graphemes<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.inner.next_back().map(|(byte_offset, text)| Grapheme {
            byte_offset,
            text,
            width: grapheme_width(text, self.policy),
        })
    }
}

/// Measurements for a possibly multiline string.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TextMetrics {
    /// Maximum display width of any logical line.
    pub width: usize,
    /// Number of logical lines, including an empty trailing line.
    pub height: usize,
}

/// Iterates over the extended grapheme clusters in `text`.
#[must_use]
pub fn graphemes(text: &str, policy: WidthPolicy) -> Graphemes<'_> {
    Graphemes {
        inner: text.grapheme_indices(true),
        policy,
    }
}

/// Estimates the display width of one grapheme cluster.
///
/// Line separators and tabs have width zero. Higher layers handle line breaks
/// and tab expansion because their widths depend on layout context.
#[must_use]
pub fn grapheme_width(grapheme: &str, policy: WidthPolicy) -> usize {
    if grapheme
        .chars()
        .any(|character| matches!(character, '\r' | '\n' | '\t'))
    {
        return 0;
    }

    match policy {
        WidthPolicy::Unicode => UnicodeWidthStr::width(grapheme),
        WidthPolicy::Cjk => UnicodeWidthStr::width_cjk(grapheme),
        WidthPolicy::WcWidth => grapheme
            .chars()
            .map(|character| UnicodeWidthChar::width(character).unwrap_or(0))
            .sum(),
    }
}

/// Measures a possibly multiline string.
///
/// Empty text occupies one empty logical line. Both LF and CRLF create one new
/// line; standalone carriage returns are also treated as line separators.
#[must_use]
pub fn measure(text: &str, policy: WidthPolicy) -> TextMetrics {
    let mut width = 0;
    let mut line_width = 0;
    let mut height = 1;

    for grapheme in graphemes(text, policy) {
        if grapheme
            .text
            .chars()
            .any(|character| matches!(character, '\r' | '\n'))
        {
            width = width.max(line_width);
            line_width = 0;
            height += 1;
        } else {
            line_width += grapheme.width;
        }
    }

    TextMetrics {
        width: width.max(line_width),
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterates_extended_graphemes_with_byte_offsets() {
        let clusters: Vec<_> = graphemes("a\u{310}👨‍👩‍👧‍👦", WidthPolicy::Unicode).collect();

        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].byte_offset, 0);
        assert_eq!(clusters[0].text, "a\u{310}");
        assert_eq!(clusters[0].width, 1);
        assert_eq!(clusters[1].byte_offset, 3);
        assert_eq!(clusters[1].width, 2);
    }

    #[test]
    fn wcwidth_policy_preserves_codepoint_sum_behavior() {
        let family = "👨‍👩‍👧‍👦";

        assert_eq!(grapheme_width(family, WidthPolicy::Unicode), 2);
        assert!(
            grapheme_width(family, WidthPolicy::WcWidth)
                > grapheme_width(family, WidthPolicy::Unicode)
        );
    }

    #[test]
    fn cjk_policy_makes_ambiguous_characters_wide() {
        assert_eq!(grapheme_width("·", WidthPolicy::Unicode), 1);
        assert_eq!(grapheme_width("·", WidthPolicy::Cjk), 2);
    }

    #[test]
    fn measures_logical_lines_without_counting_newlines() {
        assert_eq!(
            measure("ab\r\n界\n", WidthPolicy::Unicode),
            TextMetrics {
                width: 2,
                height: 3,
            }
        );
        assert_eq!(
            measure("", WidthPolicy::Unicode),
            TextMetrics {
                width: 0,
                height: 1,
            }
        );
    }
}
