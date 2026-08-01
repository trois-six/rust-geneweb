//! Turning a flat sequence of blocks into a connected genealogy.
//!
//! A `.gw` file has no identifiers. A person *is* their `(first name, surname,
//! occurrence)` key, they may be defined in one block and referred to from several
//! others, and their notes, events and relations arrive in blocks of their own. This
//! module interns people by key, resolves every reference to an index, and stitches the
//! stray blocks onto the person they name.
//!
//! The one exception is the anonymous person, written `? ?`. Two of those are not the
//! same person, so they are never interned and never merged.

use std::collections::HashMap;

use crate::date::GwDate;
use crate::error::Result;
use crate::model::block::GwBlock;
use crate::model::event::Witness;
use crate::model::family::{Divorce, RelationKind};
use crate::model::key::{Key, Somebody};
use crate::model::person::{Person, Sex};
use crate::model::{FamilyEvent, Title};
use crate::parser::block::BlockReader;

/// An index into [`GwDatabase::persons`].
pub type PersonId = usize;

/// An index into [`GwDatabase::families`].
pub type FamilyId = usize;

/// A witness with their person resolved to an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWitness {
    /// Who witnessed.
    pub person: PersonId,
    /// Their sex, as written on the `wit` line.
    pub sex: Sex,
    /// The capacity in which they attended.
    pub kind: crate::model::event::WitnessKind,
}

/// A family with every person resolved to an index.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyRecord {
    /// The first parent.
    pub father: PersonId,
    /// The second parent.
    pub mother: PersonId,
    /// The nature of the union.
    pub relation: RelationKind,
    /// Date of the union.
    pub marriage: Option<GwDate>,
    /// Place of the union.
    pub marriage_place: String,
    /// Note on the union.
    pub marriage_note: String,
    /// Source for the union.
    pub marriage_src: String,
    /// How the union ended.
    pub divorce: Divorce,
    /// Witnesses to the union.
    pub witnesses: Vec<ResolvedWitness>,
    /// Source for the family.
    pub sources: String,
    /// Free-text comment.
    pub comment: String,
    /// Structured family events.
    pub events: Vec<FamilyEvent>,
    /// The children.
    pub children: Vec<PersonId>,
    /// Basename of the file this family came from.
    pub origin_file: String,
}

/// A named free-text page: the base's presentation notes, an extended page, or a wizard's
/// note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// The page name, or the wizard's identifier. Empty for the base's own notes.
    pub name: String,
    /// The page text, in GeneWeb's wiki syntax.
    pub text: String,
}

/// A whole `.gw` file, with references resolved.
#[derive(Debug, Clone, Default)]
pub struct GwDatabase {
    /// Every person, in the order they were first seen.
    pub persons: Vec<Person>,
    /// Every family, in file order.
    pub families: Vec<FamilyRecord>,
    /// The base's presentation notes and extended pages.
    pub pages: Vec<Page>,
    /// Notes attached to contributors rather than to people.
    pub wizard_notes: Vec<Page>,
    /// Whether the file declared `gwplus`.
    pub gwplus: bool,
    index: HashMap<Key, PersonId>,
}

