//! XGST (`.xgt`) texture decoder.
//!
//! 32-byte header (little endian):
//!   0x00  "XGST"
//!   0x04  u32 version: 0x001A0920 (1.0.1) or 0x001C0020 (2.9.x)
//!   0x08  u8  mip level count
//!   0x0C  u8  pixel format. 1.0.1: 0x02 = RGBA4444, 0x03 = RGBA8888, 0x0D = luminance+alpha, 0xFC = ETC1.
//!             2.9.x: the `EXGSBaseTexFormat` enum, see `base`.
//!   0x10  u16 width, u16 height   (allocated texture size)
//!   0x14  u16 width, u16 height   (visible/used size)
//!   0x1C  u32 payload size
//!   0x20  payload: mip 0 first, then each smaller mip

/// Header version word of the 2.9.x build (1.0.1 used 0x001A0920). The pixel-format byte is the EXGSBaseTexFormat enum
/// (see `base`); `TXGSTexture_FileHandlerXGT::Load` maps the old 0x1A/0x1B header versions onto it.
pub const VERSION_2_9: u32 = 0x001C0020;

pub struct Xgt<'a> {
    pub version: u32,
    pub mips: u8,
    pub format: u8,
    pub width: usize,
    pub height: usize,
    pub used_width: usize,
    pub used_height: usize,
    pub payload: &'a [u8],
}

pub fn parse(data: &[u8]) -> Result<Xgt<'_>, String> {
    if data.len() < 32 || &data[0..4] != b"XGST" {
        return Err("xgt: bad magic".into());
    }
    let u16_at = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]) as usize;
    let payload_size = u32::from_le_bytes([data[28], data[29], data[30], data[31]]) as usize;
    let payload = data
        .get(32..32 + payload_size)
        .ok_or("xgt: payload size exceeds file")?;
    Ok(Xgt {
        version: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
        mips: data[8],
        format: data[12],
        width: u16_at(16),
        height: u16_at(18),
        used_width: u16_at(20),
        used_height: u16_at(22),
        payload,
    })
}

/// Nibble order for 16-bit pixels, listed from the most significant nibble down.
#[derive(Clone, Copy)]
pub struct Swizzle([usize; 4]);

impl Swizzle {
    /// `name` is any permutation of "rgba", e.g. "abgr" = top nibble is alpha.
    pub fn parse(name: &str) -> Result<Swizzle, String> {
        let mut order = [0usize; 4];
        if name.len() != 4 {
            return Err("swizzle must be 4 letters".into());
        }
        for (slot, ch) in name.chars().enumerate() {
            order[slot] = "rgba".find(ch).ok_or("swizzle letters must be r,g,b,a")?;
        }
        Ok(Swizzle(order))
    }
}

/// Base pixel formats (`EXGSBaseTexFormat`) of the 2.9.x build: the byte at header offset 12. Enum order taken from the
/// name table used by `XGSTex_GetBaseTextureFormatName` in libABK291 (index = value).
pub mod base {
    pub const R5G6B5: u8 = 0x01;
    pub const R5G5B5A1: u8 = 0x02;
    pub const R4G4B4A4: u8 = 0x03;
    pub const R8G8B8A8: u8 = 0x04;
    pub const R8G8B8: u8 = 0x05;
    pub const R8G8: u8 = 0x06;
    pub const L8: u8 = 0x07;
    pub const L8A8: u8 = 0x08;
    pub const A8: u8 = 0x0C;
    pub const DXT1: u8 = 0x18;
    pub const DXT3: u8 = 0x19;
    pub const DXT5: u8 = 0x1A;
    pub const PVRTC4_RGB: u8 = 0x1E;
    pub const PVRTC4_RGBA: u8 = 0x1F;
    pub const ETC1: u8 = 0x23;
    pub const ATC_RGB: u8 = 0x25;
    pub const ATC_RGBA_INT: u8 = 0x27;
}

