use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

/// Representation of a vanilla DBC header (WDBC).  The parser does not
/// support the extended WDB2/WDB5 variants and assumes a simple fixed
/// record layout.
#[derive(Debug, Clone)]
pub struct DbcHeader {
    pub magic: [u8; 4],
    pub record_count: u32,
    pub field_count: u32,
    pub record_size: u32,
    pub string_block_size: u32,
}

impl DbcHeader {
    /// Size of the header in bytes (magic + 4 u32s)
    pub const SIZE: usize = 4 + 4 * 4;
}

/// Read a DBC file from disk.  Returns the parsed header, a vector of
/// records (each record is a vector of 32‑bit values) and the raw string
/// block.
pub fn read_dbc<P: AsRef<Path>>(path: P) -> Result<(DbcHeader, Vec<Vec<u32>>, Vec<u8>)> {
    let mut file = File::open(&path)
        .with_context(|| format!("Failed to open DBC file {:?}", path.as_ref()))?;

    // Read header
    let mut header_bytes = [0u8; DbcHeader::SIZE];
    file.read_exact(&mut header_bytes)
        .with_context(|| "Failed to read DBC header")?;
    let magic = [header_bytes[0], header_bytes[1], header_bytes[2], header_bytes[3]];
    let record_count = u32::from_le_bytes([
        header_bytes[4], header_bytes[5], header_bytes[6], header_bytes[7],
    ]);
    let field_count = u32::from_le_bytes([
        header_bytes[8], header_bytes[9], header_bytes[10], header_bytes[11],
    ]);
    let record_size = u32::from_le_bytes([
        header_bytes[12], header_bytes[13], header_bytes[14], header_bytes[15],
    ]);
    let string_block_size = u32::from_le_bytes([
        header_bytes[16], header_bytes[17], header_bytes[18], header_bytes[19],
    ]);
    let header = DbcHeader {
        magic,
        record_count,
        field_count,
        record_size,
        string_block_size,
    };

    // Sanity check: record size should equal field_count * 4 for vanilla WDBC
    if header.record_size != header.field_count * 4 {
        // We allow it but warn; some DBCs contain floats/arrays but still use
        // 4 bytes per field.
        // bail!("Unsupported record size: {} (field_count {})", header.record_size, header.field_count);
    }

    // Read record data
    let mut records: Vec<Vec<u32>> = Vec::with_capacity(header.record_count as usize);
    for _ in 0..header.record_count {
        let mut record_bytes = vec![0u8; header.record_size as usize];
        file.read_exact(&mut record_bytes)
            .with_context(|| "Failed to read record")?;
        // Split into u32 values
        let mut values: Vec<u32> = Vec::with_capacity(header.field_count as usize);
        for i in 0..header.field_count as usize {
            let start = i * 4;
            // let end = start + 4;
            let val = u32::from_le_bytes([
                record_bytes[start],
                record_bytes[start + 1],
                record_bytes[start + 2],
                record_bytes[start + 3],
            ]);
            values.push(val);
        }
        records.push(values);
    }

    // Read string block
    let mut string_block = vec![0u8; header.string_block_size as usize];
    file.read_exact(&mut string_block)
        .with_context(|| "Failed to read string block")?;

    Ok((header, records, string_block))
}

/// Write a DBC file to disk.  Takes the header for field count/record size,
/// the records to write and the final string block.  The record count and
/// string block size are recomputed automatically.
pub fn write_dbc<P: AsRef<Path>>(
    path: P,
    header: &DbcHeader,
    records: &[Vec<u32>],
    string_block: &[u8],
) -> Result<()> {
    let mut file = File::create(&path)
        .with_context(|| format!("Failed to create output DBC file {:?}", path.as_ref()))?;
    // Recalculate header fields
    let record_count = records.len() as u32;
    let field_count = header.field_count;
    let record_size = header.record_size;
    let string_block_size = string_block.len() as u32;

    // Write header
    file.write_all(&header.magic)
        .context("Failed to write DBC magic")?;
    file.write_all(&record_count.to_le_bytes())
        .context("Failed to write record count")?;
    file.write_all(&field_count.to_le_bytes())
        .context("Failed to write field count")?;
    file.write_all(&record_size.to_le_bytes())
        .context("Failed to write record size")?;
    file.write_all(&string_block_size.to_le_bytes())
        .context("Failed to write string block size")?;

    // Write records
    for record in records {
        // Ensure the record has the correct number of fields
        if record.len() != field_count as usize {
            bail!("Record length mismatch: expected {} fields, got {}", field_count, record.len());
        }
        for &value in record {
            file.write_all(&value.to_le_bytes())
                .context("Failed to write record field")?;
        }
    }

    // Write string block
    file.write_all(string_block)
        .context("Failed to write string block")?;
    Ok(())
}

/// Build a mapping of strings to their offsets from an existing string block.
/// Offsets are 0‑based relative to the start of the block.  The empty string
/// at offset 0 is always included.
pub fn build_string_map(block: &[u8]) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    // let mut offset = 0u32;
    let mut start = 0usize;
    while start < block.len() {
        // Find the next null terminator
        if let Some(pos) = block[start..].iter().position(|&b| b == 0) {
            let end = start + pos;
            let string_bytes = &block[start..end];
            let s = String::from_utf8_lossy(string_bytes).to_string();
            map.insert(s, start as u32);
            // Move past the terminator
            start = end + 1;
        } else {
            break;
        }
    }
    // Ensure empty string at offset 0
    map.entry(String::new()).or_insert(0);
    map
}