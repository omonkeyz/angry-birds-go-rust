//! Interpreter for the 2.9.1 screen layouts (`xml/ui/*.xml`, decoded by abgtool).
//!
//! Units follow the game's UI system: `pt` = 1/100 of the screen height, `%` = percent of the parent window,
//! positions name a pivot point inside the parent, `Include block=` expands a parameterised block from `uiblocks.xml`
//! (`_name` in an attribute value is replaced by the parameter), `style=` supplies default attributes.
use crate::gfx::{Draw, SpriteRenderer, TextureId};
use crate::loc::Loc;
use crate::sdf::SdfFont;
use crate::ui::{solid, tinted, Rect, Tex, STAGE_H, STAGE_W};
use abgtool::atlas;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub type Attrs = Vec<(String, String)>;

#[derive(Clone, Debug)]
pub struct Node {
    pub tag: String,
    pub attrs: Attrs,
    pub children: Vec<Node>,
}

impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// `CWindow_Container` -> `Container`: the instance name after the class prefix.
    pub fn name(&self) -> &str {
        self.tag.split_once('_').map(|(_, n)| n).unwrap_or("")
    }

    pub fn id(&self) -> &str {
        self.attr("id").unwrap_or_else(|| self.name())
    }
}

fn convert(n: roxmltree::Node) -> Node {
    Node {
        tag: n.tag_name().name().to_string(),
        attrs: n.attributes().map(|a| (a.name().to_ascii_lowercase(), a.value().to_string())).collect(),
        children: n.children().filter(|c| c.is_element()).map(convert).collect(),
    }
}

fn parse_file(path: &Path) -> Result<Node, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(convert(doc.root_element()))
}

struct Block {
    params: Vec<String>,
    children: Vec<Node>,
}

/// Styles and blocks shared by every screen.
struct Defs {
    styles: HashMap<String, Attrs>,
    blocks: HashMap<String, Block>,
}

impl Defs {
    fn load(ui_dir: &Path) -> Result<Defs, String> {
        let mut defs = Defs { styles: HashMap::new(), blocks: HashMap::new() };
        for style in parse_file(&ui_dir.join("uistyles.xml"))?.children {
            if let Some(name) = style.attr("name") {
                defs.styles.insert(name.to_ascii_lowercase(), style.attrs.iter().filter(|(k, _)| k != "name").cloned().collect());
            }
        }
        for block in parse_file(&ui_dir.join("uiblocks.xml"))?.children {
            if let Some(name) = block.attr("name") {
                let params = block.attr("parameters").map(|p| p.split(',').map(|s| s.trim().to_ascii_lowercase()).collect()).unwrap_or_default();
                defs.blocks.insert(name.to_ascii_lowercase(), Block { params, children: block.children });
            }
        }
        Ok(defs)
    }

    fn expand(&self, node: &Node, params: &HashMap<String, String>, out: &mut Vec<Node>, depth: u32) {
        if depth > 16 {
            return;
        }
        let sub = |v: &str| -> String {
            match v.strip_prefix('_').and_then(|p| params.get(&p.to_ascii_lowercase())) {
                Some(value) => value.clone(),
                None => v.to_string(),
            }
        };
        if node.tag == "Include" {
            let Some(block) = node.attr("block").and_then(|b| self.blocks.get(&b.to_ascii_lowercase())) else { return };
            let mut inner: HashMap<String, String> = HashMap::new();
            for p in &node.children {
                if p.tag == "Parameter" {
                    if let (Some(id), Some(value)) = (p.attr("id"), p.attr("value")) {
                        inner.insert(id.to_ascii_lowercase(), sub(value));
                    }
                }
            }
            for name in &block.params {
                inner.entry(name.clone()).or_default();
            }
            for child in &block.children {
                self.expand(child, &inner, out, depth + 1);
            }
            return;
        }
        let mut copy = Node { tag: node.tag.clone(), attrs: node.attrs.iter().map(|(k, v)| (k.clone(), sub(v))).collect(), children: Vec::new() };
        for child in &node.children {
            self.expand(child, params, &mut copy.children, depth + 1);
        }
        out.push(copy);
    }
}

/// Everything loaded onto the GPU that screens refer to by name.
pub struct Resources {
    atlas_tex: Vec<Tex>,
    /// lower-case sprite path (`uishared/abk_gem.png`) -> (atlas texture index, pixel rect)
    sprites: HashMap<String, (usize, Rect)>,
    files: HashMap<String, Tex>,
    fonts: Vec<SdfFont>,
    white: TextureId,
    pub loc: Loc,
}

