//! XMAT (`shaders.xmat`): the CXGSMatLib material library of the 2.9.x build (`CXGSMatLib::LoadMatLib`, version 0x21).
//!
//! Stream layout (little endian, no alignment; verified on the 2.9.2 file: every byte up to the tail is accounted for):
//!   u32 version = 0x21, u32 = 2, u32 = 4 (checked by the engine against its own value), u32 scene count S (19)
//!   S x 64-byte NUL padded scene names (FEOutside, FEShowroom, theme002_run000 ...)
//!   S x scene record: u32 A, u32 B, then A + B bytes of lighting / fog parameters for that scene
//!   u32 M shader count, then M x { u32 size, u32 scene mask (bit i = used by scene i), `size` bytes }
//!       shader blob = 6 words {_, source length, source offset (0x18), _, _, _} + GLSL ES source text
//!   u32 material count, u32 (value 0x48), debuggable-constants block (12 bytes here)
//!   N x material item { u32 size, u32 scene mask, `size` bytes of record, S x u32 per-scene value }
//!   tail (~160 KB): index tables / data-bridge constants, not decoded here
//!
//! Material record (offsets from the first byte after the mask; every `*_off` is relative to the record start):
//!   +0x00 name_off, +0x08 u32 id hash,
//!   +0x20 constants_off, +0x30 samplers_off, +0x38 passes_off,
//!   +0x1B8 constant count, +0x1BC sampler count, +0x1C0 pass count
//!   constants: 0x58 bytes each {name_off, _, hash, _, u32 engine constant id (0xFFFF.. high bits when engine supplied),
//!       u32 type word (bytes: base type 4 = float, rows/cols, register), 16 x f32 default value at +0x18}
//!   samplers: 0x30 bytes each {name_off, _, hash, ... 9 more words (slot, filter / wrap codes: not decoded)}
//!   passes: 0x10 bytes each {name_off, _, hash, _} - technique names (ABG_ENV1_Track ... DepthPass, DepthPassFE, VelocityPass)
//!   then the NUL separated name strings and a u16 table of shader indices.

pub const VERSION: u32 = 0x21;

#[derive(Debug, Clone)]
pub struct Scene {
    pub name: String,
    /// first A bytes of the scene record (lighting/fog floats; layout is `CXGSMatLibSceneData`, not decoded)
    pub data: Vec<u8>,
    pub extra: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Shader {
    pub scene_mask: u32,
    pub header: [u32; 6],
    pub source: String,
}

impl Shader {
    pub fn is_vertex(&self) -> bool {
        self.source.contains("gl_Position")
    }
    pub fn is_fragment(&self) -> bool {
        self.source.contains("gl_FragColor") || self.source.contains("gl_FragData")
    }
}

#[derive(Debug, Clone)]
pub struct Constant {
    pub name: String,
    pub hash: u32,
    /// engine-side constant id; 0 = a material constant with the default below
    pub engine_id: u32,
    pub type_word: u32,
    pub default: [f32; 16],
}

#[derive(Debug, Clone)]
pub struct Sampler {
    pub name: String,
    pub hash: u32,
    /// the 12 raw words of the 0x30-byte entry (slot / filter / wrap codes are not decoded)
    pub words: [u32; 12],
}

#[derive(Debug, Clone)]
pub struct Pass {
    pub name: String,
    pub hash: u32,
}

#[derive(Debug, Clone)]
pub struct Material {
    pub name: String,
    pub hash: u32,
    pub scene_mask: u32,
    pub constants: Vec<Constant>,
    pub samplers: Vec<Sampler>,
    pub passes: Vec<Pass>,
    pub per_scene: Vec<u32>,
    /// the 0x1E0-byte header as words, for fields that are not decoded (blend / cull / depth state lives somewhere in here
    /// and in the sampler words - not proven)
    pub header: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct XMat {
    pub scenes: Vec<Scene>,
    pub shaders: Vec<Shader>,
    pub materials: Vec<Material>,
    /// file offset where the decoded part ends
    pub end: usize,
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.d.get(self.p..self.p + 4).ok_or_else(|| format!("xmat: read past end at 0x{:X}", self.p))?;
        self.p += 4;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let b = self.d.get(self.p..self.p.checked_add(n).ok_or("xmat: size overflow")?).ok_or_else(|| format!("xmat: {n} bytes past end at 0x{:X}", self.p))?;
        self.p += n;
        Ok(b)
    }
}

fn word(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])).ok_or_else(|| format!("xmat: record read past end at +0x{at:X}"))
}

