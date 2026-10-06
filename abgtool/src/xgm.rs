//! XGSM model (`*.xgm`): meshes, materials, skeletons and skinning of karts, characters and track objects.
//!
//! Handles the v1.0.1 files (platform id 1 mesh blocks) and the 2.9.x files (platform id 6 "unified" mesh blocks,
//! file versions 0x01010126 and 0x01010128). Everything was read from `CXGSModel::InitModel`,
//! `CXGS_XGMLoader::*`, `CXGSModelUnified::LoadPlatformModel`, `CXGSPlatformMesh::*`, `CXGSMaterial::*` and the
//! `DoSkinBlock*` routines of libABK 2.9.1 (see FORMATS_part_xgm.md for the byte level description).
//!
//! File: u32 21, u32 header size (24), "XGSM", u32 version, u32 a, u32 b (counts), then chunks
//! `u32 id, u32 size (size INCLUDES the 8-byte chunk header)`. The chunk list ends at chunk 0x16 (end marker);
//! anything after it is not a chunk.

use std::collections::BTreeMap;

pub const CHUNK_MESH: u32 = 0x31;
pub const CHUNK_MESH_END: u32 = 0x11;
pub const CHUNK_MATERIAL: u32 = 0x14;
pub const CHUNK_END: u32 = 0x16;
pub const CHUNK_PHYSIQUE_HEADER: u32 = 0x1B;
pub const CHUNK_PHYSIQUE_BLOCK: u32 = 0x1C;
pub const CHUNK_BONE_NAME: u32 = 0x1E;
pub const CHUNK_EXT_HEADER: u32 = 0x1F;
pub const CHUNK_HELPER_HEADER: u32 = 0x22;
pub const CHUNK_HELPER_BLOCK: u32 = 0x23;
pub const CHUNK_HIERARCHY: u32 = 0x25;
pub const CHUNK_MESH_NAME: u32 = 0x2D;

pub struct Chunk {
    pub id: u32,
    pub offset: usize,
    pub size: usize,
}

/// One element of a vertex descriptor (`TXGSVertexDescriptor`, 0x18 bytes each, the list ends with type -1).
/// `kind` is the data type (0..3 float1-4, 0x10 ubyte4n, 0x11 ubyte4, 0x12 byte4, 0x13 ubyte4n, 0x14 byte4n,
/// 0x15 short2, 0x16 short4, 0x17 ushort2, 0x18 ushort4, 0x19 short2n, 0x1A short4n, 0x1B ushort2n, 0x1C ushort4n);
/// `usage` is the D3D style semantic (0 position, 1 blend weight, 2 blend indices, 3 normal, 5 texcoord, 6 tangent,
/// 7 binormal, 10 colour).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VertexElement {
    pub kind: u32,
    pub usage: u32,
    pub usage_index: u32,
    pub offset: u32,
    pub stream: u32,
}

/// A draw range of a mesh: `tri_count` triangles of the index list starting at index `first_index`, one material.
#[derive(Clone, Debug, Default)]
pub struct SubMesh {
    /// u16 at +0 of the 0x14 byte record (always 0 in the shipped files).
    pub flags: u16,
    /// Index into `Xgm::materials` (u16 at +2, 0xFFFF = none).
    pub material: Option<usize>,
    pub tri_count: u32,
    pub first_index: u32,
    /// u16 at +0xC / +0xE: first vertex and vertex count used by the range (render hints).
    pub first_vertex: u16,
    pub vertex_count: u16,
    /// u32 at +0x10: 0xFFFFFFFF marks a CPU skinned range, anything else a static range.
    pub skin_tag: u32,
}

impl SubMesh {
    pub fn skinned(&self) -> bool {
        self.skin_tag == 0xFFFF_FFFF
    }
}

/// One `CXGSSkinBlockUnified`: `vertex_count` consecutive skinned vertices sharing one `influences` count and one
/// bone palette `bones` (palette slot value = bone index of the skeleton).
#[derive(Clone, Debug, Default)]
pub struct SkinBlock {
    pub vertex_count: u16,
    pub influences: u16,
    pub bones: [u8; 8],
}

#[derive(Clone, Debug, Default)]
pub struct SkinInfo {
    pub num_bones: u32,
    /// Number of leading vertices of the vertex array that are skinned (the rest are static).
    pub skinned_vertices: u32,
    /// Total weight bytes in the stream (sum of vertex_count * influences).
    pub weight_bytes: u32,
    /// Bytes at +0x30 / +0x31 / +0x32 of the skin struct (has normal, extra 4 byte words after the normal, has tangent).
    pub flags: [u8; 3],
    pub blocks: Vec<SkinBlock>,
}

#[derive(Default, Clone)]
pub struct Mesh {
    pub stride: usize,
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[u8; 4]>,
    pub uvs: Vec<[f32; 2]>,
    /// Triangle list, absolute vertex indices into `positions`.
    pub indices: Vec<u16>,
    // ---- added for 2.9.x ----
    /// Mesh name from chunk 0x2D (e.g. "Red"), empty when absent.
    pub name: String,
    /// Platform id in the mesh block (1 = old 1.0.1 layout, 6 = unified layout).
    pub platform: u16,
    /// u16 at +0x2E (bit 1: texture coordinates are 16 bit and need `uv_scale` / `uv_bias`).
    pub mesh_flags: u16,
    /// f32 at +0xC (bounding radius).
    pub radius: f32,
    /// 4x4 matrix at +0x30 (identity in every shipped file).
    pub matrix: [f32; 16],
    pub uv_scale: [f32; 2],
    pub uv_bias: [f32; 2],
    pub elements: Vec<VertexElement>,
    pub submeshes: Vec<SubMesh>,
    /// Second texture coordinate set (usage 5, index 1), empty when absent.
    pub uvs2: Vec<[f32; 2]>,
    pub tangents: Vec<[f32; 4]>,
    /// Per vertex skin influences (only the first `skinned_vertices` vertices carry any, the rest are zero).
    pub bone_indices: Vec<[u8; 8]>,
    pub bone_weights: Vec<[f32; 8]>,
    pub influences: Vec<u8>,
    pub skin: Option<SkinInfo>,
    /// Index into `Xgm::skeletons`.
    pub skeleton: Option<usize>,
    /// Per mesh transform from the embedded static animation (chunk 0x12 type 0), row vector convention `p * M`
    /// (translation in the last row). Present on the "_outline" meshes: it maps their vertices onto the base mesh
    /// (scale about 1.07, y and z swapped). `None` = identity.
    pub anim_matrix: Option<[f32; 16]>,
}

