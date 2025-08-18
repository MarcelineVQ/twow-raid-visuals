use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_yaml;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};

mod dbc;
mod patch;

use dbc::{build_string_map, read_dbc, write_dbc};
use patch::{PatchEntry, PatchFile, ValueType};

/// Command line interface for the WoW DBC manager.  Supports applying
/// patches to one or more DBC files and optionally packaging them into an
/// MPQ archive.  See the README for details and examples.
#[derive(Debug, Parser)]
#[command(name = "wow_dbc_manager_rs", about = "Vanilla WoW DBC patcher and MPQ packer")] 
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Apply patches to the given DBC files and output the modified files
    Apply {
        /// Paths to specific DBC files to process.  If omitted the tool
        /// automatically determines which tables to patch based on the
        /// contents of your YAML patches and looks for those files in the
        /// default `dbc` directory (see `--dbc-dir`).
        #[arg(short = 'd', long = "dbc-files")]
        dbc_files: Vec<PathBuf>,
        /// Directory containing source DBC files.  Used when
        /// `--dbc-files` is not specified.  Defaults to `dbc`.
        #[arg(long = "dbc-dir", default_value = "dbc")]
        dbc_dir: PathBuf,
        /// YAML patch files to apply.  If omitted the tool will load all
        /// `.yaml` and `.yml` files from the default patch directory (see
        /// `--patch-dir`).  Patches targeting unknown tables are ignored
        /// with a warning.
        #[arg(short = 'p', long = "patches")]
        patches: Vec<PathBuf>,
        /// Directory containing patch YAML files.  Used when `--patches`
        /// is not specified.  Defaults to `patches`.
        #[arg(long = "patch-dir", default_value = "patches")]
        patch_dir: PathBuf,
        /// Output directory where modified DBCs will be written.  The
        /// directory will be created if it does not exist.  Defaults to
        /// `build`.
        #[arg(short = 'o', long = "out-dir", default_value = "build")]
        out_dir: PathBuf,
        /// Directory containing schema definitions (YAML files listing field
        /// names in order).  For each DBC file `Foo.dbc` the tool looks
        /// for `schema_dir/Foo.dbc.yaml` and uses it to map field names to
        /// column indices.  Defaults to `schema`.
        #[arg(long = "schema-dir", default_value = "schema")]
        schema_dir: PathBuf,
    },
    /// Apply patches and then build an MPQ archive containing the
    /// resulting DBC files.  The MPQ will contain files under
    /// `DBFilesClient/<name>`.
    Build {
        /// Input DBC files to patch.  If omitted the tool derives
        /// which tables to patch from the YAML and looks them up in
        /// `--dbc-dir`.
        #[arg(short = 'd', long = "dbc-files")]
        dbc_files: Vec<PathBuf>,
        /// Directory containing source DBC files.  Used when
        /// `--dbc-files` is not specified.  Defaults to `dbc`.
        #[arg(long = "dbc-dir", default_value = "dbc")]
        dbc_dir: PathBuf,
        /// Patch files in YAML format.  If omitted the tool will load all
        /// `.yaml` and `.yml` files from the default patch directory (see
        /// `--patch-dir`).
        #[arg(short = 'p', long = "patches")]
        patches: Vec<PathBuf>,
        /// Directory containing patch YAML files.  Used when `--patches` is
        /// not specified.  Defaults to `patches`.
        #[arg(long = "patch-dir", default_value = "patches")]
        patch_dir: PathBuf,
        /// Directory where modified DBCs will be written.  Defaults to
        /// `build`.
        #[arg(short = 'o', long = "out-dir", default_value = "build")]
        out_dir: PathBuf,
        /// Path of the MPQ archive to create
        #[arg(short = 'm', long = "mpq", required = true)]
        mpq_path: PathBuf,
        /// MPQ format version (1, 2, 3 or 4).  Defaults to 2.
        #[arg(long = "mpq-version", default_value_t = 2)]
        mpq_version: u8,
        /// Directory containing schema definitions (see `apply`)
        #[arg(long = "schema-dir", default_value = "schema")]
        schema_dir: PathBuf,
        /// Directory containing additional files to include in the MPQ.
        /// All files under this directory will be added to the archive
        /// preserving their relative paths.  Defaults to `includes`.
        #[arg(long = "includes-dir", default_value = "includes")]
        includes_dir: PathBuf,
    },
}

