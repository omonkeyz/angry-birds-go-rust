# Angry Birds Go! v1.0.1 - decoded formats

Source: `Angry Birds Go! (v1.0.1).apk`, package `com.rovio.angrybirdsgo`. One native library
(`libABK.so`, ARMv7), OpenGL ES, custom asset formats. Not Unity.

## KPX archive (`*.pak`) - DONE (abgtool: `list`, `unpack`)
All little endian. Magic `01 4B 50 58`.

Version 1 (all paks except cargeom):
- 0x04 u32 version = 1, 0x08 u32 entry count, 0x0C u32 name-table size
- 0x30 entry records, 32 bytes: name_off u32, 0, size u32, offset u32, compressed u32 (1 = zlib),
  mtime u32, compressed_size u32, 0
- names (NUL separated) follow the records, then the data. zlib streams start `78 DA`.

Version 0x41 (`cars/cargeom.pak`, 2584 files in 64 kart folders, 64-bit fields):
- directory table at 0x40: tuple k = (0, file_count_k, name_off_{k+1}, first_file_{k+1}) as u64s;
  directory 0 starts at name 0 / file 0; the last tuple is only 16 bytes
- flat file table right after: name_off u64, size u32, offset u32, flag u32, mtime u32, csize u64
- names follow the file table. Files here are stored uncompressed.

## XGST texture (`*.xgt`) - DONE (abgtool: `xgt`, `xgt-info`, `convert-all`)
32-byte header: "XGST", u32 0x001A0920, u8 mips @8, u8 format @12, u16 w,h @16, u16 used w,h @20,
u32 payload size @28. Payload = mip 0 first, then smaller mips.
Formats: 0x02 RGBA4444 (top nibble = R), 0x03 RGBA8888, 0x0D luminance+alpha 8-bit (fonts),
0xFC ETC1 (4x4 blocks, row-major).

## Still to decode
- `.xgm` models (2221 in cargeom + envobjects) - next phase
- `.xga` animation, `.xlc`, `.fnt` font metrics, `.atlas` sprite rects, `.xmat` shaders
- `XOX1` binary XML (the `.xml` entries inside the paks)
- game logic: only in `libABK.so`

## XOX1 tokenised XML (`*.xml`) - DONE (abgtool: `xox`, `xox-all`)
"XOX1", u32 N strings, N+1 cumulative u32 end offsets, N NUL-terminated strings, then XML text where every name and
value is a token: a run of bytes from a fixed 115-byte table read as base-115 digits (see `abgtool/src/xox.rs`).
Structural bytes `< > / = "` and whitespace are literal.

## XGSM model (`*.xgm`) - geometry DONE (abgtool: `xgm-info`, `xgm-obj`, `xgm-preview`)
u32 21, u32 header size, "XGSM"; chunks `(u32 id, u32 size incl. header)`. Mesh chunk 0x31: bbox, vertex/index byte counts,
stride (28 = pos f32x3, normal i8x4, colour u8x4, uv f32x2; 20 = pos + uv), vertices then u16 triangle indices at the end.
Nodes (wheel hubs, pilot seats): 0x60-byte block with a 32-byte name, followed by a 56-byte transform chunk
(position f32x3, quaternion f32x4, scale f32x3). Materials / skinning not decoded yet.

## XGSF font (`*.fnt`), XGSA atlas (`*.atlas`) - DONE
Font: "XGSF", glyph sheet width, count, pages; 12-byte glyph records (code, x, y, w, h). Glyph sheets are LA88 `.xgt`.
Atlas: "XGSTA", count, 40-byte records (hash, x/y/w/h as fractions of the texture, w/h in pixels), then one block of
NUL-separated sprite paths.

## XGSM animation (`*.xga`) - DONE (abgtool: `anim-info`, `anim-scan`, `anim-xref`, `anim-sample`; library `abgtool::anim`)
An `.xga` is **not** a separate format: it is an XGSM model container (same family as `.xgm`) that carries animation
chunks. The engine loads it with `CXGSAnim::CXGSAnim(const char*)` -> `CXGSModel::InitModel` -> `CXGS_XGMLoader::*`
(2.9.1 decompile). The `CXGSAnimation` / `CXGSAnimController` path (magic "XGSA", version 0x15, event emitters) is a
different engine path that no shipped `.xga` uses, so there are no events/markers in these files (no 0x2e/0x2f chunks).
271 files under `assets292/pak` (characters, environments theme002/006, envobjects): all parse, zero validation problems (7124 tracks, 286226 keys).

Header (little endian): `u32` (low 16 bits must be 0x15, high half is noise e.g. 0x6e, 0x6f, 0x12, 0xfdc8), `u32` header size 0x18,
"XGSM", `u32` format version: 0x01010126 in 270 files, 0x01010127 in one (`sennahelmetbird_readytorace.xga`). `CanLoadXGS` maps
0x26 / 0x27 / 0x28 to loader table index 9 / 10 / 11; the table was decoded from the binary (base address verified against the
dispatchers): index 9 = anim header `LoadAnimHeader_02`, anim block `LoadAnimBlock_04`, physique header `_03`, physique block
`_04`; index 10 and 11 differ only in the physique block (`_06`, no rotation negation),
`u32` bone count, `u32` collision blocks. Then chunks `u32 id, u32 size` (size includes the 8 byte header; only the
low 16 bits of id are tested by the loader, the high half is noise) until chunk 0x16 (size 8) at end of file.

