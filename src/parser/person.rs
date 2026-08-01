//! Parsing a person from a line of tokens.
//!
//! Port of `set_infos`, `parse_parent`, `parse_child` and the `get_*` helpers they use.
//!
//! # The order is the grammar
//!
//! There is no keyword that says "here comes the occupation". A person's fields are
//! recognised by *position*: each helper looks at the head of the remaining tokens, takes
//! it if it matches, and leaves the cursor alone if it does not. Consuming the fields in a
//! different order does not produce a different parse — it produces a wrong one, silently.
//! [`set_infos`] therefore calls them in exactly the sequence `set_infos` does, and the
//! sequence is asserted by the `field_order_is_positional` test below.

use crate::date::{self, GwDate};
use crate::error::{GwError, GwErrorKind, Result};
use crate::lexer::{cut_space, Cursor};
use crate::model::key::{Key, Somebody};
use crate::model::person::{Access, Burial, Death, DeathReason, Person, Sex};
use crate::model::title::{Title, TitleName};

/// Where a line came from, for error reporting.
#[derive(Debug, Clone, Copy)]
pub struct LineCtx<'a> {
    /// 1-based line number.
    pub no: usize,
    /// The line as read.
    pub raw: &'a str,
}

impl LineCtx<'_> {
    /// Builds an error anchored at this line.
    #[must_use]
    pub fn err(self, kind: GwErrorKind) -> GwError {
        GwError::at(self.no, self.raw, kind)
    }

    /// Builds a syntax error anchored at this line.
    pub fn syntax(self, what: impl Into<String>) -> GwError {
        self.err(GwErrorKind::Syntax(what.into()))
    }
}

/// Who a person is, as read from the head of the line before their fields.
#[derive(Debug, Clone, Copy)]
pub struct Identity<'a> {
    /// Given name, with the `.n` occurrence suffix stripped.
    pub first_name: &'a str,
    /// Family name.
    pub surname: &'a str,
    /// Occurrence number disambiguating homonyms.
    pub occ: u32,
    /// Sex, where the surrounding block states one.
    pub sex: Sex,
}

impl<'a> Identity<'a> {
    /// An identity with no recorded sex, which is what a parent or witness starts as.
    #[must_use]
    pub fn new(first_name: &'a str, surname: &'a str, occ: u32) -> Self {
        Self {
            first_name,
            surname,
            occ,
            sex: Sex::Neuter,
        }
    }
}

/// Values a family supplies to a child who declares none of their own.
///
/// These come from the family's `csrc` and `cbp` lines and apply to children only; a
/// parent or a witness carries no defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct FamilyDefaults<'a> {
    /// Default source, from `csrc`.
    pub sources: &'a str,
    /// Default birth place, from `cbp`.
    pub birth_place: &'a str,
}

/// Whether a byte may begin a name.
///
/// The two high ranges are GeneWeb's, and were written for ISO-8859-1 accented letters.
/// They keep working after the UTF-8 conversion because the lead byte of a two- or
/// three-byte sequence falls inside them: `é` is `C3 A9`, and `C3` is in `C0..=DD`.
fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, 0xC0..=0xDD | 0xE0..=0xFF | b'?' | b' ')
}

fn first_byte(token: &str) -> Option<u8> {
    token.as_bytes().first().copied()
}

/// Reads a surname, as `get_name` does.
///
/// Returns the empty string without consuming when the head token cannot be a surname —
/// which is how an omitted surname is detected.
fn get_name(cur: &mut Cursor<'_>) -> String {
    let Some(token) = cur.peek() else {
        return String::new();
    };
    if token == "#nick" || token == "#alias" {
        return String::new();
    }
    match first_byte(token) {
        Some(b'{') | None => String::new(),
        Some(b) if is_name_start(b) => {
            cur.advance();
            cut_space(token).to_owned()
        }
        Some(_) => String::new(),
    }
}

