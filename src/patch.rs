use serde::Deserialize;
use std::collections::HashMap;

/// Top level structure for a patch file.  A patch targets a single DBC
/// table and contains a list of individual changes.  The DBC path is used
/// purely for identification; the caller decides which patch applies to
/// which file based on file name matching.
#[derive(Debug, Deserialize)]
pub struct PatchFile {
    /// Name of the DBC this patch is intended for (e.g. `Spell.dbc`).
    pub dbc: String,
    /// A list of changes to apply.  Each change may update an existing
    /// record or insert a new one.
    pub changes: Vec<PatchEntry>,

    /// Optional path to the patch file this patch was loaded from.  This is
    /// not populated by the YAML parser (hence `serde(skip)`) but filled
    /// in by the loader so warnings can reference the source file.
    #[serde(skip)]
    pub origin: Option<std::path::PathBuf>,
}

/// A single patch entry.  Serialized using an internal tagging strategy so
/// that entries can be either `update` or `insert` variants.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PatchEntry {
    /// Modify fields of an existing record identified by a key.  The key is
    /// looked up in the record by the `key_column` (defaults to column 0).
    Update {
        /// Key value used to find the record to modify.  It is assumed that
        /// the key column holds a 32‑bit integer identifier.
        key: u32,
        /// Column containing the key.  You can specify either a field
        /// name or a numeric index.  If omitted the first field (column 0)
        /// is assumed.
        #[serde(default)]
        key_column: Option<String>,
        /// Mapping of field names (or indices in string form) to new values.
        /// The index mapping will be resolved at runtime against the
        /// provided schema.  Fields not found in the schema are ignored
        /// with a warning.
        updates: HashMap<String, ValueType>,
    },
    /// Insert a completely new record.  Only the fields listed in
    /// `values` will be set; unspecified fields default to zero.  When
    /// inserting a string value the writer will append the string to
    /// the string block and store its offset as the field value.
    Insert {
        /// Optional key value for the new record.  If specified the value
        /// will be written into the key column (defaults to 0) unless an
        /// explicit value for that field is provided in `values`.
        #[serde(default)]
        key: Option<u32>,
        /// Column containing the key.  May be a field name or numeric index.
        #[serde(default)]
        key_column: Option<String>,
        /// Mapping of field names (or indices) to values for the new record.
        values: HashMap<String, ValueType>,
    },
    /// Copy an existing record identified by a key into a new record,
    /// then apply field updates.  The key lookup works like Update: the
    /// key is matched in the specified key_column (defaults to column 0).
    /// The new record starts as an exact copy of the matched record and
    /// only the provided fields are modified.
    Copy {
        /// Key value used to find the record to copy.
        key: u32,
        /// Column containing the key.  May be a field name or numeric string.
        #[serde(default)]
        key_column: Option<String>,
        /// Mapping of field names (or indices) to new values for the copied record.
        updates: HashMap<String, ValueType>,
    },
}


/// Values in patches are represented by an untagged enum.  Supported
/// primitives include signed and unsigned integers, floating point numbers,
/// booleans and strings.  When a string is specified the writer will
/// allocate a new entry in the DBC string block and replace the field with
/// the offset to the string.  Floats and booleans are currently parsed but
/// will be truncated to integers because the underlying simple DBC writer
/// assumes 32‑bit integers.  Extending the type system and record writer
/// to honour floats and booleans is left as a future exercise.
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum ValueType {
    Int(i64),
    UInt(u64),
    Float(f64),
    Bool(bool),
    String(String),
}

impl ValueType {
    /// Convert this `ValueType` into a u32 suitable for storage in the DBC
    /// record.  Floats are truncated, booleans become 0 or 1 and strings
    /// cannot be directly converted (the caller must handle string
    /// allocation and supply the resulting offset instead).
    pub fn as_u32(&self) -> Option<u32> {
        match *self {
            ValueType::Int(v) if v >= 0 && v <= u32::MAX as i64 => Some(v as u32),
            ValueType::UInt(v) if v <= u32::MAX as u64 => Some(v as u32),
            ValueType::Float(v) => {
                // Interpret floats as 32‑bit IEEE‐754 values.  The caller
                // ensures that only float‑compatible fields receive this
                // conversion.  Values outside the f32 range will be clamped.
                let f = v as f32;
                Some(f.to_bits())
            }
            ValueType::Bool(b) => Some(if b { 1 } else { 0 }),
            // Strings are not directly representable as integers; return None
            ValueType::String(_) => None,
            _ => None,
        }
    }
}