Chunks that matter:
- `0x1f` extended header (9 words): [1] helper count, [2] f32 frame rate (30.0; 24.0 in one file), [3] f32 key stride
  (1, 2 or 4 frames between dense samples), [0]/[6] 0, [7] 0x0200001C/1E/28 (unresolved).
- `0x1b` physique header + `0x1c` block (all character animations): header `u32 type, u32 count, u32 frames, u32 (runtime),
  u32 tracks`, then `u32[tracks]` (unknown meaning, kept raw), and for type 2 `tracks x 0x30` bytes track headers whose
  only used field is the key count at +0x1c. Chunk 0x1c: type 2 = per track `keycount x 0x3c` keys, then `u16[frames]`
  frame -> key index map. Types 1 and 4 = `count x tracks` dense 0x3c records, sample-major (index = sample*tracks+track).
  Header `count` = total keys (type 2) or sample count (1/4), `frames` = animation length in frames.
- `0x12` anim header (0x14 bytes, same fields) + `0x13` block (4 skipped bytes then `count x 0x3c` dense keys of one track):
  props / environment objects (type 1). The track is named by the group's `0x2d` chunk (u16, name; name field is 31 chars max).
- `0x1e` track name, one chunk per track in track order: `u16 index, NUL terminated name` (bytes after the NUL are stale).
- `0x11` model chunk (0x54 bytes, all zero here) closes a group; `0x25` hierarchy: `u16 nodes, u16, u32,` nodes x 0x48
  (`u8 first child, u8 next sibling` (0xff none; a walk from the unreferenced roots reaches every node exactly once in all 244 character files), `u16`, `f32[16]` static node matrix, `u32`).
- `0x22` helper header (0x58 bytes, name at +0, u32 sample limit at +0x40) + `0x23` helper blocks (`u16 index, u8 pos?, u8 rot?,
  u8 scale?`, then f32[3], f32[4] (xyz negated by the loader), f32[3]); each shipped block holds one component; the file has
  samples 0..=limit but the loader keeps only index < limit (60 helpers have limit 1 -> only sample 0 is kept). In-engine helper
  sampling was not traced.
  `0x30` PVS, `0x17`, `0x2a/0x2b/0x2c` collision hull: kept as raw chunk references.
