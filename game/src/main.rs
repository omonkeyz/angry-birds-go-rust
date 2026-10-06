//! Angry Birds Go! - native Rust port.
//!
//!   cargo run --profile fast                      open the game window
//!   cargo run --profile fast -- --landing         skip the intro, start on the landing screen
//!   cargo run --profile fast -- --dev-race        enable the PLACEHOLDER race (not part of the 1:1 port, see game.rs)
//!   cargo run --profile fast -- --capture 7.0 out.png [--hover] [--events] [--tracks]
//!                                                 render one menu frame offscreen to a PNG and exit
//!   cargo run --profile fast -- --capture-race <track 0-2> <seconds> out.png [--drive]
//!                                                 run a race headlessly for <seconds> and save a frame
//!
//! Keys: arrows / WASD drive, Space handbrake, R reset (or race again), Enter continue, Esc back, F fullscreen.
mod abilities;
mod audio;
mod damage;
mod modes;
mod powerups;
mod camera;
mod carsim;
mod playerinput;
mod drive;
mod eventextras;
mod eventrun;
mod flow;
mod fullkart;
mod font;
mod game;
mod gfx;
mod items;
mod meta;
mod kart;
mod launch;
mod loc;
mod menu;
mod models;
mod race;
mod raceai;
mod racerules;
mod render3d;
mod sdf;
mod soundbank;
mod track;
mod trackworld;
mod xmodel;
mod ui;
mod uix;

use game::{Game, GameKey, Outcome};
use gfx::{Draw, SpriteRenderer};
use glam::Vec3;
use launch::LANDING_START;
use race::Keys;
use render3d::Scene3d;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use ui::{Textures, STAGE_H, STAGE_W};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{CursorIcon, Fullscreen, Window, WindowId},
};

/// Finds the `assets` folder produced by abgtool, next to the exe or a few levels above it.
fn find_assets() -> Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("ABG_ASSETS") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(cwd) = std::env::current_dir() {
        for up in [".", "..", "../.."] {
            candidates.push(cwd.join(up).join("assets"));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..5 {
            if let Some(d) = dir {
                candidates.push(d.join("assets"));
                dir = d.parent().map(|p| p.to_path_buf());
            } else {
                break;
            }
        }
    }
    candidates
        .into_iter()
        .find(|p| p.join("textures/screens/splash_cmp_noalpha_1_of_1.png").is_file())
        .ok_or_else(|| "could not find the assets folder (set ABG_ASSETS to its path)".to_string())
}

/// Letterbox transform from the 1920x1080 stage to the window.
#[derive(Clone, Copy)]
struct Stage {
    scale: f32,
    ox: f32,
    oy: f32,
}

impl Stage {
    fn fit(w: u32, h: u32) -> Stage {
        let scale = (w as f32 / STAGE_W).min(h as f32 / STAGE_H);
        Stage { scale, ox: (w as f32 - STAGE_W * scale) / 2.0, oy: (h as f32 - STAGE_H * scale) / 2.0 }
    }

    fn to_stage(&self, px: f64, py: f64) -> (f32, f32) {
        ((px as f32 - self.ox) / self.scale, (py as f32 - self.oy) / self.scale)
    }

    fn apply(&self, draws: &mut [Draw]) {
        for d in draws {
            let r = &mut d.inst.rect;
            *r = [r[0] * self.scale + self.ox, r[1] * self.scale + self.oy, r[2] * self.scale, r[3] * self.scale];
        }
    }
}

fn non_srgb(caps: &wgpu::SurfaceCapabilities) -> wgpu::TextureFormat {
    caps.formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or_else(|| caps.formats.first().copied().unwrap_or(wgpu::TextureFormat::Bgra8Unorm))
}

fn map_key(key: &Key) -> Option<GameKey> {
    match key {
        Key::Named(NamedKey::ArrowUp) => Some(GameKey::Up),
        Key::Named(NamedKey::ArrowDown) => Some(GameKey::Down),
        Key::Named(NamedKey::ArrowLeft) => Some(GameKey::Left),
        Key::Named(NamedKey::ArrowRight) => Some(GameKey::Right),
        Key::Named(NamedKey::Space) | Key::Named(NamedKey::Shift) => Some(GameKey::Handbrake),
        Key::Named(NamedKey::Enter) => Some(GameKey::Enter),
        Key::Character(c) => match c.to_ascii_lowercase().as_str() {
            "w" => Some(GameKey::Up),
            "s" => Some(GameKey::Down),
            "a" => Some(GameKey::Left),
            "d" => Some(GameKey::Right),
            "r" => Some(GameKey::Reset),
            "e" => Some(GameKey::Ability),
            _ => None,
        },
        _ => None,
    }
}

/// 3D scene first (when the current screen has one), then the 2D layer on top.
fn render_frame(
    sprites: &mut SpriteRenderer,
    scene3d: &mut Scene3d,
    game: &Game,
    draws: &[Draw],
    target: &wgpu::TextureView,
    w: u32,
    h: u32,
) {
    match game.world(w as f32 / h as f32) {
        Some(world) if world.overlay => {
            let split = game.split().min(draws.len());
            sprites.render(&draws[..split], target, w, h, Some([0.0, 0.0, 0.0]));
            scene3d.render(&world, target, w, h);
            sprites.render(&draws[split..], target, w, h, None);
        }
        Some(world) => {
            scene3d.render(&world, target, w, h);
            sprites.render(draws, target, w, h, None);
        }
        None => sprites.render(draws, target, w, h, Some([0.0, 0.0, 0.0])),
    }
}