pub struct Hit {
    pub rect: Rect,
    pub action: String,
    pub sound: Option<String>,
    pub id: String,
}

pub struct Screen {
    pub root: Node,
}

pub struct UiSystem {
    defs: Defs,
    res: Resources,
    root_dir: PathBuf,
    screens: HashMap<String, Screen>,
}

impl UiSystem {
    pub fn new(renderer: &mut SpriteRenderer, root: &Path) -> Result<UiSystem, String> {
        let defs = Defs::load(&root.join("xml/xml/ui"))?;
        let mut res = Resources {
            atlas_tex: Vec::new(),
            sprites: HashMap::new(),
            files: HashMap::new(),
            fonts: Vec::new(),
            white: renderer.white_pixel(),
            loc: Loc::load(&root.join("loc/locdb.xlc"))?,
        };
        for font in ["bold_ab_sdf_32", "regular_ab_sdf_32", "plain_ab_sdf_32", "opensans_regular_sdf_32"] {
            res.fonts.push(SdfFont::load(renderer, root, font)?);
        }
        for group in ["ui_core", "ui_tournaments"] {
            let dir = root.join(format!("pak/ui/{group}/textures"));
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            for item in read.flatten() {
                let path = item.path();
                if path.extension().and_then(|e| e.to_str()) != Some("atlas") {
                    continue;
                }
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                let png = root.join(format!("textures/ui/{group}/textures/{stem}_1_of_1.png"));
                let png = if png.is_file() { png } else { root.join(format!("textures/ui/{group}/textures/{stem}_1_of_2.png")) };
                let Ok((id, w, h)) = renderer.load_png(&png) else { continue };
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let parsed = atlas::parse(&bytes, w, h)?;
                let index = res.atlas_tex.len();
                res.atlas_tex.push(Tex { id, w: w as f32, h: h as f32 });
                for s in parsed.sprites {
                    res.sprites.insert(s.name.to_ascii_lowercase(), (index, [s.x as f32, s.y as f32, s.w as f32, s.h as f32]));
                }
            }
        }
        Ok(UiSystem { defs, res, root_dir: root.to_path_buf(), screens: HashMap::new() })
    }

    /// Loads `xml/ui/<name>.xml` (e.g. `uilandingscreen`), expanding blocks and preloading its stand-alone textures.
    pub fn load_screen(&mut self, renderer: &mut SpriteRenderer, name: &str) -> Result<(), String> {
        let raw = parse_file(&self.root_dir.join(format!("xml/xml/ui/{name}.xml")))?;
        let mut out = Vec::new();
        self.defs.expand(&raw, &HashMap::new(), &mut out, 0);
        let root = out.pop().ok_or("empty screen")?;
        let mut files = HashSet::new();
        collect_files(&root, &mut files);
        for file in files {
            self.res.load_file(renderer, &self.root_dir, &file);
        }
        self.screens.insert(name.to_string(), Screen { root });
        Ok(())
    }

    pub fn has_screen(&self, name: &str) -> bool {
        self.screens.contains_key(name)
    }

    /// Draws a loaded screen over the whole stage. `show` names windows (by id or instance name) that the game code
    /// makes visible although the layout starts them hidden.
    pub fn draw(&self, name: &str, show: &[&str], texts: &HashMap<String, String>, out: &mut Vec<Draw>, hits: &mut Vec<Hit>) -> Option<usize> {
        let Some(screen) = self.screens.get(name) else { return None };
        let mut cx = Ctx { defs: &self.defs, res: &self.res, unit: STAGE_H / 100.0, show, texts, out, hits, alpha: 1.0, split: None };
        cx.node(&screen.root, [0.0, 0.0, STAGE_W, STAGE_H], true);
        cx.split
    }
}

fn collect_files(n: &Node, into: &mut HashSet<String>) {
    for (k, v) in &n.attrs {
        if k.starts_with("texture") || k == "paneltexture" || k == "innertexture" {
            if let Some(path) = v.strip_prefix("file:") {
                into.insert(path.to_string());
            }
        }
    }
    for c in &n.children {
        collect_files(c, into);
    }
}

impl Resources {
    fn load_file(&mut self, renderer: &mut SpriteRenderer, root: &Path, spec: &str) {
        // `UIPAK:Textures/NonAtlased/X.png` -> textures/ui/ui_core/nonatlased/x.png
        let rel = spec.split_once(':').map(|(_, r)| r).unwrap_or(spec).to_ascii_lowercase();
        let rel = rel.strip_prefix("textures/").unwrap_or(&rel).to_string();
        let path = root.join("textures/ui/ui_core").join(&rel);
        if let Ok((id, w, h)) = renderer.load_png(&path) {
            self.files.insert(spec.to_ascii_lowercase(), Tex { id, w: w as f32, h: h as f32 });
        } else {
            eprintln!("ui: missing texture file {}", path.display());
        }
    }

