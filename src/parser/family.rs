//! The `fam` block.
//!
//! Port of `get_mar_date` and the `fam` arm of `read_family`.
//!
//! A family is one dense header line followed by optional sections in a fixed order:
//!
//! ```text
//! fam <father> +<date> [#tag <sexes>] [#mp place] [#mn note] [#ms src] [-<date>|#sep] <mother>
//! wit [m:|f:] <person>          (repeated)
//! src <family source>
//! csrc <default source for children>
//! cbp <default birth place for children>
//! comm <free text>
//! fevt … end fevt
//! beg
//! - [h|m|f] <child>             (repeated)
//! end
//! ```

use crate::date::{self, GwDate};
use crate::error::{GwErrorKind, Result};
use crate::lexer::{rest_after, Cursor};
use crate::model::event::{FamilyEventName, Witness, WitnessKind};
use crate::model::family::{Divorce, Family, RelationKind};
use crate::model::person::Person;
use crate::model::person::Sex;
use crate::parser::events::read_events;
use crate::parser::person::{get_optional_sex, parse_child, parse_parent, FamilyDefaults, LineCtx};
use crate::parser::{Line, LineReader};

/// Everything the `+…` middle of a family line carries.
struct Marriage {
    relation: RelationKind,
    father_sex: Sex,
    mother_sex: Sex,
    date: Option<GwDate>,
    place: String,
    note: String,
    src: String,
    divorce: Divorce,
}

/// The relation tags that carry an explicit two-letter sex code.
const SEXED_RELATIONS: &[(&str, RelationKind)] = &[
    ("#noment", RelationKind::NoMention),
    ("#nsck", RelationKind::NoSexesCheckNotMarried),
    ("#nsckm", RelationKind::NoSexesCheckMarried),
    ("#banns", RelationKind::MarriageBann),
    ("#contract", RelationKind::MarriageContract),
    ("#license", RelationKind::MarriageLicense),
    ("#pacs", RelationKind::Pacs),
    ("#residence", RelationKind::Residence),
];

/// Decodes a two-letter sex code such as `mf`, `f?` or `??`.
fn decode_sex_pair(code: &str) -> Option<(Sex, Sex)> {
    let letter = |c: u8| match c {
        b'm' => Some(Sex::Male),
        b'f' => Some(Sex::Female),
        b'?' => Some(Sex::Neuter),
        _ => None,
    };
    let b = code.as_bytes();
    if b.len() != 2 {
        return None;
    }
    Some((letter(b[0])?, letter(b[1])?))
}

/// Reads the `+…` portion of a family line, as `get_mar_date` does.
fn get_mar_date(cur: &mut Cursor<'_>, ctx: LineCtx<'_>) -> Result<Marriage> {
    let token = cur
        .advance()
        .ok_or_else(|| ctx.syntax("expected `+` to open the union"))?;
    if !token.starts_with('+') {
        return Err(ctx.syntax(format!("expected `+`, found `{token}`")));
    }
    let date = if token.len() > 1 {
        date::parse_from(token, 1).map_err(|kind| ctx.err(kind))?
    } else {
        None
    };

    let (relation, father_sex, mother_sex) = if cur.eat("#nm") {
        (RelationKind::NotMarried, Sex::Male, Sex::Female)
    } else if cur.eat("#eng") {
        (RelationKind::Engaged, Sex::Male, Sex::Female)
    } else {
        let matched = SEXED_RELATIONS
            .iter()
            .find(|(tag, _)| cur.peek() == Some(*tag));
        match matched {
            // `#noment` is the one tag that is also valid without a sex code.
            Some(("#noment", kind)) if cur.peek_at(1).is_none_or(|c| c.len() != 2) => {
                cur.advance();
                (*kind, Sex::Male, Sex::Female)
            }
            Some((_, kind)) if cur.peek_at(1).is_some_and(|c| c.len() == 2) => {
                let kind = *kind;
                cur.advance();
                // A code of the right length but the wrong letters keeps the relation
                // kind and leaves the token to be read as part of the mother's name.
                match decode_sex_pair(cur.peek().unwrap_or_default()) {
                    Some((f, m)) => {
                        cur.advance();
                        (kind, f, m)
                    }
                    None => (kind, Sex::Male, Sex::Female),
                }
            }
            // A sexed tag without its code is not recognised at all: the tag stays put
            // and will fail when the mother's name is parsed. This mirrors GeneWeb.
            _ => (RelationKind::Married, Sex::Male, Sex::Female),
        }
    };

    let place = cur.field("#mp");
    let note = cur.field("#mn");
    let src = cur.field("#ms");

    let divorce = match cur.peek() {
        Some("#sep") => {
            cur.advance();
            Divorce::Separated(None)
        }
        Some(token) if token.starts_with('-') => {
            let token = cur.advance().unwrap_or_default();
            if token.len() > 1 {
                Divorce::Divorced(date::parse_from(token, 1).map_err(|kind| ctx.err(kind))?)
            } else {
                Divorce::Divorced(None)
            }
        }
        _ => Divorce::NotDivorced,
    };

    Ok(Marriage {
        relation,
        father_sex,
        mother_sex,
        date,
        place,
        note,
        src,
        divorce,
    })
}

