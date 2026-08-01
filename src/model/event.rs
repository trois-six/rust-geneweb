//! Event names, witnesses, and the `gwplus` event record.
//!
//! Port of `get_pevent_name`, `get_fevent_name` and `get_event_witness_kind`.
//!
//! The tag sets below are *open*: GeneWeb falls back to a user-named event for any
//! unrecognised `#tag`, so a file using a tag this crate has never heard of parses
//! successfully into [`PersonEventName::Named`] rather than failing. Only a token that
//! does not start with `#` is an error.

use super::key::Somebody;
use super::person::Sex;
use crate::date::GwDate;

/// Generates an event-name enum with its tag table, plus `from_tag`/`tag` conversions.
macro_rules! event_names {
    (
        $(#[$meta:meta])*
        $name:ident { $($tag:literal => $variant:ident),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum $name {
            $(
                #[doc = concat!("Written `", $tag, "`.")]
                $variant,
            )*
            /// An event GeneWeb does not name, written with any other `#tag`.
            ///
            /// Holds the tag without its leading `#`.
            Named(String),
        }

        impl $name {
            /// Every tag this enum names explicitly, excluding the open fallback.
            pub const TAGS: &'static [&'static str] = &[$($tag),*];

            /// Parses an event tag.
            ///
            /// Returns `None` when `token` does not start with `#`, which is the only
            /// case GeneWeb treats as a syntax error.
            #[must_use]
            pub fn from_tag(token: &str) -> Option<Self> {
                match token {
                    $($tag => Some(Self::$variant),)*
                    other => other.strip_prefix('#').map(|t| Self::Named(t.to_owned())),
                }
            }

            /// The tag this event is written with, including the leading `#`.
            #[must_use]
            pub fn tag(&self) -> String {
                match self {
                    $(Self::$variant => $tag.to_owned(),)*
                    Self::Named(name) => format!("#{name}"),
                }
            }
        }
    };
}

event_names! {
    /// The name of a personal event, as introduced in a `pevt` block.
    PersonEventName {
        "#birt" => Birth,
        "#bapt" => Baptism,
        "#deat" => Death,
        "#buri" => Burial,
        "#crem" => Cremation,
        "#acco" => Accomplishment,
        "#acqu" => Acquisition,
        "#adhe" => Adhesion,
        "#awar" => Decoration,
        "#bapl" => BaptismLds,
        "#barm" => BarMitzvah,
        "#basm" => BatMitzvah,
        "#bles" => Benediction,
        "#cens" => Census,
        "#chgn" => ChangeName,
        "#circ" => Circumcision,
        "#conf" => Confirmation,
        "#conl" => ConfirmationLds,
        "#degr" => Diploma,
        "#demm" => MilitaryDemobilisation,
        "#dist" => Distinction,
        "#dotl" => DotationLds,
        "#educ" => Education,
        "#elec" => Election,
        "#emig" => Emigration,
        "#endl" => Dotation,
        "#exco" => Excommunication,
        "#fcom" => FirstCommunion,
        "#flkl" => FamilyLinkLds,
        "#fune" => Funeral,
        "#grad" => Graduate,
        "#hosp" => Hospitalisation,
        "#illn" => Illness,
        "#immi" => Immigration,
        "#lpas" => PassengerList,
        "#mdis" => MilitaryDistinction,
        "#mobm" => MilitaryMobilisation,
        "#mpro" => MilitaryPromotion,
        "#mser" => MilitaryService,
        "#natu" => Naturalisation,
        "#occu" => Occupation,
        "#ordn" => Ordination,
        "#prop" => Property,
        "#resi" => Residence,
        "#reti" => Retired,
        "#slgc" => ScellentChildLds,
        "#slgp" => ScellentParentLds,
        "#slgs" => ScellentSpouseLds,
        "#vteb" => PropertySale,
        "#will" => Will,
    }
}

event_names! {
    /// The name of a family event, as introduced in a `fevt` block.
    FamilyEventName {
        "#marr" => Marriage,
        "#nmar" => NoMarriage,
        "#nmen" => NoMention,
        "#enga" => Engagement,
        "#div" => Divorce,
        "#sep" => Separated,
        "#anul" => Annulment,
        "#marb" => MarriageBann,
        "#marc" => MarriageContract,
        "#marl" => MarriageLicense,
        "#pacs" => Pacs,
        "#resi" => Residence,
    }
}

/// The capacity in which a witness attended an event.
///
/// Only `#godp` and `#offi` are documented on the GeneWeb wiki; the remaining five exist
/// in `get_event_witness_kind` and appear in files produced by current versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WitnessKind {
    /// A plain witness. The default when no kind tag is given.
    #[default]
    Witness,
    /// Godparent, written `#godp`.
    GodParent,
    /// Civil officer, written `#offi`.
    CivilOfficer,
    /// Religious officer, written `#reli`.
    ReligiousOfficer,
    /// Informant, written `#info`.
    Informant,
    /// Attending, written `#atte`.
    Attending,
    /// Mentioned, written `#ment`.
    Mentioned,
    /// Other, written `#othe`.
    Other,
}

