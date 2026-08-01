//! A reader for GeneWeb `.gw` genealogy files, with conversion to GEDCOM via [`ged_io`].
//!
//! `.gw` is the textual interchange format of the [GeneWeb](https://geneweb.tuxfamily.org)
//! genealogy software — the format produced by `gwu` and consumed by `gwc`, and the
//! backup format GeneWeb itself recommends. This crate reads it, including the `gwplus`
//! extension introduced in GeneWeb 7.00 that adds structured personal and family events.
//!
//! # Two layers
//!
//! Reading happens in two stages, because the two data models are not equivalent:
//!
//! 1. A faithful, lossless `.gw` syntax tree. GeneWeb has concepts GEDCOM has no room for
//!    — per-person access rights, wizard notes, extended wiki pages, occurrence-numbered
//!    keys — and this layer keeps all of them.
//! 2. A conversion of that tree into [`ged_io`]'s GEDCOM model, where anything without a
//!    standard GEDCOM counterpart is preserved as a user-defined tag rather than dropped.
//!
//! # Reference
//!
//! The normative reference for the grammar is GeneWeb's own OCaml implementation,
//! `bin/gwc/gwcomp.ml`, not the wiki page, which lags it. Where this crate reproduces a
//! non-obvious rule, the corresponding OCaml function is named in a comment.

pub mod database;
pub mod date;
pub mod encoding;
pub mod error;
pub mod gedcom;
pub mod lexer;
pub mod model;
pub mod parser;

pub use error::{GwError, GwErrorKind, Result};
