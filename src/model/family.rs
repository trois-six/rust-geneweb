//! The family record.
//!
//! Counterpart of the `gen_family` and `gen_descend` pair built by `read_family`.

use super::event::{FamilyEvent, Witness};
use super::key::Somebody;
use super::person::{Person, Sex};
use crate::date::GwDate;

/// The nature of the union between two parents.
///
/// Several of these carry no marriage in the civil sense; GeneWeb records the
/// distinction because it changes how the couple is displayed and indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RelationKind {
    /// Married. The default when no tag follows the `+`.
    #[default]
    Married,
    /// Not married, written `#nm`.
    NotMarried,
    /// Engaged, written `#eng`.
    Engaged,
    /// Unmarried, with the usual sex check suppressed, written `#nsck`.
    NoSexesCheckNotMarried,
    /// Married, with the usual sex check suppressed, written `#nsckm`.
    NoSexesCheckMarried,
    /// No mention of the union's nature, written `#noment`.
    NoMention,
    /// Marriage banns, written `#banns`.
    MarriageBann,
    /// Marriage contract, written `#contract`.
    MarriageContract,
    /// Marriage licence, written `#license`.
    MarriageLicense,
    /// Civil partnership, written `#pacs`.
    Pacs,
    /// Shared residence, written `#residence`.
    Residence,
}

/// How a union ended, if it did.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Divorce {
    /// Still together, or nothing recorded. The default.
    #[default]
    NotDivorced,
    /// Divorced, written `-` with an optional date.
    Divorced(Option<GwDate>),
    /// Separated, written `#sep`.
    Separated(Option<GwDate>),
}

/// A family: a couple, how they were joined, and their children.
#[derive(Debug, Clone, PartialEq)]
pub struct Family {
    /// The first parent, defined inline or referenced.
    pub father: Somebody,
    /// The second parent, defined inline or referenced.
    pub mother: Somebody,
    /// The first parent's sex. Taken from the two-letter code after the relation tag,
    /// defaulting to male.
    pub father_sex: Sex,
    /// The second parent's sex, defaulting to female.
    pub mother_sex: Sex,
    /// The nature of their union.
    pub relation: RelationKind,
    /// Date of the union, following the `+`.
    pub marriage: Option<GwDate>,
    /// Place of the union, written `#mp`.
    pub marriage_place: String,
    /// Note on the union, written `#mn`.
    pub marriage_note: String,
    /// Source for the union, written `#ms`.
    pub marriage_src: String,
    /// How the union ended.
    pub divorce: Divorce,
    /// Witnesses to the union, from the `wit` lines directly under the `fam` line.
    ///
    /// These carry no [`super::event::WitnessKind`]: the family line's witness syntax
    /// predates event witnesses and admits only a sex marker.
    pub witnesses: Vec<Witness>,
    /// Source for the family, from the `src` line.
    pub sources: String,
    /// Default source for children who declare none, from the `csrc` line.
    pub children_sources: String,
    /// Default birth place for children who declare none, from the `cbp` line.
    pub children_birth_place: String,
    /// Free-text comment, from the `comm` line.
    pub comment: String,
    /// Structured events, from the `fevt` block.
    ///
    /// Where these overlap the `fam` line's own fields, GeneWeb gives the event
    /// precedence.
    pub events: Vec<FamilyEvent>,
    /// The children, from the `beg`…`end` block. Always full definitions, never
    /// references.
    pub children: Vec<Person>,
    /// Basename of the file this family was read from, as GeneWeb records in
    /// `origin_file`.
    pub origin_file: String,
}

impl Family {
    /// Builds a family from its couple, with everything else empty.
    #[must_use]
    pub fn new(father: Somebody, mother: Somebody) -> Self {
        Self {
            father,
            mother,
            father_sex: Sex::Male,
            mother_sex: Sex::Female,
            relation: RelationKind::default(),
            marriage: None,
            marriage_place: String::new(),
            marriage_note: String::new(),
            marriage_src: String::new(),
            divorce: Divorce::default(),
            witnesses: Vec::new(),
            sources: String::new(),
            children_sources: String::new(),
            children_birth_place: String::new(),
            comment: String::new(),
            events: Vec::new(),
            children: Vec::new(),
            origin_file: String::new(),
        }
    }
}
