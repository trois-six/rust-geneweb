//! Rendering `.gw` dates as GEDCOM date values.
//!
//! Follows `ged_date_dmy` in GeneWeb's `bin/gwb2ged/gwb2gedLib.ml`, so that this crate
//! and GeneWeb's own exporter produce the same strings.
//!
//! `ged_io` stores a date as an opaque string in GEDCOM syntax, so the whole job here is
//! formatting: precision keyword, calendar escape, zero-padded day, month *name*, year.

use std::fmt::Write as _;

use ged_io::types::date::Date;

use crate::date::{Calendar, Dmy, Dmy2, GwDate, Precision};

/// Gregorian and Julian month abbreviations.
const GREGORIAN_MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// French Republican month abbreviations. The thirteenth covers the complementary days.
const FRENCH_MONTHS: [&str; 13] = [
    "VEND", "BRUM", "FRIM", "NIVO", "PLUV", "VENT", "GERM", "FLOR", "PRAI", "MESS", "THER", "FRUC",
    "COMP",
];

/// Hebrew month abbreviations. The thirteenth is the leap-year intercalation.
const HEBREW_MONTHS: [&str; 13] = [
    "TSH", "CSH", "KSL", "TVT", "SHV", "ADR", "ADS", "NSN", "IYR", "SVN", "TMZ", "AAV", "ELL",
];

/// The GEDCOM escape that introduces a non-Gregorian date.
fn calendar_escape(calendar: Calendar) -> &'static str {
    match calendar {
        // Gregorian is the default and is written without an escape.
        Calendar::Gregorian => "",
        Calendar::Julian => "@#DJULIAN@ ",
        Calendar::French => "@#DFRENCH R@ ",
        Calendar::Hebrew => "@#DHEBREW@ ",
    }
}

/// The month abbreviation for a calendar, or `None` when the number has no name.
fn month_name(calendar: Calendar, month: u8) -> Option<&'static str> {
    let index = usize::from(month.checked_sub(1)?);
    match calendar {
        Calendar::Gregorian | Calendar::Julian => GREGORIAN_MONTHS.get(index).copied(),
        Calendar::French => FRENCH_MONTHS.get(index).copied(),
        Calendar::Hebrew => HEBREW_MONTHS.get(index).copied(),
    }
}

/// Renders a year, marking years before the common era.
fn year(value: i32) -> String {
    if value >= 0 {
        value.to_string()
    } else {
        format!("{} BCE", -value)
    }
}

/// Appends `day month year` in GEDCOM order, omitting components that are zero.
fn push_ymd(out: &mut String, calendar: Calendar, day: u8, month: u8, y: i32) {
    out.push_str(calendar_escape(calendar));
    if day != 0 {
        let _ = write!(out, "{day:02} ");
    }
    if month != 0 {
        match month_name(calendar, month) {
            Some(name) => out.push_str(name),
            // A thirteenth Gregorian month has no abbreviation. GeneWeb's exporter fails
            // here; emitting the number keeps the value rather than losing it.
            None => out.push_str(&month.to_string()),
        }
        out.push(' ');
    }
    out.push_str(&year(y));
}

/// The keyword that introduces a date of this precision.
fn precision_keyword(precision: &Precision) -> &'static str {
    match precision {
        Precision::Sure => "",
        Precision::About => "ABT ",
        // GEDCOM has no "possibly"; GeneWeb maps it to "estimated".
        Precision::Maybe => "EST ",
        Precision::Before => "BEF ",
        Precision::After => "AFT ",
        // Both an alternative and an interval become a GEDCOM range.
        Precision::OrYear(_) | Precision::YearInt(_) => "BET ",
    }
}

