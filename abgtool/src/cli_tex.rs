use abgtool::xgt;
use std::fs;
use std::path::{Path, PathBuf};

pub fn walk(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for item in rd.flatten() {
            let path = item.path();
            if path.is_dir() { pending.push(path) } else { found.push(path) }
        }
    }
    found.sort();
    found
}

pub fn dispatch(args: &[String]) -> Option<Result<(), String>> {
    let cmd = args.first()?.as_str();
    match cmd {
        "alpha-probe" => { let t = alphabet(0x950334ab, 0x1733a2f3, 1024); println!("{:02X?}", t); let t2 = alphabet(0x1733a2f3, 0x950334ab, 1024); println!("{:02X?}", t2); Some(Ok(())) }
        "xox-probe" if args.len() == 2 => Some(xox_probe(Path::new(&args[1]))),
        "xox-dump" if args.len() == 3 => Some(xox_dump(Path::new(&args[1]), args[2].parse().unwrap())),
        "xox-direct" if args.len() == 3 => Some(xox_direct(Path::new(&args[1]), args[2].parse().unwrap())),
        "xox-seq" if args.len() == 2 => Some(xox_seq(Path::new(&args[1]))),
        "xox-m" if args.len() == 2 => Some(xox_m(Path::new(&args[1]))),
        "xox-all" if args.len() == 3 => Some(xml_all(Path::new(&args[1]), Path::new(&args[2]))),
        "xxtea-probe" if args.len() == 3 => Some(xxtea_probe(Path::new(&args[1]), &(0..16).map(|i| u8::from_str_radix(&args[2][2*i..2*i+2], 16).unwrap()).collect::<Vec<u8>>())),
        "deviceconfig-all" if args.len() == 3 => Some(deviceconfig_all(Path::new(&args[1]), Path::new(&args[2]))),
        "convert-all" if args.len() == 3 => Some(convert_all(Path::new(&args[1]), Path::new(&args[2]))),
        "tex-compare" if args.len() == 2 => Some(tex_compare(Path::new(&args[1]))),
        "xmat-floats" if args.len() == 2 => Some(xmat_floats(Path::new(&args[1]))),
        "xmat-ptrscan" if args.len() == 4 => Some(xmat_ptrscan(Path::new(&args[1]), usize::from_str_radix(&args[2], 16).unwrap(), usize::from_str_radix(&args[3], 16).unwrap())),
        "xmat-probe" if args.len() == 2 => Some(xmat_probe(Path::new(&args[1]))),
        "xmat-probe2" if args.len() == 2 => Some(xmat_probe2(Path::new(&args[1]))),
        "xmat-probe3" if args.len() == 3 => Some(xmat_probe3(Path::new(&args[1]), args[2].parse().unwrap())),
        "xmat-info" if args.len() >= 2 => Some(xmat_info(Path::new(&args[1]), args.get(2).is_some_and(|a| a == "-v"))),
        "tex-scan" if args.len() == 2 => Some(tex_scan(Path::new(&args[1]))),
        "magic-scan" if args.len() == 2 => Some(magic_scan(Path::new(&args[1]))),
        _ => None,
    }
}

fn tex_scan(root: &Path) -> Result<(), String> {
    let mut stats: std::collections::BTreeMap<String, (usize, String)> = Default::default();
    for f in walk(root) {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
        if !ext.starts_with("xgt") { continue; }
        let data = fs::read(&f).map_err(|e| e.to_string())?;
        let Ok(x) = xgt::parse(&data) else { println!("BAD {}", f.display()); continue };
        let key = format!("{ext} ver=0x{:08X} fmt=0x{:02X}", x.version, x.format);
        let e = stats.entry(key).or_insert((0, f.display().to_string()));
        e.0 += 1;
    }
    for (k, (n, ex)) in stats { println!("{n:5} {k}  e.g. {ex}"); }
    Ok(())
}

pub fn magic_scan(root: &Path) -> Result<(), String> {
    let mut stats: std::collections::BTreeMap<String, (usize, String)> = Default::default();
    for f in walk(root) {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let data = fs::read(&f).map_err(|e| e.to_string())?;
        let m: String = data.iter().take(4).map(|&b| if b.is_ascii_graphic() { b as char } else { '.' }).collect();
        let e = stats.entry(format!("{ext:8} {m}")).or_insert((0, f.display().to_string()));
        e.0 += 1;
    }
    for (k, (n, ex)) in stats { println!("{n:6} {k}  e.g. {ex}"); }
    Ok(())
}