struct WindowGfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: SpriteRenderer,
    scene3d: Scene3d,
    game: Game,
}

struct App {
    gfx: Option<WindowGfx>,
    audio: Option<audio::Audio>,
    last_title: String,
    assets: PathBuf,
    start: Instant,
    last_update: Instant,
    skip_seconds: f32,
    dev_race: bool,
    cursor: (f64, f64),
    script: Vec<String>,
    script_pos: usize,
    script_wait_until: f32,
    script_releases: Vec<(f32, String)>,
    frames: u32,
    fps_mark: (f32, u32),
}

impl App {
    fn now(&self) -> f32 {
        self.start.elapsed().as_secs_f32()
    }

    /// Per-frame housekeeping: simulation step, window title, queued sound effects, music and the engine loop.
    fn tick(&mut self) {
        let now = self.now();
        let dt = self.last_update.elapsed().as_secs_f32();
        self.last_update = Instant::now();
        let Some(g) = self.gfx.as_mut() else { return };
        g.game.tick(now);
        g.game.update(dt);
        let title = g.game.title();
        if self.last_title != title {
            g.window.set_title(&title);
            self.last_title = title;
        }
        let sounds = g.game.take_sounds();
        let music = g.game.music(now);
        let engine = g.game.engine();
        if let Some(audio) = self.audio.as_mut() {
            for sound in sounds {
                // the race can queue music stingers as sounds; everything else is a normal effect
                audio.play_sfx(sound);
            }
            match music {
                Some(track) => audio.play_music(track),
                None => audio.stop_music(),
            }
            audio.set_engine(engine);
        }
    }

    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let attributes = Window::default_attributes()
            .with_title("Angry Birds Go!")
            .with_inner_size(PhysicalSize::new(1280u32, 720u32));
        let window = Arc::new(event_loop.create_window(attributes).map_err(|e| e.to_string())?);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| format!("no suitable GPU adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| format!("request_device failed: {e}"))?;
        let caps = surface.get_capabilities(&adapter);
        let format = non_srgb(&caps);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        let mut scene3d = Scene3d::new(device.clone(), queue.clone(), format);
        let mut renderer = SpriteRenderer::new(device, queue, format);
        let textures = Textures::load(&mut renderer, &self.assets)?;
        let mut game = Game::new(textures, self.assets.clone(), self.skip_seconds, self.dev_race);
        game.preload_kart(&mut scene3d);
        match find_assets291(&self.assets).and_then(|root| flow::UiFlow::new(&mut renderer, &root)) {
            Ok(flow) => game.set_flow(flow),
            Err(e) => eprintln!("the 2.9.1 screens are unavailable ({e}); using the 1.0.1 menus"),
        }
        self.audio = audio::Audio::new(&self.assets);
        self.start = Instant::now();
        self.last_update = Instant::now();
        self.gfx = Some(WindowGfx { window, surface, config, renderer, scene3d, game });
        Ok(())
    }

    fn draw(&mut self) {
        self.frames += 1;
        let now = self.now();
        let Some(g) = self.gfx.as_mut() else { return };
        let (w, h) = (g.config.width, g.config.height);
        let mut draws = g.game.draws(now);
        Stage::fit(w, h).apply(&mut draws);

        let frame = match g.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                g.surface.configure(g.renderer.device(), &g.config);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        render_frame(&mut g.renderer, &mut g.scene3d, &g.game, &draws, &view, w, h);
        g.renderer.queue().present(frame);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_none() {
            if let Err(message) = self.init(event_loop) {
                eprintln!("error: {message}");
                event_loop.exit();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.tick();
        if self.gfx.is_some() && !self.script.is_empty() {
            self.run_script(event_loop);
        }
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let now = self.now();
        if std::env::var_os("ABG_LOG_INPUT").is_some() {
            match &event {
                WindowEvent::CursorMoved { position, .. } => eprintln!("[{now:.2}] cursor {:.0},{:.0}", position.x, position.y),
                WindowEvent::MouseInput { state, button, .. } => eprintln!("[{now:.2}] mouse {button:?} {state:?}"),
                WindowEvent::Focused(f) => eprintln!("[{now:.2}] focused {f}"),
                _ => {}
            }
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(g) = self.gfx.as_mut() {
                    g.config.width = size.width.max(1);
                    g.config.height = size.height.max(1);
                    g.surface.configure(g.renderer.device(), &g.config);
                }
            }
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state, repeat, .. }, .. } => {
                if repeat {
                    return;
                }
                self.on_key(event_loop, logical_key, state == ElementState::Pressed);
            }
            WindowEvent::CursorMoved { position, .. } => self.on_cursor(position.x, position.y),
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => self.on_mouse(state == ElementState::Pressed),
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }
}