/// A named transform inside the model (wheel hubs, pilot seats, pivots).
/// Node block: chunk 0x22 (size 0x60) with a 32 byte NUL padded name, followed by chunk(s) 0x23: u16 frame,
/// 3 flag bytes (position / rotation / scale present), f32 position[3], f32 rotation quaternion xyzw[4], f32 scale[3]
/// (only the parts whose flag is set are present).
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub position: [f32; 3],
    /// Quaternion exactly as stored in the file.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// Quaternion as the engine has it after loading (file version ...126 stores the inverse rotation, ...128 stores it
    /// directly).
    pub rotation_engine: [f32; 4],
    /// Number of transform blocks (animation frames) the node declares.
    pub frames: u32,
}

/// One `TXGSMaterialDesc` (chunk 0x14, 360 bytes).
#[derive(Clone, Debug, Default)]
pub struct Material {
    /// Material name (e.g. "ABG_CAR1_Char~Red") at +0x11E.
    pub name: String,
    /// Colours at +0x08 / +0x0C / +0x10, stored BGRA, returned as RGBA 0..1 (engine fields 0x15C / 0x16C / 0x17C).
    pub colors: [[f32; 4]; 3],
    /// Alpha byte of the third colour (+0x13); the engine multiplies it by a constant to get field 0x188.
    pub shininess_raw: u8,
    /// Texture slots (slot type u16, file name), `u16 at +0x1C` of them (max 4). Seen: type 0 = base colour map,
    /// 1 = "*_shade" ramp texture, 5 / 6 = other maps (the loader sets render bit 0x10 when any slot has type 5).
    pub textures: Vec<(u16, String)>,
    /// u32 at +0x164 (ASCII looking tag such as "77GB"; engine field 0x158).
    pub tag: u32,
}

/// Bone of a skeleton: hierarchy node (chunk 0x25) + name (chunk 0x1E) + rest transform (chunk 0x1C).
#[derive(Clone, Debug, Default)]
pub struct Bone {
    pub name: String,
    pub first_child: Option<usize>,
    pub next_sibling: Option<usize>,
    pub parent: Option<usize>,
    /// 4x4 matrix at +4 of the 0x48 byte hierarchy node (row vectors, translation in the last row). All zero for bones no
    /// vertex uses; otherwise close to the inverse of the chained rest pose (probably the inverse bind matrix, exact
    /// convention not reproduced).
    pub matrix: [f32; 16],
    /// u16 at +2 and u32 at +0x44 of the node (0xFFFF / 0xFFFFFF01 in every shipped file).
    pub node_tail: [u32; 2],
    /// Rest pose from the physique block (local to the parent): words 2-4 ("scale a"), rotation quaternion xyzw (engine
    /// convention), position, words 12-14 ("scale b"). The two scale triples are named by position only (both ~1.0).
    pub rest_scale_a: [f32; 3],
    pub rest_rotation: [f32; 4],
    pub rest_position: [f32; 3],
    pub rest_scale_b: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
    /// u32 at the start of the physique header (1 / 2 / 4; 4 = one 60 byte record per bone and frame).
    pub physique_type: u32,
    /// Frames in the physique block (1 in every shipped file = rest pose).
    pub physique_frames: u32,
    /// The extra u32 table of chunk 0x1B (one entry per bone). Meaning not resolved.
    pub physique_map: Vec<u32>,
}

pub struct Xgm {
    pub chunks: Vec<Chunk>,
    pub meshes: Vec<Mesh>,
    pub nodes: Vec<Node>,
    /// Printable names found in the file (node names, texture file names).
    pub names: Vec<String>,
    // ---- added for 2.9.x ----
    /// File version dword (0x01010126 or 0x01010128).
    pub version: u32,
    pub materials: Vec<Material>,
    pub skeletons: Vec<Skeleton>,
    /// Collision shapes (chunks 0x17 / 0x18 / 0x19 / 0x1A / 0x2A): (chunk id, name).
    pub collisions: Vec<(u32, String)>,
    /// Embedded animation headers (chunk 0x12) with the size of the following 0x13 data block. Not decoded further.
    pub anims: Vec<AnimHeader>,
}

fn u32_at(d: &[u8], at: usize) -> Result<u32, String> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("xgm: read past end at 0x{at:X}"))
}

fn u16_at(d: &[u8], at: usize) -> Result<u16, String> {
    d.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| format!("xgm: read past end at 0x{at:X}"))
}

fn f32_at(d: &[u8], at: usize) -> Result<f32, String> {
    u32_at(d, at).map(f32::from_bits)
}

fn cstr(d: &[u8], at: usize, max: usize) -> String {
    let end = (at + max).min(d.len());
    let s = d.get(at..end).unwrap_or(&[]);
    let n = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..n]).into_owned()
}

/// Per-mesh-group data that arrives in separate chunks before the mesh block (`this+0x70` = index of the current mesh).
#[derive(Default)]
struct Group {
    hierarchy: Option<usize>,
    phys_header: Option<usize>,
    phys_block: Option<usize>,
    bone_names: Vec<(usize, String)>,
    name: String,
}