pub fn alphabet(mut z: u32, mut w: u32, n: u32) -> [u8; 115] {
    let mut table: Vec<u8> = vec![0x3A];
    table.extend(0x41..=0x5Au8);
    table.extend(0x61..=0x7Au8);
    table.extend(0xC0..=0xD6u8);
    table.extend(0xD8..=0xF6u8);
    table.extend(0xF8..=0xFFu8);
    for _ in 0..n {
        let z1 = (z & 0xFFFF).wrapping_mul(0x9069).wrapping_add(z >> 16);
        let w1 = (w & 0xFFFF).wrapping_mul(18000).wrapping_add(w >> 16);
        z = (z1 & 0xFFFF).wrapping_mul(0x9069).wrapping_add(z1 >> 16);
        w = (w1 & 0xFFFF).wrapping_mul(18000).wrapping_add(w1 >> 16);
        let a = (w1.wrapping_add(z1.wrapping_shl(16)) % 115) as usize;
        let b = (w.wrapping_add(z.wrapping_shl(16)) % 115) as usize;
        table.swap(a, b);
    }
    let mut out = [0u8; 115];
    out.copy_from_slice(&table);
    out
}

fn u32le(d: &[u8], at: usize) -> usize { u32::from_le_bytes([d[at], d[at+1], d[at+2], d[at+3]]) as usize }

pub fn xox_probe(root: &Path) -> Result<(), String> {
    // alphabet candidates: bytes >= 0xC0, letters, ':' (anything not structural)
    let is_tok = |b: u8| b == b':' || b.is_ascii_alphabetic() || b >= 0xC0;
    let mut n_ok = 0; let mut n_bad = 0;
    for f in walk(root) {
        if f.extension().and_then(|e| e.to_str()) != Some("xml") { continue; }
        let d = fs::read(&f).map_err(|e| e.to_string())?;
        if d.len() < 12 || &d[0..4] != b"XOX2" { continue; }
        let n = u32le(&d, 4);
        let doc_at = 8 + 4 * (n + 1) + u32le(&d, 8 + 4 * n);
        let doc = &d[doc_at..];
        // tags/attr tokens: runs of token bytes
        let mut order: Vec<Vec<u8>> = Vec::new();
        let mut run: Vec<u8> = Vec::new();
        for &b in doc.iter().chain(std::iter::once(&b'<')) {
            if is_tok(b) { run.push(b); } else { if !run.is_empty() { if !order.contains(&run) { order.push(run.clone()); } run.clear(); } }
        }
        if n_ok + n_bad < 6 || (order.len() != n + 1 && n_bad < 4) {
            let singles: String = order.iter().take(20).map(|r| if r.len()==1 { format!("{:02X} ", r[0]) } else { format!("[{}] ", r.len()) }).collect();
            println!("{} N={} distinct tokens={} first: {}", f.display(), n, order.len(), singles);
        }
        if order.len() == n + 1 { n_ok += 1 } else { n_bad += 1 }
    }
    println!("distinct==N+1: {n_ok}, otherwise {n_bad}");
    Ok(())
}

pub fn xox_dump(file: &Path, limit: usize) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| e.to_string())?;
    let n = u32le(&d, 4);
    let tab = 8 + 4 * (n + 1);
    for i in 0..n {
        let s = tab + u32le(&d, 8 + 4 * i); let e = tab + u32le(&d, 8 + 4 * (i + 1));
        println!("S{i}: {:?}", String::from_utf8_lossy(&d[s..e - 1]));
    }
    let doc = &d[tab + u32le(&d, 8 + 4 * n)..];
    let mut out = String::new();
    for &b in doc.iter().take(limit) {
        if b == b'<' || b == b'>' || b == b'/' || b == b'=' || b == b'"' || b == b' ' || b == b'\n' || b == b'\t' || b == b'!' || b == b'-' || b == b'?' { out.push(b as char) } else { out.push_str(&format!("{{{b:02X}}}")); }
    }
    println!("{out}");
    Ok(())
}