impl App {
    /// Keyboard input (the window handler and the `--script` harness both come through here).
    fn on_key(&mut self, event_loop: &ActiveEventLoop, logical_key: Key, pressed: bool) {
        let now = self.now();
        if let Some(key) = map_key(&logical_key) {
            if let Some(g) = self.gfx.as_mut() {
                g.game.key(key, pressed, now, &mut g.scene3d);
            }
        }
        if !pressed {
            return;
        }
        match logical_key {
            Key::Named(NamedKey::Escape) => {
                if let Some(g) = self.gfx.as_mut() {
                    if !g.game.escape(now) {
                        event_loop.exit();
                    }
                }
            }
            Key::Character(c) if c.eq_ignore_ascii_case("f") => {
                if let Some(g) = &self.gfx {
                    let next = if g.window.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) };
                    g.window.set_fullscreen(next);
                }
            }
            _ => {}
        }
    }

    fn on_cursor(&mut self, px: f64, py: f64) {
        let now = self.now();
        self.cursor = (px, py);
        if let Some(g) = self.gfx.as_mut() {
            let (x, y) = Stage::fit(g.config.width, g.config.height).to_stage(px, py);
            let pointer = g.game.cursor_moved(x, y, now);
            g.window.set_cursor(if pointer { CursorIcon::Pointer } else { CursorIcon::Default });
        }
    }

    fn on_mouse(&mut self, pressed: bool) {
        let now = self.now();
        if let Some(g) = self.gfx.as_mut() {
            let (x, y) = Stage::fit(g.config.width, g.config.height).to_stage(self.cursor.0, self.cursor.1);
            if pressed {
                g.game.press(x, y, now);
                return;
            }
            match g.game.release(x, y, now) {
                Outcome::StartRace(track) => {
                    if let Err(e) = g.game.start_race(&mut g.scene3d, track, now) {
                        eprintln!("could not start the race: {e}");
                        g.game.show_notice("COULD NOT LOAD THE RACE - SEE CONSOLE", now);
                    }
                }
                Outcome::StartEvent(index) => {
                    if let Err(e) = g.game.start_event(&mut g.scene3d, index, now) {
                        eprintln!("could not start the event: {e}");
                        let short: String = e.chars().take(64).collect();
                        g.game.show_notice(&format!("EVENT NOT PLAYABLE: {short}").to_uppercase(), now);
                    }
                }
                Outcome::StartDrive => {
                    if let Err(e) = g.game.start_drive(&mut g.scene3d, now) {
                        eprintln!("could not start driving: {e}");
                        g.game.show_notice("COULD NOT LOAD THE KART - SEE CONSOLE", now);
                    }
                }
                Outcome::None => {}
            }
        }
    }

    /// `--script <file>`: scripted input for testing the real window path. One command per line:
    /// `wait <s>`, `key <name> down|up|tap`, `hold <name> <s>`, `click <stageX> <stageY>`, `press/release/move <stageX> <stageY>`,
    /// `shot <file.png>`, `say <text>`, `quit`. Events go through the same `on_key` / `on_cursor` / `on_mouse` as real window events.
    fn run_script(&mut self, event_loop: &ActiveEventLoop) {
        let now = self.now();
        loop {
            // timed key releases
            let due: Vec<usize> = (0..self.script_releases.len()).filter(|&i| self.script_releases[i].0 <= now).collect();
            for &i in due.iter().rev() {
                let (_, name) = self.script_releases.remove(i);
                if let Some(k) = script_key(&name) {
                    self.on_key(event_loop, k, false);
                }
            }
            if now < self.script_wait_until {
                return;
            }
            if self.script_pos >= self.script.len() {
                if self.script_pos == self.script.len() && !self.script.is_empty() {
                    self.script_pos += 1;
                    eprintln!("[script] finished");
                }
                return;
            }
            let line = self.script[self.script_pos].clone();
            self.script_pos += 1;
            let words: Vec<&str> = line.split_whitespace().collect();
            let stage_px = |s: &Self, x: f64, y: f64| -> (f64, f64) {
                let (w, h) = s.gfx.as_ref().map(|g| (g.config.width, g.config.height)).unwrap_or((1280, 720));
                let st = Stage::fit(w, h);
                (st.ox as f64 + x * st.scale as f64, st.oy as f64 + y * st.scale as f64)
            };
            let num = |i: usize| words.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            match words.as_slice() {
                [] => {}
                [c, ..] if c.starts_with('#') => {}
                ["wait", ..] => {
                    self.script_wait_until = now + num(1) as f32;
                    return;
                }
                ["key", name, how] => {
                    if let Some(k) = script_key(name) {
                        match *how {
                            "down" => self.on_key(event_loop, k, true),
                            "up" => self.on_key(event_loop, k, false),
                            _ => {
                                self.on_key(event_loop, k.clone(), true);
                                self.on_key(event_loop, k, false);
                            }
                        }
                    }
                }
                ["hold", name, secs] => {
                    if let Some(k) = script_key(name) {
                        self.on_key(event_loop, k, true);
                        self.script_releases.push((now + secs.parse::<f32>().unwrap_or(1.0), name.to_string()));
                    }
                }
                ["move", ..] => {
                    let (px, py) = stage_px(self, num(1), num(2));
                    self.on_cursor(px, py);
                }
                ["press", ..] => {
                    let (px, py) = stage_px(self, num(1), num(2));
                    self.on_cursor(px, py);
                    self.on_mouse(true);
                }
                ["release", ..] => {
                    let (px, py) = stage_px(self, num(1), num(2));
                    self.on_cursor(px, py);
                    self.on_mouse(false);
                }
                ["click", ..] => {
                    let (px, py) = stage_px(self, num(1), num(2));
                    self.on_cursor(px, py);
                    self.on_mouse(true);
                    self.script_wait_until = now + 0.12;
                    self.script.insert(self.script_pos, format!("release {} {}", num(1), num(2)));
                    return;
                }
                ["shot", path] => {
                    let path = path.to_string();
                    self.screenshot(&path);
                }
                ["say", ..] => {
                    let title = self.gfx.as_ref().map(|g| g.game.title()).unwrap_or_default();
                    let dbg = self.gfx.as_ref().map(|g| g.game.debug_line()).unwrap_or_default();
                    let fps = (self.frames - self.fps_mark.1) as f32 / (now - self.fps_mark.0).max(0.001);
                    self.fps_mark = (now, self.frames);
                    eprintln!("[script t={now:.1} fps {fps:.0}] {} | {title} | {dbg}", words[1..].join(" "));
                }
                ["size", w, h] => {
                    if let Some(g) = self.gfx.as_ref() {
                        let _ = g.window.request_inner_size(PhysicalSize::new(w.parse().unwrap_or(1280u32), h.parse().unwrap_or(720u32)));
                    }
                }
                ["autopilot", v] => {
                    if let Some(g) = self.gfx.as_mut() {
                        g.game.autopilot = *v == "on";
                    }
                }
                ["event", n] => {
                    // test aid: start campaign event N directly (skips the lock / energy gate but runs the real start path)
                    if let Some(g) = self.gfx.as_mut() {
                        g.game.skip_gate = true;
                        let r = g.game.start_event(&mut g.scene3d, n.parse().unwrap_or(0), now);
                        g.game.skip_gate = false;
                        if let Err(e) = r {
                            eprintln!("[script] event {n}: {e}");
                        }
                    }
                }
                ["quit"] => {
                    event_loop.exit();
                    return;
                }
                other => eprintln!("[script] unknown command {other:?}"),
            }
        }
    }

    /// Renders the current frame exactly like the window path does into a PNG (surface size).
    fn screenshot(&mut self, path: &str) {
        let now = self.now();
        let Some(g) = self.gfx.as_mut() else { return };
        let (w, h) = (g.config.width, g.config.height);
        let mut draws = g.game.draws(now);
        Stage::fit(w, h).apply(&mut draws);
        let texture = g.renderer.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("shot"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: g.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        render_frame(&mut g.renderer, &mut g.scene3d, &g.game, &draws, &view, w, h);
        let mut rgba = g.renderer.read_back(&texture, w, h);
        if matches!(g.config.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb) {
            for px in rgba.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        match image::save_buffer(path, &rgba, w, h, image::ColorType::Rgba8) {
            Ok(()) => eprintln!("[script] wrote {path} ({w}x{h})"),
            Err(e) => eprintln!("[script] {path}: {e}"),
        }
    }
}

fn script_key(name: &str) -> Option<Key> {
    Some(match name.to_ascii_lowercase().as_str() {
        "up" => Key::Named(NamedKey::ArrowUp),
        "down" => Key::Named(NamedKey::ArrowDown),
        "left" => Key::Named(NamedKey::ArrowLeft),
        "right" => Key::Named(NamedKey::ArrowRight),
        "space" => Key::Named(NamedKey::Space),
        "enter" => Key::Named(NamedKey::Enter),
        "esc" => Key::Named(NamedKey::Escape),
        s if s.len() == 1 => Key::Character(s.into()),
        _ => return None,
    })
}

/// Headless GPU context for the capture modes.
fn headless(assets: &PathBuf, skip: f32) -> Result<(SpriteRenderer, Scene3d, Game), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .map_err(|e| format!("no suitable GPU adapter: {e}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|e| format!("request_device failed: {e}"))?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let scene3d = Scene3d::new(device.clone(), queue.clone(), format);
    let mut renderer = SpriteRenderer::new(device, queue, format);
    let textures = Textures::load(&mut renderer, assets)?;
    // the capture / self-test modes exist to exercise the placeholder race, so they turn it on
    Ok((renderer, scene3d, Game::new(textures, assets.clone(), skip, true)))
}

fn save_frame(renderer: &mut SpriteRenderer, scene3d: &mut Scene3d, game: &Game, now: f32, out: &str) -> Result<(), String> {
    let (w, h) = (1920u32, 1080u32);
    let mut draws = game.draws(now);
    Stage::fit(w, h).apply(&mut draws);
    let texture = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("capture"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    render_frame(renderer, scene3d, game, &draws, &view, w, h);
    let rgba = renderer.read_back(&texture, w, h);
    image::save_buffer(out, &rgba, w, h, image::ColorType::Rgba8).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}

/// Renders one menu frame offscreen into a PNG, without opening a window.
fn capture(assets: PathBuf, t: f32, hover: bool, events: bool, tracks: bool, out: &str) -> Result<(), String> {
    let (mut renderer, mut scene3d, mut game) = headless(&assets, 0.0)?;
    game.set_launch_hover(hover);
    if events {
        game.open_events(0.0);
    }
    if tracks {
        game.open_tracks_for_capture();
    }
    save_frame(&mut renderer, &mut scene3d, &game, t, out)
}

/// Runs a race headlessly and saves a frame. `drive` = the player holds the throttle (no steering),
/// `autopilot` = the AI drives the player's kart, `cam` = kart-local camera offset "x,y,z" (left, up, forward).
fn capture_race(assets: PathBuf, track: usize, seconds: f32, drive: bool, autopilot: bool, cam: Option<Vec3>, out: &str) -> Result<(), String> {
    let (mut renderer, mut scene3d, mut game) = headless(&assets, 0.0)?;
    game.start_race(&mut scene3d, track, 0.0)?;
    if let Some(race) = game.race_mut() {
        race.autopilot = autopilot;
    }
    game.fast_forward_race(seconds, Keys { up: drive, ..Default::default() });
    if let Some(race) = game.race_mut() {
        race.debug_cam_rel = cam;
        // let the camera settle on the new position
        race.update(0.0, &Keys::default());
    }
    save_frame(&mut renderer, &mut scene3d, &game, seconds, out)
}

/// Drives on the baseplate headlessly: `throttle` / `steer` are held for `seconds`, then a frame is saved.
fn capture_drive(assets: PathBuf, seconds: f32, throttle: bool, steer: f32, cam: Option<Vec3>, out: &str) -> Result<(), String> {
    let (mut renderer, mut scene3d, mut game) = headless(&assets, 0.0)?;
    game.autopilot = std::env::args().any(|a| a == "--autopilot");
    match std::env::args().skip_while(|a| a != "--event").nth(1).and_then(|v| v.parse::<usize>().ok()) {
        Some(index) => game.start_event(&mut scene3d, index, 0.0)?,
        None => game.start_drive(&mut scene3d, 0.0)?,
    }
    if std::env::args().any(|a| a == "--pull") {
        // the pad path of UpdateSlingshotLaunch: hold Down to pull back, tap Space to fire
        game.fast_forward_drive(1.0, Keys { down: true, ..Default::default() });
        game.fast_forward_drive(1.0 / 60.0, Keys { handbrake: true, ..Default::default() });
    } else if throttle {
        if let Some(drive) = game.drive_mut() {
            drive.launch_with(1.0);
        }
    }
    let keys = Keys { left: steer < 0.0, right: steer > 0.0, ..Default::default() };
    game.fast_forward_drive(seconds, keys);
    if let Some(drive) = game.drive_mut() {
        drive.debug_cam_rel = cam;
    }
    let speed = game.drive_mut().map(|d| d.speed() * 3.6).unwrap_or(0.0);
    let p = game.drive_mut().map(|d| d.pos()).unwrap_or_default();
    println!("after {seconds}s: {speed:.0} km/h at x={:.1} z={:.1}", p.x, p.z);
    save_frame(&mut renderer, &mut scene3d, &game, seconds, out)
}

/// Plays a whole race with the AI in every kart (including the player's) and prints what happened.
fn selftest(assets: PathBuf, track: usize) -> Result<(), String> {
    let (_renderer, mut scene3d, mut game) = headless(&assets, 0.0)?;
    game.start_race(&mut scene3d, track, 0.0)?;
    game.race_mut().unwrap().autopilot = true;
    let started = Instant::now();
    let mut t = 0.0f32;
    while t < 420.0 {
        game.update(1.0 / 60.0);
        t += 1.0 / 60.0;
        if game.race_ref().is_some_and(|r| r.phase == race::Phase::Results) {
            break;
        }
    }
    let race = game.race_ref().unwrap();
    println!("track {} ({}): simulated {:.1}s of racing in {:.2}s of real time", track, race.track.def.name, race.clock, started.elapsed().as_secs_f32());
    println!("track length {:.0} m, {} laps, phase {:?}", race.track.length, race.laps(), race.phase);
    println!("{}", race.summary());
    if race.phase != race::Phase::Results {
        return Err("the race did not finish within 420 s".into());
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = find_assets()?;
    if let Some(i) = args.iter().position(|a| a == "--capture-race") {
        let track: usize = args.get(i + 1).and_then(|s| s.parse().ok()).ok_or("--capture-race needs <track> <seconds> <out.png>")?;
        let seconds: f32 = args.get(i + 2).and_then(|s| s.parse().ok()).ok_or("--capture-race needs <track> <seconds> <out.png>")?;
        let out = args.get(i + 3).ok_or("--capture-race needs <track> <seconds> <out.png>")?;
        let cam = args.iter().position(|a| a == "--cam").and_then(|j| args.get(j + 1)).and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|p| p.parse().ok()).collect();
            (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
        });
        return capture_race(assets, track, seconds, args.iter().any(|a| a == "--drive"), args.iter().any(|a| a == "--autopilot"), cam, out);
    }
    if let Some(i) = args.iter().position(|a| a == "--capture-event") {
        let usage = "--capture-event needs <campaign index> <seconds> <out.png>";
        let index: usize = args.get(i + 1).and_then(|s| s.parse().ok()).ok_or(usage)?;
        let seconds: f32 = args.get(i + 2).and_then(|s| s.parse().ok()).ok_or(usage)?;
        let out = args.get(i + 3).ok_or(usage)?;
        return capture_event(assets, index, seconds, out, &args);
    }
    if let Some(i) = args.iter().position(|a| a == "--capture-drive") {
        let seconds: f32 = args.get(i + 1).and_then(|s| s.parse().ok()).ok_or("--capture-drive needs <seconds> <out.png>")?;
        let out = args.get(i + 2).ok_or("--capture-drive needs <seconds> <out.png>")?;
        let cam = args.iter().position(|a| a == "--cam").and_then(|j| args.get(j + 1)).and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|p| p.parse().ok()).collect();
            (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
        });
        let steer = if args.iter().any(|a| a == "--right") { 1.0 } else if args.iter().any(|a| a == "--left") { -1.0 } else { 0.0 };
        return capture_drive(assets, seconds, args.iter().any(|a| a == "--throttle"), steer, cam, out);
    }
    if let Some(i) = args.iter().position(|a| a == "--capture-ui") {
        let screen = args.get(i + 1).ok_or("--capture-ui needs <screen> <out.png>")?;
        let out = args.get(i + 2).ok_or("--capture-ui needs <screen> <out.png>")?;
        let show = args.iter().position(|a| a == "--show").and_then(|j| args.get(j + 1)).map(|s| s.split(',').map(|p| p.to_string()).collect()).unwrap_or_default();
        return capture_ui(assets, screen, show, out);
    }
    if let Some(i) = args.iter().position(|a| a == "--capture-flow") {
        let state = args.get(i + 1).ok_or("--capture-flow needs <state> <out.png>")?;
        let out = args.get(i + 2).ok_or("--capture-flow needs <state> <out.png>")?;
        return capture_flow(assets, state, out);
    }
    if let Some(i) = args.iter().position(|a| a == "--capture-track") {
        let (theme, run, out) = (args.get(i + 1), args.get(i + 2), args.get(i + 3));
        let (Some(theme), Some(run), Some(out)) = (theme, run, out) else { return Err("--capture-track needs <theme> <run> <out.png>".into()) };
        return capture_track(assets, theme, run, out, &args);
    }
    if let Some(i) = args.iter().position(|a| a == "--capture-model") {
        let (Some(file), Some(out)) = (args.get(i + 1), args.get(i + 2)) else { return Err("--capture-model needs <file.xgm> <out.png>".into()) };
        return capture_model(assets, file, out, &args);
    }
    if let Some(i) = args.iter().position(|a| a == "--capture-kart") {
        let (Some(folder), Some(out)) = (args.get(i + 1), args.get(i + 2)) else { return Err("--capture-kart needs <folder> <out.png>".into()) };
        return capture_kart(assets, folder, out, &args);
    }
    if let Some(i) = args.iter().position(|a| a == "--selftest") {
        let track: usize = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
        return selftest(assets, track);
    }
    if let Some(i) = args.iter().position(|a| a == "--capture") {
        let t: f32 = args.get(i + 1).and_then(|s| s.parse().ok()).ok_or("--capture needs <seconds> <out.png>")?;
        let out = args.get(i + 2).ok_or("--capture needs <seconds> <out.png>")?;
        return capture(assets, t, args.iter().any(|a| a == "--hover"), args.iter().any(|a| a == "--events"), args.iter().any(|a| a == "--tracks"), out);
    }
    let skip_seconds = if args.iter().any(|a| a == "--landing") { LANDING_START } else { 0.0 };
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        gfx: None,
        audio: None,
        last_title: String::new(),
        assets,
        start: Instant::now(),
        last_update: Instant::now(),
        skip_seconds,
        dev_race: args.iter().any(|a| a == "--dev-race"),
        cursor: (0.0, 0.0),
        script: args.iter().position(|a| a == "--script").and_then(|i| args.get(i + 1)).and_then(|p| std::fs::read_to_string(p).ok()).map(|t| t.lines().map(|l| l.to_string()).collect()).unwrap_or_default(),
        script_pos: 0,
        script_wait_until: 0.0,
        script_releases: Vec::new(),
        frames: 0,
        fps_mark: (0.0, 0),
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

/// The 2.9.1 data folder (`assets291`) next to `assets`.
fn find_assets291(assets: &std::path::Path) -> Result<PathBuf, String> {
    let dir = assets.parent().map(|p| p.join("assets291")).ok_or("no parent folder")?;
    if dir.join("xml/xml/ui/ui.xml").is_file() {
        Ok(dir)
    } else {
        Err(format!("{} is missing the decoded 2.9.1 data", dir.display()))
    }
}

/// `--capture-ui <screen> <out.png> [--show Name,Name]`: renders a real 2.9.1 layout offscreen.
fn capture_ui(assets: PathBuf, screen: &str, show: Vec<String>, out: &str) -> Result<(), String> {
    let root = find_assets291(&assets)?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).map_err(|e| e.to_string())?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).map_err(|e| e.to_string())?;
    let mut renderer = SpriteRenderer::new(device, queue, wgpu::TextureFormat::Rgba8Unorm);
    let mut ui = uix::UiSystem::new(&mut renderer, &root)?;
    let mut draws = Vec::new();
    let mut hits = Vec::new();
    let show_refs: Vec<&str> = show.iter().map(|s| s.as_str()).collect();
    for layer in screen.split('+') {
        ui.load_screen(&mut renderer, layer)?;
        let _ = ui.draw(layer, &show_refs, &Default::default(), &mut draws, &mut hits);
    }
    println!("{} draws, {} click targets", draws.len(), hits.len());
    for d in draws.iter().rev().take(2) { println!("draw tex={:?} rect={:?} uv={:?} tint={:?}", d.tex, d.inst.rect, d.inst.uv, d.inst.tint); }
    for h in &hits {
        println!("  hit {} -> {} {:?}", h.id, h.action, h.sound);
    }
    let (w, h) = (1920u32, 1080u32);
    Stage::fit(w, h).apply(&mut draws);
    let rgba = renderer.render_to_rgba(&draws, w, h, [0.1, 0.1, 0.12]);
    image::save_buffer(out, &rgba, w, h, image::ColorType::Rgba8).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}

