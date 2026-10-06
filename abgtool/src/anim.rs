//! Animation files (`.xga`) of the 2.9.x data.
//!
//! An `.xga` is NOT a separate "XGSA animation" format: it is an XGSM model container (the same file type as `.xgm`)
//! that carries animation chunks, loaded by `CXGSAnim::CXGSAnim(const char*)` -> `CXGSModel::InitModel`
//! (decompile: `CXGSModel::InitModel` @004993xx, `CXGS_XGMLoader::*`). `CXGSAnimation` / `CXGSAnimController` (magic
//! "XGSA", version 0x15, events) is a different engine path that none of the shipped `.xga` files use.
//!
//! Layout (little endian). 270 of the 271 shipped files use format version 0x01010126, one (sennahelmetbird_readytorace) 0x01010127:
//!   0x00 u32  low 16 bits = 0x15 (the loader checks only the low 16 bits; the high half is noise)
//!   0x04 u32  header size, must be 0x18
//!   0x08      "XGSM"
//!   0x0C u32  format version (CanLoadXGS maps 0x...26 / 27 / 28 to loader-table index 9 / 10 / 11)
//!   0x10 u32  bone count (`local_2f50`, array sizes only)
//!   0x14 u32  collision block count
//!   0x18      chunks: u32 id (only the low 16 bits are tested, the high half is noise), u32 size INCLUDING the
//!             8-byte chunk header. The stream ends with chunk 0x16 (size 8).
//!
//! Chunks used by animations (decompile names in brackets):
//!   0x1f extended header [LoadExtendedHeader]: u32 words. [2] f32 frame rate (this+0x18, 30.0), [3] f32 key stride
//!        (this+0xc, frames between dense samples), [1] helper count, [0] camera count, [6] (this+0xe8): when 1 the
//!        sampler negates matrix column 0 (m[0], m[4], m[8], m[12]), otherwise it swaps columns 1 and 2 (Y/Z); both fix-ups
//!        apply only to storage type 1 in the plain path, others kept raw.
//!   0x12 anim header [LoadAnimHeader_02] 0x14 bytes: u32 type, u32 count, u32 frames, u32 (runtime), u32 (unused)
//!   0x13 anim block  [LoadAnimBlock_04]: u32 skipped, then count x 0x3c-byte keys (type 1, dense, one track)
//!   0x1b physique header [LoadPhysiqueHeader_03]: 0x14 bytes u32 type, count, frames, (runtime), tracks; then
//!        u32[tracks] (meaning unknown, kept raw); type 2 adds tracks x 0x30 bytes (u32 key count at +0x1c)
//!   0x1c physique block [LoadPhysiqueBlock_04]: type 2: per track keycount x 0x3c keys then u16[frames] frame->key map;
//!        types 1/4: count x tracks x 0x3c keys, sample-major (record = sample * tracks + track)
//!   0x1e bone name (one chunk per track): u16 index, NUL terminated name (bytes after the NUL are stale)
//!   0x2d object name: u16, name. 0x11 model chunk: closes a group (the loader's model index advances here).
//!   0x25 hierarchy: u16 nodes, u16, u32, nodes x 0x48 bytes: u8 first child, u8 next sibling (0xff = none), u16,
//!        f32[16] (a static node matrix, scale 0.92 on the chuck rig: model-space bind, not the animated pose), u32.
//!   0x22 helper header (0x58 bytes: 0x20 name, u32 at +0x40 = sample limit) followed by 0x23 helper blocks:
//!        u16 sample index, u8 has-position, u8 has-rotation, u8 has-scale, then f32[3] / f32[4] (x, y, z negated by the
//!        loader) / f32[3]. Each shipped 0x23 holds exactly one of the three; the file has samples 0..=limit (the loader
//!        keeps only index < limit, so the 60 helpers with limit 1 keep only sample 0).
//!   0x30 PVS block, 0x17, 0x2a/0x2b/0x2c collision hull: kept as raw chunk references.
//!   No event / marker chunks (0x2e/0x2f markup) occur in any shipped .xga.
//! Extended header 0x1f in the shipped files: [0] cameras (0), [1] helper count, [2] 30.0 fps (24.0 in one file),
//! [3] key stride 1/2/4 frames, [6] 0 (not 1: the sampler's Y/Z swap branch, not the negate-X branch), [7] 0x0200001C /
//! 0x0200001E / 0x02000028 (unresolved flags).
//! Dense samples (storage 1/4): sample i sits at frame min(i * stride, frames).
//!
//! Key record (0x3c bytes, `TXGSAnimKeySampleTM`):
//!   +0x00 f32 1/(frame - previous key frame)  (the engine reads `next.+0` as the interpolation scale; 0 for key 0)
//!   +0x04 u16 frame number   +0x06 u8 flag   +0x07 u8 non-unit-scale flag
//!   +0x08 f32[3] scale   +0x14 f32[4] rotation xyzw   +0x24 f32[3] translation   +0x30 f32[3] (unresolved)
//! For format 0x01010126 the loader (`LoadPhysiqueBlock_04`, `LoadAnimBlock_04`) negates x, y, z of the stored
//! rotation after reading (the file holds the conjugate). `Key::rot` is the in-memory value the engine samples with.
//! Table entries 10 / 11 (0x27 / 0x28) keep `LoadAnimBlock_04` (0x13 blocks still negate) but use `LoadPhysiqueBlock_06`
//! (0x1c blocks are NOT negated). Only one shipped file uses 0x27, so this is decompile-only, not data-checked.

