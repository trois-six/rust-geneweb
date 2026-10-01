//! Top-level block dispatch.
//!
//! Port of `read_family`'s outer `match`, which decides what each block is from its first
//! line and hands the body to the right reader.

use crate::encoding::Encoding;
use crate::error::{GwErrorKind, Result};
use crate::lexer::rest_after;
use crate::model::block::GwBlock;
use crate::model::event::PersonEventName;
use crate::model::person::Sex;
use crate::parser::events::read_events;
use crate::parser::family::read_family;
use crate::parser::notes::{read_notes, read_notes_db};
use crate::parser::person::{parse_parent, LineCtx};
use crate::parser::relations::read_relations;
use crate::parser::{Line, LineReader};

/// Reads a `.gw` file as a sequence of blocks.
///
/// Implements [`Iterator`], yielding one [`GwBlock`] per block. The two directives —
/// `encoding: utf-8` and `gwplus` — are not blocks: they change how the rest of the file
/// is read, and are consumed silently.
///
/// In strict mode a malformed block ends iteration after yielding its error. In lenient
/// mode the block is skipped and reading continues, which mirrors GeneWeb's `no_fail`.
pub struct BlockReader<'a> {
    reader: LineReader<'a>,
    origin_file: String,
    gwplus: bool,
    lenient: bool,
    failed: bool,
}

impl<'a> BlockReader<'a> {
    /// Opens a reader over the raw bytes of a `.gw` file.
    ///
    /// `origin_file` is recorded on every family, as GeneWeb records the basename of the
    /// file a family came from.
    pub fn new(input: &'a [u8], origin_file: impl Into<String>) -> Self {
        Self {
            reader: LineReader::new(input),
            origin_file: origin_file.into(),
            gwplus: false,
            lenient: false,
            failed: false,
        }
    }

    /// Continues past malformed blocks instead of stopping at the first one.
    #[must_use]
    pub fn lenient(mut self, lenient: bool) -> Self {
        self.lenient = lenient;
        self
    }

    /// Whether a `gwplus` directive has been seen so far.
    ///
    /// The directive is normally the second line, but nothing in the format requires it,
    /// so this is only final once the file has been read.
    #[must_use]
    pub fn is_gwplus(&self) -> bool {
        self.gwplus
    }

    /// The encoding currently in force.
    #[must_use]
    pub fn encoding(&self) -> Encoding {
        self.reader.encoding()
    }