pub fn parse(d: &[u8]) -> Result<Xgm, String> {
    if d.len() < 0x18 || &d[8..12] != b"XGSM" {
        return Err("xgm: bad magic".into());
    }
    let header = u32_at(d, 4)? as usize;
    let version = u32_at(d, 12)?;
    let mut chunks = Vec::new();
    let mut at = header;
    while at + 8 <= d.len() {
        let id = u32_at(d, at)?;
        let size = u32_at(d, at + 4)? as usize;
        if size < 8 || at + size > d.len() {
            break; // trailing data that is not a plain chunk
        }
        chunks.push(Chunk { id, offset: at, size });
        at += size;
        if id & 0xFFFF == CHUNK_END {
            break; // the loader stops here; whatever follows is not part of the chunk list
        }
    }

    let mut meshes: Vec<(usize, Mesh)> = Vec::new();
    let mut materials: Vec<Material> = Vec::new();
    let mut groups: BTreeMap<usize, Group> = BTreeMap::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut collisions = Vec::new();
    let mut anims: Vec<AnimHeader> = Vec::new();
    let mut cur = 0usize; // index of the current mesh (counted by the 0x11 end-of-mesh markers)
    for c in &chunks {
        let id = c.id & 0xFFFF;
        match id {
            CHUNK_MESH_END => {
                if c.size == 8 {
                    cur += 1;
                }
            }
            CHUNK_MESH => meshes.push((cur, parse_mesh(d, c, version)?)),
            CHUNK_MATERIAL => materials.push(parse_material(d, c)?),
            CHUNK_HIERARCHY => groups.entry(cur).or_default().hierarchy = Some(c.offset),
            CHUNK_PHYSIQUE_HEADER => groups.entry(cur).or_default().phys_header = Some(c.offset),
            CHUNK_PHYSIQUE_BLOCK => groups.entry(cur).or_default().phys_block = Some(c.offset),
            CHUNK_BONE_NAME if c.size >= 10 => {
                let idx = u16_at(d, c.offset + 8)? as usize;
                groups.entry(cur).or_default().bone_names.push((idx, cstr(d, c.offset + 10, (c.size - 10).min(31))));
            }
            CHUNK_MESH_NAME if c.size >= 10 => {
                groups.entry(cur).or_default().name = cstr(d, c.offset + 10, (c.size - 10).min(31));
            }
            CHUNK_HELPER_HEADER if c.size >= 0x58 + 8 => {
                // u32 at +0x48 = number of transform blocks; the frame 0 block is the node's transform
                nodes.push(Node {
                    name: cstr(d, c.offset + 8, 32),
                    position: [0.0; 3],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                    rotation_engine: [0.0, 0.0, 0.0, 1.0],
                    frames: u32_at(d, c.offset + 0x48)?,
                });
            }
            CHUNK_HELPER_BLOCK if c.size >= 16 => {
                if let Some(n) = nodes.last_mut() {
                    if u16_at(d, c.offset + 8)? == 0 {
                        let (hp, hr, hs) = (d[c.offset + 10] != 0, d[c.offset + 11] != 0, d[c.offset + 12] != 0);
                        let mut p = c.offset + 16;
                        if hp {
                            n.position = [f32_at(d, p)?, f32_at(d, p + 4)?, f32_at(d, p + 8)?];
                            p += 12;
                        }
                        if hr {
                            n.rotation = [f32_at(d, p)?, f32_at(d, p + 4)?, f32_at(d, p + 8)?, f32_at(d, p + 12)?];
                            p += 16;
                        }
                        if hs {
                            n.scale = [f32_at(d, p)?, f32_at(d, p + 4)?, f32_at(d, p + 8)?];
                        }
                        n.rotation_engine = if version == 0x0101_0126 {
                            [0.0 - n.rotation[0], 0.0 - n.rotation[1], 0.0 - n.rotation[2], n.rotation[3]]
                        } else {
                            n.rotation
                        };
                    }
                }
            }
            0x12 if c.size >= 0x1C => {
                let mut words = [0u32; 5];
                for (k, w) in words.iter_mut().enumerate() {
                    *w = u32_at(d, c.offset + 8 + 4 * k)?;
                }
                anims.push(AnimHeader { words, block_bytes: 0, matrices: Vec::new(), mesh_index: cur });
            }
            0x13 => {
                if let Some(a) = anims.last_mut() {
                    a.block_bytes = c.size - 8;
                    // type 0: u32 + `count` 4x4 matrices (LoadAnimBlock_04)
                    if a.words[0] == 0 && a.block_bytes == 4 + 64 * a.words[1] as usize {
                        for m in 0..a.words[1] as usize {
                            let mut mat = [0.0f32; 16];
                            for (k, v) in mat.iter_mut().enumerate() {
                                *v = f32_at(d, c.offset + 12 + 64 * m + 4 * k)?;
                            }
                            a.matrices.push(mat);
                        }
                    }
                }
            }
            0x17 | 0x18 | 0x19 | 0x1A | 0x2A if c.size >= 0x38 + 1 => collisions.push((id, cstr(d, c.offset + 0x38, 32))),
            _ => {}
        }
    }

    // skeletons: one per mesh group that has a hierarchy or physique chunk
    let mut skeletons = Vec::new();
    let mut group_skeleton: BTreeMap<usize, usize> = BTreeMap::new();
    for (&g, grp) in &groups {
        if grp.hierarchy.is_some() || grp.phys_header.is_some() {
            group_skeleton.insert(g, skeletons.len());
            skeletons.push(build_skeleton(d, grp, version)?);
        }
    }
    let mut out_meshes = Vec::new();
    for (g, mut m) in meshes {
        if let Some(grp) = groups.get(&g) {
            m.name = grp.name.clone();
        }
        m.skeleton = group_skeleton.get(&g).copied();
        m.anim_matrix = anims.iter().find(|a| a.mesh_index == g && a.words[0] == 0 && !a.matrices.is_empty()).map(|a| a.matrices[0]);
        for s in &mut m.submeshes {
            if let Some(i) = s.material {
                if i >= materials.len() {
                    s.material = None;
                }
            }
        }
        out_meshes.push(m);
    }

    let mut names = Vec::new();
    let mut run = Vec::new();
    for &b in d {
        if (0x20..0x7F).contains(&b) {
            run.push(b);
        } else {
            if run.len() >= 4 {
                names.push(String::from_utf8_lossy(&run).into_owned());
            }
            run.clear();
        }
    }
    Ok(Xgm { chunks, meshes: out_meshes, nodes, names, version, materials, skeletons, collisions, anims })
}

fn parse_material(d: &[u8], c: &Chunk) -> Result<Material, String> {
    if c.size != 0x168 {
        return Err(format!("xgm: material chunk is {} bytes, expected 360", c.size));
    }
    let at = c.offset;
    let mut m = Material::default();
    for k in 0..3 {
        let w = u32_at(d, at + 8 + 4 * k)?;
        let (b, g, r, a) = ((w & 0xFF) as f32, ((w >> 8) & 0xFF) as f32, ((w >> 16) & 0xFF) as f32, (w >> 24) as f32);
        m.colors[k] = [r / 255.0, g / 255.0, b / 255.0, a / 255.0];
    }
    m.shininess_raw = d[at + 0x13];
    let n = (u16_at(d, at + 0x1C)? as usize).min(4);
    for i in 0..n {
        m.textures.push((u16_at(d, at + 0x14 + 2 * i)?, cstr(d, at + 0x1E + 0x40 * i, 64)));
    }
    m.name = cstr(d, at + 0x11E, 64);
    m.tag = u32_at(d, at + 0x164)?;
    Ok(m)
}