/// Parses a run of digits covering the whole of `s`, as `make_int` does.
///
/// Returns `None` unless every character is a digit and there is at least one.
fn make_int(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Reads a given name and its occurrence number, as `get_fst_name` does.
///
/// The occurrence number is the `.n` suffix. A trailing dot with no digits, or a dot
/// followed by anything but digits, is part of the name.
///
/// # Errors
///
/// Returns an error when the head token cannot begin a name.
fn get_first_name(cur: &mut Cursor<'_>, ctx: LineCtx<'_>) -> Result<(String, u32)> {
    let token = cur
        .peek()
        .ok_or_else(|| ctx.syntax("expected a given name"))?;
    let valid = match first_byte(token) {
        Some(b'[') => true,
        Some(b) => is_name_start(b),
        None => false,
    };
    if !valid {
        return Err(ctx.syntax(format!("`{token}` cannot begin a given name")));
    }
    cur.advance();
    let name = cut_space(token);
    match name.rfind('.') {
        Some(i) => match make_int(&name[i + 1..]) {
            Some(occ) => Ok((name[..i].to_owned(), occ)),
            None => Ok((name.to_owned(), 0)),
        },
        None => Ok((name.to_owned(), 0)),
    }
}

/// Reads `{Alias}` given-name aliases.
fn get_first_name_aliases(cur: &mut Cursor<'_>) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(token) = cur.peek() {
        let bytes = token.as_bytes();
        if bytes.len() >= 2 && bytes[0] == b'{' && bytes[bytes.len() - 1] == b'}' {
            cur.advance();
            out.push(cut_space(&token[1..token.len() - 1]).to_owned());
        } else {
            break;
        }
    }
    out
}

/// Reads a repeated `tag value` pair into a list.
fn get_repeated(cur: &mut Cursor<'_>, tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        let value = cur.field(tag);
        // `field` only returns empty when it did not match, so this cannot loop forever
        // on an empty value: an empty value means the tag was absent.
        if value.is_empty() && cur.peek() != Some(tag) {
            break;
        }
        out.push(value);
    }
    out
}

/// Reads a `(Public Name)`.
fn get_public_name(cur: &mut Cursor<'_>) -> String {
    let Some(token) = cur.peek() else {
        return String::new();
    };
    let bytes = token.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'(' && bytes[bytes.len() - 1] == b')' {
        cur.advance();
        cut_space(&token[1..token.len() - 1]).to_owned()
    } else {
        String::new()
    }
}

/// Reads a portrait path, written `#image` or the older `#photo`.
fn get_image(cur: &mut Cursor<'_>) -> String {
    let image = cur.field("#image");
    if image.is_empty() && cur.peek() != Some("#image") {
        cur.field("#photo")
    } else {
        image
    }
}

/// Reads a person's visibility, as `get_access` does.
///
/// GeneWeb additionally consults the base's `.auth` file and, under RGPD mode, a
/// directory of consent documents, to *promote* a person to semi-public. Both need a
/// served base rather than a file, so neither applies here: this reads only what the
/// `.gw` itself states.
fn get_access(cur: &mut Cursor<'_>) -> Access {
    match cur.eat_any(&["#apubl", "#apriv", "#semipub", "#afriend"]) {
        Some("#apubl") => Access::Public,
        Some("#apriv") => Access::Private,
        // `#afriend` is the retired spelling of `#semipub`.
        Some("#semipub" | "#afriend") => Access::SemiPublic,
        _ => Access::IfTitles,
    }
}

/// Splits one `:`-delimited title field, honouring `\` escapes.
///
/// Returns the field and the offset just past its terminator. Mirrors `next_field`.
fn title_field(t: &str, start: usize) -> (String, usize) {
    let bytes = t.as_bytes();
    let mut out = String::new();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b':' => return (out, i + 1),
            b'\\' if i + 1 < bytes.len() => {
                // Escapes are byte-wise in GeneWeb; copy the escaped byte's whole
                // character so that `\é` does not split a UTF-8 sequence.
                let rest = &t[i + 1..];
                if let Some(c) = rest.chars().next() {
                    out.push(c);
                    i += 1 + c.len_utf8();
                } else {
                    i += 1;
                }
            }
            _ => {
                let rest = &t[i..];
                if let Some(c) = rest.chars().next() {
                    out.push(c);
                    i += c.len_utf8();
                } else {
                    i += 1;
                }
            }
        }
    }
    (out, i)
}