    /// Reads one block, or a directive.
    ///
    /// Returns `Ok(None)` at end of file, and `Ok(Some(None))` for a directive, which is
    /// consumed rather than yielded.
    fn read_one(&mut self, line: &Line) -> Result<Option<GwBlock>> {
        let ctx = line.ctx();

        // Directives. Neither produces a block.
        if line.tokens == ["encoding:", "utf-8"] {
            self.reader.set_encoding(Encoding::Utf8);
            return Ok(None);
        }
        if line.tokens == ["gwplus"] {
            self.gwplus = true;
            return Ok(None);
        }

        match line.keyword() {
            "fam" => Ok(Some(GwBlock::Family(read_family(
                &mut self.reader,
                line,
                &self.origin_file,
            )?))),

            "notes-db" => Ok(Some(GwBlock::DatabaseNotes {
                page: String::new(),
                text: read_notes_db(&mut self.reader, "end notes-db")?,
            })),

            // An extended page's name is one token, since spaces are written `_`.
            "page-ext" if line.tokens.len() == 2 => Ok(Some(GwBlock::DatabaseNotes {
                page: rest_after(&line.raw, "page-ext").to_owned(),
                text: read_notes_db(&mut self.reader, "end page-ext")?,
            })),

            "wizard-note" => Ok(Some(GwBlock::WizardNotes {
                wizard: rest_after(&line.raw, "wizard-note").to_owned(),
                text: read_notes_db(&mut self.reader, "end wizard-note")?,
            })),

            // A bare `notes` is the pre-5.00 spelling of the database notes block.
            "notes" if line.tokens.len() == 1 => Ok(Some(GwBlock::DatabaseNotes {
                page: String::new(),
                text: read_notes(&mut self.reader)?,
            })),

            "notes" => {
                let mut cur = line.cursor();
                cur.advance();
                let (person, _) = parse_parent(&mut cur, ctx)?;
                if !cur.is_empty() {
                    return Err(ctx.err(GwErrorKind::TrailingTokens(cur.remaining().to_vec())));
                }
                self.expect_beg(ctx)?;
                Ok(Some(GwBlock::PersonNotes {
                    key: person.key(),
                    text: read_notes(&mut self.reader)?,
                }))
            }

            "rel" => {
                let mut cur = line.cursor();
                cur.advance();
                let (person, _) = parse_parent(&mut cur, ctx)?;
                let sex = match cur.eat_any(&["#h", "#m", "#f"]) {
                    Some("#h" | "#m") => Sex::Male,
                    Some("#f") => Sex::Female,
                    _ => Sex::Neuter,
                };
                if !cur.is_empty() {
                    return Err(ctx.err(GwErrorKind::TrailingTokens(cur.remaining().to_vec())));
                }
                self.expect_beg(ctx)?;
                Ok(Some(GwBlock::Relations {
                    person,
                    sex,
                    relations: read_relations(&mut self.reader)?,
                }))
            }

            "pevt" => {
                let mut cur = line.cursor();
                cur.advance();
                let (person, _) = parse_parent(&mut cur, ctx)?;
                if !cur.is_empty() {
                    return Err(ctx.err(GwErrorKind::TrailingTokens(cur.remaining().to_vec())));
                }
                Ok(Some(GwBlock::PersonEvents {
                    person,
                    // GeneWeb hardcodes this: the block has no syntax for a sex.
                    sex: Sex::Neuter,
                    events: read_events(&mut self.reader, "end pevt", PersonEventName::from_tag)?,
                }))
            }

            other => Err(ctx.err(GwErrorKind::UnknownBlock(other.to_owned()))),
        }
    }

    /// Consumes the `beg` line that opens a `notes` or `rel` body.
    fn expect_beg(&mut self, ctx: LineCtx<'_>) -> Result<()> {
        match self.reader.next_block_line() {
            Some(line) if line.tokens == ["beg"] => Ok(()),
            Some(line) => Err(line.ctx().syntax("expected `beg`")),
            None => Err(ctx.err(GwErrorKind::UnexpectedEof { expected: "beg" })),
        }
    }
}

