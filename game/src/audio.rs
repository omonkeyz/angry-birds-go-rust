//! Sound: looping music + one-shot effects, decoded from the game's own mp3 files (pure-Rust symphonia via rodio).
//!
//! Sounds are played by the names used in the original `sound.xml` (see `soundbank`): the atom decides the file variant,
//! the gain (master x mix group x atom volume), the pitch range and looping. A key that is not an atom name is treated as
//! a plain path relative to the assets folder (used only by the placeholder race).
use crate::soundbank::SoundBank;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::io::Cursor;
use std::path::{Path, PathBuf};

pub struct Audio {
    sink: MixerDeviceSink,
    music: Player,
    engine: Player,
    engine_loaded: bool,
    assets: PathBuf,
    bank: Option<SoundBank>,
    current_music: Option<String>,
    rng: u32,
}

impl Audio {
    /// `None` (with a message on stderr) when there is no usable output device; the game then runs silent.
    pub fn new(assets: &Path) -> Option<Audio> {
        let mut sink = match DeviceSinkBuilder::open_default_sink() {
            Ok(sink) => sink,
            Err(e) => {
                eprintln!("audio disabled: {e}");
                return None;
            }
        };
        sink.log_on_drop(false);
        let music = Player::connect_new(&sink.mixer());
        let engine = Player::connect_new(&sink.mixer());
        engine.set_volume(0.0);
        let bank = match SoundBank::load(assets) {
            Ok(bank) => Some(bank),
            Err(e) => {
                eprintln!("sound.xml not loaded, using default volumes: {e}");
                None
            }
        };
        Some(Audio { sink, music, engine, engine_loaded: false, assets: assets.to_path_buf(), bank, current_music: None, rng: 0x1234_5678 })
    }

    fn random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.rng >> 8) & 0xFFFF) as f32 / 65535.0
    }

    /// The kart's rolling / engine loop. `Some((volume, pitch))` while racing, `None` when silent.
    pub fn set_engine(&mut self, state: Option<(f32, f32)>) {
        match state {
            Some((volume, pitch)) => {
                if !self.engine_loaded {
                    match self.decode_path("audio/karts/aby_kart_rolling_wood_upgrade1.mp3") {
                        Ok(source) => {
                            self.engine.append(source.repeat_infinite());
                            self.engine.play();
                            self.engine_loaded = true;
                        }
                        Err(e) => {
                            eprintln!("engine: {e}");
                            return;
                        }
                    }
                }
                self.engine.set_volume(volume);
                self.engine.set_speed(pitch);
            }
            None => self.engine.set_volume(0.0),
        }
    }

    fn decode_path(&self, relative: impl AsRef<Path>) -> Result<Decoder<Cursor<Vec<u8>>>, String> {
        let path = self.assets.join(relative);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Decoder::new(Cursor::new(bytes)).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// (file, gain, semitone shift, looping) for `key`: an atom name from sound.xml, else a plain path.
    fn resolve(&mut self, key: &str) -> (PathBuf, f32, f32, bool) {
        if let Some(bank) = &self.bank {
            if let Some(atom) = bank.atom(key).cloned() {
                let pick = (self.random() * atom.variants.len().max(1) as f32) as usize;
                let semitones = if atom.max_pitch > atom.min_pitch { atom.min_pitch + (atom.max_pitch - atom.min_pitch) * self.random() } else { 0.0 };
                if let Some(file) = self.bank.as_ref().unwrap().file(&atom, pick) {
                    return (file.to_path_buf(), self.bank.as_ref().unwrap().gain(&atom), semitones, atom.looping);
                }
                eprintln!("sound {key}: file not in the APK (downloaded content)");
            }
        }
        (PathBuf::from(key), 0.8, 0.0, false)
    }

    /// Starts music `key` looping, unless it is already the current track.
    pub fn play_music(&mut self, key: &str) {
        if self.current_music.as_deref() == Some(key) {
            return;
        }
        let (file, gain, _, _) = self.resolve(key);
        match self.decode_path(&file) {
            Ok(source) => {
                self.music.clear();
                self.music.set_volume(gain);
                self.music.append(source.repeat_infinite());
                self.music.play();
                self.current_music = Some(key.to_string());
            }
            Err(e) => eprintln!("music: {e}"),
        }
    }

    pub fn stop_music(&mut self) {
        self.music.clear();
        self.current_music = None;
    }

    pub fn play_sfx(&mut self, key: &str) {
        let (file, gain, semitones, _) = self.resolve(key);
        match self.decode_path(&file) {
            Ok(source) => {
                let speed = 2f32.powf(semitones / 12.0);
                self.sink.mixer().add(source.speed(speed).amplify(gain));
            }
            Err(e) => eprintln!("sfx: {e}"),
        }
    }
}