/// `--capture-flow <state> <out.png> [--page N]`: renders the front end the way the flow would show it.
/// `--capture-flow <state> <out.png>`: renders the front end the way the flow would show it.
fn capture_flow(assets: PathBuf, state: &str, out: &str) -> Result<(), String> {
    let root = find_assets291(&assets)?;
    let (mut renderer, mut scene3d, mut game) = headless(&assets, 0.0)?;
    game.set_flow(flow::UiFlow::new(&mut renderer, &root)?);
    game.preload_kart(&mut scene3d);
    game.goto_flow(state);
    let _ = game.draws(0.0);
    if let Some(n) = std::env::args().skip_while(|a| a != "--kart").nth(1).and_then(|v| v.parse::<i32>().ok()) {
        for _ in 0..n {
            game.key(GameKey::Right, true, 0.0, &mut scene3d);
        }
    }
    save_frame(&mut renderer, &mut scene3d, &game, 1.0, out)
}

/// `--capture-track <theme> <run> <out.png> [--from x,y,z] [--at x,y,z] [--q 3]`: renders a real 2.9.x track with its textures.
fn capture_track(assets: PathBuf, theme: &str, run: &str, out: &str, args: &[String]) -> Result<(), String> {
    let root = find_assets291(&assets)?.parent().unwrap().join("assets292");
    let (mut renderer, mut scene3d, _game) = headless(&assets, 0.0)?;
    let quality = args.iter().position(|a| a == "--q").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(2);
    let track = trackworld::TrackWorld::load(&mut scene3d, &root, theme, run, quality)?;
    let vec3 = |flag: &str| -> Option<Vec3> {
        let v: Vec<f32> = args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1))?.split(',').filter_map(|p| p.parse().ok()).collect();
        (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
    };
    let (start, dir) = track.start.unwrap_or((Vec3::ZERO, Vec3::Z));
    println!("{}: start {start:?} dir {dir:?}, race line {} points, {} draws", track.name, track.race_line.len(), track.draws.len());
    if args.iter().any(|a| a == "--probe") {
        for (i, p) in track.race_line.iter().enumerate().take(8) {
            println!("line {i}: {p:?}");
        }
        use crate::carsim::Ground;
        for (i, p) in track.race_line.iter().enumerate().step_by(10) {
            let r = track.ground.ray(*p + Vec3::Y * 5.0, Vec3::new(0.0, -60.0, 0.0));
            println!("line {i}: spline y {:.1}  ground {:?}", p.y, r.map(|h| (h.point.y, h.material)));
        }
    }
    let eye = vec3("--from").unwrap_or(start - dir * 12.0 + Vec3::Y * 6.0);
    let at = vec3("--at").unwrap_or(start + dir * 20.0);
    let view = glam::Mat4::look_at_rh(eye, at, Vec3::Y);
    let proj = glam::Mat4::perspective_rh(65f32.to_radians(), 16.0 / 9.0, 0.3, 6000.0);
    let world = render3d::World {
        camera: render3d::Camera { view_proj: proj * view, eye },
        draws: track.draws.clone(),
        sky: track.sky,
        fog_density: 0.0004,
        sun_dir: Vec3::new(-0.45, -0.8, -0.35),
        overlay: false,
    };
    let (w, h) = (1920u32, 1080u32);
    let texture = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("capture"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view_tex = texture.create_view(&Default::default());
    scene3d.render(&world, &view_tex, w, h);
    let rgba = renderer.read_back(&texture, w, h);
    image::save_buffer(out, &rgba, w, h, image::ColorType::Rgba8).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}

