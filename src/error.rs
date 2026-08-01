//! Error types for the `.gw` reader.
//!
//! Every failure carries the source line number and the offending line, mirroring the
//! `failwith str` reporting of GeneWeb's own `gwcomp.ml`.

use std::fmt;

/// An error encountered while reading a `.gw` file.
#[derive(Debug)]
pub struct GwError {
    /// 1-based line number in the source file, or `0` when not tied to a line.
    pub line: usize,
    /// The offending source line, when available.
    pub context: Option<String>,
    /// What went wrong.
    pub kind: GwErrorKind,
}

impl GwError {
    /// Builds an error attached to a source line and its text.
    pub fn at(line: usize, context: impl Into<String>, kind: GwErrorKind) -> Self {
        Self {
            line,
            context: Some(context.into()),
            kind,
        }
    }

    /// Builds an error attached to a source line, without the line text.
    #[must_use]
    pub fn at_line(line: usize, kind: GwErrorKind) -> Self {
        Self {
            line,
            context: None,
            kind,
        }
    }
}

impl fmt::Display for GwError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.kind)?;
        } else {
            write!(f, "{}", self.kind)?;
        }
        if let Some(context) = &self.context {
            write!(f, "\n  in: {context}")?;
        }
        Ok(())
    }
}

impl std::error::Error for GwError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            GwErrorKind::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for GwError {
    fn from(e: std::io::Error) -> Self {
        Self::at_line(0, GwErrorKind::Io(e))
    }
}

/// The cause of a [`GwError`].
#[derive(Debug)]
#[non_exhaustive]
pub enum GwErrorKind {
    /// Underlying I/O failure.
    Io(std::io::Error),
    /// The line could not be parsed as the construct it appeared to be.
    Syntax(String),
    /// A block was left unterminated at end of file.
    UnexpectedEof {
        /// The terminator that was being looked for, e.g. `end fevt`.
        expected: &'static str,
    },
    /// A date field did not match the `.gw` date grammar.
    InvalidDate(String),
    /// A `[..]` title field did not match the `name:title:place:start:end:nth` shape.
    InvalidTitle(String),
    /// An unrecognised `#tag`, in a position where the tag set is closed.
    UnknownTag(String),
    /// Trailing tokens remained after a line was fully parsed.
    TrailingTokens(Vec<String>),
    /// A block keyword was found where none is valid.
    UnknownBlock(String),
}

impl fmt::Display for GwErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Syntax(what) => write!(f, "syntax error: {what}"),
            Self::UnexpectedEof { expected } => {
                write!(f, "unexpected end of file, expected `{expected}`")
            }
            Self::InvalidDate(s) => write!(f, "invalid date: `{s}`"),
            Self::InvalidTitle(s) => write!(f, "invalid title: `{s}`"),
            Self::UnknownTag(s) => write!(f, "unknown tag: `{s}`"),
            Self::TrailingTokens(toks) => {
                write!(f, "unexpected trailing tokens: {}", toks.join(" "))
            }
            Self::UnknownBlock(s) => write!(f, "unknown block keyword: `{s}`"),
        }
    }
}

/// Convenience alias for results produced by this crate.
pub type Result<T> = std::result::Result<T, GwError>;