/// Name of a 2.9 base format code (for `xgt-info`).
pub fn base_format_name(code: u8) -> &'static str {
    const NAMES: [&str; 54] = [
        "Invalid", "R5G6B5", "R5G5B5A1", "R4G4B4A4", "R8G8B8A8", "R8G8B8", "R8G8", "L8", "L8A8", "L4", "A4", "L4A4", "A8", "IDX4",
        "IDX8", "D16", "D24S8", "D24X8", "D24", "D32", "R32_F", "R16G16B16A16_F", "R16G16_F", "R16_F", "DXT1", "DXT3", "DXT5", "BC7",
        "PVRTC2_RGB", "PVRTC2_RGBA", "PVRTC4_RGB", "PVRTC4_RGBA", "R5G5B5A3", "CMPR", "3DSProcedural", "ETC1", "ETCA4", "ATC_RGB",
        "ATC_RGBA_EXP", "ATC_RGBA_INT", "R32G32_F", "R32G32", "R32G32B32A32_F", "R32G32B32A32", "S8", "R16G16", "R16G16B16A16",
        "R32", "R16", "R10G10B10A2", "R11G11B10_F", "R9E5G9E5B9E5_F", "ETC2_RGB", "ETC2_RGBA",
    ];
    NAMES.get(code as usize).copied().unwrap_or("?")
}

fn put(rgba: &mut [u8], w: usize, h: usize, x: usize, y: usize, px: [u8; 4]) {
    if x < w && y < h {
        let at = (y * w + x) * 4;
        rgba[at..at + 4].copy_from_slice(&px);
    }
}

fn expand565(c: u16) -> [u32; 3] {
    [((c >> 11) & 31) as u32 * 255 / 31, ((c >> 5) & 63) as u32 * 255 / 63, (c & 31) as u32 * 255 / 31]
}

/// Colour half of a BC1/BC2/BC3 block (8 bytes). `punch` = DXT1 (c0 <= c1 selects 3 colours + transparent black);
/// DXT3/DXT5 always use the 4-colour mode.
fn bc_colours(block: &[u8], punch: bool) -> ([[u8; 4]; 4], u32) {
    let (c0, c1) = (u16::from_le_bytes([block[0], block[1]]), u16::from_le_bytes([block[2], block[3]]));
    let (a, b) = (expand565(c0), expand565(c1));
    let mut palette = [[0u8; 4]; 4];
    palette[0] = [a[0] as u8, a[1] as u8, a[2] as u8, 255];
    palette[1] = [b[0] as u8, b[1] as u8, b[2] as u8, 255];
    if c0 > c1 || !punch {
        for k in 0..3 {
            palette[2][k] = ((2 * a[k] + b[k]) / 3) as u8;
            palette[3][k] = ((a[k] + 2 * b[k]) / 3) as u8;
        }
        palette[2][3] = 255;
        palette[3][3] = 255;
    } else {
        for k in 0..3 {
            palette[2][k] = ((a[k] + b[k]) / 2) as u8;
        }
        palette[2][3] = 255;
        palette[3] = [0, 0, 0, 0];
    }
    (palette, u32::from_le_bytes([block[4], block[5], block[6], block[7]]))
}

