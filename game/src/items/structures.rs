//! Structure layouts (`structuretype` / `structurematerial` of a `<TrackItem helpername="structure">`), ported from
//! `CEventDefinitionManager::GetStructureCount @000fb09c`, `LayoutStructure @000fb70c`, `SingleBlock @000fb214`,
//! `LargeBlock @000fb3bc`, `LongBlock @000fb564` (tables read from the decompile; weights read from libABK291.so).
//!
//! Each structure is a list of blocks. A block replaces the record's helper name (`smck_block_<material>_<size>`) and adds a local
//! offset to the record (`+0x5c` along the item's X axis = lateral, `+0x58` along its Y axis = up); `upright` also adds pi/2 to the
//! record's `+0x68` rotation. Type strings are matched with the case-sensitive `strcmp` of the original, so the typos that exist in the
//! shipped xml (`smalltower`, `smallsquare`, `arch`, `longbock`, ...) have count 0 and the original skips those items entirely.
use super::eventdef::ItemRecord;

#[derive(Clone, Copy)]
enum Size {
    B2x2,
    B2x6,
    B4x4,
}

/// (size, dx = lateral, dy = up, yaw added to record +0x68)
type Block = (Size, f32, f32, f32);

const HALF_PI: f32 = 1.5707964; // DAT_000fc59c = 0x3fc90fdb

fn blocks_of(name: &str) -> Option<&'static [Block]> {
    use Size::*;
    const TEST: [Block; 5] = [(B4x4, 0.0, 0.0, 0.0), (B2x6, 3.0, 4.0, 0.0), (B2x6, -3.0, 4.0, 0.0), (B2x2, 5.0, 6.0, 0.0), (B2x2, -5.0, 6.0, 0.0)];
    const LONGBLOCK: [Block; 1] = [(B2x6, 0.0, 0.0, 0.0)];
    const BIGBLOCK: [Block; 1] = [(B4x4, 0.0, 0.0, 0.0)];
    const SMALLBLOCK: [Block; 1] = [(B2x2, 0.0, 0.0, 0.0)];
    const UPRIGHT: [Block; 1] = [(B2x6, -1.0, 3.0, HALF_PI)];
    const SMALLSQUARE4: [Block; 4] = [(B2x2, 1.0, 0.0, 0.0), (B2x2, -1.0, 0.0, 0.0), (B2x2, 1.0, 2.0, 0.0), (B2x2, -1.0, 2.0, 0.0)];
    const SMALLTOWER1: [Block; 2] = [(B2x2, 0.0, 0.0, 0.0), (B2x2, 0.5, 2.0, 0.0)];
    const SMALLTOWER2: [Block; 3] = [(B2x2, 0.0, 0.0, 0.0), (B2x2, 0.5, 2.0, 0.0), (B2x2, 0.0, 4.0, 0.0)];
    const SMALLTRIANGLE: [Block; 3] = [(B2x2, -1.0, 0.0, 0.0), (B2x2, 1.0, 0.0, 0.0), (B2x2, 0.0, 2.0, 0.0)];
    const TOWER: [Block; 3] = [(B2x2, 0.0, 0.0, 0.0), (B2x2, 0.5, 2.0, 0.0), (B2x6, 0.5, 4.0, 0.0)];
    const LONGTOWER: [Block; 3] = [(B2x6, 0.0, 0.0, 0.0), (B2x6, 0.5, 2.0, 0.0), (B2x6, 0.0, 4.0, 0.0)];
    const TRIANGLE: [Block; 4] = [(B2x6, 0.0, 0.0, 0.0), (B2x2, -1.0, 2.0, 0.0), (B2x2, 1.0, 2.0, 0.0), (B2x2, 0.0, 4.0, 0.0)];
    const ARCH1: [Block; 5] = [(B2x2, 2.0, 0.0, 0.0), (B2x2, -2.0, 0.0, 0.0), (B2x2, 1.5, 2.0, 0.0), (B2x2, -1.5, 2.0, 0.0), (B2x6, 0.0, 4.0, 0.0)];
    const ARCH2: [Block; 8] = [
        (B2x2, -5.0, 0.0, 0.0),
        (B2x2, 0.0, 0.0, 0.0),
        (B2x2, 5.0, 0.0, 0.0),
        (B2x2, -4.5, 2.0, 0.0),
        (B2x2, 0.0, 2.0, 0.0),
        (B2x2, 4.5, 2.0, 0.0),
        (B2x6, -3.0, 4.0, 0.0),
        (B2x6, 3.0, 4.0, 0.0),
    ];
    Some(match name {
        "test" => &TEST,
        "longblock" => &LONGBLOCK,
        "bigblock" => &BIGBLOCK,
        "smallblock" => &SMALLBLOCK,
        "upright" => &UPRIGHT,
        "smallsquare4" => &SMALLSQUARE4,
        "smalltower1" => &SMALLTOWER1,
        "smalltower2" => &SMALLTOWER2,
        "smalltriangle" => &SMALLTRIANGLE,
        "tower" => &TOWER,
        "longtower" => &LONGTOWER,
        "triangle" => &TRIANGLE,
        "arch1" => &ARCH1,
        "arch2" => &ARCH2,
        _ => return None,
    })
}