A file holds one or more *groups* (1 for all character animations; up to 15 for rigid multi-object props such as the train
or cable cars). Group boundary = the `0x11` chunk (the loader's model index advances there).

Key record, 0x3c bytes (`TXGSAnimKeySampleTM`): `+0 f32 1/(frame - previous key frame)` (the engine reads the NEXT key's
+0 as the interpolation scale; 0 for key 0; sparse tracks end with a copy of the last key at the same frame and inv_dt 0),
`+4 u16 frame`, `+6 u8`, `+7 u8` (non-unit-scale flag), `+8 f32[3] scale`, `+0x14 f32[4] rotation x y z w`,
`+0x24 f32[3] translation`, `+0x30 f32[3]` (unresolved, 1.0 or junk).
For format 0x26 `LoadPhysiqueBlock_04` / `LoadAnimBlock_04` negate x, y, z of the stored rotation after reading (the file
holds the conjugate); `abgtool::anim::Key::rot` is the in-memory value the sampler uses. The 0x27 physique block (`_06`) is
not negated (decompile only: one file, no data cross-check possible; the 0x13 block loader is `_04` in every version).
The engine builds the row-vector 4x4 with rotation block m[1] = 2(xy+zw), m[4] = 2(xy-zw) (i.e. transpose of the standard
quaternion matrix) times scale, translation in row 3 (`Trs::to_matrix`).

Sampling (`CXGSAnimBlend::Update`, `GenerateAnimationData*`): `frame = clamp(time * fps, 0, frames-1)`; duration = frames / fps.
Type 2: `k = map[floor(frame)]`; one-key tracks are constant; else `w = (frame - key[k].frame) * key[k+1].inv_dt`,
`lerp` scale/translation, rotation = shortest-path lerp, switching to slerp below dot 0.99 (constants read from the .so;
the slerp branch is garbled in the decompile and implemented as standard slerp), no renormalisation. Types 1/4:
`i = floor(frame/stride)`, `f = (frame - i*stride)/stride`, interpolate sample i -> i+1 when f > 1e-5. Dense sample i is
stored at frame `min(i*stride, frames)`. The engine's later fix-up (ext word 6 == 1: negate matrix column 0, otherwise swap columns 1 and 2 = Y/Z) is not applied by
`sample()`; it runs only in the plain path of storage type 1 (the 80 prop track sets, `TrackSet::engine_swaps_yz()`), never
for storage 2 or 4 (all characters).

Verified on all 271 files by `abgtool anim-scan assets292/pak`: exact chunk sizes (0x1b, 0x1c, 0x13), key total ==
header count, strictly increasing key frames, `inv_dt == 1/dt`, the frame map invariant
`key[map[f]].frame <= f < key[map[f]+1].frame` for every frame of every track, no NaN/inf, all groups agree on `frames`,
helper count == ext word. Targets: for all 244 character animations the 0x1e names (30/24/25/... bones) equal, index by
index, the 0x1e names of a character `.xgm`, and the 0x25 hierarchy head/tail bytes (child/sibling links) are identical
(animation dir -> model: bluebird -> blue, corporalpig -> helmet_pig, kingpig -> king_pig, redbird -> red, sennabird -> senna, ...).
Environment animations name their object in 0x2d; 79 of 93 names are found in a sibling `.xgm`. Of the 14 others, the pirate pig
names (`pig_pirate_002`, `pig_pirate_02`) are in `smackables/smck_piratepig*_3m.xgm`; the 4 cable car files (`CableCar01_Anim02_...`,
31-character names) were found nowhere else under pak.

Other animation-like files: `*.scml` (2, `ui/ui_core/spriter`) are plain-XML Spriter 2D animations (BrashMonkey SCML 1.0:
entity -> animation (length ms, looping) -> mainline keys + timelines with `<object folder file x y angle scale_x scale_y a>`),
decoded by `abgtool::anim::scml` / `abgtool anim-scml`. `*.meta` (4) are crash-report text (`ProductName=XGS&BuildID=...`),
`locdb.xlc` is the "XGSL" localisation database: neither is an animation.

## STME static environment (`track.stm`, 2.9.x) - DONE (abgtool: `stm-info`, `stm-obj`, `stm-col`, `stm-scan`, `stm-toc`, `stm-dump`, `stm-preview`; library `abgtool::stm::parse`)
Source: decompile of `CXGSEnv::LoadInitialData / LoadTOC / LoadPVS / LoadMaterials / LoadSplines / LoadCameras / NonStreamedLoad /
PointerFixup / FindTOCEntry`, `CXGSEnvOGL::Platform_GetVertexDescriptor`, `CXGSKDTree::LoadHandle / Fixup`, `CXGSEnv::RayIntersect` (all
2.9.1 `libABK291.so`). 15 files (`environments/theme00N/tracks/runNNN/track.stm`, 3-26 MB), all parse; the file is tiled exactly by its
entries (checked by `cargo test --test stm_tracks`). All little endian. A track = one `CXGSEnv`: a streaming container of in-memory images.

### Container
```
0x00 "STME"
0x04 u32 version 0x01010115
0x08 "30LG" (u32 0x474C3033)
0x0C u32 N   entry count
0x10 N x 64-byte TOC entries, sorted by strcasecmp(name) (FindTOCEntry binary-searches them)
     +0x00 u32 kind   1 pvs, 2 kd meta, 3 kd tree, 4 ".tex" (name only), 5 ".mp1" (name only), 6 section ".q02", 7 section ".q03", (8 handled like 6/7, unused)
     +0x04 char[0x24] name, NUL padded
     +0x28 u32 file offset of the entry
     +0x2C u32 bytes the entry occupies on disk (image + fixup table + pad to 16; for kinds 6/7 NOT including the streamed data, see below)
     +0x30,+0x34 u32 split/skip fields of NonStreamedLoad (0 in every shipped file)
     +0x38 u32 image size in bytes
     +0x3C u32 size in bytes of the pointer-fixup table that follows the image
```
Entries of kind 4/5 are names only (size 0): `xxx.tex` = texture `environments/themeNNN/textures/xxx.xgt*` (full size), `xxx.mp1` = the
reduced-size variant the loader uses when the low-memory flag (`CXGSEnv+0xC8`) is set (`LoadMaterials` builds the `.mp1` name with `sprintf(".mp%d", 1)`).
Everything after the TOC is laid out in order: pvs, kd meta, kd tree(s), then sections in streaming order.

Memory-image entries (kinds 2, 3, 6, 7) hold block-relative pointers. After the image comes the fixup table: u16 entries (= byte offset / 4)
when the image is <= 0x40000 bytes, else u32 byte offsets; each names a u32 in the image that the loader adds the load address to. The parser
does not need the table because the layouts are known, but any consumer that wants raw structs does.

### PVS block (kind 1, `trackN.pvs`) - `CXGSEnv::LoadPVS`, sequential reads
```
u32 probe_set_count; per set: u32 size, if size!=0: skip to 16-byte alignment (relative to the block start) then skip size bytes   (0 in all files)
u32 N sections, u32 C visibility cells
u32 cell_id[C]
u32 vis[C][ceil(N/32)]          bit s of row c = section s is potentially visible from cell c
u32 M base sections
u32 base_of_section[N]          sections with equal base are alternative-quality versions of each other
u32 vis_base[C][ceil(M/32)]
char name[N][0x24]              TOC names of the sections ("road_00.q02"); the loader looks them up in the TOC
u32 K helpers; K x { char name[0x40]; f32 matrix[16] (row-major 4x4, translation in elements 12..14) }
u32 cell_extra[C+2]             meaning unknown
splines:  u32 S; u8 flag(=1); S x { char name[0x20]; u32 n; f32 pos[n][3]; if flag: f32 extra[n][7] }
cameras:  u32 count (0 in all files); count x { char name[0x20]; u32 n; f32 pos[n][3]; f32 rot[n][4] }
markup:   u32 count (0 in all files); count x 0x5c bytes
materials: u32 T textures; u32 tex_vis[T][ceil(N/32)] (which sections use texture t); char tex_name[T][0x24] ("xxx.tex")
           u32 B materials; B x 12-byte refs; B x 0x160-byte descriptions
f32 env_vec3[3]; f32 env_matrix[16]; f32 cell_bounds[C][6] (min xyz, max xyz of each visibility cell)
```
The block ends exactly after `cell_bounds` in all 15 files.

Material 12-byte ref: `u16 vertex_flags` (argument of `CXGSEnvOGL::Platform_GetVertexDescriptor`), `u16` runtime material-manager handle (0 on disk), `u16 tex[4]`
(0xFFFF on disk; the loader fills them by matching slot names against the texture list).
Material 0x160-byte description (packed): `+0x00 u32 colour0 (0xFF000000 typical), +0x04 u32 colour1 (e.g. 0x00969696), +0x08..0x15 flag bytes (byte +0x0B = 0x19 seen),
+0x16 4 texture slots of 0x40 bytes {NUL-terminated "xxx.tex", then uninitialised stack bytes}` (only slot 0 carries a name in the shipped data),
`+0x116 char name[0x40]` = shaders.xmat material name `~` instance, e.g. `ABG_ENV1_Track~trkcen_fair`, `ABG_ENV1_Terrain~Rock_Base`,
`ABG_ENV1_TerrainFlag0Wave~barriers`; `+0x158 f32` (9.8627 in all track materials), `+0x15C u32` (0 or 4).

Helpers (`K` of them, 70-172 per track) are the named transforms the game turns into objects: `spline_startline`, `spline_finishline`,
`spline_trackendline`, `spline_smashline`, `spline_splittime*` (race logic, matched with StringPartialMatch in the env loader), `smck_xml_N_M`
(smackable markers), `gameplay_*`, `vfx_*` / `flare_*` / `sprite_helper*` (effects), `photocam_pos` / `phototrigger_pos`,
`camera_racefinish_position` / `_aim`, `end_camera_*`, `shadowsourcepos`, `animgeo_*` (animated props on theme006).
The start line is the helper `spline_startline` (CEnvObjectManager records its object); the starting-grid slots themselves are not stored in the track.

Splines (4-10 per track): `race_001..` (the racing line, the driving AI follows these; their weights come from track.xml `<Spline name min_ai_weighting max_ai_weighting>`),
`centre_00N`, `camposition_track`, `camposition_path` (camera rails, `CSpline` types by name: race_=0, camposition_path=2, camposition_track=3, other=4).
Per point: position, plus 7 floats: `[0..3]` a unit vector perpendicular to the path (the surface up/normal at that point), `[3]` a height-like value
(about y minus 10 on theme002/run000), `[4]`, `[5]` track half-widths left/right (about 20, capped at 50 where there is no edge), `[6]` read as an int by `CSpline::CSpline` (value 1; >=0 marks the point).
The race spline floats 0-5 m above the collision surface (checked), and lies over a collision triangle at >=90% of its points in all 15 tracks.

### Sections (kinds 6 `.q02` and 7 `.q03`) - the visible geometry
A section is a 192-1100 byte image followed, in the file, by its streamed vertex data:
```
image (offsets relative to the image start)
  +0x00 u32 -> model (always 0x20)         +0x08 u32 -> stream table
  +0x10 f32 position[3] (= bbox centre), +0x1C f32 alpha (1.0)
  model: +0x10 u32 -> submesh array, +0x18 u32 submesh count, +0x1C f32 bbox centre[3], +0x28 f32 bbox half extents[3]
  submesh (0x28 bytes; the first 0x18 are runtime fields):
         +0x18 u32 material index (into the pvs materials), +0x1C u32 byte offset of its first vertex in the vertex buffer,
         +0x20 u16 first triangle, +0x24 u32 triangle count  (the u16 at +0x22 is a second count that is 0 for big meshes - ignored)
  stream table: +0x20 u32 index count, +0x24 u32 vertex count (summed over submeshes), +0x28 u32 vertex buffer bytes
data after the image (at offset + disk size):  u16 index[index_count]; then the vertex buffer; the pair is padded so that the next entry starts
  at the next 16-byte boundary strictly above (len + 16) & ~15
```
Indices are relative to the submesh's first vertex (triangle list, 3 per triangle, one buffer for the whole section; submesh k uses triangles
`first .. first+count`). Submesh vertex ranges are NOT ordered by offset. Vertices are world-space (no node transform; checked against the collision surface).
Vertex layout depends only on the material's `vertex_flags` (decoded from `Platform_GetVertexDescriptor` and the GL type table at 0xCF97B4):
```
f32 x,y,z                                  12 bytes, always
bit 0x04  normal   BYTE4N  (x,y,z,w) /127  -> unit length in 100% of 160k vertices tested
bit 0x08  colour   UBYTE4N RGBA
bit 0x20  uv0      SHORT2 (not normalised)
bit 0x40  uv1      SHORT2
bit 0x10  attr5    BYTE4N (usage 6 -> attribute 5), meaning unknown
```
in that order; stride = 12 + 4 x (number of those bits): seen flags 0x00 (12), 0x08 (16), 0x28 (20), 0x2C / 0x2F / 0xAC / 0xAF (24), 0x7C (32), 0xA9 / 0xAB (20), 0x8B (16).
Bits 0x01, 0x02, 0x80 are not vertex attributes. UV: the vertex shader computes `uv = g_vTex0_OffsetScale_VSC.zw + i_vTex0 * g_vTex0_OffsetScale_VSC.xy`;
prop materials use the whole 0..2048 range for one texture repeat, so xy = 1/2048 (see UNRESOLVED in the report); `Vertex::uv_raw` keeps the integers.
`.q02` is the full material set (several submeshes); `.q03` of the same base has the same triangle count and one vertex-colour-only submesh
(flags 0x08, no texture). `LoadPVS` swaps q02 for q03 in the visibility sets when the init-params low-quality flag (`CXGSEnv+0x40`) is set.
Typical track: 90-230 sections, 130-450 submeshes, 0.14-0.28 M triangles (q02).

