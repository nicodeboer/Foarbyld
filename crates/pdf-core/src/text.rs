//! Page text model and search. Pure Rust: no PDFium types, fully unit-testable.

use serde::Serialize;

/// Text of one page with a bounding box per character.
///
/// `text` holds exactly one Rust `char` per PDFium character (including the
/// generated spaces and line breaks PDFium inserts), so a character index is
/// valid in both `text` and `boxes`. `boxes` has four floats per character:
/// `x, y, width, height` in PDF points, origin top-left of the *displayed*
/// (rotated) page. Characters without a glyph box (generated ones) are all zero.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PageText {
    pub text: String,
    pub boxes: Vec<f32>,
}

/// A search hit: `len` characters starting at character index `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SearchMatch {
    pub page: usize,
    pub start: usize,
    pub len: usize,
}

/// Normalise one character for matching. `None` = ignore it entirely
/// (carriage returns, PDFium's soft-hyphen marker U+0002, other controls).
fn norm(c: char, case_sensitive: bool) -> Option<char> {
    match c {
        '\n' | '\t' | '\u{a0}' => Some(' '),
        c if c.is_control() => None,
        c if case_sensitive => Some(c),
        c => c.to_lowercase().next(),
    }
}

/// Normalised characters with runs of whitespace collapsed, each tagged with
/// the index of its (first) source character.
fn normalise(text: impl Iterator<Item = char>, case_sensitive: bool) -> Vec<(char, usize)> {
    let mut out: Vec<(char, usize)> = Vec::new();
    // PDFium marks a hyphen at a line end with U+0002; the word continues on
    // the next line, so the line break after it is not a space.
    let mut after_hyphen = false;
    for (i, c) in text.enumerate() {
        if c == '\u{2}' {
            after_hyphen = true;
            continue;
        }
        let Some(c) = norm(c, case_sensitive) else { continue };
        if after_hyphen && c.is_whitespace() {
            continue;
        }
        after_hyphen = false;
        if c.is_whitespace() && out.last().is_some_and(|(p, _)| *p == ' ') {
            continue;
        }
        out.push((if c.is_whitespace() { ' ' } else { c }, i));
    }
    out
}

/// Find all non-overlapping occurrences of `query` in `text`. Returns
/// `(start, len)` in character indices of `text`. Line breaks and runs of
/// whitespace in the page match a single space in the query.
pub fn find_in_text(text: &str, query: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    let mut q: Vec<char> = normalise(query.chars(), case_sensitive)
        .into_iter()
        .map(|(c, _)| c)
        .collect();
    while q.first() == Some(&' ') {
        q.remove(0);
    }
    while q.last() == Some(&' ') {
        q.pop();
    }
    if q.is_empty() {
        return Vec::new();
    }
    let hay = normalise(text.chars(), case_sensitive);
    let mut hits = Vec::new();
    let mut i = 0;
    while i + q.len() <= hay.len() {
        if hay[i..i + q.len()].iter().map(|(c, _)| c).eq(q.iter()) {
            let start = hay[i].1;
            let end = hay[i + q.len() - 1].1;
            hits.push((start, end - start + 1));
            i += q.len();
        } else {
            i += 1;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_case_insensitive_by_default() {
        assert_eq!(find_in_text("The Quick fox, quick", "quick", false), vec![(4, 5), (15, 5)]);
    }

    #[test]
    fn respects_case_sensitivity() {
        assert_eq!(find_in_text("Quick quick", "quick", true), vec![(6, 5)]);
    }

    #[test]
    fn matches_across_line_breaks_and_soft_hyphens() {
        // PDFium emits "\r\n" for a line break.
        let t = "foo\r\nbar";
        assert_eq!(find_in_text(t, "foo bar", false), vec![(0, 8)]);
        let t = "co\u{2}\r\nop";
        assert_eq!(find_in_text(t, "coop", false), vec![(0, 7)]);
    }

    #[test]
    fn empty_or_blank_query_matches_nothing() {
        assert!(find_in_text("abc", "", false).is_empty());
        assert!(find_in_text("abc", "  ", false).is_empty());
    }

    #[test]
    fn matches_do_not_overlap() {
        assert_eq!(find_in_text("aaaa", "aa", false), vec![(0, 2), (2, 2)]);
    }

    #[test]
    fn indices_count_chars_not_bytes() {
        assert_eq!(find_in_text("é😀x", "x", false), vec![(2, 1)]);
    }
}