/// Reads a one-token section such as `src X`, which must have exactly two tokens.
fn read_tagged_section(reader: &mut LineReader<'_>, keyword: &str) -> Result<String> {
    let Some(line) = reader.peek_block_line() else {
        return Ok(String::new());
    };
    if line.keyword() != keyword {
        return Ok(String::new());
    }
    reader.next_block_line();
    match line.tokens.len() {
        2 => Ok(crate::lexer::cut_space(&line.tokens[1]).to_owned()),
        _ => Err(line
            .ctx()
            .syntax(format!("`{keyword}` takes exactly one value"))),
    }
}

/// Reads the run of `wit` lines under a family line.
///
/// Unlike event witnesses these carry no kind: the family line's `wit` syntax predates
/// event witnesses and admits only a sex marker.
fn read_family_witnesses(reader: &mut LineReader<'_>) -> Result<Vec<Witness>> {
    let mut witnesses = Vec::new();
    while let Some(line) = reader.peek_block_line() {
        if !matches!(line.keyword(), "wit" | "wit:") {
            break;
        }
        reader.next_block_line();
        let ctx = line.ctx();
        let mut cur = line.cursor();
        cur.advance();
        let sex = match cur.eat_any(&["m:", "f:"]) {
            Some("m:") => Sex::Male,
            Some("f:") => Sex::Female,
            _ => Sex::Neuter,
        };
        let (person, _) = parse_parent(&mut cur, ctx)?;
        if !cur.is_empty() {
            return Err(ctx.err(GwErrorKind::TrailingTokens(cur.remaining().to_vec())));
        }
        witnesses.push(Witness {
            person,
            sex,
            kind: WitnessKind::Witness,
        });
    }
    Ok(witnesses)
}

/// Reads a run of `comm` lines, joined back into the multi-line comment they came from.
///
/// `gwc` reads exactly one, and `gwu` writes exactly one because it flattens newlines on
/// the way out. Files in the wild — from other exporters — put each line of a comment on
/// its own `comm` line, and stopping after the first desynchronises the reader for the
/// rest of the file. Accepting the run is a strict superset: a single `comm` still parses
/// identically.
fn read_comment(reader: &mut LineReader<'_>) -> String {
    let mut lines: Vec<String> = Vec::new();
    while let Some(line) = reader.peek_block_line() {
        if line.keyword() != "comm" {
            break;
        }
        reader.next_block_line();
        // The raw remainder of the line, so that prose keeps its underscores. GeneWeb
        // slices unconditionally here and would crash on a bare `comm`; this yields an
        // empty comment instead.
        lines.push(rest_after(&line.raw, "comm").to_owned());
    }
    lines.join("\n")
}

/// Reads the `beg`…`end` block of children, if there is one.
fn read_children(
    reader: &mut LineReader<'_>,
    surname: &str,
    defaults: FamilyDefaults<'_>,
) -> Result<Vec<Person>> {
    let opens = matches!(reader.peek_block_line(), Some(line) if line.tokens == ["beg"]);
    if !opens {
        return Ok(Vec::new());
    }
    reader.next_block_line();

    let mut children = Vec::new();
    loop {
        let line = reader.next_block_line().ok_or_else(|| {
            LineCtx { no: 0, raw: "" }.err(GwErrorKind::UnexpectedEof { expected: "end" })
        })?;
        if line.tokens == ["end"] {
            return Ok(children);
        }
        let ctx = line.ctx();
        let mut cur = line.cursor();
        if !cur.eat("-") {
            return Err(ctx.syntax("expected a child line starting with `-`"));
        }
        let sex = get_optional_sex(&mut cur);
        let child = parse_child(&mut cur, surname, sex, defaults, ctx)?;
        if !cur.is_empty() {
            return Err(ctx.err(GwErrorKind::TrailingTokens(cur.remaining().to_vec())));
        }
        children.push(child);
    }
}

