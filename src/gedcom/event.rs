//! Mapping `.gw` event names onto GEDCOM events.
//!
//! Follows `ged_tag_pevent`, `ged_tag_fevent`, `is_primary_pevents` and
//! `is_primary_fevents` in GeneWeb's `bin/gwb2ged/gwb2gedLib.ml`.
//!
//! GeneWeb names about fifty personal events; GEDCOM standardises far fewer. Each `.gw`
//! event therefore falls into one of two shapes:
//!
//! - a **standard** event, which becomes its GEDCOM tag directly (`#birt` → `BIRT`);
//! - a **generic** event, which becomes `EVEN` carrying a `TYPE` label — the same label
//!   GeneWeb writes, so the two exporters agree.
//!
//! Nothing is dropped: an event with no GEDCOM counterpart keeps its name in the label,
//! and callers additionally record the original `.gw` tag as a user-defined tag.

use ged_io::model::EventKind;

use crate::model::event::{FamilyEventName, PersonEventName, WitnessKind};

/// How a `.gw` event maps onto GEDCOM.
#[derive(Debug, Clone, PartialEq)]
pub struct EventMapping {
    /// The GEDCOM event to record.
    pub event: EventKind,
    /// The `TYPE` label, set only for generic events.
    pub event_type: Option<String>,
}

impl EventMapping {
    fn standard(event: EventKind) -> Self {
        Self {
            event,
            event_type: None,
        }
    }

    fn generic(label: &str) -> Self {
        Self {
            event: EventKind::Event,
            event_type: Some(label.to_owned()),
        }
    }
}

/// Maps a personal event name.
// Arms are listed one `.gw` tag at a time, even where two share a label, so the table
// can be diffed against `ged_tag_pevent`.
#[must_use]
pub fn person_event(name: &PersonEventName) -> EventMapping {
    use PersonEventName as P;
    match name {
        P::Birth => EventMapping::standard(EventKind::Birth),
        P::Death => EventMapping::standard(EventKind::Death),
        P::Burial => EventMapping::standard(EventKind::Burial),
        P::Cremation => EventMapping::standard(EventKind::Cremation),
        // GEDCOM has one `BAPM`; GeneWeb distinguishes the LDS ordinance, which the
        // `.gw` tag recorded alongside the event preserves.
        P::Baptism => EventMapping::standard(EventKind::Baptism),
        P::BarMitzvah => EventMapping::standard(EventKind::BarMitzvah),
        P::BatMitzvah => EventMapping::standard(EventKind::BasMitzvah),
        P::Benediction => EventMapping::standard(EventKind::Blessing),
        P::Confirmation => EventMapping::standard(EventKind::Confirmation),

        P::Emigration => EventMapping::standard(EventKind::Emigration),
        P::FirstCommunion => EventMapping::standard(EventKind::FirstCommunion),
        P::Graduate => EventMapping::standard(EventKind::Graduation),
        P::Immigration => EventMapping::standard(EventKind::Immigration),
        P::Naturalisation => EventMapping::standard(EventKind::Naturalization),
        P::Ordination => EventMapping::standard(EventKind::Ordination),
        P::Census => EventMapping::standard(EventKind::Census),
        P::Residence => EventMapping::standard(EventKind::Residence),
        P::Retired => EventMapping::standard(EventKind::Retirement),
        P::Will => EventMapping::standard(EventKind::Will),

        // Standard GEDCOM tags that are not events. `gwb2ged` writes them as `1 EDUC`,
        // `1 BAPL`, …, but no GEDCOM structure of that tag can hold a `.gw` event:
        // `EDUC`, `OCCU` and `PROP` are attributes whose payload (the achievement,
        // the occupation, the possessions) is required and which a `.gw` event does not
        // have, and an LDS ordinance has no room for witnesses or a cause. The tag
        // therefore becomes the label of a generic event, which holds all of it.
        P::BaptismLds => EventMapping::generic("BAPL"),
        P::ConfirmationLds => EventMapping::generic("CONL"),
        P::Dotation => EventMapping::generic("ENDL"),
        P::Education => EventMapping::generic("EDUC"),
        P::Occupation => EventMapping::generic("OCCU"),
        P::Property => EventMapping::generic("PROP"),
        P::ScellentChildLds => EventMapping::generic("SLGC"),
        P::ScellentSpouseLds => EventMapping::generic("SLGS"),

        // Events with no GEDCOM counterpart. The labels are GeneWeb's own.
        P::Accomplishment => EventMapping::generic("Accomplishment"),
        P::Acquisition => EventMapping::generic("Acquisition"),
        P::Adhesion => EventMapping::generic("Membership"),
        P::ChangeName => EventMapping::generic("Change name"),
        P::Circumcision => EventMapping::generic("Circumcision"),
        P::Decoration => EventMapping::generic("Award"),
        P::MilitaryDemobilisation => EventMapping::generic("Military discharge"),
        P::Diploma => EventMapping::generic("Degree"),
        P::Distinction => EventMapping::generic("Distinction"),
        P::DotationLds => EventMapping::generic("DotationLDS"),
        P::Election => EventMapping::generic("Election"),
        P::Excommunication => EventMapping::generic("Excommunication"),
        P::FamilyLinkLds => EventMapping::generic("Family link LDS"),
        P::Funeral => EventMapping::generic("Funeral"),
        P::Hospitalisation => EventMapping::generic("Hospitalization"),
        P::Illness => EventMapping::generic("Illness"),
        P::PassengerList => EventMapping::generic("Passenger list"),
        P::MilitaryDistinction => EventMapping::generic("Military distinction"),
        P::MilitaryPromotion => EventMapping::generic("Military promotion"),
        P::MilitaryService => EventMapping::generic("Military service"),
        P::MilitaryMobilisation => EventMapping::generic("Military mobilization"),
        P::ScellentParentLds => EventMapping::generic("Scellent parent LDS"),
        P::PropertySale => EventMapping::generic("Property sale"),

        // A tag GeneWeb itself does not name: the label is the tag.
        P::Named(label) => EventMapping::generic(label),
    }
}

