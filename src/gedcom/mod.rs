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
//! notes, family links and the relations of a `rel` block: adoptive and foster parents
//! as a family of their own (`FAMC` with `PEDI`, and an `ADOP` event for an adoption), the
//! other relations as `ASSO`.

pub mod date;
pub mod event;

use std::fmt::Write as _;

use ged_io::model::ChildLink;
use ged_io::model::{
    Adoption, Association, Citation, CitationSource, Dataset, EnumList, Event, EventFamily,
    EventKind, Family, File, Header, HeaderSource, Individual, IndividualRef, MultimediaLink, Name,
    NamePiece, NamePieceKind, NameType, Node, Note, Pedigree, Phrased, Place, Restriction,
    Sex as GedSex, SpouseLink, Store, TagId, Text, ThinVec, Value, XrefId,
};
use ged_io::GedcomVersion;

use crate::database::{FamilyId, FamilyRecord, GwDatabase, PersonId, ResolvedWitness};
use crate::model::event::{Event as GwEvent, FamilyEventName as F, WitnessKind};
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

/// The identifiers and extension tags of one conversion, interned in the dataset's
/// store before any record is built, so that building them reads the store no more.
struct Ids {
    /// `@I{n}@` of each person, by [`PersonId`].
    persons: Vec<XrefId>,
    /// `@F{n}@` of each family: the file's, then the ones `rel` blocks imply.
    families: Vec<XrefId>,
    occurrence: TagId,
    access: TagId,
    image: TagId,
    death_reason: TagId,
    relation_kind: TagId,
    event: TagId,
    page: TagId,
    wizard: TagId,
    note: TagId,
}

/// Interns an identifier. A store holds four billion of them; a `.gw` file never
/// comes close.
fn intern(store: &mut Store, xref: &str) -> XrefId {
    store
        .intern_xref(xref)
        .expect("a store holds four billion identifiers")
}

impl Ids {
    fn new(store: &mut Store, persons: usize, families: usize) -> Self {
        Self {
            persons: (1..=persons)
                .map(|n| intern(store, &format!("@I{n}@")))
                .collect(),
            families: (1..=families)
                .map(|n| intern(store, &format!("@F{n}@")))
                .collect(),
            occurrence: store.intern_tag(TAG_OCCURRENCE),
            access: store.intern_tag(TAG_ACCESS),
            image: store.intern_tag(TAG_IMAGE),
            death_reason: store.intern_tag(TAG_DEATH_REASON),
            relation_kind: store.intern_tag(TAG_RELATION_KIND),
            event: store.intern_tag(TAG_EVENT),
            page: store.intern_tag(TAG_PAGE),
            wizard: store.intern_tag(TAG_WIZARD),
            note: store.intern_tag("NOTE"),
        }
    }

    fn person(&self, id: PersonId) -> XrefId {
        self.persons[id]
    }

    fn family(&self, id: FamilyId) -> XrefId {
        self.families[id]
    }
}

/// A user-defined structure holding a text.
fn custom(tag: TagId, value: &str) -> Node {
    Node {
        payload: Value::Text(Text::from(value)),
        ..Node::new(tag)
    }
}

/// A note, or none when the text is empty.
fn notes<C: FromIterator<Note>>(text: &str) -> C {
    (!text.is_empty())
        .then(|| Note::text(text))
        .into_iter()
        .collect()
}

/// A place, or nothing when the name is empty.
fn place(name: &str) -> Option<Place> {
    (!name.is_empty()).then(|| Place {
        name: Text::from(name),
        ..Place::default()
    })
}

/// A source citation carrying its text inline.
///
/// `.gw` sources are free text, not pointers into a source record, so they become
/// `SOUR <text>` rather than a cross-reference.
fn citations<C: FromIterator<Citation>>(text: &str) -> C {
    (!text.is_empty())
        .then(|| Citation {
            source: CitationSource::Description(Text::from(text)),
            ..Citation::default()
        })
        .into_iter()
        .collect()
}

/// Sets the notes of an event, leaving its detail unallocated when there are none.
fn set_notes(event: &mut Event, text: &str) {
    if !text.is_empty() {
        event.detail_mut().notes = notes(text);
    }
}

/// A name: its value, and its given name and surname as pieces.
fn name_record(value: String, given: &str, surname: &str, name_type: Option<NameType>) -> Name {
    let mut name = Name::new(value);
    for (kind, piece) in [
        (NamePieceKind::Given, given),
        (NamePieceKind::Surname, surname),
    ] {
        if !piece.is_empty() {
            name.pieces.push(NamePiece {
                kind,
                value: Text::from(piece),
            });
        }
    }
    if let Some(kind) = name_type {
        name.detail_mut().kind = Some(Phrased::new(kind));
    }
    name
}

