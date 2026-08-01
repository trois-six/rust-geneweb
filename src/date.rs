//! The `.gw` date grammar.
//!
//! Port of `date_of_string` from GeneWeb's `bin/gwc/gwcomp.ml`.
//!
//! A date is `[precision] year[/month[/year]] [alternative] [calendar]`, where the
//! components shift meaning as slashes are added: `1850` is a year, `5/1850` is a month
//! and a year, and `12/5/1850` is a day, a month and a year. `0` alone means "unknown",
//! and `0(free text)` carries a date GeneWeb could not structure.
//!
//! Dates are kept **as written**. GeneWeb converts Julian, French Republican and Hebrew
//! dates to Gregorian on import and back again on export; storing the original values
//! alongside the calendar tag is both lossless and exactly what GEDCOM's
//! `@#DJULIAN@`-style escapes expect, so no calendrical arithmetic is needed here.

use crate::error::GwErrorKind;
use crate::lexer::{copy_decode, cut_space};

/// The calendar a date is expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Calendar {
    /// Gregorian, the default; written with an optional `G` suffix.
    #[default]
    Gregorian,
    /// Julian, written with a `J` suffix.
    Julian,
    /// French Republican, written with an `F` suffix. Uses a 13th month for the
    /// complementary days.
    French,
    /// Hebrew, written with an `H` suffix. Uses a 13th month in leap years.
    Hebrew,
}

/// The secondary date of an alternative or an interval.
///
/// Any component may be `0`, meaning "not given at this level of detail".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Dmy2 {
    /// Day of month, or `0`.
    pub day: u8,
    /// Month, `1..=13`, or `0`.
    pub month: u8,
    /// Year, possibly negative.
    pub year: i32,
}

/// How precisely a date is known.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Precision {
    /// Exact. No prefix.
    #[default]
    Sure,
    /// Approximate, written `~`.
    About,
    /// Uncertain, written `?`.
    Maybe,
    /// No later than this date, written `<`.
    Before,
    /// No earlier than this date, written `>`.
    After,
    /// One of two dates, written `1850|1851`.
    OrYear(Dmy2),
    /// Somewhere in a range, written `1850..1860`.
    YearInt(Dmy2),
}

/// A structured `.gw` date.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dmy {
    /// Day of month, or `0` when the date is only known to the month or the year.
    pub day: u8,
    /// Month, `1..=13`, or `0` when the date is only known to the year.
    pub month: u8,
    /// Year, possibly negative.
    pub year: i32,
    /// How precisely the date is known.
    pub precision: Precision,
}

/// A date as it appears in a `.gw` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GwDate {
    /// A structured date in a given calendar.
    Structured {
        /// The date components, as written.
        dmy: Dmy,
        /// The calendar the components belong to.
        calendar: Calendar,
    },
    /// A date GeneWeb could not structure, written `0(free text)`.
    Text(String),
}

/// The characters that may open a date, used to decide whether an optional date is
/// present at the cursor.
///
/// `!` is deliberately absent: it introduces a *baptism* date, and its presence means the
/// birth date slot is empty. See `get_optional_birthdate`.
#[must_use]
pub fn starts_date(token: &str) -> bool {
    matches!(
        token.as_bytes().first(),
        Some(b'~' | b'?' | b'<' | b'>' | b'-' | b'0'..=b'9')
    )
}

/// Returns `true` when `token` is a baptism date, i.e. begins with `!`.
#[must_use]
pub fn starts_baptism_date(token: &str) -> bool {
    token.as_bytes().first() == Some(&b'!')
}

fn invalid(s: &str) -> GwErrorKind {
    GwErrorKind::InvalidDate(s.to_owned())
}
/// Reads an optionally signed integer, as `champ` does.
///
/// Returns the value and the offset just past it. Accumulation saturates rather than
/// overflowing, so a pathologically long digit run yields an out-of-range year that the
/// caller rejects instead of panicking.
fn read_int(bytes: &[u8], mut pos: usize) -> (i64, usize) {
    let negative = if bytes.get(pos) == Some(&b'-') {
        pos += 1;
        true
    } else {
        false
    };
    let mut value: i64 = 0;
    while let Some(digit) = bytes.get(pos) {
        if !digit.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(digit - b'0'));
        pos += 1;
    }
    (if negative { -value } else { value }, pos)
}