    /// Resolves `shared:UIShared/ABK_Gem.png`, `UITopBar/Red.png` or `file:UIPAK:...` to (texture, source rect).
    fn texture(&self, spec: &str) -> Option<(Tex, Rect)> {
        if let Some(path) = spec.strip_prefix("file:") {
            let t = self.files.get(&path.to_ascii_lowercase())?;
            return Some((Tex { id: t.id, w: t.w, h: t.h }, [0.0, 0.0, t.w, t.h]));
        }
        let name = spec.split_once(':').map(|(_, n)| n).unwrap_or(spec).to_ascii_lowercase();
        let &(index, rect) = self.sprites.get(&name)?;
        let t = &self.atlas_tex[index];
        Some((Tex { id: t.id, w: t.w, h: t.h }, rect))
    }
}

struct Ctx<'a> {
    defs: &'a Defs,
    res: &'a Resources,
    unit: f32,
    show: &'a [&'a str],
    texts: &'a HashMap<String, String>,
    out: &'a mut Vec<Draw>,
    hits: &'a mut Vec<Hit>,
    alpha: f32,
    /// Index in `out` where the first 3D render-callback window sits.
    split: Option<usize>,
}

/// Attribute lookup: the window's own value first, then its style's.
fn get<'n>(defs: &'n Defs, n: &'n Node, key: &str) -> Option<&'n str> {
    n.attr(key).or_else(|| {
        let style = n.attr("style")?;
        defs.styles.get(&style.to_ascii_lowercase())?.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    })
}

/// `0xRRGGBBAA`; a bare `0` is fully transparent.
fn parse_colour(v: &str) -> Option<[f32; 4]> {
    let v = v.trim();
    let n = if let Some(hex) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        v.parse::<u32>().ok()?
    };
    Some([(n >> 24) as f32 / 255.0, ((n >> 16) & 255) as f32 / 255.0, ((n >> 8) & 255) as f32 / 255.0, (n & 255) as f32 / 255.0])
}

fn parse_bool(v: Option<&str>, default: bool) -> bool {
    match v {
        Some(s) => s.eq_ignore_ascii_case("true") || s == "1",
        None => default,
    }
}

impl<'a> Ctx<'a> {
    /// A length with its unit, against `parent` (the parent's size on the axis the value belongs to).
    fn dim(&self, v: &str, parent: f32) -> f32 {
        let v = v.trim();
        let number = |s: &str| s.trim().parse::<f32>().unwrap_or(0.0);
        if let Some(p) = v.strip_suffix("%t") {
            number(p) / 100.0 * parent
        } else if let Some(p) = v.strip_suffix('%') {
            number(p) / 100.0 * parent
        } else if let Some(p) = v.strip_suffix("pt") {
            number(p) * self.unit
        } else if let Some(p) = v.strip_suffix("px") {
            number(p) * self.unit / 10.0
        } else {
            number(v) * self.unit
        }
    }

    fn texture_of(&self, n: &Node) -> Option<(Tex, Rect)> {
        let spec = get(self.defs, n, "texture").or_else(|| get(self.defs, n, "texture0"))?;
        self.res.texture(spec)
    }