/// Maps a family event name.
#[must_use]
pub fn family_event(name: &FamilyEventName) -> EventMapping {
    use FamilyEventName as F;
    match name {
        F::Marriage => EventMapping::standard(EventKind::Marriage),
        F::Engagement => EventMapping::standard(EventKind::Engagement),
        F::Divorce => EventMapping::standard(EventKind::Divorce),
        // GEDCOM has no separation event (`gwb2ged` writes `SEP`, a tag of no GEDCOM
        // version): it is a generic event, labelled as GEDCOM readers label one.
        F::Separated => EventMapping::generic("Separation"),
        F::Annulment => EventMapping::standard(EventKind::Annulment),
        F::MarriageBann => EventMapping::standard(EventKind::MarriageBann),
        F::MarriageContract => EventMapping::standard(EventKind::MarriageContract),
        F::MarriageLicense => EventMapping::standard(EventKind::MarriageLicense),
        F::Residence => EventMapping::standard(EventKind::Residence),

        // No GEDCOM equivalent; GeneWeb's own labels.
        F::NoMarriage => EventMapping::generic("unmarried"),
        F::NoMention => EventMapping::generic("nomen"),
        F::Pacs => EventMapping::generic("pacs"),

        F::Named(label) => EventMapping::generic(label),
    }
}

