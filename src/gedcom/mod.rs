//! Converting a `.gw` file into `ged_io`'s GEDCOM model.
//!
//! # What survives, and how
//!
//! GEDCOM cannot express everything GeneWeb records. Rather than drop what does not fit,
//! this conversion writes it as a user-defined tag — the `_GW…` family below — so a
//! round trip through GEDCOM keeps the information even when a consumer ignores it.
//!
//! | GeneWeb concept | GEDCOM |
//! |---|---|
//! | Person key occurrence number | `_GWOCC` |
//! | Access rights (`#apubl`, `#apriv`, `#semipub`) | `_GWACCESS`, plus `RESN confidential` for a restricted person |
//! | Portrait path (`#image`) | `_GWIMAGE` |
//! | Death reason (killed, murdered, executed, disappeared) | `_GWDEATH` on the death event |
//! | Union kind with no GEDCOM event (`#pacs`, `#noment`, …) | `_GWRELKIND` |
//! | Original `.gw` event tag, for generic events | `_GWTAG` on the event |
//! | Extended pages, database notes | `_GWPAGE` at record level |
//! | Wizard notes | `_GWWIZARD` at record level |
//!
//! Everything else maps onto standard GEDCOM: names and their aliases, sex, the four
//! life events, `gwplus` events with their witnesses, occupations, titles, sources,
//! notes, family links and the relations of a `rel` block.

pub mod date;
pub mod event;

use ged_io::types::custom::UserDefinedTag;
use std::fmt::Write as _;

use ged_io::types::event::detail::Detail;
use ged_io::types::header::source::HeadSour;
use ged_io::types::header::Header;
use ged_io::types::individual::association::Association;
use ged_io::types::individual::attribute::detail::AttributeDetail;
use ged_io::types::individual::attribute::IndividualAttribute;
use ged_io::types::individual::family_link::{FamilyLink, FamilyLinkType};
use ged_io::types::individual::gender::{Gender, GenderType};
use ged_io::types::individual::name::Name;
use ged_io::types::individual::Individual;
use ged_io::types::note::Note;
use ged_io::types::place::Place;
use ged_io::types::source::citation::{Citation, CitationSource};
use ged_io::types::{family::Family as GedFamily, GedcomData};

use crate::database::{FamilyId, FamilyRecord, GwDatabase, PersonId};
use crate::model::event::{Event as GwEvent, FamilyEventName as F, Witness, WitnessKind};
use crate::model::family::{Divorce, RelationKind};
use crate::model::person::{Access, Burial, Death, DeathReason, Person, Sex};
use crate::model::relation::RelationType;
use crate::model::{FamilyEvent, PersonEvent, Title, TitleName};

/// Occurrence number of a person's `.gw` key.
pub const TAG_OCCURRENCE: &str = "_GWOCC";
/// A person's access rights on a served base.
pub const TAG_ACCESS: &str = "_GWACCESS";
/// Path to a person's portrait.
pub const TAG_IMAGE: &str = "_GWIMAGE";
/// How a death came about.
pub const TAG_DEATH_REASON: &str = "_GWDEATH";
/// The kind of union, when GEDCOM has no event for it.
pub const TAG_RELATION_KIND: &str = "_GWRELKIND";
/// The original `.gw` tag of a generic event.
pub const TAG_EVENT: &str = "_GWTAG";
/// An extended wiki page or the base's presentation notes.
pub const TAG_PAGE: &str = "_GWPAGE";
/// A note attached to a contributor.
pub const TAG_WIZARD: &str = "_GWWIZARD";

fn individual_xref(id: PersonId) -> String {
    format!("@I{}@", id + 1)
}

fn family_xref(id: FamilyId) -> String {
    format!("@F{}@", id + 1)
}

fn custom(tag: &str, value: &str) -> UserDefinedTag {
    UserDefinedTag {
        tag: tag.to_owned(),
        value: Some(value.to_owned()),
        children: Vec::new(),
    }
}

/// A note, or nothing when the text is empty.
fn note(text: &str) -> Option<Note> {
    (!text.is_empty()).then(|| Note {
        value: Some(text.to_owned()),
        ..Note::default()
    })
}

/// A place, or nothing when the name is empty.
fn place(name: &str) -> Option<Place> {
    (!name.is_empty()).then(|| Place {
        value: Some(name.to_owned()),
        ..Place::default()
    })
}