/// Resolves a key column name or index to a numeric index
fn resolve_key_column_index(
    key_column: &Option<String>,
    schema_map: &Option<HashMap<String, usize>>,
    file_name: &str,
    pf_origin: &str,
) -> usize {
    match key_column {
        Some(ref col_name) => {
            // Try to parse as a number first
            if let Ok(idx) = col_name.parse::<usize>() {
                idx
            } else {
                // Look up by name in schema map
                if let Some(ref schema) = schema_map {
                    if let Some(&idx) = schema.get(&col_name.to_lowercase()) {
                        idx
                    } else {
                        println!(
                            "Warning: unknown key column '{}' in {} (patch file: {}) – defaulting to 0",
                            col_name, file_name, pf_origin
                        );
                        0
                    }
                } else {
                    println!(
                        "Warning: no schema for {} (patch file: {}), cannot resolve key column '{}', defaulting to 0",
                        file_name, pf_origin, col_name
                    );
                    0
                }
            }
        }
        None => 0,
    }
}

/// Resolves a field name or index to a numeric index
fn resolve_field_index(
    field_name: &str,
    schema_map: &Option<HashMap<String, usize>>,
) -> Option<usize> {
    // Try parse as number
    if let Ok(idx) = field_name.parse::<usize>() {
        Some(idx)
    } else {
        schema_map
            .as_ref()
            .and_then(|schema| schema.get(&field_name.to_lowercase()).cloned())
    }
}

