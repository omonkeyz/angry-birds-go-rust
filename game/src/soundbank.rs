//! The game's sound definitions (`data/audio/sound.xml`, decoded with `abgtool xox`).
//!
//! Layout of the original file: `<MasterVolume>` (Min/Max), `<Profiles>` (device quality -> MaxInstances),
//! `<Containers>` with `<Atoms>` (one `<Atom>` per sound: name, mix group, volume, pitch range in semitones, loop,
//! variants = the wav files to pick from) and `<Switches>`, and `<MixGroups>` (per-group volume and instance limits).
//! The wav files of the original were shipped as mp3 in this APK, so a variant is resolved by its file name stem.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Node {
    pub name: String,
    pub attrs: HashMap<String, String>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }

    pub fn number(&self, name: &str) -> Option<f32> {
        self.child(name)?.text.trim().parse().ok()
    }

    pub fn flag(&self, name: &str) -> Option<bool> {
        match self.child(name)?.text.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}

/// Minimal XML reader for the decoded game files (elements, attributes, text; no entities, comments or CDATA).
pub fn parse_xml(text: &str) -> Result<Node, String> {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    let mut stack: Vec<Node> = vec![Node { name: "#document".into(), ..Default::default() }];
    while pos < bytes.len() {
        if bytes[pos] == b'<' {
            let end = text[pos..].find('>').ok_or("xml: unterminated tag")? + pos;
            let tag = &text[pos + 1..end];
            if let Some(name) = tag.strip_prefix('/') {
                let done = stack.pop().ok_or("xml: stray closing tag")?;
                if done.name != name.trim() {
                    return Err(format!("xml: </{}> closes <{}>", name.trim(), done.name));
                }
                stack.last_mut().ok_or("xml: stray closing tag")?.children.push(done);
            } else {
                let self_closing = tag.ends_with('/');
                let body = tag.trim_end_matches('/');
                let mut parts = body.splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or("").to_string();
                let mut attrs = HashMap::new();
                let mut rest = parts.next().unwrap_or("");
                while let Some(eq) = rest.find("=\"") {
                    let key = rest[..eq].trim().to_string();
                    let after = &rest[eq + 2..];
                    let close = after.find('"').ok_or("xml: unterminated attribute")?;
                    attrs.insert(key, after[..close].to_string());
                    rest = &after[close + 1..];
                }
                let node = Node { name, attrs, ..Default::default() };
                if self_closing {
                    stack.last_mut().unwrap().children.push(node);
                } else {
                    stack.push(node);
                }
            }
            pos = end + 1;
        } else {
            let next = text[pos..].find('<').map_or(bytes.len(), |i| i + pos);
            stack.last_mut().unwrap().text.push_str(&text[pos..next]);
            pos = next;
        }
    }
    if stack.len() != 1 {
        return Err("xml: unclosed element".into());
    }
    stack.pop().unwrap().children.into_iter().next().ok_or_else(|| "xml: empty document".to_string())
}