/// Copies `new` over `existing` wherever `existing` has nothing.
///
/// A person can be written more than once — defined as a child in one family and again
/// as a spouse in another — and the two spellings need not carry the same detail. Filling
/// only the gaps keeps whichever block was more complete without letting a later, sparser
/// mention erase it.
fn merge_person(existing: &mut Person, new: Person) {
    fn fill(target: &mut String, value: String) {
        if target.is_empty() {
            *target = value;
        }
    }
    fn fill_vec<T>(target: &mut Vec<T>, value: Vec<T>) {
        if target.is_empty() {
            *target = value;
        }
    }
    fn fill_date(target: &mut Option<GwDate>, value: Option<GwDate>) {
        if target.is_none() {
            *target = value;
        }
    }
    fn fill_titles(target: &mut Vec<Title>, value: Vec<Title>) {
        if target.is_empty() {
            *target = value;
        }
    }

    if existing.sex == Sex::Neuter {
        existing.sex = new.sex;
    }
    fill_vec(&mut existing.first_names_aliases, new.first_names_aliases);
    fill_vec(&mut existing.surnames_aliases, new.surnames_aliases);
    fill(&mut existing.public_name, new.public_name);
    fill(&mut existing.image, new.image);
    fill_vec(&mut existing.qualifiers, new.qualifiers);
    fill_vec(&mut existing.aliases, new.aliases);
    fill_titles(&mut existing.titles, new.titles);
    if existing.access == crate::model::person::Access::IfTitles {
        existing.access = new.access;
    }
    fill(&mut existing.occupation, new.occupation);
    fill(&mut existing.sources, new.sources);
    fill_date(&mut existing.birth, new.birth);
    fill(&mut existing.birth_place, new.birth_place);
    fill(&mut existing.birth_note, new.birth_note);
    fill(&mut existing.birth_src, new.birth_src);
    fill_date(&mut existing.baptism, new.baptism);
    fill(&mut existing.baptism_place, new.baptism_place);
    fill(&mut existing.baptism_note, new.baptism_note);
    fill(&mut existing.baptism_src, new.baptism_src);
    if existing.death == crate::model::person::Death::DontKnowIfDead {
        existing.death = new.death;
    }
    fill(&mut existing.death_place, new.death_place);
    fill(&mut existing.death_note, new.death_note);
    fill(&mut existing.death_src, new.death_src);
    if existing.burial == crate::model::person::Burial::Unknown {
        existing.burial = new.burial;
    }
    fill(&mut existing.burial_place, new.burial_place);
    fill(&mut existing.burial_note, new.burial_note);
    fill(&mut existing.burial_src, new.burial_src);
    fill(&mut existing.notes, new.notes);
    fill_vec(&mut existing.events, new.events);
    fill_vec(&mut existing.relations, new.relations);
}

impl GwDatabase {
    /// Reads a whole `.gw` file.
    ///
    /// `origin_file` is recorded on every family, as GeneWeb records it.
    ///
    /// # Errors
    ///
    /// Returns the first parse error. Use [`GwDatabase::read_lenient`] to skip malformed
    /// blocks instead.
    pub fn read(input: &[u8], origin_file: &str) -> Result<Self> {
        Self::build(BlockReader::new(input, origin_file))
    }

    /// Reads a whole `.gw` file, skipping blocks that fail to parse.
    ///
    /// Returns the database and the errors that were skipped.
    ///
    /// # Errors
    ///
    /// Never returns `Err`; the signature matches [`GwDatabase::read`] for symmetry.
    #[must_use]
    pub fn read_lenient(input: &[u8], origin_file: &str) -> (Self, Vec<crate::GwError>) {
        let mut errors = Vec::new();
        let mut reader = BlockReader::new(input, origin_file).lenient(true);
        let mut db = Self::default();
        loop {
            match reader.next() {
                None => break,
                Some(Ok(block)) => db.add_block(block),
                Some(Err(e)) => errors.push(e),
            }
        }
        db.gwplus = reader.is_gwplus();
        (db, errors)
    }

    fn build(mut reader: BlockReader<'_>) -> Result<Self> {
        let mut db = Self::default();
        // `by_ref` so the reader survives the loop and can report its directives.
        for block in reader.by_ref() {
            db.add_block(block?);
        }
        db.gwplus = reader.is_gwplus();
        Ok(db)
    }

    /// The person a key names, if the file defines or mentions one.
    #[must_use]
    pub fn lookup(&self, key: &Key) -> Option<PersonId> {
        self.index.get(key).copied()
    }

    /// Interns a person by key, creating a stub if only referenced so far.
    fn intern_key(&mut self, key: Key) -> PersonId {
        if let Some(&id) = self.index.get(&key) {
            return id;
        }
        let id = self.persons.len();
        self.persons
            .push(Person::new(&key.first_name, &key.surname, key.occ));
        self.index.insert(key, id);
        id
    }

