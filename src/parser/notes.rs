//! Free-text note bodies.
//!
//! Port of `read_notes`, `read_notes_db` and `Mutil.strip_all_trailing_spaces`.
//!
//! Note bodies are read with the raw line reader, never the block one: their content
//! legitimately starts with `#`, and their blank lines are part of the text.

use crate::error::{GwError, GwErrorKind, Result};
use crate::parser::LineReader;

/// Normalises a note body, as `Mutil.strip_all_trailing_spaces` does.
///
/// Drops every carriage return, removes spaces and tabs that sit at the end of a line,
/// and trims trailing whitespace from the text as a whole. Spaces elsewhere are kept.
#[must_use]
pub fn strip_all_trailing_spaces(s: &str) -> String {
    let bytes = s.as_bytes();
    // Everything past the last non-whitespace byte goes.
    let end = bytes
        .iter()
        .rposition(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .map_or(0, |i| i + 1);

    let mut out = String::with_capacity(end);
    let mut i = 0;
    while i < end {
        match bytes[i] {
            b'\r' => i += 1,
            b' ' | b'\t' => {
                // Look past the run of blanks: if it ends at a newline, the whole run is
                // trailing whitespace and is dropped.
                let mut j = i + 1;
                while j < end && matches!(bytes[j], b' ' | b'\t' | b'\r') {
                    j += 1;
                }
                if j < end && bytes[j] == b'\n' {
                    i = j;
                } else if j >= end {
                    break;
                } else {
                    out.push(char::from(bytes[i]));
                    i += 1;
                }
            }
            _ => {
                // Copy a whole character so multi-byte sequences stay intact.
                let c = s[i..].chars().next().unwrap_or('\u{FFFD}');
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

fn eof(expected: &'static str) -> GwError {
    GwError::at_line(0, GwErrorKind::UnexpectedEof { expected })
}

/// Reads a person's `notes` body, up to `end notes`.
///
/// # Errors
///
/// Returns an error if the file ends before the terminator.
pub fn read_notes(reader: &mut LineReader<'_>) -> Result<String> {
    let mut text = String::new();
    loop {
        let line = reader.next_raw_line().ok_or_else(|| eof("end notes"))?;
        if line.raw == "end notes" {
            return Ok(strip_all_trailing_spaces(&text));
        }
        text.push_str(&line.raw);
        text.push('\n');
    }
}

/// Reads a `notes-db`, `page-ext` or `wizard-note` body, up to `terminator`.
///
/// These bodies are written indented by two spaces, which is stripped here.
///
/// # Errors
///
/// Returns an error if the file ends before the terminator.
pub fn read_notes_db(reader: &mut LineReader<'_>, terminator: &'static str) -> Result<String> {
    let mut text = String::new();
    loop {
        let line = reader.next_raw_line().ok_or_else(|| eof(terminator))?;
        if line.raw == terminator {
            return Ok(strip_all_trailing_spaces(&text));
        }
        // Strictly more than two bytes, as GeneWeb has it: a line of exactly two spaces
        // is left alone rather than becoming empty.
        let body = if line.raw.len() > 2 && line.raw.starts_with("  ") {
            &line.raw[2..]
        } else {
            &line.raw
        };
        text.push_str(body);
        text.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_blanks_are_stripped_per_line_and_overall() {
        assert_eq!(strip_all_trailing_spaces("a  \nb\t\nc"), "a\nb\nc");
        assert_eq!(strip_all_trailing_spaces("a\n\n  \n"), "a");
        assert_eq!(strip_all_trailing_spaces(""), "");
    }

    #[test]
    fn interior_spaces_survive() {
        assert_eq!(strip_all_trailing_spaces("a  b\nc d"), "a  b\nc d");
    }

    #[test]
    fn carriage_returns_are_dropped() {
        assert_eq!(strip_all_trailing_spaces("a\r\nb\r\n"), "a\nb");
    }

    #[test]
    fn multibyte_text_survives() {
        assert_eq!(strip_all_trailing_spaces("déjà décédé  \n"), "déjà décédé");
    }

    #[test]
    fn person_notes_stop_at_the_terminator() {
        let mut r = LineReader::new(b"line one\nline two\nend notes\nafter\n");
        assert_eq!(read_notes(&mut r).unwrap(), "line one\nline two");
        assert_eq!(r.next_raw_line().unwrap().raw, "after");
    }

    #[test]
    fn note_bodies_keep_hashes_and_blank_lines() {
        let mut r = LineReader::new(b"# not a comment here\n\ntext\nend notes\n");
        assert_eq!(read_notes(&mut r).unwrap(), "# not a comment here\n\ntext");
    }

    #[test]
    fn database_notes_are_dedented_by_two_spaces() {
        let mut r = LineReader::new(b"  indented\n    more\nend notes-db\n");
        assert_eq!(
            read_notes_db(&mut r, "end notes-db").unwrap(),
            "indented\n  more"
        );
    }

    #[test]
    fn a_line_of_exactly_two_spaces_is_not_dedented() {
        // `len > 2` in GeneWeb, not `>= 2`. The line survives as two spaces, which the
        // trailing-space pass then removes.
        let mut r = LineReader::new(b"  \na\nend notes-db\n");
        assert_eq!(read_notes_db(&mut r, "end notes-db").unwrap(), "\na");
    }

    #[test]
    fn an_unterminated_body_is_an_error() {
        let mut r = LineReader::new(b"text\n");
        assert!(read_notes(&mut r).is_err());
        let mut r = LineReader::new(b"text\n");
        assert!(read_notes_db(&mut r, "end notes-db").is_err());
    }
}