fn build_skeleton(d: &[u8], grp: &Group, version: u32) -> Result<Skeleton, String> {
    let mut sk = Skeleton::default();
    if let Some(h) = grp.hierarchy {
        let count = u16_at(d, h + 8)? as usize;
        let nodes_at = h + 16;
        if nodes_at + count * 0x48 > d.len() {
            return Err("xgm: hierarchy runs past the end".into());
        }
        for i in 0..count {
            let n = nodes_at + i * 0x48;
            let link = |b: u8| if b == 0xFF || b as usize >= count { None } else { Some(b as usize) };
            let mut bone = Bone {
                first_child: link(d[n]),
                next_sibling: link(d[n + 1]),
                node_tail: [u16_at(d, n + 2)? as u32, u32_at(d, n + 0x44)?],
                ..Default::default()
            };
            for k in 0..16 {
                bone.matrix[k] = f32_at(d, n + 4 + 4 * k)?;
            }
            sk.bones.push(bone);
        }
        // parents from the first-child / next-sibling lists
        for i in 0..count {
            let mut ch = sk.bones[i].first_child;
            let mut guard = 0;
            while let Some(c) = ch {
                if sk.bones[c].parent.is_none() {
                    sk.bones[c].parent = Some(i);
                }
                ch = sk.bones[c].next_sibling;
                guard += 1;
                if guard > count {
                    break;
                }
            }
        }
        for (i, name) in &grp.bone_names {
            if let Some(b) = sk.bones.get_mut(*i) {
                b.name = name.clone();
            }
        }
    }
    if let Some(p) = grp.phys_header {
        sk.physique_type = u32_at(d, p + 8)?;
        sk.physique_frames = u32_at(d, p + 12)?;
        let count = u32_at(d, p + 0x18)? as usize;
        if p + 0x1C + count * 4 > d.len() {
            return Err("xgm: physique header runs past the end".into());
        }
        for k in 0..count {
            sk.physique_map.push(u32_at(d, p + 0x1C + 4 * k)?);
        }
        if sk.bones.is_empty() {
            sk.bones = vec![Bone::default(); count];
            for (i, name) in &grp.bone_names {
                if let Some(b) = sk.bones.get_mut(*i) {
                    b.name = name.clone();
                }
            }
        }
        if let (Some(b), 1 | 4) = (grp.phys_block, sk.physique_type) {
            // one 60 byte record per bone (and frame); frame 0 = rest pose
            for k in 0..count.min(sk.bones.len()) {
                let r = b + 8 + 60 * k;
                if r + 60 > d.len() {
                    break;
                }
                let f = |i: usize| f32_at(d, r + 4 * i).unwrap_or(0.0);
                let bone = &mut sk.bones[k];
                bone.rest_scale_a = [f(2), f(3), f(4)];
                // the loader negates words 5,6,7 (quaternion x,y,z) for file version ...126
                let neg = if version == 0x0101_0126 { -1.0 } else { 1.0 };
                bone.rest_rotation = [neg * f(5), neg * f(6), neg * f(7), f(8)];
                bone.rest_position = [f(9), f(10), f(11)];
                bone.rest_scale_b = [f(12), f(13), f(14)];
            }
        }
    }
    Ok(sk)
}

fn parse_mesh(d: &[u8], c: &Chunk, version: u32) -> Result<Mesh, String> {
    if c.size < 0x30 {
        return Err("xgm: mesh chunk too small".into());
    }
    let platform = u16_at(d, c.offset + 0xA)?;
    match platform {
        1 => {
            let mut m = parse_mesh_v1(d, c)?;
            m.platform = 1;
            Ok(m)
        }
        6 => parse_mesh_v6(d, c, version),
        p => Err(format!("xgm: unknown mesh platform {p}")),
    }
}

/// (components, normalised, is_unsigned, bytes per component, is_float)
fn element_layout(kind: u32) -> Option<(usize, bool, bool, usize, bool)> {
    Some(match kind {
        0..=3 => ((kind + 1) as usize, false, false, 4, true),
        0x10 | 0x13 => (4, true, true, 1, false),
        0x11 => (4, false, true, 1, false),
        0x12 => (4, false, false, 1, false),
        0x14 => (4, true, false, 1, false),
        0x15 => (2, false, false, 2, false),
        0x16 => (4, false, false, 2, false),
        0x17 => (2, false, true, 2, false),
        0x18 => (4, false, true, 2, false),
        0x19 => (2, true, false, 2, false),
        0x1A => (4, true, false, 2, false),
        0x1B => (2, true, true, 2, false),
        0x1C => (4, true, true, 2, false),
        _ => return None,
    })
}

/// Decode the components of one vertex element into floats. Normalised signed bytes divide by 127 (the engine uses
/// 1/127 in `DoSkinBlock*`), signed shorts by 32767, unsigned by their maximum.
fn decode_element(d: &[u8], at: usize, kind: u32) -> Result<([f32; 4], [u8; 4]), String> {
    let (n, norm, unsigned, bytes, float) = element_layout(kind).ok_or_else(|| format!("xgm: vertex element type 0x{kind:X} not supported"))?;
    let mut v = [0.0f32; 4];
    let mut raw = [0u8; 4];
    for i in 0..n {
        let p = at + i * bytes;
        v[i] = if float {
            f32_at(d, p)?
        } else {
            match (bytes, unsigned) {
                (1, true) => {
                    let b = *d.get(p).ok_or("xgm: vertex past end")?;
                    raw[i] = b;
                    if norm { b as f32 / 255.0 } else { b as f32 }
                }
                (1, false) => {
                    let b = *d.get(p).ok_or("xgm: vertex past end")? as i8;
                    raw[i] = b as u8;
                    if norm { b as f32 / 127.0 } else { b as f32 }
                }
                (2, true) => {
                    let s = u16_at(d, p)?;
                    raw[i] = (s >> 8) as u8;
                    if norm { s as f32 / 65535.0 } else { s as f32 }
                }
                _ => {
                    let s = u16_at(d, p)? as i16;
                    raw[i] = (s >> 8) as u8;
                    if norm { s as f32 / 32767.0 } else { s as f32 }
                }
            }
        };
    }
    Ok((v, raw))
}

