# Wow DBC Manager (Rust)

This crate provides a simple command‑line tool for modifying vanilla (1.12)
World of Warcraft DBC tables and optionally packaging the results into an
MPQ archive.  It is designed to make it easy to apply scripted patches to
spell‑related data such as `Spell.dbc`, `SpellVisual.dbc`,
`SpellVisualKit.dbc` and `SpellVisualEffectNames.dbc` without the overhead of
a relational database.  The core functionality is implemented in pure Rust
and leverages the `wow_mpq` and `wow_cdbc` crates (version `0.2`) from the
[`warcraft-rs` project] to handle MPQ archives and DBC file parsing/writing.

## Features

* **DBC parsing and writing** – Reads a DBC file, applies updates/inserts,
  rebuilds the string block and writes the modified file back to disk using a
  lightweight parser.  The default implementation treats each field as an
  unsigned 32‑bit value, but you can provide field names via a YAML schema to
  refer to columns symbolically.  When a patch introduces a string value the
  tool automatically appends it to the string block and fixes up the offset.
* **Patch format** – Patches are defined in YAML to keep them human readable.
  A single patch file may target multiple tables and each change can update
  existing records or insert new ones.  Fields may be addressed either by
  numeric index or by name when a schema is available.  See below for examples.
* **Schema support** – By default the tool treats all fields as anonymous
  integers.  If you provide a `schema` directory containing YAML files
  describing the field order for each DBC (e.g. `Spell.dbc.yaml` listing
  `id`, `schoolMask`, `category`, etc.), you can refer to fields by name in
  your patches.  Field lookups are case‑insensitive.  Unknown names fall back
  to numeric indices with a warning.
* **MPQ packaging** – When the `wow_mpq` crate is available the tool can
  package your modified DBCs into a single MPQ archive.  Files are added
  under the conventional `DBFilesClient/` prefix.  Use the `build` subcommand
  to apply patches and write an MPQ in one go.

## Patch format

Patches are described in YAML.  Each file can define modifications for one
or more DBCs.  A patch document can take three forms:

1. A single patch object with fields `dbc` and `changes`.
2. A sequence of such patch objects.
3. A mapping whose keys are DBC file names and whose values are arrays of
   change objects.

Each change is either an `update` or an `insert` (tagged by the `type` field).
Updates identify a record by its `key` in a chosen `key_column` (default 0)
and then specify a mapping of fields to new values.  Inserts specify values
for only the columns that differ from zero.  Field identifiers can be either
numeric strings (e.g. `"57"`) or names when a schema is supplied.  You may
define multiple changes across different tables in a single file.

For example, the following patch updates two spells and inserts a new
visual effect.  Field names are resolved using schema definitions in the
`schema/` directory.  If a schema is missing, numeric indices must be used.

```yaml
Spell.dbc:
  - type: update
    # Spell ID 1234: change its cast time and name.  Without a schema
    # these would be indices (3 and 57).
    key: 1234
    updates:
      CastTimeIndex: 5678
      SpellName: "My Rebalanced Spell"

  - type: insert
    # Add a completely new spell.  Only non‑default fields need to be
    # specified; unspecified columns default to zero.
    values:
      Id: 900000
      CastTimeIndex: 9000
      SchoolMask: 1
      SpellName: "Custom Spell"

SpellVisual.dbc:
  - type: update
    # Modify a visual entry keyed by its ID
    key: 321
    updates:
      Name: "New Visual Name"
      Param1: 42
```

If you do not provide a schema for a given DBC, the tool treats all fields as
anonymous indices starting at 0.  It will attempt to parse strings only when
explicitly requested in a patch; otherwise fields remain as u32 values.  For
fully typed access to DBC data (including arrays and floats) consider using
the higher level `dbc_tool` in the `warcraft-rs` repository.

## Building

This project is a standard Cargo package.  To build it yourself you must have a
Rust toolchain installed and ensure that the `wow-cdbc` and `wow-mpq` crates are
resolvable.  At the time of writing these crates live in the
`wowemulation-dev/warcraft-rs` repository; if they are not published on
crates.io you can point Cargo at the repository using a `git` dependency.

```toml
[dependencies]
wow-cdbc = { git = "https://github.com/wowemulation-dev/warcraft-rs", package = "wow-cdbc" }
wow-mpq  = { git = "https://github.com/wowemulation-dev/warcraft-rs", package = "wow-mpq" }
```

## Usage

After building with `cargo build --release`, run `wow_dbc_manager_rs` with one
of the subcommands:

```bash
# Apply patches to the given DBC files and write modified versions to
# the `build/` directory
./target/release/wow_dbc_manager_rs apply

# Apply patches and build an MPQ archive (defaults to using files in
# `dbc/` and `patches/` and writes output to `build/`)
./target/release/wow_dbc_manager_rs build \
  --mpq build/patch-1.mpq
```

By default the tool will determine which DBC files to load by inspecting the
patches and looking for those files in a `dbc/` directory.  Likewise it
loads field name schemas from a `schema/` directory if present.  You can
override these defaults with:

* `--dbc-files <paths…>` – explicitly specify one or more DBC files to patch.
  When provided, the tool does not scan the default `dbc/` directory.
* `--dbc-dir <dir>` – change the default directory used to locate DBC files
  when `--dbc-files` is omitted.  The default is `dbc`.
* `--patches <paths…>` – explicitly specify one or more patch files.  When
  omitted, all `.yaml` and `.yml` files in `--patch-dir` are used.
* `--patch-dir <dir>` – change the directory used to discover patch files.
  The default is `patches`.
* `--out-dir <dir>` – change the output directory for modified DBCs.  The
  default is `build`.
* `--schema-dir <dir>` – change the directory from which schema YAML files
  are loaded.  The default is `schema`.

If MPQ support is not desired or the dependency cannot be resolved, omit the
`--mpq` flag.  The tool will still write updated DBC files to the output
directory.

## Limitations

* **No automatic schema discovery** – By default the parser treats every field
  as a 32‑bit integer.  This suffices for many integer fields but does not
  expose floats or arrays.  Schema files allow you to refer to fields by
  name but do not change their type; floats and arrays are truncated to
  integers.  To work with proper types and names you can extend the code to
  load a `wow_cdbc::Schema` from a YAML definition and implement typed
  writing.
* **Vanilla only** – The default record size handling assumes the classic
  WDBC format used up to WoW 3.x.  Later versions (WDB2/WDB5) include hash
  tables and other metadata that this simple parser does not support.  Use
  `wow_cdbc` directly for those versions.
* **Unsafe duplicate strings** – The writer appends new strings to the end of
  the existing string block without checking for duplicates.  If you patch
  multiple fields to the same value, duplicate copies will be inserted.

For more advanced usage and a fully featured schema system, see the
[`wow-cdbc` documentation](https://raw.githubusercontent.com/wowemulation-dev/warcraft-rs/master/file-formats/database/wow-cdbc/README.md).