#[derive(Debug, Clone)]
pub struct Atom {
    pub id: u32,
    pub name: String,
    pub mix_group: u32,
    pub priority: i32,
    pub volume: f32,
    pub min_pitch: f32,
    pub max_pitch: f32,
    pub looping: bool,
    pub music: bool,
    /// Variant file stems (lower case), e.g. `aby_ui_forward`.
    pub variants: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MixGroup {
    pub name: String,
    pub volume: f32,
}

pub struct SoundBank {
    pub master_max: f32,
    atoms: HashMap<String, Atom>,
    mix_groups: HashMap<u32, MixGroup>,
    /// file stem (lower case) -> path relative to the assets folder
    files: HashMap<String, PathBuf>,
}

fn stem_of(variant: &str) -> String {
    let file = variant.rsplit(['/', '\\']).next().unwrap_or(variant);
    file.rsplit_once('.').map_or(file, |(s, _)| s).to_ascii_lowercase()
}

impl SoundBank {
    /// `assets` = the assets folder; reads `xml_data/audio/sound.xml` and indexes `audio/**/*.mp3`.
    pub fn load(assets: &Path) -> Result<SoundBank, String> {
        let xml_path = assets.join("xml_data/audio/sound.xml");
        let text = std::fs::read_to_string(&xml_path).map_err(|e| format!("{}: {e}", xml_path.display()))?;
        let root = parse_xml(&text)?;
        let master_max = root.child("MasterVolume").and_then(|m| m.number("Max")).unwrap_or(1.0);

        let mut atoms = HashMap::new();
        if let Some(atom_list) = root.child("Containers").and_then(|c| c.child("Atoms")) {
            for a in atom_list.children_named("Atom") {
                let Some(name) = a.child("Name").map(|n| n.text.trim().to_string()) else { continue };
                // sounds that differ between the human player and AI karts carry per-player properties; use the human's
                let human = a.children_named("PlayerProperties").find(|p| p.child("Type").is_some_and(|t| t.text.trim() == "Human"));
                let volume = a.number("Volume").or_else(|| human.and_then(|h| h.number("Volume"))).unwrap_or(1.0);
                let priority = a.number("Priority").or_else(|| human.and_then(|h| h.number("Priority"))).unwrap_or(1.0) as i32;
                atoms.insert(
                    name.to_ascii_lowercase(),
                    Atom {
                        id: a.attrs.get("id").and_then(|v| v.parse().ok()).unwrap_or(0),
                        name,
                        mix_group: a.attrs.get("mixgroup").and_then(|v| v.parse().ok()).unwrap_or(0),
                        priority,
                        volume,
                        min_pitch: a.number("MinPitch").unwrap_or(0.0),
                        max_pitch: a.number("MaxPitch").unwrap_or(0.0),
                        looping: a.flag("Loop").unwrap_or(false),
                        music: a.flag("Music").unwrap_or(false),
                        variants: a.children_named("Variant").map(|v| stem_of(&v.text)).collect(),
                    },
                );
            }
        }

        let mut mix_groups = HashMap::new();
        if let Some(groups) = root.child("MixGroups") {
            for g in groups.children_named("MixGroup") {
                if let Some(id) = g.attrs.get("id").and_then(|v| v.parse().ok()) {
                    mix_groups.insert(
                        id,
                        MixGroup { name: g.child("Name").map(|n| n.text.trim().to_string()).unwrap_or_default(), volume: g.number("Volume").unwrap_or(1.0) },
                    );
                }
            }
        }

        let mut files = HashMap::new();
        let audio_root = assets.join("audio");
        let mut pending = vec![audio_root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|e| e == "mp3") {
                    let relative = path.strip_prefix(assets).unwrap().to_path_buf();
                    files.insert(path.file_stem().unwrap().to_string_lossy().to_ascii_lowercase(), relative);
                }
            }
        }
        Ok(SoundBank { master_max, atoms, mix_groups, files })
    }

    pub fn atom(&self, name: &str) -> Option<&Atom> {
        self.atoms.get(&name.to_ascii_lowercase())
    }

    /// Final gain of `atom`: master maximum x mix group volume x the atom's own volume.
    pub fn gain(&self, atom: &Atom) -> f32 {
        let group = self.mix_groups.get(&atom.mix_group).map_or(1.0, |g| g.volume);
        self.master_max * group * atom.volume
    }

    /// Path (relative to the assets folder) of variant `pick` of `atom`.
    pub fn file(&self, atom: &Atom, pick: usize) -> Option<&Path> {
        let stem = atom.variants.get(pick % atom.variants.len().max(1))?;
        self.files.get(stem).map(|p| p.as_path())
    }

    pub fn atom_count(&self) -> usize {
        self.atoms.len()
    }

    /// Atoms whose variants have no matching file in the APK (they were downloaded content).
    pub fn missing_files(&self) -> Vec<&str> {
        let mut missing: Vec<&str> = self
            .atoms
            .values()
            .filter(|a| a.variants.iter().any(|v| !self.files.contains_key(v)))
            .map(|a| a.name.as_str())
            .collect();
        missing.sort();
        missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank() -> SoundBank {
        SoundBank::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets")).expect("sound bank")
    }

    #[test]
    fn parses_the_original_definitions() {
        let b = bank();
        assert!((b.master_max - 0.7).abs() < 1e-6);
        assert!(b.atom_count() > 600, "{} atoms", b.atom_count());
        let forward = b.atom("ABY_ui_forward").expect("ui forward");
        assert_eq!(forward.priority, 1);
        assert!(b.file(forward, 0).is_some_and(|p| p.ends_with("aby_ui_forward.mp3")));
        println!("ui forward gain {:.3}", b.gain(forward));
    }

    #[test]
    fn music_atoms_resolve_to_files() {
        let b = bank();
        for name in ["ABY_music_main_menu", "ABY_music_Cobalt", "ABY_music_results"] {
            let a = b.atom(name).unwrap_or_else(|| panic!("{name} missing"));
            assert!(a.music, "{name} should be flagged as music");
            assert!(b.file(a, 0).is_some(), "{name} has no file");
            println!("{name}: gain {:.3}, loop {}", b.gain(a), a.looping);
        }
    }

    #[test]
    fn reports_which_sounds_need_downloaded_files() {
        let b = bank();
        let missing = b.missing_files();
        println!("{} of {} atoms reference files that are not in the APK: {:?}", missing.len(), b.atom_count(), &missing[..missing.len().min(12)]);
    }
}