impl WitnessKind {
    /// Every witness-kind tag, in the order `get_event_witness_kind` tests them.
    pub const TAGS: &'static [&'static str] = &[
        "#godp", "#offi", "#reli", "#info", "#atte", "#ment", "#othe",
    ];

    /// Parses a witness-kind tag, returning `None` when the token is not one.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "#godp" => Some(Self::GodParent),
            "#offi" => Some(Self::CivilOfficer),
            "#reli" => Some(Self::ReligiousOfficer),
            "#info" => Some(Self::Informant),
            "#atte" => Some(Self::Attending),
            "#ment" => Some(Self::Mentioned),
            "#othe" => Some(Self::Other),
            _ => None,
        }
    }

    /// The tag this kind is written with, or `None` for a plain witness.
    #[must_use]
    pub fn tag(self) -> Option<&'static str> {
        match self {
            Self::Witness => None,
            Self::GodParent => Some("#godp"),
            Self::CivilOfficer => Some("#offi"),
            Self::ReligiousOfficer => Some("#reli"),
            Self::Informant => Some("#info"),
            Self::Attending => Some("#atte"),
            Self::Mentioned => Some("#ment"),
            Self::Other => Some("#othe"),
        }
    }
}

/// A witness to an event.
#[derive(Debug, Clone, PartialEq)]
pub struct Witness {
    /// The witness, defined inline or referenced.
    pub person: Somebody,
    /// The witness's sex, from the `m:`/`f:` marker on the `wit` line.
    pub sex: Sex,
    /// The capacity in which they attended.
    pub kind: WitnessKind,
}

/// A `gwplus` event: a name, when and where it happened, and who was there.
///
/// Generic over the name type so that personal and family events share one shape, which
/// is what they have in the file format.
#[derive(Debug, Clone, PartialEq)]
pub struct Event<N> {
    /// What kind of event this is.
    pub name: N,
    /// When it happened.
    pub date: Option<GwDate>,
    /// Where it happened, from `#p`.
    pub place: String,
    /// Its cause, from `#c`.
    pub cause: String,
    /// Its source, from `#s`.
    pub source: String,
    /// Free-text note, accumulated from the `note` lines that follow.
    pub note: String,
    /// Witnesses, from the `wit` lines that follow.
    pub witnesses: Vec<Witness>,
}

impl<N> Event<N> {
    /// Builds an event with no details beyond its name.
    pub fn new(name: N) -> Self {
        Self {
            name,
            date: None,
            place: String::new(),
            cause: String::new(),
            source: String::new(),
            note: String::new(),
            witnesses: Vec::new(),
        }
    }
}

/// A personal event, from a `pevt` block.
pub type PersonEvent = Event<PersonEventName>;

/// A family event, from a `fevt` block.
pub type FamilyEvent = Event<FamilyEventName>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tags_round_trip() {
        for tag in PersonEventName::TAGS {
            let name = PersonEventName::from_tag(tag).expect("known tag must parse");
            assert_eq!(&name.tag(), tag);
            assert!(
                !matches!(name, PersonEventName::Named(_)),
                "{tag} fell through"
            );
        }
        for tag in FamilyEventName::TAGS {
            let name = FamilyEventName::from_tag(tag).expect("known tag must parse");
            assert_eq!(&name.tag(), tag);
        }
        for tag in WitnessKind::TAGS {
            assert_eq!(
                WitnessKind::from_tag(tag).and_then(WitnessKind::tag),
                Some(*tag)
            );
        }
    }

    #[test]
    fn the_tag_set_is_open() {
        // A tag from a future GeneWeb release must not be a parse error.
        assert_eq!(
            PersonEventName::from_tag("#nope"),
            Some(PersonEventName::Named("nope".into()))
        );
        assert_eq!(PersonEventName::from_tag("#nope").unwrap().tag(), "#nope");
    }

    #[test]
    fn a_token_without_a_hash_is_not_an_event() {
        assert_eq!(PersonEventName::from_tag("birt"), None);
        assert_eq!(FamilyEventName::from_tag(""), None);
    }

    #[test]
    fn tag_counts_match_geneweb() {
        assert_eq!(PersonEventName::TAGS.len(), 50);
        assert_eq!(FamilyEventName::TAGS.len(), 12);
    }
}