/// Parses the inside of a `[...]` title, as `scan_title` does.
///
/// All six fields must be present: GeneWeb requires five colons and rejects the rest.
///
/// # Errors
///
/// Returns [`GwErrorKind::InvalidTitle`] when the shape is wrong, and
/// [`GwErrorKind::InvalidDate`] when a date field is malformed.
fn scan_title(t: &str, ctx: LineCtx<'_>) -> Result<Title> {
    let bad = || ctx.err(GwErrorKind::InvalidTitle(t.to_owned()));

    let (name, i) = title_field(t, 0);
    if i == t.len() {
        return Err(bad());
    }
    let name = match name.as_str() {
        "" => TitleName::None,
        "*" => TitleName::Main,
        other => TitleName::Name(other.to_owned()),
    };

    let (ident, i) = title_field(t, i);
    // The ident must have been terminated by a colon, not by the end of the string.
    if i == 0 || t.as_bytes().get(i - 1) != Some(&b':') {
        return Err(bad());
    }

    let (place, i) = title_field(t, i);
    let (start, i) = title_field(t, i);
    let (end, i) = title_field(t, i);
    let (nth, i) = title_field(t, i);
    if i != t.len() {
        return Err(bad());
    }

    let parse_date = |s: &str| -> Result<Option<GwDate>> {
        if s.is_empty() {
            Ok(None)
        } else {
            date::parse(s).map_err(|kind| ctx.err(kind))
        }
    };

    Ok(Title {
        name,
        ident,
        place,
        date_start: parse_date(&start)?,
        date_end: parse_date(&end)?,
        nth: if nth.is_empty() {
            0
        } else {
            nth.parse().map_err(|_| bad())?
        },
    })
}

/// Reads a run of `[...]` titles.
///
/// A title with an empty ident is dropped, as GeneWeb drops it.
fn get_titles(cur: &mut Cursor<'_>, ctx: LineCtx<'_>) -> Result<Vec<Title>> {
    let mut out = Vec::new();
    while let Some(token) = cur.peek() {
        let bytes = token.as_bytes();
        if bytes.len() >= 2 && bytes[0] == b'[' && bytes[bytes.len() - 1] == b']' {
            cur.advance();
            let title = scan_title(&token[1..token.len() - 1], ctx)?;
            if !title.ident.is_empty() {
                out.push(title);
            }
        } else {
            break;
        }
    }
    Ok(out)
}

/// What a date slot in a person's line held.
///
/// The distinction between the last two matters: a person with a *real* birth date and no
/// death field is taken to be living, whereas a birth written `0` says nothing at all. See
/// [`set_infos`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateField {
    /// No date at this position.
    Absent,
    /// A field that says the date is unknown, written `0`.
    Unknown,
    /// A usable date.
    Known(GwDate),
}

impl DateField {
    /// The date, if there was a usable one.
    #[must_use]
    pub fn into_date(self) -> Option<GwDate> {
        match self {
            Self::Known(date) => Some(date),
            Self::Absent | Self::Unknown => None,
        }
    }
}

/// Reads an optional date at the cursor.
///
/// `baptism` selects the `!`-prefixed form; a birth date and a baptism date share a slot
/// and are told apart by that prefix.
pub(crate) fn optional_date(
    cur: &mut Cursor<'_>,
    ctx: LineCtx<'_>,
    baptism: bool,
) -> Result<DateField> {
    let Some(token) = cur.peek() else {
        return Ok(DateField::Absent);
    };
    let body = if baptism {
        // A baptism date is a birth-shaped date behind a `!`.
        match token.strip_prefix('!') {
            Some(rest) => rest,
            None => return Ok(DateField::Absent),
        }
    } else {
        if date::starts_baptism_date(token) {
            return Ok(DateField::Absent);
        }
        token
    };
    if !date::starts_date(body) {
        return Ok(DateField::Absent);
    }
    cur.advance();
    let offset = token.len() - body.len();
    Ok(
        match date::parse_from(token, offset).map_err(|kind| ctx.err(kind))? {
            Some(date) => DateField::Known(date),
            None => DateField::Unknown,
        },
    )
}