pub const KEY_SIZE: usize = 0x3C;
pub const FORMAT_V26: u32 = 0x0101_0126;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trs {
    pub scale: [f32; 3],
    /// quaternion x, y, z, w as the engine holds it after loading
    pub rot: [f32; 4],
    pub pos: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Key {
    pub inv_dt: f32,
    pub frame: u16,
    pub flag6: u8,
    pub flag7: u8,
    pub scale: [f32; 3],
    pub rot: [f32; 4],
    pub pos: [f32; 3],
    pub tail: [f32; 3],
}

impl Key {
    fn trs(&self) -> Trs {
        Trs { scale: self.scale, rot: self.rot, pos: self.pos }
    }
}

#[derive(Clone, Debug)]
pub struct Track {
    pub name: String,
    /// u16 index stored in the 0x1e name entry (equals the track position in every shipped file)
    pub node_index: u16,
    pub keys: Vec<Key>,
    /// storage type 2 only: frame number -> index into `keys` (u16 per frame)
    pub frame_map: Vec<u16>,
    /// storage type 2 only: the raw 0x30 byte track header as u32 words (only +0x1c, the key count, is non-zero)
    pub header_words: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct TrackSet {
    /// 0x1b/0x1c (physique) or 0x12/0x13 (anim)
    pub chunk_kind: u16,
    /// 1 = dense samples, 2 = sparse keys + per-frame map, 4 = dense samples (physique), 0 = matrices (unsupported)
    pub storage: u32,
    /// header word [1]: key total (type 2) / sample count (types 1, 4)
    pub count: u32,
    /// header word [2]: number of frames of the animation
    pub frames: u32,
    pub track_count: u32,
    /// u32[tracks] list in the 0x1b chunk (unknown meaning)
    pub bone_map: Vec<u32>,
    pub tracks: Vec<Track>,
}

impl TrackSet {
    /// True when the engine's plain (non-blended) path for this storage type swaps the Y and Z columns of the built
    /// matrix while ext word 6 != 1 (`GenerateAnimationData`: type 1 only; storage 2 and 4 never swap). All shipped
    /// files have ext word 6 == 0, so this is exactly the 80 prop (0x12/0x13) track sets.
    pub fn engine_swaps_yz(&self) -> bool {
        self.storage == 1
    }
    pub fn is_dense(&self) -> bool {
        self.storage == 1 || self.storage == 4
    }
}

#[derive(Clone, Debug)]
pub struct HierNode {
    /// the 0x48 raw bytes: u8 b0, u8 b1, u16 w2 (0xffff = none), f32[16] matrix, u32 tail
    pub raw: Vec<u8>,
}

impl HierNode {
    pub fn head(&self) -> [u8; 4] {
        [self.raw[0], self.raw[1], self.raw[2], self.raw[3]]
    }
    /// the 16 floats at +4 (row-major; translation looks to sit in the last row)
    pub fn matrix(&self) -> [f32; 16] {
        let mut m = [0f32; 16];
        for (i, v) in m.iter_mut().enumerate() {
            let o = 4 + i * 4;
            *v = f32::from_le_bytes([self.raw[o], self.raw[o + 1], self.raw[o + 2], self.raw[o + 3]]);
        }
        m
    }
    pub fn tail(&self) -> [u8; 4] {
        [self.raw[0x44], self.raw[0x45], self.raw[0x46], self.raw[0x47]]
    }
}

#[derive(Clone, Debug)]
pub struct Hierarchy {
    pub node_count: u16,
    pub aux16: u16,
    pub aux32: u32,
    pub nodes: Vec<HierNode>,
}

#[derive(Clone, Debug, Default)]
pub struct ChunkRef {
    pub id: u16,
    pub id_high: u16,
    pub offset: usize,
    pub size: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Group {
    pub index: usize,
    /// 0x2d object name
    pub name: Option<String>,
    pub track_set: Option<TrackSet>,
    pub hierarchy: Option<Hierarchy>,
    pub chunks: Vec<ChunkRef>,
}

#[derive(Clone, Debug)]
pub struct HelperSample {
    pub index: u16,
    pub pos: Option<[f32; 3]>,
    /// xyzw with x, y, z negated by the loader (`LoadHelperBlock_01`)
    pub rot: Option<[f32; 4]>,
    pub scale: Option<[f32; 3]>,
}

#[derive(Clone, Debug)]
pub struct Helper {
    pub name: String,
    pub header: Vec<u8>,
    /// u32 at +0x40 of the 0x58-byte header: the loader keeps only samples with index < this value
    pub limit: u32,
    /// number of 0x23 chunks that fed `samples` (consecutive chunks with the same index are merged: each carries only
    /// one of position / rotation / scale in the shipped files)
    pub blocks: usize,
    pub samples: Vec<HelperSample>,
}

#[derive(Clone, Debug)]
pub struct Anim {
    pub word0: u32,
    pub format_version: u32,
    pub bone_count: u32,
    pub collision_blocks: u32,
    /// raw u32 words of the 0x1f extended header
    pub ext: Vec<u32>,
    /// frames per second (ext word 2)
    pub fps: f32,
    /// frames between dense samples (ext word 3)
    pub key_stride: f32,
    /// number of frames (last track set that defined one; equal in every group of every shipped file)
    pub frames: u32,
    /// rotation xyz are negated on load in the physique (0x1c) blocks (format 0x26 only; 0x13 blocks always negate)
    pub negate_quat: bool,
    pub groups: Vec<Group>,
    pub helpers: Vec<Helper>,
    pub chunks: Vec<ChunkRef>,
    /// bytes after the 0x16 end chunk
    pub trailing: usize,
}

fn u16_at(d: &[u8], at: usize) -> Result<u16, String> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| format!("xga: read past end at 0x{at:X}"))
}
fn u32_at(d: &[u8], at: usize) -> Result<u32, String> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("xga: read past end at 0x{at:X}"))
}
fn f32_at(d: &[u8], at: usize) -> Result<f32, String> {
    u32_at(d, at).map(f32::from_bits)
}
fn cstr(d: &[u8]) -> String {
    let end = d.iter().position(|&b| b == 0).unwrap_or(d.len());
    String::from_utf8_lossy(&d[..end]).into_owned()
}