const T2: [u8; 115] = [
    0x78, 0x65, 0xD3, 0xE2, 0x6E, 0xF3, 0xDA, 0xC5, 0x76, 0xD2, 0x59, 0xC3, 0x52, 0x48, 0x6B, 0x58, 0xFC, 0xE8, 0xF2, 0x72, 0x62, 0x63, 0x61, 0xD1, 0xED, 0xFB, 0x4B, 0xEA, 0x44, 0x6A, 0x42, 0x49, 0x43, 0x4E, 0xE0, 0xEF, 0x3A, 0x41, 0x6C, 0x46, 0xE1, 0x4C, 0xC2, 0xCC, 0x64, 0xF8, 0x66, 0xCE, 0xF9, 0x5A, 0x56, 0x77, 0xD8, 0x4F, 0xF0, 0xDE, 0xCA, 0x57, 0xDD, 0xDF, 0x53, 0xFA, 0x4A, 0xE9, 0xDC, 0x51, 0xE7, 0x73, 0x68, 0x7A, 0x71, 0xD4, 0x74, 0xFD, 0xEE, 0xC8, 0xF1, 0xF4, 0xF6, 0xC9, 0xC4, 0xC6, 0xD6, 0xD9, 0xCF, 0xE3, 0x70, 0x75, 0xDB, 0xFE, 0xF5, 0xC7, 0xEB, 0xD5, 0x79, 0xCB, 0x47, 0xCD, 0xEC, 0x50, 0xC1, 0x6F, 0x69, 0xE6, 0x54, 0x45, 0xFF, 0xE5, 0x6D, 0x55, 0xC0, 0x67, 0x4D, 0xE4, 0xD0,
];

pub fn xox_direct(file: &Path, shift: usize) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| e.to_string())?;
    let n = u32le(&d, 4);
    let tab = 8 + 4 * (n + 1);
    let strs: Vec<String> = (0..n).map(|i| { let s = tab + u32le(&d, 8 + 4 * i); let e = tab + u32le(&d, 8 + 4 * (i + 1)); String::from_utf8_lossy(&d[s..e - 1]).into_owned() }).collect();
    let doc = &d[tab + u32le(&d, 8 + 4 * n)..];
    let mut out = String::new();
    let mut run: Option<usize> = None;
    let flush = |run: &mut Option<usize>, out: &mut String| {
        if let Some(v) = run.take() {
            if v < shift { out.push_str("{ROOT}") } else { out.push_str(strs.get(v - shift).map(|s| s.as_str()).unwrap_or("{OOR}")) }
        }
    };
    for &b in doc {
        if b == 0 { break; }
        if let Some(p) = T2.iter().position(|&t| t == b) { run = Some(run.map_or(p, |v| v * 115 + p)); }
        else { flush(&mut run, &mut out); out.push(b as char); }
    }
    flush(&mut run, &mut out);
    println!("{out}");
    Ok(())
}

pub fn xox_seq(file: &Path) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| e.to_string())?;
    let n = u32le(&d, 4);
    let tab = 8 + 4 * (n + 1);
    let doc = &d[tab + u32le(&d, 8 + 4 * n)..];
    let mut run: Option<usize> = None;
    let mut seen: std::collections::HashSet<usize> = Default::default();
    let mut max_seen = 0usize;
    let mut line = String::new();
    let mut count = 0;
    let mut flush = |run: &mut Option<usize>, line: &mut String, count: &mut usize| {
        if let Some(v) = run.take() {
            if seen.insert(v) { line.push_str(&format!("NEW({v}{}) ", if v == max_seen + 1 { "" } else { "!" })); max_seen = max_seen.max(v); } else if v == 0 { line.push_str("X "); } 
            *count += 1;
        }
    };
    for &b in doc {
        if b == 0 { break; }
        if let Some(p) = T2.iter().position(|&t| t == b) { run = Some(run.map_or(p, |v| v * 115 + p)); }
        else { flush(&mut run, &mut line, &mut count); }
    }
    flush(&mut run, &mut line, &mut count);
    println!("{line}");
    Ok(())
}