fn parse_mesh_v6(d: &[u8], c: &Chunk, _version: u32) -> Result<Mesh, String> {
    let at = c.offset;
    let end = at + c.size;
    let rel = |field: usize| -> Result<usize, String> {
        let v = u32_at(d, at + field)? as usize;
        if v > c.size {
            return Err(format!("xgm: mesh offset 0x{v:X} (field 0x{field:X}) outside the chunk"));
        }
        Ok(v)
    };
    let mut m = Mesh { platform: 6, ..Default::default() };
    m.radius = f32_at(d, at + 0xC)?;
    for k in 0..3 {
        m.bbox_min[k] = f32_at(d, at + 0x14 + 4 * k)?;
        m.bbox_max[k] = f32_at(d, at + 0x20 + 4 * k)?;
    }
    m.stride = u16_at(d, at + 0x2C)? as usize;
    m.mesh_flags = u16_at(d, at + 0x2E)?;
    for k in 0..16 {
        m.matrix[k] = f32_at(d, at + 0x30 + 4 * k)?;
    }
    m.uv_scale = [f32_at(d, at + 0x70)?, f32_at(d, at + 0x74)?];
    m.uv_bias = [f32_at(d, at + 0x78)?, f32_at(d, at + 0x7C)?];
    let vtx_off = rel(0x80)?;
    let idx_off = rel(0x88)?;
    let skin_off = rel(0xA0)?;
    let desc_off = rel(0xA8)?;
    let sub_off = rel(0xB8)?;
    let vtx_bytes = u32_at(d, at + 0xC0)? as usize;
    let idx_bytes = u32_at(d, at + 0xC4)? as usize;
    let nsub = u32_at(d, at + 0xC8)? as usize;
    if m.stride < 12 || vtx_bytes % m.stride != 0 || idx_bytes % 2 != 0 || vtx_off + vtx_bytes > c.size || idx_off + idx_bytes > c.size {
        return Err(format!("xgm: odd mesh header (vertex {vtx_bytes} B, index {idx_bytes} B, stride {})", m.stride));
    }
    let nv = vtx_bytes / m.stride;

    // vertex descriptor
    if desc_off == 0 {
        return Err("xgm: mesh without a vertex descriptor".into());
    }
    let mut e = at + desc_off;
    loop {
        let kind = u32_at(d, e)?;
        if kind == 0xFFFF_FFFF {
            break;
        }
        m.elements.push(VertexElement {
            kind,
            usage: u32_at(d, e + 4)?,
            usage_index: u32_at(d, e + 8)?,
            offset: u32_at(d, e + 12)?,
            stream: u32_at(d, e + 16)?,
        });
        e += 0x18;
        if m.elements.len() > 16 || e + 4 > end {
            return Err("xgm: vertex descriptor is not terminated".into());
        }
    }
    let uv16 = m.mesh_flags & 2 != 0;
    let base = at + vtx_off;
    for i in 0..nv {
        let v = base + i * m.stride;
        let mut got_pos = false;
        for el in &m.elements {
            let p = v + el.offset as usize;
            let (val, raw) = decode_element(d, p, el.kind)?;
            match (el.usage, el.usage_index) {
                (0, 0) => {
                    m.positions.push([val[0], val[1], val[2]]);
                    got_pos = true;
                }
                (3, 0) => m.normals.push([val[0], val[1], val[2]]),
                (10, 0) => m.colors.push(match el.kind {
                    0x10 | 0x11 | 0x13 => raw,
                    _ => [(val[0] * 255.0) as u8, (val[1] * 255.0) as u8, (val[2] * 255.0) as u8, (val[3] * 255.0) as u8],
                }),
                (5, idx @ (0 | 1)) => {
                    let mut uv = [val[0], val[1]];
                    if uv16 && idx == 0 {
                        // loader: SHORT2 -> SHORT2N and scale * 32767, so uv = short * scale + bias in both cases
                        let k = if el.kind == 0x19 { 32767.0 } else { 1.0 };
                        uv = [uv[0] * k * m.uv_scale[0] + m.uv_bias[0], uv[1] * k * m.uv_scale[1] + m.uv_bias[1]];
                    }
                    if idx == 0 {
                        m.uvs.push(uv);
                    } else {
                        m.uvs2.push(uv);
                    }
                }
                (6, 0) => m.tangents.push([val[0], val[1], val[2], if el.kind >= 3 { val[3] } else { 1.0 }]),
                _ => {}
            }
        }
        if !got_pos {
            return Err("xgm: vertex descriptor has no position".into());
        }
    }

    // sub meshes
    for s in 0..nsub {
        let r = at + sub_off + 0x14 * s;
        if sub_off == 0 || r + 0x14 > end {
            return Err("xgm: sub mesh table runs past the chunk".into());
        }
        let mat = u16_at(d, r + 2)?;
        m.submeshes.push(SubMesh {
            flags: u16_at(d, r)?,
            material: if mat == 0xFFFF { None } else { Some(mat as usize) },
            tri_count: u32_at(d, r + 4)?,
            first_index: u32_at(d, r + 8)?,
            first_vertex: u16_at(d, r + 0xC)?,
            vertex_count: u16_at(d, r + 0xE)?,
            skin_tag: u32_at(d, r + 0x10)?,
        });
    }

    // skin
    let mut dyn_verts = 0usize;
    if skin_off != 0 {
        let s = at + skin_off;
        let blocks_off = u32_at(d, s)? as usize;
        let weights_off = u32_at(d, s + 8)? as usize;
        let info = SkinInfo {
            num_bones: u32_at(d, s + 0x20)?,
            skinned_vertices: u32_at(d, s + 0x28)?,
            weight_bytes: u32_at(d, s + 0x2C)?,
            flags: [d[s + 0x30], d[s + 0x31], d[s + 0x32]],
            blocks: Vec::new(),
        };
        let nblocks = u32_at(d, s + 0x24)? as usize;
        let mut info = info;
        dyn_verts = info.skinned_vertices as usize;
        if dyn_verts > nv || at + blocks_off + 12 * nblocks > end || at + weights_off + info.weight_bytes as usize > end {
            return Err("xgm: skin data outside the mesh chunk".into());
        }
        m.bone_indices = vec![[0; 8]; nv];
        m.bone_weights = vec![[0.0; 8]; nv];
        m.influences = vec![0; nv];
        let mut v = 0usize;
        let mut w = at + weights_off;
        for b in 0..nblocks {
            let r = at + blocks_off + 12 * b;
            let mut blk = SkinBlock { vertex_count: u16_at(d, r)?, influences: u16_at(d, r + 2)?, bones: [0; 8] };
            blk.bones.copy_from_slice(&d[r + 4..r + 12]);
            if blk.influences == 0 || blk.influences > 8 {
                return Err(format!("xgm: skin block with {} influences", blk.influences));
            }
            for _ in 0..blk.vertex_count {
                if v >= dyn_verts || w + blk.influences as usize > end {
                    return Err("xgm: skin blocks cover more vertices than the mesh skins".into());
                }
                for j in 0..blk.influences as usize {
                    m.bone_indices[v][j] = blk.bones[j];
                    m.bone_weights[v][j] = d[w + j] as f32 / 255.0;
                }
                m.influences[v] = blk.influences as u8;
                w += blk.influences as usize;
                v += 1;
            }
            info.blocks.push(blk);
        }
        if v != dyn_verts {
            return Err(format!("xgm: skin blocks cover {v} vertices, mesh skins {dyn_verts}"));
        }
        m.skin = Some(info);
    }

    // indices: absolute vertex indices; static ranges of a skinned mesh are relative to the static part of the buffer
    let ibase = at + idx_off;
    let raw: Vec<u16> = (0..idx_bytes / 2).map(|i| u16_at(d, ibase + 2 * i)).collect::<Result<_, _>>()?;
    m.indices = raw.clone();
    if dyn_verts > 0 {
        for s in &m.submeshes {
            if !s.skinned() {
                let lo = s.first_index as usize;
                let hi = (lo + 3 * s.tri_count as usize).min(m.indices.len());
                for ix in &mut m.indices[lo.min(hi)..hi] {
                    *ix = ix.wrapping_add(dyn_verts as u16);
                }
            }
        }
    }
    Ok(m)
}