/// The `RELA` value GeneWeb writes for a witness kind.
#[must_use]
pub fn witness_relationship(kind: WitnessKind) -> &'static str {
    match kind {
        WitnessKind::Witness => "Witness",
        WitnessKind::GodParent => "GODP",
        WitnessKind::CivilOfficer => "Civil officer",
        WitnessKind::ReligiousOfficer => "Religious officer",
        WitnessKind::Informant => "Informant",
        WitnessKind::Attending => "Attending",
        WitnessKind::Mentioned => "Mentioned",
        WitnessKind::Other => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn education_and_graduation_stay_distinct() {
        // `#educ` and `#grad` are two different facts: schooling, and the degree that
        // ends it. Both used to become `GRAD`, so a person's education was
        // indistinguishable from their graduation once exported — and reading the file
        // back could not tell them apart either. GEDCOM records schooling as the `EDUC`
        // attribute, whose required payload a `.gw` event lacks, so it takes the same
        // shape as `OCCU` and `PROP` above.
        let education = person_event(&PersonEventName::Education);
        assert_eq!(education.event, EventKind::Event);
        assert_eq!(education.event_type.as_deref(), Some("EDUC"));

        let graduation = person_event(&PersonEventName::Graduate);
        assert_eq!(graduation.event, EventKind::Graduation);
        assert_eq!(graduation.event_type, None);
    }

    #[test]
    fn lds_ordinances_stay_distinct_from_their_civil_counterparts() {
        // An LDS baptism and confirmation are ordinances of their own, with their
        // own GEDCOM tags. Folding them onto BAPM/CONF made a member's ordinance
        // indistinguishable from the ordinary sacrament once exported.
        for (name, tag) in [
            (PersonEventName::BaptismLds, "BAPL"),
            (PersonEventName::ConfirmationLds, "CONL"),
        ] {
            let m = person_event(&name);
            assert_eq!(m.event, EventKind::Event, "{tag}");
            assert_eq!(m.event_type.as_deref(), Some(tag));
        }

        // The civil ones keep their own standard tags.
        assert_eq!(
            person_event(&PersonEventName::Baptism).event,
            EventKind::Baptism
        );
        assert_eq!(
            person_event(&PersonEventName::Confirmation).event,
            EventKind::Confirmation
        );
    }

    #[test]
    fn every_named_event_maps_to_something() {
        // No tag may fall through to a placeholder: each is either a GEDCOM event or a
        // labelled generic one.
        for tag in PersonEventName::TAGS {
            let name = PersonEventName::from_tag(tag).expect("a known tag");
            let mapping = person_event(&name);
            if mapping.event == EventKind::Event {
                let label = mapping.event_type.expect("a generic event carries a label");
                assert!(!label.is_empty(), "{tag} has an empty label");
            }
        }
        for tag in FamilyEventName::TAGS {
            let name = FamilyEventName::from_tag(tag).expect("a known tag");
            let mapping = family_event(&name);
            if mapping.event == EventKind::Event {
                assert!(mapping.event_type.is_some_and(|l| !l.is_empty()));
            }
        }
    }

    #[test]
    fn the_common_events_use_standard_tags() {
        assert_eq!(
            person_event(&PersonEventName::Birth).event,
            EventKind::Birth
        );
        assert_eq!(
            person_event(&PersonEventName::Death).event,
            EventKind::Death
        );
        assert_eq!(
            family_event(&FamilyEventName::Marriage).event,
            EventKind::Marriage
        );
        assert_eq!(
            family_event(&FamilyEventName::Divorce).event,
            EventKind::Divorce
        );
    }

    #[test]
    fn an_unknown_tag_keeps_its_name_as_the_label() {
        let name = PersonEventName::from_tag("#future").unwrap();
        let mapping = person_event(&name);
        assert_eq!(mapping.event, EventKind::Event);
        assert_eq!(mapping.event_type.as_deref(), Some("future"));
    }

    #[test]
    fn witness_kinds_match_geneweb_rela_values() {
        assert_eq!(witness_relationship(WitnessKind::GodParent), "GODP");
        assert_eq!(witness_relationship(WitnessKind::Witness), "Witness");
        for tag in WitnessKind::TAGS {
            let kind = WitnessKind::from_tag(tag).unwrap();
            assert_ne!(witness_relationship(kind).len(), 0);
        }
    }
}