/// `--capture-model <file.xgm> <out.png> [--yaw deg] [--outline]`: renders one 2.9.x model with its textures.
fn capture_model(assets: PathBuf, file: &str, out: &str, args: &[String]) -> Result<(), String> {
    let root = find_assets291(&assets)?.parent().unwrap().join("assets292");
    let (mut renderer, mut scene3d, _game) = headless(&assets, 0.0)?;
    let mut textures = xmodel::TextureIndex::new(&root);
    let model = xmodel::XModel::load(&mut scene3d, &mut textures, std::path::Path::new(file), !args.iter().any(|a| a == "--outline"))?;
    let (lo, hi) = (Vec3::from(model.bounds.0), Vec3::from(model.bounds.1));
    let (centre, size) = ((lo + hi) * 0.5, (hi - lo).length().max(0.1));
    let yaw = args.iter().position(|a| a == "--yaw").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<f32>().ok()).unwrap_or(35.0).to_radians();
    let eye = centre + Vec3::new(yaw.sin(), 0.35, yaw.cos()) * size * 1.1;
    let view = glam::Mat4::look_at_rh(eye, centre, Vec3::Y);
    let proj = glam::Mat4::perspective_rh(45f32.to_radians(), 16.0 / 9.0, size * 0.02, size * 10.0);
    let mut draws = Vec::new();
    model.draws(&mut draws, glam::Mat4::IDENTITY, [1.0; 4]);
    let world = render3d::World { camera: render3d::Camera { view_proj: proj * view, eye }, draws, sky: [0.75, 0.82, 0.92], fog_density: 0.0, sun_dir: Vec3::new(-0.45, -0.8, -0.35), overlay: false };
    let (w, h) = (1280u32, 720u32);
    let texture = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("capture"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view_tex = texture.create_view(&Default::default());
    scene3d.render(&world, &view_tex, w, h);
    let rgba = renderer.read_back(&texture, w, h);
    image::save_buffer(out, &rgba, w, h, image::ColorType::Rgba8).map_err(|e| format!("{out}: {e}"))?;
    println!("{} parts, bounds {:?}..{:?}; wrote {out}", model.parts.len(), lo, hi);
    Ok(())
}

