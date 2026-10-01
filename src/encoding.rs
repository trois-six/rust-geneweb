//! Character encoding and physical line handling for `.gw` files.
//!
//! A `.gw` file is ISO-8859-1 by default. A file may opt into UTF-8 with an
//! `encoding: utf-8` directive, which GeneWeb treats as a block: everything read *after*
//! it is decoded as UTF-8. `GwReader` therefore carries a current [`Encoding`] and
//! switches it when the directive is seen, rather than sniffing the whole file up front.
//!
//! See `input_line0` / `input_a_line` in GeneWeb's `bin/gwc/gwcomp.ml`.

/// The encoding a `.gw` file's bytes are interpreted with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    /// ISO-8859-1 (Latin-1). The format's default when no directive is present.
    #[default]
    Iso8859_1,
    /// UTF-8, selected by an `encoding: utf-8` directive.
    Utf8,
}

impl Encoding {
    /// Decodes one line of source bytes into a `String`.
    ///
    /// Never fails: invalid UTF-8 is replaced with U+FFFD so that a malformed file
    /// produces a parse error rather than a panic.
    #[must_use]
    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            // Every ISO-8859-1 byte is the Unicode code point of the same value.
            Self::Iso8859_1 => bytes.iter().map(|&b| char::from(b)).collect(),
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        }
    }
}

/// The UTF-8 byte order mark.
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Strips a leading UTF-8 byte order mark, if present.
#[must_use]
pub fn strip_bom(input: &[u8]) -> &[u8] {
    input.strip_prefix(BOM).unwrap_or(input)
}

/// Splits raw file bytes into physical lines, dropping the line terminators.
///
/// Handles LF and CRLF; a lone trailing `\r` is stripped from each line, matching
/// `input_line0`. A trailing newline at end of file does not yield a final empty line.
#[must_use]
pub fn split_lines(input: &[u8]) -> Vec<&[u8]> {
    let input = strip_bom(input);
    if input.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&[u8]> = input.split(|&b| b == b'\n').collect();
    // `split` yields a trailing empty slice when the input ends with `\n`; that is the
    // absence of a further line, not an empty one.
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    for line in &mut lines {
        if let [rest @ .., b'\r'] = *line {
            *line = rest;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_8859_1_maps_bytes_to_code_points() {
        // 0xE9 is `é` in Latin-1.
        assert_eq!(Encoding::Iso8859_1.decode(b"caf\xE9"), "café");
    }

    #[test]
    fn utf_8_decodes_natively_and_never_panics() {
        assert_eq!(Encoding::Utf8.decode("café".as_bytes()), "café");
        // A bare 0xE9 is not valid UTF-8; it must degrade, not panic.
        assert!(Encoding::Utf8.decode(b"caf\xE9").contains('\u{FFFD}'));
    }

    #[test]
    fn splits_lf_and_crlf_and_strips_bom() {
        assert_eq!(split_lines(b"a\nb\n"), vec![&b"a"[..], &b"b"[..]]);
        assert_eq!(split_lines(b"a\r\nb"), vec![&b"a"[..], &b"b"[..]]);
        assert_eq!(split_lines(b"\xEF\xBB\xBFa\n"), vec![&b"a"[..]]);
        assert_eq!(split_lines(b"").len(), 0);
    }

    #[test]
    fn keeps_interior_blank_lines() {
        assert_eq!(
            split_lines(b"a\n\nb\n"),
            vec![&b"a"[..], &b""[..], &b"b"[..]]
        );
    }
}