fn skip_slash(bytes: &[u8], pos: usize) -> Option<usize> {
    (bytes.get(pos) == Some(&b'/')).then_some(pos + 1)
}

fn to_year(value: i64, src: &str) -> Result<i32, GwErrorKind> {
    i32::try_from(value).map_err(|_| invalid(src))
}

fn to_month(value: i64, src: &str) -> Result<u8, GwErrorKind> {
    // GeneWeb allows 13 months: the French Republican complementary days, and the Hebrew
    // leap-year intercalation.
    u8::try_from(value)
        .ok()
        .filter(|month| (1..=13).contains(month))
        .ok_or_else(|| invalid(src))
}

fn to_day(value: i64, src: &str) -> Result<u8, GwErrorKind> {
    u8::try_from(value)
        .ok()
        .filter(|day| (1..=31).contains(day))
        .ok_or_else(|| invalid(src))
}

/// Reads the precision prefix, if there is one.
fn read_precision(bytes: &[u8], start: usize) -> (Precision, usize) {
    match bytes.get(start) {
        Some(b'~') => (Precision::About, start + 1),
        Some(b'?') => (Precision::Maybe, start + 1),
        Some(b'>') => (Precision::After, start + 1),
        Some(b'<') => (Precision::Before, start + 1),
        _ => (Precision::Sure, start),
    }
}

/// Reads the secondary date of a `|` alternative or a `..` interval.
///
/// `first` is the number already read; how it is interpreted depends on how many slashes
/// follow it, exactly as in the primary date.
fn read_alternative(
    bytes: &[u8],
    first: i64,
    pos: usize,
    src: &str,
) -> Result<(Dmy2, usize), GwErrorKind> {
    let Some(pos) = skip_slash(bytes, pos) else {
        return Ok((
            Dmy2 {
                day: 0,
                month: 0,
                year: to_year(first, src)?,
            },
            pos,
        ));
    };
    let (second, pos) = read_int(bytes, pos);
    let Some(pos) = skip_slash(bytes, pos) else {
        // month/year
        return Ok((
            Dmy2 {
                day: 0,
                month: to_month(first, src)?,
                year: to_year(second, src)?,
            },
            pos,
        ));
    };
    // day/month/year
    let (third, pos) = read_int(bytes, pos);
    Ok((
        Dmy2 {
            day: to_day(first, src)?,
            month: to_month(second, src)?,
            year: to_year(third, src)?,
        },
        pos,
    ))
}

/// Reads the `0(free text)` form.
///
/// Returns `Ok(None)` for a bare `0`, which says the date is simply unknown.
fn read_text_date(src: &str, bytes: &[u8], pos: usize) -> Result<Option<GwDate>, GwErrorKind> {
    if pos == bytes.len() {
        return Ok(None);
    }
    if bytes[pos] != b'(' || bytes[bytes.len() - 1] != b')' {
        return Err(invalid(src));
    }
    let inner = &src[pos + 1..bytes.len() - 1];
    // Decoded a second time on purpose: the token already went through `fields`, and
    // GeneWeb applies `copy_decode` again here, so `\_` inside a text date survives the
    // first pass to become a space in the second.
    Ok(Some(GwDate::Text(copy_decode(cut_space(inner).as_bytes()))))
}

/// Reads the day, month and year, whose meanings shift with the number of slashes.
///
/// `Ok(None)` means the date is explicitly unknown rather than malformed.
fn read_components(
    bytes: &[u8],
    first: i64,
    pos: usize,
    precision: Precision,
    src: &str,
) -> Result<Option<(Dmy, usize)>, GwErrorKind> {
    let Some(pos) = skip_slash(bytes, pos) else {
        // Bare year.
        return Ok(Some((
            Dmy {
                day: 0,
                month: 0,
                year: to_year(first, src)?,
                precision,
            },
            pos,
        )));
    };
    let (second, pos) = read_int(bytes, pos);
    let Some(pos) = skip_slash(bytes, pos) else {
        // month/year, where a zero year means the whole date is unknown.
        if second == 0 {
            return Ok(None);
        }
        return Ok(Some((
            Dmy {
                day: 0,
                month: to_month(first, src)?,
                year: to_year(second, src)?,
                precision,
            },
            pos,
        )));
    };
    // day/month/year
    let (third, pos) = read_int(bytes, pos);
    Ok(Some((
        Dmy {
            // Order matters: GeneWeb validates the month before the day.
            month: to_month(second, src)?,
            day: to_day(first, src)?,
            year: to_year(third, src)?,
            precision,
        },
        pos,
    )))
}

