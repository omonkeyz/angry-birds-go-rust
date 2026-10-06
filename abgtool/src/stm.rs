//! STME static environment (`track.stm`) - the 2.9.x `CXGSEnv` streaming container.
//! See FORMATS.md section "STME" for the layout; every rule here comes from the decompile of
//! `CXGSEnv::LoadInitialData / LoadPVS / LoadMaterials / LoadSplines / LoadCameras / NonStreamedLoad` or from the data.

pub fn u32_at(d: &[u8], at: usize) -> u32 {
    d.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
pub fn f32_at(d: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(d, at))
}
pub fn u16_at(d: &[u8], at: usize) -> u16 {
    d.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

#[derive(Debug, Clone)]
pub struct Toc {
    pub kind: u32,
    pub name: String,
    pub offset: u32,
    pub disk_size: u32,
    pub skip_a: u32,
    pub skip_b: u32,
    pub size: u32,
    pub fixup_bytes: u32,
}

pub fn toc(d: &[u8]) -> Result<Vec<Toc>, String> {
    if d.len() < 16 || &d[0..4] != b"STME" {
        return Err("stm: bad magic".into());
    }
    let n = u32_at(d, 12) as usize;
    let mut v = Vec::new();
    for i in 0..n {
        let o = 16 + 64 * i;
        if o + 64 > d.len() {
            return Err("stm: toc past end".into());
        }
        let name_b = &d[o + 4..o + 36];
        let len = name_b.iter().position(|&b| b == 0).unwrap_or(32);
        v.push(Toc {
            kind: u32_at(d, o),
            name: String::from_utf8_lossy(&name_b[..len]).into_owned(),
            offset: u32_at(d, o + 0x28),
            disk_size: u32_at(d, o + 0x2C),
            skip_a: u32_at(d, o + 0x30),
            skip_b: u32_at(d, o + 0x34),
            size: u32_at(d, o + 0x38),
            fixup_bytes: u32_at(d, o + 0x3C),
        });
    }
    Ok(v)
}

struct Rd<'a> {
    d: &'a [u8],
    at: usize,
    end: usize,
}
impl<'a> Rd<'a> {
    fn u32(&mut self) -> Result<u32, String> {
        if self.at + 4 > self.end {
            return Err(format!("stm: read past end of block at 0x{:X}", self.at));
        }
        let v = u32_at(self.d, self.at);
        self.at += 4;
        Ok(v)
    }
    fn f32(&mut self) -> Result<f32, String> {
        self.u32().map(f32::from_bits)
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.at + n > self.end {
            return Err(format!("stm: read of {n} bytes past end of block at 0x{:X}", self.at));
        }
        let s = &self.d[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn skip(&mut self, n: usize) -> Result<(), String> {
        self.bytes(n).map(|_| ())
    }
}
fn cstr(b: &[u8]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..n]).into_owned()
}