/// Applies values to a record, handling string allocation
fn apply_values_to_record(
    values: &HashMap<String, ValueType>,
    record: &mut Vec<u32>,
    schema_map: &Option<HashMap<String, usize>>,
    string_map: &mut HashMap<String, u32>,
    new_strings: &mut Vec<String>,
    string_block: &[u8],
    file_name: &str,
    pf_origin: &str,
    record_key: u32,
) {
    for (field_name, value) in values {
        let field_idx = match resolve_field_index(field_name, schema_map) {
            Some(i) => i,
            None => {
                println!(
                    "Warning: unknown field '{}' in {} (patch file: {}) – skipping",
                    field_name, file_name, pf_origin
                );
                continue;
            }
        };
        
        if field_idx >= record.len() {
            println!(
                "Warning: field {} out of range for record with key {} in {} (patch file: {})",
                field_idx, record_key, file_name, pf_origin
            );
            continue;
        }
        
        match value {
            ValueType::String(s) => {
                // Check if string already exists
                let offset = if let Some(&off) = string_map.get(s) {
                    off
                } else {
                    let offset = (string_block.len()
                        + new_strings.iter().map(|ss| ss.len() + 1).sum::<usize>()) as u32;
                    string_map.insert(s.clone(), offset);
                    new_strings.push(s.clone());
                    offset
                };
                record[field_idx] = offset;
            }
            _ => {
                if let Some(int_val) = value.as_u32() {
                    record[field_idx] = int_val;
                }
            }
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Apply {
            dbc_files,
            patches,
            out_dir,
            schema_dir,
            dbc_dir,
            patch_dir,
        } => {
            // Determine which patch files to use.  If none were specified,
            // read all .yaml and .yml files from the patch_dir.
            let patch_paths: Vec<PathBuf> = if patches.is_empty() {
                let mut files = Vec::new();
                if patch_dir.exists() {
                    for entry in fs::read_dir(&patch_dir)? {
                        let entry = entry?;
                        let path = entry.path();
                        if path.extension().map_or(false, |ext| {
                            let ext = ext.to_string_lossy().to_lowercase();
                            ext == "yaml" || ext == "yml"
                        }) {
                            files.push(path);
                        }
                    }
                }
                files
            } else {
                patches.clone()
            };
            // Determine which DBC files to process.  If the user did not
            // explicitly specify any, infer them from the patch files and
            // load them from the dbc_dir directory.
            let dbc_paths: Vec<PathBuf> = if dbc_files.is_empty() {
                let patch_map = load_patches(&patch_paths)?;
                let mut set: HashSet<String> = HashSet::new();
                for key in patch_map.keys() {
                    set.insert(key.clone());
                }
                let mut paths = Vec::new();
                for name in set {
                    // Attempt to resolve the file in dbc_dir by case‑insensitive match.
                    let mut found_path: Option<PathBuf> = None;
                    if dbc_dir.exists() {
                        for entry in fs::read_dir(&dbc_dir)? {
                            let entry = entry?;
                            if let Some(file_name) = entry.file_name().to_str() {
                                if file_name.to_lowercase() == name {
                                    found_path = Some(entry.path());
                                    break;
                                }
                            }
                        }
                    }
                    let path = found_path.unwrap_or_else(|| dbc_dir.join(&name));
                    paths.push(path);
                }
                paths
            } else {
                dbc_files.clone()
            };
            apply_command(&dbc_paths, &patch_paths, &out_dir, &schema_dir)?;
        }
        Commands::Build {
            dbc_files,
            patches,
            out_dir,
            mpq_path,
            mpq_version,
            schema_dir,
            dbc_dir,
            patch_dir,
            includes_dir,
        } => {
            // Determine which patch files to use.
            let patch_paths: Vec<PathBuf> = if patches.is_empty() {
                let mut files = Vec::new();
                if patch_dir.exists() {
                    for entry in fs::read_dir(&patch_dir)? {
                        let entry = entry?;
                        let path = entry.path();
                        if path.extension().map_or(false, |ext| {
                            let ext = ext.to_string_lossy().to_lowercase();
                            ext == "yaml" || ext == "yml"
                        }) {
                            files.push(path);
                        }
                    }
                }
                files
            } else {
                patches.clone()
            };
            // Determine input DBC files for building.  Same logic as apply.
            let dbc_paths: Vec<PathBuf> = if dbc_files.is_empty() {
                let patch_map = load_patches(&patch_paths)?;
                let mut set: HashSet<String> = HashSet::new();
                for key in patch_map.keys() {
                    set.insert(key.clone());
                }
                let mut paths = Vec::new();
                for name in set {
                    let mut found_path: Option<PathBuf> = None;
                    if dbc_dir.exists() {
                        for entry in fs::read_dir(&dbc_dir)? {
                            let entry = entry?;
                            if let Some(file_name) = entry.file_name().to_str() {
                                if file_name.to_lowercase() == name {
                                    found_path = Some(entry.path());
                                    break;
                                }
                            }
                        }
                    }
                    let path = found_path.unwrap_or_else(|| dbc_dir.join(&name));
                    paths.push(path);
                }
                paths
            } else {
                dbc_files.clone()
            };
            build_command(
                &dbc_paths,
                &patch_paths,
                &out_dir,
                &mpq_path,
                mpq_version,
                &schema_dir,
                &includes_dir,
            )?;
        }
    }
    Ok(())
}

/// Read and parse all patch files.  Returns a vector of `PatchFile` and a
/// map from lower‑cased DBC file name to patches.  A DBC file may have
/// multiple patch files targeting it.
/// Parse a YAML document into one or more `PatchFile` values.  A patch
/// document can take several forms:
///
/// 1. A single patch object with fields `dbc` and `changes`.
/// 2. A sequence of patch objects as described above.
/// 3. A mapping of DBC file names to arrays of changes.  In this case
///    the key becomes the `dbc` field of a new `PatchFile` and the value
///    must be a sequence of change objects.
fn parse_patch_value(value: serde_yaml::Value, path: &Path) -> Result<Vec<PatchFile>> {
    use serde_yaml::Value;
    let mut patch_files = Vec::new();
    match value {
        // An empty document (null) or an empty mapping yields no patches.  This
        // allows YAML files with only comments or whitespace to be ignored.
        Value::Null => {
            return Ok(patch_files);
        }
        Value::Mapping(ref map) if map.is_empty() => {
            return Ok(patch_files);
        }
        Value::Sequence(seq) => {
            for item in seq {
                // Try to parse each element as a PatchFile
                let pf: PatchFile = serde_yaml::from_value(item.clone()).with_context(|| {
                    format!("Failed to parse patch entry in {:?}", path)
                })?;
                patch_files.push(pf);
            }
        }
        Value::Mapping(map) => {
            // Heuristic: if the mapping contains keys "dbc" and "changes", treat
            // it as a single patch file
            let has_dbc = map.contains_key(&Value::String("dbc".to_string()));
            let has_changes = map.contains_key(&Value::String("changes".to_string()));
            if has_dbc && has_changes {
                let pf: PatchFile = serde_yaml::from_value(Value::Mapping(map)).with_context(|| {
                    format!("Failed to parse patch file {:?}", path)
                })?;
                patch_files.push(pf);
            } else {
                // Otherwise treat the mapping as a collection of DBC name to changes
                for (k, v) in map {
                    // Key must be a string representing the DBC name
                    let dbc_name = match k {
                        Value::String(s) => s,
                        _ => {
                            return Err(
                                anyhow::anyhow!("Invalid DBC key in patch file {:?}: {:?}", path, k)
                            );
                        }
                    };
                    // Value must be a sequence of changes
                    let changes: Vec<PatchEntry> = serde_yaml::from_value(v).with_context(|| {
                        format!("Failed to parse changes for {} in {:?}", dbc_name, path)
                    })?;
                    let pf = PatchFile {
                        dbc: dbc_name,
                        changes,
                        origin: None,
                    };
                    patch_files.push(pf);
                }
            }
        }
        _ => {
            return Err(anyhow::anyhow!("Unexpected YAML structure in patch file {:?}", path));
        }
    }
    Ok(patch_files)
}

/// Split a patch file into multiple YAML sections based on repeated top‑level DBC keys.
/// This allows users to specify the same DBC name multiple times in a single file
/// (e.g. `SpellVisual.dbc:` followed by another `SpellVisual.dbc:`).  We scan the
/// file line by line; whenever we encounter a line with no leading indentation
/// and ending in `.dbc:`, we treat that as the start of a new section.  Each
/// section is parsed independently via `parse_patch_value` and aggregated.
fn parse_patch_file(path: &Path) -> Result<Vec<PatchFile>> {
    use std::fs;
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read patch file {:?}", path))?;
    // Split into sections by top‑level DBC keys
    let mut sections: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in content.lines() {
        // If the line has no leading indentation and ends with `.dbc:`, start a new section
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent == 0 && trimmed.ends_with(".dbc:") {
            if !current.trim().is_empty() {
                sections.push(current);
                current = String::new();
            }
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        sections.push(current);
    }
    // If no sections were detected, treat the whole file as a single section
    if sections.is_empty() {
        sections.push(content);
    }
    let mut pfs_all = Vec::new();
    for section in sections {
        // Parse each section as YAML
        let value: serde_yaml::Value = serde_yaml::from_str(&section).with_context(|| {
            format!("Failed to parse YAML section in {:?}", path)
        })?;
        let mut pfs = parse_patch_value(value, path)?;
        // Set the origin on each patch file to the current path
        for pf in &mut pfs {
            pf.origin = Some(path.to_path_buf());
        }
        pfs_all.append(&mut pfs);
    }
    Ok(pfs_all)
}

fn load_patches(patch_paths: &[PathBuf]) -> Result<HashMap<String, Vec<PatchFile>>> {
    let mut patches_map: HashMap<String, Vec<PatchFile>> = HashMap::new();
    // Sort patch paths alphabetically by their file name to enforce deterministic ordering
    let mut sorted: Vec<&PathBuf> = patch_paths.iter().collect();
    sorted.sort_by(|a, b| {
        let a_name = a
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let b_name = b
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        a_name.cmp(b_name)
    });
    for path in sorted {
        let pfs = parse_patch_file(path)?;
        for pf in pfs {
            let key = pf.dbc.to_lowercase();
            patches_map.entry(key).or_default().push(pf);
        }
    }
    Ok(patches_map)
}

/// Load a schema mapping for a given DBC file.  The schema directory must
/// contain a YAML file whose name is derived from the DBC file name with
/// `.yaml` appended (for example `Spell.dbc.yaml`).  The YAML can be either
/// a sequence of strings representing field names in order, or a mapping with
/// a `fields` entry that is such a sequence.  Field names are converted to
/// lowercase for case‑insensitive lookup.  Returns `None` if the file
/// doesn't exist or cannot be parsed.
fn load_schema_map(schema_dir: &Path, dbc_file_name: &str) -> Option<HashMap<String, usize>> {
    use std::io::BufReader;
    // Attempt to load a YAML file for this DBC from the provided
    // schema directory.  If it does not exist there, fall back to
    // the built‑in defaults under `schema` in the project
    // root.  This allows shipping canonical 1.12 definitions with
    // the tool while still permitting overrides via --schema-dir.
    let yaml_name = format!("{}.yaml", dbc_file_name);
    let candidate_dirs = [schema_dir, Path::new("schema")];
    for dir in &candidate_dirs {
        let path = dir.join(&yaml_name);
        if !path.exists() {
            continue;
        }
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);
        let value: serde_yaml::Value = match serde_yaml::from_reader(reader) {
            Ok(v) => v,
            Err(err) => {
                println!("Warning: failed to parse schema {}: {}", path.display(), err);
                continue;
            }
        };
        let mut mapping: HashMap<String, usize> = HashMap::new();
        match value {
            serde_yaml::Value::Sequence(seq) => {
                for (i, item) in seq.iter().enumerate() {
                    if let serde_yaml::Value::String(name) = item {
                        mapping.insert(name.to_lowercase(), i);
                    }
                }
                return Some(mapping);
            }
            serde_yaml::Value::Mapping(map) => {
                // Try to find "fields" entry
                let mut found_fields = false;
                for (k, v) in map.iter() {
                    if let serde_yaml::Value::String(ref key_name) = k {
                        if key_name == "fields" {
                            if let serde_yaml::Value::Sequence(seq) = v {
                                for (i, item) in seq.iter().enumerate() {
                                    if let serde_yaml::Value::String(name) = item {
                                        mapping.insert(name.to_lowercase(), i);
                                    }
                                }
                                found_fields = true;
                                break;
                            }
                        }
                    }
                }
                if found_fields {
                    return Some(mapping);
                }
                // Fallback: treat mapping keys as names and values as indices
                let mut ok = false;
                for (k, v) in map.iter() {
                    if let (serde_yaml::Value::String(name), serde_yaml::Value::Number(num)) = (k, v) {
                        if let Some(i) = num.as_u64() {
                            mapping.insert(name.to_lowercase(), i as usize);
                            ok = true;
                        }
                    }
                }
                if ok {
                    return Some(mapping);
                }
            }
            _ => {}
        }
    }
    None
}

/// Apply patches to the given DBC files and write modified versions into
/// the output directory.  Returns the list of paths written.  Called by
/// both the `apply` and `build` subcommands.
fn apply_command(
    dbc_files: &[PathBuf],
    patch_files: &[PathBuf],
    out_dir: &Path,
    schema_dir: &Path,
) -> Result<Vec<PathBuf>> {
    // Ensure output directory exists
    fs::create_dir_all(out_dir)
        .with_context(|| format!("Failed to create output directory {:?}", out_dir))?;

    // Load patch files and group them by DBC name
    let patches_map = load_patches(patch_files)?;

    // Keep track of written paths
    let mut written = Vec::new();

    for dbc_path in dbc_files {
        let file_name = dbc_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Invalid DBC file path: {:?}", dbc_path))?;
        println!("Processing {}", file_name);

        // Read the DBC
        let (header, mut records, mut string_block) = read_dbc(dbc_path)
            .with_context(|| format!("Failed to read DBC file {:?}", dbc_path))?;

        // Build string offset map for existing strings
        let mut string_map = build_string_map(&string_block);
        // Keep track of new strings appended (in order)
        let mut new_strings: Vec<String> = Vec::new();

        // Load a schema mapping for this DBC (if available)
        let schema_map = load_schema_map(schema_dir, &file_name);

        // Apply all patches matching this DBC name (case insensitive)
        let mut any_patch_applied = false;
        if let Some(patches_for_file) = patches_map.get(&file_name.to_lowercase()) {
            for pf in patches_for_file {
                // Determine the origin of this patch file for warnings
                let pf_origin = pf
                    .origin
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());
                for change in &pf.changes {
                    match change {
                        PatchEntry::Update {
                            key,
                            key_column,
                            values,
                        } => {
                            let key_col_index = resolve_key_column_index(key_column, &schema_map, &file_name, &pf_origin);

                            // Find the record with matching key
                            let mut found = false;
                            for record in &mut records {
                                if key_col_index >= record.len() {
                                    continue;
                                }
                                if record[key_col_index] == *key {
                                    found = true;
                                    apply_values_to_record(
                                        values,
                                        record,
                                        &schema_map,
                                        &mut string_map,
                                        &mut new_strings,
                                        &string_block,
                                        &file_name,
                                        &pf_origin,
                                        *key,
                                    );
                                    break;
                                }
                            }
                            if !found {
                                println!(
                                    "Warning: no record found with key {} in {} (patch file: {})",
                                    key,
                                    file_name,
                                    pf_origin
                                );
                            }
                        }
                        PatchEntry::Insert { key, key_column, values } => {
                            let key_col_index = resolve_key_column_index(key_column, &schema_map, &file_name, &pf_origin);

                            // Create new record filled with zeros
                            let mut new_record = vec![0u32; header.field_count as usize];

                            // If a key is provided and the field is not explicitly set in values, write it to the key column
                            if let Some(k) = key {
                                let provided_key = values.keys().any(|field_name| {
                                    // Determine if this field matches the key column
                                    if let Ok(idx) = field_name.parse::<usize>() {
                                        idx == key_col_index
                                    } else {
                                        schema_map
                                            .as_ref()
                                            .and_then(|schema| schema.get(&field_name.to_lowercase()))
                                            .map_or(false, |&idx| idx == key_col_index)
                                    }
                                });
                                if key_col_index < new_record.len() && !provided_key {
                                    // `key` is a reference when matching on &PatchEntry; dereference it
                                    new_record[key_col_index] = *k;
                                }
                            }

                            // Fill in specified fields from the values map
                            let effective_key = key.unwrap_or(0); // Use a default key for apply_values_to_record
                            apply_values_to_record(
                                values,
                                &mut new_record,
                                &schema_map,
                                &mut string_map,
                                &mut new_strings,
                                &string_block,
                                &file_name,
                                &pf_origin,
                                effective_key,
                            );

                            // Check for duplicate keys: if the key value in the new record already exists in the
                            // records list at the same key column, warn and skip this insert.
                            if key_col_index < new_record.len() {
                                let new_key_val = new_record[key_col_index];
                                if records.iter().any(|r| {
                                    if key_col_index < r.len() {
                                        r[key_col_index] == new_key_val
                                    } else {
                                        false
                                    }
                                }) {
                                    println!(
                                        "Warning: record with key {} already exists in {} (patch file: {}) – skipping insert",
                                        new_key_val,
                                        file_name,
                                        pf_origin
                                    );
                                    // Do not push the duplicate record
                                } else {
                                    records.push(new_record);
                                }
                            } else {
                                // If the key column is out of bounds, just append the record (no duplicate check)
                                records.push(new_record);
                            }
                        }
                        PatchEntry::Copy {
                            key,
                            key_column,
                            values,
                        } => {
                            let key_col_index = resolve_key_column_index(key_column, &schema_map, &file_name, &pf_origin);
                            // Find the record to copy
                            let mut found = false;
                            for record in &records {
                                if key_col_index >= record.len() {
                                    continue;
                                }
                                if record[key_col_index] == *key {
                                    found = true;
                                    // Clone the existing record
                                    let mut new_record = record.clone();
                                    // Apply updates to the new record
                                    apply_values_to_record(
                                        values,
                                        &mut new_record,
                                        &schema_map,
                                        &mut string_map,
                                        &mut new_strings,
                                        &string_block,
                                        &file_name,
                                        &pf_origin,
                                        *key,
                                    );
                                    // After applying updates, ensure we are not duplicating the key.  Use the
                                    // resolved key column to retrieve the new key value and check against
                                    // existing records.  If a duplicate is found, skip adding the new record and
                                    // warn.  Otherwise, push it to the list.
                                    if key_col_index < new_record.len() {
                                        let new_key_val = new_record[key_col_index];
                                        if records.iter().any(|r| {
                                            if key_col_index < r.len() {
                                                r[key_col_index] == new_key_val
                                            } else {
                                                false
                                            }
                                        }) {
                                            println!(
                                                "Warning: record with key {} already exists in {} (patch file: {}) – skipping copy",
                                                new_key_val,
                                                file_name,
                                                pf_origin
                                            );
                                        } else {
                                            records.push(new_record);
                                        }
                                    } else {
                                        // If the key column is out of bounds, append without duplicate check
                                        records.push(new_record);
                                    }
                                    break;
                                }
                            }
                            if !found {
                                println!(
                                    "Warning: no record found with key {} in {} (patch file: {}) to copy",
                                    key,
                                    file_name,
                                    pf_origin
                                );
                            }
                        }
                    }
                }
            }
            any_patch_applied = true;
        }

        // Build final string block by appending new strings
        if any_patch_applied {
            // Append all new strings to the original block
            for s in &new_strings {
                // Strings are stored as bytes followed by a null terminator
                string_block.extend_from_slice(s.as_bytes());
                string_block.push(0);
            }
        }

        // Build output path
        let out_path = out_dir.join(&file_name);
        write_dbc(&out_path, &header, &records, &string_block)
            .with_context(|| format!("Failed to write output DBC for {}", file_name))?;
        println!("Wrote {}", out_path.display());
        written.push(out_path);
    }

    Ok(written)
}