/// `GetStructureCount`: number of records a structure type expands to (0 for unknown strings).
pub fn structure_item_count(name: &str) -> i32 {
    blocks_of(name).map_or(0, |b| b.len() as i32)
}

/// Random material weights `[glass, wood, stone, stone]` indexed by `(int)(difficulty level * 10)` (0..=10); read from
/// libABK291.so at 0xCEB1E0 (glass), 0xCEB210 (wood), 0xCEB240 and 0xCEB1B0 (both map to stone).
const W_GLASS: [i32; 11] = [100, 100, 50, 50, 30, 30, 45, 45, 35, 35, 10];
const W_WOOD: [i32; 11] = [0, 0, 50, 50, 70, 70, 45, 45, 35, 35, 40];
const W_STONE_A: [i32; 11] = [0, 0, 0, 0, 0, 0, 10, 10, 30, 30, 40];
const W_STONE_B: [i32; 11] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10];

/// Small deterministic generator. UNRESOLVED: the original draws from the game's `CXGSRandom` (virtual call, seed not traced), so which
/// material a *random* block gets cannot be reproduced; only the weights are 1:1. Only 2 shipped event items hit this path
/// (`structurematerial="triangle"`).
fn rand_in(seed: u32, total: i32) -> i32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    1 + (x % total.max(1) as u32) as i32
}

fn material_name(material: Option<&str>, level: f32, seed: u32) -> &'static str {
    match material {
        Some("glass") => return "glass",
        Some("wood") => return "wood",
        Some("stone") => return "stone",
        _ => {}
    }
    let idx = (level * 10.0) as i32;
    let (w0, w1, w2, w3) = if (0..=10).contains(&idx) {
        let i = idx as usize;
        (W_GLASS[i], W_WOOD[i], W_STONE_A[i], W_STONE_B[i])
    } else {
        (0, 0, 0, 0)
    };
    let total = w0 + w1 + w2 + w3;
    if total <= 0 {
        return "glass";
    }
    let r = rand_in(seed, total);
    if r <= w0 {
        "glass"
    } else if r <= w0 + w1 {
        "wood"
    } else {
        "stone"
    }
}

fn block_name(mat: &str, size: Size) -> String {
    let s = match size {
        Size::B2x2 => "2X2",
        Size::B2x6 => "2X6",
        Size::B4x4 => "4X4",
    };
    format!("smck_block_{mat}_{s}")
}

/// `LayoutStructure(type, firstRecord, material)`: rewrites the helper names / local offsets of the `count` records of one structure.
/// `level` is the event's `<Difficulty level>` (only used for random materials), `seed` distinguishes the random draws.
pub fn layout_structure(records: &mut [ItemRecord], name: &str, material: Option<&str>, level: f32, seed: u32) {
    let Some(blocks) = blocks_of(name) else { return };
    for (i, (r, b)) in records.iter_mut().zip(blocks.iter()).enumerate() {
        let mat = material_name(material, level, seed.wrapping_mul(31).wrapping_add(i as u32));
        r.helper = block_name(mat, b.0);
        r.local_y += b.2;
        r.local_x += b.1;
        r.pitch += b.3;
    }
}