/// Reads a death field, as `get_optional_deathdate` does.
///
/// `Ok(None)` means there was no death field at all, which is what drives the
/// alive/dead inference in [`set_infos`].
fn get_death(cur: &mut Cursor<'_>, ctx: LineCtx<'_>) -> Result<Option<Death>> {
    let Some(token) = cur.peek() else {
        return Ok(None);
    };
    match token {
        "?" => {
            cur.advance();
            return Ok(Some(Death::DontKnowIfDead));
        }
        "mj" => {
            cur.advance();
            return Ok(Some(Death::DeadYoung));
        }
        "od" => {
            cur.advance();
            return Ok(Some(Death::OfCourseDead));
        }
        _ => {}
    }

    let (reason, offset) = match first_byte(token) {
        Some(b'k') => (DeathReason::Killed, 1),
        Some(b'm') => (DeathReason::Murdered, 1),
        Some(b'e') => (DeathReason::Executed, 1),
        Some(b's') => (DeathReason::Disappeared, 1),
        _ => (DeathReason::Unspecified, 0),
    };
    // A bare `k` with no date is not a death field.
    if offset >= token.len() || !date::starts_date(&token[offset..]) {
        return Ok(None);
    }
    cur.advance();
    let death = match date::parse_from(token, offset).map_err(|kind| ctx.err(kind))? {
        Some(date) => Death::Dead { reason, date },
        None => Death::DeadDontKnowWhen,
    };
    Ok(Some(death))
}

/// Reads a burial field, as `get_burial` does.
fn get_burial(cur: &mut Cursor<'_>, ctx: LineCtx<'_>) -> Result<Burial> {
    let Some(tag) = cur.eat_any(&["#buri", "#crem"]) else {
        return Ok(Burial::Unknown);
    };
    let mut date = None;
    if let Some(token) = cur.peek() {
        if date::starts_date(token) {
            cur.advance();
            date = date::parse(token).map_err(|kind| ctx.err(kind))?;
        }
    }
    Ok(if tag == "#buri" {
        Burial::Buried(date)
    } else {
        Burial::Cremated(date)
    })
}

/// Reads an optional leading sex marker, as `get_optional_sexe` does.
///
/// The marker only counts when at least one token follows it, so that a child line whose
/// given name happens to be `m` is not misread as a sex marker with no name.
pub(crate) fn get_optional_sex(cur: &mut Cursor<'_>) -> Sex {
    let (sex, token) = match cur.peek() {
        Some("h" | "m") => (Sex::Male, true),
        Some("f") => (Sex::Female, true),
        _ => (Sex::Neuter, false),
    };
    if token && cur.peek_at(1).is_some() {
        cur.advance();
        sex
    } else {
        Sex::Neuter
    }
}