/// Build an MPQ archive after applying patches.  First calls
/// `apply_command` to produce the modified DBCs and then uses the
/// `wow_mpq` crate to create an archive.  If MPQ creation fails the
/// modified DBCs remain in the output directory.
fn build_command(
    dbc_files: &[PathBuf],
    patch_files: &[PathBuf],
    out_dir: &Path,
    mpq_path: &Path,
    mpq_version: u8,
    schema_dir: &Path,
    includes_dir: &Path,
) -> Result<()> {
    // Apply patches first.  The modified DBCs will be written into out_dir.
    let modified_paths = apply_command(dbc_files, patch_files, out_dir, schema_dir)?;

    // Collect the file names and archive paths
    // Start building the archive
    let mut builder = wow_mpq::ArchiveBuilder::new();
    // Set the version if provided
    let version = match mpq_version {
        1 => wow_mpq::FormatVersion::V1,
        2 => wow_mpq::FormatVersion::V2,
        3 => wow_mpq::FormatVersion::V3,
        4 => wow_mpq::FormatVersion::V4,
        _ => {
            println!("Warning: unknown MPQ version {}, defaulting to 2", mpq_version);
            wow_mpq::FormatVersion::V2
        }
    };
    builder = builder.version(version);

    // Add modified DBC files under DBFilesClient/
    for path in modified_paths {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid file name for {:?}", path))?;
        let archive_name = format!("DBFilesClient/{}", file_name);
        builder = builder.add_file(&path, &archive_name);
    }

    // Include additional files from includes_dir, preserving relative paths
    if includes_dir.exists() {
        // Recursively gather all files
        let mut stack: Vec<PathBuf> = vec![includes_dir.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    // Determine archive name by stripping the includes_dir prefix
                    let rel = path
                        .strip_prefix(includes_dir)
                        .unwrap_or(&path);
                    let mut dest = String::new();
                    for component in rel.components() {
                        let part = component.as_os_str().to_string_lossy();
                        if !dest.is_empty() {
                            dest.push('/');
                        }
                        dest.push_str(&part);
                    }
                    builder = builder.add_file(&path, &dest);
                }
            }
        }
    }

    // Build the archive
    builder
        .build(mpq_path)
        .with_context(|| format!("Failed to create MPQ at {:?}", mpq_path))?;
    println!("Created MPQ {}", mpq_path.display());
    Ok(())
}