fn read_key(d: &[u8], at: usize, negate: bool) -> Result<Key, String> {
    let f = |o: usize| f32_at(d, at + o);
    let s = if negate { -1.0 } else { 1.0 };
    Ok(Key {
        inv_dt: f(0)?,
        frame: u16_at(d, at + 4)?,
        flag6: *d.get(at + 6).ok_or("xga: key past end")?,
        flag7: *d.get(at + 7).ok_or("xga: key past end")?,
        scale: [f(8)?, f(0xC)?, f(0x10)?],
        rot: [f(0x14)? * s, f(0x18)? * s, f(0x1C)? * s, f(0x20)?],
        pos: [f(0x24)?, f(0x28)?, f(0x2C)?],
        tail: [f(0x30)?, f(0x34)?, f(0x38)?],
    })
}

struct PendingSet {
    chunk_kind: u16,
    storage: u32,
    count: u32,
    frames: u32,
    track_count: u32,
    bone_map: Vec<u32>,
    track_headers: Vec<Vec<u32>>,
}

/// Parse an `.xga` file. Unknown chunks are kept as raw references, never rejected.
pub fn parse(d: &[u8]) -> Result<Anim, String> {
    if d.len() < 0x18 {
        return Err("xga: file too short".into());
    }
    let word0 = u32_at(d, 0)?;
    if word0 & 0xFFFF != 0x15 {
        return Err(format!("xga: bad first word 0x{word0:08X}"));
    }
    if u32_at(d, 4)? != 0x18 {
        return Err("xga: header size is not 0x18".into());
    }
    if &d[8..12] != b"XGSM" {
        return Err("xga: bad magic".into());
    }
    let format_version = u32_at(d, 12)?;
    if !(FORMAT_V26..=0x0101_0128).contains(&format_version) {
        return Err(format!("xga: unsupported format version 0x{format_version:08X}"));
    }
    // loader table entries 9..=11 all use LoadAnimBlock_04 (negates rotation xyz); the physique block is _04 (negates) for
    // 0x26 and _06 (does not) for 0x27 / 0x28
    let negate_quat = format_version == FORMAT_V26;
    let bone_count = u32_at(d, 16)?;
    let collision_blocks = u32_at(d, 20)?;

    let mut anim = Anim {
        word0,
        format_version,
        bone_count,
        collision_blocks,
        ext: Vec::new(),
        fps: 30.0,
        key_stride: 1.0,
        frames: 0,
        negate_quat,
        groups: Vec::new(),
        helpers: Vec::new(),
        chunks: Vec::new(),
        trailing: 0,
    };

    let mut group = Group::default();
    let mut pending: Option<PendingSet> = None;
    let mut names: Vec<(u16, String)> = Vec::new();
    let mut at = 0x18usize;
    let mut ended = false;

    // Close the current group: attach names, push, start the next one.
    fn close(anim: &mut Anim, group: &mut Group, names: &mut Vec<(u16, String)>) -> Result<(), String> {
        if let Some(ts) = group.track_set.as_mut() {
            if !names.is_empty() {
                if names.len() != ts.tracks.len() {
                    return Err(format!(
                        "xga: group {} has {} 0x1e names for {} tracks",
                        group.index,
                        names.len(),
                        ts.tracks.len()
                    ));
                }
                for (t, (idx, n)) in ts.tracks.iter_mut().zip(names.iter()) {
                    t.node_index = *idx;
                    t.name = n.clone();
                }
            } else if ts.tracks.len() == 1 {
                ts.tracks[0].name = group.name.clone().unwrap_or_default();
            }
            anim.frames = ts.frames;
        }
        names.clear();
        let next = group.index + 1;
        anim.groups.push(std::mem::take(group));
        group.index = next;
        Ok(())
    }

    while at + 8 <= d.len() {
        let id_full = u32_at(d, at)?;
        let size = u32_at(d, at + 4)? as usize;
        if size < 8 || at + size > d.len() {
            return Err(format!("xga: bad chunk size 0x{size:X} at 0x{at:X}"));
        }
        let id = (id_full & 0xFFFF) as u16;
        let cref = ChunkRef { id, id_high: (id_full >> 16) as u16, offset: at, size };
        let data = &d[at + 8..at + size];
        anim.chunks.push(cref.clone());
        group.chunks.push(cref);
        match id {
            0x1F => {
                anim.ext = (0..data.len() / 4).map(|i| u32_at(data, i * 4)).collect::<Result<_, _>>()?;
                if anim.ext.len() >= 4 {
                    anim.fps = f32::from_bits(anim.ext[2]);
                    anim.key_stride = f32::from_bits(anim.ext[3]);
                }
            }
            0x12 | 0x1B => {
                if data.len() < 0x14 {
                    return Err(format!("xga: chunk 0x{id:02X} too small at 0x{at:X}"));
                }
                let storage = u32_at(data, 0)?;
                let count = u32_at(data, 4)?;
                let frames = u32_at(data, 8)?;
                let track_count = if id == 0x12 { 1 } else { u32_at(data, 0x10)? };
                if storage != 1 && storage != 2 && storage != 4 {
                    return Err(format!("xga: unsupported storage type {storage} in chunk 0x{id:02X} at 0x{at:X}"));
                }
                if storage == 2 && id == 0x12 {
                    return Err("xga: storage type 2 in a 0x12 chunk is not used by any shipped file".into());
                }
                let mut bone_map = Vec::new();
                let mut track_headers = Vec::new();
                if id == 0x1B {
                    let want = 0x14 + track_count as usize * 4 + if storage == 2 { track_count as usize * 0x30 } else { 0 };
                    if data.len() != want {
                        return Err(format!("xga: 0x1b size {} != expected {} at 0x{at:X}", data.len(), want));
                    }
                    for i in 0..track_count as usize {
                        bone_map.push(u32_at(data, 0x14 + i * 4)?);
                    }
                    if storage == 2 {
                        let base = 0x14 + track_count as usize * 4;
                        for t in 0..track_count as usize {
                            let words = (0..12).map(|w| u32_at(data, base + t * 0x30 + w * 4)).collect::<Result<Vec<_>, _>>()?;
                            track_headers.push(words);
                        }
                    }
                } else if data.len() != 0x14 {
                    return Err(format!("xga: 0x12 size {} != 0x14 at 0x{at:X}", data.len()));
                }
                if let Some(old) = pending.take() {
                    return Err(format!("xga: header chunk 0x{:02X} without block before chunk at 0x{at:X}", old.chunk_kind));
                }
                pending = Some(PendingSet {
                    chunk_kind: id,
                    storage,
                    count,
                    frames,
                    track_count,
                    bone_map,
                    track_headers,
                });
            }
            0x13 | 0x1C => {
                let p = pending.take().ok_or_else(|| format!("xga: block chunk 0x{id:02X} at 0x{at:X} without header"))?;
                if (id == 0x13) != (p.chunk_kind == 0x12) {
                    return Err(format!("xga: block 0x{id:02X} does not match header 0x{:02X}", p.chunk_kind));
                }
                let neg = id == 0x13 || negate_quat;
                let mut tracks: Vec<Track> = Vec::new();
                let mut o = if id == 0x13 { 4 } else { 0 };
                let nt = p.track_count as usize;
                if p.storage == 2 {
                    for t in 0..nt {
                        let kc = p.track_headers[t][7] as usize;
                        let mut keys = Vec::with_capacity(kc);
                        for _ in 0..kc {
                            keys.push(read_key(data, o, neg)?);
                            o += KEY_SIZE;
                        }
                        let mut map = Vec::with_capacity(p.frames as usize);
                        for _ in 0..p.frames {
                            map.push(u16_at(data, o)?);
                            o += 2;
                        }
                        tracks.push(Track {
                            name: String::new(),
                            node_index: t as u16,
                            keys,
                            frame_map: map,
                            header_words: p.track_headers[t].clone(),
                        });
                    }
                    let total: u32 = tracks.iter().map(|t| t.keys.len() as u32).sum();
                    if total != p.count {
                        return Err(format!("xga: key total {total} != header count {}", p.count));
                    }
                } else {
                    let samples = p.count as usize;
                    let mut per: Vec<Vec<Key>> = vec![Vec::with_capacity(samples); nt];
                    for _s in 0..samples {
                        for track in per.iter_mut() {
                            track.push(read_key(data, o, neg)?);
                            o += KEY_SIZE;
                        }
                    }
                    for (t, keys) in per.into_iter().enumerate() {
                        tracks.push(Track {
                            name: String::new(),
                            node_index: t as u16,
                            keys,
                            frame_map: Vec::new(),
                            header_words: Vec::new(),
                        });
                    }
                }
                if o != data.len() {
                    return Err(format!("xga: block 0x{id:02X} at 0x{at:X} used {o} of {} payload bytes", data.len()));
                }
                if group.track_set.is_some() {
                    return Err(format!("xga: second track set inside group {}", group.index));
                }
                group.track_set = Some(TrackSet {
                    chunk_kind: p.chunk_kind,
                    storage: p.storage,
                    count: p.count,
                    frames: p.frames,
                    track_count: p.track_count,
                    bone_map: p.bone_map,
                    tracks,
                });
            }
            0x1E => {
                if data.len() < 3 {
                    return Err("xga: 0x1e chunk too small".into());
                }
                names.push((u16_at(data, 0)?, cstr(&data[2..])));
            }
            0x2D => {
                if data.len() >= 3 {
                    group.name = Some(cstr(&data[2..]));
                }
            }
            0x25 => {
                if data.len() < 8 {
                    return Err("xga: 0x25 chunk too small".into());
                }
                let n = u16_at(data, 0)? as usize;
                if data.len() != 8 + n * 0x48 {
                    return Err(format!("xga: 0x25 size {} != 8 + {n} x 0x48", data.len()));
                }
                group.hierarchy = Some(Hierarchy {
                    node_count: n as u16,
                    aux16: u16_at(data, 2)?,
                    aux32: u32_at(data, 4)?,
                    nodes: (0..n).map(|i| HierNode { raw: data[8 + i * 0x48..8 + (i + 1) * 0x48].to_vec() }).collect(),
                });
            }
            0x22 => {
                if data.len() != 0x58 {
                    return Err(format!("xga: helper header size {} != 0x58", data.len()));
                }
                anim.helpers.push(Helper {
                    name: cstr(&data[..0x20]),
                    header: data.to_vec(),
                    limit: u32_at(data, 0x40)?,
                    blocks: 0,
                    samples: Vec::new(),
                });
            }
            0x23 => {
                let h = anim.helpers.last_mut().ok_or("xga: helper block before helper header")?;
                if data.len() < 8 {
                    return Err("xga: helper block too small".into());
                }
                let (fp, fr, fs) = (data[2] != 0, data[3] != 0, data[4] != 0);
                let want = 8 + fp as usize * 12 + fr as usize * 16 + fs as usize * 12;
                if data.len() != want {
                    return Err(format!("xga: helper block size {} != {want}", data.len()));
                }
                let mut o = 8;
                let v3 = |o: &mut usize| -> Result<[f32; 3], String> {
                    let r = [f32_at(data, *o)?, f32_at(data, *o + 4)?, f32_at(data, *o + 8)?];
                    *o += 12;
                    Ok(r)
                };
                let pos = if fp { Some(v3(&mut o)?) } else { None };
                let rot = if fr {
                    let r = [-f32_at(data, o)?, -f32_at(data, o + 4)?, -f32_at(data, o + 8)?, f32_at(data, o + 12)?];
                    o += 16;
                    Some(r)
                } else {
                    None
                };
                let scale = if fs { Some(v3(&mut o)?) } else { None };
                let index = u16_at(data, 0)?;
                h.blocks += 1;
                h.samples.push(HelperSample { index, pos, rot, scale });
            }
            0x11 => {
                close(&mut anim, &mut group, &mut names)?;
            }
            0x16 => {
                ended = true;
                at += size;
                break;
            }
            _ => {}
        }
        at += size;
    }
    if !ended {
        return Err("xga: no 0x16 end chunk".into());
    }
    if pending.is_some() {
        return Err("xga: header chunk without block at end".into());
    }
    if group.track_set.is_some() || !names.is_empty() {
        close(&mut anim, &mut group, &mut names)?;
    }
    for h in &mut anim.helpers {
        let mut merged: std::collections::BTreeMap<u16, HelperSample> = std::collections::BTreeMap::new();
        for s in h.samples.drain(..) {
            let e = merged.entry(s.index).or_insert(HelperSample { index: s.index, pos: None, rot: None, scale: None });
            e.pos = e.pos.or(s.pos);
            e.rot = e.rot.or(s.rot);
            e.scale = e.scale.or(s.scale);
        }
        h.samples = merged.into_values().collect();
    }
    anim.trailing = d.len() - at;
    Ok(anim)
}