/// A source citation carrying its text inline.
///
/// `.gw` sources are free text, not pointers into a source record, so they become
/// `SOUR <text>` rather than a cross-reference.
fn citation(text: &str) -> Option<Citation> {
    (!text.is_empty()).then(|| Citation {
        source: CitationSource::Description(text.to_owned()),
        page: None,
        data: None,
        note: None,
        certainty_assessment: None,
        submitter_registered_rfn: None,
        multimedia: Vec::new(),
        custom_data: Vec::new(),
        event_type: None,
        role: None,
    })
}

fn citations(text: &str) -> Vec<Citation> {
    citation(text).into_iter().collect()
}

/// An empty event detail of the given kind, ready to be filled in.
fn detail(event: ged_io::types::event::Event) -> Detail {
    Detail {
        event,
        value: None,
        date: None,
        place: None,
        note: None,
        family_link: None,
        family_event_details: Vec::new(),
        event_type: None,
        citations: Vec::new(),
        multimedia: Vec::new(),
        sort_date: None,
        associations: Vec::new(),
        cause: None,
        restriction: None,
        age: None,
        agency: None,
        religion: None,
    }
}

fn name_record(
    value: String,
    given: &str,
    surname: &str,
    name_type: Option<ged_io::types::individual::name::NameType>,
) -> Name {
    Name {
        value: Some(value),
        given: (!given.is_empty()).then(|| given.to_owned()),
        surname: (!surname.is_empty()).then(|| surname.to_owned()),
        prefix: None,
        surname_prefix: None,
        note: None,
        suffix: None,
        nickname: None,
        source: Vec::new(),
        name_type,
        phonetic: Vec::new(),
        romanized: Vec::new(),
        custom_data: Vec::new(),
    }
}

/// The `NAME` value GEDCOM expects: given name, then surname between slashes.
fn gedcom_name(given: &str, surname: &str) -> String {
    format!("{given} /{surname}/")
}

fn access_label(access: Access) -> Option<&'static str> {
    match access {
        // The default carries no information worth writing.
        Access::IfTitles => None,
        Access::Public => Some("public"),
        Access::Private => Some("private"),
        Access::SemiPublic => Some("semipublic"),
    }
}

fn death_reason_label(reason: DeathReason) -> Option<&'static str> {
    match reason {
        DeathReason::Unspecified => None,
        DeathReason::Killed => Some("killed"),
        DeathReason::Murdered => Some("murdered"),
        DeathReason::Executed => Some("executed"),
        DeathReason::Disappeared => Some("disappeared"),
    }
}

/// The GEDCOM event that records a union of this kind.
///
/// Several `.gw` union kinds explicitly assert that there was *no* marriage — `#nm`,
/// `#noment`, `#pacs`. Writing `MARR` for those would state the opposite of what the
/// file says, so they become a generic event carrying their own label instead.
fn relation_kind_event(kind: RelationKind) -> event::EventMapping {
    use ged_io::types::event::Event as E;
    let standard = |e: E| event::EventMapping {
        event: e,
        event_type: None,
    };
    let generic = |label: &str| event::EventMapping {
        event: E::Event,
        event_type: Some(label.to_owned()),
    };
    match kind {
        RelationKind::Married | RelationKind::NoSexesCheckMarried => standard(E::Marriage),
        RelationKind::Engaged => standard(E::Engagement),
        RelationKind::MarriageBann => standard(E::MarriageBann),
        RelationKind::MarriageContract => standard(E::MarriageContract),
        RelationKind::MarriageLicense => standard(E::MarriageLicense),
        RelationKind::Residence => standard(E::Residence),
        RelationKind::NotMarried | RelationKind::NoSexesCheckNotMarried => generic("unmarried"),
        RelationKind::NoMention => generic("nomen"),
        RelationKind::Pacs => generic("pacs"),
    }
}

fn relation_kind_label(kind: RelationKind) -> Option<&'static str> {
    match kind {
        RelationKind::Married => None,
        RelationKind::NotMarried => Some("not married"),
        RelationKind::Engaged => Some("engaged"),
        RelationKind::NoSexesCheckNotMarried => Some("not married, sexes unchecked"),
        RelationKind::NoSexesCheckMarried => Some("married, sexes unchecked"),
        RelationKind::NoMention => Some("no mention"),
        RelationKind::MarriageBann => Some("marriage bann"),
        RelationKind::MarriageContract => Some("marriage contract"),
        RelationKind::MarriageLicense => Some("marriage licence"),
        RelationKind::Pacs => Some("pacs"),
        RelationKind::Residence => Some("residence"),
    }
}