fn cstr(b: &[u8], at: usize) -> Result<String, String> {
    let s = b.get(at..).ok_or_else(|| format!("xmat: string offset 0x{at:X} outside record"))?;
    let n = s.iter().position(|&c| c == 0).ok_or("xmat: unterminated string")?;
    Ok(String::from_utf8_lossy(&s[..n]).into_owned())
}

pub fn parse(data: &[u8]) -> Result<XMat, String> {
    let mut r = Reader { d: data, p: 0 };
    let version = r.u32()?;
    if version != VERSION {
        return Err(format!("xmat: version 0x{version:X}, expected 0x{VERSION:X}"));
    }
    let _two = r.u32()?;
    let _four = r.u32()?;
    let scene_count = r.u32()? as usize;
    if scene_count > 64 {
        return Err("xmat: implausible scene count".into());
    }
    let mut names = Vec::new();
    for _ in 0..scene_count {
        let b = r.bytes(64)?;
        names.push(cstr(b, 0)?);
    }
    let mut scenes = Vec::new();
    for name in names {
        let a = r.u32()? as usize;
        let b = r.u32()? as usize;
        let data = r.bytes(a)?.to_vec();
        let extra = r.bytes(b)?.to_vec();
        scenes.push(Scene { name, data, extra });
    }
    let shader_count = r.u32()? as usize;
    let mut shaders = Vec::with_capacity(shader_count.min(1 << 16));
    for _ in 0..shader_count {
        let size = r.u32()? as usize;
        let scene_mask = r.u32()?;
        let blob = r.bytes(size)?;
        if size < 0x18 {
            return Err("xmat: shader blob too small".into());
        }
        let mut header = [0u32; 6];
        for (i, h) in header.iter_mut().enumerate() {
            *h = word(blob, 4 * i)?;
        }
        let (len, off) = (header[1] as usize, header[2] as usize);
        let text = blob.get(off..off + len).ok_or("xmat: shader source out of range")?;
        let text = text.strip_suffix(&[0]).unwrap_or(text);
        shaders.push(Shader { scene_mask, header, source: String::from_utf8_lossy(text).into_owned() });
    }
    let material_count = r.u32()? as usize;
    let _unknown = r.u32()?; // 0x48
    r.bytes(12)?; // CXGSDebuggableConsts::Load block (empty in the shipped file)
    let mut materials = Vec::with_capacity(material_count.min(1 << 16));
    for _ in 0..material_count {
        let size = r.u32()? as usize;
        let scene_mask = r.u32()?;
        let rec = r.bytes(size)?;
        let mut per_scene = Vec::new();
        for _ in 0..scene_count {
            per_scene.push(r.u32()?);
        }
        let name = cstr(rec, word(rec, 0)? as usize).map_err(|e| format!("{e} (material name, record size 0x{size:X} at item {})", materials.len()))?;
        let hash = word(rec, 8)?;
        let (consts_off, samplers_off, passes_off) = (word(rec, 0x20)? as usize, word(rec, 0x30)? as usize, word(rec, 0x38)? as usize);
        let (nc, ns, np) = (word(rec, 0x1B8)? as usize, word(rec, 0x1BC)? as usize, word(rec, 0x1C0)? as usize);
        if nc > 1024 || ns > 1024 || np > 1024 {
            return Err(format!("xmat: material {name}: implausible counts {nc}/{ns}/{np}"));
        }
        let mut constants = Vec::new();
        for i in 0..nc {
            let e = consts_off + i * 0x58;
            let mut default = [0f32; 16];
            for (k, d) in default.iter_mut().enumerate() {
                *d = f32::from_bits(word(rec, e + 0x18 + 4 * k)?);
            }
            constants.push(Constant {
                name: cstr(rec, word(rec, e)? as usize).map_err(|e| format!("{e} (constant {i} of {name})"))?,
                hash: word(rec, e + 8)?,
                engine_id: word(rec, e + 0x10)?,
                type_word: word(rec, e + 0x14)?,
                default,
            });
        }
        let mut samplers = Vec::new();
        for i in 0..ns {
            let e = samplers_off + i * 0x30;
            let mut words = [0u32; 12];
            for (k, w) in words.iter_mut().enumerate() {
                *w = word(rec, e + 4 * k)?;
            }
            samplers.push(Sampler { name: cstr(rec, words[0] as usize).ok().filter(|n| n.bytes().all(|c| c.is_ascii_graphic() || c == b' ')).unwrap_or_default(), hash: words[2], words });
        }
        let mut passes = Vec::new();
        for i in 0..np {
            let e = passes_off + i * 0x10;
            passes.push(Pass { name: cstr(rec, word(rec, e)? as usize).map_err(|e| format!("{e} (pass {i} of {name})"))?, hash: word(rec, e + 8)? });
        }
        let header = (0..0x1E0 / 4).map(|k| word(rec, 4 * k)).collect::<Result<Vec<_>, _>>()?;
        materials.push(Material { name, hash, scene_mask, constants, samplers, passes, per_scene, header });
    }
    Ok(XMat { scenes, shaders, materials, end: r.p })
}

