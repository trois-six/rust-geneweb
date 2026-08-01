//! `rel` blocks: relations to parent figures outside the family structure.
//!
//! Port of `get_relation`.
//!
//! A relation line takes one of three shapes, distinguished by where the colon sits:
//!
//! ```text
//! - adop: Father + Mother     both parents on one line
//! - adop fath: Father         father only
//! - adop moth: Mother         mother only
//! ```

use crate::error::{GwErrorKind, Result};
use crate::model::relation::{Relation, RelationType};
use crate::parser::person::{parse_parent, LineCtx};
use crate::parser::LineReader;

/// Parses one relation line.
///
/// # Errors
///
/// Returns an error when the line is not a well-formed relation.
pub fn parse_relation(line: &crate::parser::Line) -> Result<Relation> {
    let ctx = line.ctx();
    let mut cur = line.cursor();

    if !cur.eat("-") {
        return Err(ctx.syntax("a relation line must start with `-`"));
    }
    let keyword = cur
        .advance()
        .ok_or_else(|| ctx.syntax("expected a relation keyword"))?;
    let relation_type = RelationType::from_keyword(keyword)
        .ok_or_else(|| ctx.err(GwErrorKind::UnknownTag(keyword.to_owned())))?;

    // The colon on the keyword itself means both parents follow, joined by `+`.
    let (father, mother) = if keyword.ends_with(':') {
        let (father, _) = parse_parent(&mut cur, ctx)?;
        if !cur.eat("+") {
            return Err(ctx.syntax("expected `+` between the two parents"));
        }
        let (mother, _) = parse_parent(&mut cur, ctx)?;
        (Some(father), Some(mother))
    } else {
        match cur.advance() {
            Some("fath:") => (Some(parse_parent(&mut cur, ctx)?.0), None),
            Some("moth:") => (None, Some(parse_parent(&mut cur, ctx)?.0)),
            _ => return Err(ctx.syntax("expected `fath:` or `moth:`")),
        }
    };

    if !cur.is_empty() {
        return Err(ctx.err(GwErrorKind::TrailingTokens(cur.remaining().to_vec())));
    }

    Ok(Relation {
        relation_type,
        father,
        mother,
        // The grammar has no syntax for a relation source, but GeneWeb's record has the
        // field, so it is kept and always empty.
        sources: String::new(),
    })
}

/// Reads a `rel` block body, up to the `end` line.
///
/// # Errors
///
/// Returns an error when a relation is malformed or the block is unterminated.
pub fn read_relations(reader: &mut LineReader<'_>) -> Result<Vec<Relation>> {
    let mut relations = Vec::new();
    loop {
        let line = reader.next_raw_line().ok_or_else(|| {
            LineCtx { no: 0, raw: "" }.err(GwErrorKind::UnexpectedEof { expected: "end" })
        })?;
        if line.raw == "end" {
            return Ok(relations);
        }
        relations.push(parse_relation(&line)?);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::key::Somebody;

    fn relations(input: &str) -> Vec<Relation> {
        let mut r = LineReader::new(input.as_bytes());
        read_relations(&mut r).expect("should parse")
    }

    #[test]
    fn both_parents_on_one_line() {
        let rs = relations("- adop: Dupont Jean + Martin Marie\nend\n");
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].relation_type, RelationType::Adoption);
        assert!(matches!(rs[0].father, Some(Somebody::Reference(_))));
        assert!(matches!(rs[0].mother, Some(Somebody::Reference(_))));
    }

    #[test]
    fn one_parent_at_a_time() {
        let rs = relations("- godp fath: Dupont Jean\n- fost moth: Martin Marie\nend\n");
        assert_eq!(rs[0].relation_type, RelationType::GodParent);
        assert!(rs[0].mother.is_none());
        assert_eq!(rs[1].relation_type, RelationType::FosterParent);
        assert!(rs[1].father.is_none());
    }

    #[test]
    fn every_relation_keyword_parses() {
        for keyword in RelationType::KEYWORDS {
            let input = format!("- {keyword} fath: Dupont Jean\nend\n");
            let rs = relations(&input);
            assert_eq!(rs[0].relation_type.keyword(), *keyword);
        }
    }

    #[test]
    fn malformed_lines_are_errors() {
        for input in [
            "- nope fath: A B\nend\n",
            "- adop: A B\nend\n",
            "adop fath: A B\nend\n",
            "- adop\nend\n",
            "- adop fath: A B trailing junk here\nend\n",
        ] {
            let mut r = LineReader::new(input.as_bytes());
            assert!(read_relations(&mut r).is_err(), "{input:?} should fail");
        }
    }

    #[test]
    fn an_unterminated_block_is_an_error() {
        let mut r = LineReader::new(b"- adop fath: A B\n");
        assert!(read_relations(&mut r).is_err());
    }
}
