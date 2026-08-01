//! Tokenization of `.gw` lines.
//!
//! This is a character-for-character port of `copy_decode`, `fields`, `cut_space` and
//! `get_field` from GeneWeb's `bin/gwc/gwcomp.ml`. The rules look trivial and are not:
//! getting them subtly wrong silently corrupts place names and sources rather than
//! failing loudly, so each one is pinned by a test below.
//!
//! The three rules that matter:
//!
//! 1. A line splits on spaces and tabs; runs of separators collapse.
//! 2. Within a token, `_` becomes a space and `\c` becomes `c` — *except* that the very
//!    last byte of a token is copied verbatim unless it is `_`. A trailing backslash is
//!    therefore a literal backslash, not a dangling escape.
//! 3. [`cut_space`] then removes *exactly one* leading and one trailing space — it is not
//!    a `trim`. This is what lets a `.gw` file encode a genuinely leading space: `__Foo`
//!    decodes to `"  Foo"`, which `cut_space` reduces to `" Foo"`.

/// Decodes one raw token: `_` to space, `\c` to `c`.
///
/// Operates on bytes, like the original. This is safe for UTF-8 input because every byte
/// it examines (`_`, `\`) is ASCII, and UTF-8 continuation bytes always have the high bit
/// set, so they can never be mistaken for one.
pub(crate) fn copy_decode(s: &[u8]) -> String {
    let end = s.len();
    let mut out = Vec::with_capacity(end);
    let mut i = 0;
    while i < end {
        // The final byte is taken literally unless it is `_`; notably this means a
        // trailing `\` escapes nothing.
        if i == end - 1 && s[i] != b'_' {
            out.push(s[i]);
            break;
        }
        let (byte, consumed) = match s[i] {
            b'_' => (b' ', i),
            // Reachable only when `i < end - 1`, so `i + 1` is in bounds: the branch
            // above already returned for a backslash in final position.
            b'\\' => (s[i + 1], i + 1),
            other => (other, i),
        };
        out.push(byte);
        i = consumed + 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Splits a `.gw` line into decoded tokens.
///
/// Separators are space and tab; consecutive separators produce no empty tokens.
#[must_use]
pub fn fields(line: &str) -> Vec<String> {
    let s = line.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    for i in 0..s.len() {
        if s[i] == b' ' || s[i] == b'\t' {
            if start != i {
                out.push(copy_decode(&s[start..i]));
            }
            start = i + 1;
        }
    }
    if start != s.len() {
        out.push(copy_decode(&s[start..]));
    }
    out
}

/// Removes exactly one leading and one trailing space.
///
/// Deliberately *not* `str::trim`: see the module documentation.
#[must_use]
pub fn cut_space(x: &str) -> &str {
    let bytes = x.as_bytes();
    let len = bytes.len();
    if len == 0 {
        return x;
    }
    if x == " " {
        return "";
    }
    let start = usize::from(bytes[0] == b' ');
    let stop = if bytes[len - 1] == b' ' { len - 1 } else { len };
    // Both bounds land on ASCII space positions or the string ends, so they are always
    // UTF-8 character boundaries.
    &x[start..stop]
}

/// Returns the raw remainder of `raw` after `keyword` and its following separator.
///
/// Used for the constructs that take free text to end of line — `comm`, `note`,
/// `page-ext`, `wizard-note` — which bypass tokenization entirely so that underscores and
/// backslashes in prose survive untouched.
#[must_use]
pub fn rest_after<'a>(raw: &'a str, keyword: &str) -> &'a str {
    raw.strip_prefix(keyword)
        .map_or("", |rest| rest.strip_prefix(' ').unwrap_or(rest))
}