### Collision (kind 2 grid + kind 3 kd trees)
`KDMetaData1.dat` (36 bytes): `f32 y_min, y_max` (this+0x1E0), `u32 W, H` grid cells (+0x1E8), `f32 origin_x, origin_z` (+0x1F0), `f32 inv_cell_x, inv_cell_z` (+0x1F8),
then W*H bytes: 0xFF = no tree, else tree file `kd{b+1}_{x}_{y}.dat` (`CXGSEnv::GetCellFromPos`: `ix = (x - origin_x) * inv_cell_x`, `iz = (z - origin_z) * inv_cell_z`).
All shipped tracks use a single 1x1 cell (`kd1_0_0.dat`) covering the whole circuit.
`kd1_0_0.dat` is a `CXGSKDTree` memory image (3.6-8 MB, ~0.6 MB of u32 fixups):
```
+0x00 u32 -> nodes (16 bytes each)   +0x08 u32 -> leaf list (8 bytes each: u32 -> triangle, 4 pad)
+0x10 u32 -> triangles (0x58 each)   +0x18 u32 -> vertices (16 bytes each)
+0x20 f32 bbox min[3]   +0x2C f32 bbox max[3]
+0x38 u32 vertex count  +0x3C u32 triangle count  +0x40 u32 leaf entries  +0x44 u32 node count
vertex   f32 x,y,z + RGBA8 (baked colour, 0xFFFFFFFF where unused)
triangle +0x00 u32 v0 ptr, +0x08 u32 v1 ptr, +0x10 u32 v2 ptr (each followed by 4 pad bytes), +0x18 f32 normal[3] (matches the geometry in 100%),
         +0x24 u16 flags (0, 1 or 2), +0x26 u16 surface material id, +0x28 .. +0x50 precomputed plane/edge floats (CXGSTriangle::Setup),
         +0x50 u32 = triangle index + 1
node     +0x00 f32 split position (inner) or byte offset into the leaf list (leaf), +0x08 u32 info: low 2 bits axis 0/1/2 or 3 = leaf,
         upper bits child index (inner) / triangle count (leaf)
```
Surface ids seen: 1,2,3,5,7,9,13,15,25,29,30,32-36. `MaterialCollisionCallback` (the ray filter) ignores 7, 9, 29, 30, 37, 38. The names of the ids are not in the track data.
11k-26k triangles per track; the circuit extent is 1-3.7 km. `abgtool stm-preview` draws the triangles (coloured by surface id) with all splines (white) top-down.

