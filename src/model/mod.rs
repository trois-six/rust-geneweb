//! A lossless syntax tree for the `.gw` format.
//!
//! These types mirror GeneWeb's own records rather than GEDCOM's, so that reading a file
//! discards nothing. Conversion to GEDCOM happens separately, in [`crate::gedcom`], where
//! the concepts that have no standard counterpart are mapped to user-defined tags.

pub mod block;
pub mod event;
pub mod family;
pub mod key;
pub mod person;
pub mod relation;
pub mod title;

pub use block::GwBlock;
pub use event::{
    Event, FamilyEvent, FamilyEventName, PersonEvent, PersonEventName, Witness, WitnessKind,
};
pub use family::{Divorce, Family, RelationKind};
pub use key::{Key, Somebody};
pub use person::{Access, Burial, Death, DeathReason, Person, Sex};
pub use relation::{Relation, RelationType};
pub use title::{Title, TitleName};