impl Anim {
    /// Duration in seconds: frames / fps (`CXGSModel+0x1c`, `GetAnimTime`).
    pub fn duration(&self) -> f32 {
        if self.fps > 0.0 {
            self.frames as f32 / self.fps
        } else {
            0.0
        }
    }

    /// Frame position for a time in seconds: time * fps, clamped to 0..=frames-1 (`CXGSAnimBlend::Update`).
    pub fn frame_at(&self, time: f32) -> f32 {
        let f = time * self.fps;
        let last = self.frames.saturating_sub(1) as f32;
        f.clamp(0.0, last)
    }

    /// Local scale / rotation / translation of one track at `time` seconds, evaluated like the engine
    /// (`GenerateAnimationData_Physique`, `GenerateAnimationData`). Values are in file space; the later
    /// handedness fix-up (ext word 6) is not applied.
    pub fn sample(&self, group: usize, track: usize, time: f32) -> Option<Trs> {
        let ts = self.groups.get(group)?.track_set.as_ref()?;
        ts.sample(track, self.frame_at(time), self.key_stride)
    }
}

impl TrackSet {
    /// `frame` is already clamped (see `Anim::frame_at`); `stride` is the ext-header key stride.
    pub fn sample(&self, track: usize, frame: f32, stride: f32) -> Option<Trs> {
        let t = self.tracks.get(track)?;
        if t.keys.is_empty() {
            return None;
        }
        if self.is_dense() {
            let n = t.keys.len();
            let stride = if stride > 0.0 { stride } else { 1.0 };
            let idx = ((frame / stride).floor() as usize).min(n - 1);
            let frac = (frame - idx as f32 * stride) / stride;
            let a = &t.keys[idx];
            if frac > 1.0e-5 && idx + 1 < n {
                Some(lerp_keys(a, &t.keys[idx + 1], frac))
            } else {
                Some(a.trs())
            }
        } else {
            if t.keys.len() == 1 {
                return Some(t.keys[0].trs());
            }
            let f = (frame.max(0.0) as usize).min(t.frame_map.len().checked_sub(1)?);
            let k = *t.frame_map.get(f)? as usize;
            let a = t.keys.get(k)?;
            match t.keys.get(k + 1) {
                Some(b) => {
                    let w = (frame - a.frame as f32) * b.inv_dt;
                    if w == 0.0 {
                        Some(a.trs())
                    } else {
                        Some(lerp_keys(a, b, w))
                    }
                }
                None => Some(a.trs()),
            }
        }
    }
}

