# rust-geneweb

**A reader for GeneWeb `.gw` genealogy files, with conversion to GEDCOM**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## What is `geneweb`?

`geneweb` is a Rust library for reading [GW](https://geneweb.tuxfamily.org/wiki/gw) files —
the textual interchange format of the [GeneWeb](https://geneweb.tuxfamily.org) genealogy
software, produced by `gwu` and consumed by `gwc`. It supports the `gwplus` extension
introduced in GeneWeb 7.00, which adds structured personal and family events.

Once a file is read you can convert it to [`ged_io`](https://github.com/ge3224/ged_io)'s
GEDCOM model and use everything that crate offers.

```rust
use geneweb::database::GwDatabase;
use ged_io::writer::GedcomWriter;

let bytes = std::fs::read("family.gw")?;
let db = GwDatabase::read(&bytes, "family.gw")?;

println!("{} persons, {} families", db.persons.len(), db.families.len());

let gedcom = GedcomWriter::new().write_to_string(&db.to_gedcom())?;
std::fs::write("family.ged", gedcom)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

A command-line converter comes with the crate:

```sh
cargo run --example gw2ged -- family.gw family.ged
```

## Design

Reading happens in two layers, because the two data models are not equivalent.

1. **A lossless `.gw` syntax tree.** GeneWeb records things GEDCOM has no room for —
   per-person access rights, wizard notes, extended wiki pages, occurrence-numbered keys,
   fine-grained witness roles — and this layer keeps all of them.
2. **A conversion to GEDCOM.** Anything without a standard counterpart is preserved as a
   user-defined `_GW…` tag rather than dropped. See the [`gedcom`] module docs for the
   full table.

The normative reference for the grammar is GeneWeb's own OCaml implementation
(`bin/gwc/gwcomp.ml`), not the wiki page, which lags it. The GEDCOM mapping follows
`bin/gwb2ged/gwb2gedLib.ml`, so this crate and GeneWeb's own exporter agree on date
formats, event tags and witness roles.

## What is covered

Every block of the format: `fam`, `notes`, `notes-db`, `page-ext`, `wizard-note`, `rel`,
`pevt`, and the `encoding: utf-8` and `gwplus` directives. Every field of a person and a
family, all 50 personal and 12 family event names plus the open-ended `#tag` fallback, all
five relation kinds, all eight witness roles, and the full date grammar — precisions,
Julian, French Republican and Hebrew calendars, alternatives, intervals and text dates.

Both encodings are handled: `.gw` is ISO-8859-1 unless a file opts into UTF-8, and the
switch takes effect mid-file, as GeneWeb does it.

## Not covered

- Writing `.gw` files. This crate reads only.
- The binary `.gwb` database and the compiled `.gwo` intermediate.
- GEDCOM to `.gw`, which is what GeneWeb's `ged2gwb` is for.
- Image files. The `.gw` format carries paths, not the images themselves.

## Testing

The main fixture is `galichet.gw`, taken verbatim from GeneWeb's own test suite. GeneWeb's
`gwc` cram test records that it compiles to 35 persons and 15 families; this crate produces
the same counts, which is the strongest available check that references resolve to the same
people GeneWeb resolves them to.

```sh
cargo test
cargo clippy --all-targets
```

## Licence

MIT.

[`gedcom`]: https://docs.rs/geneweb/latest/geneweb/gedcom/
