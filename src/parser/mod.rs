//! The `.gw` block reader.

pub mod block;
pub mod events;
pub mod family;
pub mod notes;
pub mod person;
pub mod relations;

use crate::encoding::{split_lines, Encoding};
use crate::lexer::{fields, Cursor};
use person::LineCtx;

/// One physical line, decoded and tokenized.
#[derive(Debug, Clone)]
pub struct Line {
    /// 1-based line number.
    pub no: usize,
    /// The line as decoded, before tokenization.
    pub raw: String,
    /// The line's tokens.
    pub tokens: Vec<String>,
}

impl Line {
    /// A cursor over this line's tokens.
    #[must_use]
    pub fn cursor(&self) -> Cursor<'_> {
        Cursor::new(&self.tokens)
    }

    /// This line's identity, for error reporting.
    #[must_use]
    pub fn ctx(&self) -> LineCtx<'_> {
        LineCtx {
            no: self.no,
            raw: &self.raw,
        }
    }

    /// The first token, or `""` for a blank line.
    #[must_use]
    pub fn keyword(&self) -> &str {
        self.tokens.first().map_or("", String::as_str)
    }
}

/// A cursor over a file's physical lines.
///
/// # Two ways to read a line
///
/// GeneWeb reads lines through two different functions, and the difference is not
/// cosmetic:
///
/// - [`LineReader::next_block_line`] (`input_real_line`) skips blank lines and lines
///   starting with `#`, which are comments **at block level**.
/// - [`LineReader::next_raw_line`] (`input_a_line`) returns every line untouched.
///
/// Event and note bodies must use the raw form, because their own content starts with
/// `#` — `#marr` is an event name, not a comment — and because a note's blank lines are
/// part of the note. Using the wrong one silently eats data instead of failing.
pub struct LineReader<'a> {
    lines: Vec<&'a [u8]>,
    pos: usize,
    encoding: Encoding,
}

impl<'a> LineReader<'a> {
    /// Opens a reader over raw file bytes.
    ///
    /// The encoding starts as ISO-8859-1, the format's default, and is switched by an
    /// `encoding: utf-8` directive as the file is read — which is why decoding happens
    /// per line rather than up front.
    #[must_use]
    pub fn new(input: &'a [u8]) -> Self {
        Self {
            lines: split_lines(input),
            pos: 0,
            encoding: Encoding::default(),
        }
    }

    /// The encoding currently in force.
    #[must_use]
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Switches the encoding for every line read from now on.
    pub fn set_encoding(&mut self, encoding: Encoding) {
        self.encoding = encoding;
    }

    /// The 1-based number of the line most recently returned.
    #[must_use]
    pub fn line_no(&self) -> usize {
        self.pos
    }

    /// Reads the next line verbatim, including blanks and `#` comments.
    pub fn next_raw_line(&mut self) -> Option<Line> {
        let bytes = *self.lines.get(self.pos)?;
        self.pos += 1;
        let raw = self.encoding.decode(bytes);
        let tokens = fields(&raw);
        Some(Line {
            no: self.pos,
            raw,
            tokens,
        })
    }

    /// Reads the next line that begins a block, skipping blanks and `#` comments.
    pub fn next_block_line(&mut self) -> Option<Line> {
        loop {
            let line = self.next_raw_line()?;
            if !line.raw.is_empty() && !line.raw.starts_with('#') {
                return Some(line);
            }
        }
    }

    /// Returns the next block line without consuming it.
    ///
    /// The optional sections of a `fam` block are recognised by looking at the next line
    /// and backing off when it belongs to a later section.
    pub fn peek_block_line(&mut self) -> Option<Line> {
        let line = self.next_block_line();
        if line.is_some() {
            self.unread();
        }
        line
    }

    /// Puts the most recently read line back, so the next read returns it again.
    ///
    /// One slot is enough: the grammar never looks more than one line ahead.
    pub fn unread(&mut self) {
        self.pos = self.pos.saturating_sub(1);
    }

    /// Whether every line has been consumed.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.pos >= self.lines.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_reading_skips_comments_and_blanks() {
        let mut r = LineReader::new(b"# a comment\n\nfam A B\n");
        let line = r.next_block_line().expect("a block line");
        assert_eq!(line.keyword(), "fam");
        assert_eq!(line.no, 3);
        assert!(r.next_block_line().is_none());
    }

    #[test]
    fn raw_reading_keeps_comments_and_blanks() {
        let mut r = LineReader::new(b"#marr\n\nnote x\n");
        assert_eq!(r.next_raw_line().unwrap().raw, "#marr");
        assert_eq!(r.next_raw_line().unwrap().raw, "");
        assert_eq!(r.next_raw_line().unwrap().raw, "note x");
        assert!(r.next_raw_line().is_none());
    }

    #[test]
    fn unread_replays_one_line() {
        let mut r = LineReader::new(b"a\nb\n");
        assert_eq!(r.next_raw_line().unwrap().raw, "a");
        r.unread();
        assert_eq!(r.next_raw_line().unwrap().raw, "a");
        assert_eq!(r.next_raw_line().unwrap().raw, "b");
    }

    #[test]
    fn the_encoding_switch_applies_from_the_next_line() {
        // Byte 0xE9 is `é` in Latin-1 and invalid alone in UTF-8.
        let mut r = LineReader::new(b"caf\xE9\ncaf\xC3\xA9\n");
        assert_eq!(r.next_raw_line().unwrap().raw, "café");
        r.set_encoding(Encoding::Utf8);
        assert_eq!(r.next_raw_line().unwrap().raw, "café");
    }
}