/// A forward-only cursor over the tokens of a single line.
///
/// The `.gw` person grammar is strictly positional: fields must be consumed in a fixed
/// order, and a field that is absent simply leaves the cursor where it was. Every
/// accessor here follows that contract — it either consumes and returns, or returns a
/// default and leaves the cursor untouched.
#[derive(Debug, Clone)]
pub struct Cursor<'a> {
    tokens: &'a [String],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Creates a cursor over an already-tokenized line.
    #[must_use]
    pub fn new(tokens: &'a [String]) -> Self {
        Self { tokens, pos: 0 }
    }

    /// Returns `true` when every token has been consumed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Returns the next token without consuming it.
    #[must_use]
    pub fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.pos).map(String::as_str)
    }

    /// Returns the token `n` positions ahead without consuming anything.
    #[must_use]
    pub fn peek_at(&self, n: usize) -> Option<&'a str> {
        self.tokens.get(self.pos + n).map(String::as_str)
    }

    /// Consumes and returns the next token.
    pub fn advance(&mut self) -> Option<&'a str> {
        let token = self.tokens.get(self.pos).map(String::as_str);
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    /// Consumes the next token if it equals `token`, reporting whether it did.
    pub fn eat(&mut self, token: &str) -> bool {
        if self.peek() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Consumes the next token if it equals any of `tokens`, returning which one matched.
    pub fn eat_any(&mut self, tokens: &[&'static str]) -> Option<&'static str> {
        let next = self.peek()?;
        let matched = tokens.iter().copied().find(|t| *t == next)?;
        self.pos += 1;
        Some(matched)
    }

    /// Reads a `tag value` pair, as `get_field` does.
    ///
    /// Returns the space-cut value and consumes both tokens when `tag` is next; otherwise
    /// returns the empty string and consumes nothing. An empty result is indistinguishable
    /// from an absent field, which matches GeneWeb's own data model.
    pub fn field(&mut self, tag: &str) -> String {
        match (self.peek(), self.peek_at(1)) {
            (Some(t), Some(value)) if t == tag => {
                self.pos += 2;
                cut_space(value).to_owned()
            }
            _ => String::new(),
        }
    }

    /// The tokens not yet consumed.
    #[must_use]
    pub fn remaining(&self) -> &'a [String] {
        &self.tokens[self.pos.min(self.tokens.len())..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underscores_become_spaces() {
        assert_eq!(fields("Jean_Pierre"), vec!["Jean Pierre"]);
        assert_eq!(fields("a b\tc"), vec!["a", "b", "c"]);
        assert_eq!(fields("  a   b  "), vec!["a", "b"]);
        assert!(fields("").is_empty());
    }

    #[test]
    fn backslash_escapes_the_next_byte() {
        assert_eq!(fields(r"a\_b"), vec!["a_b"]);
        assert_eq!(fields(r"a\\b"), vec![r"a\b"]);
    }

    #[test]
    fn a_trailing_backslash_is_literal() {
        // The last byte is copied verbatim, so this is not a dangling escape.
        assert_eq!(fields(r"ab\"), vec![r"ab\"]);
    }

    #[test]
    fn a_trailing_underscore_still_becomes_a_space() {
        assert_eq!(fields("ab_"), vec!["ab "]);
    }

    #[test]
    fn cut_space_removes_one_space_not_all() {
        assert_eq!(cut_space("  Foo"), " Foo");
        assert_eq!(cut_space(" Foo "), "Foo");
        assert_eq!(cut_space("Foo"), "Foo");
        assert_eq!(cut_space(" "), "");
        assert_eq!(cut_space("  "), "");
        assert_eq!(cut_space(""), "");
    }

    #[test]
    fn leading_space_survives_the_full_pipeline() {
        // From `test/galichet.gw`: a place name that genuinely starts with `[`, encoded
        // with a leading underscore so the token does not begin with a bare bracket.
        let toks = fields("#bp _[Châlons-sur-Marne]_-_Châlons-en-Champagne,51");
        assert_eq!(toks[0], "#bp");
        assert_eq!(toks[1], " [Châlons-sur-Marne] - Châlons-en-Champagne,51");
        assert_eq!(
            cut_space(&toks[1]),
            "[Châlons-sur-Marne] - Châlons-en-Champagne,51"
        );
    }

    #[test]
    fn multibyte_characters_survive_tokenization() {
        assert_eq!(fields("déjà_décédés"), vec!["déjà décédés"]);
        assert_eq!(fields("Thérèse_Eugénie"), vec!["Thérèse Eugénie"]);
    }

    #[test]
    fn field_consumes_only_on_match() {
        let toks = fields("#bp Paris #bs acte");
        let mut cur = Cursor::new(&toks);
        assert_eq!(cur.field("#dp"), "");
        assert_eq!(cur.peek(), Some("#bp"));
        assert_eq!(cur.field("#bp"), "Paris");
        assert_eq!(cur.field("#bs"), "acte");
        assert!(cur.is_empty());
    }

    #[test]
    fn field_without_a_value_is_not_consumed() {
        let toks = fields("#bp");
        let mut cur = Cursor::new(&toks);
        assert_eq!(cur.field("#bp"), "");
        assert_eq!(cur.peek(), Some("#bp"));
    }

    #[test]
    fn rest_after_keeps_free_text_verbatim() {
        assert_eq!(rest_after("comm a_b \\c", "comm"), "a_b \\c");
        assert_eq!(rest_after("note", "note"), "");
        assert_eq!(rest_after("note <br>1880 - x", "note"), "<br>1880 - x");
    }
}