/// Fills in a person's fields from the remaining tokens of a line.
///
/// `common_sources` and `common_birth_place` are a family's `csrc` and `cbp` defaults,
/// applied only when the person declares none of their own.
///
/// # Errors
///
/// Returns an error when a title or a date is malformed.
pub fn set_infos(
    identity: Identity<'_>,
    defaults: FamilyDefaults<'_>,
    cur: &mut Cursor<'_>,
    ctx: LineCtx<'_>,
) -> Result<Person> {
    let Identity {
        first_name,
        surname,
        occ,
        sex,
    } = identity;
    let FamilyDefaults {
        sources: common_sources,
        birth_place: common_birth_place,
    } = defaults;
    // Order is load-bearing; see the module documentation.
    let first_names_aliases = get_first_name_aliases(cur);
    let surnames_aliases = get_repeated(cur, "#salias");
    let public_name = get_public_name(cur);
    let image = get_image(cur);
    let qualifiers = get_repeated(cur, "#nick");
    let aliases = get_repeated(cur, "#alias");
    let titles = get_titles(cur, ctx)?;
    let access = get_access(cur);
    let occupation = cur.field("#occu");
    let sources = cur.field("#src");

    let birth = optional_date(cur, ctx, false)?;
    let birth_place = cur.field("#bp");
    let birth_note = cur.field("#bn");
    let birth_src = cur.field("#bs");

    let baptism = optional_date(cur, ctx, true)?;
    let baptism_place = {
        let pp = cur.field("#pp");
        // GeneWeb re-reads `#bp` here, commented "if no baptism place then it's equals to
        // birth place". The fallback almost never fires: `#bp` was already consumed
        // above, and `gwu` always writes the baptism place as `#pp`, never as a second
        // `#bp`. Reproduced as written rather than as intended — a real fallback would
        // invent a baptism place that `gwc` does not record.
        if pp.is_empty() {
            cur.field("#bp")
        } else {
            pp
        }
    };
    let baptism_note = cur.field("#pn");
    let baptism_src = cur.field("#ps");

    let death_field = get_death(cur, ctx)?;
    let death_place = cur.field("#dp");
    let death_note = cur.field("#dn");
    let death_src = cur.field("#ds");

    // A person with a real birth date and no death field at all is taken to be living.
    // A birth date of `0` does not count: it carries no information to reason from.
    let death = match (&birth, death_field) {
        (_, Some(death)) => death,
        (DateField::Known(_), None) => Death::NotDead,
        (DateField::Absent | DateField::Unknown, None) => Death::DontKnowIfDead,
    };

    let burial = get_burial(cur, ctx)?;
    let burial_place = cur.field("#rp");
    let burial_note = cur.field("#rn");
    let burial_src = cur.field("#rs");

    Ok(Person {
        first_name: first_name.to_owned(),
        surname: surname.to_owned(),
        occ,
        sex,
        first_names_aliases,
        surnames_aliases,
        public_name,
        image,
        qualifiers,
        aliases,
        titles,
        access,
        occupation,
        sources: if sources.is_empty() {
            common_sources.to_owned()
        } else {
            sources
        },
        birth: birth.into_date(),
        birth_place: if birth_place.is_empty() {
            common_birth_place.to_owned()
        } else {
            birth_place
        },
        birth_note,
        birth_src,
        baptism: baptism.into_date(),
        baptism_place,
        baptism_note,
        baptism_src,
        death,
        death_place,
        death_note,
        death_src,
        burial,
        burial_place,
        burial_note,
        burial_src,
        notes: String::new(),
        events: Vec::new(),
        relations: Vec::new(),
    })
}

/// Parses a person in a position that admits either a definition or a reference.
///
/// Used for parents on a `fam` line, witnesses, and the subjects of `rel` and `pevt`
/// blocks.
///
/// Whether this is a definition is decided by what *follows*: a bare name followed by
/// nothing, or by the `+` that opens the marriage, is a reference to a person defined
/// elsewhere. Anything else means details follow, so it is a definition. The anonymous
/// `? ?` is always a definition, since there is nothing to refer to.
///
/// Returns the person and their surname, which a family line reuses as the default
/// surname for children.
///
/// # Errors
///
/// Returns an error when the name cannot be parsed, or when a definition is malformed.
pub fn parse_parent(cur: &mut Cursor<'_>, ctx: LineCtx<'_>) -> Result<(Somebody, String)> {
    let surname = get_name(cur);
    let (first_name, occ) = get_first_name(cur, ctx)?;

    let anonymous = first_name == "?" || surname == "?";
    let defined = anonymous
        || match cur.peek() {
            None => false,
            Some(token) => first_byte(token) != Some(b'+'),
        };

    if !defined {
        let key = Key::new(&first_name, &surname, occ);
        return Ok((Somebody::Reference(key), surname));
    }
    let person = set_infos(
        Identity::new(&first_name, &surname, occ),
        FamilyDefaults::default(),
        cur,
        ctx,
    )?;
    Ok((Somebody::Definition(Box::new(person)), surname))
}

