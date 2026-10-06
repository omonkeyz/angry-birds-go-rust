//! XGSTA sprite-atlas descriptor (`*.atlas`).
//!
//! Layout (little endian), reverse-engineered from the v1.0.1 APK:
//!   0x00  "XGSTA" 00 00 05
//!   0x08  u32 version (1)
//!   0x0C  u32 sprite count
//!   0x10  u32 hash of the texture name
//!   0x14  32-byte NUL padded texture name
//!   0x34  sprite records, 40 bytes each:
//!           u32 name hash, u32 0,
//!           f32 x, f32 y, f32 w, f32 h   (fractions of the texture size)
//!           u32 w_px, u32 h_px, u32 0, u32 0
//!   then one u32 byte length followed by that many bytes of NUL-separated
//!   asset paths, one per sprite, in record order.

pub struct Sprite {
    pub name: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub struct Atlas {
    pub texture: String,
    pub sprites: Vec<Sprite>,
}

fn u32_at(data: &[u8], at: usize) -> Result<u32, String> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("atlas: read past end at 0x{at:X}"))
}

fn f32_at(data: &[u8], at: usize) -> Result<f32, String> {
    u32_at(data, at).map(f32::from_bits)
}

pub fn parse(data: &[u8], texture_width: u32, texture_height: u32) -> Result<Atlas, String> {
    if data.len() < 0x34 || &data[0..5] != b"XGSTA" {
        return Err("atlas: bad magic".into());
    }
    let count = u32_at(data, 0x0C)? as usize;
    let name_field = &data[0x14..0x34];
    let name_len = name_field.iter().position(|&b| b == 0).unwrap_or(name_field.len());
    let texture = String::from_utf8_lossy(&name_field[..name_len]).into_owned();

    let names_at = 0x34 + 40 * count;
    let names_len = u32_at(data, names_at)? as usize;
    let names_block = data
        .get(names_at + 4..names_at + 4 + names_len)
        .ok_or("atlas: name block past end")?;
    let mut names = names_block.split(|&b| b == 0);

    let mut sprites = Vec::with_capacity(count);
    for index in 0..count {
        let record = 0x34 + 40 * index;
        let x = (f32_at(data, record + 8)? * texture_width as f32).round() as u32;
        let y = (f32_at(data, record + 12)? * texture_height as f32).round() as u32;
        let w = u32_at(data, record + 24)?;
        let h = u32_at(data, record + 28)?;
        let name = String::from_utf8_lossy(names.next().ok_or("atlas: fewer names than sprites")?).into_owned();
        sprites.push(Sprite { name, x, y, w, h });
    }
    Ok(Atlas { texture, sprites })
}

pub fn to_json(atlas: &Atlas) -> String {
    let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut out = format!("{{\"texture\":\"{}\",\"sprites\":[", escape(&atlas.texture));
    for (i, s) in atlas.sprites.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":\"{}\",\"x\":{},\"y\":{},\"w\":{},\"h\":{}}}",
            escape(&s.name),
            s.x,
            s.y,
            s.w,
            s.h
        ));
    }
    out.push_str("]}");
    out
}
