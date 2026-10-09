//! Converts a GeneWeb `.gw` file to GEDCOM.
//!
//! ```text
//! cargo run --example gw2ged -- input.gw [output.ged]
//! ```
//!
//! With no output path the GEDCOM goes to standard output. Malformed blocks are reported
//! on standard error and skipped, so a partly broken file still yields what it can.

use std::io::Write;
use std::path::Path;

use ged_io::GedcomWriter;
use geneweb::database::GwDatabase;

fn main() -> std::process::ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: gw2ged <input.gw> [output.ged]");
        return std::process::ExitCode::FAILURE;
    };
    let output = args.next();

    let input = Path::new(&input);
    let bytes = match std::fs::read(input) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("{}: {e}", input.display());
            return std::process::ExitCode::FAILURE;
        }
    };

    let origin = input
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());

    let (db, errors) = GwDatabase::read_lenient(&bytes, &origin);
    for error in &errors {
        eprintln!("warning: {error}");
    }
    eprintln!(
        "{} persons, {} families{}",
        db.persons.len(),
        db.families.len(),
        if db.gwplus { " (gwplus)" } else { "" }
    );

    let text = match GedcomWriter::new().write_to_string(&db.to_gedcom()) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("writing GEDCOM: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let wrote = match &output {
        Some(path) => {
            std::fs::write(path, &text).map_err(|e| format!("{}: {e}", Path::new(path).display()))
        }
        None => std::io::stdout()
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string()),
    };
    if let Err(e) = wrote {
        eprintln!("{e}");
        return std::process::ExitCode::FAILURE;
    }

    // A file that produced no records almost certainly failed to parse rather than
    // being genuinely empty; say so instead of silently succeeding.
    if db.persons.is_empty() && !errors.is_empty() {
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