/// `XGSGenerateKeyFrameMatrixSimple`: lerp scale and translation; rotation is a shortest-path lerp, switching to a
/// slerp when the 4D dot is below 0.99 (constant read from the binary). The result is not renormalised.
pub fn lerp_keys(a: &Key, b: &Key, t: f32) -> Trs {
    let l3 = |x: [f32; 3], y: [f32; 3]| [x[0] + t * (y[0] - x[0]), x[1] + t * (y[1] - x[1]), x[2] + t * (y[2] - x[2])];
    let mut dot = a.rot[0] * b.rot[0] + a.rot[1] * b.rot[1] + a.rot[2] * b.rot[2] + a.rot[3] * b.rot[3];
    let mut sign = 1.0f32;
    if dot < 0.0 {
        dot = -dot;
        sign = -1.0;
    }
    let (mut wa, mut wb) = (1.0 - t, sign * t);
    if dot < 0.99 {
        let omega = dot.acos();
        let s = omega.sin();
        wa = ((1.0 - t) * omega).sin() / s;
        wb = sign * (t * omega).sin() / s;
    }
    let rot = [
        a.rot[0] * wa + b.rot[0] * wb,
        a.rot[1] * wa + b.rot[1] * wb,
        a.rot[2] * wa + b.rot[2] * wb,
        a.rot[3] * wa + b.rot[3] * wb,
    ];
    Trs { scale: l3(a.scale, b.scale), rot, pos: l3(a.pos, b.pos) }
}

