//! Dependency-free zlib/DEFLATE decoder (RFC 1950/1951).
//! Complexity O(n) in output size; memory = output buffer only.

struct BitReader<'a> {
    src: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
}

impl<'a> BitReader<'a> {
    fn bits(&mut self, n: u32) -> Result<u32, String> {
        while self.cnt < n {
            let byte = *self.src.get(self.pos).ok_or("inflate: unexpected end of input")?;
            self.pos += 1;
            self.buf |= (byte as u32) << self.cnt;
            self.cnt += 8;
        }
        let value = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.cnt -= n;
        Ok(value)
    }

    fn align_to_byte(&mut self) {
        self.pos -= (self.cnt / 8) as usize;
        self.buf = 0;
        self.cnt = 0;
    }

    fn decode(&mut self, table: &Huffman) -> Result<u16, String> {
        let (mut sum, mut cur, mut len) = (0i32, 0i32, 0usize);
        loop {
            cur = 2 * cur + self.bits(1)? as i32;
            len += 1;
            if len > 15 {
                return Err("inflate: invalid huffman code".into());
            }
            sum += table.counts[len] as i32;
            cur -= table.counts[len] as i32;
            if cur < 0 {
                break;
            }
        }
        table
            .symbols
            .get((sum + cur) as usize)
            .copied()
            .ok_or_else(|| "inflate: huffman symbol out of range".to_string())
    }
}

struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn build(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &length in lengths {
            counts[length as usize] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for i in 1..16 {
            offsets[i] = offsets[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[offsets[length as usize] as usize] = symbol as u16;
                offsets[length as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }
}

pub(crate) const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
pub(crate) const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
pub(crate) const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
pub(crate) const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CODE_LENGTH_ORDER: [usize; 19] =
    [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

fn inflate_block(
    br: &mut BitReader,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
) -> Result<(), String> {
    loop {
        let symbol = br.decode(lit)?;
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Ok(()),
            _ => {
                let idx = (symbol - 257) as usize;
                if idx >= 29 {
                    return Err("inflate: bad length symbol".into());
                }
                let length = LENGTH_BASE[idx] as usize + br.bits(LENGTH_EXTRA[idx] as u32)? as usize;
                let dsym = br.decode(dist)? as usize;
                if dsym >= 30 {
                    return Err("inflate: bad distance symbol".into());
                }
                let distance = DIST_BASE[dsym] as usize + br.bits(DIST_EXTRA[dsym] as u32)? as usize;
                if distance > out.len() {
                    return Err("inflate: distance beyond output start".into());
                }
                let start = out.len() - distance;
                for i in 0..length {
                    let byte = out[start + i];
                    out.push(byte);
                }
            }
        }
    }
}

pub fn inflate(src: &[u8], size_hint: usize) -> Result<Vec<u8>, String> {
    let mut br = BitReader { src, pos: 0, buf: 0, cnt: 0 };
    let mut out = Vec::with_capacity(size_hint);
    loop {
        let is_last = br.bits(1)? == 1;
        match br.bits(2)? {
            0 => {
                br.align_to_byte();
                let header = src.get(br.pos..br.pos + 4).ok_or("inflate: truncated stored block")?;
                let len = u16::from_le_bytes([header[0], header[1]]);
                let nlen = u16::from_le_bytes([header[2], header[3]]);
                if len != !nlen {
                    return Err("inflate: stored block length mismatch".into());
                }
                br.pos += 4;
                let body = src
                    .get(br.pos..br.pos + len as usize)
                    .ok_or("inflate: truncated stored data")?;
                out.extend_from_slice(body);
                br.pos += len as usize;
            }
            1 => {
                let mut lengths = [0u8; 288];
                lengths[..144].fill(8);
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                lengths[280..].fill(8);
                let lit = Huffman::build(&lengths);
                let dist = Huffman::build(&[5u8; 30]);
                inflate_block(&mut br, &mut out, &lit, &dist)?;
            }
            2 => {
                let hlit = br.bits(5)? as usize + 257;
                let hdist = br.bits(5)? as usize + 1;
                let hclen = br.bits(4)? as usize + 4;
                let mut code_lengths = [0u8; 19];
                for &slot in CODE_LENGTH_ORDER.iter().take(hclen) {
                    code_lengths[slot] = br.bits(3)? as u8;
                }
                let code_table = Huffman::build(&code_lengths);
                let total = hlit + hdist;
                let mut lengths = vec![0u8; total];
                let mut i = 0;
                while i < total {
                    let symbol = br.decode(&code_table)?;
                    let (value, repeat) = match symbol {
                        0..=15 => (symbol as u8, 1),
                        16 => {
                            if i == 0 {
                                return Err("inflate: repeat with no previous length".into());
                            }
                            (lengths[i - 1], 3 + br.bits(2)? as usize)
                        }
                        17 => (0, 3 + br.bits(3)? as usize),
                        _ => (0, 11 + br.bits(7)? as usize),
                    };
                    if i + repeat > total {
                        return Err("inflate: code length overflow".into());
                    }
                    lengths[i..i + repeat].fill(value);
                    i += repeat;
                }
                let lit = Huffman::build(&lengths[..hlit]);
                let dist = Huffman::build(&lengths[hlit..]);
                inflate_block(&mut br, &mut out, &lit, &dist)?;
            }
            _ => return Err("inflate: invalid block type".into()),
        }
        if is_last {
            return Ok(out);
        }
    }
}

pub fn zlib_decompress(src: &[u8], size_hint: usize) -> Result<Vec<u8>, String> {
    if src.len() < 2 || src[0] & 0x0F != 8 {
        return Err("zlib: bad header".into());
    }
    inflate(&src[2..], size_hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fixed_huffman_hello() {
        let stream = [0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x07, 0x00, 0x06, 0x2C, 0x02, 0x15];
        assert_eq!(zlib_decompress(&stream, 5).unwrap(), b"hello");
    }

    #[test]
    fn decodes_stored_block() {
        let stream = [0x78, 0x01, 0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c'];
        assert_eq!(zlib_decompress(&stream, 3).unwrap(), b"abc");
    }
}