pub fn xox_m(file: &Path) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| e.to_string())?;
    let n = u32le(&d, 4);
    let tab = 8 + 4 * (n + 1);
    let strs: Vec<String> = (0..n).map(|i| { let s = tab + u32le(&d, 8 + 4 * i); let e = tab + u32le(&d, 8 + 4 * (i + 1)); String::from_utf8_lossy(&d[s..e - 1]).into_owned() }).collect();
    let doc = &d[tab + u32le(&d, 8 + 4 * n)..];
    let mut out = String::new();
    let mut run: Vec<usize> = Vec::new();
    let flush = |run: &mut Vec<usize>, out: &mut String| {
        if run.is_empty() { return; }
        if run.len() == 1 && run[0] == 0 { out.push_str("{X}"); run.clear(); return; }
        let mut v = 0usize;
        for &p in run.iter() { v = v * 114 + (p.wrapping_sub(1)); }
        out.push_str(strs.get(v).map(|s| s.as_str()).unwrap_or("{OOR}"));
        run.clear();
    };
    for &b in doc {
        if b == 0 { break; }
        if let Some(p) = T2.iter().position(|&t| t == b) { run.push(p); }
        else { flush(&mut run, &mut out); out.push(b as char); }
    }
    flush(&mut run, &mut out);
    println!("{out}");
    Ok(())
}

/// `xox-all <src_dir> <out_dir>`: every `.xml` under src_dir -> plain xml at the same relative path (XOX1 / XOX2 decoded,
/// plain text copied). Prints the counts.
fn xml_all(root: &Path, out_dir: &Path) -> Result<(), String> {
    use abgtool::xox;
    let (mut x1, mut x2, mut plain, mut failed, mut other, mut enc) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    for file in walk(root) {
        if !file.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("xml")) { continue; }
        let data = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let rel = file.strip_prefix(root).map_err(|e| e.to_string())?;
        let target = out_dir.join(rel);
        if let Some(p) = target.parent() { fs::create_dir_all(p).map_err(|e| e.to_string())?; }
        if xox::is_xox(&data) {
            match xox::decode(&data) {
                Ok(text) => {
                    if &data[0..4] == b"XOX1" { x1 += 1 } else { x2 += 1 }
                    fs::write(&target, text).map_err(|e| e.to_string())?;
                }
                Err(m) => { eprintln!("FAILED {}: {m}", file.display()); failed += 1; }
            }
        } else if std::str::from_utf8(&data).is_ok_and(|s| s.trim_start_matches('\u{feff}').trim_start().starts_with('<')) {
            plain += 1;
            fs::write(&target, &data).map_err(|e| e.to_string())?;
        } else {
            // eligo.xml and the device configs are XXTEA encrypted with the key stored at 0xCDE9F0 of libABK291
            let mut d = data.clone();
            xxtea_decrypt(&mut d, DEVICE_CONFIG_KEY);
            let text = if xox::is_xox(&d) { xox::decode(&d).ok() } else { String::from_utf8(d).ok().filter(|s| s.trim_start().starts_with('<')) };
            match text {
                Some(t) => { enc += 1; fs::write(&target, t).map_err(|e| e.to_string())?; }
                None => { eprintln!("UNKNOWN (not XOX, not text xml, not XXTEA xml) {}", file.display()); other += 1; }
            }
        }
    }
    println!("XOX1 {x1}, XOX2 {x2}, plain text xml {plain}, XXTEA-encrypted xml {enc}, failed {failed}, unrecognised {other}");
    if failed + other > 0 { Err(format!("{} files not decoded", failed + other)) } else { Ok(()) }
}