impl XMat {
    pub fn material(&self, name: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.name.eq_ignore_ascii_case(name))
    }
}

/// `abgtool xmat-info` text for one parsed library.
pub fn describe(x: &XMat, verbose: bool) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{} scenes, {} shaders ({} vertex, {} fragment), {} materials, decoded to byte 0x{:X}\n",
        x.scenes.len(),
        x.shaders.len(),
        x.shaders.iter().filter(|s| s.is_vertex()).count(),
        x.shaders.iter().filter(|s| s.is_fragment()).count(),
        x.materials.len(),
        x.end
    ));
    s.push_str(&format!("scenes: {}\n", x.scenes.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")));
    for m in &x.materials {
        s.push_str(&format!(
            "{} (hash {:08X}, scenes {:05X}): {} constants, {} samplers, passes [{}]\n",
            m.name,
            m.hash,
            m.scene_mask,
            m.constants.len(),
            m.samplers.len(),
            m.passes.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
        ));
        if verbose {
            for c in &m.constants {
                s.push_str(&format!("    const {:<24} engine_id {:08X} type {:08X} default {:?}\n", c.name, c.engine_id, c.type_word, &c.default[..4]));
            }
            for sm in &m.samplers {
                s.push_str(&format!("    sampler {:<22} words {:X?}\n", format!("{:?}", sm.name), &sm.words[3..]));
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage() {
        assert!(parse(&[1, 2, 3]).is_err());
    }

    #[test]
    fn parses_shipped_file_when_present() {
        let path = std::path::Path::new(r"C:\Users\Brady\Desktop\AngryBirdsGo\apk292\ex\assets\data\shaders.xmat");
        let Ok(data) = std::fs::read(path) else { return };
        let x = parse(&data).unwrap();
        assert_eq!((x.scenes.len(), x.shaders.len(), x.materials.len()), (19, 1585, 272));
        let m = x.material("ABG_ENV1_Track").unwrap();
        assert_eq!(m.constants.iter().find(|c| c.name == "Tex0_OffsetScale").unwrap().engine_id, 0x25);
    }
}