fn relation_type_label(relation: RelationType) -> &'static str {
    match relation {
        RelationType::Adoption => "adoptive parent",
        RelationType::Recognition => "recognising parent",
        RelationType::CandidateParent => "candidate parent",
        RelationType::GodParent => "godparent",
        RelationType::FosterParent => "foster parent",
    }
}

/// Renders a title as the free text GEDCOM's `TITL` attribute expects.
fn title_value(title: &Title) -> String {
    let mut out = title.ident.clone();
    if !title.place.is_empty() {
        let _ = write!(out, " {}", title.place);
    }
    if title.nth != 0 {
        let _ = write!(out, " {}", title.nth);
    }
    if let TitleName::Name(name) = &title.name {
        let _ = write!(out, " ({name})");
    }
    out
}

// Some of these read no database state today, but they are part of one conversion
// and are kept together so callers see a single, consistent surface.
impl GwDatabase {
    /// Converts this database into `ged_io`'s GEDCOM model.
    ///
    /// The result can be serialised with [`ged_io::writer::GedcomWriter`] or inspected
    /// with the rest of the `ged_io` API.
    #[must_use]
    pub fn to_gedcom(&self) -> GedcomData {
        let mut data = GedcomData {
            header: Some(header()),
            individuals: Vec::with_capacity(self.persons.len()),
            families: Vec::with_capacity(self.families.len()),
            ..GedcomData::default()
        };

        // Which families each person belongs to, and how.
        let mut links: Vec<Vec<FamilyLink>> = vec![Vec::new(); self.persons.len()];
        for (id, family) in self.families.iter().enumerate() {
            let xref = family_xref(id);
            for parent in [family.father, family.mother] {
                links[parent].push(family_link(&xref, FamilyLinkType::Spouse));
            }
            for &child in &family.children {
                links[child].push(family_link(&xref, FamilyLinkType::Child));
            }
        }

        for (id, person) in self.persons.iter().enumerate() {
            data.individuals
                .push(self.individual(id, person, std::mem::take(&mut links[id])));
        }
        for (id, family) in self.families.iter().enumerate() {
            data.families.push(self.family(id, family));
        }

        for page in &self.pages {
            data.custom_data.push(Box::new(page_tag(TAG_PAGE, page)));
        }
        for wizard in &self.wizard_notes {
            data.custom_data
                .push(Box::new(page_tag(TAG_WIZARD, wizard)));
        }

        data
    }

    fn individual(&self, id: PersonId, person: &Person, families: Vec<FamilyLink>) -> Individual {
        let mut individual = Individual {
            xref: Some(individual_xref(id)),
            families,
            ..Individual::default()
        };

        individual.names = names(person);
        individual.sex = gender(person.sex);
        individual.note = note(&person.notes);
        individual.source = citations(&person.sources);
        individual.events = self.life_events(person);
        individual.attributes = attributes(person);
        individual.associations = self.associations(person);
        // Witnesses to a person's own events belong on the individual, not nested inside
        // the event: GEDCOM 5.5.1 puts `ASSO` directly under `INDI`, and readers reject
        // the nested form. Which event each witness attended is not expressible here; the
        // `.gw` syntax tree keeps it.
        for gw_event in &person.events {
            individual
                .associations
                .extend(self.witness_associations(&gw_event.witnesses));
        }

        // A portrait is a standard GEDCOM multimedia object, not just a custom tag.
        if !person.image.is_empty() {
            individual.multimedia.push(multimedia(&person.image));
        }

        if person.occ != 0 {
            individual
                .custom_data
                .push(Box::new(custom(TAG_OCCURRENCE, &person.occ.to_string())));
        }
        if let Some(label) = access_label(person.access) {
            individual
                .custom_data
                .push(Box::new(custom(TAG_ACCESS, label)));
        }
        // What GEDCOM itself can say about it: a person hidden from the public is
        // `RESN confidential`. `_GWACCESS` keeps the exact GeneWeb setting.
        if matches!(person.access, Access::Private | Access::SemiPublic) {
            individual.restriction = Some("confidential".to_owned());
        }
        if !person.image.is_empty() {
            individual
                .custom_data
                .push(Box::new(custom(TAG_IMAGE, &person.image)));
        }

        individual
    }