/// Adds a nickname (`NICK`) to a name, in the place GEDCOM gives the piece: after the
/// given names, before the surname.
fn add_nickname(name: &mut Name, nickname: &str) {
    let at = name
        .pieces
        .iter()
        .position(|p| p.kind == NamePieceKind::Surname)
        .unwrap_or(name.pieces.len());
    let mut pieces = std::mem::take(&mut name.pieces).into_vec();
    pieces.insert(
        at,
        NamePiece {
            kind: NamePieceKind::Nickname,
            value: Text::from(nickname),
        },
    );
    name.pieces = ThinVec::from(pieces);
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
    let standard = |e: EventKind| event::EventMapping {
        event: e,
        event_type: None,
    };
    let generic = |label: &str| event::EventMapping {
        event: EventKind::Event,
        event_type: Some(label.to_owned()),
    };
    match kind {
        RelationKind::Married | RelationKind::NoSexesCheckMarried => standard(EventKind::Marriage),
        RelationKind::Engaged => standard(EventKind::Engagement),
        RelationKind::MarriageBann => standard(EventKind::MarriageBann),
        RelationKind::MarriageContract => standard(EventKind::MarriageContract),
        RelationKind::MarriageLicense => standard(EventKind::MarriageLicense),
        RelationKind::Residence => standard(EventKind::Residence),
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

/// A family a `rel` block implies: a child with their adoptive or foster parents.
struct ImpliedFamily<'a> {
    child: PersonId,
    father: Option<PersonId>,
    mother: Option<PersonId>,
    pedigree: Pedigree,
    sources: &'a str,
}

/// The `ADOP` event of an adopted child, pointing at the adoptive family and saying
/// which of its parents adopted.
fn adoption_event(family: XrefId, implied: &ImpliedFamily<'_>) -> Event {
    let adopted_by = match (implied.father, implied.mother) {
        (Some(_), Some(_)) => Adoption::Both,
        (Some(_), None) => Adoption::Husband,
        _ => Adoption::Wife,
    };
    let mut d = Event::new(EventKind::Adoption);
    d.detail_mut().family = Some(EventFamily {
        family: Some(family),
        adopted_by: Some(Phrased::new(adopted_by)),
        ..EventFamily::default()
    });
    d
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

/// Renders a title as the free text GEDCOM's `TITL` attribute expects, as `gwb2ged`
/// writes it: `ident, place, nth`.
///
/// The place of a title is the domain it is held over (`de France`, `d'Anvers`), part of
/// its name, not a locality: it stays in the text and is not a `PLAC`.
fn title_value(title: &Title) -> String {
    let mut out = title.ident.clone();
    if !title.place.is_empty() {
        let _ = write!(out, ", {}", title.place);
    }
    if title.nth != 0 {
        let _ = write!(out, ", {}", title.nth);
    }
    out
}

/// The period a title was held, as `gwb2ged` writes it: `FROM start TO end`, either end
/// possibly missing.
fn title_period(title: &Title) -> Option<ged_io::model::Date> {
    let bound =
        |d: &Option<crate::date::GwDate>| d.as_ref().map(date::value).filter(|v| !v.is_empty());
    let value = match (bound(&title.date_start), bound(&title.date_end)) {
        (None, None) => return None,
        (Some(start), None) => format!("FROM {start}"),
        (None, Some(end)) => format!("TO {end}"),
        (Some(start), Some(end)) => format!("FROM {start} TO {end}"),
    };
    Some(ged_io::model::Date::new(value))
}

/// The name a title is held under, as `gwb2ged` notes it: the person's public name for
/// the main title, or the name given in the title.
fn title_holder<'a>(title: &'a Title, person: &'a Person) -> &'a str {
    match &title.name {
        TitleName::Main => &person.public_name,
        TitleName::Name(name) => name,
        TitleName::None => "",
    }
}

// Some of these read no database state today, but they are part of one conversion
// and are kept together so callers see a single, consistent surface.
impl GwDatabase {
    /// Converts this database into `ged_io`'s GEDCOM model: a GEDCOM 5.5.1 dataset
    /// whose texts it owns.
    ///
    /// The result can be written with [`ged_io::GedcomWriter`], in any version it
    /// writes, or inspected with the rest of the `ged_io` API.
    #[must_use]
    pub fn to_gedcom(&self) -> Dataset {
        let implied = self.implied_families();
        let mut data = Dataset::new(GedcomVersion::V5_5_1);
        let ids = Ids::new(
            data.store_mut(),
            self.persons.len(),
            self.families.len() + implied.len(),
        );
        data.header = Some(header());

        // Which families each person belongs to, and how.
        let mut spouse_of: Vec<Vec<SpouseLink>> = vec![Vec::new(); self.persons.len()];
        let mut child_of: Vec<Vec<ChildLink>> = vec![Vec::new(); self.persons.len()];
        for (id, family) in self.families.iter().enumerate() {
            let xref = ids.family(id);
            for parent in [family.father, family.mother] {
                spouse_of[parent].push(SpouseLink::new(xref));
            }
            for &child in &family.children {
                child_of[child].push(ChildLink::new(xref));
            }
        }

        // Adoptive and foster parents named in `rel` blocks form families of their own,
        // numbered after the file's.
        for (i, family) in implied.iter().enumerate() {
            let xref = ids.family(self.families.len() + i);
            for parent in [family.father, family.mother].into_iter().flatten() {
                spouse_of[parent].push(SpouseLink::new(xref));
            }
            let mut link = ChildLink::new(xref);
            link.detail_mut().pedigree = Some(Phrased::new(family.pedigree.clone()));
            child_of[family.child].push(link);
        }

        data.individuals.reserve(self.persons.len());
        for (id, person) in self.persons.iter().enumerate() {
            let mut individual = self.individual(&ids, id, person);
            individual.spouse_of = std::mem::take(&mut spouse_of[id]).into();
            individual.child_of = std::mem::take(&mut child_of[id]).into();
            data.individuals.push(individual);
        }
        data.families.reserve(self.families.len() + implied.len());
        for (id, family) in self.families.iter().enumerate() {
            data.families.push(self.family(&ids, id, family));
        }
        for (i, family) in implied.iter().enumerate() {
            let xref = ids.family(self.families.len() + i);
            if family.pedigree == Pedigree::Adopted {
                data.individuals[family.child]
                    .events
                    .push(adoption_event(xref, family));
            }
            let partner = |who: Option<PersonId>| who.map(|p| IndividualRef::new(ids.person(p)));
            data.families.push(Family {
                xref: Some(xref),
                husband: partner(family.father),
                wife: partner(family.mother),
                children: vec![IndividualRef::new(ids.person(family.child))],
                citations: citations(family.sources),
                ..Family::default()
            });
        }

        for page in &self.pages {
            data.extra.push(page_tag(&ids, ids.page, page));
        }
        for wizard in &self.wizard_notes {
            data.extra.push(page_tag(&ids, ids.wizard, wizard));
        }

        data
    }

    fn individual(&self, ids: &Ids, id: PersonId, person: &Person) -> Individual {
        let mut individual = Individual {
            xref: Some(ids.person(id)),
            names: names(person),
            sex: gender(person.sex),
            notes: notes(&person.notes),
            citations: citations(&person.sources),
            ..Individual::default()
        };
        individual.events = self.life_events(ids, id, person);
        individual.events.extend(attributes(person));
        // Only the `rel` relations go on the individual. Witnesses of `pevt` events stay
        // nested in the event they attended (see `event_detail`), as GeneWeb's own
        // `gwb2ged` writes them: a second, individual-level copy said nothing about which
        // event it was, so a reader attaching it to some event of its own choosing turned
        // a witness of a death into a witness of a baptism, next to the correct one.
        let associations = self.associations(ids, person);
        if !associations.is_empty() {
            individual.detail_mut().associations = associations;
        }

        // A portrait is a standard GEDCOM multimedia object, not just a custom tag.
        if !person.image.is_empty() {
            individual
                .detail_mut()
                .multimedia
                .push(multimedia(&person.image));
        }

        if person.occ != 0 {
            individual
                .extra
                .push(custom(ids.occurrence, &person.occ.to_string()));
        }
        if let Some(label) = access_label(person.access) {
            individual.extra.push(custom(ids.access, label));
        }
        // What GEDCOM itself can say about it: a person hidden from the public is
        // `RESN confidential`. `_GWACCESS` keeps the exact GeneWeb setting.
        if matches!(person.access, Access::Private | Access::SemiPublic) {
            individual.detail_mut().restriction = Some(EnumList(vec![Restriction::Confidential]));
        }
        if !person.image.is_empty() {
            individual.extra.push(custom(ids.image, &person.image));
        }

        individual
    }

    /// Birth, baptism, death and burial, followed by any `pevt` events.
    ///
    /// A `pevt` block may restate an event the person's own line already carries. The
    /// `gwplus` rule is that the structured event wins, so a life event that also appears
    /// in the block is emitted once, from the block.
    fn life_events(&self, ids: &Ids, id: PersonId, person: &Person) -> Vec<Event> {
        use crate::model::event::PersonEventName as P;
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
            let mut d = Event::new(EventKind::Birth);
            d.date = person.birth.as_ref().map(date::to_gedcom);
            d.place = place(&person.birth_place);
            set_notes(&mut d, &person.birth_note);
            d.citations = citations(&person.birth_src);
            events.push(d);
        }

        if !baptism_superseded
            && (person.baptism.is_some()
                || !person.baptism_place.is_empty()
                || !person.baptism_src.is_empty()
                || !person.baptism_note.is_empty())
        {
            let mut d = Event::new(EventKind::Baptism);
            d.date = person.baptism.as_ref().map(date::to_gedcom);
            d.place = place(&person.baptism_place);
            set_notes(&mut d, &person.baptism_note);
            d.citations = citations(&person.baptism_src);
            events.push(d);
        }

        if let Some(mut d) = death_event(ids, &person.death).filter(|_| !death_superseded) {
            d.place = place(&person.death_place);
            set_notes(&mut d, &person.death_note);
            d.citations = citations(&person.death_src);
            events.push(d);
        }

        let burial = match &person.burial {
            _ if burial_superseded => None,
            Burial::Unknown => None,
            Burial::Buried(date) => Some((EventKind::Burial, date)),
            Burial::Cremated(date) => Some((EventKind::Cremation, date)),
        };
        if let Some((kind, when)) = burial {
            let mut d = Event::new(kind);
            d.date = when.as_ref().map(date::to_gedcom);
            d.place = place(&person.burial_place);
            set_notes(&mut d, &person.burial_note);
            d.citations = citations(&person.burial_src);
            events.push(d);
        }

        for (i, gw_event) in person.events.iter().enumerate() {
            let mapping = event::person_event(&gw_event.name);
            let witnesses = self.person_event_witnesses(id, i);
            events.push(Self::event_detail(
                ids,
                gw_event,
                mapping,
                &gw_event.name.tag(),
                witnesses,
            ));
        }

        // A `pevt` death supersedes the death on the person's line, but `#deat` has no
        // way to say how the person died: the line's reason goes on the `pevt` death.
        if death_superseded {
            if let Some(reason) = death_reason(&person.death) {
                if let Some(d) = events.iter_mut().find(|d| d.kind == EventKind::Death) {
                    d.extra.push(custom(ids.death_reason, reason));
                }
            }
        }

        events
    }

    /// Shapes a `gwplus` event, personal or familial, into a GEDCOM event.
    fn event_detail<N>(
        ids: &Ids,
        gw_event: &GwEvent<N>,
        mapping: event::EventMapping,
        original_tag: &str,
        witnesses: &[ResolvedWitness],
    ) -> Event {
        let mut d = Event::new(mapping.event);
        d.date = gw_event.date.as_ref().map(date::to_gedcom);
        d.place = place(&gw_event.place);
        d.citations = citations(&gw_event.source);
        set_notes(&mut d, &gw_event.note);
        let associations = witness_associations(ids, witnesses);
        if !associations.is_empty() {
            d.detail_mut().associations = associations;
        }
        if !gw_event.cause.is_empty() {
            d.detail_mut().cause = Some(Text::from(gw_event.cause.as_str()));
        }
        // An event that became a generic `EVEN` has lost which `.gw` tag it came from.
        // Recording the tag keeps the mapping reversible.
        if let Some(label) = mapping.event_type {
            d.detail_mut().classification = Some(Text::from(label));
            d.extra.push(custom(ids.event, original_tag));
        }
        d
    }

    /// The relations of a `rel` block, as GEDCOM associations.
    fn associations(&self, ids: &Ids, person: &Person) -> Vec<Association> {
        // Only the relations GEDCOM has no structure for: adoptive and foster parents
        // become a family of their own (see `implied_families`). Godparents are labelled
        // as `gwb2ged` labels them, `GODF` and `GODM`.
        let mut out = Vec::new();
        for relation in &person.relations {
            if matches!(
                relation.relation_type,
                RelationType::Adoption | RelationType::FosterParent
            ) {
                continue;
            }
            for (who, slot) in [(relation.father.as_ref(), 0), (relation.mother.as_ref(), 1)] {
                let Some(who) = who else { continue };
                let label = match (relation.relation_type, slot) {
                    (RelationType::GodParent, 0) => "GODF",
                    (RelationType::GodParent, _) => "GODM",
                    (other, _) => relation_type_label(other),
                };
                if let Some(id) = self.lookup(&who.key()) {
                    out.push(association(ids.person(id), label));
                }
            }
        }
        out
    }

    /// The adoptive and foster families the `rel` blocks imply: one per relation that
    /// names at least one parent the file knows.
    fn implied_families(&self) -> Vec<ImpliedFamily<'_>> {
        let mut out = Vec::new();
        for (child, person) in self.persons.iter().enumerate() {
            for relation in &person.relations {
                let pedigree = match relation.relation_type {
                    RelationType::Adoption => Pedigree::Adopted,
                    RelationType::FosterParent => Pedigree::Foster,
                    _ => continue,
                };
                let resolve = |who: &Option<crate::model::key::Somebody>| {
                    who.as_ref().and_then(|w| self.lookup(&w.key()))
                };
                let (father, mother) = (resolve(&relation.father), resolve(&relation.mother));
                if father.is_some() || mother.is_some() {
                    out.push(ImpliedFamily {
                        child,
                        father,
                        mother,
                        pedigree,
                        sources: &relation.sources,
                    });
                }
            }
        }
        out
    }

    fn family(&self, ids: &Ids, id: FamilyId, family: &FamilyRecord) -> Family {
        let mut out = Family {
            xref: Some(ids.family(id)),
            husband: Some(IndividualRef::new(ids.person(family.father))),
            wife: Some(IndividualRef::new(ids.person(family.mother))),
            children: family
                .children
                .iter()
                .map(|&child| IndividualRef::new(ids.person(child)))
                .collect(),
            citations: citations(&family.sources),
            notes: notes(&family.comment),
            ..Family::default()
        };

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
                || family.relation != RelationKind::Married
                // Witnesses of the union are worth an event on their own: without it
                // there is nothing to hang their `ASSO` on.
                || !family.witnesses.is_empty())
        {
            let mut d = Event::new(union.event);
            if let Some(label) = union.event_type {
                d.detail_mut().classification = Some(Text::from(label));
            }
            d.date = family.marriage.as_ref().map(date::to_gedcom);
            d.place = place(&family.marriage_place);
            set_notes(&mut d, &family.marriage_note);
            d.citations = citations(&family.marriage_src);
            let witnesses = witness_associations(ids, &family.witnesses);
            if !witnesses.is_empty() {
                d.detail_mut().associations = witnesses;
            }
            out.events.push(d);
        }

        let ending = match &family.divorce {
            Divorce::Divorced(when) if !divorce_superseded => {
                Some((event::family_event(&F::Divorce), when))
            }
            Divorce::Separated(when) if !separation_superseded => {
                Some((event::family_event(&F::Separated), when))
            }
            // Either the union never ended, or a `fevt` entry already says how it did.
            _ => None,
        };
        if let Some((mapping, when)) = ending {
            let mut d = Event::new(mapping.event);
            if let Some(label) = mapping.event_type {
                d.detail_mut().classification = Some(Text::from(label));
            }
            d.date = when.as_ref().map(date::to_gedcom);
            out.events.push(d);
        }

        for (i, gw_event) in family.events.iter().enumerate() {
            let mapping = event::family_event(&gw_event.name);
            let witnesses = self.family_event_witnesses(id, i);
            out.events.push(Self::event_detail(
                ids,
                gw_event,
                mapping,
                &gw_event.name.tag(),
                witnesses,
            ));
        }

        // A `fevt` marriage supersedes the `fam` line's union, but the witnesses
        // written on that line still attended it: they join the event's own.
        if marriage_superseded {
            if let Some(marriage) = out
                .events
                .iter_mut()
                .find(|d| d.kind == EventKind::Marriage)
            {
                for witness in witness_associations(ids, &family.witnesses) {
                    let known = marriage
                        .detail()
                        .associations
                        .iter()
                        .any(|a| a.individual == witness.individual);
                    if !known {
                        marriage.detail_mut().associations.push(witness);
                    }
                }
            }
        }

        if let Some(label) = relation_kind_label(family.relation) {
            out.extra.push(custom(ids.relation_kind, label));
        }

        out
    }
}

