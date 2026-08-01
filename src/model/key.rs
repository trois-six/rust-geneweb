//! Person identity and the reference/definition distinction.

use super::person::Person;

/// How a person is identified in a `.gw` file.
///
/// GeneWeb has no surrogate identifiers: a person *is* the triple of first name, surname
/// and occurrence number. The occurrence number disambiguates homonyms and is written as
/// a `.n` suffix on the first name (`Jean.2 Dupont`), defaulting to `0`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Key {
    /// Given name, with the `.n` occurrence suffix stripped.
    pub first_name: String,
    /// Family name.
    pub surname: String,
    /// Occurrence number distinguishing people who share a name.
    pub occ: u32,
}

impl Key {
    /// Builds a key.
    pub fn new(first_name: impl Into<String>, surname: impl Into<String>, occ: u32) -> Self {
        Self {
            first_name: first_name.into(),
            surname: surname.into(),
            occ,
        }
    }

    /// Whether this key denotes the anonymous person, written `? ?`.
    ///
    /// Anonymous people are never merged: each `? ?` is a distinct individual, so this
    /// key must not be used for lookup. See `bogus_def`.
    #[must_use]
    pub fn is_anonymous(&self) -> bool {
        self.first_name == "?" || self.surname == "?"
    }
}

/// A person appearing in a position that admits either a full definition or a
/// back-reference to one made elsewhere.
///
/// Which one it is depends on context rather than syntax: in a `fam` line a parent is a
/// reference when nothing follows it but the marriage marker, and a definition otherwise.
/// See `parse_parent`.
#[derive(Debug, Clone, PartialEq)]
pub enum Somebody {
    /// A back-reference to a person defined in another block.
    Reference(Key),
    /// A person defined inline, here.
    Definition(Box<Person>),
}

impl Somebody {
    /// The key identifying this person, whether referenced or defined.
    #[must_use]
    pub fn key(&self) -> Key {
        match self {
            Self::Reference(key) => key.clone(),
            Self::Definition(person) => Key::new(&person.first_name, &person.surname, person.occ),
        }
    }

    /// The inline definition, if this is one.
    #[must_use]
    pub fn definition(&self) -> Option<&Person> {
        match self {
            Self::Definition(person) => Some(person),
            Self::Reference(_) => None,
        }
    }
}