/// XXTEA decrypt in place (`XGSEncrypt_decryptXXTEA`; standard algorithm, little endian words, trailing len%4 bytes untouched).
pub fn xxtea_decrypt(data: &mut [u8], key: [u32; 4]) {
    let n = data.len() / 4;
    if n < 2 { return; }
    let mut v: Vec<u32> = (0..n).map(|i| u32::from_le_bytes([data[4*i], data[4*i+1], data[4*i+2], data[4*i+3]])).collect();
    let rounds = 6 + 52 / n as u32;
    let mut sum = rounds.wrapping_mul(0x9E3779B9);
    let mut y = v[0];
    while sum != 0 {
        let e = ((sum >> 2) & 3) as usize;
        for p in (1..n).rev() {
            let z = v[p - 1];
            let mx = ((z >> 5 ^ y << 2).wrapping_add(y >> 3 ^ z << 4)) ^ ((sum ^ y).wrapping_add(key[(p & 3) ^ e] ^ z));
            v[p] = v[p].wrapping_sub(mx);
            y = v[p];
        }
        let z = v[n - 1];
        let mx = ((z >> 5 ^ y << 2).wrapping_add(y >> 3 ^ z << 4)) ^ ((sum ^ y).wrapping_add(key[e] ^ z));
        v[0] = v[0].wrapping_sub(mx);
        y = v[0];
        sum = sum.wrapping_sub(0x9E3779B9);
    }
    for (i, w) in v.iter().enumerate() { data[4*i..4*i+4].copy_from_slice(&w.to_le_bytes()); }
}

pub fn xxtea_probe(file: &Path, key: &[u8]) -> Result<(), String> {
    let mut d = fs::read(file).map_err(|e| e.to_string())?;
    let k = [0, 1, 2, 3].map(|i| u32::from_le_bytes([key[4*i], key[4*i+1], key[4*i+2], key[4*i+3]]));
    xxtea_decrypt(&mut d, k);
    println!("{}", String::from_utf8_lossy(&d[..d.len().min(600)]));
    Ok(())
}

/// Key of `XGSEncrypt_decryptXXTEA` as used for `deviceconfigs/*.json` and `analytics/eligo.xml`
/// (16 bytes at Ghidra 0xCDE9F0 of libABK291: 94 44 16 44 18 01 4e 30 85 1a 2e 5a 3b 7b 3d 8d).
pub const DEVICE_CONFIG_KEY: [u32; 4] = [0x4416_4494, 0x304E_0118, 0x5A2E_1A85, 0x8D3D_7B3B];

/// `deviceconfig-all <src_dir> <out_dir>`: XXTEA-decrypt every `*.json` under src_dir.
fn deviceconfig_all(root: &Path, out_dir: &Path) -> Result<(), String> {
    let (mut ok, mut bad) = (0, 0);
    for file in walk(root) {
        if file.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        let mut d = fs::read(&file).map_err(|e| e.to_string())?;
        xxtea_decrypt(&mut d, DEVICE_CONFIG_KEY);
        let rel = file.strip_prefix(root).map_err(|e| e.to_string())?;
        let target = out_dir.join(rel);
        if let Some(p) = target.parent() { fs::create_dir_all(p).map_err(|e| e.to_string())?; }
        // length is padded to 4: drop trailing NULs
        while d.last() == Some(&0) { d.pop(); }
        match String::from_utf8(d) {
            Ok(t) if t.trim_start().starts_with('{') => { fs::write(&target, t).map_err(|e| e.to_string())?; ok += 1 }
            _ => { eprintln!("FAILED {}", file.display()); bad += 1 }
        }
    }
    println!("device configs decrypted {ok}, failed {bad}");
    if bad > 0 { Err("failures".into()) } else { Ok(()) }
}

/// One file per texture: the best variant (see `xgt::variant_rank`), keyed by path without extension.
fn texture_files(root: &Path) -> Vec<PathBuf> {
    let mut best: std::collections::BTreeMap<PathBuf, (usize, PathBuf)> = Default::default();
    for path in walk(root) {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(rank) = xgt::variant_rank(name) else { continue };
        let stem = path.with_extension("");
        if best.get(&stem).map_or(true, |(r, _)| rank < *r) {
            best.insert(stem, (rank, path));
        }
    }
    best.into_values().map(|(_, p)| p).collect()
}

/// Decode with the preferred variant; if that variant's format is not decodable fall back to the next ones.
fn decode_texture(path: &Path) -> Result<(u32, u32, Vec<u8>, String), String> {
    let first = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    match xgt::decode_bytes(&first) {
        Ok((w, h, px)) => Ok((w, h, px, path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string())),
        Err(first_error) => {
            for ext in xgt::VARIANT_EXTS {
                let alt = path.with_extension(ext);
                if alt != path {
                    if let Ok(d) = fs::read(&alt) {
                        if let Ok((w, h, px)) = xgt::decode_bytes(&d) { return Ok((w, h, px, ext.to_string())); }
                    }
                }
            }
            Err(first_error)
        }
    }
}