/// Parses a child line inside a family's `beg`…`end` block.
///
/// A child may omit their surname, in which case they take the father's. Whether the
/// surname was omitted is decided by looking at what comes next: a date, a tag or a
/// bracket means the surname slot was skipped.
///
/// # Errors
///
/// Returns an error when the name or any field is malformed.
pub fn parse_child(
    cur: &mut Cursor<'_>,
    father_surname: &str,
    sex: Sex,
    defaults: FamilyDefaults<'_>,
    ctx: LineCtx<'_>,
) -> Result<Person> {
    let (first_name, occ) = get_first_name(cur, ctx)?;

    let surname = match cur.peek() {
        // An explicit `?` means the surname is unknown, not inherited.
        Some("?") => get_name(cur),
        Some(token) => match first_byte(token) {
            Some(b'<' | b'>' | b'!' | b'~' | b'?' | b'-' | b'0'..=b'9' | b'{' | b'#') => {
                father_surname.to_owned()
            }
            // A public name or a title may follow a bare given name, but only inherits
            // the surname when there actually is a given name.
            Some(b'(' | b'[') => {
                if first_name.is_empty() {
                    String::new()
                } else {
                    father_surname.to_owned()
                }
            }
            _ => get_name(cur),
        },
        None => father_surname.to_owned(),
    };

    set_infos(
        Identity {
            sex,
            ..Identity::new(&first_name, &surname, occ)
        },
        defaults,
        cur,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::{Calendar, Dmy, Precision};
    use crate::lexer::fields;

    fn ctx() -> LineCtx<'static> {
        LineCtx { no: 1, raw: "" }
    }

    fn person(line: &str) -> Person {
        let toks = fields(line);
        let mut cur = Cursor::new(&toks);
        let p = set_infos(
            Identity {
                sex: Sex::Male,
                ..Identity::new("Jean", "Dupont", 0)
            },
            FamilyDefaults::default(),
            &mut cur,
            ctx(),
        )
        .expect("should parse");
        assert!(cur.is_empty(), "unconsumed tokens: {:?}", cur.remaining());
        p
    }

    #[test]
    fn field_order_is_positional() {
        // Every optional field at once, in the only order GeneWeb accepts.
        let p = person(concat!(
            "{Jeannot} #salias Dupond (Le_Grand) #image p.jpg #nick Titi #alias Toto ",
            "[comte:comte:Paris:1850:1860:2] #apubl #occu Marchand #src Archives ",
            "7/9/1830 #bp Reims #bn nb #bs bs ",
            "!8/9/1830 #pp Rouen #pn pn #ps ps ",
            "12/5/1900 #dp Lyon #dn dn #ds ds ",
            "#buri 14/5/1900 #rp Lyon #rn rn #rs rs",
        ));
        assert_eq!(p.first_names_aliases, ["Jeannot"]);
        assert_eq!(p.surnames_aliases, ["Dupond"]);
        assert_eq!(p.public_name, "Le Grand");
        assert_eq!(p.image, "p.jpg");
        assert_eq!(p.qualifiers, ["Titi"]);
        assert_eq!(p.aliases, ["Toto"]);
        assert_eq!(p.titles.len(), 1);
        assert_eq!(p.access, Access::Public);
        assert_eq!(p.occupation, "Marchand");
        assert_eq!(p.sources, "Archives");
        assert_eq!(p.birth_place, "Reims");
        assert_eq!(p.birth_note, "nb");
        assert_eq!(p.baptism_place, "Rouen");
        assert_eq!(p.death_place, "Lyon");
        assert!(matches!(p.burial, Burial::Buried(Some(_))));
        assert_eq!(p.burial_src, "rs");
    }

    #[test]
    fn the_baptism_place_fallback_does_not_actually_fall_back() {
        // Pinning GeneWeb's behaviour, not its stated intent: `#bp` has already been
        // consumed by the birth place, so the "same as birth place" fallback finds
        // nothing and the baptism place stays empty.
        let p = person("1830 #bp Reims !1831");
        assert_eq!(p.birth_place, "Reims");
        assert_eq!(p.baptism_place, "");

        // It does fire on a hand-written line carrying a second `#bp`, which is the only
        // input for which the fallback was ever reachable.
        let p = person("1830 #bp Reims !1831 #bp Rouen");
        assert_eq!(p.baptism_place, "Rouen");
    }

    #[test]
    fn a_birth_date_without_a_death_field_means_alive() {
        assert_eq!(person("1950").death, Death::NotDead);
        // A birth of `0` carries no information, so nothing can be inferred.
        assert_eq!(person("0").death, Death::DontKnowIfDead);
        // No birth field at all.
        assert_eq!(person("").death, Death::DontKnowIfDead);
    }

    #[test]
    fn death_markers() {
        assert_eq!(person("1900 ?").death, Death::DontKnowIfDead);
        assert_eq!(person("1900 mj").death, Death::DeadYoung);
        assert_eq!(person("1900 od").death, Death::OfCourseDead);
        assert!(matches!(
            person("1900 k1916").death,
            Death::Dead {
                reason: DeathReason::Killed,
                ..
            }
        ));
        assert!(matches!(
            person("1900 s1916").death,
            Death::Dead {
                reason: DeathReason::Disappeared,
                ..
            }
        ));
        assert!(matches!(
            person("1900 1980").death,
            Death::Dead {
                reason: DeathReason::Unspecified,
                ..
            }
        ));
    }

    #[test]
    fn titles_need_all_six_fields() {
        let p = person("[*:duc:Bretagne:1450:1470:3]");
        let t = &p.titles[0];
        assert_eq!(t.name, TitleName::Main);
        assert_eq!(t.ident, "duc");
        assert_eq!(t.place, "Bretagne");
        assert_eq!(t.nth, 3);
        assert!(matches!(
            t.date_start,
            Some(GwDate::Structured {
                dmy: Dmy { year: 1450, .. },
                calendar: Calendar::Gregorian
            })
        ));

        // Empty slots are fine; missing colons are not.
        let p = person("[:duc::::]");
        assert_eq!(p.titles[0].name, TitleName::None);

        let toks = fields("[duc:Bretagne]");
        let mut cur = Cursor::new(&toks);
        assert!(set_infos(
            Identity::new("a", "b", 0),
            FamilyDefaults::default(),
            &mut cur,
            ctx()
        )
        .is_err());
    }

    #[test]
    fn a_title_with_an_empty_ident_is_dropped() {
        assert!(person("[nom:::::]").titles.is_empty());
    }

    #[test]
    fn escaping_a_colon_in_a_title_takes_two_backslashes() {
        // Escapes are resolved twice. `fields` runs first and turns `\\` into `\`, so a
        // file must write `\\:` for `scan_title` to see `\:` and keep the colon inside
        // the field.
        let p = person(r"[*:duc\\:roi:Bretagne:::]");
        assert_eq!(p.titles[0].ident, "duc:roi");

        // A single backslash is eaten by the tokenizer, so the colon really does split
        // the field — `roi` lands in the place slot.
        let p = person(r"[*:duc\:roi:::]");
        assert_eq!(p.titles[0].ident, "duc");
        assert_eq!(p.titles[0].place, "roi");
    }

    #[test]
    fn common_family_defaults_apply_only_when_absent() {
        let toks = fields("1830");
        let mut cur = Cursor::new(&toks);
        let p = set_infos(
            Identity {
                sex: Sex::Male,
                ..Identity::new("A", "B", 0)
            },
            FamilyDefaults {
                sources: "csrc",
                birth_place: "cbp",
            },
            &mut cur,
            ctx(),
        )
        .unwrap();
        assert_eq!(p.sources, "csrc");
        assert_eq!(p.birth_place, "cbp");

        let toks = fields("#src own 1830 #bp here");
        let mut cur = Cursor::new(&toks);
        let p = set_infos(
            Identity {
                sex: Sex::Male,
                ..Identity::new("A", "B", 0)
            },
            FamilyDefaults {
                sources: "csrc",
                birth_place: "cbp",
            },
            &mut cur,
            ctx(),
        )
        .unwrap();
        assert_eq!(p.sources, "own");
        assert_eq!(p.birth_place, "here");
    }

    #[test]
    fn occurrence_numbers() {
        let parse = |s: &str| {
            let toks = fields(s);
            let mut cur = Cursor::new(&toks);
            get_first_name(&mut cur, ctx()).unwrap()
        };
        assert_eq!(parse("Jean"), ("Jean".into(), 0));
        assert_eq!(parse("Jean.2"), ("Jean".into(), 2));
        // Not digits, so the dot is part of the name.
        assert_eq!(parse("Jean."), ("Jean.".into(), 0));
        assert_eq!(parse("St.Jean"), ("St.Jean".into(), 0));
    }

    #[test]
    fn a_parent_is_a_reference_when_nothing_but_the_marriage_follows() {
        let toks = fields("Dupont Jean +1850");
        let mut cur = Cursor::new(&toks);
        let (who, surname) = parse_parent(&mut cur, ctx()).unwrap();
        assert_eq!(surname, "Dupont");
        assert!(matches!(who, Somebody::Reference(_)));

        let toks = fields("Dupont Jean 1820 +1850");
        let mut cur = Cursor::new(&toks);
        let (who, _) = parse_parent(&mut cur, ctx()).unwrap();
        assert!(matches!(who, Somebody::Definition(_)));
    }

    #[test]
    fn the_anonymous_person_is_always_a_definition() {
        let toks = fields("? ?");
        let mut cur = Cursor::new(&toks);
        let (who, _) = parse_parent(&mut cur, ctx()).unwrap();
        assert!(matches!(who, Somebody::Definition(_)));
    }

    #[test]
    fn a_child_inherits_the_father_surname_unless_it_gives_one() {
        let child = |s: &str| {
            let toks = fields(s);
            let mut cur = Cursor::new(&toks);
            parse_child(
                &mut cur,
                "Galichet",
                Sex::Male,
                FamilyDefaults::default(),
                ctx(),
            )
            .unwrap()
        };
        // A date follows, so the surname slot was skipped.
        assert_eq!(child("Pierre 1814").surname, "Galichet");
        // A tag follows: likewise.
        assert_eq!(child("Pierre #occu Négociant").surname, "Galichet");
        // An explicit surname wins.
        assert_eq!(child("Pierre Martin 1814").surname, "Martin");
        // An explicit `?` means unknown, not inherited.
        assert_eq!(child("Pierre ? 1814").surname, "?");
        // Nothing at all follows.
        assert_eq!(child("Pierre").surname, "Galichet");
    }

    #[test]
    fn a_sex_marker_needs_something_after_it() {
        let toks = fields("m Pierre");
        let mut cur = Cursor::new(&toks);
        assert_eq!(get_optional_sex(&mut cur), Sex::Male);
        assert_eq!(cur.peek(), Some("Pierre"));

        // `m` alone is a given name, not a marker.
        let toks = fields("m");
        let mut cur = Cursor::new(&toks);
        assert_eq!(get_optional_sex(&mut cur), Sex::Neuter);
        assert_eq!(cur.peek(), Some("m"));
    }

    #[test]
    fn access_accepts_the_retired_spelling() {
        assert_eq!(person("#apriv").access, Access::Private);
        assert_eq!(person("#semipub").access, Access::SemiPublic);
        assert_eq!(person("#afriend").access, Access::SemiPublic);
        assert_eq!(person("").access, Access::IfTitles);
    }

    #[test]
    fn image_accepts_both_spellings() {
        assert_eq!(person("#image a.jpg").image, "a.jpg");
        assert_eq!(person("#photo b.jpg").image, "b.jpg");
    }

    #[test]
    fn repeated_tags_accumulate() {
        let p = person("#nick Titi #nick Toto #alias A #alias B");
        assert_eq!(p.qualifiers, ["Titi", "Toto"]);
        assert_eq!(p.aliases, ["A", "B"]);
    }

    #[test]
    fn precision_survives_into_the_person() {
        let p = person("<1849");
        assert!(matches!(
            p.birth,
            Some(GwDate::Structured {
                dmy: Dmy {
                    precision: Precision::Before,
                    year: 1849,
                    ..
                },
                ..
            })
        ));
    }
}