impl Iterator for BlockReader<'_> {
    type Item = Result<GwBlock>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        loop {
            let line = self.reader.next_block_line()?;
            match self.read_one(&line) {
                Ok(Some(block)) => return Some(Ok(block)),
                // A directive: keep going.
                Ok(None) => {}
                Err(e) => {
                    if !self.lenient {
                        self.failed = true;
                    }
                    return Some(Err(e));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::key::Key;

    fn blocks(input: &str) -> Vec<GwBlock> {
        BlockReader::new(input.as_bytes(), "t.gw")
            .collect::<Result<Vec<_>>>()
            .expect("should parse")
    }

    #[test]
    fn directives_are_consumed_not_yielded() {
        let mut r = BlockReader::new(b"encoding: utf-8\ngwplus\n\nfam A B + C D\n", "t.gw");
        let block = r.next().expect("a block").expect("ok");
        assert!(matches!(block, GwBlock::Family(_)));
        assert!(r.next().is_none());
        assert!(r.is_gwplus());
        assert_eq!(r.encoding(), Encoding::Utf8);
    }

    #[test]
    fn the_encoding_directive_applies_to_later_lines() {
        // `é` as UTF-8 bytes, which would be mojibake without the directive.
        let input = "encoding: utf-8\nfam Galichet Thérèse + Loche Marie\n";
        let bs = blocks(input);
        let GwBlock::Family(f) = &bs[0] else {
            panic!("expected a family")
        };
        assert_eq!(f.father.key().first_name, "Thérèse");
    }

    #[test]
    fn without_the_directive_the_format_is_latin_1() {
        // The same file without the directive: 0xC3 0xA9 is read as two Latin-1 chars.
        let bs = blocks("fam Galichet Thérèse + Loche Marie\n");
        let GwBlock::Family(f) = &bs[0] else {
            panic!("expected a family")
        };
        assert_eq!(f.father.key().first_name, "ThÃ©rÃ¨se");
    }

    #[test]
    fn person_notes() {
        let bs = blocks("notes Galichet Jean_Charles\nbeg\nsome text\nend notes\n");
        assert_eq!(
            bs[0],
            GwBlock::PersonNotes {
                key: Key::new("Jean Charles", "Galichet", 0),
                text: "some text".into(),
            }
        );
    }

    #[test]
    fn a_bare_notes_block_is_database_notes() {
        let bs = blocks("notes\ndatabase presentation\nend notes\n");
        assert_eq!(
            bs[0],
            GwBlock::DatabaseNotes {
                page: String::new(),
                text: "database presentation".into(),
            }
        );
    }

    #[test]
    fn database_notes_and_extended_pages() {
        let bs = blocks("notes-db\n  presentation\nend notes-db\n");
        assert!(matches!(&bs[0], GwBlock::DatabaseNotes { page, .. } if page.is_empty()));

        let bs = blocks("page-ext Gallery\n  content\nend page-ext\n");
        assert_eq!(
            bs[0],
            GwBlock::DatabaseNotes {
                page: "Gallery".into(),
                text: "content".into(),
            }
        );
    }

    #[test]
    fn wizard_notes_keep_their_timestamp_line() {
        let bs = blocks("wizard-note henri\n  1234567890\n  a note\nend wizard-note\n");
        assert_eq!(
            bs[0],
            GwBlock::WizardNotes {
                wizard: "henri".into(),
                text: "1234567890\na note".into(),
            }
        );
    }

    #[test]
    fn relation_blocks() {
        let bs = blocks("rel Dupont Jean #m\nbeg\n- adop fath: Martin Paul\nend\n");
        let GwBlock::Relations { sex, relations, .. } = &bs[0] else {
            panic!("expected relations")
        };
        assert_eq!(*sex, Sex::Male);
        assert_eq!(relations.len(), 1);
    }

    #[test]
    fn person_event_blocks() {
        let bs = blocks("pevt Dupont Jean\n#birt 1850 #p Reims\nend pevt\n");
        let GwBlock::PersonEvents { person, events, .. } = &bs[0] else {
            panic!("expected person events")
        };
        assert_eq!(person.key(), Key::new("Jean", "Dupont", 0));
        assert_eq!(events[0].name, PersonEventName::Birth);
    }

    #[test]
    fn an_unknown_block_is_an_error() {
        let mut r = BlockReader::new(b"nonsense here\n", "t.gw");
        assert!(r.next().unwrap().is_err());
        // Strict mode stops after the first failure.
        assert!(r.next().is_none());
    }

    #[test]
    fn lenient_mode_skips_past_a_bad_block() {
        let input = "nonsense here\nfam A B + C D\n";
        let mut r = BlockReader::new(input.as_bytes(), "t.gw").lenient(true);
        assert!(r.next().unwrap().is_err());
        assert!(matches!(r.next().unwrap().unwrap(), GwBlock::Family(_)));
        assert!(r.next().is_none());
    }

    #[test]
    fn comments_between_blocks_are_ignored() {
        let bs = blocks("# a comment\nfam A B + C D\n\n# another\nfam E F + G H\n");
        assert_eq!(bs.len(), 2);
    }

    #[test]
    fn an_empty_file_yields_nothing() {
        assert_eq!(blocks("").len(), 0);
        assert_eq!(blocks("\n\n# just comments\n").len(), 0);
    }
}