    fn node(&mut self, n: &Node, parent: Rect, is_root: bool) {
        let tag = n.tag.as_str();
        if tag.starts_with("CBehaviour") || tag == "CRenderOrder" || tag == "Parameter" {
            return;
        }
        let forced = self.show.iter().any(|s| s.eq_ignore_ascii_case(n.id()) || s.eq_ignore_ascii_case(n.name()));
        let hidden = get(self.defs, n, "visibility").map(|v| v.eq_ignore_ascii_case("hidden") || v.eq_ignore_ascii_case("hidetree")).unwrap_or(false);
        let own_alpha: f32 = get(self.defs, n, "alpha").and_then(|v| v.parse().ok()).unwrap_or(1.0);
        if own_alpha <= 0.0 && !forced {
            return;
        }
        if hidden && !forced && !is_root {
            return;
        }
        let saved_alpha = self.alpha;
        self.alpha *= own_alpha;
        let tex = self.texture_of(n);
        let aspect = tex.as_ref().map(|(_, r)| r[2] / r[3].max(1.0));
        let width = get(self.defs, n, "width").map(|v| self.dim(v, parent[2]));
        let height = get(self.defs, n, "height").map(|v| self.dim(v, parent[3]));
        let (w, h) = match (width, height, aspect) {
            (Some(w), Some(h), _) => (w, h),
            (Some(w), None, Some(a)) => (w, w / a),
            (None, Some(h), Some(a)) => (h * a, h),
            (Some(w), None, None) => (w, parent[3]),
            (None, Some(h), None) => (parent[2], h),
            (None, None, _) => (parent[2], parent[3]),
        };
        let pivot = get(self.defs, n, "pivot").unwrap_or("topleft").to_ascii_lowercase();
        let centred = pivot.contains("centre") || pivot.contains("center");
        let px = if pivot.contains("left") { 0.0 } else if pivot.contains("right") { 1.0 } else if centred || pivot == "top" || pivot == "bottom" { 0.5 } else { 0.0 };
        let py = if pivot.contains("top") { 0.0 } else if pivot.contains("bottom") { 1.0 } else if centred || pivot == "left" || pivot == "right" { 0.5 } else { 0.0 };
        let x = get(self.defs, n, "xpos").map(|v| self.dim(v, parent[2])).unwrap_or(0.0);
        let y = get(self.defs, n, "ypos").map(|v| self.dim(v, parent[3])).unwrap_or(0.0);
        let rect = if is_root { parent } else { [parent[0] + x - px * w, parent[1] + y - py * h, w, h] };

        let colour = get(self.defs, n, "colour").and_then(parse_colour);
        if tag.starts_with("CTextLabel") {
            self.text(n, rect);
        } else if tag.starts_with("CPanelWindow") {
            {
                let mut c = colour.unwrap_or([1.0; 4]);
                c[3] *= self.alpha;
                self.panel(n, rect, c)
            };
        } else if let Some((t, src)) = tex {
            let c = colour.unwrap_or([1.0; 4]);
            let a = c[3] * self.alpha;
            let mut d = tinted(&t, src, rect, [c[0] * a, c[1] * a, c[2] * a, a]);
            if parse_bool(get(self.defs, n, "hflip"), false) {
                d.inst.uv.swap(0, 2);
            }
            if parse_bool(get(self.defs, n, "vflip"), false) {
                d.inst.uv.swap(1, 3);
            }
            self.out.push(d);
        } else if tag.starts_with("CRenderCallbackWindow") {
            // filled by game code (the 3D kart view): everything drawn after this point goes over it
            self.split.get_or_insert(self.out.len());
        } else if let Some(c) = colour {
            // a CWindow with only a colour is a flat rectangle; the 1/255-alpha ones are invisible sort anchors
            if c[3] > 0.01 && width.is_some() && height.is_some() {
                self.out.push(solid(self.res.white, rect, [c[0], c[1], c[2]], c[3] * self.alpha));
            }
        }

        // click targets declared by a CBehaviourTouchInput child, with the click sound of a CBehaviourSound child
        for child in &n.children {
            if child.tag == "CBehaviourTouchInput" {
                let action = child.attr("onclickedglobalstate").or_else(|| child.attr("ontappedglobalstate"));
                if let Some(action) = action {
                    let sound = n
                        .children
                        .iter()
                        .find(|c| c.tag == "CBehaviourSound")
                        .and_then(|c| c.children.first())
                        .and_then(|i| i.attr("onpressedstatesound"))
                        .map(|s| s.to_string());
                    self.hits.push(Hit { rect, action: action.to_string(), sound, id: n.id().to_string() });
                }
            }
        }
        for child in &n.children {
            self.node(child, rect, false);
        }
        self.alpha = saved_alpha;
    }