    /// Adds a person defined inline, merging into an existing entry for the same key.
    fn intern_definition(&mut self, person: Person) -> PersonId {
        let key = Key::new(&person.first_name, &person.surname, person.occ);
        // Two `? ?` are two people. They get no index entry, so nothing can ever be
        // merged into them or resolved to them.
        if key.is_anonymous() {
            let id = self.persons.len();
            self.persons.push(person);
            return id;
        }
        if let Some(id) = self.index.get(&key).copied() {
            merge_person(&mut self.persons[id], person);
            return id;
        }
        let id = self.persons.len();
        self.persons.push(person);
        self.index.insert(key, id);
        id
    }

    fn intern(&mut self, who: Somebody) -> PersonId {
        match who {
            Somebody::Reference(key) => self.intern_key(key),
            Somebody::Definition(person) => self.intern_definition(*person),
        }
    }

    fn intern_witnesses(&mut self, witnesses: Vec<Witness>) -> Vec<ResolvedWitness> {
        witnesses
            .into_iter()
            .map(|w| ResolvedWitness {
                person: self.intern(w.person),
                sex: w.sex,
                kind: w.kind,
            })
            .collect()
    }

    fn set_sex(&mut self, id: PersonId, sex: Sex) {
        if sex != Sex::Neuter && self.persons[id].sex == Sex::Neuter {
            self.persons[id].sex = sex;
        }
    }

    fn add_block(&mut self, block: GwBlock) {
        match block {
            GwBlock::Family(family) => {
                let family = *family;
                let father = self.intern(family.father);
                let mother = self.intern(family.mother);
                self.set_sex(father, family.father_sex);
                self.set_sex(mother, family.mother_sex);
                let witnesses = self.intern_witnesses(family.witnesses);
                let children = family
                    .children
                    .into_iter()
                    .map(|c| self.intern_definition(c))
                    .collect();
                self.families.push(FamilyRecord {
                    father,
                    mother,
                    relation: family.relation,
                    marriage: family.marriage,
                    marriage_place: family.marriage_place,
                    marriage_note: family.marriage_note,
                    marriage_src: family.marriage_src,
                    divorce: family.divorce,
                    witnesses,
                    sources: family.sources,
                    comment: family.comment,
                    events: family.events,
                    children,
                    origin_file: family.origin_file,
                });
            }
            GwBlock::PersonNotes { key, text } => {
                let id = self.intern_key(key);
                self.persons[id].notes = text;
            }
            GwBlock::Relations {
                person,
                sex,
                relations,
            } => {
                // Resolve the relations' own people before borrowing the subject.
                let resolved = relations
                    .into_iter()
                    .map(|mut r| {
                        r.father = r.father.map(|f| Somebody::Reference(self.resolve_key(f)));
                        r.mother = r.mother.map(|m| Somebody::Reference(self.resolve_key(m)));
                        r
                    })
                    .collect();
                let id = self.intern(person);
                self.set_sex(id, sex);
                self.persons[id].relations = resolved;
            }
            GwBlock::PersonEvents { person, events, .. } => {
                let id = self.intern(person);
                self.persons[id].events = events;
            }
            GwBlock::DatabaseNotes { page, text } => {
                self.pages.push(Page { name: page, text });
            }
            GwBlock::WizardNotes { wizard, text } => {
                self.wizard_notes.push(Page { name: wizard, text });
            }
        }
    }