## XGST textures, 2.9.x (`*.xgt`, `*.xgt_dxt`, `*.xgt_etc`, `*.xgt_pvr`, `*.xgt_atc`) - DONE (abgtool: `convert-all`, `xgt::decode_bytes`)
Same 32-byte header as 1.0.1 (see above) but version word `0x001C0020` and byte 12 is the engine's `EXGSBaseTexFormat` enum
(`TXGSTexture_FileHandlerXGT::Load` maps the old 0x1A/0x1B header versions onto it; names from the table behind
`XGSTex_GetBaseTextureFormatName` in libABK291; value = index): 1 R5G6B5, 2 R5G5B5A1, 3 R4G4B4A4, 4 R8G8B8A8, 5 R8G8B8, 6 R8G8,
7 L8, 8 L8A8, 12 A8, 24 DXT1, 25 DXT3, 26 DXT5, 30 PVRTC4_RGB, 31 PVRTC4_RGBA, 35 ETC1, 37 ATC_RGB, 38 ATC_RGBA_EXP,
39 ATC_RGBA_INT, 52 ETC2_RGB, 53 ETC2_RGBA (full list in `xgt::base_format_name`).
Packed 16-bit formats are little-endian words with the first-named channel in the top bits (0xRGBA, like GL_UNSIGNED_SHORT_4_4_4_4 /
5_5_5_1). Pixel rows are stored top row first (no flip needed; checked visually on UI, kart and track textures). Mips follow mip 0.

Variants: every texture is either a plain `.xgt` (426 files; formats 2, 3, 4, 8, 35) or exists as four device variants
(411 stems): `.xgt_dxt` (always DXT1, 0x18), `.xgt_etc` (ETC1 0x23, or R4G4B4A4 for 20 textures with alpha), `.xgt_pvr` (0x1E/0x1F) and
`.xgt_atc` (0x25/0x27). Preference order used by abgtool: plain, dxt, etc, pvr, atc (`xgt::variant_rank`, `xgt::choose_variant`).
837 textures -> 837 PNG, 0 failures; PVRTC and ATC are not decoded (never needed: every stem has a DXT variant). DXT vs ETC decodes of the same
texture agree to a mean 4.8 / 255 absolute error over 410 comparable textures (`abgtool tex-compare`), which validates both decoders.
Some `.xgt_etc` files are stored at a different (smaller, non power of two) size than the `.xgt_dxt` of the same stem.

