mod cli_anim;
mod cli_stm;
mod cli_tex;
mod cli_xgm;
use abgtool::{atlas, kpx, png, stm, xgm, xgt, xox};
use std::fs;
use std::path::{Path, PathBuf};

fn usage() -> ! {
    eprintln!(
        "abgtool - Angry Birds Go! v1.0.1 asset tool\n\
         \n\
         USAGE:\n\
         \x20 abgtool list   <file.pak>\n\
         \x20 abgtool unpack <file.pak> <out_dir>\n\
         \x20 abgtool xgt    <file.xgt> <out.png> [swizzle=rgba]   (swizzle applies to 16-bit textures)\n\
         \x20 abgtool xgt-info <file.xgt>\n\
         \x20 abgtool atlas <file.atlas> <file.xgt> <out.json>   (sprite rectangles as JSON)\n\
         \x20 abgtool xgm-info <file.xgm>               (chunks, meshes, names)\n\
         \x20 abgtool xgm-obj <file.xgm> <out.obj>      (export geometry as Wavefront OBJ)\n\
         \x20 abgtool xgm-preview <file.xgm> <out.png>  (wireframe: front / side / top)\n\
         \x20 abgtool xgm-scan <dir>                    (parse every .xgm under dir, report failures and vertex strides)\n\
         \x20 abgtool xox <file.xml>                    (print a tokenised XOX1 xml as plain xml)\n\
         \x20 abgtool xox-all <src_dir> <out_dir>      (decode every XOX1 .xml under src_dir, same relative path)\n\
         \x20 abgtool convert-all <src_dir> <out_dir>   (every .xgt under src_dir -> PNG, same relative path)\n\
         \x20 abgtool copy-ext <src_dir> <out_dir> <ext>  (copy every *.ext under src_dir, same relative path)"
    );
    std::process::exit(2);
}

fn files_with_extension(root: &Path, ext: &str) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for item in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = item.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Every texture under `root`, one file per texture. The 2.9.1 build ships each texture in several device variants
/// (`.xgt`, `.xgt_dxt`, `.xgt_etc`, `.xgt_pvr`, `.xgt_atc`); the first of dxt / etc that exists is used, PVRTC / ATC only
/// when nothing else is there.
fn texture_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut best: std::collections::BTreeMap<PathBuf, (usize, PathBuf)> = Default::default();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for item in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = item.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
            let rank = match ext.to_ascii_lowercase().as_str() {
                "xgt" => 0,
                "xgt_dxt" => 1,
                "xgt_etc" => 2,
                "xgt_pvr" => 3,
                "xgt_atc" => 4,
                _ => continue,
            };
            let stem = path.with_extension("");
            if best.get(&stem).map_or(true, |(r, _)| rank < *r) {
                best.insert(stem, (rank, path));
            }
        }
    }
    Ok(best.into_values().map(|(_, p)| p).collect())
}

fn relative_target(root: &Path, file: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let relative = file.strip_prefix(root).map_err(|e| e.to_string())?;
    let target = out_dir.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(target)
}