    /// Birth, baptism, death and burial, followed by any `pevt` events.
    ///
    /// A `pevt` block may restate an event the person's own line already carries. The
    /// `gwplus` rule is that the structured event wins, so a life event that also appears
    /// in the block is emitted once, from the block.
    fn life_events(&self, person: &Person) -> Vec<Detail> {
        use crate::model::event::PersonEventName as P;
        use ged_io::types::event::Event as E;
        let mut events = Vec::new();

        let superseded = |kind: &P| person.events.iter().any(|e| &e.name == kind);
        let birth_superseded = superseded(&P::Birth);
        let baptism_superseded = superseded(&P::Baptism);
        let death_superseded = superseded(&P::Death);
        let burial_superseded = superseded(&P::Burial) || superseded(&P::Cremation);

        if !birth_superseded
            && (person.birth.is_some()
                || !person.birth_place.is_empty()
                || !person.birth_src.is_empty()
                || !person.birth_note.is_empty())
        {
            let mut d = detail(E::Birth);
            d.date = person.birth.as_ref().map(date::to_gedcom);
            d.place = place(&person.birth_place);
            d.note = note(&person.birth_note);
            d.citations = citations(&person.birth_src);
            events.push(d);
        }

        if !baptism_superseded
            && (person.baptism.is_some()
                || !person.baptism_place.is_empty()
                || !person.baptism_src.is_empty()
                || !person.baptism_note.is_empty())
        {
            let mut d = detail(E::Baptism);
            d.date = person.baptism.as_ref().map(date::to_gedcom);
            d.place = place(&person.baptism_place);
            d.note = note(&person.baptism_note);
            d.citations = citations(&person.baptism_src);
            events.push(d);
        }

        if let Some(mut d) = death_event(&person.death).filter(|_| !death_superseded) {
            d.place = place(&person.death_place);
            d.note = note(&person.death_note);
            d.citations = citations(&person.death_src);
            events.push(d);
        }

        let burial = match &person.burial {
            _ if burial_superseded => None,
            Burial::Unknown => None,
            Burial::Buried(date) => Some((E::Burial, date)),
            Burial::Cremated(date) => Some((E::Cremation, date)),
        };
        if let Some((kind, when)) = burial {
            let mut d = detail(kind);
            d.date = when.as_ref().map(date::to_gedcom);
            d.place = place(&person.burial_place);
            d.note = note(&person.burial_note);
            d.citations = citations(&person.burial_src);
            events.push(d);
        }

        for gw_event in &person.events {
            let mapping = event::person_event(&gw_event.name);
            events.push(self.event_detail(gw_event, mapping, &gw_event.name.tag()));
        }

        events
    }

    /// Shapes a `gwplus` event, personal or familial, into a GEDCOM event detail.
    fn event_detail<N>(
        &self,
        gw_event: &GwEvent<N>,
        mapping: event::EventMapping,
        original_tag: &str,
    ) -> Detail {
        let mut d = detail(mapping.event.clone());
        d.event_type = mapping.event_type;
        d.date = gw_event.date.as_ref().map(date::to_gedcom);
        d.place = place(&gw_event.place);
        d.note = note(&gw_event.note);
        d.citations = citations(&gw_event.source);
        d.cause = (!gw_event.cause.is_empty()).then(|| gw_event.cause.clone());
        d.associations = self.witness_associations(&gw_event.witnesses);
        // An event that became a generic `EVEN` has lost which `.gw` tag it came from.
        // Recording the tag keeps the mapping reversible.
        if d.event_type.is_some() {
            d.custom_data_push(TAG_EVENT, original_tag);
        }
        d
    }

    /// Turns event witnesses into GEDCOM associations pointing at the witness.
    fn witness_associations(&self, witnesses: &[Witness]) -> Vec<Association> {
        witnesses
            .iter()
            .filter_map(|w| {
                let id = self.lookup(&w.person.key())?;
                Some(Association {
                    xref: individual_xref(id),
                    relationship: Some(event::witness_relationship(w.kind).to_owned()),
                    association_type: Some("INDI".to_owned()),
                    note: None,
                    custom_data: Vec::new(),
                })
            })
            .collect()
    }

    /// The relations of a `rel` block, as GEDCOM associations.
    fn associations(&self, person: &Person) -> Vec<Association> {
        let mut out = Vec::new();
        for relation in &person.relations {
            let label = relation_type_label(relation.relation_type);
            for who in [relation.father.as_ref(), relation.mother.as_ref()]
                .into_iter()
                .flatten()
            {
                if let Some(id) = self.lookup(&who.key()) {
                    out.push(Association {
                        xref: individual_xref(id),
                        relationship: Some(label.to_owned()),
                        association_type: Some("INDI".to_owned()),
                        note: None,
                        custom_data: Vec::new(),
                    });
                }
            }
        }
        out
    }