## XOX2 tokenised XML (2.9.x `*.xml`) - DONE (abgtool: `xox`, `xox-all`)
Container as XOX1 but magic `XOX2` and its own digit alphabet. A token (every element / attribute name and every value) is a run of
alphabet bytes read as base-115 digits, most significant first, and the number is the index of the string in the table
(`XGSXMLObfuscator_IndexDeobfuscate` + `CXGSXmlReader::NodeDeobfuscate` + `CreateXmlDoc` in libABK291). The document is wrapped in one
element named by the literal byte `x`; the reader skips it and iterates its children. Closing tags repeat the opening token. Alphabet
(`xox::xox2_alphabet`): the 115 bytes in the order of first appearance in a document (78 65 D3 E2 6E F3 DA ...), except that 0x78 sits at
index 39 instead of first. Strings used only by the dropped remains of `<!-- -->` comments (" -->\n\t") are in the table but never
referenced. 1855 pak xml + 41 apk xml + 31 track xml all decode and validate (all tags balanced, every string used).
`analytics/eligo.xml` and `deviceconfigs/*.json` are XXTEA encrypted (`XGSEncrypt_decryptXXTEA`, standard algorithm, 16 byte key at
Ghidra 0xCDE9F0 = 94 44 16 44 18 01 4E 30 85 1A 2E 5A 3B 7B 3D 8D as LE words); `abgtool deviceconfig-all` and `xox-all` decrypt them.

## XMAT material library (`shaders.xmat`, version 0x21) - structure DONE, render-state flags NOT decoded (abgtool: `xmat-info`, `xmat::parse`)
See the layout comment at the top of `abgtool/src/xmat.rs`. Summary: header (version 0x21, 2, 4, scene count 19), 19 scene names (64 bytes) and
scene records (lighting parameters, not decoded), 1585 GLSL ES shader sources (1242 vertex, 343 fragment; each with a bitmask of the scenes that use it),
272 materials. A material has constants (name, hash, engine id, type word, 16 default floats), samplers (raw words) and pass names
(technique chain, e.g. ABG_ENV1_Track -> ABG_ENV2_Track ... DepthPass, DepthPassFE, VelocityPass). Material names used by the track models are
the xmat material names themselves: ABG_ENV1_Track (ENV1..ENV4 = shader tiers), ABG_ENV<n>_Terrain*, ABG_ENV<n>_TrackSnow ...; the part after `~`
in an STM material name is not in the xmat (no `~` in the file): it names the texture instance. Constants whose engine id is not 1 are supplied
by the engine at run time (FogParams 0x24, FogColour 0x23, Tex0_OffsetScale 0x25, matWorld 0x08, matWorldViewProj 0x0A, matViewInv 0x0C);
`Tex0_OffsetScale` has no default value in the file.

## XGSM model (`*.xgm`) - 1.0.1 and 2.9.x, DONE (abgtool: `xgm-info`, `xgm-obj`, `xgm-scan`, `xgm-skin`, `xgm-preview`)
Source: `CXGSModel::InitModel`, `CXGS_XGMLoader::*`, `CXGSModelUnified::LoadPlatformModel`, `CXGSPlatformMesh::*`,
`CXGSMaterial::*`, `DoSkinBlock*` in libABK 2.9.1. All integers little endian. 3010/3010 files of `assets292/pak` and
2221/2221 of the 1.0.1 `assets` parse; 2.9 meshes: 3381 (all platform 6), 1.0.1 meshes: 2246 (all platform 1).

**Header** (24 bytes): u32 21, u32 24 (chunk list start), "XGSM", u32 version, u32 a, u32 b (counts, ignored).
Version dword selects the loader set (`CanLoadXGS`): 0x01010126 (2914 files + all 1.0.1 files) and 0x01010128 (96 kart
files); 0x110-0x112, 0x120-0x125, 0x127 are accepted by the engine but never occur. Differences 126 -> 128:
quaternions in helper blocks (0x23), physique records (0x1C) are stored inverted in 126 (the loader negates x,y,z / words
5,6,7), stored directly in 128.

**Chunks**: `u32 id, u32 size` (size includes these 8 bytes), walked from offset 24 until id 0x16 (end marker, size 8);
bytes after it are not chunks. Mesh index = number of 0x11 end-of-mesh markers (size 8) seen so far; chunks 0x25 / 0x1B /
0x1C / 0x1E / 0x2D / 0x30 / 0x12 / 0x13 belong to the mesh that is current when they appear (they precede the 0x31 block
of that mesh). Seen in 2.9: 11 x3381, 12/13 x1189, 14 x4382, 16 x3010, 17 x35, 1A x5, 1B/1C/25 x40, 1E x1042, 1F x3010,
22/23 x3963, 2A x1135, 2B/2C x943, 2D x3381, 30 x432, 31 x3381.

| id | meaning |
|---|---|
| 0x11 | end of mesh (size 8; a larger body would be a legacy `TXGSModel`, never present) |
| 0x12 / 0x13 | embedded animation header (5 u32: type, count, frames, ptr slot, spare) + block. All 1189 are type 0: block = u32 + `count` 4x4 matrices (see "anim matrix") |
| 0x14 | material, 360 bytes (below) |
| 0x16 | end of chunk list |
| 0x17 0x18 0x19 0x1A 0x2A | collision sphere / box / ... / hull blocks (`CXGSCollisionObject::LoadChunk`: 0x17 sphere, 0x18 box, 0x2A trimesh); name (e.g. "collision") at +0x38 (char[32]). 0x2B / 0x2C hull vertex / face data. Not decoded beyond the name |
| 0x1B / 0x1C | physique header / block (skeleton rest pose), see below |
| 0x1E | bone name: u16 bone index, NUL padded name (36 byte body) |
| 0x1F | extended header (52 bytes of counts, only used for allocation) |
| 0x22 / 0x23 | helper node header (96 bytes) / transform block (see below) |
| 0x25 | hierarchy (bones), see below |
| 0x2D | mesh name: u16 index + name |
| 0x30 | PVS block (20 bytes), ignored |
| 0x31 | mesh |

