//! XOX1: the game's tokenised XML (`*.xml` inside the paks and the loose `track.xml` files).
//!
//! Layout (little endian), reverse-engineered from the v1.0.1 APK:
//!   0x00  "XOX1"
//!   0x04  u32 string count N
//!   0x08  (N + 1) u32 cumulative end offsets, the first is 0
//!   then   the string table: N NUL-terminated strings, in order of first use
//!   then   the document: ordinary XML text where every name and every value is replaced by a
//!          token. `TOKENS` is a fixed table of 115 non-structural bytes (identical in all 517
//!          files that were checked). A token is a run of table bytes read as base-115 digits,
//!          most significant first: one byte = string 0..114, two bytes (`TOKENS[1]`, `TOKENS[k]`)
//!          = string 115 + k, and so on. The bytes `< > / = "` whitespace and NUL are structural.

use std::collections::HashMap;

const TOKENS_V1: [u8; 115] = [
    0xEB, 0x72, 0x51, 0xE1, 0x6A, 0xC0, 0xFB, 0x50, 0x53, 0x54, 0xD4, 0xCE, 0x6D, 0xE7, 0xD1, 0xDF, 0x77, 0xE4, 0xEF, 0xF1,
    0xD5, 0xD2, 0xD8, 0xC8, 0xC2, 0xEA, 0xCD, 0xFC, 0x4F, 0xCB, 0xCA, 0x52, 0x49, 0x5A, 0x4C, 0x69, 0x78, 0x4B, 0xD6, 0xF2,
    0x6B, 0x70, 0xE0, 0x76, 0xE3, 0x48, 0xF5, 0x71, 0xCC, 0xF6, 0x73, 0xFE, 0xF9, 0xDC, 0xC7, 0x68, 0x58, 0xDE, 0x4E, 0x59,
    0xD3, 0x66, 0xEC, 0xDD, 0xFD, 0xFA, 0x61, 0xE6, 0xC9, 0x44, 0xDA, 0x56, 0xE9, 0x67, 0x74, 0x65, 0x47, 0x57, 0x43, 0x75,
    0xDB, 0xC3, 0x64, 0xF0, 0xE2, 0xC4, 0x6F, 0x3A, 0xE8, 0x6E, 0x79, 0x4D, 0xEE, 0xE5, 0x7A, 0x45, 0x4A, 0xC6, 0xD0, 0x6C,
    0xED, 0x46, 0xD9, 0xF3, 0xF4, 0xF8, 0xCF, 0xC5, 0x63, 0x42, 0x55, 0x62, 0x41, 0xFF, 0xC1,
];

/// XOX2 (2.9.1 build): same container, different fixed token table.
const TOKENS_V2: [u8; 115] = [
    0x78, 0x65, 0xD3, 0xE2, 0x6E, 0xF3, 0xDA, 0xC5, 0x76, 0xD2, 0x59, 0xC3, 0x52, 0x48, 0x6B, 0x58, 0xFC, 0xE8, 0xF2, 0x72, 0x62, 0x63, 0x61, 0xD1, 0xED, 0xFB, 0x4B, 0xEA, 0x44, 0x6A, 0x42, 0x49, 0x43, 0x4E, 0xE0, 0xEF, 0x3A, 0x41, 0x6C, 0x46, 0xE1, 0x4C, 0xC2, 0xCC, 0x64, 0xF8, 0x66, 0xCE, 0xF9, 0x5A, 0x56, 0x77, 0xD8, 0x4F, 0xF0, 0xDE, 0xCA, 0x57, 0xDD, 0xDF, 0x53, 0xFA, 0x4A, 0xE9, 0xDC, 0x51, 0xE7, 0x73, 0x68, 0x7A, 0x71, 0xD4, 0x74, 0xFD, 0xEE, 0xC8, 0xF1, 0xF4, 0xF6, 0xC9, 0xC4, 0xC6, 0xD6, 0xD9, 0xCF, 0xE3, 0x70, 0x75, 0xDB, 0xFE, 0xF5, 0xC7, 0xEB, 0xD5, 0x79, 0xCB, 0x47, 0xCD, 0xEC, 0x50, 0xC1, 0x6F, 0x69, 0xE6, 0x54, 0x45, 0xFF, 0xE5, 0x6D, 0x55, 0xC0, 0x67, 0x4D, 0xE4, 0xD0,
];

fn u32_at(data: &[u8], at: usize) -> Result<u32, String> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("xox: read past end at 0x{at:X}"))
}

pub fn is_xox(data: &[u8]) -> bool {
    data.len() >= 8 && (&data[0..4] == b"XOX1" || &data[0..4] == b"XOX2")
}