/// Reads the `|` alternative or `..` interval that may follow a date.
fn read_range(
    bytes: &[u8],
    pos: usize,
    src: &str,
) -> Result<Option<(Precision, usize)>, GwErrorKind> {
    match bytes.get(pos) {
        Some(b'|') => {
            let (first, next) = read_int(bytes, pos + 1);
            let (alternative, next) = read_alternative(bytes, first, next, src)?;
            Ok(Some((Precision::OrYear(alternative), next)))
        }
        Some(b'.') if bytes.get(pos + 1) == Some(&b'.') => {
            let (first, next) = read_int(bytes, pos + 2);
            let (alternative, next) = read_alternative(bytes, first, next, src)?;
            Ok(Some((Precision::YearInt(alternative), next)))
        }
        _ => Ok(None),
    }
}

/// Reads the trailing calendar letter, if there is one.
fn read_calendar(bytes: &[u8], pos: usize) -> (Calendar, usize) {
    match bytes.get(pos) {
        Some(b'G') => (Calendar::Gregorian, pos + 1),
        Some(b'J') => (Calendar::Julian, pos + 1),
        Some(b'F') => (Calendar::French, pos + 1),
        Some(b'H') => (Calendar::Hebrew, pos + 1),
        // Gregorian is the default and needs no letter.
        _ => (Calendar::Gregorian, pos),
    }
}

/// Parses a date from the whole of `s`.
///
/// # Errors
///
/// Returns [`GwErrorKind::InvalidDate`] when `s` is not a well-formed `.gw` date.
pub fn parse(s: &str) -> Result<Option<GwDate>, GwErrorKind> {
    parse_from(s, 0)
}