    /// Interns a person mentioned inside a relation and returns their key.
    ///
    /// Relations keep keys rather than indices so that [`Person`] stays free of database
    /// concerns; interning here is what guarantees the key resolves later.
    fn resolve_key(&mut self, who: Somebody) -> Key {
        let key = who.key();
        self.intern(who);
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(input: &str) -> GwDatabase {
        GwDatabase::read(input.as_bytes(), "t.gw").expect("should parse")
    }

    #[test]
    fn a_person_defined_once_and_referenced_twice_is_one_person() {
        let db = db(concat!(
            "fam Dupont Jean 1900 + Martin Marie\n",
            "beg\n- m Paul 1930\nend\n",
            "fam Dupont Paul + Durand Anne\n",
        ));
        // Jean, Marie, Paul, Anne.
        assert_eq!(db.persons.len(), 4);
        let paul = db.lookup(&Key::new("Paul", "Dupont", 0)).expect("Paul");
        assert_eq!(db.families[0].children, vec![paul]);
        assert_eq!(db.families[1].father, paul);
        // The child definition carried the birth date; the later reference did not
        // erase it.
        assert!(db.persons[paul].birth.is_some());
    }

    #[test]
    fn two_anonymous_people_are_two_people() {
        let db = db("fam Doe John 0 + ? ?\nfam Roe Jane 0 + ? ?\n");
        assert_eq!(db.persons.len(), 4);
        assert!(db.lookup(&Key::new("?", "?", 0)).is_none());
    }

    #[test]
    fn occurrence_numbers_separate_homonyms() {
        let db = db("fam Dupont Jean + A B\nfam Dupont Jean.1 + C D\n");
        assert_eq!(db.persons.len(), 4);
        assert!(db.lookup(&Key::new("Jean", "Dupont", 0)).is_some());
        assert!(db.lookup(&Key::new("Jean", "Dupont", 1)).is_some());
    }

    #[test]
    fn notes_and_events_attach_to_the_person_they_name() {
        let db = db(concat!(
            "fam Dupont Jean + Martin Marie\n",
            "notes Dupont Jean\nbeg\nsome notes\nend notes\n",
            "pevt Dupont Jean\n#birt 1850\nend pevt\n",
        ));
        let jean = db.lookup(&Key::new("Jean", "Dupont", 0)).expect("Jean");
        assert_eq!(db.persons[jean].notes, "some notes");
        assert_eq!(db.persons[jean].events.len(), 1);
    }

    #[test]
    fn a_block_may_name_a_person_before_any_family_does() {
        let db = db("notes Dupont Jean\nbeg\nearly\nend notes\nfam Dupont Jean + A B\n");
        // Jean Dupont, plus the spouse whose surname is `A` and given name `B`.
        assert_eq!(db.persons.len(), 2);
        let jean = db.lookup(&Key::new("Jean", "Dupont", 0)).expect("Jean");
        assert_eq!(db.persons[jean].notes, "early");
        assert_eq!(db.families[0].father, jean);
    }

    #[test]
    fn the_family_line_supplies_parent_sexes() {
        let db = db("fam Dupont Jean + Martin Marie\n");
        assert_eq!(db.persons[db.families[0].father].sex, Sex::Male);
        assert_eq!(db.persons[db.families[0].mother].sex, Sex::Female);
    }

    #[test]
    fn relation_targets_are_interned_too() {
        let db = db("rel Dupont Jean\nbeg\n- adop fath: Martin Paul\nend\n");
        assert!(db.lookup(&Key::new("Paul", "Martin", 0)).is_some());
        let jean = db.lookup(&Key::new("Jean", "Dupont", 0)).expect("Jean");
        assert_eq!(db.persons[jean].relations.len(), 1);
    }

    #[test]
    fn pages_and_wizard_notes_are_collected() {
        let db = db(concat!(
            "notes-db\n  presentation\nend notes-db\n",
            "page-ext Gallery\n  content\nend page-ext\n",
            "wizard-note henri\n  1234\nend wizard-note\n",
        ));
        assert_eq!(db.pages.len(), 2);
        assert_eq!(db.pages[0].name, "");
        assert_eq!(db.pages[1].name, "Gallery");
        assert_eq!(db.wizard_notes.len(), 1);
        assert_eq!(db.wizard_notes[0].name, "henri");
    }

    #[test]
    fn lenient_reading_reports_errors_and_keeps_going() {
        let (db, errors) = GwDatabase::read_lenient(b"nonsense\nfam A B + C D\n", "t.gw");
        assert_eq!(errors.len(), 1);
        assert_eq!(db.families.len(), 1);
    }
}
