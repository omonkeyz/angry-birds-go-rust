//! String database (`locdb.xlc`, magic XGSL) of the 2.9.1 build: the key table followed by the English block.
use std::collections::HashMap;
use std::path::Path;

pub struct Loc {
    strings: HashMap<String, String>,
}

impl Loc {
    pub fn load(path: &Path) -> Result<Loc, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if data.len() < 0x78 || &data[0..4] != b"XGSL" {
            return Err(format!("{}: not an XGSL string database", path.display()));
        }
        let count = u32::from_le_bytes([data[0x10], data[0x11], data[0x12], data[0x13]]) as usize;
        let mut at = 0x78usize;
        let mut next = |at: &mut usize| -> Option<String> {
            if *at >= data.len() {
                return None;
            }
            let end = *at + data[*at..].iter().position(|&b| b == 0).unwrap_or(data.len() - *at);
            let s = String::from_utf8_lossy(&data[*at..end]).into_owned();
            *at = end + 1;
            Some(s)
        };
        let keys: Vec<String> = (0..count).filter_map(|_| next(&mut at)).collect();
        let mut strings = HashMap::with_capacity(keys.len());
        for key in keys {
            let Some(value) = next(&mut at) else { break };
            strings.insert(key, value);
        }
        Ok(Loc { strings })
    }

    /// The English text for `key`, or the key itself when there is none (the game does the same).
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings.get(key).map(|s| s.as_str()).unwrap_or(key)
    }
}