/// Reads a `fam` block, given its opening line.
///
/// # Errors
///
/// Returns an error when any part of the block is malformed.
pub fn read_family(
    reader: &mut LineReader<'_>,
    line: &Line,
    origin_file: &str,
) -> Result<Box<Family>> {
    let ctx = line.ctx();
    let mut cur = line.cursor();
    if !cur.eat("fam") {
        return Err(ctx.syntax("expected `fam`"));
    }

    let (father, surname) = parse_parent(&mut cur, ctx)?;
    let marriage = get_mar_date(&mut cur, ctx)?;
    let (mother, _) = parse_parent(&mut cur, ctx)?;
    if !cur.is_empty() {
        return Err(ctx.err(GwErrorKind::TrailingTokens(cur.remaining().to_vec())));
    }

    let witnesses = read_family_witnesses(reader)?;

    let sources = read_tagged_section(reader, "src")?;
    let children_sources = read_tagged_section(reader, "csrc")?;
    let children_birth_place = read_tagged_section(reader, "cbp")?;

    let comment = read_comment(reader);

    let events = match reader.peek_block_line() {
        Some(line) if line.keyword() == "fevt" => {
            reader.next_block_line();
            read_events(reader, "end fevt", FamilyEventName::from_tag)?
        }
        _ => Vec::new(),
    };

    let children = read_children(
        reader,
        &surname,
        FamilyDefaults {
            sources: &children_sources,
            birth_place: &children_birth_place,
        },
    )?;

    Ok(Box::new(Family {
        father,
        mother,
        father_sex: marriage.father_sex,
        mother_sex: marriage.mother_sex,
        relation: marriage.relation,
        marriage: marriage.date,
        marriage_place: marriage.place,
        marriage_note: marriage.note,
        marriage_src: marriage.src,
        divorce: marriage.divorce,
        witnesses,
        sources,
        children_sources,
        children_birth_place,
        comment,
        events,
        children,
        origin_file: origin_file.to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::key::Somebody;

    fn family(input: &str) -> Box<Family> {
        let mut r = LineReader::new(input.as_bytes());
        // Test sources are UTF-8; a real file would say so with an `encoding: utf-8`
        // directive, without which the format is ISO-8859-1.
        r.set_encoding(crate::encoding::Encoding::Utf8);
        let line = r.next_block_line().expect("a fam line");
        read_family(&mut r, &line, "test.gw").expect("should parse")
    }

    #[test]
    fn a_minimal_family() {
        let f = family("fam Dupont Jean + Martin Marie\n");
        assert_eq!(f.relation, RelationKind::Married);
        assert!(matches!(f.father, Somebody::Reference(_)));
        assert!(matches!(f.mother, Somebody::Reference(_)));
        assert!(f.marriage.is_none());
        assert_eq!(f.divorce, Divorce::NotDivorced);
        assert_eq!(f.origin_file, "test.gw");
    }

    #[test]
    fn the_full_header_line() {
        let f = family(
            "fam Dupont Jean +3/6/1850 #nsckm mf #mp Reims #mn note #ms acte -1860 Martin Marie\n",
        );
        assert_eq!(f.relation, RelationKind::NoSexesCheckMarried);
        assert_eq!(f.father_sex, Sex::Male);
        assert_eq!(f.mother_sex, Sex::Female);
        assert_eq!(f.marriage_place, "Reims");
        assert_eq!(f.marriage_note, "note");
        assert_eq!(f.marriage_src, "acte");
        assert!(matches!(f.divorce, Divorce::Divorced(Some(_))));
    }

    #[test]
    fn sex_codes_can_be_swapped_or_unknown() {
        let f = family("fam A B + #nsck fm C D\n");
        assert_eq!(f.father_sex, Sex::Female);
        assert_eq!(f.mother_sex, Sex::Male);

        let f = family("fam A B + #pacs ?? C D\n");
        assert_eq!(f.father_sex, Sex::Neuter);
        assert_eq!(f.mother_sex, Sex::Neuter);
    }

    #[test]
    fn noment_is_the_only_sexed_tag_valid_without_a_code() {
        let f = family("fam A B + #noment C D\n");
        assert_eq!(f.relation, RelationKind::NoMention);
        assert_eq!(f.father_sex, Sex::Male);
    }

    #[test]
    fn separation_and_divorce_without_a_date() {
        assert_eq!(
            family("fam A B + #sep C D\n").divorce,
            Divorce::Separated(None)
        );
        assert_eq!(family("fam A B + - C D\n").divorce, Divorce::Divorced(None));
    }

    #[test]
    fn witnesses_sources_and_comment() {
        let f = family(concat!(
            "fam A B + C D\n",
            "wit m: Témoin Paul\n",
            "wit f: Témoin Anne\n",
            "src source_famille\n",
            "csrc source_enfants\n",
            "cbp Reims\n",
            "comm un commentaire avec des espaces\n",
        ));
        assert_eq!(f.witnesses.len(), 2);
        assert_eq!(f.witnesses[0].sex, Sex::Male);
        assert_eq!(f.witnesses[1].sex, Sex::Female);
        // Family witnesses never carry a kind.
        assert_eq!(f.witnesses[0].kind, WitnessKind::Witness);
        assert_eq!(f.sources, "source famille");
        assert_eq!(f.children_sources, "source enfants");
        assert_eq!(f.children_birth_place, "Reims");
        assert_eq!(f.comment, "un commentaire avec des espaces");
    }

    #[test]
    fn children_inherit_the_father_surname_and_family_defaults() {
        let f = family(concat!(
            "fam Galichet Jean + Loche Marie\n",
            "csrc défaut\n",
            "cbp Yzernay\n",
            "beg\n",
            "- m Pierre 1814\n",
            "- f Thérèse 1830 #bp Reims\n",
            "- h Paul Martin 1816\n",
            "end\n",
        ));
        assert_eq!(f.children.len(), 3);
        assert_eq!(f.children[0].surname, "Galichet");
        assert_eq!(f.children[0].sex, Sex::Male);
        assert_eq!(f.children[0].sources, "défaut");
        assert_eq!(f.children[0].birth_place, "Yzernay");
        assert_eq!(f.children[1].sex, Sex::Female);
        // An explicit birth place wins over `cbp`.
        assert_eq!(f.children[1].birth_place, "Reims");
        // An explicit surname wins over the father's.
        assert_eq!(f.children[2].surname, "Martin");
    }

    #[test]
    fn family_events() {
        let f = family(concat!(
            "fam A B + C D\n",
            "fevt\n",
            "#marr 1850 #p Reims\n",
            "note une note\n",
            "#div 1860\n",
            "end fevt\n",
        ));
        assert_eq!(f.events.len(), 2);
        assert_eq!(f.events[0].name, FamilyEventName::Marriage);
        assert_eq!(f.events[0].place, "Reims");
        assert_eq!(f.events[0].note, "une note");
        assert_eq!(f.events[1].name, FamilyEventName::Divorce);
    }

    #[test]
    fn every_section_at_once_in_order() {
        let f = family(concat!(
            "fam A B + C D\n",
            "wit Témoin Paul\n",
            "src s\n",
            "csrc cs\n",
            "cbp cb\n",
            "comm c\n",
            "fevt\n",
            "#marr\n",
            "end fevt\n",
            "beg\n",
            "- m Enfant\n",
            "end\n",
        ));
        assert_eq!(f.witnesses.len(), 1);
        assert_eq!(f.sources, "s");
        assert_eq!(f.children_sources, "cs");
        assert_eq!(f.children_birth_place, "cb");
        assert_eq!(f.comment, "c");
        assert_eq!(f.events.len(), 1);
        assert_eq!(f.children.len(), 1);
    }

    #[test]
    fn a_defined_parent_carries_its_own_details() {
        let f = family("fam Corno John 1935 #bp Soisy 1997 + Rempp Zabeth\n");
        let father = f.father.definition().expect("an inline definition");
        assert_eq!(father.first_name, "John");
        assert_eq!(father.birth_place, "Soisy");
        assert!(matches!(f.mother, Somebody::Reference(_)));
    }

    #[test]
    fn the_anonymous_spouse() {
        let f = family("fam Doe John 0 + ? ?\n");
        assert!(f.mother.definition().is_some());
        assert_eq!(f.mother.key().first_name, "?");
    }

    #[test]
    fn a_run_of_comm_lines_is_one_multi_line_comment() {
        // `gwu` flattens a comment onto a single `comm` line, but other exporters emit
        // one line each. Stopping after the first used to leave the remaining lines
        // looking like top-level blocks, which desynchronised the reader for the whole
        // rest of the file.
        let f = family(concat!(
            "fam A B + C D\n",
            "comm première ligne\n",
            "comm deuxième ligne\n",
            "comm troisième ligne\n",
            "beg\n",
            "- m Enfant\n",
            "end\n",
        ));
        assert_eq!(f.comment, "première ligne\ndeuxième ligne\ntroisième ligne");
        // The block after the comment still parses, which is the point.
        assert_eq!(f.children.len(), 1);
    }

    #[test]
    fn a_multivalued_src_line_is_an_error() {
        let mut r = LineReader::new(b"fam A B + C D\nsrc one two\n");
        let line = r.next_block_line().unwrap();
        assert!(read_family(&mut r, &line, "t.gw").is_err());
    }

    #[test]
    fn an_unterminated_children_block_is_an_error() {
        let mut r = LineReader::new(b"fam A B + C D\nbeg\n- m X\n");
        let line = r.next_block_line().unwrap();
        assert!(read_family(&mut r, &line, "t.gw").is_err());
    }
}
