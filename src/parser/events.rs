//! `gwplus` event blocks, and the witness and note lines that follow each event.
//!
//! Port of the `fevt`/`pevt` loops, `loop_witn` and `loop_note`.

use crate::error::{GwError, GwErrorKind, Result};
use crate::lexer::rest_after;
use crate::model::event::{Event, Witness, WitnessKind};
use crate::model::person::Sex;
use crate::parser::notes::strip_all_trailing_spaces;
use crate::parser::person::{optional_date, parse_parent};
use crate::parser::LineReader;

fn eof(expected: &'static str) -> GwError {
    GwError::at_line(0, GwErrorKind::UnexpectedEof { expected })
}

/// Reads the run of `wit` lines that follows an event, stopping at the first line that is
/// not one and putting it back.
///
/// The sex marker is part of the keyword token — `wit m:` — not a separate field.
///
/// # Errors
///
/// Returns an error when a witness line is malformed.
pub fn read_witnesses(reader: &mut LineReader<'_>) -> Result<Vec<Witness>> {
    let mut witnesses = Vec::new();
    while let Some(line) = reader.next_raw_line() {
        let mut cur = line.cursor();
        if cur.eat_any(&["wit", "wit:"]).is_none() {
            reader.unread();
            break;
        }
        let sex = match cur.eat_any(&["m:", "f:"]) {
            Some("m:") => Sex::Male,
            Some("f:") => Sex::Female,
            _ => Sex::Neuter,
        };
        let kind = cur
            .peek()
            .and_then(WitnessKind::from_tag)
            .inspect(|_| {
                cur.advance();
            })
            .unwrap_or_default();
        let ctx = line.ctx();
        let (person, _) = parse_parent(&mut cur, ctx)?;
        if !cur.is_empty() {
            return Err(ctx.err(GwErrorKind::TrailingTokens(cur.remaining().to_vec())));
        }
        witnesses.push(Witness { person, sex, kind });
    }
    Ok(witnesses)
}

/// Reads the run of `note` lines that follows an event, stopping at the first line that
/// is not one and putting it back.
///
/// Each line contributes its raw remainder, untokenized, so that underscores and
/// backslashes in prose survive.
#[must_use]
pub fn read_event_notes(reader: &mut LineReader<'_>) -> String {
    let mut lines: Vec<String> = Vec::new();
    while let Some(line) = reader.next_raw_line() {
        if line.keyword() != "note" {
            reader.unread();
            break;
        }
        lines.push(rest_after(&line.raw, "note").to_owned());
    }
    if lines.is_empty() {
        return String::new();
    }
    // GeneWeb appends an empty element before joining, so the body ends with a newline;
    // the trailing-space pass then removes it again.
    lines.push(String::new());
    strip_all_trailing_spaces(&lines.join("\n"))
}

/// Reads an event block body, up to `terminator`.
///
/// `parse_name` turns the leading tag into an event name; it returns `None` for a token
/// that does not start with `#`, which is the only shape GeneWeb rejects.
///
/// # Errors
///
/// Returns an error when an event header is malformed or the block is unterminated.
pub fn read_events<N>(
    reader: &mut LineReader<'_>,
    terminator: &'static str,
    parse_name: impl Fn(&str) -> Option<N>,
) -> Result<Vec<Event<N>>> {
    let mut events = Vec::new();
    loop {
        let line = reader.next_raw_line().ok_or_else(|| eof(terminator))?;
        if line.raw == terminator {
            return Ok(events);
        }
        let ctx = line.ctx();
        let mut cur = line.cursor();

        let tag = cur
            .advance()
            .ok_or_else(|| ctx.syntax("expected an event name"))?;
        let name =
            parse_name(tag).ok_or_else(|| ctx.err(GwErrorKind::UnknownTag(tag.to_owned())))?;

        let date = optional_date(&mut cur, ctx, false)?.into_date();
        let place = cur.field("#p");
        let cause = cur.field("#c");
        let source = cur.field("#s");
        if !cur.is_empty() {
            return Err(ctx.err(GwErrorKind::TrailingTokens(cur.remaining().to_vec())));
        }

        let witnesses = read_witnesses(reader)?;
        let note = read_event_notes(reader);

        events.push(Event {
            name,
            date,
            place,
            cause,
            source,
            note,
            witnesses,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::event::{FamilyEventName, PersonEventName};
    use crate::model::key::Somebody;

    #[test]
    fn reads_a_family_event_with_notes() {
        let input =
            b"#marr 3/6/1850 #p Reims #c raison #s acte\nnote first\nnote second\nend fevt\n";
        let mut r = LineReader::new(input);
        let events = read_events(&mut r, "end fevt", FamilyEventName::from_tag).unwrap();
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.name, FamilyEventName::Marriage);
        assert_eq!(e.place, "Reims");
        assert_eq!(e.cause, "raison");
        assert_eq!(e.source, "acte");
        assert_eq!(e.note, "first\nsecond");
        assert!(e.date.is_some());
    }

    #[test]
    fn reads_witnesses_with_sex_and_kind() {
        let input = b"#birt 1850\nwit m: #godp Dupont Jean\nwit f: Martin Marie\nend pevt\n";
        let mut r = LineReader::new(input);
        let events = read_events(&mut r, "end pevt", PersonEventName::from_tag).unwrap();
        let w = &events[0].witnesses;
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].sex, Sex::Male);
        assert_eq!(w[0].kind, WitnessKind::GodParent);
        assert_eq!(w[1].sex, Sex::Female);
        assert_eq!(w[1].kind, WitnessKind::Witness);
        assert!(matches!(w[0].person, Somebody::Reference(_)));
    }

    #[test]
    fn several_events_in_one_block() {
        let input = b"#birt 1850\n#deat 1920\n#occu\nend pevt\n";
        let mut r = LineReader::new(input);
        let events = read_events(&mut r, "end pevt", PersonEventName::from_tag).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].name, PersonEventName::Occupation);
    }

    #[test]
    fn html_in_notes_survives_untokenized() {
        // From `test/galichet.gw`: underscores and angle brackets must come through as
        // written, which is why note lines bypass the tokenizer.
        let input = "#marr\nnote <br>1886 - Habitent la Verdelière (6 enfants)\nend fevt\n";
        let mut r = LineReader::new(input.as_bytes());
        r.set_encoding(crate::encoding::Encoding::Utf8);
        let events = read_events(&mut r, "end fevt", FamilyEventName::from_tag).unwrap();
        assert_eq!(
            events[0].note,
            "<br>1886 - Habitent la Verdelière (6 enfants)"
        );
    }

    #[test]
    fn an_unknown_hash_tag_becomes_a_named_event() {
        let mut r = LineReader::new(b"#future 1850\nend pevt\n");
        let events = read_events(&mut r, "end pevt", PersonEventName::from_tag).unwrap();
        assert_eq!(events[0].name, PersonEventName::Named("future".into()));
    }

    #[test]
    fn a_tag_without_a_hash_is_rejected() {
        let mut r = LineReader::new(b"birt 1850\nend pevt\n");
        assert!(read_events(&mut r, "end pevt", PersonEventName::from_tag).is_err());
    }

    #[test]
    fn an_unterminated_block_is_an_error() {
        let mut r = LineReader::new(b"#birt 1850\n");
        assert!(read_events(&mut r, "end pevt", PersonEventName::from_tag).is_err());
    }
}