fn safe_join(root: &Path, name: &str) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for part in name.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("unsafe entry name: {name}"));
        }
        path.push(part);
    }
    Ok(path)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or_else(|| usage());
    if let Some(r) = cli_stm::dispatch(&args) {
        return r;
    }
    if let Some(r) = cli_anim::dispatch(&args) {
        return r;
    }
    if let Some(r) = cli_tex::dispatch(&args) {
        return r;
    }
    if let Some(r) = cli_xgm::dispatch(&args) {
        return r;
    }
    match (command, args.len()) {
        ("list", 2) => {
            let data = fs::read(&args[1]).map_err(|e| e.to_string())?;
            for e in kpx::parse(&data)? {
                println!("{:>9} {:>9} {} {}", e.size, e.offset, if e.compressed { "z" } else { "-" }, e.name);
            }
            Ok(())
        }
        ("unpack", 3) => {
            let data = fs::read(&args[1]).map_err(|e| e.to_string())?;
            let out_dir = Path::new(&args[2]);
            let entries = kpx::parse(&data)?;
            let mut total = 0usize;
            for entry in &entries {
                let bytes = kpx::read_entry(&data, entry)?;
                let target = safe_join(out_dir, &entry.name)?;
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                fs::write(&target, &bytes).map_err(|e| format!("{}: {e}", target.display()))?;
                total += bytes.len();
            }
            println!("{}: {} entries, {} bytes", args[1], entries.len(), total);
            Ok(())
        }
        ("xgt-info", 2) => {
            let data = fs::read(&args[1]).map_err(|e| e.to_string())?;
            let x = xgt::parse(&data)?;
            println!(
                "{}x{} (used {}x{}) mips={} format=0x{:02X} payload={}",
                x.width, x.height, x.used_width, x.used_height, x.mips, x.format, x.payload.len()
            );
            Ok(())
        }
        ("xgm-info", 2) => {
            let data = fs::read(&args[1]).map_err(|e| format!("{}: {e}", args[1]))?;
            let model = xgm::parse(&data)?;
            for c in &model.chunks {
                println!("chunk id 0x{:02X} at 0x{:X} size {}", c.id, c.offset, c.size);
            }
            for (i, m) in model.meshes.iter().enumerate() {
                println!(
                    "mesh {i}: {} vertices (stride {}), {} indices, bbox {:?} .. {:?}, valid: {:?}",
                    m.positions.len(), m.stride, m.indices.len(), m.bbox_min, m.bbox_max, m.validate()
                );
            }
            for n in &model.nodes {
                println!("node {:<22} pos {:?} rot {:?} scale {:?}", n.name, n.position, n.rotation, n.scale);
            }
            Ok(())
        }
        ("xgm-obj", 3) => {
            let data = fs::read(&args[1]).map_err(|e| format!("{}: {e}", args[1]))?;
            fs::write(&args[2], xgm::to_obj(&xgm::parse(&data)?)).map_err(|e| e.to_string())?;
            println!("wrote {}", args[2]);
            Ok(())
        }
        ("xgm-preview", 3) => {
            let data = fs::read(&args[1]).map_err(|e| format!("{}: {e}", args[1]))?;
            let (w, h, rgba) = xgm::preview(&xgm::parse(&data)?, 420);
            fs::write(&args[2], png::encode_rgba(w as u32, h as u32, &rgba)).map_err(|e| e.to_string())?;
            println!("wrote {}", args[2]);
            Ok(())
        }
        ("xgm-scan", 2) => {
            let mut strides: std::collections::BTreeMap<usize, usize> = Default::default();
            let mut chunk_ids: std::collections::BTreeMap<u32, usize> = Default::default();
            let (mut ok, mut failed, mut no_mesh, mut invalid) = (0usize, 0usize, 0usize, 0usize);
            for file in files_with_extension(Path::new(&args[1]), "xgm")? {
                let data = fs::read(&file).map_err(|e| e.to_string())?;
                match xgm::parse(&data) {
                    Ok(model) => {
                        ok += 1;
                        for c in &model.chunks {
                            *chunk_ids.entry(c.id).or_default() += 1;
                        }
                        if model.meshes.is_empty() {
                            no_mesh += 1;
                        }
                        for m in &model.meshes {
                            *strides.entry(m.stride).or_default() += 1;
                            if let Err(e) = m.validate() {
                                invalid += 1;
                                if invalid <= 5 {
                                    eprintln!("INVALID {}: {e}", file.display());
                                }
                            }
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        if failed <= 8 {
                            eprintln!("FAILED {}: {e}", file.display());
                        }
                    }
                }
            }
            println!("parsed {ok}, failed {failed}, without a mesh chunk {no_mesh}, meshes failing validation {invalid}");
            println!("vertex strides (stride: meshes): {strides:?}");
            println!("chunk ids (id: count): {}", chunk_ids.iter().map(|(k, v)| format!("0x{k:02X}:{v}")).collect::<Vec<_>>().join(" "));
            Ok(())
        }
        ("xox", 2) => {
            let data = fs::read(&args[1]).map_err(|e| format!("{}: {e}", args[1]))?;
            println!("{}", xox::decode(&data)?);
            Ok(())
        }
        ("loc", 3) => {
            // XGSL string database: dump `KEY=value` for the block starting at the given byte offset
            let data = fs::read(&args[1]).map_err(|e| format!("{}: {e}", args[1]))?;
            let start: usize = args[2].parse().map_err(|_| "offset must be a number")?;
            let count = u32::from_le_bytes([data[0x10], data[0x11], data[0x12], data[0x13]]) as usize;
            let mut at = 0x78usize;
            let mut keys = Vec::new();
            for _ in 0..count {
                let end = at + data[at..].iter().position(|&b| b == 0).ok_or("unterminated key")?;
                keys.push(String::from_utf8_lossy(&data[at..end]).into_owned());
                at = end + 1;
            }
            eprintln!("{} keys, key table ends at 0x{at:X}", keys.len());
            let mut at = if start == 0 { at } else { start };
            for key in &keys {
                let end = at + data[at..].iter().position(|&b| b == 0).unwrap_or(data.len() - at);
                println!("{key}={}", String::from_utf8_lossy(&data[at..end]).replace('\n', "\n"));
                at = end + 1;
                if at >= data.len() {
                    break;
                }
            }
            Ok(())
        }
        ("xox-all", 3) => {
            let (root, out_dir) = (Path::new(&args[1]), Path::new(&args[2]));
            let (mut decoded, mut failed, mut skipped) = (0usize, 0usize, 0usize);
            for file in files_with_extension(root, "xml")? {
                let data = fs::read(&file).map_err(|e| e.to_string())?;
                if !xox::is_xox(&data) {
                    skipped += 1;
                    continue;
                }
                match xox::decode(&data) {
                    Ok(text) => {
                        let target = relative_target(root, &file, out_dir)?;
                        fs::write(&target, text).map_err(|e| e.to_string())?;
                        decoded += 1;
                    }
                    Err(message) => {
                        eprintln!("FAILED {}: {message}", file.display());
                        failed += 1;
                    }
                }
            }
            println!("decoded {decoded} xml files, {failed} failed, {skipped} were already plain text");
            Ok(())
        }
        ("atlas", 4) => {
            let atlas_bytes = fs::read(&args[1]).map_err(|e| format!("{}: {e}", args[1]))?;
            let texture_bytes = fs::read(&args[2]).map_err(|e| format!("{}: {e}", args[2]))?;
            let texture = xgt::parse(&texture_bytes)?;
            let parsed = atlas::parse(&atlas_bytes, texture.width as u32, texture.height as u32)?;
            fs::write(&args[3], atlas::to_json(&parsed)).map_err(|e| e.to_string())?;
            for s in &parsed.sprites {
                println!("{:>5} {:>5} {:>5}x{:<5} {}", s.x, s.y, s.w, s.h, s.name);
            }
            Ok(())
        }
        ("convert-all", 3) => {
            let (root, out_dir) = (Path::new(&args[1]), Path::new(&args[2]));
            let (mut converted, mut failed, mut bytes) = (0usize, 0usize, 0u64);
            for file in texture_files(root)? {
                let result = fs::read(&file).map_err(|e| e.to_string()).and_then(|data| {
                    let x = xgt::parse(&data)?;
                    let rgba = xgt::decode_rgba(&x, xgt::Swizzle::parse("rgba")?)?;
                    Ok(png::encode_rgba(x.width as u32, x.height as u32, &rgba))
                });
                match result {
                    Ok(png) => {
                        let target = relative_target(root, &file, out_dir)?.with_extension("png");
                        bytes += png.len() as u64;
                        fs::write(&target, png).map_err(|e| e.to_string())?;
                        converted += 1;
                    }
                    Err(message) => {
                        eprintln!("FAILED {}: {message}", file.display());
                        failed += 1;
                    }
                }
            }
            println!("converted {converted} textures ({:.1} MB of PNG), {failed} failed", bytes as f64 / 1048576.0);
            Ok(())
        }
        ("copy-ext", 4) => {
            let (root, out_dir) = (Path::new(&args[1]), Path::new(&args[2]));
            let files = files_with_extension(root, &args[3])?;
            let mut bytes = 0u64;
            for file in &files {
                bytes += fs::copy(file, relative_target(root, file, out_dir)?).map_err(|e| e.to_string())?;
            }
            println!("copied {} .{} files ({:.1} MB)", files.len(), args[3], bytes as f64 / 1048576.0);
            Ok(())
        }
        ("xgt", 3) | ("xgt", 4) => {
            let data = fs::read(&args[1]).map_err(|e| e.to_string())?;
            let swizzle = xgt::Swizzle::parse(args.get(3).map(String::as_str).unwrap_or("rgba"))?;
            let x = xgt::parse(&data)?;
            let rgba = xgt::decode_rgba(&x, swizzle)?;
            let png = png::encode_rgba(x.width as u32, x.height as u32, &rgba);
            fs::write(&args[2], png).map_err(|e| e.to_string())?;
            println!("{} -> {} ({}x{})", args[1], args[2], x.width, x.height);
            Ok(())
        }
        _ => usage(),
    }
}

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}
