//! LZ4 block decoder (the second pak compression scheme: header word 0x10 == 2).

pub fn decompress(src: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::with_capacity(expected);
    let mut i = 0usize;
    while i < src.len() {
        let token = src[i];
        i += 1;
        let mut literals = (token >> 4) as usize;
        if literals == 15 {
            loop {
                let b = *src.get(i).ok_or("lz4: truncated literal length")?;
                i += 1;
                literals += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        let lit = src.get(i..i + literals).ok_or("lz4: literals past end")?;
        out.extend_from_slice(lit);
        i += literals;
        if i >= src.len() {
            break; // the last sequence has no match
        }
        let offset = u16::from_le_bytes([src[i], *src.get(i + 1).ok_or("lz4: truncated offset")?]) as usize;
        i += 2;
        let mut len = (token & 15) as usize;
        if len == 15 {
            loop {
                let b = *src.get(i).ok_or("lz4: truncated match length")?;
                i += 1;
                len += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        len += 4;
        if offset == 0 || offset > out.len() {
            return Err(format!("lz4: bad match offset {offset} at output {}", out.len()));
        }
        let start = out.len() - offset;
        for k in 0..len {
            let b = out[start + k];
            out.push(b);
        }
    }
    if out.len() != expected {
        return Err(format!("lz4: produced {} bytes, expected {expected}", out.len()));
    }
    Ok(out)
}