/// `convert-all <src_dir> <out_dir>`
fn convert_all(root: &Path, out_dir: &Path) -> Result<(), String> {
    use abgtool::png;
    let (mut converted, mut failed, mut bytes) = (0usize, 0usize, 0u64);
    let mut by_ext: std::collections::BTreeMap<String, usize> = Default::default();
    let mut by_fmt: std::collections::BTreeMap<String, usize> = Default::default();
    for file in texture_files(root) {
        match decode_texture(&file) {
            Ok((w, h, rgba, ext)) => {
                let rel = file.strip_prefix(root).map_err(|e| e.to_string())?.with_extension("png");
                let target = out_dir.join(rel);
                if let Some(p) = target.parent() { fs::create_dir_all(p).map_err(|e| e.to_string())?; }
                let png = png::encode_rgba(w, h, &rgba);
                bytes += png.len() as u64;
                fs::write(&target, png).map_err(|e| e.to_string())?;
                converted += 1;
                *by_ext.entry(ext).or_default() += 1;
                if let Ok(d) = fs::read(&file) { if let Ok(x) = xgt::parse(&d) { *by_fmt.entry(format!("v0x{:X} fmt 0x{:02X} {}", x.version, x.format, if x.version == xgt::VERSION_2_9 { xgt::base_format_name(x.format) } else { "legacy" })).or_default() += 1; } }
            }
            Err(m) => { eprintln!("FAILED {}: {m}", file.display()); failed += 1; }
        }
    }
    println!("converted {converted} textures ({:.1} MB of PNG), {failed} failed", bytes as f64 / 1048576.0);
    println!("source variants used: {by_ext:?}");
    println!("pixel formats of the chosen files: {by_fmt:?}");
    if failed > 0 { Err(format!("{failed} textures failed")) } else { Ok(()) }
}

/// `tex-compare <dir>`: for every texture with several variants decode each decodable one and print the mean absolute
/// difference of the colour channels against the DXT (or first) variant - a sanity check of the decoders.
fn tex_compare(root: &Path) -> Result<(), String> {
    let mut rows = 0;
    let mut worst: Vec<(f64, String)> = Vec::new();
    let mut sum_by_pair: std::collections::BTreeMap<String, (f64, usize)> = Default::default();
    let mut stems: std::collections::BTreeSet<PathBuf> = Default::default();
    for p in walk(root) { if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| xgt::variant_rank(n).is_some()) { stems.insert(p.with_extension("")); } }
    for stem in stems {
        let mut dec: Vec<(String, u32, u32, Vec<u8>)> = Vec::new();
        for ext in xgt::VARIANT_EXTS {
            if let Ok(d) = fs::read(stem.with_extension(ext)) { if let Ok((w, h, px)) = xgt::decode_bytes(&d) { dec.push((ext.to_string(), w, h, px)); } }
        }
        if dec.len() < 2 { continue; }
        rows += 1;
        let (name0, w0, h0, base) = (&dec[0].0, dec[0].1, dec[0].2, &dec[0].3);
        for (name, w, h, px) in &dec[1..] {
            if (*w, *h) != (w0, h0) { println!("SIZE MISMATCH {} {name0} {w0}x{h0} vs {name} {w}x{h}", stem.display()); continue; }
            let (mut s, mut sa) = (0u64, 0u64);
            for (a, b) in base.chunks_exact(4).zip(px.chunks_exact(4)) {
                for k in 0..3 { s += (a[k] as i32 - b[k] as i32).unsigned_abs() as u64; }
                sa += (a[3] as i32 - b[3] as i32).unsigned_abs() as u64;
            }
            let n = (base.len() / 4) as f64;
            let mean = s as f64 / (3.0 * n);
            let key = format!("{name0} vs {name}");
            let e = sum_by_pair.entry(key).or_default(); e.0 += mean; e.1 += 1;
            worst.push((mean, format!("{} {name0}/{name} rgb {:.2} alpha {:.2}", stem.display(), mean, sa as f64 / n)));
        }
    }
    worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (_, line) in worst.iter().take(8) { println!("{line}"); }
    for (k, (s, n)) in sum_by_pair { println!("{k}: mean abs rgb diff {:.3} over {n} textures", s / n as f64); }
    println!("{rows} multi-variant textures compared");
    Ok(())
}

