//! Parental relations recorded outside the family structure.
//!
//! Port of `get_relation`. These appear in a `rel` block and attach a person to one or
//! two parent figures who are not their family-block parents.

use super::key::Somebody;

/// The kind of parental relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationType {
    /// Adoptive parents, written `adop`.
    Adoption,
    /// Recognising parents, written `reco`.
    Recognition,
    /// Candidate parents, written `cand`.
    CandidateParent,
    /// Godparents, written `godp`.
    GodParent,
    /// Foster parents, written `fost`.
    FosterParent,
}

impl RelationType {
    /// Every relation keyword, without the trailing colon form.
    pub const KEYWORDS: &'static [&'static str] = &["adop", "reco", "cand", "godp", "fost"];

    /// Parses a relation keyword.
    ///
    /// Both `adop` and `adop:` are accepted: the colon form introduces a pair of parents
    /// on one line, the bare form is followed by `fath:` or `moth:`.
    #[must_use]
    pub fn from_keyword(keyword: &str) -> Option<Self> {
        match keyword.strip_suffix(':').unwrap_or(keyword) {
            "adop" => Some(Self::Adoption),
            "reco" => Some(Self::Recognition),
            "cand" => Some(Self::CandidateParent),
            "godp" => Some(Self::GodParent),
            "fost" => Some(Self::FosterParent),
            _ => None,
        }
    }

    /// The keyword this relation is written with.
    #[must_use]
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Adoption => "adop",
            Self::Recognition => "reco",
            Self::CandidateParent => "cand",
            Self::GodParent => "godp",
            Self::FosterParent => "fost",
        }
    }
}

/// A relation to one or two parent figures.
#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
    /// What kind of relation this is.
    pub relation_type: RelationType,
    /// The father figure, if given.
    pub father: Option<Somebody>,
    /// The mother figure, if given.
    pub mother: Option<Somebody>,
    /// Source for the relation. Always empty in the current `.gw` grammar, which has no
    /// syntax for it; kept because GeneWeb's own record has the field.
    pub sources: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_round_trip_in_both_forms() {
        for keyword in RelationType::KEYWORDS {
            let bare = RelationType::from_keyword(keyword).expect("bare form");
            let colon = RelationType::from_keyword(&format!("{keyword}:")).expect("colon form");
            assert_eq!(bare, colon);
            assert_eq!(bare.keyword(), *keyword);
        }
        assert_eq!(RelationType::from_keyword("nope"), None);
    }
}