/// Minimal reader for the node-name (0x1e) and hierarchy (0x25) chunks of an `.xgm`, to cross-check an animation's
/// track names against the skeleton it targets. Returns (names in index order, raw 0x25 payloads).
pub fn skeleton_of_xgm(d: &[u8]) -> Result<(Vec<String>, Vec<Vec<u8>>), String> {
    if d.len() < 0x18 || &d[8..12] != b"XGSM" {
        return Err("xgm: bad magic".into());
    }
    let mut at = u32_at(d, 4)? as usize;
    let mut names = Vec::new();
    let mut hier = Vec::new();
    while at + 8 <= d.len() {
        let id = (u32_at(d, at)? & 0xFFFF) as u16;
        let size = u32_at(d, at + 4)? as usize;
        if size < 8 || at + size > d.len() {
            break;
        }
        let data = &d[at + 8..at + size];
        match id {
            0x1E if data.len() >= 3 => names.push(cstr(&data[2..])),
            0x25 => hier.push(data.to_vec()),
            0x16 => break,
            _ => {}
        }
        at += size;
    }
    Ok((names, hier))
}

impl Trs {
    /// Row-vector 4x4 matrix the way `GenerateAnimationData_Physique` builds it from a key: row i of the rotation block
    /// is the transpose of the standard quaternion rotation (m[1] = 2(xy+zw)) times scale i, translation in row 3.
    pub fn to_matrix(&self) -> [f32; 16] {
        let [x, y, z, w] = self.rot;
        let r = [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (z * x - y * w)],
            [2.0 * (x * y - z * w), 1.0 - 2.0 * (z * z + x * x), 2.0 * (y * z + x * w)],
            [2.0 * (z * x + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)],
        ];
        let mut m = [0f32; 16];
        for i in 0..3 {
            for j in 0..3 {
                m[i * 4 + j] = r[i][j] * self.scale[i];
            }
        }
        m[12] = self.pos[0];
        m[13] = self.pos[1];
        m[14] = self.pos[2];
        m[15] = 1.0;
        m
    }
}

/// Spriter (`.scml`, BrashMonkey SCML 1.0) 2D skeletal animation files used by the UI (`ui/ui_core/spriter`).
/// They are plain XML, not tokenised. This is a small tolerant reader plus a typed view of the animation data.
pub mod scml {
    #[derive(Clone, Debug, Default)]
    pub struct Element {
        pub name: String,
        pub attrs: Vec<(String, String)>,
        pub children: Vec<Element>,
    }

