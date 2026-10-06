//! KPX archive reader (Angry Birds Go! `.pak` files).
//!
//! Layout (little endian), reverse-engineered from the v1.0.1 APK:
//!   0x00  magic bytes 01 'K' 'P' 'X'
//!   0x04  u32 version/flags (1 in most paks, 0x41 in cargeom.pak)
//!   0x08  u32 entry count
//!   0x0C  u32 name-table size in bytes
//!   0x30  entry records, 32 bytes each:
//!           +0  u32 name offset into the name table
//!           +4  u32 reserved
//!           +8  u32 uncompressed size
//!           +12 u32 absolute data offset
//!           +16 u32 compressed flag (1 = zlib)
//!           +20 u32 mtime (unix)
//!           +24 u32 compressed size
//!           +28 u32 reserved
//!   after the records: NUL-terminated names, then entry data.

use crate::inflate::zlib_decompress;

pub struct Entry {
    pub name: String,
    pub size: usize,
    pub offset: usize,
    pub compressed: bool,
    pub compressed_size: usize,
    pub lz4: bool,
}

fn u32_at(data: &[u8], at: usize) -> Result<u32, String> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("kpx: read past end at 0x{at:X}"))
}

fn u64_at(data: &[u8], at: usize) -> Result<u64, String> {
    data.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .ok_or_else(|| format!("kpx: read past end at 0x{at:X}"))
}

fn cstr_at(data: &[u8], at: usize) -> Result<String, String> {
    let bytes = data.get(at..).ok_or("kpx: name offset out of range")?;
    let len = bytes.iter().position(|&b| b == 0).ok_or("kpx: unterminated name")?;
    Ok(String::from_utf8_lossy(&bytes[..len]).into_owned())
}

/// Version 0x41 layout (cargeom.pak): a directory table at 0x40 followed by a flat
/// file table. Directory k is described by a 32-byte tuple
/// `(0, file_count_k, name_offset_{k+1}, first_file_{k+1})`; directory 0 starts at
/// name 0 / file 0 and the final tuple is only 16 bytes long. The flat table has
/// 32-byte records: `name u64, size u32, offset u32, flag u32, mtime u32, csize u64`.
fn parse_v64(data: &[u8]) -> Result<Vec<Entry>, String> {
    let count = u32_at(data, 8)? as usize;
    let mut dirs: Vec<(u64, usize, usize)> = Vec::new(); // (name offset, first file, file count)
    let (mut name_offset, mut first) = (0u64, 0usize);
    let mut k = 0usize;
    loop {
        let record = 0x40 + 32 * k;
        let files_here = u64_at(data, record + 8)? as usize;
        dirs.push((name_offset, first, files_here));
        first += files_here;
        if first >= count {
            break;
        }
        name_offset = u64_at(data, record + 16)?;
        if u64_at(data, record + 24)? as usize != first {
            return Err("kpx: directory table does not tile the file table".into());
        }
        k += 1;
    }
    if first != count {
        return Err(format!("kpx: directories cover {first} files, header says {count}"));
    }
    let file_table = 0x40 + 32 * k + 16;
    let names_start = file_table + 32 * count;

    let mut entries = Vec::with_capacity(count);
    for (dir_name_offset, first_file, file_count) in dirs {
        let dir_name = cstr_at(data, names_start + dir_name_offset as usize)?;
        for index in first_file..first_file + file_count {
            let record = file_table + 32 * index;
            let file_name = cstr_at(data, names_start + u64_at(data, record)? as usize)?;
            entries.push(Entry {
                name: format!("{dir_name}/{file_name}"),
                size: u32_at(data, record + 8)? as usize,
                offset: u32_at(data, record + 12)? as usize,
                compressed: u32_at(data, record + 16)? == 1,
                compressed_size: u64_at(data, record + 24)? as usize,
                lz4: false,
            });
        }
    }
    Ok(entries)
}

