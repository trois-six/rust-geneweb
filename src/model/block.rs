//! The top-level blocks a `.gw` file is made of.
//!
//! Counterpart of the `gw_syntax` type in `bin/gwc/gwcomp.mli`.

use super::event::PersonEvent;
use super::family::Family;
use super::key::{Key, Somebody};
use super::person::Sex;
use super::relation::Relation;

/// One block of a `.gw` file.
///
/// A file is a flat sequence of these. Nothing is nested: a person's notes, events and
/// relations each arrive as their own block, keyed by name, and are stitched onto the
/// person afterwards.
#[derive(Debug, Clone, PartialEq)]
pub enum GwBlock {
    /// A `fam` block: a couple and their children.
    Family(Box<Family>),
    /// A `notes` block: free text attached to one person.
    PersonNotes {
        /// Who the notes are about.
        key: Key,
        /// The note text, in GeneWeb's wiki syntax.
        text: String,
    },
    /// A `rel` block: a person's relations to parent figures outside their family.
    Relations {
        /// Who the relations belong to.
        person: Somebody,
        /// Their sex, from the `#h`/`#m`/`#f` marker.
        sex: Sex,
        /// The relations themselves.
        relations: Vec<Relation>,
    },
    /// A `pevt` block: a person's structured events. `gwplus` only.
    PersonEvents {
        /// Who the events belong to.
        person: Somebody,
        /// Their sex. Always [`Sex::Neuter`]: the block has no syntax for it, and
        /// GeneWeb hardcodes the value.
        sex: Sex,
        /// The events themselves.
        events: Vec<PersonEvent>,
    },
    /// A `notes-db` or `page-ext` block.
    ///
    /// The two share a representation because GeneWeb stores them the same way: database
    /// notes are the extended page whose name is empty.
    DatabaseNotes {
        /// The page name, or empty for the base's own presentation notes.
        page: String,
        /// The page text, in GeneWeb's wiki syntax.
        text: String,
    },
    /// A `wizard-note` block: a note attached to a contributor rather than a person.
    WizardNotes {
        /// The wizard's identifier.
        wizard: String,
        /// The note text. Its first line is a Unix timestamp.
        text: String,
    },
}