/// Formats a structured date as a GEDCOM date value.
#[must_use]
pub fn format_dmy(dmy: &Dmy, calendar: Calendar) -> String {
    let mut out = String::from(precision_keyword(&dmy.precision));
    push_ymd(&mut out, calendar, dmy.day, dmy.month, dmy.year);
    if let Precision::OrYear(alt) | Precision::YearInt(alt) = &dmy.precision {
        let Dmy2 {
            day,
            month,
            year: y,
        } = *alt;
        out.push_str(" AND ");
        push_ymd(&mut out, calendar, day, month, y);
    }
    out
}

/// Converts a `.gw` date into a `ged_io` date.
#[must_use]
pub fn to_gedcom(date: &GwDate) -> Date {
    let mut out = Date::default();
    match date {
        GwDate::Structured { dmy, calendar } => {
            out.value = Some(format_dmy(dmy, *calendar));
        }
        // GEDCOM 5.5.1 admits a parenthesised phrase where a date is expected, which is
        // what GeneWeb emits. `phrase` carries the same text for GEDCOM 7 consumers.
        GwDate::Text(text) => {
            out.value = Some(format!("({text})"));
            out.phrase = Some(text.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::parse;

    fn ged(s: &str) -> String {
        let date = parse(s)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("{s:?} is an unknown date"));
        to_gedcom(&date).value.expect("a date value")
    }

    #[test]
    fn plain_dates() {
        assert_eq!(ged("1850"), "1850");
        assert_eq!(ged("5/1850"), "MAY 1850");
        assert_eq!(ged("12/5/1850"), "12 MAY 1850");
        // Days are zero-padded, as GEDCOM expects.
        assert_eq!(ged("3/5/1850"), "03 MAY 1850");
    }

    #[test]
    fn precision_keywords() {
        assert_eq!(ged("~1850"), "ABT 1850");
        assert_eq!(ged("?1850"), "EST 1850");
        assert_eq!(ged("<1850"), "BEF 1850");
        assert_eq!(ged(">1850"), "AFT 1850");
    }

    #[test]
    fn ranges_become_bet_and() {
        assert_eq!(ged("1850..1860"), "BET 1850 AND 1860");
        assert_eq!(ged("1850|1851"), "BET 1850 AND 1851");
        assert_eq!(ged("1/1/1850..3/6/1860"), "BET 01 JAN 1850 AND 03 JUN 1860");
    }

    #[test]
    fn calendars_use_gedcom_escapes() {
        assert_eq!(ged("12/5/1750J"), "@#DJULIAN@ 12 MAY 1750");
        assert_eq!(ged("3/13/8F"), "@#DFRENCH R@ 03 COMP 8");
        assert_eq!(ged("12/5/5750H"), "@#DHEBREW@ 12 SHV 5750");
        // Gregorian carries no escape.
        assert_eq!(ged("12/5/1750G"), "12 MAY 1750");
    }

    #[test]
    fn the_calendar_escape_repeats_on_the_second_half_of_a_range() {
        assert_eq!(
            ged("1750..1760J"),
            "BET @#DJULIAN@ 1750 AND @#DJULIAN@ 1760"
        );
    }

    #[test]
    fn years_before_the_common_era() {
        assert_eq!(ged("-52"), "52 BCE");
        assert_eq!(ged("15/3/-44"), "15 MAR 44 BCE");
    }

    #[test]
    fn text_dates_become_a_phrase() {
        let date = parse("0(vers la Saint-Jean)").unwrap().unwrap();
        let out = to_gedcom(&date);
        assert_eq!(out.value.as_deref(), Some("(vers la Saint-Jean)"));
        assert_eq!(out.phrase.as_deref(), Some("vers la Saint-Jean"));
    }

    #[test]
    fn every_french_republican_month_has_a_name() {
        for month in 1..=13u8 {
            assert!(month_name(Calendar::French, month).is_some());
            assert!(month_name(Calendar::Hebrew, month).is_some());
        }
        // The Gregorian calendar has only twelve.
        assert!(month_name(Calendar::Gregorian, 12).is_some());
        assert!(month_name(Calendar::Gregorian, 13).is_none());
        assert!(month_name(Calendar::Gregorian, 0).is_none());
    }
}
