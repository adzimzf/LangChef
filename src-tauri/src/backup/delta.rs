//! Delta file format, ported from `adzimzf/sqlite-incremental-backup` (`src/delta.rs`) unchanged
//! on disk: an 85-byte header followed by a zstd-compressed list of `[page_number | page]`.

use std::io::Read;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

pub const DELTA_MAGIC: &[u8; 8] = b"SQLIDELT";
pub const DELTA_VERSION: u32 = 1;
const HEADER_SIZE: usize = 8 + 4 + 8 + 4 + 4 + 4 + 32 + 1 + 8 + 8; // 85 bytes

#[derive(Debug, Clone)]
pub struct DeltaHeader {
    pub parent_backup_id: u64,
    /// Page count of the database the delta brings you to (used to truncate on restore).
    pub source_page_count: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeltaPageEntry {
    pub page_number: u32,
    pub data: Vec<u8>,
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

pub fn serialize_delta(
    pages: &[DeltaPageEntry],
    source_page_count: u32,
    page_size: u32,
    parent_id: u64,
    compression_level: i32,
) -> Result<Vec<u8>, String> {
    let mut payload = Vec::with_capacity(pages.len() * (4 + page_size as usize));
    for page in pages {
        payload.write_u32::<LittleEndian>(page.page_number).map_err(io)?;
        payload.extend_from_slice(&page.data);
    }
    let original_size = payload.len() as u64;
    let checksum = blake3::hash(&payload);

    let (body, alg) = if compression_level > 0 {
        let c = zstd::encode_all(payload.as_slice(), compression_level).map_err(io)?;
        (c, compression_level.clamp(1, 21) as u8)
    } else {
        (payload, 0u8)
    };

    let mut buf = Vec::with_capacity(HEADER_SIZE + body.len());
    buf.extend_from_slice(DELTA_MAGIC);
    buf.write_u32::<LittleEndian>(DELTA_VERSION).map_err(io)?;
    buf.write_u64::<LittleEndian>(parent_id).map_err(io)?;
    buf.write_u32::<LittleEndian>(source_page_count).map_err(io)?;
    buf.write_u32::<LittleEndian>(page_size).map_err(io)?;
    buf.write_u32::<LittleEndian>(pages.len() as u32).map_err(io)?;
    buf.extend_from_slice(checksum.as_bytes());
    buf.push(alg);
    buf.write_u64::<LittleEndian>(original_size).map_err(io)?;
    buf.write_u64::<LittleEndian>(body.len() as u64).map_err(io)?;
    buf.extend_from_slice(&body);
    Ok(buf)
}

pub fn parse_delta(data: &[u8]) -> Result<(DeltaHeader, Vec<DeltaPageEntry>), String> {
    let corrupt = |why: &str| format!("corrupt delta: {why}");
    if data.len() < HEADER_SIZE {
        return Err(corrupt("file too small for header"));
    }
    let mut cur = std::io::Cursor::new(data);
    let mut magic = [0u8; 8];
    cur.read_exact(&mut magic).map_err(io)?;
    if &magic != DELTA_MAGIC {
        return Err(corrupt("invalid magic"));
    }
    if cur.read_u32::<LittleEndian>().map_err(io)? != DELTA_VERSION {
        return Err(corrupt("unsupported version"));
    }
    let parent_backup_id = cur.read_u64::<LittleEndian>().map_err(io)?;
    let source_page_count = cur.read_u32::<LittleEndian>().map_err(io)?;
    let page_size = cur.read_u32::<LittleEndian>().map_err(io)?;
    let changed_page_count = cur.read_u32::<LittleEndian>().map_err(io)?;
    let mut sum = [0u8; 32];
    cur.read_exact(&mut sum).map_err(io)?;
    let mut alg = [0u8; 1];
    cur.read_exact(&mut alg).map_err(io)?;
    let original_size = cur.read_u64::<LittleEndian>().map_err(io)?;
    let compressed_size = cur.read_u64::<LittleEndian>().map_err(io)?;

    let body = &data[HEADER_SIZE..];
    if body.len() as u64 != compressed_size {
        return Err(corrupt("compressed size mismatch"));
    }
    let payload = match alg[0] {
        0 => body.to_vec(),
        1..=21 => zstd::decode_all(body).map_err(|e| corrupt(&e.to_string()))?,
        _ => return Err(corrupt("invalid compression byte")),
    };
    if blake3::hash(&payload) != blake3::Hash::from(sum) {
        return Err(corrupt("checksum mismatch"));
    }
    if payload.len() as u64 != original_size
        || payload.len() != changed_page_count as usize * (4 + page_size as usize)
    {
        return Err(corrupt("size mismatch"));
    }

    let mut pages = Vec::with_capacity(changed_page_count as usize);
    let mut cur = std::io::Cursor::new(&payload);
    for _ in 0..changed_page_count {
        let page_number = cur.read_u32::<LittleEndian>().map_err(io)?;
        let mut data = vec![0u8; page_size as usize];
        cur.read_exact(&mut data).map_err(io)?;
        pages.push(DeltaPageEntry { page_number, data });
    }
    Ok((
        DeltaHeader { parent_backup_id, source_page_count, page_size },
        pages,
    ))
}