pub fn decode(data: &[u8]) -> Result<String, String> {
    if !is_xox(data) {
        return Err("xox: bad magic".into());
    }
    let count = u32_at(data, 4)? as usize;
    let offsets_at = 8;
    let table_at = offsets_at + 4 * (count + 1);
    let mut strings: Vec<String> = Vec::with_capacity(count);
    for i in 0..count {
        let start = table_at + u32_at(data, offsets_at + 4 * i)? as usize;
        let end = table_at + u32_at(data, offsets_at + 4 * (i + 1))? as usize;
        let raw = data.get(start..end).ok_or("xox: string table out of range")?;
        let text = raw.strip_suffix(&[0]).unwrap_or(raw);
        strings.push(String::from_utf8_lossy(text).into_owned());
    }
    let document_at = table_at + u32_at(data, offsets_at + 4 * count)? as usize;
    let document = data.get(document_at..).ok_or("xox: document out of range")?;

    let v2 = &data[0..4] == b"XOX2";
    if v2 {
        return decode_v2(&strings, document);
    }
    let tokens: &[u8; 115] = &TOKENS_V1;
    // XOX2 reserves token 0 for a wrapper element around the whole document, so string i is token i + 1
    let shift = usize::from(v2);
    let slot_of: HashMap<u8, usize> = tokens.iter().enumerate().map(|(slot, &byte)| (byte, slot)).collect();
    let mut out = String::with_capacity(document.len() * 2);
    let mut run: Option<usize> = None; // index of the token currently being read
    let mut highest = 0usize;
    let mut flush = |run: &mut Option<usize>, out: &mut String| -> Result<(), String> {
        if let Some(token) = run.take() {
            highest = highest.max(token + 1);
            if token < shift {
                out.push_str("XOX2ROOT");
                return Ok(());
            }
            let index = token - shift;
            let text = strings.get(index).ok_or_else(|| format!("xox: token #{token} but only {count} strings"))?;
            out.push_str(text);
        }
        Ok(())
    };
    for &byte in document {
        if byte == 0 {
            break;
        }
        match slot_of.get(&byte) {
            // a token is a run of table bytes read as digits in base 115, most significant first
            Some(&digit) => run = Some(run.map_or(digit, |value| value * tokens.len() + digit)),
            None => {
                flush(&mut run, &mut out)?;
                out.push(byte as char);
            }
        }
    }
    flush(&mut run, &mut out)?;
    if highest != count + shift {
        return Err(format!("xox: highest token is #{highest}, string table has {count} (+{shift} reserved)"));
    }
    check_balanced(&out)?;
    if v2 {
        // drop the reserved wrapper element
        if let Some(inner) = out.strip_prefix("<XOX2ROOT>").and_then(|s| s.strip_suffix("</XOX2ROOT>")) {
            return Ok(inner.to_string());
        }
    }
    Ok(out)
}

/// The real XOX2 digit alphabet: `TOKENS_V2` lists the bytes in order of first appearance in a document, which starts with the
/// wrapper element's byte 0x78; the game's table has that byte at index 39 and everything else in the order listed.
fn xox2_alphabet() -> [u8; 115] {
    let mut t = [0u8; 115];
    let mut k = 0;
    for (i, &b) in TOKENS_V2.iter().enumerate() {
        if i == 0 {
            continue;
        }
        if k == 39 {
            t[k] = TOKENS_V2[0];
            k += 1;
        }
        t[k] = b;
        k += 1;
    }
    t
}

