//! The person record and its scalar attributes.
//!
//! Field-for-field counterpart of the `gen_person` built by `set_infos`.
//!
//! Absent string fields are the empty string rather than `None`. That is not laziness:
//! GeneWeb's `get_field` returns `""` both when a tag is missing and when it carries an
//! empty value, and `gwu` writes nothing for either, so the two are genuinely the same
//! state in this format.

use super::event::PersonEvent;
use super::relation::Relation;
use super::title::Title;
use crate::date::GwDate;

/// A person's recorded sex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Sex {
    /// Male, written `h` or `m`.
    Male,
    /// Female, written `f`.
    Female,
    /// Unrecorded. The default.
    #[default]
    Neuter,
}

/// Who may see a person's details on a served GeneWeb base.
///
/// This has no GEDCOM counterpart and is one of the reasons this crate keeps its own
/// model rather than parsing straight into `ged_io`'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Access {
    /// Visible only if the person holds a title. The default when no tag is given.
    #[default]
    IfTitles,
    /// Visible to everyone, written `#apubl`.
    Public,
    /// Visible to wizards only, written `#apriv`.
    Private,
    /// Visible to wizards and friends, written `#semipub`, or `#afriend` in older files.
    SemiPublic,
}

/// How a death came about, from the single-letter prefix on the death date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DeathReason {
    /// No prefix.
    #[default]
    Unspecified,
    /// Killed, written `k`.
    Killed,
    /// Murdered, written `m`.
    Murdered,
    /// Executed, written `e`.
    Executed,
    /// Disappeared, written `s`.
    Disappeared,
}

/// What is known about a person's death.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Death {
    /// Known to be alive. Inferred, not written: a person with a birth date and no death
    /// field is taken to be living.
    NotDead,
    /// Dead on a known date.
    Dead {
        /// How they died.
        reason: DeathReason,
        /// When they died.
        date: GwDate,
    },
    /// Dead, but the written date could not be structured.
    DeadDontKnowWhen,
    /// Died young, written `mj`.
    DeadYoung,
    /// Long enough ago that they must be dead, written `od`.
    OfCourseDead,
    /// Nothing is known, written `?`. The default.
    #[default]
    DontKnowIfDead,
}

/// What is known about a person's burial.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Burial {
    /// Nothing recorded. The default.
    #[default]
    Unknown,
    /// Buried, written `#buri` with an optional date.
    Buried(Option<GwDate>),
    /// Cremated, written `#crem` with an optional date.
    Cremated(Option<GwDate>),
}

/// A person as defined in a `.gw` file.
///
/// `notes`, `pevents` and `rparents` are not filled by the line that defines the person:
/// they come from the separate `notes`, `pevt` and `rel` blocks, which are merged in by
/// [`crate::database`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Person {
    /// Given name.
    pub first_name: String,
    /// Family name. Inherited from the father when a child omits it.
    pub surname: String,
    /// Occurrence number disambiguating homonyms.
    pub occ: u32,
    /// Recorded sex.
    pub sex: Sex,
    /// Alternative given names, written `{Alias}`.
    pub first_names_aliases: Vec<String>,
    /// Alternative family names, written `#salias`.
    pub surnames_aliases: Vec<String>,
    /// Public name, written `(Public Name)`.
    pub public_name: String,
    /// Portrait path, written `#image` or `#photo`. The file itself is not part of the
    /// `.gw`.
    pub image: String,
    /// Nicknames, written `#nick`.
    pub qualifiers: Vec<String>,
    /// Aliases, written `#alias`.
    pub aliases: Vec<String>,
    /// Titles, written `[name:title:place:start:end:nth]`.
    pub titles: Vec<Title>,
    /// Visibility of this person on a served base.
    pub access: Access,
    /// Occupation, written `#occu`.
    pub occupation: String,
    /// Source for the person as a whole, written `#src`.
    pub sources: String,
    /// Birth date.
    pub birth: Option<GwDate>,
    /// Birth place, written `#bp`.
    pub birth_place: String,
    /// Birth note, written `#bn`.
    pub birth_note: String,
    /// Birth source, written `#bs`.
    pub birth_src: String,
    /// Baptism date, written with a leading `!`.
    pub baptism: Option<GwDate>,
    /// Baptism place, written `#pp`. Falls back to the birth place when absent.
    pub baptism_place: String,
    /// Baptism note, written `#pn`.
    pub baptism_note: String,
    /// Baptism source, written `#ps`.
    pub baptism_src: String,
    /// What is known about the death.
    pub death: Death,
    /// Death place, written `#dp`.
    pub death_place: String,
    /// Death note, written `#dn`.
    pub death_note: String,
    /// Death source, written `#ds`.
    pub death_src: String,
    /// What is known about the burial.
    pub burial: Burial,
    /// Burial place, written `#rp`.
    pub burial_place: String,
    /// Burial note, written `#rn`.
    pub burial_note: String,
    /// Burial source, written `#rs`.
    pub burial_src: String,
    /// Free-text notes, from a `notes` block naming this person.
    pub notes: String,
    /// Structured events, from a `pevt` block naming this person.
    pub events: Vec<PersonEvent>,
    /// Relations to people outside the family structure, from a `rel` block.
    pub relations: Vec<Relation>,
}

impl Person {
    /// Builds a person with a name and nothing else.
    pub fn new(first_name: impl Into<String>, surname: impl Into<String>, occ: u32) -> Self {
        Self {
            first_name: first_name.into(),
            surname: surname.into(),
            occ,
            ..Self::default()
        }
    }
}