pub fn xmat_floats(file: &Path) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| e.to_string())?;
    let mut counts: std::collections::BTreeMap<u32, (usize, usize)> = Default::default();
    for i in 0..d.len().saturating_sub(16) {
        let w = |k: usize| u32::from_le_bytes([d[i+4*k], d[i+4*k+1], d[i+4*k+2], d[i+4*k+3]]);
        let (a, b, c, e) = (w(0), w(1), w(2), w(3));
        if a == b && a != 0 && c == 0 && e == 0 {
            let f = f32::from_bits(a);
            if f.is_finite() && f.abs() < 1.0 && f.abs() > 1e-5 { let en = counts.entry(a).or_insert((0, i)); en.0 += 1; }
        }
    }
    for (k, (n, at)) in counts { println!("{:e} (0x{k:08X}) x{n} first at 0x{at:X}", f32::from_bits(k)); }
    Ok(())
}

pub fn xmat_ptrscan(file: &Path, target: usize, lo: usize) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| e.to_string())?;
    for p in lo..target {
        let v = u32::from_le_bytes([d[p], d[p+1], d[p+2], d[p+3]]) as usize;
        if p + v == target { println!("ptr at 0x{p:X} = 0x{v:X} -> 0x{target:X}"); }
    }
    Ok(())
}

pub fn xmat_probe(file: &Path) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| e.to_string())?;
    let mut p = 0x10 + 19 * 64;
    for i in 0..19 {
        let a = u32le(&d, p); let b = u32le(&d, p + 4);
        println!("scene {i} at 0x{p:X} A=0x{a:X} B=0x{b:X}");
        p += 8 + a + b;
    }
    println!("end of scenes 0x{p:X}");
    for r in 0..6 { println!("{:08X}: {:08X}", p + 4 * r, u32le(&d, p + 4 * r)); }
    Ok(())
}

pub fn xmat_probe2(file: &Path) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| e.to_string())?;
    let mut p = 0x10 + 19 * 64;
    for _ in 0..19 { p += 8 + u32le(&d, p) + u32le(&d, p + 4); }
    let m = u32le(&d, p); p += 4;
    println!("M={m}");
    let mut masks: std::collections::BTreeMap<usize, usize> = Default::default();
    for _ in 0..m {
        let size = u32le(&d, p); let mask = u32le(&d, p + 4);
        *masks.entry(mask).or_default() += 1;
        p += 8 + size;
    }
    println!("end 0x{p:X} of {} ; masks {masks:x?}", d.len());
    for r in 0..24 { println!("{:08X}: {:08X}", p + 4 * r, u32le(&d, p + 4 * r)); }
    Ok(())
}

pub fn xmat_probe3(file: &Path, extra: usize) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| e.to_string())?;
    let mut p = 0x3C2643;
    for i in 0..0x110usize {
        if p + 8 > d.len() { println!("ran off at item {i}"); break; }
        let size = u32le(&d, p); let mask = u32le(&d, p + 4);
        let name_off = u32le(&d, p + 8);
        let name = if name_off < size { let s = p + 8 + name_off; let e = s + d[s..].iter().position(|&b| b == 0).unwrap_or(0); String::from_utf8_lossy(&d[s..e]).into_owned() } else { "?".into() };
        if i < 8 || i % 40 == 0 { println!("{i} at 0x{p:X} size 0x{size:X} mask 0x{mask:X} name {name:?}"); }
        p += 8 + size + extra;
    }
    println!("end 0x{p:X} of {}", d.len());
    for r in 0..16 { println!("{:08X}: {:08X}", p + 4 * r, u32le(&d, p + 4 * r)); }
    Ok(())
}

/// `xmat-info <file.xmat> [-v]`
fn xmat_info(file: &Path, verbose: bool) -> Result<(), String> {
    let d = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let x = abgtool::xmat::parse(&d)?;
    print!("{}", abgtool::xmat::describe(&x, verbose));
    Ok(())
}