    fn family(&self, id: FamilyId, family: &FamilyRecord) -> GedFamily {
        use ged_io::types::event::Event as E;

        let mut out = GedFamily {
            xref: Some(family_xref(id)),
            individual1: Some(individual_xref(family.father)),
            individual2: Some(individual_xref(family.mother)),
            children: family
                .children
                .iter()
                .copied()
                .map(individual_xref)
                .collect(),
            ..GedFamily::default()
        };

        out.sources = citations(&family.sources);
        out.notes = note(&family.comment).into_iter().collect();

        // As with personal events, a `fevt` entry supersedes what the `fam` line says
        // about the same thing rather than adding a second copy of it.
        let has_event = |kind: &F| family.events.iter().any(|e| &e.name == kind);
        let marriage_superseded = has_event(&F::Marriage);
        let divorce_superseded = has_event(&F::Divorce);
        let separation_superseded = has_event(&F::Separated);

        // The union itself.
        let union = relation_kind_event(family.relation);
        if !marriage_superseded
            && (family.marriage.is_some()
                || !family.marriage_place.is_empty()
                || !family.marriage_src.is_empty()
                || !family.marriage_note.is_empty()
                || family.relation != RelationKind::Married)
        {
            let mut d = detail(union.event);
            d.event_type = union.event_type;
            d.date = family.marriage.as_ref().map(date::to_gedcom);
            d.place = place(&family.marriage_place);
            d.note = note(&family.marriage_note);
            d.citations = citations(&family.marriage_src);
            d.associations = family_witness_associations(family);
            out.events.push(d);
        }

        let ending = match &family.divorce {
            Divorce::Divorced(when) if !divorce_superseded => Some((E::Divorce, when)),
            Divorce::Separated(when) if !separation_superseded => Some((E::Separated, when)),
            // Either the union never ended, or a `fevt` entry already says how it did.
            _ => None,
        };
        if let Some((kind, when)) = ending {
            let mut d = detail(kind);
            d.date = when.as_ref().map(date::to_gedcom);
            out.events.push(d);
        }

        for gw_event in &family.events {
            let mapping = event::family_event(&gw_event.name);
            out.events
                .push(self.event_detail(gw_event, mapping, &gw_event.name.tag()));
        }

        if let Some(label) = relation_kind_label(family.relation) {
            out.custom_data
                .push(Box::new(custom(TAG_RELATION_KIND, label)));
        }

        out
    }
}

fn header() -> Header {
    Header {
        source: Some(HeadSour {
            value: Some("GENEWEB".to_owned()),
            version: None,
            name: Some(concat!("geneweb ", env!("CARGO_PKG_VERSION")).to_owned()),
            corporation: None,
            data: None,
        }),
        ..Header::default()
    }
}

/// Builds the `NAME` records: the primary one, then every alias GeneWeb records.
fn names(person: &Person) -> Vec<Name> {
    use ged_io::types::individual::name::NameType;

    let mut names = vec![name_record(
        gedcom_name(&person.first_name, &person.surname),
        &person.first_name,
        &person.surname,
        None,
    )];

    // A nickname belongs on the primary name.
    if let Some(nickname) = person.qualifiers.first() {
        names[0].nickname = Some(nickname.clone());
    }

    if !person.public_name.is_empty() {
        names.push(name_record(
            gedcom_name(&person.public_name, &person.surname),
            &person.public_name,
            &person.surname,
            Some(NameType::Aka),
        ));
    }
    for given in &person.first_names_aliases {
        names.push(name_record(
            gedcom_name(given, &person.surname),
            given,
            &person.surname,
            Some(NameType::Aka),
        ));
    }
    for surname in &person.surnames_aliases {
        names.push(name_record(
            gedcom_name(&person.first_name, surname),
            &person.first_name,
            surname,
            Some(NameType::Aka),
        ));
    }
    for alias in &person.aliases {
        names.push(name_record(alias.clone(), "", "", Some(NameType::Aka)));
    }
    names
}

fn family_witness_associations(family: &FamilyRecord) -> Vec<Association> {
    family
        .witnesses
        .iter()
        .map(|w| Association {
            xref: individual_xref(w.person),
            relationship: Some(event::witness_relationship(w.kind).to_owned()),
            association_type: Some("INDI".to_owned()),
            note: None,
            custom_data: Vec::new(),
        })
        .collect()
}