/// BC1 / BC2 / BC3 (DXT1 / DXT3 / DXT5). `kind` is 1, 3 or 5.
fn decode_bc(x: &Xgt, rgba: &mut [u8], kind: u8) -> Result<(), String> {
    let (blocks_x, blocks_y) = (x.width.div_ceil(4), x.height.div_ceil(4));
    let size = if kind == 1 { 8 } else { 16 };
    let src = x.payload.get(..blocks_x * blocks_y * size).ok_or("xgt: short DXT payload")?;
    for (index, block) in src.chunks_exact(size).enumerate() {
        let (bx, by) = (index % blocks_x, index / blocks_x);
        let (palette, indices) = bc_colours(&block[size - 8..], kind == 1);
        let mut alpha = [255u8; 16];
        if kind == 3 {
            for (i, a) in alpha.iter_mut().enumerate() {
                *a = ((block[i / 2] >> (4 * (i % 2))) & 0xF) * 17;
            }
        } else if kind == 5 {
            let (a0, a1) = (block[0] as u32, block[1] as u32);
            let mut table = [0u32; 8];
            table[0] = a0;
            table[1] = a1;
            if a0 > a1 {
                for k in 1..7u32 {
                    table[k as usize + 1] = ((7 - k) * a0 + k * a1) / 7;
                }
            } else {
                for k in 1..5u32 {
                    table[k as usize + 1] = ((5 - k) * a0 + k * a1) / 5;
                }
                table[6] = 0;
                table[7] = 255;
            }
            let mut bits = 0u64;
            for k in 0..6 {
                bits |= (block[2 + k] as u64) << (8 * k);
            }
            for (i, a) in alpha.iter_mut().enumerate() {
                *a = table[((bits >> (3 * i)) & 7) as usize] as u8;
            }
        }
        for py in 0..4usize {
            for px in 0..4usize {
                let i = py * 4 + px;
                let mut c = palette[((indices >> (2 * i)) & 3) as usize];
                if kind != 1 {
                    c[3] = alpha[i];
                }
                put(rgba, x.width, x.height, bx * 4 + px, by * 4 + py, c);
            }
        }
    }
    Ok(())
}

fn expand_bits(value: u32, bits: u32) -> u8 {
    let max = (1u32 << bits) - 1;
    ((value * 255 + max / 2) / max) as u8
}

/// Uncompressed 2.9 base formats. Packed 16-bit formats put the first-named channel in the top bits (R4G4B4A4 = 0xRGBA,
/// like GL_UNSIGNED_SHORT_4_4_4_4 / _5_5_5_1 / _5_6_5); byte formats are in memory order. Returns false for other formats.
fn decode_uncompressed(x: &Xgt, rgba: &mut [u8], swizzle: Swizzle) -> Result<bool, String> {
    let pixels = x.width * x.height;
    let need = |bytes: usize| x.payload.get(..pixels * bytes).ok_or_else(|| "xgt: short payload".to_string());
    match x.format {
        base::R4G4B4A4 => {
            for (i, t) in need(2)?.chunks_exact(2).enumerate() {
                let value = u16::from_le_bytes([t[0], t[1]]);
                for (n, &channel) in swizzle.0.iter().enumerate() {
                    rgba[i * 4 + channel] = (((value >> (12 - 4 * n)) & 0xF) as u8) * 17;
                }
            }
        }
        base::R5G5B5A1 => {
            for (i, t) in need(2)?.chunks_exact(2).enumerate() {
                let v = u16::from_le_bytes([t[0], t[1]]) as u32;
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[
                    expand_bits(v >> 11 & 31, 5),
                    expand_bits(v >> 6 & 31, 5),
                    expand_bits(v >> 1 & 31, 5),
                    if v & 1 != 0 { 255 } else { 0 },
                ]);
            }
        }
        base::R5G6B5 => {
            for (i, t) in need(2)?.chunks_exact(2).enumerate() {
                let v = u16::from_le_bytes([t[0], t[1]]) as u32;
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[expand_bits(v >> 11 & 31, 5), expand_bits(v >> 5 & 63, 6), expand_bits(v & 31, 5), 255]);
            }
        }
        base::R8G8B8A8 => rgba.copy_from_slice(need(4)?),
        base::R8G8B8 => {
            for (i, t) in need(3)?.chunks_exact(3).enumerate() {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[t[0], t[1], t[2], 255]);
            }
        }
        base::R8G8 => {
            for (i, t) in need(2)?.chunks_exact(2).enumerate() {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[t[0], t[1], 0, 255]);
            }
        }
        base::L8 => {
            for (i, t) in need(1)?.iter().enumerate() {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[*t, *t, *t, 255]);
            }
        }
        base::A8 => {
            for (i, t) in need(1)?.iter().enumerate() {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, *t]);
            }
        }
        base::L8A8 => {
            // two 8-bit channels; the SDF font sheets keep a distance field in each: rgb = first, alpha = second
            for (i, t) in need(2)?.chunks_exact(2).enumerate() {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[t[0], t[0], t[0], t[1]]);
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub fn decode_rgba(x: &Xgt, swizzle: Swizzle) -> Result<Vec<u8>, String> {
    let pixels = x.width * x.height;
    let mut rgba = vec![0u8; pixels * 4];
    if x.version == VERSION_2_9 {
        if decode_uncompressed(x, &mut rgba, swizzle)? {
            return Ok(rgba);
        }
        match x.format {
            base::DXT1 => decode_bc(x, &mut rgba, 1)?,
            base::DXT3 => decode_bc(x, &mut rgba, 3)?,
            base::DXT5 => decode_bc(x, &mut rgba, 5)?,
            base::ETC1 => decode_etc1(x, &mut rgba)?,
            other => return Err(format!("xgt: 2.9 pixel format 0x{other:02X} ({}) is not decoded", base_format_name(other))),
        }
        return Ok(rgba);
    }
    match x.format {
        0x02 => {
            let src = x.payload.get(..pixels * 2).ok_or("xgt: short 16-bit payload")?;
            for (i, texel) in src.chunks_exact(2).enumerate() {
                let value = u16::from_le_bytes([texel[0], texel[1]]);
                for (nibble_index, &channel) in swizzle.0.iter().enumerate() {
                    let nibble = ((value >> (12 - 4 * nibble_index)) & 0xF) as u8;
                    rgba[i * 4 + channel] = nibble * 17;
                }
            }
        }
        0x03 => {
            let src = x.payload.get(..pixels * 4).ok_or("xgt: short 32-bit payload")?;
            rgba.copy_from_slice(src);
        }
        0x0D => {
            // 8-bit luminance + 8-bit alpha (font glyph sheets).
            let src = x.payload.get(..pixels * 2).ok_or("xgt: short LA88 payload")?;
            for (i, texel) in src.chunks_exact(2).enumerate() {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[texel[0], texel[0], texel[0], texel[1]]);
            }
        }
        0xFC => decode_etc1(x, &mut rgba)?,
        other => return Err(format!("xgt: unknown pixel format 0x{other:02X}")),
    }
    Ok(rgba)
}