    impl Element {
        pub fn attr(&self, key: &str) -> Option<&str> {
            self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
        }
        pub fn num(&self, key: &str) -> Option<f64> {
            self.attr(key).and_then(|v| v.parse().ok())
        }
        pub fn kids<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
            self.children.iter().filter(move |c| c.name == name)
        }
    }

    /// Parse XML into an element tree (declaration, comments and text are skipped).
    pub fn parse_xml(s: &str) -> Result<Element, String> {
        let b = s.as_bytes();
        let mut i = 0;
        let mut stack: Vec<Element> = vec![Element::default()];
        while i < b.len() {
            if b[i] != b'<' {
                i += 1;
                continue;
            }
            if s[i..].starts_with("<?") {
                i += s[i..].find("?>").ok_or("scml: unterminated declaration")? + 2;
                continue;
            }
            if s[i..].starts_with("<!--") {
                i += s[i..].find("-->").ok_or("scml: unterminated comment")? + 3;
                continue;
            }
            let end = i + s[i..].find('>').ok_or("scml: unterminated tag")?;
            let tag = &s[i + 1..end];
            if let Some(name) = tag.strip_prefix('/') {
                let done = stack.pop().ok_or("scml: unbalanced close tag")?;
                if done.name != name.trim() {
                    return Err(format!("scml: </{}> closes <{}>", name.trim(), done.name));
                }
                stack.last_mut().ok_or("scml: unbalanced close tag")?.children.push(done);
            } else {
                let self_close = tag.ends_with('/');
                let body = tag.trim_end_matches('/');
                let mut el = Element::default();
                let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
                el.name = body[..name_end].to_string();
                let mut rest = body[name_end..].trim_start();
                while let Some(eq) = rest.find('=') {
                    let key = rest[..eq].trim().to_string();
                    let after = rest[eq + 1..].trim_start();
                    let q = after.chars().next().ok_or("scml: bad attribute")?;
                    if q != '"' && q != '\'' {
                        return Err("scml: unquoted attribute".into());
                    }
                    let close = after[1..].find(q).ok_or("scml: unterminated attribute")?;
                    el.attrs.push((key, after[1..1 + close].to_string()));
                    rest = after[close + 2..].trim_start();
                }
                if self_close {
                    stack.last_mut().ok_or("scml: stray tag")?.children.push(el);
                } else {
                    stack.push(el);
                }
            }
            i = end + 1;
        }
        if stack.len() != 1 {
            return Err("scml: unclosed tag".into());
        }
        let mut root = stack.pop().unwrap();
        root.children.pop().ok_or_else(|| "scml: empty document".to_string())
    }

    #[derive(Clone, Debug)]
    pub struct ImageFile {
        pub folder: u32,
        pub id: u32,
        pub name: String,
        pub width: f64,
        pub height: f64,
        pub pivot: (f64, f64),
    }

    #[derive(Clone, Debug)]
    pub struct ObjectRef {
        pub id: u32,
        pub timeline: u32,
        pub key: u32,
        pub z_index: i32,
    }

    #[derive(Clone, Debug)]
    pub struct MainKey {
        pub time_ms: u32,
        pub refs: Vec<ObjectRef>,
    }

    #[derive(Clone, Debug)]
    pub struct TimelineKey {
        pub time_ms: u32,
        pub spin: i32,
        pub curve_type: Option<String>,
        /// attributes of the `<object>` (folder, file, x, y, angle, scale_x, scale_y, a, pivot_x, pivot_y ...)
        pub object: Vec<(String, String)>,
    }

    #[derive(Clone, Debug)]
    pub struct Timeline {
        pub id: u32,
        pub name: String,
        pub keys: Vec<TimelineKey>,
    }

    #[derive(Clone, Debug)]
    pub struct Animation {
        pub name: String,
        pub length_ms: u32,
        pub looping: bool,
        pub mainline: Vec<MainKey>,
        pub timelines: Vec<Timeline>,
    }

    #[derive(Clone, Debug)]
    pub struct Entity {
        pub name: String,
        pub animations: Vec<Animation>,
    }

    #[derive(Clone, Debug)]
    pub struct Scml {
        pub files: Vec<ImageFile>,
        pub entities: Vec<Entity>,
    }

    pub fn parse(s: &str) -> Result<Scml, String> {
        let root = parse_xml(s)?;
        if root.name != "spriter_data" {
            return Err(format!("scml: root element is <{}>", root.name));
        }
        let mut files = Vec::new();
        for folder in root.kids("folder") {
            let fid = folder.num("id").unwrap_or(0.0) as u32;
            for f in folder.kids("file") {
                files.push(ImageFile {
                    folder: fid,
                    id: f.num("id").unwrap_or(0.0) as u32,
                    name: f.attr("name").unwrap_or("").to_string(),
                    width: f.num("width").unwrap_or(0.0),
                    height: f.num("height").unwrap_or(0.0),
                    pivot: (f.num("pivot_x").unwrap_or(0.0), f.num("pivot_y").unwrap_or(1.0)),
                });
            }
        }
        let mut entities = Vec::new();
        for e in root.kids("entity") {
            let mut animations = Vec::new();
            for a in e.kids("animation") {
                let mut mainline = Vec::new();
                if let Some(ml) = a.kids("mainline").next() {
                    for k in ml.kids("key") {
                        mainline.push(MainKey {
                            time_ms: k.num("time").unwrap_or(0.0) as u32,
                            refs: k
                                .children
                                .iter()
                                .filter(|c| c.name == "object_ref" || c.name == "bone_ref")
                                .map(|r| ObjectRef {
                                    id: r.num("id").unwrap_or(0.0) as u32,
                                    timeline: r.num("timeline").unwrap_or(0.0) as u32,
                                    key: r.num("key").unwrap_or(0.0) as u32,
                                    z_index: r.num("z_index").unwrap_or(0.0) as i32,
                                })
                                .collect(),
                        });
                    }
                }
                let timelines = a
                    .kids("timeline")
                    .map(|t| Timeline {
                        id: t.num("id").unwrap_or(0.0) as u32,
                        name: t.attr("name").unwrap_or("").to_string(),
                        keys: t
                            .kids("key")
                            .map(|k| TimelineKey {
                                time_ms: k.num("time").unwrap_or(0.0) as u32,
                                spin: k.num("spin").unwrap_or(1.0) as i32,
                                curve_type: k.attr("curve_type").map(str::to_string),
                                object: k
                                    .children
                                    .iter()
                                    .find(|c| c.name == "object" || c.name == "bone")
                                    .map(|o| o.attrs.clone())
                                    .unwrap_or_default(),
                            })
                            .collect(),
                    })
                    .collect();
                animations.push(Animation {
                    name: a.attr("name").unwrap_or("").to_string(),
                    length_ms: a.num("length").unwrap_or(0.0) as u32,
                    looping: a.attr("looping").map_or(true, |v| v != "false"),
                    mainline,
                    timelines,
                });
            }
            entities.push(Entity { name: e.attr("name").unwrap_or("").to_string(), animations });
        }
        Ok(Scml { files, entities })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend_from_slice(&((payload.len() + 8) as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }
    fn words(w: &[u32]) -> Vec<u8> {
        w.iter().flat_map(|x| x.to_le_bytes()).collect()
    }
    fn key(inv_dt: f32, frame: u16, rot: [f32; 4], pos: [f32; 3]) -> Vec<u8> {
        let mut v = inv_dt.to_le_bytes().to_vec();
        v.extend_from_slice(&frame.to_le_bytes());
        v.extend_from_slice(&[0, 0]);
        for f in [1.0f32, 1.0, 1.0].iter().chain(&rot).chain(&pos).chain(&[1.0f32, 1.0, 1.0]) {
            v.extend_from_slice(&f.to_le_bytes());
        }
        assert_eq!(v.len(), KEY_SIZE);
        v
    }

    /// One storage-type-2 track "Root", 5 frames, keys at frames 0 and 4 (file rotation is the conjugate).
    fn synthetic() -> Vec<u8> {
        let mut d = words(&[0x15, 0x18]);
        d.extend_from_slice(b"XGSM");
        d.extend(words(&[FORMAT_V26, 1, 0]));
        d.extend(chunk(0x1F, &words(&[0, 0, 30.0f32.to_bits(), 1.0f32.to_bits(), 0, 0, 0, 0x0200_001C, 0])));
        let mut h = words(&[2, 2, 5, 0, 1, 0xAABBCCDD]);
        h.extend(words(&[0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0]));
        d.extend(chunk(0x1B, &h));
        let s = (std::f32::consts::FRAC_PI_4).sin();
        let c = (std::f32::consts::FRAC_PI_4).cos();
        let mut b = key(0.0, 0, [0.0, 0.0, 0.0, 1.0], [0.0; 3]);
        b.extend(key(0.25, 4, [0.0, 0.0, -s, c], [4.0, 0.0, 0.0]));
        for m in [0u16, 0, 0, 0, 1] {
            b.extend_from_slice(&m.to_le_bytes());
        }
        d.extend(chunk(0x1C, &b));
        let mut n = 0u16.to_le_bytes().to_vec();
        n.extend_from_slice(b"Root\0\0\0\0");
        d.extend(chunk(0x1E, &n));
        d.extend(chunk(0x11, &[0; 8]));
        d.extend(chunk(0x16, &[]));
        d
    }

    #[test]
    fn parses_synthetic_type2() {
        let a = parse(&synthetic()).unwrap();
        assert_eq!(a.fps, 30.0);
        assert_eq!(a.frames, 5);
        assert_eq!(a.groups.len(), 1);
        let ts = a.groups[0].track_set.as_ref().unwrap();
        assert_eq!(ts.storage, 2);
        assert_eq!(ts.tracks[0].name, "Root");
        assert_eq!(ts.tracks[0].keys.len(), 2);
        assert_eq!(ts.tracks[0].frame_map, vec![0, 0, 0, 0, 1]);
        // loader negates the stored rotation xyz
        assert!(ts.tracks[0].keys[1].rot[2] > 0.7);
        assert_eq!(a.trailing, 0);
    }

    #[test]
    fn samples_interpolate() {
        let a = parse(&synthetic()).unwrap();
        // 2 frames in: halfway between key 0 and key 1
        let t = a.sample(0, 0, 2.0 / 30.0).unwrap();
        assert!((t.pos[0] - 2.0).abs() < 1e-5);
        let half = (std::f32::consts::FRAC_PI_8).sin();
        assert!((t.rot[2] - half).abs() < 1e-4, "{:?}", t.rot);
        // clamped to the last frame
        let e = a.sample(0, 0, 100.0).unwrap();
        assert!((e.pos[0] - 4.0).abs() < 1e-5);
        assert!((a.duration() - 5.0 / 30.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"nope").is_err());
        let mut d = synthetic();
        d.truncate(d.len() - 8); // lose the end chunk
        assert!(parse(&d).is_err());
        let mut d = synthetic();
        d[8] = b'Z';
        assert!(parse(&d).is_err());
    }

    #[test]
    fn spriter_parses() {
        let xml = r#"<?xml version="1.0"?><spriter_data scml_version="1.0"><folder id="0"><file id="0" name="a.png" width="4" height="4"/></folder>
<entity id="0" name="e"><animation id="0" name="spin" length="1500" looping="true"><mainline><key id="0"><object_ref id="0" timeline="0" key="0" z_index="0"/></key></mainline>
<timeline id="0" name="t"><key id="0" spin="-1"><object folder="0" file="0" angle="0"/></key><key id="1" time="750"><object folder="0" file="0" angle="180"/></key></timeline></animation></entity></spriter_data>"#;
        let s = scml::parse(xml).unwrap();
        assert_eq!(s.entities[0].animations[0].length_ms, 1500);
        assert_eq!(s.entities[0].animations[0].timelines[0].keys[1].time_ms, 750);
        assert_eq!(s.files[0].name, "a.png");
    }

    /// Runs only when the extracted 2.9.2 assets are next to the repo.
    #[test]
    fn real_files_parse_when_present() {
        let base = std::env::var("ABG_PAK").map(std::path::PathBuf::from).unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets292/pak")
        });
        let root = base.join("characters/animation/big_red");
        let Ok(rd) = std::fs::read_dir(&root) else {
            eprintln!("skipped: {} not found (set ABG_PAK to the extracted pak directory)", root.display());
            return;
        };
        let mut n = 0;
        for e in rd.flatten() {
            let d = std::fs::read(e.path()).unwrap();
            let a = parse(&d).unwrap();
            assert_eq!(a.format_version, FORMAT_V26);
            assert!(a.frames > 0 && a.fps > 0.0);
            let ts = a.groups[0].track_set.as_ref().unwrap();
            assert_eq!(ts.tracks.len(), 30);
            assert_eq!(ts.tracks[0].name, "Beak");
            for t in 0..ts.tracks.len() {
                let s = a.sample(0, t, a.duration() * 0.5).unwrap();
                assert!(s.rot.iter().chain(&s.pos).all(|v| v.is_finite()));
            }
            n += 1;
        }
        assert!(n > 0);
    }
}