    fn panel(&mut self, n: &Node, rect: Rect, c: [f32; 4]) {
        let Some(spec) = get(self.defs, n, "paneltexture") else { return };
        let (prefix, base) = spec.split_once(':').map(|(a, b)| (format!("{a}:"), b)).unwrap_or((String::new(), spec));
        let piece = |suffix: &str| self.res.texture(&format!("{prefix}{base}{suffix}.png"));
        let tint = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
        let bw = get(self.defs, n, "panel_borderwidth").map(|v| self.dim(v, rect[2]));
        let bh = get(self.defs, n, "panel_borderheight").map(|v| self.dim(v, rect[3]));
        if let (Some(tl), Some(t), Some(tr), Some(l), Some(m), Some(r), Some(bl), Some(b), Some(br)) =
            (piece("TL"), piece("T"), piece("TR"), piece("L"), piece("M"), piece("R"), piece("BL"), piece("B"), piece("BR"))
        {
            let bw = bw.unwrap_or(rect[3] * 0.2).min(rect[2] / 2.0);
            let bh = bh.unwrap_or(bw).min(rect[3] / 2.0);
            let [x, y, w, h] = rect;
            let (ix, iy, iw, ih) = (x + bw, y + bh, (w - 2.0 * bw).max(0.0), (h - 2.0 * bh).max(0.0));
            let parts = [
                (&tl, [x, y, bw, bh]),
                (&t, [ix, y, iw, bh]),
                (&tr, [ix + iw, y, bw, bh]),
                (&l, [x, iy, bw, ih]),
                (&m, [ix, iy, iw, ih]),
                (&r, [ix + iw, iy, bw, ih]),
                (&bl, [x, iy + ih, bw, bh]),
                (&b, [ix, iy + ih, iw, bh]),
                (&br, [ix + iw, iy + ih, bw, bh]),
            ];
            for (p, dst) in parts {
                self.out.push(tinted(&p.0, p.1, dst, tint));
            }
        } else if let (Some(l), Some(m), Some(r)) = (piece("L"), piece("M"), piece("R")) {
            // horizontal three-slice (the pill buttons): the caps keep their aspect at the full height
            let [x, y, w, h] = rect;
            let cap = |p: &(Tex, Rect)| (h * p.1[2] / p.1[3].max(1.0)).min(w / 2.0);
            let (lw, rw) = (cap(&l), cap(&r));
            self.out.push(tinted(&l.0, l.1, [x, y, lw, h], tint));
            self.out.push(tinted(&m.0, m.1, [x + lw, y, (w - lw - rw).max(0.0), h], tint));
            self.out.push(tinted(&r.0, r.1, [x + w - rw, y, rw, h], tint));
        }
    }

    fn text(&mut self, n: &Node, rect: Rect) {
        let over = self.texts.get(n.id());
        let Some(raw) = over.map(|s| s.as_str()).or_else(|| get(self.defs, n, "text")) else { return };
        let localise = over.is_none() && parse_bool(get(self.defs, n, "localise"), true);
        let mut s = if localise { self.res.loc.get(raw).to_string() } else { raw.to_string() };
        if parse_bool(get(self.defs, n, "forceuppercase"), true) {
            s = s.to_uppercase();
        }
        let font_id = get(self.defs, n, "fontid").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(self.res.fonts.len() - 1);
        let font = &self.res.fonts[font_id];
        let mut em = get(self.defs, n, "fontsize").map(|v| self.dim(v, rect[3])).unwrap_or(rect[3]);
        let mut colour = get(self.defs, n, "textcolour").and_then(parse_colour).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        colour[3] *= self.alpha;
        let wrap = get(self.defs, n, "wrapmode").map(|v| v.eq_ignore_ascii_case("wrap")).unwrap_or(false);
        let align = get(self.defs, n, "alignment").unwrap_or("left").to_ascii_lowercase();
        // lines: wrapped at the window width when asked, otherwise one line shrunk to fit
        let mut lines: Vec<String> = Vec::new();
        if wrap {
            let mut line = String::new();
            for word in s.split_whitespace() {
                let trial = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if font.width(&trial, em) > rect[2] && !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    line = word.to_string();
                } else {
                    line = trial;
                }
            }
            lines.push(line);
        } else {
            let width = font.width(&s, em);
            if width > rect[2] && width > 0.0 {
                em *= rect[2] / width;
            }
            lines.push(s);
        }
        let line_h = font.line_height / font.size * em;
        let total = line_h * lines.len() as f32;
        let mut y = rect[1] + (rect[3] - total) / 2.0;
        for line in lines {
            let w = font.width(&line, em);
            let x = match align.as_str() {
                "centre" | "center" => rect[0] + (rect[2] - w) / 2.0,
                "right" => rect[0] + rect[2] - w,
                _ => rect[0],
            };
            font.draw(&line, x, y, em, colour, self.out);
            y += line_h;
        }
    }
}

impl UiSystem {
    /// One atlas sprite (`UIMapScreen/ABK_Map_Race.png`) stretched into `rect`.
    pub fn sprite(&self, name: &str, rect: Rect, alpha: f32) -> Option<Draw> {
        let (t, src) = self.res.texture(name)?;
        Some(tinted(&t, src, rect, [alpha, alpha, alpha, alpha]))
    }

    pub fn string(&self, key: &str) -> String {
        self.res.loc.get(key).to_string()
    }

    /// The height of one `pt` on the stage.
    pub fn unit(&self) -> f32 {
        STAGE_H / 100.0
    }
}