/// Decode a texture file straight from its bytes: (width, height, RGBA8 with the top row first). Handles every 1.0.1 format
/// and the 2.9 uncompressed / DXT / ETC1 formats; PVRTC / ATC are not decoded (use the `.xgt_dxt` / `.xgt_etc` sibling,
/// see `choose_variant`).
pub fn decode_bytes(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let x = parse(data)?;
    let rgba = decode_rgba(&x, Swizzle::parse("rgba")?)?;
    Ok((x.width as u32, x.height as u32, rgba))
}

/// Variant extensions of the 2.9 build in order of preference: the plain `.xgt` (when a texture has one it is its only
/// file), then DXT, ETC, and PVRTC / ATC (not decoded) last.
pub const VARIANT_EXTS: [&str; 5] = ["xgt", "xgt_dxt", "xgt_etc", "xgt_pvr", "xgt_atc"];

/// Preference rank of a file name (0 = best), `None` when it is not a texture file.
pub fn variant_rank(name: &str) -> Option<usize> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    VARIANT_EXTS.iter().position(|e| *e == ext)
}

/// Given the file names present (in a directory or pak), pick the one variant to use for texture `stem` (the name without
/// extension), e.g. `choose_variant(&names, "cars/x/dif")` -> `Some("cars/x/dif.xgt_dxt")`.
pub fn choose_variant<S: AsRef<str>>(names: &[S], stem: &str) -> Option<String> {
    names
        .iter()
        .map(|n| n.as_ref())
        .filter(|n| n.rsplit_once('.').is_some_and(|(s, _)| s == stem))
        .filter_map(|n| variant_rank(n).map(|r| (r, n)))
        .min()
        .map(|(_, n)| n.to_string())
}