/// 1.0.1 mesh block (platform id 1): bbox +0x14 / +0x20, vertex bytes +0x2C, index bytes +0x30, stride +0x40,
/// vertex data then u16 triangle indices at the end of the chunk.
fn parse_mesh_v1(d: &[u8], c: &Chunk) -> Result<Mesh, String> {
    let at = c.offset;
    let vertex_bytes = u32_at(d, at + 0x2C)? as usize;
    let index_bytes = u32_at(d, at + 0x30)? as usize;
    let stride = u32_at(d, at + 0x40)? as usize;
    let end = c.offset + c.size;
    if stride == 0 || vertex_bytes % stride != 0 || index_bytes % 2 != 0 || vertex_bytes + index_bytes > c.size {
        return Err(format!("xgm: odd mesh header (vertex {vertex_bytes} B, index {index_bytes} B, stride {stride})"));
    }
    let index_start = end - index_bytes;
    let vertex_start = index_start - vertex_bytes;

    let mut mesh = Mesh { stride, ..Default::default() };
    for k in 0..3 {
        mesh.bbox_min[k] = f32_at(d, at + 0x14 + 4 * k)?;
        mesh.bbox_max[k] = f32_at(d, at + 0x20 + 4 * k)?;
    }
    if stride < 12 {
        return Err("xgm: stride too small for a position".into());
    }
    for i in 0..vertex_bytes / stride {
        let v = vertex_start + i * stride;
        mesh.positions.push([f32_at(d, v)?, f32_at(d, v + 4)?, f32_at(d, v + 8)?]);
        if stride >= 28 {
            let n = |k: usize| d[v + 12 + k] as i8 as f32 / 127.0;
            mesh.normals.push([n(0), n(1), n(2)]);
            mesh.colors.push([d[v + 16], d[v + 17], d[v + 18], d[v + 19]]);
            mesh.uvs.push([f32_at(d, v + 20)?, f32_at(d, v + 24)?]);
        } else if stride == 20 {
            // outline meshes: position, normal (byte4n), colour - no texture coordinates (same as the 2.9 layout)
            let n = |k: usize| d[v + 12 + k] as i8 as f32 / 127.0;
            mesh.normals.push([n(0), n(1), n(2)]);
            mesh.colors.push([d[v + 16], d[v + 17], d[v + 18], d[v + 19]]);
        }
    }
    for i in 0..index_bytes / 2 {
        let at = index_start + 2 * i;
        mesh.indices.push(u16::from_le_bytes([d[at], d[at + 1]]));
    }
    Ok(mesh)
}

