# Changelog

All notable changes to this crate are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate adheres to
[Semantic Versioning](https://semver.org/).

## [0.6.0] — 2026-10-17

### Changed

- Depends on [`oxidged`](https://github.com/trois-six/oxidged) 0.3 instead of 0.2. The
  `oxidged::Dataset` that `GwDatabase::to_gedcom` returns is now oxidged 0.3's: a crate
  that also uses oxidged must use 0.3. The conversion and its output are unchanged.

## [0.5.0] — 2026-10-15

### Changed

- Depends on [`oxidged`](https://github.com/trois-six/oxidged) 0.2 instead of 0.1. The
  `oxidged::Dataset` that `GwDatabase::to_gedcom` returns is now oxidged 0.2's: a crate
  that also uses oxidged must use 0.2. The conversion and its output are unchanged.

## [0.4.0] — 2026-10-15

The GEDCOM conversion moves from [`ged_io`](https://github.com/ge3224/ged_io) 0.17 to
[`oxidged`](https://github.com/trois-six/oxidged) 0.1, a fork of ged_io 0.17 redesigned
as a lenient reader that keeps every line and a conformant writer of GEDCOM 5.5.1, 7.0
and 7.1. Breaking: the public API exposes its model.

### Changed

- `GwDatabase::to_gedcom` returns an `oxidged::Dataset` (a GEDCOM 5.5.1 dataset, as
  before) instead of a `ged_io::GedcomData`; write it with `oxidged::GedcomWriter` in any
  version, or read it through the dataset's views.
- A separation (`#sep` on a `fam` line, `#sep` in a `fevt` block) is written as a generic
  `EVEN` typed `Separation` instead of `SEP`, a tag gwb2ged uses that no GEDCOM version
  defines. The `fevt` tag is still kept in `_GWTAG`.
- The GEDCOM 5.5.1 written from a `.gw` file is conformant: its header has `CHAR UTF-8`
  and a `SUBM` pointer to a stub submitter record, which ged_io 0.17 left out; a name's
  `TYPE` is written in its 5.5.1 spelling (`aka`); inline sources come before the
  structures that follow them; the last line is terminated. A portrait's `FORM`, which a
  `.gw` file does not state, is derived from the file name, and event witnesses, which
  5.5.1 has no `ASSO` under an event for, are kept as `_ASSO`.
- Written as GEDCOM 7, the dataset is converted as the FamilySearch migration guide says
  (an association's `RELA` becomes a `ROLE` with its phrase, a portrait's inline `OBJE` a
  multimedia record) instead of being repaired.
- The text of an extended page or a wizard note is a single extension value whose line
  breaks the writer writes as `CONT` lines and the reader joins back; the output is the
  same as before.
- Minimum supported Rust version: 1.88, oxidged's.

## [0.3.0] — 2026-10-08

Follows ged_io to 0.17, whose model the public API exposes. The death reason (`_GWDEATH`)
and the original event tag (`_GWTAG`) are written as extension substructures of the event
rather than as lines of its note.

Earlier releases are described in the git history.
