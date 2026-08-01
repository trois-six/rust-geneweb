//! Nobiliary titles.
//!
//! Port of `scan_title`. A title is written `[name:title:place:start:end:nth]`, where any
//! field may be empty and `:` inside a field is escaped with a backslash.

use crate::date::GwDate;

/// The name slot of a title, which doubles as a marker for the principal title.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TitleName {
    /// The slot was empty.
    #[default]
    None,
    /// The slot held `*`, marking this as the person's main title.
    Main,
    /// The slot held a name.
    Name(String),
}

/// A title held by a person.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Title {
    /// The name slot, or the main-title marker.
    pub name: TitleName,
    /// The title itself, for instance `duc`. A title with an empty ident is discarded by
    /// GeneWeb and never reaches this struct.
    pub ident: String,
    /// The place the title is attached to.
    pub place: String,
    /// When the person acquired it.
    pub date_start: Option<GwDate>,
    /// When they ceased to hold it.
    pub date_end: Option<GwDate>,
    /// Which holder of the title they were, or `0`.
    pub nth: i32,
}