/// XOX2 (2.9.x build): the same container as XOX1 (`XOX2`, N, N+1 offsets, strings, document) with its own digit alphabet
/// (`xox2_alphabet`). Names and values are tokens = runs of alphabet bytes read as base-115 digits, most significant first,
/// and the number is the index of the string in the table (`XGSXMLObfuscator_IndexDeobfuscate` + `CXGSXmlReader::NodeDeobfuscate`
/// in libABK291). The whole document is wrapped in one element whose name is the literal byte 'x' (the reader skips it and
/// iterates its children). Closing tags repeat the opening tag's token.
fn decode_v2(strings: &[String], document: &[u8]) -> Result<String, String> {
    let alphabet = xox2_alphabet();
    let slot_of: HashMap<u8, usize> = alphabet.iter().enumerate().map(|(slot, &byte)| (byte, slot)).collect();
    let mut out = String::with_capacity(document.len() * 2);
    let mut used = vec![false; strings.len()];
    let mut stack: Vec<String> = Vec::new();
    let mut run: Option<usize> = None;
    let mut closing = false;
    let mut first = true;
    let mut flush = |run: &mut Option<usize>, out: &mut String, stack: &mut Vec<String>, closing: bool| -> Result<(), String> {
        let Some(value) = run.take() else { return Ok(()) };
        let text = if std::mem::replace(&mut first, false) {
            "XOX2ROOT".to_string() // wrapper element
        } else if closing {
            stack.last().cloned().ok_or("xox2: closing tag with nothing open")?
        } else {
            used.get_mut(value).map(|u| *u = true);
            strings.get(value).cloned().ok_or_else(|| format!("xox2: token #{value} but only {} strings", strings.len()))?
        };
        if out.ends_with('<') {
            stack.push(text.clone());
        }
        out.push_str(&text);
        Ok(())
    };
    for &byte in document {
        if byte == 0 {
            break;
        }
        if let Some(&digit) = slot_of.get(&byte) {
            run = Some(run.map_or(digit, |v| v * 115 + digit));
            continue;
        }
        flush(&mut run, &mut out, &mut stack, closing)?;
        match byte {
            b'/' if out.ends_with('<') => closing = true,
            b'>' if out.ends_with('/') && !closing => {
                stack.pop(); // self-closing element
            }
            b'>' if closing => {
                stack.pop();
                closing = false;
            }
            _ => {}
        }
        out.push(byte as char);
    }
    flush(&mut run, &mut out, &mut stack, closing)?;
    // strings the document never refers to are left-over text of `<!-- -->` comments, which the writer numbers but never emits
    if let Some(i) = used.iter().position(|u| !u) {
        if !strings[i..].iter().zip(&used[i..]).all(|(s, u)| *u || s.trim_end().ends_with("-->")) {
            return Err(format!("xox2: string #{i} {:?} is never used", strings[i]));
        }
    }
    check_balanced(&out)?;
    Ok(out.strip_prefix("<XOX2ROOT>").and_then(|s| s.strip_suffix("</XOX2ROOT>")).map(str::to_string).unwrap_or(out))
}

/// Cheap well-formedness check: every `<name ...>` is closed by the matching `</name>`.
fn check_balanced(xml: &str) -> Result<(), String> {
    let mut stack: Vec<&str> = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let close = after.find('>').ok_or("xox: unterminated tag")?;
        let tag = &after[..close];
        rest = &after[close + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            let top = stack.pop().ok_or_else(|| format!("xox: stray </{name}>"))?;
            if top != name.trim() {
                return Err(format!("xox: </{name}> closes <{top}>"));
            }
        } else if !tag.ends_with('/') && !tag.starts_with('?') && !tag.starts_with('!') {
            stack.push(tag.split_whitespace().next().unwrap_or(""));
        }
    }
    if let Some(open) = stack.last() {
        return Err(format!("xox: <{open}> never closed"));
    }
    Ok(())
}




/// The game builds its token alphabet at start-up: ':', 'A'-'Z', 'a'-'z', then the Latin-1 letters without x and /,
/// shuffled by 65,536 random swaps of two slots (indices taken modulo 115) from a Marsaglia multiply-with-carry
/// generator with fixed seeds (`XGSXMLObfuscator_Initialize`).
pub fn shuffled_alphabet(mut z: u32, mut w: u32) -> [u8; 115] {
    let mut table: Vec<u8> = vec![0x3A];
    table.extend(0x41..=0x5Au8);
    table.extend(0x61..=0x7Au8);
    table.extend(0xC0..=0xD6u8);
    table.extend(0xD8..=0xF6u8);
    table.extend(0xF8..=0xFFu8);
    for _ in 0..0x10000 {
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

#[cfg(test)]
mod alphabet_tests {
    use super::*;

    #[test]
    fn prints_candidate_alphabets() {
        for (z, w) in [(362436069u32, 521288629u32), (521288629, 362436069), (123456789, 987654321), (1, 1)] {
            let t = shuffled_alphabet(z, w);
            let hits = t.iter().zip(TOKENS_V1.iter()).filter(|(a, b)| a == b).count();
            println!("seeds {z},{w}: {:02X?}.. matches XOX1 table in {hits}/115 slots", &t[..8]);
        }
    }
}


#[cfg(test)]
mod xox2_tests {
    use super::*;

    #[test]
    fn xox2_shipped_files_when_present() {
        let root = std::path::Path::new(r"C:\Users\Brady\Desktop\AngryBirdsGo\assets292\pak");
        let Ok(data) = std::fs::read(root.join(r"cars\carxml\kart_base_max.xml")) else { return };
        let text = decode(&data).unwrap();
        assert!(text.starts_with("<CarSpec m_fMinDesiredSpeed=\"40.0\" m_fDrag=\"1\""), "{text}");
        let data = std::fs::read(root.join(r"gameplay\eventdef_episode00\eventdef_episode00_event01_stage04.xml")).unwrap();
        assert!(decode(&data).unwrap().contains("<Stars Star1=\"60000\" Star2=\"85000\" Star3=\"95000\"/>"));
    }
}