fn family_link(xref: &str, kind: FamilyLinkType) -> FamilyLink {
    FamilyLink {
        xref: xref.to_owned(),
        family_link_type: kind,
        pedigree_linkage_type: None,
        child_linkage_status: None,
        adopted_by: None,
        note: None,
        custom_data: Vec::new(),
    }
}

fn gender(sex: Sex) -> Option<Gender> {
    let value = match sex {
        Sex::Male => GenderType::Male,
        Sex::Female => GenderType::Female,
        // GeneWeb's "neuter" means unrecorded, not non-binary.
        Sex::Neuter => return None,
    };
    Some(Gender {
        value,
        fact: None,
        sources: Vec::new(),
        custom_data: Vec::new(),
    })
}

/// The death event, if the person is recorded as dead.
fn death_event(death: &Death) -> Option<Detail> {
    use ged_io::types::event::Event as E;
    let mut d = detail(E::Death);
    match death {
        // Nothing to record: alive, or simply unknown.
        Death::NotDead | Death::DontKnowIfDead => return None,
        Death::Dead { reason, date } => {
            d.date = Some(date::to_gedcom(date));
            if let Some(label) = death_reason_label(*reason) {
                d.custom_data_push(TAG_DEATH_REASON, label);
            }
        }
        // `1 DEAT Y` is the GEDCOM idiom for a death with no details. The nuance
        // GeneWeb draws between these three goes in the note, since GEDCOM has no
        // vocabulary for it.
        Death::DeadDontKnowWhen => d.value = Some("Y".to_owned()),
        Death::DeadYoung => {
            d.value = Some("Y".to_owned());
            d.custom_data_push(TAG_DEATH_REASON, "died young");
        }
        Death::OfCourseDead => {
            d.value = Some("Y".to_owned());
            d.custom_data_push(TAG_DEATH_REASON, "presumed dead");
        }
    }
    Some(d)
}

/// A multimedia object pointing at a portrait file.
fn multimedia(path: &str) -> ged_io::types::multimedia::Multimedia {
    use ged_io::types::multimedia::{file::Reference, Multimedia};
    Multimedia {
        file: Some(Reference {
            value: Some(path.to_owned()),
            ..Reference::default()
        }),
        ..Multimedia::default()
    }
}

/// `Detail` has no custom-data field, so a death reason rides on the event's note.
trait DetailExt {
    fn custom_data_push(&mut self, tag: &str, value: &str);
}

impl DetailExt for Detail {
    fn custom_data_push(&mut self, tag: &str, value: &str) {
        let text = format!("{tag} {value}");
        self.note = Some(match self.note.take() {
            Some(mut existing) => {
                let value = existing.value.get_or_insert_with(String::new);
                if !value.is_empty() {
                    value.push('\n');
                }
                value.push_str(&text);
                existing
            }
            None => Note {
                value: Some(text),
                ..Note::default()
            },
        });
    }
}

fn attributes(person: &Person) -> Vec<AttributeDetail> {
    let mut out = Vec::new();
    if !person.occupation.is_empty() {
        out.push(attribute(
            IndividualAttribute::Occupation,
            &person.occupation,
        ));
    }
    for title in &person.titles {
        let mut a = attribute(IndividualAttribute::NobilityTypeTitle, &title_value(title));
        a.date = title.date_start.as_ref().map(date::to_gedcom);
        a.place = place(&title.place);
        out.push(a);
    }
    out
}

fn attribute(kind: IndividualAttribute, value: &str) -> AttributeDetail {
    AttributeDetail {
        attribute: kind,
        value: Some(value.to_owned()),
        place: None,
        date: None,
        sources: Vec::new(),
        note: None,
        attribute_type: None,
        restriction: None,
        age: None,
        address: None,
        cause: None,
        agency: None,
        multimedia: Vec::new(),
    }
}

fn page_tag(tag: &str, page: &crate::database::Page) -> UserDefinedTag {
    UserDefinedTag {
        tag: tag.to_owned(),
        value: (!page.name.is_empty()).then(|| page.name.clone()),
        children: vec![Box::new(UserDefinedTag {
            tag: "NOTE".to_owned(),
            value: Some(page.text.clone()),
            children: Vec::new(),
        })],
    }
}

/// Silences the unused-import warning for types referenced only in documentation.
const _: Option<(&PersonEvent, &FamilyEvent, WitnessKind)> = None;

