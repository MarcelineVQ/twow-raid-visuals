# Twow Raid Visuals + WoW 1.12 DBC Patch Manager

![](./ss1.png)  
To use this project's visuals, download [the latest release](https://github.com/MarcelineVQ/twow-raid-visuals/releases/latest) and unpack to your Turtle WoW `Data` folder.  

## Patches overview

The patches included in this repository adjust the visuals and sounds for several various spells and boss abilities.  They are applied in alphabetical order based on the patch file name so that later patches may override or build upon earlier ones.  
The patches currently are based on twow `1.17.2` dbc's, simply change out the .dbc files in the `dbc/` folder to use other versions.  
The highlights are summarised below:

- **AQ20:** Captain Tuubid visibly marks the player targeted for Attack Command.  

- **Molten Core:** Baron Geddon indicates the radius of his Inferno ability.  Living Bomb has a rough indication of area of effect.  

- **Blackwing Lair:** Warlocks and Spellbinders indicate the radius of their AoE damage.  

- **AQ 40:**
  - Anubisath Sentinels have additional visual cues to help mageless compositions.  
    - Reflect bubbles and glowing shadow or fire in hand to show which reflect it is.  
    - A deep purple curse for Mana Burn.  
    - Rejuvenation effect for HoT.  
    - Lightning breath to indicate Thunder Clap.  
    - Red weapon to indicate Mortal Strike.  
    - Shadow breath to indicate Shadow Volley.  
  - Lord Kri and Viscidus poison area of effects indicate their radius.  
  - Twin Emporer Blizzard and Bug Explosion indicate radius.  

- **Naxxrams:**
  - Poison Charge now colors your character like other enemy poisons in the game do.  
  - Faerlina Acolytes are more obvious when casting Shadow Bolt Volley.  
  - Faerlina indicates the radius of her Rain of Fire.  
  - Thaddius polarity directly colors your character.  
  - Sapphiron Blizzards show their point of impact.  

- **Kara 40:**
  - Gnarlmoon Moon debuffs have reduced spell effects to improve fps.  
  - Incantagos Blizzard and Guided-Ley Beam have additional indication to show what area they affect.  
  - Anomalus soak zones have much improved visibility.  
  - Medivh Flamestrike indicates its area of effect. Corruption of Medivh has much stronger indication on your character.  
  - Rupturan Flamestrikes indicate their area of effect.  
  - Sanv Tas'dal indicates the radius of his Overflowing hatred.  Netherwalkers, when visible in the first place, have improved visibility.  Arena has improved fps.  
  - Mark of the Highlord decurse explosion given rough area of effect indication.  
  - Trash:
    - Karazhan Protectors indicate Spell Reflect with green coloring.  
    - Crumbling Protectors indicate Self-Destruct Protocol with red coloring.  
    - Lingering Arcanists indicate their Blizzard area of effect.  
    - Arcane Anomaly indicates the application range of its stacking debuff (untested).  

- **Other:**
  - Silenced the looping Baby Murloc dance sound.  
  - Gave Concussion Blow a different sound than Shield Slam.  
  - Gave Totemic Recall a different sound than totem placement.  
  - Removed obnoxious green tinting from Decaying Flesh debuff.  

- **Credits:** These contributors did not ask to be credited but should be mentioned.  
  - **Lhian** – `.wmo` edits adapted from his Karazhan performance patch.  
  - **Incantation / Fauna** – Major visual improvement to Anomalus soak zones.  

---
## What this tool does

This Rust crate offers a command‑line workflow for modifying World of Warcraft 1.12 DBC files and packaging them into an MPQ archive.  
It allows you to define readable YAML patches that insert, update or copy rows across multiple tables, resolve field names through a schema, and bundle additional assets.  
The tool applies all patches in ascending filename order and reports warnings along with the patch file that triggered them.

### Features