fn header() -> Header {
    Header {
        source: Some(HeaderSource {
            product: Text::from("GENEWEB"),
            name: Some(Text::from(concat!("geneweb ", env!("CARGO_PKG_VERSION")))),
            ..HeaderSource::default()
        }),
        ..Header::default()
    }
}

/// Builds the `NAME` records: the primary one, then every alias GeneWeb records.
fn names(person: &Person) -> Vec<Name> {
    let mut names = vec![name_record(
        gedcom_name(&person.first_name, &person.surname),
        &person.first_name,
        &person.surname,
        None,
    )];

    // A nickname belongs on the primary name. Each further nickname rides on a name of
    // its own, as a GEDCOM reader expects one `NICK` a name.
    let mut nicknames = person.qualifiers.iter();
    if let Some(nickname) = nicknames.next() {
        add_nickname(&mut names[0], nickname);
    }
    for nickname in nicknames {
        let mut name = name_record(
            gedcom_name(&person.first_name, &person.surname),
            &person.first_name,
            &person.surname,
            Some(NameType::Aka),
        );
        add_nickname(&mut name, nickname);
        names.push(name);
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

/// An association with an individual, its relation in words (`RELA`).
fn association(individual: XrefId, relation: &str) -> Association {
    Association {
        individual: Some(individual),
        relation: Some(Text::from(relation)),
        ..Association::default()
    }
}

/// Turns resolved witnesses into GEDCOM associations pointing at each witness.
fn witness_associations(ids: &Ids, witnesses: &[ResolvedWitness]) -> Vec<Association> {
    witnesses
        .iter()
        .map(|w| association(ids.person(w.person), event::witness_relationship(w.kind)))
        .collect()
}

fn gender(sex: Sex) -> Option<GedSex> {
    match sex {
        Sex::Male => Some(GedSex::Male),
        Sex::Female => Some(GedSex::Female),
        // GeneWeb's "neuter" means unrecorded, not non-binary.
        Sex::Neuter => None,
    }
}

/// The death event, if the person is recorded as dead.
fn death_event(ids: &Ids, death: &Death) -> Option<Event> {
    let mut d = Event::new(EventKind::Death);
    match death {
        // Nothing to record: alive, or simply unknown.
        Death::NotDead | Death::DontKnowIfDead => return None,
        Death::Dead { date, .. } => d.date = Some(date::to_gedcom(date)),
        // `1 DEAT Y` is the GEDCOM idiom for a death with no details. The nuance
        // GeneWeb draws between these three goes in `_GWDEATH`, since GEDCOM has no
        // vocabulary for it.
        Death::DeadDontKnowWhen | Death::DeadYoung | Death::OfCourseDead => {
            d.value = Text::from("Y");
        }
    }
    if let Some(reason) = death_reason(death) {
        d.extra.push(custom(ids.death_reason, reason));
    }
    Some(d)
}

/// How a person died, or what GeneWeb knows of it, when GEDCOM has no word for it.
fn death_reason(death: &Death) -> Option<&'static str> {
    match death {
        Death::Dead { reason, .. } => death_reason_label(*reason),
        Death::DeadYoung => Some("died young"),
        Death::OfCourseDead => Some("presumed dead"),
        Death::NotDead | Death::DontKnowIfDead | Death::DeadDontKnowWhen => None,
    }
}

/// A multimedia link holding the path of a portrait file.
fn multimedia(path: &str) -> MultimediaLink {
    MultimediaLink {
        files: std::iter::once(File {
            path: Text::from(path),
            ..File::default()
        })
        .collect(),
        ..MultimediaLink::default()
    }
}

/// The occupation and the titles, as GEDCOM attributes.
fn attributes(person: &Person) -> Vec<Event> {
    let mut out = Vec::new();
    if !person.occupation.is_empty() {
        out.push(attribute(EventKind::Occupation, &person.occupation));
    }
    for title in &person.titles {
        let mut a = attribute(EventKind::Title, &title_value(title));
        a.date = title_period(title);
        set_notes(&mut a, title_holder(title, person));
        out.push(a);
    }
    out
}

fn attribute(kind: EventKind, value: &str) -> Event {
    Event {
        value: Text::from(value),
        ..Event::new(kind)
    }
}

/// An extended page or a wizard note: its name, and its text as a note, whose line
/// breaks the writer turns into `CONT` lines.
fn page_tag(ids: &Ids, tag: TagId, page: &crate::database::Page) -> Node {
    let mut node = Node::new(tag);
    if !page.name.is_empty() {
        node.payload = Value::Text(Text::from(page.name.as_str()));
    }
    node.children.push(Node {
        payload: if page.text.is_empty() {
            Value::None
        } else {
            Value::Text(Text::from(page.text.as_str()))
        },
        ..Node::new(ids.note)
    });
    node
}

/// Silences the unused-import warning for types referenced only in documentation.
const _: Option<(&PersonEvent, &FamilyEvent, WitnessKind)> = None;

#[cfg(test)]
mod tests {
    use super::*;
    use ged_io::model::NoteContent;

    fn convert(input: &str) -> Dataset {
        GwDatabase::read(input.as_bytes(), "t.gw")
            .expect("parses")
            .to_gedcom()
    }

    /// The characters of a text of `data`.
    fn text(data: &Dataset, text: &Text) -> String {
        text.to_str(data).into_owned()
    }

    /// The identifier of a record or the target of a pointer.
    fn xref(data: &Dataset, id: Option<XrefId>) -> Option<&str> {
        id.map(|id| data.store().xref(id))
    }

    /// The tag and text of each extension structure.
    fn extensions<'d>(data: &'d Dataset, nodes: &'d [Node]) -> Vec<(&'d str, Option<String>)> {
        nodes
            .iter()
            .map(|n| {
                let value = match &n.payload {
                    Value::Text(t) => Some(text(data, t)),
                    _ => None,
                };
                (data.store().tag(n.tag), value)
            })
            .collect()
    }

    fn note_texts(data: &Dataset, notes: &[Note]) -> Vec<String> {
        notes
            .iter()
            .filter_map(|n| match &n.content {
                NoteContent::Text(t) => Some(text(data, t)),
                NoteContent::Shared(_) => None,
            })
            .collect()
    }

    fn event_of<'i>(individual: &'i Individual, kind: &EventKind) -> &'i Event {
        individual
            .events_of(kind.clone())
            .next()
            .unwrap_or_else(|| panic!("a {kind:?}"))
    }

    fn associated<'d>(data: &'d Dataset, event: &Event) -> Vec<&'d str> {
        event
            .detail()
            .associations
            .iter()
            .filter_map(|a| xref(data, a.individual))
            .collect()
    }

    #[test]
    fn event_witnesses_are_not_repeated_on_the_individual() {
        let data = convert(concat!(
            "fam Doe John + Roe Jane\n",
            "pevt Doe John\n",
            "#birt 1900\n",
            "#deat 1970\n",
            "wit m: Poe Paul\n",
            "end pevt\n",
            "fam Poe Paul + Moe Mary\n",
        ));
        let john = &data.individuals[0];
        let death = event_of(john, &EventKind::Death);
        assert_eq!(associated(&data, death), ["@I3@"]);
        // The witness is stated once, on the event, and nowhere else.
        assert_eq!(john.detail().associations.len(), 0);
        assert!(john
            .events
            .iter()
            .all(|e| e.kind == EventKind::Death || e.detail().associations.is_empty()));
    }

    #[test]
    fn a_family_becomes_two_individuals_and_a_family_record() {
        let data = convert("fam Dupont Jean + Martin Marie\nbeg\n- m Paul 1930\nend\n");
        assert_eq!(data.individuals.len(), 3);
        assert_eq!(data.families.len(), 1);

        let family = &data.families[0];
        assert_eq!(xref(&data, family.xref), Some("@F1@"));
        assert_eq!(xref(&data, family.husband_id()), Some("@I1@"));
        assert_eq!(xref(&data, family.wife_id()), Some("@I2@"));
        let children: Vec<_> = family
            .children
            .iter()
            .filter_map(|c| xref(&data, c.individual))
            .collect();
        assert_eq!(children, ["@I3@"]);
    }

    #[test]
    fn names_use_gedcom_slash_syntax() {
        let data = convert("fam Dupont Jean + Martin Marie\n");
        let name = &data.individuals[0].names[0];
        assert_eq!(text(&data, &name.value), "Jean /Dupont/");
        assert_eq!(
            name.surname().map(|s| text(&data, s)).as_deref(),
            Some("Dupont")
        );
        assert_eq!(
            name.given().map(|g| text(&data, g)).as_deref(),
            Some("Jean")
        );
    }

    #[test]
    fn aliases_become_additional_name_records() {
        let data = convert("fam Dupont Jean {Jeannot} #salias Dupond (Le_Grand) 1900 + A B\n");
        let values: Vec<_> = data.individuals[0]
            .names
            .iter()
            .map(|n| text(&data, &n.value))
            .collect();
        assert!(values.iter().any(|v| v == "Jean /Dupont/"));
        assert!(values.iter().any(|v| v == "Le Grand /Dupont/"));
        assert!(values.iter().any(|v| v == "Jeannot /Dupont/"));
        assert!(values.iter().any(|v| v == "Jean /Dupond/"));
    }

    fn nicknames(data: &Dataset, name: &Name) -> Vec<String> {
        name.pieces_of(NamePieceKind::Nickname)
            .map(|n| text(data, n))
            .collect()
    }

    #[test]
    fn every_nickname_is_kept() {
        let data = convert("fam Doe John #nick Johnny #nick Jacko + Roe Jane\n");
        let names = &data.individuals[0].names;
        assert_eq!(nicknames(&data, &names[0]), ["Johnny"]);
        let second = names
            .iter()
            .find(|n| nicknames(&data, n) == ["Jacko"])
            .expect("the second nickname");
        assert_eq!(text(&data, &second.value), "John /Doe/");
        assert_eq!(
            second.detail().kind.as_ref().map(|k| &k.value),
            Some(&NameType::Aka)
        );
    }

    #[test]
    fn a_nickname_is_a_piece_between_the_given_name_and_the_surname() {
        let data = convert("fam Doe John #nick Johnny + Roe Jane\n");
        let kinds: Vec<_> = data.individuals[0].names[0]
            .pieces
            .iter()
            .map(|p| p.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                NamePieceKind::Given,
                NamePieceKind::Nickname,
                NamePieceKind::Surname
            ]
        );
    }

    #[test]
    fn parents_get_a_spouse_link_and_children_a_child_link() {
        let data = convert("fam Dupont Jean + Martin Marie\nbeg\n- m Paul\nend\n");
        let father = &data.individuals[0];
        assert_eq!(xref(&data, father.spouse_of[0].family), Some("@F1@"));
        assert!(father.child_of.is_empty());
        let child = &data.individuals[2];
        assert_eq!(xref(&data, child.child_of[0].family), Some("@F1@"));
        assert!(child.spouse_of.is_empty());
    }

    #[test]
    fn access_restrictions_are_also_a_resn() {
        let data = convert(concat!(
            "fam Doe John #apriv + Roe Jane #semipub\n",
            "fam Poe Paul #apubl + Moe Mary\n",
        ));
        let restriction = |i: usize| data.individuals[i].detail().restriction.clone();
        let confidential = Some(EnumList(vec![Restriction::Confidential]));
        assert_eq!(restriction(0), confidential);
        assert_eq!(restriction(1), confidential);
        assert_eq!(restriction(2), None);
        assert_eq!(restriction(3), None);
    }

    #[test]
    fn sex_is_written_only_when_recorded() {
        let data = convert("fam Dupont Jean + Martin Marie\nbeg\n- Paul\nend\n");
        assert_eq!(data.individuals[0].sex, Some(GedSex::Male));
        assert_eq!(data.individuals[1].sex, Some(GedSex::Female));
        assert_eq!(data.individuals[2].sex, None);
    }

    #[test]
    fn life_events_carry_dates_places_and_sources() {
        let data =
            convert("fam Dupont Jean 7/9/1830 #bp Reims #bs acte 12/5/1900 #dp Lyon + A B\n");
        let individual = &data.individuals[0];
        let birth = event_of(individual, &EventKind::Birth);
        assert_eq!(
            text(&data, &birth.date.as_ref().unwrap().value),
            "07 SEP 1830"
        );
        assert_eq!(text(&data, &birth.place.as_ref().unwrap().name), "Reims");
        assert_eq!(birth.citations.len(), 1);
        let death = event_of(individual, &EventKind::Death);
        assert_eq!(text(&data, &death.place.as_ref().unwrap().name), "Lyon");
    }

    #[test]
    fn a_living_person_gets_no_death_event() {
        let data = convert("fam Dupont Jean 1990 + A B\n");
        assert!(data.individuals[0].death().is_none());
    }

    #[test]
    fn occupations_and_titles_become_attributes() {
        let data = convert("fam Dupont Jean [*:duc:Bretagne:::] #occu Marchand 1900 + A B\n");
        let individual = &data.individuals[0];
        let value = |kind| text(&data, &event_of(individual, &kind).value);
        assert_eq!(value(EventKind::Occupation), "Marchand");
        assert_eq!(value(EventKind::Title), "duc, Bretagne");
    }

    #[test]
    fn titles_are_written_as_gwb2ged_writes_them() {
        let data = convert(concat!(
            "fam Doe John [Samplename:Count:Sampleshire:1800:1810:2] ",
            "[:Baron:Sampleton:::] + A B\n",
        ));
        let titles: Vec<_> = data.individuals[0].events_of(EventKind::Title).collect();
        assert_eq!(titles.len(), 2);

        let count = titles[0];
        assert_eq!(text(&data, &count.value), "Count, Sampleshire, 2");
        // The domain is part of the title, not a place.
        assert!(count.place.is_none());
        // Both ends of the period are kept.
        assert_eq!(
            text(&data, &count.date.as_ref().unwrap().value),
            "FROM 1800 TO 1810"
        );
        assert_eq!(note_texts(&data, &count.detail().notes), ["Samplename"]);

        let baron = titles[1];
        assert_eq!(text(&data, &baron.value), "Baron, Sampleton");
        assert!(baron.date.is_none() && baron.detail.is_none() && baron.place.is_none());
    }

    #[test]
    fn geneweb_only_concepts_ride_on_custom_tags() {
        let data = convert("fam Dupont Jean.2 #image p.jpg #apriv 1900 + A B\n");
        let individual = &data.individuals[0];
        let portrait = individual
            .detail()
            .multimedia
            .first()
            .expect("the portrait becomes GEDCOM multimedia");
        assert_eq!(text(&data, &portrait.files[0].path), "p.jpg");
        let tags = extensions(&data, &individual.extra);
        assert!(tags.contains(&(TAG_OCCURRENCE, Some("2".to_owned()))));
        assert!(tags.contains(&(TAG_ACCESS, Some("private".to_owned()))));
        assert!(tags.contains(&(TAG_IMAGE, Some("p.jpg".to_owned()))));
    }

    /// The death reason and the note of a death event, as converted.
    fn death_reason_and_note(data: &Dataset) -> (Vec<String>, Vec<String>) {
        let death = data.individuals[0].death().expect("a death");
        let reasons = extensions(data, &death.extra)
            .into_iter()
            .filter(|(tag, _)| *tag == TAG_DEATH_REASON)
            .filter_map(|(_, value)| value)
            .collect();
        (reasons, note_texts(data, &death.detail().notes))
    }

    #[test]
    fn the_death_reason_is_a_tag_of_the_death_beside_its_note() {
        let data = convert("fam Doe John k1900 + Roe Jane\n");
        assert_eq!(
            death_reason_and_note(&data),
            (vec!["killed".to_owned()], Vec::new())
        );

        // A death with its own note keeps it whole, apart from the reason.
        let mut db = GwDatabase::read(b"fam Doe John k1900 + Roe Jane\n", "t.gw").expect("parses");
        db.persons[0].death_note = "Fell at the front".to_owned();
        assert_eq!(
            death_reason_and_note(&db.to_gedcom()),
            (
                vec!["killed".to_owned()],
                vec!["Fell at the front".to_owned()]
            )
        );
    }

    #[test]
    fn the_death_reason_survives_a_pevt_death() {
        let data = convert("fam Doe John k1900 + Roe Jane\npevt Doe John\n#deat 1900\nend pevt\n");
        assert_eq!(data.individuals[0].events_of(EventKind::Death).count(), 1);
        assert_eq!(
            death_reason_and_note(&data),
            (vec!["killed".to_owned()], Vec::new())
        );
    }

    #[test]
    fn a_pacs_keeps_its_kind_because_gedcom_has_no_event_for_it() {
        let data = convert("fam A B + #pacs mf C D\n");
        let family = &data.families[0];
        assert!(extensions(&data, &family.extra)
            .contains(&(TAG_RELATION_KIND, Some("pacs".to_owned()))));
    }

    #[test]
    fn family_events_and_divorce() {
        let data = convert("fam A B +1850 -1860 C D\n");
        let events = &data.families[0].events;
        assert!(events.iter().any(|e| e.kind == EventKind::Marriage));
        let divorce = events
            .iter()
            .find(|e| e.kind == EventKind::Divorce)
            .expect("a divorce");
        assert_eq!(text(&data, &divorce.date.as_ref().unwrap().value), "1860");
    }

    #[test]
    fn a_separation_is_a_generic_event() {
        let data = convert("fam A B +1850 #sep C D\n");
        let separation = data.families[0]
            .events
            .iter()
            .find(|e| e.kind == EventKind::Event)
            .expect("a separation");
        let label = separation.detail().classification.as_ref().unwrap();
        assert_eq!(text(&data, label), "Separation");
    }

    #[test]
    fn witnesses_known_only_from_their_event_are_kept() {
        let data = convert(concat!(
            "fam Doe John + Roe Jane\n",
            "fevt\n#marr 1925\nwit m: Poe Paul\nend fevt\n",
            "pevt Doe John\n#deat 1970\nwit m: Moe Mark 1850\nend pevt\n",
        ));
        let marriage = &data.families[0].events[0];
        assert_eq!(associated(&data, marriage), ["@I3@"]);
        let death = event_of(&data.individuals[0], &EventKind::Death);
        assert_eq!(associated(&data, death), ["@I4@"]);
        // The witnesses are individuals of the file.
        assert_eq!(data.individuals.len(), 4);
    }

    #[test]
    fn generic_events_keep_their_label() {
        let data = convert("fam A B + C D\npevt A B\n#hosp 1914\nend pevt\n");
        let event = event_of(&data.individuals[0], &EventKind::Event);
        let label = event.detail().classification.as_ref().unwrap();
        assert_eq!(text(&data, label), "Hospitalization");
        assert!(extensions(&data, &event.extra).contains(&(TAG_EVENT, Some("#hosp".to_owned()))));
    }

    #[test]
    fn family_witnesses_survive_a_union_without_details() {
        // Nothing but witnesses on the `fam` line: no date, place, source or note.
        let data = convert("fam Doe John + Roe Jane\nwit m: Poe Paul\nfam Poe Paul + Moe Mary\n");
        let union = &data.families[0].events;
        assert_eq!(union.len(), 1);
        assert_eq!(union[0].kind, EventKind::Marriage);
        assert_eq!(associated(&data, &union[0]), ["@I3@"]);
    }

    #[test]
    fn family_witnesses_join_a_superseding_fevt_marriage() {
        let data = convert(concat!(
            "fam Doe John + Roe Jane\n",
            "wit m: Poe Paul\n",
            "fevt\n#marr 1925\nwit f: Moe Mary\nend fevt\n",
            "fam Poe Paul + Moe Mary\n",
        ));
        let events = &data.families[0].events;
        assert_eq!(events.len(), 1);
        assert_eq!(associated(&data, &events[0]), ["@I4@", "@I3@"]);
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
        let birth = event_of(&data.individuals[0], &EventKind::Birth);
        assert_eq!(associated(&data, birth), ["@I2@"]);
        let relation = birth.detail().associations[0].relation.as_ref().unwrap();
        assert_eq!(text(&data, relation), "GODP");
    }

    #[test]
    fn relations_without_a_gedcom_structure_become_associations() {
        let data = convert(concat!(
            "fam Dupont Jean + A B\n",
            "rel Dupont Jean\nbeg\n",
            "- reco fath: Martin Paul\n",
            "- godp: Durand Luc + Durand Anne\n",
            "end\n",
        ));
        let labels: Vec<_> = data.individuals[0]
            .detail()
            .associations
            .iter()
            .map(|a| text(&data, a.relation.as_ref().unwrap()))
            .collect();
        // Godparents are labelled as gwb2ged labels them.
        assert_eq!(labels, ["recognising parent", "GODF", "GODM"]);
    }

    #[test]
    fn an_adoption_becomes_an_adoptive_family() {
        let data = convert(concat!(
            "fam Doe John + Roe Jane\nbeg\n- h Paul 1930\nend\n",
            "rel Doe Paul\nbeg\n- adop fath: Poe Peter\nend\n",
        ));
        // Not an association: nothing to mistake for a witness.
        assert!(data
            .individuals
            .iter()
            .all(|i| i.detail().associations.is_empty()));

        let adoptive = &data.families[1];
        assert_eq!(xref(&data, adoptive.xref), Some("@F2@"));
        assert_eq!(xref(&data, adoptive.husband_id()), Some("@I4@"));
        assert_eq!(adoptive.wife, None);
        assert_eq!(xref(&data, adoptive.children[0].individual), Some("@I3@"));

        let paul = &data.individuals[2];
        let link = paul
            .child_of
            .iter()
            .find(|l| xref(&data, l.family) == Some("@F2@"))
            .expect("a link to the adoptive family");
        assert_eq!(
            link.detail().pedigree.as_ref().map(|p| &p.value),
            Some(&Pedigree::Adopted)
        );
        let adop = event_of(paul, &EventKind::Adoption);
        let adop_family = adop.detail().family.as_ref().expect("its family");
        assert_eq!(xref(&data, adop_family.family), Some("@F2@"));
        assert_eq!(
            adop_family.adopted_by.as_ref().map(|a| &a.value),
            Some(&Adoption::Husband)
        );
        // The adoptive father is a spouse of that family.
        assert!(data.individuals[3]
            .spouse_of
            .iter()
            .any(|l| xref(&data, l.family) == Some("@F2@")));
    }

    #[test]
    fn foster_parents_become_a_foster_family() {
        let data = convert(concat!(
            "fam Doe John + Roe Jane\nbeg\n- h Paul 1930\nend\n",
            "rel Doe Paul\nbeg\n- fost: Poe Peter + Poe Mary\nend\n",
        ));
        let foster = &data.families[1];
        assert!(foster.husband.is_some() && foster.wife.is_some());
        let paul = &data.individuals[2];
        assert!(paul.child_of.iter().any(|l| {
            xref(&data, l.family) == Some("@F2@")
                && l.detail().pedigree.as_ref().map(|p| &p.value) == Some(&Pedigree::Foster)
        }));
        // Fostering is no adoption.
        assert!(paul.events_of(EventKind::Adoption).next().is_none());
    }

    #[test]
    fn pages_and_wizard_notes_are_kept_at_record_level() {
        let data = convert(concat!(
            "page-ext Gallery\n  content\nend page-ext\n",
            "wizard-note henri\n  1234\nend wizard-note\n",
        ));
        let tags: Vec<_> = extensions(&data, &data.extra)
            .into_iter()
            .map(|(tag, _)| tag)
            .collect();
        assert!(tags.contains(&TAG_PAGE));
        assert!(tags.contains(&TAG_WIZARD));
    }

    /// A page over several lines: `ged_io` writes the line breaks of an extension's text
    /// as `CONT` lines, and reads them back into the text.
    #[test]
    fn a_page_of_several_lines_reads_back_as_written() {
        use ged_io::GedcomWriter;
        let page = crate::database::Page {
            name: "Sample".to_owned(),
            text: "first\n\nthird".to_owned(),
        };
        let mut data = Dataset::new(GedcomVersion::V5_5_1);
        let ids = Ids::new(data.store_mut(), 0, 0);
        data.extra.push(page_tag(&ids, ids.page, &page));
        let written = GedcomWriter::new().write_to_string(&data).expect("writes");
        assert!(
            written.contains("0 _GWPAGE Sample\n1 NOTE first\n2 CONT\n2 CONT third\n"),
            "{written}"
        );
        let read = Dataset::parse(written);
        assert_eq!(
            read.extra[0].to_structure(&read),
            data.extra[0].to_structure(&data)
        );
    }

    #[test]
    fn the_header_records_the_producer() {
        let data = convert("fam A B + C D\n");
        let source = data.header.as_ref().unwrap().source.as_ref().unwrap();
        assert_eq!(text(&data, &source.product), "GENEWEB");
        assert!(text(&data, source.name.as_ref().unwrap()).starts_with("geneweb "));
    }

    /// The conversion, a GEDCOM 5.5.1 dataset, is written as conformant GEDCOM 5.5.1 as
    /// it is, but for what a `.gw` file cannot say and the writer completes or moves: the
    /// format of a portrait, which the writer reads from the file's extension, and the
    /// event witnesses, which it keeps as `_ASSO` since 5.5.1 has no `ASSO` under an
    /// event.
    #[test]
    fn the_writer_repairs_only_what_a_gw_file_cannot_say() {
        use ged_io::GedcomWriter;
        let data = convert(concat!(
            "fam Doe John #image p.jpg #nick Johnny [:Baron:Sampleton:1800::] #apriv k1900 ",
            "+1850 #sep Roe Jane\n",
            "wit m: Poe Paul\n",
            "beg\n- h Paul 0(around_midsummer)\nend\n",
            "pevt Doe John\n#hosp 1914\nwit m: Poe Paul\nend pevt\n",
            "rel Doe Paul\nbeg\n- adop fath: Poe Peter\n- godp fath: Moe Mark\nend\n",
            "page-ext Gallery\n  first\n  second\nend page-ext\n",
        ));
        let repairs = |version| {
            let (_, report) = GedcomWriter::new()
                .gedcom_version(version)
                .write_to_string_with_report(&data)
                .expect("writes");
            report
                .repairs
                .iter()
                .map(|r| r.detail.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            repairs(GedcomVersion::V5_5_1),
            [
                "INDI.EVEN.ASSO: not permitted here, written as _ASSO",
                "INDI.OBJE.FILE lacks FORM: FORM jpg added",
                "FAM.MARR.ASSO: not permitted here, written as _ASSO",
            ]
        );
        // Written as GEDCOM 7, the 5.5.1 structures 7 does not have are repaired too,
        // into extensions (`_RELA`, `_FILE`): nothing is lost.
        assert_ne!(repairs(GedcomVersion::V7_0).len(), 0);
    }
}