#[cfg(test)]
mod tests {
    use super::*;
    use ged_io::types::event::Event as E;

    fn convert(input: &str) -> GedcomData {
        GwDatabase::read(input.as_bytes(), "t.gw")
            .expect("parses")
            .to_gedcom()
    }

    #[test]
    fn a_family_becomes_two_individuals_and_a_family_record() {
        let data = convert("fam Dupont Jean + Martin Marie\nbeg\n- m Paul 1930\nend\n");
        assert_eq!(data.individuals.len(), 3);
        assert_eq!(data.families.len(), 1);

        let family = &data.families[0];
        assert_eq!(family.xref.as_deref(), Some("@F1@"));
        assert_eq!(family.individual1.as_deref(), Some("@I1@"));
        assert_eq!(family.individual2.as_deref(), Some("@I2@"));
        assert_eq!(family.children, vec!["@I3@"]);
    }

    #[test]
    fn names_use_gedcom_slash_syntax() {
        let data = convert("fam Dupont Jean + Martin Marie\n");
        assert_eq!(
            data.individuals[0].names[0].value.as_deref(),
            Some("Jean /Dupont/")
        );
        assert_eq!(
            data.individuals[0].names[0].surname.as_deref(),
            Some("Dupont")
        );
    }

    #[test]
    fn aliases_become_additional_name_records() {
        let data = convert("fam Dupont Jean {Jeannot} #salias Dupond (Le_Grand) 1900 + A B\n");
        let names = &data.individuals[0].names;
        let values: Vec<_> = names.iter().filter_map(|n| n.value.as_deref()).collect();
        assert!(values.contains(&"Jean /Dupont/"));
        assert!(values.contains(&"Le Grand /Dupont/"));
        assert!(values.contains(&"Jeannot /Dupont/"));
        assert!(values.contains(&"Jean /Dupond/"));
    }

    #[test]
    fn parents_get_a_spouse_link_and_children_a_child_link() {
        let data = convert("fam Dupont Jean + Martin Marie\nbeg\n- m Paul\nend\n");
        assert_eq!(
            data.individuals[0].families[0].family_link_type,
            FamilyLinkType::Spouse
        );
        assert_eq!(
            data.individuals[2].families[0].family_link_type,
            FamilyLinkType::Child
        );
    }

    #[test]
    fn access_restrictions_are_also_a_resn() {
        let data = convert(concat!(
            "fam Doe John #apriv + Roe Jane #semipub\n",
            "fam Poe Paul #apubl + Moe Mary\n",
        ));
        let restriction = |i: usize| data.individuals[i].restriction.as_deref();
        assert_eq!(restriction(0), Some("confidential"));
        assert_eq!(restriction(1), Some("confidential"));
        assert_eq!(restriction(2), None);
        assert_eq!(restriction(3), None);
    }

    #[test]
    fn sex_is_written_only_when_recorded() {
        let data = convert("fam Dupont Jean + Martin Marie\n");
        assert_eq!(
            data.individuals[0].sex.as_ref().map(|g| g.value.clone()),
            Some(GenderType::Male)
        );
        assert_eq!(
            data.individuals[1].sex.as_ref().map(|g| g.value.clone()),
            Some(GenderType::Female)
        );
    }

    #[test]
    fn life_events_carry_dates_places_and_sources() {
        let data =
            convert("fam Dupont Jean 7/9/1830 #bp Reims #bs acte 12/5/1900 #dp Lyon + A B\n");
        let events = &data.individuals[0].events;
        let birth = events
            .iter()
            .find(|e| e.event == E::Birth)
            .expect("a birth");
        assert_eq!(
            birth.date.as_ref().unwrap().value.as_deref(),
            Some("07 SEP 1830")
        );
        assert_eq!(
            birth.place.as_ref().unwrap().value.as_deref(),
            Some("Reims")
        );
        assert_eq!(birth.citations.len(), 1);
        let death = events
            .iter()
            .find(|e| e.event == E::Death)
            .expect("a death");
        assert_eq!(death.place.as_ref().unwrap().value.as_deref(), Some("Lyon"));
    }

    #[test]
    fn a_living_person_gets_no_death_event() {
        let data = convert("fam Dupont Jean 1990 + A B\n");
        assert!(!data.individuals[0]
            .events
            .iter()
            .any(|e| e.event == E::Death));
    }

