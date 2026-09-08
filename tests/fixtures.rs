//! Reading real `.gw` files.
//!
//! `galichet.gw` is GeneWeb's own test corpus, taken verbatim from `test/galichet.gw` in
//! the GeneWeb repository. It is a `gwplus`, UTF-8 file exercising accents, HTML in
//! notes, wiki page links, family and personal events, relations and extended pages —
//! which makes it the closest thing to a conformance test this crate can have.

use geneweb::model::block::GwBlock;
use geneweb::parser::block::BlockReader;

const GALICHET: &[u8] = include_bytes!("fixtures/galichet.gw");

fn read(input: &[u8]) -> Vec<GwBlock> {
    BlockReader::new(input, "galichet.gw")
        .collect::<geneweb::Result<Vec<_>>>()
        .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn the_geneweb_reference_corpus_parses_cleanly() {
    let blocks = read(GALICHET);
    assert!(!blocks.is_empty());

    let families = blocks
        .iter()
        .filter(|b| matches!(b, GwBlock::Family(_)))
        .count();
    // The file's own `fam` line count.
    assert_eq!(families, 15);
}

#[test]
fn the_directives_are_honoured() {
    let mut reader = BlockReader::new(GALICHET, "galichet.gw");
    let blocks: Vec<_> = reader
        .by_ref()
        .collect::<geneweb::Result<Vec<_>>>()
        .unwrap();
    assert!(reader.is_gwplus(), "the file declares gwplus");
    assert_eq!(reader.encoding(), geneweb::encoding::Encoding::Utf8);

    // Accents survive, which only happens if the encoding directive took effect.
    let GwBlock::Family(first) = &blocks[0] else {
        panic!("the first block is a family")
    };
    assert_eq!(first.father.key().first_name, "Jean Pierre");
    let therese = first
        .children
        .iter()
        .find(|c| c.first_name == "Thérèse Eugénie")
        .expect("the accented child is found by its decoded name");
    assert_eq!(
        therese.birth_place,
        "[Châlons-sur-Marne] - Châlons-en-Champagne,51,Marne,Champagne-Ardenne,France"
    );
}

#[test]
fn every_block_kind_in_the_corpus_is_represented() {
    let blocks = read(GALICHET);
    let mut families = 0;
    let mut person_notes = 0;
    let mut database_notes = 0;
    let mut relations = 0;
    let mut person_events = 0;
    let mut wizard_notes = 0;
    for block in &blocks {
        match block {
            GwBlock::Family(_) => families += 1,
            GwBlock::PersonNotes { .. } => person_notes += 1,
            GwBlock::DatabaseNotes { .. } => database_notes += 1,
            GwBlock::Relations { .. } => relations += 1,
            GwBlock::PersonEvents { .. } => person_events += 1,
            GwBlock::WizardNotes { .. } => wizard_notes += 1,
        }
    }
    assert_eq!(families, 15);
    assert!(person_notes > 0, "the corpus has personal notes");
    assert!(database_notes > 0, "the corpus has extended pages");
    let _ = (relations, person_events, wizard_notes);
}

#[test]
fn family_events_and_their_multiline_notes_are_read() {
    let blocks = read(GALICHET);
    let GwBlock::Family(first) = &blocks[0] else {
        panic!("the first block is a family")
    };
    let event = &first.events[0];
    // Four `note` lines, joined with newlines, HTML intact.
    assert_eq!(event.note.lines().count(), 4);
    assert!(event.note.contains("<br>1886 - Habitent la Verdelière"));
    assert_eq!(first.children_sources, "rajout de 2 enfants pour test");
}

#[test]
fn reading_is_byte_for_byte_deterministic() {
    // Guards against any hidden dependence on iteration order or interning.
    assert_eq!(read(GALICHET), read(GALICHET));
}

#[test]
fn truncating_the_file_never_panics() {
    // Every prefix of a real file is malformed in some way. None may panic; each must
    // either parse or report an error.
    for len in 0..GALICHET.len() {
        let blocks = BlockReader::new(&GALICHET[..len], "truncated.gw").lenient(true);
        for block in blocks {
            let _ = block;
        }
    }
}

/// GeneWeb's own `gwc` cram test (`test/gwc/run.t`) records what `gwc` counts when it
/// compiles this exact file: `pcnt 35 persons` and `fcnt 15 families`. Reproducing those
/// numbers means references resolved to the same people GeneWeb resolves them to — the
/// strongest end-to-end check available without running GeneWeb.
#[test]
fn person_and_family_counts_match_gwc() {
    let db = geneweb::database::GwDatabase::read(GALICHET, "galichet.gw").expect("parses");
    assert_eq!(db.families.len(), 15, "family count");
    assert_eq!(db.persons.len(), 35, "person count");
}

#[test]
fn gallery_page_keeps_its_media_reference() {
    let db = geneweb::database::GwDatabase::read(GALICHET, "galichet.gw").expect("parses");
    let gallery = db
        .pages
        .iter()
        .find(|page| page.name == "Gallery")
        .expect("the Gallery extended page");
    assert!(gallery.text.contains("\"img\": \"jean_pierre.0.galichet.jpg\""));

    let data = db.to_gedcom();
    let page = data
        .custom_data
        .iter()
        .find(|tag| tag.tag == "_GWPAGE" && tag.value.as_deref() == Some("Gallery"))
        .expect("the Gallery GEDCOM extension");
    assert!(page.children.iter().any(|child| {
        child.tag == "NOTE"
            && child
                .value
                .as_deref()
                .is_some_and(|value| value.contains("jean_pierre.0.galichet.jpg"))
    }));
}

/// Reading a `.gw` and writing GEDCOM, then reading that GEDCOM back with `ged_io`.
///
/// This is the whole point of the crate: what comes out must be a file the wider GEDCOM
/// ecosystem can read, not just something that round-trips through our own types.
#[test]
fn a_gw_file_exports_to_gedcom_and_reads_back() {
    use ged_io::writer::GedcomWriter;
    use ged_io::GedcomBuilder;

    let db = geneweb::database::GwDatabase::read(GALICHET, "galichet.gw").expect("parses");
    let data = db.to_gedcom();
    assert_eq!(data.individuals.len(), 35);
    assert_eq!(data.families.len(), 15);

    let text = GedcomWriter::new()
        .write_to_string(&data)
        .expect("serialises");
    assert!(text.starts_with("0 HEAD"), "a GEDCOM file opens with HEAD");
    assert!(text.contains("1 NAME Jean Pierre /Galichet/"));
    assert!(text.contains("0 @I1@ INDI"));
    assert!(text.contains("0 @F1@ FAM"));

    // Back through ged_io's own parser: everybody and every family must survive.
    let reparsed = GedcomBuilder::new()
        .build_from_str(&text)
        .expect("ged_io reads what we wrote");
    assert_eq!(
        reparsed.individuals.len(),
        35,
        "individuals survive the round trip"
    );
    assert_eq!(
        reparsed.families.len(),
        15,
        "families survive the round trip"
    );
}

/// A `pevt` block may restate an event the person's own `fam` line already carries.
/// `galichet.gw` does exactly that — `pevt Galichet Jean_Pierre` repeats the `<1849`
/// death from the family line — and the gwplus rule is that the structured event wins.
/// Emitting both produced two `DEAT` records for one death.
#[test]
fn a_structured_event_supersedes_the_line_it_repeats() {
    use ged_io::types::event::Event;

    let db = geneweb::database::GwDatabase::read(GALICHET, "galichet.gw").expect("parses");
    let data = db.to_gedcom();

    for individual in &data.individuals {
        for kind in [Event::Birth, Event::Death, Event::Burial, Event::Baptism] {
            let count = individual.events.iter().filter(|e| e.event == kind).count();
            assert!(
                count <= 1,
                "{:?} has {count} {kind:?} events",
                individual.xref
            );
        }
    }
    for family in &data.families {
        for kind in [Event::Marriage, Event::Divorce] {
            let count = family.events.iter().filter(|e| e.event == kind).count();
            assert!(count <= 1, "{:?} has {count} {kind:?} events", family.xref);
        }
    }
}

/// GeneWeb's second `.gw` sample, from `test/install-cgi/test.gw`. It is small but
/// exercises a shape `galichet.gw` does not: two `pevt` blocks on different people, one
/// of them naming an event with no GEDCOM counterpart.
#[test]
fn the_second_geneweb_sample_parses() {
    use geneweb::model::event::PersonEventName;

    const SAMPLE: &[u8] = include_bytes!("fixtures/install-cgi.gw");
    let db = geneweb::database::GwDatabase::read(SAMPLE, "install-cgi.gw").expect("parses");

    assert!(db.gwplus);
    assert_eq!(db.families.len(), 1);
    // Two spouses and their child.
    assert_eq!(db.persons.len(), 3);

    let with_events: Vec<_> = db.persons.iter().filter(|p| !p.events.is_empty()).collect();
    assert_eq!(with_events.len(), 2, "two `pevt` blocks were merged in");

    // `#acco` has no standard GEDCOM tag and must survive as a labelled generic event.
    let accomplishment = db
        .persons
        .iter()
        .flat_map(|p| &p.events)
        .find(|e| e.name == PersonEventName::Accomplishment)
        .expect("the accomplishment event");
    assert!(!accomplishment.note.is_empty());

    let data = db.to_gedcom();
    assert_eq!(data.individuals.len(), 3);
    assert_eq!(data.families.len(), 1);
}