#[derive(Debug, Clone, Default)]
pub struct Helper {
    pub name: String,
    /// 4x4 matrix (16 floats as stored) - second 0x40 block
    pub matrix: [f32; 16],
    pub raw_name_block: Vec<u8>,
}
#[derive(Debug, Clone, Default)]
pub struct Spline {
    pub name: String,
    pub points: Vec<[f32; 3]>,
    /// Optional per-point 0x1c-byte records (7 floats), present when the spline flag byte is non-zero.
    pub extra: Vec<[f32; 7]>,
}
#[derive(Debug, Clone, Default)]
pub struct Camera {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub rotations: Vec<[f32; 4]>,
}
/// One 0x160-byte material description as stored in the pvs block (raw; interpretation in FORMATS.md).
#[derive(Debug, Clone)]
pub struct MaterialDesc {
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct Pvs {
    pub probe_sets: u32,
    pub section_count: u32,
    pub cell_count: u32,
    pub cell_ids: Vec<u32>,
    /// `vis[cell]` = bitset over sections (ceil(N/32) words per cell)
    pub vis: Vec<Vec<u32>>,
    pub base_section_count: u32,
    pub base_of_section: Vec<u32>,
    pub vis_base: Vec<Vec<u32>>,
    pub section_names: Vec<String>,
    pub helpers: Vec<Helper>,
    pub cell_extra: Vec<u32>,
    pub splines: Vec<Spline>,
    pub spline_flag: u8,
    pub cameras: Vec<Camera>,
    pub markup: Vec<Vec<u8>>,
    pub texture_names: Vec<String>,
    /// `texture_vis[texture]` = bitset over sections that use the texture
    pub texture_vis: Vec<Vec<u32>>,
    pub material_refs: Vec<MaterialRef>,
    pub materials: Vec<MaterialDesc>,
    pub env_vec3: [f32; 3],
    pub env_matrix: [f32; 16],
    pub cell_bounds: Vec<[f32; 6]>,
}

fn parse_pvs(d: &[u8], e: &Toc) -> Result<Pvs, String> {
    let start = e.offset as usize;
    let end = start + e.size as usize;
    if end > d.len() {
        return Err("stm: pvs past end of file".into());
    }
    let mut r = Rd { d, at: start, end };
    let mut p = Pvs::default();
    // InitialiseProbeSets: count, then per set: size, (align 16 from the stream position), skip size
    p.probe_sets = r.u32()?;
    for _ in 0..p.probe_sets {
        let sz = r.u32()? as usize;
        if sz != 0 {
            let pos = r.at - start;
            let aligned = (pos + 15) & !15;
            r.skip(aligned - pos + sz)?;
        }
    }
    p.section_count = r.u32()?;
    p.cell_count = r.u32()?;
    let n = p.section_count as usize;
    let c = p.cell_count as usize;
    for _ in 0..c {
        p.cell_ids.push(r.u32()?);
    }
    let w = (n + 31) / 32;
    for _ in 0..c {
        let mut row = Vec::with_capacity(w);
        for _ in 0..w {
            row.push(r.u32()?);
        }
        p.vis.push(row);
    }
    p.base_section_count = r.u32()?;
    for _ in 0..n {
        p.base_of_section.push(r.u32()?);
    }
    let w2 = (p.base_section_count as usize + 31) / 32;
    for _ in 0..c {
        let mut row = Vec::with_capacity(w2);
        for _ in 0..w2 {
            row.push(r.u32()?);
        }
        p.vis_base.push(row);
    }
    for _ in 0..n {
        p.section_names.push(cstr(r.bytes(0x24)?));
    }
    let k = r.u32()? as usize;
    for _ in 0..k {
        let nb = r.bytes(0x40)?.to_vec();
        let mb = r.bytes(0x40)?;
        let mut m = [0f32; 16];
        for i in 0..16 {
            m[i] = f32_at(mb, i * 4);
        }
        p.helpers.push(Helper { name: cstr(&nb), matrix: m, raw_name_block: nb });
    }
    for _ in 0..c + 2 {
        p.cell_extra.push(r.u32()?);
    }
    // LoadSplines
    let sn = r.u32()? as usize;
    let flag = r.bytes(1)?[0];
    p.spline_flag = flag;
    for _ in 0..sn {
        let name = cstr(r.bytes(0x20)?);
        let cnt = r.u32()? as usize;
        let mut s = Spline { name, ..Default::default() };
        for _ in 0..cnt {
            s.points.push([r.f32()?, r.f32()?, r.f32()?]);
        }
        if flag != 0 {
            for _ in 0..cnt {
                let mut e7 = [0f32; 7];
                for v in e7.iter_mut() {
                    *v = r.f32()?;
                }
                s.extra.push(e7);
            }
        }
        p.splines.push(s);
    }
    // LoadCameras
    let cn = r.u32()? as usize;
    for _ in 0..cn {
        let name = cstr(r.bytes(0x20)?);
        let cnt = r.u32()? as usize;
        let mut cam = Camera { name, ..Default::default() };
        for _ in 0..cnt {
            cam.positions.push([r.f32()?, r.f32()?, r.f32()?]);
        }
        for _ in 0..cnt {
            cam.rotations.push([r.f32()?, r.f32()?, r.f32()?, r.f32()?]);
        }
        p.cameras.push(cam);
    }
    // markup blocks
    let mn = r.u32()? as usize;
    for _ in 0..mn {
        p.markup.push(r.bytes(0x5c)?.to_vec());
    }
    // LoadMaterials
    let tn = r.u32()? as usize;
    for _ in 0..tn {
        let mut row = Vec::with_capacity(w);
        for _ in 0..w {
            row.push(r.u32()?);
        }
        p.texture_vis.push(row);
    }
    for _ in 0..tn {
        p.texture_names.push(cstr(r.bytes(0x24)?));
    }
    let bn = r.u32()? as usize;
    for _ in 0..bn {
        let b = r.bytes(12)?;
        p.material_refs.push(MaterialRef { vertex_flags: u16_at(b, 0), runtime: u16_at(b, 2), textures: [u16_at(b, 4), u16_at(b, 6), u16_at(b, 8), u16_at(b, 10)] });
    }
    for _ in 0..bn {
        p.materials.push(MaterialDesc { raw: r.bytes(0x160)?.to_vec() });
    }
    p.env_vec3 = [r.f32()?, r.f32()?, r.f32()?];
    for i in 0..16 {
        p.env_matrix[i] = r.f32()?;
    }
    for _ in 0..c {
        let mut b = [0f32; 6];
        for v in b.iter_mut() {
            *v = r.f32()?;
        }
        p.cell_bounds.push(b);
    }
    if r.at != end {
        return Err(format!("stm: pvs parse ended at +{} but block size is {}", r.at - start, e.size));
    }
    Ok(p)
}

pub fn parse_pvs_only(d: &[u8]) -> Result<Pvs, String> {
    let t = toc(d)?;
    let e = t.iter().find(|e| e.kind == 1).ok_or("stm: no pvs entry")?;
    parse_pvs(d, e)
}

/// The 12-byte record that precedes the 0x160-byte descriptions: `u16 vertex attribute flags` (argument of
/// `CXGSEnvOGL::Platform_GetVertexDescriptor`),
/// `u16` filled at load time (material manager handle), `u16 x4` texture-name indices (0xFFFF on disk; resolved by name at load).
#[derive(Debug, Clone, Copy, Default)]
pub struct MaterialRef {
    pub vertex_flags: u16,
    pub runtime: u16,
    pub textures: [u16; 4],
}

// ------------------------------------------------------------------------------------------------
// Sections (kind 6 = `.q02`, kind 7 = `.q03`, kind 8 = other): small in-memory image + streamed vertex data

#[derive(Debug, Clone, Default)]
pub struct SubMesh {
    /// index into `Pvs::materials` / `Pvs::material_refs`
    pub material: u32,
    /// byte offset of this submesh's first vertex inside the section's vertex buffer
    pub vertex_byte_offset: u32,
    pub first_triangle: u32,
    pub triangle_count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Section {
    pub name: String,
    pub kind: u32,
    /// translation stored in the block header (vertices are already in world space; this equals the bbox centre)
    pub position: [f32; 3],
    pub alpha: f32,
    pub bbox_center: [f32; 3],
    pub bbox_half: [f32; 3],
    pub submeshes: Vec<SubMesh>,
    pub index_count: u32,
    pub vertex_count: u32,
    pub vertex_bytes: u32,
    pub indices: Vec<u16>,
    /// raw vertex buffer (`vertex_bytes` long); stride = vertex_bytes / vertex_count
    pub vertices: Vec<u8>,
}
impl Section {
    pub fn stride(&self) -> usize {
        if self.vertex_count == 0 { 0 } else { (self.vertex_bytes / self.vertex_count) as usize }
    }
}

fn parse_section(d: &[u8], e: &Toc) -> Result<Section, String> {
    let base = e.offset as usize;
    let size = e.size as usize;
    let disk = e.disk_size as usize;
    if base + disk > d.len() || size > disk {
        return Err(format!("stm: section {} past end of file", e.name));
    }
    let blk = &d[base..base + size];
    let ptr_model = u32_at(blk, 0) as usize;
    let ptr_stream = u32_at(blk, 8) as usize;
    if ptr_model + 0x30 > size || ptr_stream + 0x2c > size {
        return Err(format!("stm: section {} header pointers out of range", e.name));
    }
    let mut s = Section { name: e.name.clone(), kind: e.kind, ..Default::default() };
    for k in 0..3 {
        s.position[k] = f32_at(blk, 0x10 + 4 * k);
    }
    s.alpha = f32_at(blk, 0x1c);
    let ptr_subs = u32_at(blk, ptr_model + 0x10) as usize;
    let nsub = u32_at(blk, ptr_model + 0x18) as usize;
    for k in 0..3 {
        s.bbox_center[k] = f32_at(blk, ptr_model + 0x1c + 4 * k);
        s.bbox_half[k] = f32_at(blk, ptr_model + 0x28 + 4 * k);
    }
    if ptr_subs + nsub * 0x28 > size {
        return Err(format!("stm: section {} submesh table out of range", e.name));
    }
    for i in 0..nsub {
        let r = ptr_subs + i * 0x28;
        s.submeshes.push(SubMesh {
            material: u32_at(blk, r + 0x18),
            vertex_byte_offset: u32_at(blk, r + 0x1c),
            first_triangle: u16_at(blk, r + 0x20) as u32,
            triangle_count: u32_at(blk, r + 0x24),
        });
    }
    s.index_count = u32_at(blk, ptr_stream + 0x20);
    s.vertex_count = u32_at(blk, ptr_stream + 0x24);
    s.vertex_bytes = u32_at(blk, ptr_stream + 0x28);
    let ib = s.index_count as usize * 2;
    let vb = s.vertex_bytes as usize;
    let sa = base + disk;
    if sa + ib + vb > d.len() {
        return Err(format!("stm: section {} stream data past end of file", e.name));
    }
    for i in 0..s.index_count as usize {
        s.indices.push(u16_at(d, sa + 2 * i));
    }
    s.vertices = d[sa + ib..sa + ib + vb].to_vec();
    Ok(s)
}

pub fn sections(d: &[u8]) -> Result<Vec<Section>, String> {
    let t = toc(d)?;
    let mut v = Vec::new();
    for e in t.iter().filter(|e| (6..=8).contains(&e.kind)) {
        v.push(parse_section(d, e)?);
    }
    Ok(v)
}

// ------------------------------------------------------------------------------------------------
// Materials

/// A material of the track: matlib name + texture names + vertex layout flags.
#[derive(Debug, Clone, Default)]
pub struct Material {
    /// 0x40-byte name at +0x116 of the description, e.g. `ABG_ENV1_Track~ABK_CP_Bamboo_bridge01`
    /// (= material name in shaders.xmat, `~`, instance name).
    pub name: String,
    /// texture slot names (`xxx.tex` -> `environments/themeNNN/textures/xxx.xgt*`); only non-empty slots are listed
    pub textures: Vec<String>,
    /// `Platform_GetVertexDescriptor` flags: 0x04 normal (BYTE4N), 0x08 colour (UBYTE4N), 0x20 uv0 (SHORT2),
    /// 0x40 uv1 (SHORT2), 0x10 aux (BYTE4N). Other bits (0x01,0x02,0x80) are not vertex attributes.
    pub vertex_flags: u16,
    pub colour0: u32,
    pub colour1: u32,
    pub raw: Vec<u8>,
}

fn make_material(r: &MaterialRef, d: &MaterialDesc) -> Material {
    let raw = &d.raw;
    let mut textures = Vec::new();
    for k in 0..4 {
        let o = 0x16 + 0x40 * k;
        let s = cstr(&raw[o..o + 0x40]);
        if !s.is_empty() && s.ends_with(".tex") {
            textures.push(s);
        }
    }
    Material {
        name: cstr(&raw[0x116..0x156]),
        textures,
        vertex_flags: r.vertex_flags,
        colour0: u32_at(raw, 0),
        colour1: u32_at(raw, 4),
        raw: raw.clone(),
    }
}

/// Number of bytes a vertex with these attribute flags occupies.
pub fn vertex_stride(flags: u16) -> usize {
    12 + 4 * (flags & (0x04 | 0x08 | 0x10 | 0x20 | 0x40)).count_ones() as usize
}

// ------------------------------------------------------------------------------------------------
// Decoded meshes

#[derive(Debug, Clone, Copy, Default)]
pub struct Vertex {
    pub pos: [f32; 3],
    /// BYTE4N / 127 (zero when the material has no normals)
    pub normal: [f32; 3],
    /// SHORT2 / 2048 (zero when absent)
    pub uv: [f32; 2],
    pub uv2: [f32; 2],
    /// the raw SHORT2 values of uv / uv2 (what the GPU gets; the vertex shader multiplies by g_vTex0_OffsetScale_VSC.xy)
    pub uv_raw: [i16; 2],
    pub uv2_raw: [i16; 2],
    /// RGBA8 (255,255,255,255 when absent)
    pub color: [u8; 4],
    /// raw 4 bytes of the optional 0x10 attribute
    pub aux: [u8; 4],
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    /// section name, e.g. `road_00.q02`
    pub section_name: String,
    pub section: usize,
    /// quality number of the section name (`q02` -> 2, `q03` -> 3)
    pub quality: u8,
    pub submesh: usize,
    /// index into `Stm::materials`
    pub material: usize,
    pub vertex_flags: u16,
    pub vertices: Vec<Vertex>,
    /// triangle list, indices relative to `vertices`
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct SectionInfo {
    pub name: String,
    pub quality: u8,
    /// index into the PVS base-section table (sections with equal base are LODs of each other)
    pub base: u32,
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
    pub index_count: u32,
    pub vertex_count: u32,
}

fn quality_of(name: &str) -> u8 {
    name.rsplit('.').next().and_then(|e| e.strip_prefix('q')).and_then(|n| n.parse().ok()).unwrap_or(0)
}

pub fn decode_vertices(buf: &[u8], flags: u16, count: usize) -> Vec<Vertex> {
    let stride = vertex_stride(flags);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let v = &buf[i * stride..(i + 1) * stride];
        let mut o = 12;
        let mut x = Vertex { color: [255; 4], ..Default::default() };
        x.pos = [f32_at(v, 0), f32_at(v, 4), f32_at(v, 8)];
        if flags & 0x04 != 0 {
            x.normal = [v[o] as i8 as f32 / 127.0, v[o + 1] as i8 as f32 / 127.0, v[o + 2] as i8 as f32 / 127.0];
            o += 4;
        }
        if flags & 0x08 != 0 {
            x.color = [v[o], v[o + 1], v[o + 2], v[o + 3]];
            o += 4;
        }
        let s16 = |at: usize| u16_at(v, at) as i16 as f32 / 2048.0;
        if flags & 0x20 != 0 {
            x.uv = [s16(o), s16(o + 2)];
            x.uv_raw = [u16_at(v, o) as i16, u16_at(v, o + 2) as i16];
            o += 4;
        }
        if flags & 0x40 != 0 {
            x.uv2 = [s16(o), s16(o + 2)];
            x.uv2_raw = [u16_at(v, o) as i16, u16_at(v, o + 2) as i16];
            o += 4;
        }
        if flags & 0x10 != 0 {
            x.aux = [v[o], v[o + 1], v[o + 2], v[o + 3]];
        }
        out.push(x);
    }
    out
}

fn section_meshes(s: &Section, sec_index: usize, materials: &[Material]) -> Result<Vec<Mesh>, String> {
    let mut out = Vec::new();
    let q = quality_of(&s.name);
    // the submesh triangle ranges must tile the index buffer exactly
    let mut ranges: Vec<(u32, u32)> = s.submeshes.iter().map(|m| (m.first_triangle, m.triangle_count)).collect();
    ranges.sort();
    let mut next = 0u32;
    for (f, c) in ranges {
        if f != next {
            return Err(format!("stm: {} submesh triangle ranges do not tile the index buffer (expected {next}, got {f})", s.name));
        }
        next = f + c;
    }
    // index_count is rounded up to an even number (one padding index after the last triangle)
    if next as usize * 3 > s.indices.len() || s.indices.len() - next as usize * 3 > 1 {
        return Err(format!("stm: {} submeshes cover {next} triangles but the section has {} indices", s.name, s.indices.len()));
    }
    for (si, m) in s.submeshes.iter().enumerate() {
        let a = m.first_triangle as usize * 3;
        let b = a + m.triangle_count as usize * 3;
        if b > s.indices.len() {
            return Err(format!("stm: {} submesh {si} indices out of range", s.name));
        }
        let idx = &s.indices[a..b];
        let nv = idx.iter().copied().max().map_or(0, |m| m as usize + 1);
        let mat = m.material as usize;
        let flags = materials.get(mat).ok_or_else(|| format!("stm: {} submesh {si} material {mat} out of range", s.name))?.vertex_flags;
        let stride = vertex_stride(flags);
        let o = m.vertex_byte_offset as usize;
        if o + nv * stride > s.vertices.len() {
            return Err(format!("stm: {} submesh {si} vertices out of range", s.name));
        }
        out.push(Mesh {
            section_name: s.name.clone(),
            section: sec_index,
            quality: q,
            submesh: si,
            material: mat,
            vertex_flags: flags,
            vertices: decode_vertices(&s.vertices[o..], flags, nv),
            indices: idx.iter().map(|&i| i as u32).collect(),
        });
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------
// Collision (kd trees)

#[derive(Debug, Clone, Copy, Default)]
pub struct CollisionTri {
    pub v: [[f32; 3]; 3],
    pub normal: [f32; 3],
    /// u16 at +0x26 of the 0x58-byte CXGSTriangle (surface material id)
    pub material: u16,
    /// u16 at +0x24
    pub flags: u16,
    /// u32 at +0x50
    pub user: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct KdNode {
    /// split plane position (inner node) or byte offset of the leaf's triangle pointer list (leaf)
    pub value: f32,
    /// low 2 bits: split axis 0/1/2 or 3 = leaf; upper bits: first child index (inner) or triangle count (leaf)
    pub info: u32,
}

#[derive(Debug, Clone, Default)]
pub struct KdTree {
    pub name: String,
    /// grid cell parsed from `kdN_x_y.dat`
    pub cell: [u32; 2],
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
    pub vertices: Vec<KdVertex>,
    pub triangles: Vec<CollisionTri>,
    /// leaf triangle references, as triangle indices
    pub leaf_triangles: Vec<u32>,
    pub nodes: Vec<KdNode>,
}

#[derive(Debug, Clone, Default)]
pub struct KdGrid {
    /// min/max of the height axis (this+0x1e0)
    pub y_range: [f32; 2],
    pub dims: [u32; 2],
    pub origin: [f32; 2],
    /// 1 / cell size along x and z
    pub inv_cell: [f32; 2],
    /// per cell: 0xFF = none, else tree `kd{b+1}_{x}_{y}.dat`
    pub cells: Vec<u8>,
}

fn parse_kd(d: &[u8], e: &Toc) -> Result<KdTree, String> {
    let base = e.offset as usize;
    let size = e.size as usize;
    if base + size > d.len() || size < 0x48 {
        return Err(format!("stm: kd {} out of range", e.name));
    }
    let b = &d[base..base + size];
    let nodes_p = u32_at(b, 0) as usize;
    let leaves_p = u32_at(b, 8) as usize;
    let tris_p = u32_at(b, 0x10) as usize;
    let verts_p = u32_at(b, 0x18) as usize;
    let (nv, nt, nl, nn) = (u32_at(b, 0x38) as usize, u32_at(b, 0x3c) as usize, u32_at(b, 0x40) as usize, u32_at(b, 0x44) as usize);
    if verts_p + nv * 16 > size || tris_p + nt * 0x58 > size || leaves_p + nl * 8 > size || nodes_p + nn * 16 > size {
        return Err(format!("stm: kd {} tables out of range", e.name));
    }
    let mut t = KdTree { name: e.name.clone(), ..Default::default() };
    for k in 0..3 {
        t.bbox_min[k] = f32_at(b, 0x20 + 4 * k);
        t.bbox_max[k] = f32_at(b, 0x2c + 4 * k);
    }
    for i in 0..nv {
        let o = verts_p + i * 16;
        t.vertices.push(KdVertex { pos: [f32_at(b, o), f32_at(b, o + 4), f32_at(b, o + 8)], color: [b[o + 12], b[o + 13], b[o + 14], b[o + 15]] });
    }
    for i in 0..nt {
        let o = tris_p + i * 0x58;
        let mut tri = CollisionTri::default();
        for k in 0..3 {
            let p = u32_at(b, o + 8 * k) as usize;
            if p < verts_p || (p - verts_p) % 16 != 0 || (p - verts_p) / 16 >= nv {
                return Err(format!("stm: kd {} triangle {i} vertex pointer 0x{p:X} invalid", e.name));
            }
            let v = &t.vertices[(p - verts_p) / 16];
            tri.v[k] = v.pos;
        }
        tri.normal = [f32_at(b, o + 0x18), f32_at(b, o + 0x1c), f32_at(b, o + 0x20)];
        tri.flags = u16_at(b, o + 0x24);
        tri.material = u16_at(b, o + 0x26);
        tri.user = u32_at(b, o + 0x50);
        t.triangles.push(tri);
    }
    for i in 0..nl {
        let p = u32_at(b, leaves_p + i * 8) as usize;
        if p < tris_p || (p - tris_p) % 0x58 != 0 || (p - tris_p) / 0x58 >= nt {
            return Err(format!("stm: kd {} leaf entry {i} 0x{p:X} invalid", e.name));
        }
        t.leaf_triangles.push(((p - tris_p) / 0x58) as u32);
    }
    for i in 0..nn {
        let o = nodes_p + i * 16;
        t.nodes.push(KdNode { value: f32_at(b, o), info: u32_at(b, o + 8) });
    }
    let n = e.name.trim_end_matches(".dat");
    let parts: Vec<&str> = n.trim_start_matches(|c: char| c.is_ascii_alphabetic()).split('_').collect();
    if parts.len() == 3 {
        t.cell = [parts[1].parse().unwrap_or(0), parts[2].parse().unwrap_or(0)];
    }
    Ok(t)
}

fn parse_kd_grid(d: &[u8], e: &Toc) -> Result<KdGrid, String> {
    let base = e.offset as usize;
    if base + 32 > d.len() {
        return Err("stm: kd meta out of range".into());
    }
    let b = &d[base..];
    let mut g = KdGrid {
        y_range: [f32_at(b, 0), f32_at(b, 4)],
        dims: [u32_at(b, 8), u32_at(b, 12)],
        origin: [f32_at(b, 16), f32_at(b, 20)],
        inv_cell: [f32_at(b, 24), f32_at(b, 28)],
        cells: Vec::new(),
    };
    let n = g.dims[0] as usize * g.dims[1] as usize;
    if 32 + n > e.size as usize || n > 1 << 20 {
        return Err("stm: kd meta grid out of range".into());
    }
    g.cells = b[32..32 + n].to_vec();
    Ok(g)
}

// ------------------------------------------------------------------------------------------------
// Whole file

#[derive(Debug, Clone, Default)]
pub struct Stm {
    /// u32 at 4 (0x01010115 in 2.9.x)
    pub version: u32,
    /// u32 at 8 ("30LG")
    pub tag: [u8; 4],
    pub toc: Vec<Toc>,
    pub pvs: Pvs,
    pub materials: Vec<Material>,
    /// names of the textures the track needs (`xxx.tex`)
    pub textures: Vec<String>,
    pub sections: Vec<SectionInfo>,
    /// every submesh of every section (all qualities)
    pub meshes: Vec<Mesh>,
    pub kd_grid: KdGrid,
    pub kd_trees: Vec<KdTree>,
}

impl Stm {
    /// Meshes of one quality level; for bases that lack that level, the nearest existing one
    /// (by quality number) is used so every base section is drawn exactly once.
    pub fn meshes_for_quality(&self, quality: u8) -> Vec<&Mesh> {
        let mut by_base: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
        for (i, s) in self.sections.iter().enumerate() {
            by_base.entry(s.base).or_default().push(i);
        }
        let mut chosen = std::collections::BTreeSet::new();
        for (_, list) in by_base {
            let pick = list
                .iter()
                .copied()
                .min_by_key(|&i| ((self.sections[i].quality as i32 - quality as i32).abs(), self.sections[i].quality));
            if let Some(i) = pick {
                chosen.insert(i);
            }
        }
        self.meshes.iter().filter(|m| chosen.contains(&m.section)).collect()
    }
    /// All collision triangles of all kd trees (world space).
    pub fn collision_triangles(&self) -> Vec<CollisionTri> {
        self.kd_trees.iter().flat_map(|t| t.triangles.iter().copied()).collect()
    }
    pub fn spline(&self, name: &str) -> Option<&Spline> {
        self.pvs.splines.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }
}

pub fn parse(d: &[u8]) -> Result<Stm, String> {
    let toc = toc(d)?;
    let pvs_e = toc.iter().find(|e| e.kind == 1).ok_or("stm: no pvs entry")?;
    let pvs = parse_pvs(d, pvs_e)?;
    let materials: Vec<Material> = pvs.material_refs.iter().zip(&pvs.materials).map(|(r, m)| make_material(r, m)).collect();
    let mut stm = Stm {
        version: u32_at(d, 4),
        tag: [d[8], d[9], d[10], d[11]],
        textures: pvs.texture_names.clone(),
        materials,
        ..Default::default()
    };
    for (i, name) in pvs.section_names.iter().enumerate() {
        let e = toc
            .iter()
            .find(|e| (6..=8).contains(&e.kind) && e.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("stm: section {name} listed in the pvs but missing from the toc"))?;
        let s = parse_section(d, e)?;
        let mut info = SectionInfo {
            name: s.name.clone(),
            quality: quality_of(&s.name),
            base: pvs.base_of_section.get(i).copied().unwrap_or(i as u32),
            index_count: s.index_count,
            vertex_count: s.vertex_count,
            ..Default::default()
        };
        for k in 0..3 {
            info.bbox_min[k] = s.bbox_center[k] - s.bbox_half[k];
            info.bbox_max[k] = s.bbox_center[k] + s.bbox_half[k];
        }
        stm.sections.push(info);
        stm.meshes.extend(section_meshes(&s, i, &stm.materials)?);
    }
    stm.pvs = pvs;
    if let Some(e) = toc.iter().find(|e| e.kind == 2) {
        stm.kd_grid = parse_kd_grid(d, e)?;
    }
    for e in toc.iter().filter(|e| e.kind == 3) {
        stm.kd_trees.push(parse_kd(d, e)?);
    }
    stm.toc = toc;
    Ok(stm)
}

/// 16-byte collision vertex: position + RGBA (baked colour; 0xFFFFFFFF where unused).
#[derive(Debug, Clone, Copy, Default)]
pub struct KdVertex {
    pub pos: [f32; 3],
    pub color: [u8; 4],
}

impl Pvs {
    /// helper (named transform) by exact, case-insensitive name
    pub fn helper(&self, name: &str) -> Option<&Helper> {
        self.helpers.iter().find(|h| h.name.eq_ignore_ascii_case(name))
    }
    /// all helpers whose name starts with `prefix` (case-insensitive), e.g. `smck_xml_`, `vfx_`, `spline_startline`
    pub fn helpers_with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a Helper> + 'a {
        let p = prefix.to_ascii_lowercase();
        self.helpers.iter().filter(move |h| h.name.to_ascii_lowercase().starts_with(&p))
    }
}