    #[test]
    fn occupations_and_titles_become_attributes() {
        let data = convert("fam Dupont Jean [*:duc:Bretagne:::] #occu Marchand 1900 + A B\n");
        let attrs = &data.individuals[0].attributes;
        assert!(attrs
            .iter()
            .any(|a| a.attribute == IndividualAttribute::Occupation
                && a.value.as_deref() == Some("Marchand")));
        assert!(attrs
            .iter()
            .any(|a| a.attribute == IndividualAttribute::NobilityTypeTitle
                && a.value.as_deref() == Some("duc Bretagne")));
    }

    #[test]
    fn geneweb_only_concepts_ride_on_custom_tags() {
        let data = convert("fam Dupont Jean.2 #image p.jpg #apriv 1900 + A B\n");
        let portrait = data.individuals[0]
            .multimedia
            .first()
            .expect("the portrait becomes GEDCOM multimedia");
        assert_eq!(
            portrait
                .file
                .as_ref()
                .and_then(|file| file.value.as_deref()),
            Some("p.jpg")
        );
        let tags: Vec<_> = data.individuals[0]
            .custom_data
            .iter()
            .map(|t| (t.tag.as_str(), t.value.as_deref()))
            .collect();
        assert!(tags.contains(&(TAG_OCCURRENCE, Some("2"))));
        assert!(tags.contains(&(TAG_ACCESS, Some("private"))));
        assert!(tags.contains(&(TAG_IMAGE, Some("p.jpg"))));
    }

    #[test]
    fn a_pacs_keeps_its_kind_because_gedcom_has_no_event_for_it() {
        let data = convert("fam A B + #pacs mf C D\n");
        let family = &data.families[0];
        assert!(family
            .custom_data
            .iter()
            .any(|t| t.tag == TAG_RELATION_KIND && t.value.as_deref() == Some("pacs")));
    }

    #[test]
    fn family_events_and_divorce() {
        let data = convert("fam A B +1850 -1860 C D\n");
        let events = &data.families[0].events;
        assert!(events.iter().any(|e| e.event == E::Marriage));
        let divorce = events
            .iter()
            .find(|e| e.event == E::Divorce)
            .expect("a divorce");
        assert_eq!(
            divorce.date.as_ref().unwrap().value.as_deref(),
            Some("1860")
        );
    }

    #[test]
    fn generic_events_keep_their_label() {
        let data = convert("fam A B + C D\npevt A B\n#hosp 1914\nend pevt\n");
        let event = data.individuals[0]
            .events
            .iter()
            .find(|e| e.event == E::Event)
            .expect("a generic event");
        assert_eq!(event.event_type.as_deref(), Some("Hospitalization"));
    }

    #[test]
    fn event_witnesses_become_associations() {
        let data = convert(concat!(
            "fam Dupont Jean + Martin Marie\n",
            "pevt Dupont Jean\n",
            "#birt 1850\n",
            "wit m: #godp Martin Marie\n",
            "end pevt\n",
        ));
        let birth = data.individuals[0]
            .events
            .iter()
            .find(|e| e.event == E::Birth)
            .expect("a birth");
        assert_eq!(birth.associations.len(), 1);
        assert_eq!(birth.associations[0].xref, "@I2@");
        assert_eq!(birth.associations[0].relationship.as_deref(), Some("GODP"));
    }

    #[test]
    fn relations_become_associations() {
        let data =
            convert("fam Dupont Jean + A B\nrel Dupont Jean\nbeg\n- adop fath: Martin Paul\nend\n");
        let assoc = &data.individuals[0].associations;
        assert_eq!(assoc.len(), 1);
        assert_eq!(assoc[0].relationship.as_deref(), Some("adoptive parent"));
    }

    #[test]
    fn pages_and_wizard_notes_are_kept_at_record_level() {
        let data = convert(concat!(
            "page-ext Gallery\n  content\nend page-ext\n",
            "wizard-note henri\n  1234\nend wizard-note\n",
        ));
        let tags: Vec<_> = data.custom_data.iter().map(|t| t.tag.as_str()).collect();
        assert!(tags.contains(&TAG_PAGE));
        assert!(tags.contains(&TAG_WIZARD));
    }

    #[test]
    fn the_header_records_the_producer() {
        let data = convert("fam A B + C D\n");
        let source = data.header.unwrap().source.unwrap();
        assert_eq!(source.value.as_deref(), Some("GENEWEB"));
        assert!(source.name.unwrap().starts_with("geneweb "));
    }
}