/// Archives from the 2.9.1 build: magic byte 2, header 0x34 bytes, then 32-byte records laid out like version 1.
/// Directory records (offset 0) may come first: the first record's size is the number of files at the root and each
/// directory's compressed-size field is its file count; files follow in order: root files, then each directory's files.
fn parse_v2(data: &[u8]) -> Result<Vec<Entry>, String> {
    const RECORDS: usize = 0x34;
    let names_size = u32_at(data, 12)? as usize;
    // the first non-zero data offset marks where the entry data (and so the end of the name table) begins
    let mut data_start = None;
    for k in 0..4096 {
        let offset = u32_at(data, RECORDS + 32 * k + 12)? as usize;
        if offset != 0 {
            data_start = Some(offset);
            break;
        }
    }
    let data_start = data_start.ok_or("kpx: no entry data")?;
    let names_start = data_start.checked_sub((names_size + 3) & !3).ok_or("kpx: name table larger than the data offset")?;
    if names_start < RECORDS || (names_start - RECORDS) % 32 != 0 {
        return Err(format!("kpx: record table does not end on a record boundary (names at 0x{names_start:X})"));
    }
    let records = (names_start - RECORDS) / 32;

    struct Rec {
        name: String,
        size: usize,
        offset: usize,
        compressed: bool,
        compressed_size: usize,
    }
    let mut dirs: Vec<(String, usize)> = Vec::new();
    let mut files: Vec<Rec> = Vec::new();
    for k in 0..records {
        let r = RECORDS + 32 * k;
        let name = cstr_at(data, names_start + u32_at(data, r)? as usize)?;
        let (size, offset) = (u32_at(data, r + 8)? as usize, u32_at(data, r + 12)? as usize);
        if offset == 0 {
            dirs.push((name, u32_at(data, r + 24)? as usize));
        } else {
            files.push(Rec { name, size, offset, compressed: u32_at(data, r + 16)? == 1, compressed_size: u32_at(data, r + 24)? as usize });
        }
    }
    let in_dirs: usize = dirs.iter().map(|d| d.1).sum();
    let root = files.len().checked_sub(in_dirs).ok_or("kpx: directories hold more files than exist")?;
    let mut out = Vec::with_capacity(files.len());
    let mut files = files.into_iter();
    for rec in files.by_ref().take(root) {
        out.push(Entry { name: rec.name, size: rec.size, offset: rec.offset, compressed: rec.compressed, compressed_size: rec.compressed_size, lz4: false });
    }
    for (dir, count) in dirs {
        for rec in files.by_ref().take(count) {
            out.push(Entry { name: format!("{dir}/{}", rec.name), size: rec.size, offset: rec.offset, compressed: rec.compressed, compressed_size: rec.compressed_size, lz4: false });
        }
    }
    Ok(out)
}

pub fn parse(data: &[u8]) -> Result<Vec<Entry>, String> {
    let mut entries = parse_any(data)?;
    // 2.9.1 archives: the header word at 0x10 names the compression scheme (1 = zlib, 2 = LZ4)
    if data.len() >= 0x14 && data[0] == 2 && u32_at(data, 0x10)? == 2 {
        for e in entries.iter_mut() {
            e.lz4 = true;
        }
    }
    Ok(entries)
}

fn parse_any(data: &[u8]) -> Result<Vec<Entry>, String> {
    if data.len() >= 0x34 && data[0] == 2 && &data[1..4] == b"KPX" {
        return parse_v2(data);
    }
    if data.len() < 0x30 || data[0] != 1 || &data[1..4] != b"KPX" {
        return Err("kpx: bad magic".into());
    }
    match u32_at(data, 4)? {
        1 => {}
        0x41 => return parse_v64(data),
        other => return Err(format!("kpx: unknown version 0x{other:X}")),
    }
    let count = u32_at(data, 8)? as usize;
    let names_start = 0x30 + 32 * count;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let record = 0x30 + 32 * index;
        let name_offset = u32_at(data, record)? as usize;
        let name_begin = names_start + name_offset;
        let name_bytes = data
            .get(name_begin..)
            .ok_or("kpx: name offset out of range")?;
        let name_len = name_bytes.iter().position(|&b| b == 0).ok_or("kpx: unterminated name")?;
        entries.push(Entry {
            name: String::from_utf8_lossy(&name_bytes[..name_len]).into_owned(),
            size: u32_at(data, record + 8)? as usize,
            offset: u32_at(data, record + 12)? as usize,
            compressed: u32_at(data, record + 16)? == 1,
            compressed_size: u32_at(data, record + 24)? as usize,
            lz4: false,
        });
    }
    Ok(entries)
}

pub fn read_entry(data: &[u8], entry: &Entry) -> Result<Vec<u8>, String> {
    let stored = if entry.compressed { entry.compressed_size } else { entry.size };
    let raw = data
        .get(entry.offset..entry.offset + stored)
        .ok_or_else(|| format!("kpx: data for {} out of range", entry.name))?;
    if !entry.compressed {
        return Ok(raw.to_vec());
    }
    let out = if entry.lz4 { crate::lz4::decompress(raw, entry.size) } else { zlib_decompress(raw, entry.size) }.map_err(|e| format!("{}: {e}", entry.name))?;
    if out.len() != entry.size {
        return Err(format!("{}: expected {} bytes, got {}", entry.name, entry.size, out.len()));
    }
    Ok(out)
}