### Mesh chunk 0x31 (platform id u16 at +0x0A)
The loader reads 0x2C header bytes, only platform 6 is accepted by 2.9.1 (others are skipped); platform 1 = the 1.0.1
layout (bbox +0x14/+0x20, vertex bytes u32 +0x2C, index bytes u32 +0x30, stride u32 +0x40, vertex data then u16 indices
at the end of the chunk; stride 28 = pos f32x3, normal i8x4 /127, colour u8x4, uv f32x2; stride 20 = pos, normal, colour).

Platform 6, offsets from the chunk start (all `offset` fields are relative to the chunk start):
```
+0x0A u16 6          +0x0C f32 bounding radius      +0x10 u32 render flags (e.g. 13, not decoded)
+0x14 f32[3] bbox min, +0x20 f32[3] bbox max  (= bbox of `positions * anim matrix`, z often mirrored: see UNRESOLVED)
+0x2C u16 vertex stride (20 or 24)       +0x2E u16 flags (bit 1 = 16 bit uvs need scale/bias)
+0x30 f32[16] matrix (identity everywhere)
+0x70 f32 uv scale u, +0x74 f32 uv scale v, +0x78 f32 uv bias u, +0x7C f32 uv bias v   (shader constant 0x21 = vec4)
+0x80 vertex data offset   +0x88 index data offset   +0xA0 skin struct offset (0 = none)
+0xA8 / +0xB0 vertex descriptor offset (both equal)  +0xB8 sub mesh table offset
+0xC0 u32 vertex data bytes   +0xC4 u32 index data bytes   +0xC8 u32 sub mesh count
```
Vertex data = `vertex bytes / stride` vertices, then (separately) u16 indices; triangle LIST (3 indices per triangle).
**Vertex descriptor** (`TXGSVertexDescriptor`): 0x18 byte elements `{i32 type, i32 usage, i32 usage index, i32 offset,
i32 stream, i32 stride}` terminated by type -1. Type: 0..3 float1-4, 0x10 ubyte4n, 0x11 ubyte4, 0x12 byte4, 0x13 ubyte4n,
0x14 byte4n, 0x15 short2, 0x16 short4, 0x17 ushort2, 0x18 ushort4, 0x19 short2n, 0x1A short4n, 0x1B ushort2n, 0x1C ushort4n
(table at 0xCF97B4: components, GL type, normalised). Usage = D3D declaration usage (0 position, 1 blend weight, 2 blend
indices, 3 normal, 5 texcoord, 6 tangent, 10 colour). Layouts that occur (every one of 3381 meshes):
* stride 24, flags 2 (2970): `0:f3 @0, normal 0x14 @12, colour 0x10 @16, texcoord 0x15 @20` = pos f32x3, normal i8x3(+pad)/127,
  colour u8x4 (file order), uv short2.
* stride 20 (411, outlines; 3 of them with uv short2 instead of colour): pos, normal, colour.
No second uv set, tangents or weights inside the vertex exist in any shipped file (the decoder handles usages 5.1 and 6).
**UV**: `uv = short * uv_scale + uv_bias` (flags bit 1; the loader converts SHORT2 to SHORT2N and multiplies the scale by
32767). Verified by rendering textured: red bird and karts, V is NOT flipped relative to the .xgt rows.
**Sub mesh table** (0x14 bytes each): u16 flags (0), u16 material index (0xFFFF none; index of the 0x14 chunk in file order),
u32 triangle count, u32 first index (index into the u16 index list, i.e. 3x triangles), u16 first vertex, u16 vertex count,
u32 tag (-1 in every file). The lowest index of a range equals first vertex, the highest is first+count or first+count-1.
The engine adds a per mesh base and, for a skinned buffer, treats `tag == -1` as "CPU skinned range"; every shipped mesh
has all vertices skinned or none, so indices are used as stored.

### Skin (struct at +0xA0, `CXGSSkinDataUnified`) - characters only (40 meshes)
```
+0x00 offset of blocks   +0x08 offset of weight stream   +0x20 u32 bone count   +0x24 u32 block count
+0x28 u32 skinned vertex count (the FIRST n vertices of the vertex array; here = all)   +0x2C u32 weight stream bytes
+0x30 u8 has normal (1)  +0x31 u8 extra 4 byte words after the normal copied unchanged (2: colour, uv)  +0x32 u8 has tangent
```
Block (12 bytes, `CXGSSkinBlockUnified`): u16 vertex count, u16 influences (1..8; 8 occurs), u8 bone[8] =
bone index (= skeleton bone, hierarchy / name / physique order). Blocks cover the skinned vertices in order. Weight stream:
per vertex `influences` bytes, `weight = byte / 255` (255.0 constant in `DoSkinBlock*`), consumed block by block; sums to 1.0
for every vertex. Skinned output = sum(weight * (v * bone matrix)), normals via the rotation part, /127 byte normals.