impl Mesh {
    /// Every index must point at a real vertex and the triangle count must make sense.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.positions.len();
        if let Some(bad) = self.indices.iter().find(|&&i| i as usize >= n) {
            return Err(format!("index {bad} but only {n} vertices"));
        }
        if self.indices.len() % 3 != 0 {
            return Err(format!("{} indices is not a whole number of triangles", self.indices.len()));
        }
        Ok(())
    }

    /// Stricter check used by `xgm-scan`: finite positions inside the bounding box (small tolerance), parallel arrays of
    /// the right length, sub meshes inside the index list and the vertex range they declare, weights summing to 1.
    pub fn validate_deep(&self, material_count: usize) -> Result<(), String> {
        self.validate()?;
        let n = self.positions.len();
        for p in &self.positions {
            if p.iter().any(|x| !x.is_finite()) {
                return Err("non finite position".into());
            }
        }
        for (name, len) in [("normals", self.normals.len()), ("colors", self.colors.len()), ("uvs", self.uvs.len())] {
            if len != 0 && len != n {
                return Err(format!("{name} has {len} entries for {n} vertices"));
            }
        }
        if self.uvs.iter().any(|u| !u[0].is_finite() || !u[1].is_finite()) {
            return Err("non finite uv".into());
        }
        if self.platform == 6 {
            // note: the bbox stored in the file is NOT checked, it is in another space for ~2/3 of the meshes
            for s in &self.submeshes {
                let hi = s.first_index as usize + 3 * s.tri_count as usize;
                if hi > self.indices.len() {
                    return Err("sub mesh runs past the index list".into());
                }
                if let Some(m) = s.material {
                    if m >= material_count {
                        return Err("sub mesh material out of range".into());
                    }
                }
                if let (Some(&lo), Some(&mx)) = (self.indices[s.first_index as usize..hi].iter().min(), self.indices[s.first_index as usize..hi].iter().max()) {
                    let dynv = self.skin.as_ref().map_or(0, |k| k.skinned_vertices as usize);
                    let shift = if dynv > 0 && !s.skinned() { dynv } else { 0 };
                    // the lowest index used is exactly `first_vertex`; the highest is first_vertex + vertex_count or one less
                    let (flo, fhi) = (s.first_vertex as usize, s.first_vertex as usize + s.vertex_count as usize);
                    if (lo as usize) != flo + shift || (mx as usize) > fhi + shift {
                        return Err(format!("sub mesh uses vertices {lo}..{mx} but declares {flo}..{fhi}"));
                    }
                }
            }
            if let Some(sk) = &self.skin {
                for v in 0..sk.skinned_vertices as usize {
                    let k = self.influences[v] as usize;
                    let sum: f32 = self.bone_weights[v][..k].iter().sum();
                    if (sum - 1.0).abs() > 0.03 {
                        return Err(format!("vertex {v} weights sum to {sum}"));
                    }
                    if self.bone_indices[v][..k].iter().any(|&b| b as u32 >= sk.num_bones) {
                        return Err(format!("vertex {v} uses a bone beyond {}", sk.num_bones));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Wavefront OBJ: one object per mesh, one group + `usemtl` per sub mesh, texture coordinates as stored (v not flipped).
/// Positions have `Mesh::anim_matrix` applied (outline meshes), so every part sits in the same space.
pub fn to_obj(x: &Xgm) -> String {
    let mut out = String::new();
    if !x.materials.is_empty() {
        out.push_str("mtllib model.mtl\n");
    }
    let mut base = 1usize;
    for (m, mesh) in x.meshes.iter().enumerate() {
        if mesh.name.is_empty() {
            out.push_str(&format!("o mesh{m}\n"));
        } else {
            out.push_str(&format!("o {}\n", mesh.name.replace(' ', "_")));
        }
        for p in &mesh.transformed_positions() {
            out.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
        }
        for t in &mesh.uvs {
            out.push_str(&format!("vt {} {}\n", t[0], t[1]));
        }
        for n in &mesh.normals {
            out.push_str(&format!("vn {} {} {}\n", n[0], n[1], n[2]));
        }
        let face = |out: &mut String, t: &[u16]| {
            let (a, b, c) = (t[0] as usize + base, t[1] as usize + base, t[2] as usize + base);
            match (mesh.uvs.is_empty(), mesh.normals.is_empty()) {
                (true, true) => out.push_str(&format!("f {a} {b} {c}\n")),
                (true, false) => out.push_str(&format!("f {a}//{a} {b}//{b} {c}//{c}\n")),
                (false, true) => out.push_str(&format!("f {a}/{a} {b}/{b} {c}/{c}\n")),
                (false, false) => out.push_str(&format!("f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}\n")),
            }
        };
        if mesh.submeshes.is_empty() {
            for t in mesh.indices.chunks_exact(3) {
                face(&mut out, t);
            }
        } else {
            for (si, s) in mesh.submeshes.iter().enumerate() {
                out.push_str(&format!("g mesh{m}_part{si}\n"));
                if let Some(mi) = s.material {
                    out.push_str(&format!("usemtl {}\n", obj_mat_name(x, mi)));
                }
                let lo = (s.first_index as usize).min(mesh.indices.len());
                let hi = (lo + 3 * s.tri_count as usize).min(mesh.indices.len());
                for t in mesh.indices[lo..hi].chunks_exact(3) {
                    face(&mut out, t);
                }
            }
        }
        base += mesh.positions.len();
    }
    out
}

fn obj_mat_name(x: &Xgm, i: usize) -> String {
    let n = x.materials.get(i).map(|m| m.name.replace(' ', "_")).unwrap_or_default();
    if n.is_empty() { format!("material{i}") } else { format!("{n}#{i}") }
}

/// Companion `.mtl` for `to_obj` (colour 0 as Kd, colour 2 as Ks, first texture slot as map_Kd).
pub fn to_mtl(x: &Xgm) -> String {
    let mut out = String::new();
    for (i, m) in x.materials.iter().enumerate() {
        out.push_str(&format!("newmtl {}\n", obj_mat_name(x, i)));
        out.push_str(&format!("Kd {} {} {}\n", m.colors[0][0], m.colors[0][1], m.colors[0][2]));
        out.push_str(&format!("Ks {} {} {}\n", m.colors[2][0], m.colors[2][1], m.colors[2][2]));
        for (ty, name) in &m.textures {
            if (*ty == 0 || *ty == 5) && !name.is_empty() {
                out.push_str(&format!("map_Kd {}\n", name.replace(".tga", ".png")));
                break;
            }
        }
        out.push('\n');
    }
    out
}

/// Wireframe preview into an RGBA buffer: three orthographic views (front, side, top) side by side
/// (`anim_matrix` applied).
pub fn preview(x: &Xgm, size: usize) -> (usize, usize, Vec<u8>) {
    let (w, h) = (size * 3, size);
    let mut img = vec![255u8; w * h * 4];
    for px in img.chunks_exact_mut(4) {
        px[0] = 24;
        px[1] = 28;
        px[2] = 36;
    }
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let world: Vec<Vec<[f32; 3]>> = x.meshes.iter().map(|m| m.transformed_positions()).collect();
    for pts in &world {
        for p in pts {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    if min[0] > max[0] {
        return (w, h, img);
    }
    let extent = (0..3).map(|k| max[k] - min[k]).fold(0.0f32, f32::max).max(1e-6);
    let scale = size as f32 * 0.85 / extent; // largest dimension fills 85% of a view
    let centre = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, (min[2] + max[2]) / 2.0];
    // (horizontal axis, vertical axis) per view: front = x/y, side = z/y, top = x/z
    let views = [(0usize, 1usize), (2, 1), (0, 2)];
    for (v, &(ax, ay)) in views.iter().enumerate() {
        let ox = (v * size + size / 2) as f32;
        let oy = (size / 2) as f32;
        let project = |p: &[f32; 3]| (ox + (p[ax] - centre[ax]) * scale, oy - (p[ay] - centre[ay]) * scale);
        for (m, wp) in x.meshes.iter().zip(&world) {
            for t in m.indices.chunks_exact(3) {
                let pts = [project(&wp[t[0] as usize]), project(&wp[t[1] as usize]), project(&wp[t[2] as usize])];
                for e in 0..3 {
                    line(&mut img, w, h, pts[e], pts[(e + 1) % 3], [120, 200, 255]);
                }
            }
        }
    }
    (w, h, img)
}

fn line(img: &mut [u8], w: usize, h: usize, a: (f32, f32), b: (f32, f32), rgb: [u8; 3]) {
    let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil() as usize).max(1);
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        if x >= 0.0 && y >= 0.0 && (x as usize) < w && (y as usize) < h {
            let at = ((y as usize) * w + x as usize) * 4;
            img[at..at + 3].copy_from_slice(&rgb);
        }
    }
}

/// Chunk 0x12 body (`TXGSAnimHeader`, 5 words: type, matrix/bone count, frames, pointer slot, spare) and the size of the
/// data block (chunk 0x13) that follows it. Every shipped model has type 0 (static): the block is a u32 then `count`
/// 4x4 matrices, returned in `matrices`. Other types (1 / 2 / 3 = keyframed) are not decoded here (see the `.xga` reader).
#[derive(Clone, Debug, Default)]
pub struct AnimHeader {
    pub words: [u32; 5],
    pub block_bytes: usize,
    pub matrices: Vec<[f32; 16]>,
    /// Index of the mesh (0x11 marker count) this header belongs to.
    pub mesh_index: usize,
}

impl Mesh {
    /// Vertex positions with `anim_matrix` applied (row vector convention); identical to `positions` when there is none.
    pub fn transformed_positions(&self) -> Vec<[f32; 3]> {
        match &self.anim_matrix {
            None => self.positions.clone(),
            Some(m) => self
                .positions
                .iter()
                .map(|p| {
                    [
                        p[0] * m[0] + p[1] * m[4] + p[2] * m[8] + m[12],
                        p[0] * m[1] + p[1] * m[5] + p[2] * m[9] + m[13],
                        p[0] * m[2] + p[1] * m[6] + p[2] * m[10] + m[14],
                    ]
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn asset(rel: &str) -> Option<Vec<u8>> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
        std::fs::read(p).ok()
    }

    #[test]
    fn element_decoding() {
        // byte4n: signed / 127, ubyte4n: unsigned / 255, short2: raw, short2n: / 32767
        let d = [127u8, 0x81, 0, 127, 255, 0, 51, 255, 0x10, 0x00, 0xF0, 0xFF, 0xFF, 0x7F, 0x01, 0x80];
        let (v, _) = decode_element(&d, 0, 0x14).unwrap();
        assert_eq!(v, [1.0, -127.0 / 127.0, 0.0, 1.0]);
        let (v, raw) = decode_element(&d, 4, 0x10).unwrap();
        assert_eq!(raw, [255, 0, 51, 255]);
        assert!((v[2] - 0.2).abs() < 1e-6);
        let (v, _) = decode_element(&d, 8, 0x15).unwrap();
        assert_eq!(&v[..2], &[16.0, -16.0]);
        let (v, _) = decode_element(&d, 12, 0x19).unwrap();
        assert_eq!(&v[..2], &[1.0, -32767.0 / 32767.0]);
        assert!(decode_element(&d, 0, 0x30).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"nope").is_err());
        let mut d = vec![0u8; 0x30];
        d[8..12].copy_from_slice(b"XGSM");
        d[4] = 24;
        let x = parse(&d).unwrap();
        assert!(x.meshes.is_empty() && x.chunks.is_empty());
    }

    #[test]
    fn bird_2_9() {
        let Some(d) = asset("assets292/pak/characters/models/red_l02.xgm") else { return };
        let x = parse(&d).unwrap();
        assert_eq!(x.version, 0x0101_0126);
        let m = &x.meshes[0];
        assert_eq!((m.name.as_str(), m.platform, m.stride), ("Red", 6, 24));
        assert_eq!((m.positions.len(), m.indices.len() / 3), (1875, 3398));
        assert_eq!((m.normals.len(), m.colors.len(), m.uvs.len()), (1875, 1875, 1875));
        m.validate_deep(x.materials.len()).unwrap();
        assert_eq!(x.materials[0].name, "ABG_CAR1_Char~Red");
        assert_eq!(x.materials[0].textures[1].1, "red_bird_dif.tga");
        let sk = &x.skeletons[m.skeleton.unwrap()];
        assert_eq!(sk.bones.len(), 34);
        assert_eq!(sk.bones[0].name, "Beek");
        assert_eq!(sk.bones[26].name, "Root");
        assert_eq!(sk.bones[26].parent, None);
        assert_eq!(sk.bones[4].parent, Some(17));
        // child / sibling lists form one tree: exactly one root, every other bone has a parent
        assert_eq!(sk.bones.iter().filter(|b| b.parent.is_none()).count(), 1);
        let info = m.skin.as_ref().unwrap();
        assert_eq!((info.num_bones, info.skinned_vertices, info.blocks.len()), (34, 1875, 132));
        for v in 0..1875 {
            let n = m.influences[v] as usize;
            let s: f32 = m.bone_weights[v][..n].iter().sum();
            assert!((s - 1.0).abs() < 0.01);
        }
        // uv = short * scale + bias lands in 0..1 for this mesh
        assert!(m.uvs.iter().all(|u| (0.0..=1.0).contains(&u[0]) && (0.0..=1.0).contains(&u[1])));
        assert_eq!(x.nodes.len(), 4);
    }

    #[test]
    fn kart_2_9() {
        let Some(d) = asset("assets292/pak/cars/theme002/cargeom/kart_red_upgrade2/chassis_l02.xgm") else { return };
        let x = parse(&d).unwrap();
        let m = &x.meshes[0];
        m.validate_deep(x.materials.len()).unwrap();
        assert_eq!(m.submeshes.len(), 2);
        assert_eq!(m.submeshes.iter().map(|s| s.tri_count).sum::<u32>() as usize, m.indices.len() / 3);
        assert!(x.nodes.iter().any(|n| n.name == "front_left_wheel" && n.scale[0] > 1.7));
        let size = m.bbox_max[1] - m.bbox_min[1];
        assert!(size > 0.5 && size < 2.0);
        // the outline mesh carries the y/z swapping matrix that maps it onto the base mesh
        let Some(o) = asset("assets292/pak/cars/theme002/cargeom/kart_red_upgrade2/chassis_outline_l02.xgm") else { return };
        let o = parse(&o).unwrap();
        let om = &o.meshes[0];
        assert!(om.anim_matrix.is_some());
        let t = om.transformed_positions();
        let hi_z = t.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        assert!((hi_z - 1.22875).abs() < 1e-3, "{hi_z}");
    }

    #[test]
    fn old_1_0_1_files_still_parse() {
        let Some(d) = asset("assets/pak/cargeom/kart_base/chassis_l02.xgm") else { return };
        let x = parse(&d).unwrap();
        assert_eq!(x.meshes[0].platform, 1);
        assert!(x.meshes[0].validate().is_ok());
        assert!(!x.meshes[0].uvs.is_empty());
        assert!(x.nodes.iter().any(|n| n.name.contains("wheel")));
    }
}
