//! Small DEFLATE compressor: LZ77 (32 KiB window, hash chains) + fixed Huffman codes.
//! Dependency-free; typically 2-4x smaller than stored blocks on game art.

use crate::inflate::{DIST_BASE, DIST_EXTRA, LENGTH_BASE, LENGTH_EXTRA};

struct BitWriter {
    out: Vec<u8>,
    buf: u64,
    cnt: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, nbits: u32) {
        self.buf |= (value as u64) << self.cnt;
        self.cnt += nbits;
        while self.cnt >= 8 {
            self.out.push(self.buf as u8);
            self.buf >>= 8;
            self.cnt -= 8;
        }
    }

    /// Huffman codes are packed most-significant bit first.
    fn put_code(&mut self, code: u32, nbits: u32) {
        let mut reversed = 0;
        for i in 0..nbits {
            if (code >> i) & 1 == 1 {
                reversed |= 1 << (nbits - 1 - i);
            }
        }
        self.put(reversed, nbits);
    }

    fn put_symbol(&mut self, symbol: u32) {
        match symbol {
            0..=143 => self.put_code(0x30 + symbol, 8),
            144..=255 => self.put_code(0x190 + symbol - 144, 9),
            256..=279 => self.put_code(symbol - 256, 7),
            _ => self.put_code(0xC0 + symbol - 280, 8),
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.cnt > 0 {
            self.out.push(self.buf as u8);
        }
        self.out
    }
}

const WINDOW: usize = 32768;
const MAX_CHAIN: usize = 48;

fn hash3(data: &[u8], at: usize) -> usize {
    (((data[at] as u32) << 10) ^ ((data[at + 1] as u32) << 5) ^ data[at + 2] as u32) as usize & 0x7FFF
}

fn bucket(bases: &[u16], value: usize) -> usize {
    bases.iter().rposition(|&base| base as usize <= value).unwrap()
}

/// Raw DEFLATE stream (a single final fixed-Huffman block).
pub fn deflate_fixed(data: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter { out: Vec::with_capacity(data.len() / 2 + 16), buf: 0, cnt: 0 };
    writer.put(1, 1); // BFINAL
    writer.put(1, 2); // BTYPE = fixed Huffman

    let mut head = vec![-1i32; 1 << 15];
    let mut prev = vec![-1i32; WINDOW];
    let insert = |head: &mut [i32], prev: &mut [i32], pos: usize| {
        if pos + 3 <= data.len() {
            let h = hash3(data, pos);
            prev[pos & (WINDOW - 1)] = head[h];
            head[h] = pos as i32;
        }
    };

    let mut i = 0;
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + 3 <= data.len() {
            let mut candidate = head[hash3(data, i)];
            let mut tries = MAX_CHAIN;
            while candidate >= 0 && tries > 0 {
                let distance = i - candidate as usize;
                if distance > WINDOW {
                    break;
                }
                let mut len = 0;
                while len < 258 && i + len < data.len() && data[candidate as usize + len] == data[i + len] {
                    len += 1;
                }
                if len > best_len {
                    best_len = len;
                    best_dist = distance;
                    if len == 258 {
                        break;
                    }
                }
                candidate = prev[candidate as usize & (WINDOW - 1)];
                tries -= 1;
            }
        }

        if best_len >= 3 {
            let l = bucket(&LENGTH_BASE, best_len);
            writer.put_symbol(257 + l as u32);
            writer.put((best_len - LENGTH_BASE[l] as usize) as u32, LENGTH_EXTRA[l] as u32);
            let d = bucket(&DIST_BASE, best_dist);
            writer.put_code(d as u32, 5);
            writer.put((best_dist - DIST_BASE[d] as usize) as u32, DIST_EXTRA[d] as u32);
            for pos in i..i + best_len {
                insert(&mut head, &mut prev, pos);
            }
            i += best_len;
        } else {
            writer.put_symbol(data[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    writer.put_symbol(256);
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inflate::inflate;

    #[test]
    fn round_trips_repetitive_and_noisy_data() {
        let mut sample = Vec::new();
        for n in 0..20000u32 {
            sample.extend_from_slice(&[(n % 7) as u8, (n % 251) as u8, 0, 0, 255, (n / 300) as u8]);
        }
        let mut seed = 12345u32;
        for _ in 0..5000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            sample.push((seed >> 24) as u8);
        }
        let packed = deflate_fixed(&sample);
        assert!(packed.len() < sample.len());
        assert_eq!(inflate(&packed, sample.len()).unwrap(), sample);
    }

    #[test]
    fn round_trips_empty_and_tiny() {
        for data in [&b""[..], b"a", b"ab", b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"] {
            assert_eq!(inflate(&deflate_fixed(data), data.len()).unwrap(), data);
        }
    }
}