/// Parses a date from `s`, starting at byte offset `start`.
///
/// `Ok(None)` means the field was explicitly unknown — a bare `0`, or a `month/0` whose
/// year is zero — which is different from a malformed date.
///
/// # Errors
///
/// Returns [`GwErrorKind::InvalidDate`] when the text from `start` is not a well-formed
/// `.gw` date, including when it has trailing characters.
pub fn parse_from(s: &str, start: usize) -> Result<Option<GwDate>, GwErrorKind> {
    let bytes = s.as_bytes();
    if start >= bytes.len() {
        return Err(invalid(s));
    }

    let (precision, pos) = read_precision(bytes, start);
    let (first, after_first) = read_int(bytes, pos);
    // A lone `0` is the explicit "unknown" marker, and also introduces a text date.
    let undefined = after_first == pos + 1 && bytes.get(pos) == Some(&b'0');

    if undefined && skip_slash(bytes, after_first).is_none() {
        return read_text_date(s, bytes, after_first);
    }

    let Some((mut dmy, mut pos)) = read_components(bytes, first, after_first, precision, s)? else {
        return Ok(None);
    };

    if let Some((precision, next)) = read_range(bytes, pos, s)? {
        dmy.precision = precision;
        pos = next;
    }

    let (calendar, pos) = read_calendar(bytes, pos);
    if pos == bytes.len() {
        Ok(Some(GwDate::Structured { dmy, calendar }))
    } else {
        Err(invalid(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dmy(s: &str) -> Dmy {
        match parse(s) {
            Ok(Some(GwDate::Structured { dmy, .. })) => dmy,
            other => panic!("expected a structured date for {s:?}, got {other:?}"),
        }
    }

    fn calendar(s: &str) -> Calendar {
        match parse(s) {
            Ok(Some(GwDate::Structured { calendar, .. })) => calendar,
            other => panic!("expected a structured date for {s:?}, got {other:?}"),
        }
    }

    #[test]
    fn components_shift_as_slashes_are_added() {
        assert_eq!(
            dmy("1850"),
            Dmy {
                day: 0,
                month: 0,
                year: 1850,
                precision: Precision::Sure
            }
        );
        assert_eq!(
            dmy("5/1850"),
            Dmy {
                day: 0,
                month: 5,
                year: 1850,
                precision: Precision::Sure
            }
        );
        assert_eq!(
            dmy("12/5/1850"),
            Dmy {
                day: 12,
                month: 5,
                year: 1850,
                precision: Precision::Sure
            }
        );
    }

    #[test]
    fn precision_prefixes() {
        assert_eq!(dmy("~1850").precision, Precision::About);
        assert_eq!(dmy("?1850").precision, Precision::Maybe);
        assert_eq!(dmy("<1849").precision, Precision::Before);
        assert_eq!(dmy(">1900").precision, Precision::After);
    }

    #[test]
    fn alternatives_and_intervals() {
        assert_eq!(
            dmy("1850|1851").precision,
            Precision::OrYear(Dmy2 {
                day: 0,
                month: 0,
                year: 1851
            })
        );
        assert_eq!(
            dmy("1850..1860").precision,
            Precision::YearInt(Dmy2 {
                day: 0,
                month: 0,
                year: 1860
            })
        );
        assert_eq!(
            dmy("1850..3/6/1860").precision,
            Precision::YearInt(Dmy2 {
                day: 3,
                month: 6,
                year: 1860
            })
        );
    }

    #[test]
    fn calendar_suffixes() {
        assert_eq!(calendar("12/5/1750"), Calendar::Gregorian);
        assert_eq!(calendar("12/5/1750G"), Calendar::Gregorian);
        assert_eq!(calendar("12/5/1750J"), Calendar::Julian);
        assert_eq!(calendar("12/13/8F"), Calendar::French);
        assert_eq!(calendar("12/5/5750H"), Calendar::Hebrew);
    }

    #[test]
    fn thirteenth_month_is_valid() {
        // Jour complémentaire in the French Republican calendar.
        assert_eq!(dmy("3/13/8F").month, 13);
        assert!(parse("3/14/8F").is_err());
    }

    #[test]
    fn zero_means_unknown_not_malformed() {
        assert_eq!(parse("0").unwrap(), None);
        // A month with a zero year is likewise unknown.
        assert_eq!(parse("5/0").unwrap(), None);
    }

    #[test]
    fn text_dates() {
        assert_eq!(
            parse("0(5 Mai 1990)").unwrap(),
            Some(GwDate::Text("5 Mai 1990".into()))
        );
        // The token arrives already decoded once; `_` here came from `\_` in the file and
        // is decoded a second time, as GeneWeb does.
        assert_eq!(
            parse("0(vers_1850)").unwrap(),
            Some(GwDate::Text("vers 1850".into()))
        );
    }

    #[test]
    fn negative_years() {
        assert_eq!(dmy("-52").year, -52);
        assert_eq!(dmy("15/3/-44").year, -44);
    }

    #[test]
    fn malformed_dates_are_errors_not_panics() {
        for s in ["", "12/32/1850", "12/0/1850", "1850X", "0abc", "abc", "0("] {
            assert!(parse(s).is_err(), "{s:?} should not parse");
        }
    }

    #[test]
    fn a_bare_precision_prefix_yields_year_zero() {
        // Not a typo in the test: `champ` reads no digits and returns 0 without
        // consuming, so GeneWeb accepts `~` as "about year 0". Reproduced deliberately —
        // a stricter reader would reject files `gwc` accepts.
        assert_eq!(
            dmy("~"),
            Dmy {
                day: 0,
                month: 0,
                year: 0,
                precision: Precision::About
            }
        );
    }

    #[test]
    fn absurd_input_saturates_instead_of_overflowing() {
        let huge = "9".repeat(64);
        assert!(parse(&huge).is_err());
    }

    #[test]
    fn recognises_date_openers() {
        assert!(starts_date("1850"));
        assert!(starts_date("~1850"));
        assert!(starts_date("-52"));
        assert!(!starts_date("!1850"));
        assert!(!starts_date("#bp"));
        assert!(!starts_date(""));
        assert!(starts_baptism_date("!1850"));
    }
}