### Skeleton (chunks 0x25, 0x1E, 0x1B, 0x1C)
* 0x25 hierarchy: u16 node count (34), u16 (26, unused), u32 0, then 0x48 byte nodes: u8 first child (data), u8 next sibling (0xFF none; confirmed in `XGSResolveHierarchyChild`),
  u16 0xFFFF, f32[16] matrix (rows, translation in the last row; all zero for bones no vertex uses, else about the inverse of the chained rest pose, probably the inverse bind matrix - not reproduced exactly), u32 0xFFFFFF01 (byte 0x44 is a flag tested by the resolver). Parents follow from the child/sibling lists
  (root = a node nobody lists). Nodes are in alphabetical bone-name order.
* 0x1E names: one chunk per bone, u16 bone index + 31 chars.
* 0x1B physique header: u32 type (4), u32 frames (1), u32 (1), u32 0, u32 bone count, then bone count u32 (unresolved table).
  0x1C block: bone count x frames records of 60 bytes (type 1/4): `u32,u32 (0), f32 scale a[3], f32 rotation xyzw, f32 position,
  f32 scale b[3]`; the loader negates words 5,6,7 (rotation x,y,z) for version 126. Rest pose, local to the parent bone.

### Helper nodes (0x22 + 0x23)
0x22 (96 bytes): char name[32] at +8, u32 transform block count at +0x48. 0x23 (56 bytes): u16 frame, 3 flag bytes (position, rotation,
scale present), pad, then f32 position[3], f32 rotation xyzw[4], f32 scale[3] (each only if flagged). Wheel hubs, pilot seats
(`attach_pilot_1..3`), `front_left_wheel` ...; rotation_engine = conjugate of the file value in version 126.

### Anim matrix (chunk 0x12 type 0 / 0x13) - `Mesh.anim_matrix`
The engine renders mesh m with `world = mesh_matrix(+0x30, identity) * anim_matrix[m] * model_world` (`CXGSModelUnified::Render`:
MatrixMultiply32_Fast of `*(this+8)[m]` with the mesh matrix). Type 0 = static: block = u32 + `count` row-major 4x4 matrices,
row-vector convention `p * M`, translation in the last row. 1189 of 3381 meshes have one (822 base meshes + 367 outlines),
all non-identity: karts parts and outlines (typically scale 1.0..1.07 and y/z swapped = Z-up export -> Y-up, a reflection),
characters (scale 1.45 for the big ones), fx meshes, boat / cable car props, skyboxes. The stored mesh bbox is the bbox of the
*transformed* vertices (matches exactly, see UNRESOLVED for the z sign). Exposed as `Mesh.anim_matrix`, `Mesh::transformed_positions()`;
`xgm-obj` / `xgm-preview` apply it. `Mesh.positions` stays raw (consumers that ignore the matrix draw those meshes in the
export space).

### Material chunk 0x14 (360 bytes), offsets from the chunk start
```
+0x08 u32 colour 0, +0x0C u32 colour 1, +0x10 u32 colour 2  (bytes B,G,R,A; /255 -> RGBA floats)
+0x13 u8 = alpha of colour 2 -> engine field 0x188 = (byte/255) * constant (shininess-like)
+0x14 u16[4] texture slot types   +0x1C u16 texture count (<= 4)   +0x1E 4 x char[64] texture file names (.tga)
+0x11E char[64] material name e.g. "ABG_CAR1_Char~Red", "ABG_CAR1_Flat~ABK_Red kart..."  +0x164 u32 tag (usually 0)
```
Slot types seen: 0 base colour map (3947), 1 `*_shade` ramp (3643), 5 (25) and 6 (1) other maps; the loader sets render bit 0x10
if a slot is type 5. Texture files are `<name>.xgt` / `.xgt_etc` (e.g. `cars/theme00N/cartextures/`).

### Units / orientation
Karts are in metres (chassis about 1.1 x 1.0 x 2.5), Y up, Z along the kart; characters (birds, pigs) are stored Z-up, beak toward +Y
(the animation matrices / skeleton root orient them). Bird about 0.84 x 1.14 x 0.98.

### UNRESOLVED (xgm)
* **Mesh bbox (+0x14/+0x20)**: equals the bbox of `positions * anim_matrix` in ~2120 meshes outright (1228 without a matrix, 892 with) and with the Z axis mirrored
  (min/max = -max/-min) in ~1220 more (921 without a matrix, ~300 with) (e.g. kart chassis: vertex z -1.26..1.23, stored z -1.23..1.26); about 43 meshes match neither.
  Which sign the engine treats as "forward" for vertex z is not determined from the file; consumers should use the vertex data.
* **Physique table**: the `u32 x bone count` after the 0x1B header (values like 0x003E0000, 0x0070004E, mostly zero tail); meaning unknown.
  Also the exact build of the skin palette (bone matrix = inverse-bind * pose) and the node matrix convention above.
* Mesh `+0x10` u32 render flags (e.g. 13), mesh `+0x2E` bits other than 1, sub mesh tag `+0x10` (-1 always, so the static-range index
  shift for partially skinned buffers is untested), the 0x1F extended header words, 0x30 PVS block, 0x2B/0x2C hull vertex/face
  payloads and the 0x17-0x1A/0x2A shape payloads (only names decoded), header words a/b at +0x10/+0x14.
* Material: which of the three colours is diffuse / ambient / specular, texture slot type meaning (5, 6), the u32 tag at +0x164.
* Keyframed animation chunks (types 1-3) are not in any shipped model; the `.xga` format is a separate reader.