/// `--capture-kart <folder> <out.png> [--yaw deg]`: a complete textured kart with driver.
fn capture_kart(assets: PathBuf, folder: &str, out: &str, args: &[String]) -> Result<(), String> {
    let root = find_assets291(&assets)?.parent().unwrap().join("assets292");
    let (mut renderer, mut scene3d, _game) = headless(&assets, 0.0)?;
    let mut textures = xmodel::TextureIndex::new(&root);
    let kart = fullkart::FullKart::load(&mut scene3d, &mut textures, &root, folder, 2)?;
    let yaw = args.iter().position(|a| a == "--yaw").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<f32>().ok()).unwrap_or(35.0).to_radians();
    let centre = Vec3::new(0.0, 0.5, 0.0);
    let eye = centre + Vec3::new(yaw.sin(), 0.3, yaw.cos()) * 5.0;
    let view = glam::Mat4::look_at_rh(eye, centre, Vec3::Y);
    let proj = glam::Mat4::perspective_rh(40f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
    let mut draws = Vec::new();
    kart.draws(&mut draws, glam::Mat4::IDENTITY, 0.0, 0.0);
    let world = render3d::World { camera: render3d::Camera { view_proj: proj * view, eye }, draws, sky: [0.75, 0.82, 0.92], fog_density: 0.0, sun_dir: Vec3::new(-0.45, -0.8, -0.35), overlay: false };
    let (w, h) = (1280u32, 720u32);
    let texture = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("capture"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view_tex = texture.create_view(&Default::default());
    scene3d.render(&world, &view_tex, w, h);
    let rgba = renderer.read_back(&texture, w, h);
    image::save_buffer(out, &rgba, w, h, image::ColorType::Rgba8).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}

/// `--capture-event <campaign index> <seconds> <out.png> [--autopilot] [--to-results]`: runs a real event headlessly (rules, AI karts,
/// items, mode, results) for `seconds` of simulated time and saves the HUD frame. Never spends energy or writes the save file.
fn capture_event(assets: PathBuf, index: usize, seconds: f32, out: &str, args: &[String]) -> Result<(), String> {
    let (mut renderer, mut scene3d, mut game) = headless(&assets, 0.0)?;
    game.capture_mode = true;
    game.autopilot = args.iter().any(|a| a == "--autopilot");
    game.stop_at_results = args.iter().any(|a| a == "--to-results");
    for (i, a) in args.iter().enumerate() {
        if a == "--event-file" {
            game.event_file_override = args.get(i + 1).cloned();
        }
        if a == "--powerup" {
            match args.get(i + 1).map(|s| s.as_str()) {
                Some("speedbooster") => game.test_powerups.push(powerups::EPowerup::SpeedBooster),
                Some("kingsling") => game.test_powerups.push(powerups::EPowerup::KingSling),
                Some("autorepair") => game.test_powerups.push(powerups::EPowerup::AutoRepair),
                other => return Err(format!("--powerup speedbooster|kingsling|autorepair, got {other:?}")),
            }
        }
        if a == "--ability-at" {
            game.test_ability_at = args.get(i + 1).and_then(|s| s.parse().ok());
        }
    }
    if args.iter().any(|a| a == "--use-flow") {
        // the real flow: its profile (next to THIS exe, not the player's own install), energy is spent and the save is written
        let root = find_assets291(&assets)?;
        game.set_flow(flow::UiFlow::new(&mut renderer, &root)?);
        game.capture_mode = false;
        println!("before: {}", game.profile_summary(index));
    }
    let started = Instant::now();
    game.start_event(&mut scene3d, index, 0.0)?;
    if args.iter().any(|a| a == "--use-flow") {
        println!("after start: {}", game.profile_summary(index));
    }
    let loaded = started.elapsed().as_secs_f32();
    game.fast_forward_event(seconds, Keys::default());
    if let Some(i) = args.iter().position(|a| a == "--cam") {
        let v: Vec<f32> = args.get(i + 1).map(|s| s.split(',').filter_map(|p| p.parse().ok()).collect()).unwrap_or_default();
        if v.len() == 3 {
            game.set_event_camera(Some(Vec3::new(v[0], v[1], v[2])));
        }
    }
    if let Some(run) = game.event_ref() {
        println!("loaded in {loaded:.1}s, simulated {seconds:.0}s in {:.1}s total; stage {:?}, rules phase {:?}, race clock {:.1}s", started.elapsed().as_secs_f32(), run.stage, run.rs.phase, run.rs.clock_s);
        print!("{}", run.standings_text());
        println!("mode object: {}", run.mode_state_text());
        println!("coins {} / fruit {}", run.coins, run.items.fruit_count);
        for l in run.log.iter().rev().take(80).collect::<Vec<_>>().into_iter().rev() {
            println!("log: {l}");
        }
        if let Some(s) = &run.summary {
            println!("summary: {s:?}");
        }
    }
    if args.iter().any(|a| a == "--use-flow") {
        println!("after results: {}", game.profile_summary(index));
    }
    save_frame(&mut renderer, &mut scene3d, &game, seconds, out)
}