const ETC1_MODIFIERS: [[i32; 2]; 8] =
    [[2, 8], [5, 17], [9, 29], [13, 42], [18, 60], [24, 80], [33, 106], [47, 183]];

fn expand5(v: i32) -> i32 {
    (v << 3) | (v >> 2)
}

fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn decode_etc1(x: &Xgt, rgba: &mut [u8]) -> Result<(), String> {
    let blocks_x = x.width.div_ceil(4);
    let blocks_y = x.height.div_ceil(4);
    let src = x.payload.get(..blocks_x * blocks_y * 8).ok_or("xgt: short ETC1 payload")?;
    for (index, block) in src.chunks_exact(8).enumerate() {
        let (bx, by) = (index % blocks_x, index / blocks_x);
        let differential = block[3] & 2 != 0;
        let flip = block[3] & 1 != 0;
        let tables = [((block[3] >> 5) & 7) as usize, ((block[3] >> 2) & 7) as usize];

        let mut base = [[0i32; 3]; 2];
        if differential {
            for c in 0..3 {
                let first = (block[c] >> 3) as i32;
                let delta = (((block[c] & 7) as i32) << 29) >> 29;
                base[0][c] = expand5(first);
                base[1][c] = expand5((first + delta).clamp(0, 31));
            }
        } else {
            for c in 0..3 {
                base[0][c] = ((block[c] >> 4) as i32) * 17;
                base[1][c] = ((block[c] & 0xF) as i32) * 17;
            }
        }

        let msb = u16::from_be_bytes([block[4], block[5]]) as u32;
        let lsb = u16::from_be_bytes([block[6], block[7]]) as u32;
        for px in 0..4usize {
            for py in 0..4usize {
                let sub = if flip { (py >= 2) as usize } else { (px >= 2) as usize };
                let bit = px * 4 + py;
                let magnitude = ETC1_MODIFIERS[tables[sub]][((lsb >> bit) & 1) as usize];
                let modifier = if (msb >> bit) & 1 != 0 { -magnitude } else { magnitude };
                let c = |k: usize| clamp(base[sub][k] + modifier);
                put(rgba, x.width, x.height, bx * 4 + px, by * 4 + py, [c(0), c(1), c(2), 255]);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_choice() {
        let names = ["a/b.xgt_etc", "a/b.xgt_dxt", "a/b.xgt_pvr", "a/c.xgt"];
        assert_eq!(choose_variant(&names, "a/b").as_deref(), Some("a/b.xgt_dxt"));
        assert_eq!(choose_variant(&names, "a/c").as_deref(), Some("a/c.xgt"));
        assert_eq!(choose_variant(&names, "a/d"), None);
    }

    fn header(format: u8, w: u16, h: u16, payload: &[u8]) -> Vec<u8> {
        let mut d = b"XGST".to_vec();
        d.extend_from_slice(&VERSION_2_9.to_le_bytes());
        d.extend_from_slice(&[1, 0, 0, 0, format, 0, 0, 0]);
        for v in [w, h, w, h] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&[0; 4]);
        d.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        d.extend_from_slice(payload);
        d
    }

    #[test]
    fn rgba5551_and_dxt5() {
        // one pixel 0xF801 = R max, G 0, B 0, A set
        let (w, h, px) = decode_bytes(&header(base::R5G5B5A1, 1, 1, &0xF801u16.to_le_bytes())).unwrap();
        assert_eq!((w, h, &px[..]), (1, 1, &[255u8, 0, 0, 255][..]));
        // DXT5 block: alpha 255/255, colour c0=c1=pure red (0xF800), all indices 0
        let block = [255, 255, 0, 0, 0, 0, 0, 0, 0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0];
        let (_, _, px) = decode_bytes(&header(base::DXT5, 4, 4, &block)).unwrap();
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
    }
}