- **DBC parsing and writing** – Reads vanilla DBC tables, applies patches and writes out new files.  
A lightweight parser operates on untyped 32‑bit fields; built‑in YAML schemas (derived from WDBXEditor’s `Classic 1.12.1` definitions) allow you to refer to fields by name.  
Schema files in `schema` are used automatically, and you can override them by providing your own `schema` directory.
- **Patch format with `update`, `insert` and `copy` actions** – YAML patches can update existing records, insert entirely new rows (optionally specifying a `key` and `key_column` for the primary key), or copy a record and modify selected fields.  
Floats in patches are transparently converted to their 32‑bit bit patterns, so writing `0.5` is equivalent to specifying its raw integer representation.
- **Multi‑DBC documents** – A single patch file may contain multiple `Table.dbc:` sections.  
Multiple sections targeting the same table are concatenated rather than overwritten.
- **Deterministic ordering** – When no explicit patch list is provided, all `.yaml` or `.yml` files in the patch directory are sorted alphabetically and applied in order.  
This allows you to layer patches (e.g. `0‑base.yaml`, `1‑boss.yaml`, `z‑test.yaml`).
- **Duplicate detection** – When inserting or copying, the tool checks whether the new primary key already exists and skips the change with a warning to prevent duplicate IDs.
- **Contextual warnings** – Any warning emitted while applying patches identifies the originating patch file, making it easier to track down invalid field names or missing schemas.
- **Includes support** – The `build` command can bundle any files under an `includes/` directory (or a directory you specify with `--includes-dir`) into the MPQ alongside your DBCs.  
This is useful for adding custom models or textures such as the new area indicators.
- **Default directories** – Unless overridden, the tool reads DBCs from `dbc/`, patches from `patches/`, schemas from `schema/` (with fallbacks to the built‑in defaults) and writes output to `build/`.

### Patch format

Patch files are YAML documents that can take several shapes:

1. A mapping where each key is a table name (e.g. `Spell.dbc`) and the value is a list of change objects.
2. Multiple such mappings separated by blank lines—useful for repeating a table name several times in one file.
3. A single object containing `dbc` and `changes` keys (backwards‑compatible).  You may also provide a sequence of such objects.

Each change object must have a `type` field, which may be `update`, `insert` or `copy`:

- **update** – Locate a row where `key_column` (default 0) matches `key`, then change the specified fields.
- **insert** – Create a new row with all columns initialised to zero.  Set values from the `values` mapping.  
You can include `key` and `key_column` to assign a primary key unless it is supplied in `values`.
- **copy** – Duplicate an existing row identified by `key`/`key_column`, then apply the `updates` mapping.  
If you omit the primary key from `updates`, it is inherited from the original, so you should normally include a new `ID`.

Field identifiers may be either numeric strings (zero‑based column numbers) or names defined in the schema.  
Values may be integers, strings or floats; floats are converted to their 32‑bit representations on write.

### Building and usage

This project is a standard Cargo package.  
You need Rust installed and access to the `wow-cdbc` and `wow-mpq` crates (version 0.2).  
After cloning, run:

```bash
cargo build --release
```

Apply patches to your DBC files and write them to the default `build/` directory:

```bash
./target/release/wow_dbc_patcher apply
```

Apply patches and build an MPQ archive (using default directories for DBCs, patches, schemas and includes):

```bash
./target/release/wow_dbc_patcher build --mpq patch-1.mpq
```

You can override any of the input or output locations:

- `--dbc-files <paths…>` – explicitly list DBC files to patch instead of scanning `dbc/`.
- `--dbc-dir <dir>` – change the directory used to locate DBCs when `--dbc-files` is omitted (default `dbc`).
- `--patches <paths…>` – explicitly list patches; when omitted, all YAML files in the patch directory are used.
- `--patch-dir <dir>` – change the directory used to discover patch files (default `patches`).
- `--schema-dir <dir>` – load schemas from a custom directory; built‑in defaults are used as a fallback.
- `--includes-dir <dir>` – include additional files from this directory when building an MPQ (default `includes`).
- `--out-dir <dir>` – change the output directory for modified DBCs (default `build`).

MPQ packaging is optional; omit `--mpq` if you only need the patched DBC files.

### Limitations

- **Simplified typing** – The default parser stores every field as a 32‑bit integer.  
The schemas allow you to reference fields by name but do not change their underlying type.  
Multi‑column arrays are flattened; floats and arrays are still written as raw bits.  
- **Vanilla format only** – The current implementation targets WoW 1.12 (WDBC).  
Newer formats with hash tables (WDB2/WDB5) are not supported.
- **Duplicate strings** – New string values are appended to the string block even if identical strings already exist.  
Large numbers of similar (string) inserts may increase file size.

For more advanced functionality, including typed access to DBC fields, see the [`wow-cdbc` documentation](https://raw.githubusercontent.com/wowemulation-dev/warcraft-rs/master/file-formats/database/wow-cdbc/README.md).
