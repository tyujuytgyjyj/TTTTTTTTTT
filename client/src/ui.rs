//! CrashNG.drive Front End (Task 7):
//! - Matches `menu_reference` (`image_e59174.jpg`) layout:
//!   * Top slanted parallelogram plate with orange end-bars & 3-node beam icon + `CrashNG.drive`
//!   * Row 1: 3 wide visual cards (`Quick-Start / Online`, `Freeroam`, `Garage`) with live 3D car thumbnails & `New!` badges
//!   * Row 2: 4 square visual cards (`Career`, `Photo Mode 4K`, `Servers`, `More...`) + Right column of 4 stacked buttons (`Options`, `Repository`, `Credits`, `Quit`)
//!   * Bottom-left slanted version pill (`Alpha v0.4.0`) + Bottom-right slanted navigation bar (Mouse, Keyboard & Gamepad)
//! - Includes dedicated pages: Play (Freeroam), Servers (Multiplayer), Settings (Graphics Low/Med/High, Dynamic Resolution, FPS Cap, Audio), Garage (Paint, Tire PSI, Bus Passengers), and Quit.

use crate::car::{Car, Drive, HiKind, SPECS};
use crate::net::{start_net_client, NetStatus, SharedNet};
use crate::servers::*;
use crate::{rrect, txt, txt_w, CarKind, Menu, Res};
use macroquad::audio::{load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
use macroquad::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const ORANGE: Color = Color::new(0.98, 0.48, 0.08, 1.0);
const ORANGE_HI: Color = Color::new(1.0, 0.62, 0.22, 1.0);
const DARK: Color = Color::new(0.10, 0.11, 0.13, 1.0);
const PANEL_BG: Color = Color::new(0.05, 0.06, 0.08, 0.90);
const TILE_BG: Color = Color::new(0.14, 0.16, 0.19, 0.82);
const TILE_HI: Color = Color::new(0.22, 0.25, 0.30, 0.92);
const BLUE: Color = Color::new(0.12, 0.48, 0.92, 1.0);
const GREEN: Color = Color::new(0.30, 0.82, 0.40, 1.0);
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct Music {
    snd: Option<Sound>,
    playing: bool,
    /// مدة المقطع بالثواني (OGG فقط) — نعيد التشغيل يدوياً قبل النهاية لأن quad-snd 0.2.8
    /// يقرأ خارج حدود المصفوفة (index out of bounds) عند الوصول لآخر عينة.
    dur: Option<f64>,
    started: Option<Instant>,
}

/// يحسب مدة ملف OGG Vorbis من معدل العينة وآخر granule position.
fn ogg_duration_secs(b: &[u8]) -> Option<f64> {
    let i = b.windows(7).position(|w| w == b"\x01vorbis")?;
    let rate = u32::from_le_bytes(b.get(i + 12..i + 16)?.try_into().ok()?) as f64;
    let j = b.windows(4).rposition(|w| w == b"OggS")?;
    let g = i64::from_le_bytes(b.get(j + 6..j + 14)?.try_into().ok()?);
    if rate > 0.0 && g > 0 { Some(g as f64 / rate) } else { None }
}

impl Music {
    /// quad-snd يدعم OGG (Vorbis) و WAV (PCM) فقط. تمرير MP3 يسبب panic داخل خيط الصوت
    /// (UnsupportedFormat) ولا يمكن التقاطه بـ .ok()، لذلك نتحقق من الصيغة قبل التحميل.
    fn is_supported_audio(b: &[u8]) -> bool {
        b.len() > 12 && (b.starts_with(b"OggS") || (b.starts_with(b"RIFF") && &b[8..12] == b"WAVE"))
    }

    pub async fn load() -> Music {
        let mut ext_bytes: Option<Vec<u8>> = None;
        for path in [
            "assets/music/main_menu_theme.ogg",
            "client/assets/music/main_menu_theme.ogg",
            "mods/main_menu_theme.ogg",
            "mods/main_menu_theme.wav",
        ] {
            if let Ok(b) = std::fs::read(path) {
                if Self::is_supported_audio(&b) {
                    ext_bytes = Some(b);
                    break;
                }
            }
        }
        if ext_bytes.is_none() {
            for dir in ["assets/music", "client/assets/music", "mods"] {
                if let Ok(rd) = std::fs::read_dir(dir) {
                    for e in rd.flatten() {
                        let p = e.path();
                        let nm = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                        if (nm.ends_with(".ogg") || nm.ends_with(".wav")) && nm != "menu_theme.ogg" {
                            if let Ok(b) = std::fs::read(&p) {
                                if Self::is_supported_audio(&b) {
                                    ext_bytes = Some(b);
                                    break;
                                }
                            }
                        }
                    }
                }
                if ext_bytes.is_some() { break; }
            }
        }
        let mut dur = None;
        let mut snd = None;
        if let Some(b) = ext_bytes {
            dur = ogg_duration_secs(&b);
            snd = load_sound_from_bytes(&b).await.ok();
        }
        if snd.is_none() {
            let b: &[u8] = include_bytes!("../assets/music/menu_theme.ogg");
            dur = ogg_duration_secs(b);
            snd = load_sound_from_bytes(b).await.ok();
        }
        Music { snd, playing: false, dur, started: None }
    }

    pub fn sync(&mut self, want: bool, st: &Settings) {
        let want = want && st.music_on && st.music_vol > 0.001;
        if let Some(s) = &self.snd {
            let looped = self.dur.is_none();
            if want && !self.playing {
                play_sound(s, PlaySoundParams { looped, volume: st.music_vol });
                self.playing = true;
                self.started = Some(Instant::now());
            } else if !want && self.playing {
                stop_sound(s);
                self.playing = false;
                self.started = None;
            } else if want {
                set_sound_volume(s, st.music_vol);
                if let (Some(d), Some(t0)) = (self.dur, self.started) {
                    if t0.elapsed().as_secs_f64() > (d - 3.0).max(1.0) {
                        stop_sound(s);
                        play_sound(s, PlaySoundParams { looped: false, volume: st.music_vol });
                        self.started = Some(Instant::now());
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Screen { Main, Freeroam, Multiplayer, Options, Garage, Credits, More, Mods }

pub enum FrontAction { None, Quit, Start, StartPhotoMode, Connected(Arc<SharedNet>) }


enum Dialog {
    Server { edit: Option<usize>, name: String, addr: String, field: usize, err: String },
    Direct { addr: String, err: String },
    Delete(usize),
    Username { buf: String, err: String },
    HeavyMapWarning { map_idx: usize, map_title: String },
}

struct Join { net: Arc<SharedNet>, started: Instant, label: String }

pub struct Front {
    pub screen: Screen,
    pub settings: Settings,
    pub branding: BrandingConfig,
    pub sel: usize,
    pub hover_anim: [f32; 11],
    servers: Vec<ServerEntry>,
    status: Vec<SharedStatus>,
    msel: usize,
    mfirst: usize,
    dialog: Option<Dialog>,
    join: Option<Join>,
    toast: Option<(String, f32, bool)>,
    last_click: (usize, f64),
    fade: f32,
    bs_hold: f32,
    drag_vol: bool,
    drag_res: bool,
    // Garage customization state
    pub custom_paint_idx: usize,
    pub custom_tire_psi: f32,
    pub custom_bus_passengers: u32,
    // Vehicle browser
    search: String,
    search_focus: bool,
    f_builtin: bool,
    f_mods: bool,
    f_rwd: bool,
    f_fwd: bool,
    f_awd: bool,
    f_fav_only: bool,
    more_filters: bool,
    rtab: usize,
    vscroll: f32,
    vfollow: bool,
    thumbs: Vec<Option<RenderTarget>>,
}

pub const GARAGE_PAINTS: [(&str, [f32; 3]); 8] = [
    ("AMG High-Tech Silver", [0.80, 0.83, 0.87]),
    ("AMG Magma Beam Orange", [0.98, 0.42, 0.06]),
    ("Solarbeam Metallic Yellow", [0.96, 0.76, 0.08]),
    ("Petronas Emerald Green", [0.05, 0.68, 0.56]),
    ("Obsidian Metallic Black", [0.08, 0.09, 0.11]),
    ("Rosso Corsa Crimson", [0.88, 0.12, 0.12]),
    ("Bayside Deep Blue", [0.12, 0.34, 0.86]),
    ("Arctic Pearl White", [0.93, 0.95, 0.97]),
];

#[derive(Clone, Copy, PartialEq)]
enum Btn { Normal, Primary, Danger }

#[derive(Clone, Copy)]
enum Icon { Key, Road, SplitCars, GarageHouse, Controller, Stopwatch, Folder }

struct Cx<'a> {
    res: &'a Res,
    u: f32,
    ox: f32,
    oy: f32,
    mx: f32,
    my: f32,
    click: bool,
    live: bool,
}

impl<'a> Cx<'a> {
    fn new(res: &'a Res, live: bool) -> Self {
        let (sw, sh) = (screen_width(), screen_height());
        let u = (sh / 720.0).min(sw / 1280.0);
        let (mx, my) = mouse_position();
        Cx { res, u, ox: (sw - 1280.0 * u) / 2.0, oy: (sh - 720.0 * u) / 2.0, mx, my, click: is_mouse_button_pressed(MouseButton::Left), live }
    }
    fn x(&self, v: f32) -> f32 { self.ox + v * self.u }
    fn y(&self, v: f32) -> f32 { self.oy + v * self.u }
    fn s(&self, v: f32) -> f32 { v * self.u }
    fn fs(&self, v: f32) -> u16 { (v * self.u).round().max(8.0) as u16 }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
        if r <= 0.5 { draw_rectangle(self.x(x), self.y(y), self.s(w), self.s(h), c); } else { rrect(self.x(x), self.y(y), self.s(w), self.s(h), self.s(r), c); }
    }
    fn outline(&self, x: f32, y: f32, w: f32, h: f32, th: f32, c: Color) {
        draw_rectangle_lines(self.x(x), self.y(y), self.s(w), self.s(h), self.s(th).max(1.0), c);
    }
    fn text(&self, bold: bool, s: &str, x: f32, y: f32, size: f32, c: Color) {
        txt(if bold { &self.res.font_b } else { &self.res.font }, s, self.x(x), self.y(y), self.fs(size), c);
    }
    fn width(&self, bold: bool, s: &str, size: f32) -> f32 {
        txt_w(if bold { &self.res.font_b } else { &self.res.font }, s, self.fs(size)) / self.u
    }
    fn text_c(&self, bold: bool, s: &str, cx: f32, y: f32, size: f32, c: Color) {
        let w = self.width(bold, s, size);
        self.text(bold, s, cx - w / 2.0, y, size, c);
    }
    fn text_r(&self, bold: bool, s: &str, rx: f32, y: f32, size: f32, c: Color) {
        let w = self.width(bold, s, size);
        self.text(bold, s, rx - w, y, size, c);
    }
    fn fit(&self, bold: bool, s: &str, size: f32, maxw: f32) -> String {
        if self.width(bold, s, size) <= maxw { return s.to_string(); }
        let mut v: Vec<char> = s.chars().collect();
        while v.len() > 1 {
            v.pop();
            let t: String = v.iter().collect::<String>() + "...";
            if self.width(bold, &t, size) <= maxw { return t; }
        }
        s.to_string()
    }
    fn hit(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.mx >= self.x(x) && self.mx <= self.x(x + w) && self.my >= self.y(y) && self.my <= self.y(y + h)
    }
    fn para(&self, x: f32, y: f32, w: f32, h: f32, sk: f32, c: Color) {
        let p1 = vec2(self.x(x + sk), self.y(y));
        let p2 = vec2(self.x(x + w + sk), self.y(y));
        let p3 = vec2(self.x(x + w), self.y(y + h));
        let p4 = vec2(self.x(x), self.y(y + h));
        draw_triangle(p1, p2, p3, c);
        draw_triangle(p1, p3, p4, c);
    }
    fn circle(&self, x: f32, y: f32, r: f32, c: Color) { draw_circle(self.x(x), self.y(y), self.s(r), c); }
    fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, th: f32, c: Color) {
        draw_line(self.x(x1), self.y(y1), self.x(x2), self.y(y2), self.s(th), c);
    }
    fn button(&self, label: &str, x: f32, y: f32, w: f32, h: f32, style: Btn, enabled: bool) -> bool {
        let hover = self.live && enabled && self.hit(x, y, w, h);
        let (base, hi) = match style {
            Btn::Normal => (Color::new(0.26, 0.28, 0.32, 0.92), Color::new(0.38, 0.40, 0.45, 0.96)),
            Btn::Primary => (ORANGE, ORANGE_HI),
            Btn::Danger => (Color::new(0.74, 0.25, 0.20, 0.95), Color::new(0.90, 0.32, 0.26, 1.0)),
        };
        let mut c = if hover { hi } else { base };
        if !enabled { c.a *= 0.35; }
        self.rect(x, y, w, h, 6.0, c);
        let tc = if style == Btn::Primary { DARK } else { WHITE };
        let tc = if enabled { tc } else { Color::new(tc.r, tc.g, tc.b, 0.4) };
        self.text_c(true, label, x + w / 2.0, y + h / 2.0 + 6.0, 17.0, tc);
        hover && self.click
    }
}

fn server_color(name: &str) -> Color {
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    const P: [(f32, f32, f32); 6] = [(0.20, 0.50, 0.85), (0.85, 0.40, 0.20), (0.25, 0.70, 0.45), (0.65, 0.35, 0.80), (0.85, 0.65, 0.15), (0.80, 0.30, 0.45)];
    let c = P[(h % 6) as usize];
    Color::new(c.0, c.1, c.2, 1.0)
}

fn ctrl_down() -> bool {
    is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl) || is_key_down(KeyCode::LeftSuper) || is_key_down(KeyCode::RightSuper)
}

fn edit_text(buf: &mut String, max: usize, hold: &mut f32, dt: f32) {
    let ctrl = ctrl_down();
    while let Some(c) = get_char_pressed() {
        if !ctrl && !c.is_control() && buf.chars().count() < max { buf.push(c); }
    }
    if ctrl && is_key_pressed(KeyCode::V) {
        if let Some(p) = macroquad::miniquad::window::clipboard_get() {
            for c in p.chars() {
                if !c.is_control() && buf.chars().count() < max { buf.push(c); }
            }
        }
    }
    if is_key_pressed(KeyCode::Backspace) { buf.pop(); *hold = -0.4; }
    else if is_key_down(KeyCode::Backspace) { *hold += dt; if *hold > 0.045 { buf.pop(); *hold = 0.0; } }
    else { *hold = 0.0; }
}

fn flush_chars() { while get_char_pressed().is_some() {} }

fn input_box(cx: &Cx, label: &str, buf: &str, hint: &str, x: f32, y: f32, w: f32, focused: bool) -> bool {
    cx.text(false, label, x, y, 14.0, Color::new(1.0, 1.0, 1.0, 0.62));
    let by = y + 8.0;
    cx.rect(x, by, w, 44.0, 8.0, Color::new(0.04, 0.05, 0.07, 0.96));
    cx.outline(x, by, w, 44.0, 2.0, if focused { ORANGE } else { Color::new(1.0, 1.0, 1.0, 0.14) });
    let mut shown: String = buf.to_string();
    if shown.is_empty() {
        cx.text(false, hint, x + 14.0, by + 29.0, 18.0, Color::new(1.0, 1.0, 1.0, 0.28));
    } else {
        while cx.width(false, &shown, 19.0) > w - 36.0 && shown.chars().count() > 1 { shown.remove(0); }
        cx.text(false, &shown, x + 14.0, by + 29.0, 19.0, WHITE);
    }
    if focused && (get_time() * 2.0) as i64 % 2 == 0 {
        let cw = if buf.is_empty() { 0.0 } else { cx.width(false, &shown, 19.0) };
        cx.rect(x + 15.0 + cw, by + 10.0, 2.0, 24.0, 0.0, ORANGE);
    }
    cx.live && cx.click && cx.hit(x, by, w, 44.0)
}

fn draw_icon(cx: &Cx, icon: Icon, x: f32, y: f32, k: f32, c: Color) {
    match icon {
        Icon::Key => {
            // Key fob + signal waves (Quick-Start Experiences style)
            cx.rect(x - 0.35 * k, y - 0.30 * k, 0.42 * k, 0.62 * k, 0.12 * k, c);
            cx.circle(x - 0.14 * k, y - 0.12 * k, 0.08 * k, DARK);
            draw_circle_lines(cx.x(x + 0.14 * k), cx.y(y - 0.16 * k), cx.s(0.18 * k), cx.s(2.4), c);
            draw_circle_lines(cx.x(x + 0.14 * k), cx.y(y - 0.16 * k), cx.s(0.32 * k), cx.s(2.4), c);
        }
        Icon::Road => {
            let (t1, t2, b1, b2) = (vec2(cx.x(x - 0.13 * k), cx.y(y - 0.48 * k)), vec2(cx.x(x + 0.13 * k), cx.y(y - 0.48 * k)), vec2(cx.x(x + 0.50 * k), cx.y(y + 0.48 * k)), vec2(cx.x(x - 0.50 * k), cx.y(y + 0.48 * k)));
            draw_triangle(t1, t2, b2, c);
            draw_triangle(t2, b1, b2, c);
            for (i, (w, h)) in [(0.035, 0.15), (0.05, 0.19), (0.07, 0.24)].iter().enumerate() {
                let yy = y - 0.38 * k + i as f32 * 0.30 * k;
                cx.rect(x - w * k, yy, 2.0 * w * k, h * k, 0.0, DARK);
            }
        }
        Icon::SplitCars => {
            // Split-Screen / Dual Garage panels icon
            cx.para(x - 0.46 * k, y - 0.34 * k, 0.38 * k, 0.68 * k, 0.08 * k, c);
            cx.para(x + 0.04 * k, y - 0.34 * k, 0.38 * k, 0.68 * k, 0.08 * k, c);
            cx.para(x - 0.38 * k, y - 0.22 * k, 0.22 * k, 0.44 * k, 0.05 * k, DARK);
            cx.para(x + 0.12 * k, y - 0.22 * k, 0.22 * k, 0.44 * k, 0.05 * k, DARK);
        }
        Icon::GarageHouse => {
            // House/Garage roof + door
            draw_triangle(vec2(cx.x(x - 0.48 * k), cx.y(y - 0.05 * k)), vec2(cx.x(x + 0.48 * k), cx.y(y - 0.05 * k)), vec2(cx.x(x), cx.y(y - 0.48 * k)), c);
            cx.rect(x - 0.36 * k, y - 0.05 * k, 0.72 * k, 0.50 * k, 0.04 * k, c);
            cx.rect(x - 0.18 * k, y + 0.12 * k, 0.36 * k, 0.33 * k, 0.02 * k, DARK);
        }
        Icon::Controller => {
            cx.rect(x - 0.44 * k, y - 0.22 * k, 0.88 * k, 0.46 * k, 0.18 * k, c);
            cx.circle(x - 0.22 * k, y + 0.01 * k, 0.08 * k, DARK);
            cx.circle(x + 0.22 * k, y - 0.04 * k, 0.05 * k, DARK);
            cx.circle(x + 0.12 * k, y + 0.06 * k, 0.05 * k, DARK);
        }
        Icon::Stopwatch => {
            draw_circle_lines(cx.x(x), cx.y(y + 0.04 * k), cx.s(0.42 * k), cx.s(3.2), c);
            cx.rect(x - 0.10 * k, y - 0.48 * k, 0.20 * k, 0.09 * k, 0.02 * k, c);
            cx.line(x, y + 0.04 * k, x + 0.18 * k, y - 0.14 * k, 3.0, c);
        }
        Icon::Folder => {
            cx.rect(x - 0.48 * k, y - 0.34 * k, 0.38 * k, 0.18 * k, 0.05 * k, c);
            cx.rect(x - 0.48 * k, y - 0.22 * k, 0.96 * k, 0.66 * k, 0.08 * k, c);
            let d = DARK;
            cx.line(x - 0.16 * k, y + 0.16 * k, x, y - 0.02 * k, 2.0, d);
            cx.line(x, y - 0.02 * k, x + 0.16 * k, y + 0.16 * k, 2.0, d);
            cx.circle(x - 0.16 * k, y + 0.18 * k, 0.065 * k, d);
            cx.circle(x, y - 0.02 * k, 0.065 * k, d);
            cx.circle(x + 0.16 * k, y + 0.18 * k, 0.065 * k, d);
        }
    }
}

impl Front {
    pub fn new() -> Front {
        let servers = load_servers();
        let status: Vec<SharedStatus> = servers.iter().map(|_| Arc::new(Mutex::new(Status::Pending))).collect();
        Front {
            screen: Screen::Main,
            settings: load_settings(),
            branding: load_branding(),
            sel: 1, // Default highlighted tile = Freeroam
            hover_anim: [0.0; 11],
            servers,
            status,
            msel: 0,
            mfirst: 0,
            dialog: None,
            join: None,
            toast: None,
            last_click: (usize::MAX, 0.0),
            fade: 0.0,
            bs_hold: 0.0,
            drag_vol: false,
            drag_res: false,
            custom_paint_idx: 0,
            custom_tire_psi: 32.0,
            custom_bus_passengers: 24,
            search: String::new(),
            search_focus: false,
            f_builtin: true,
            f_mods: true,
            f_rwd: true,
            f_fwd: true,
            f_awd: true,
            f_fav_only: false,
            more_filters: true,
            rtab: 0,
            vscroll: 0.0,
            vfollow: true,
            thumbs: (0..SPECS.len()).map(|_| None).collect(),
        }
    }

    pub fn skip_fade(&mut self) { self.fade = 1.0; }

    pub fn go(&mut self, s: Screen) {
        self.screen = s;
        self.fade = 0.0;
        if s == Screen::Multiplayer { self.refresh_all(); }
    }

    pub fn refresh_all(&mut self) {
        for (s, st) in self.servers.iter().zip(self.status.iter()) {
            query_status(s.address.clone(), st.clone());
        }
    }

    pub fn info(&mut self, m: &str) { self.toast = Some((m.to_string(), 3.0, false)); }
    pub fn error(&mut self, m: &str) { self.toast = Some((m.to_string(), 4.5, true)); }

    fn start_join(&mut self, addr: String, label: String) {
        let net = start_net_client(&addr, &self.settings.name);
        self.join = Some(Join { net, started: Instant::now(), label });
    }

    pub fn draw(&mut self, res: &Res, menu: &mut Menu, t: f32, dt: f32) -> FrontAction {
        self.fade += dt;
        let (sw, sh) = (screen_width(), screen_height());
        self.ensure_thumbs(res);

        if self.screen == Screen::Freeroam {
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.08, 0.10, 0.15, 0.58));
        } else if self.screen != Screen::Main && self.screen != Screen::Garage {
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.06, 0.08, 0.12, 0.45));
        }

        let live = self.dialog.is_none() && self.join.is_none();
        let cx = Cx::new(res, live);
        let mut action = FrontAction::None;

        match self.screen {
            Screen::Main => action = self.main_screen(&cx, menu, t, dt),
            Screen::Multiplayer => self.mp_screen(&cx, dt),
            Screen::Options => self.options_screen(&cx),
            Screen::Garage => action = self.garage_screen(&cx, menu),
            Screen::Credits => self.credits_screen(&cx),
            Screen::More => self.more_screen(&cx),
            Screen::Mods => self.mods_screen(&cx, menu),
            Screen::Freeroam => action = self.vehicles_screen(&cx, menu, dt),
        }
        self.bottom_bar(&cx);

        self.dialog_ui(res, dt);
        if let FrontAction::Connected(n) = self.join_ui(res) { action = FrontAction::Connected(n); }

        if let Some((m, ttl, err)) = &mut self.toast {
            *ttl -= dt;
            let cxx = Cx::new(res, false);
            let w = cxx.width(false, m, 18.0) + 56.0;
            cxx.rect(640.0 - w / 2.0, 22.0, w, 46.0, 10.0, if *err { Color::new(0.82, 0.20, 0.14, 0.95) } else { Color::new(0.10, 0.12, 0.16, 0.95) });
            cxx.text(false, m, 640.0 - w / 2.0 + 28.0, 52.0, 18.0, WHITE);
        }
        if matches!(&self.toast, Some((_, t, _)) if *t <= 0.0) { self.toast = None; }

        if self.fade < 0.25 {
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, (1.0 - self.fade / 0.25) * 0.65));
        }

        if live && self.screen == Screen::Main && is_key_pressed(KeyCode::Escape) { return FrontAction::Quit; }
        action
    }
}

const ROWS_VISIBLE: usize = 5;

enum Out { None, Close, Save(Option<usize>, String, String), Join(String), Remove(usize), Name(String), ConfirmHeavyMap(usize) }

fn modal(cx: &Cx, x: f32, y: f32, w: f32, h: f32, title: &str) {
    cx.rect(x - 3.0, y - 3.0, w + 6.0, h + 6.0, 13.0, Color::new(0.0, 0.0, 0.0, 0.35));
    cx.rect(x, y, w, h, 10.0, Color::new(0.10, 0.11, 0.14, 0.98));
    cx.rect(x + 10.0, y, w - 20.0, 3.0, 0.0, ORANGE);
    cx.text(true, title, x + 30.0, y + 46.0, 25.0, WHITE);
}

fn signal_bars(cx: &Cx, x: f32, bottom: f32, level: usize, col: Color) {
    for b in 0..4usize {
        let h = 8.0 + 6.0 * b as f32;
        let c = if b < level { col } else { Color::new(1.0, 1.0, 1.0, 0.16) };
        cx.rect(x + b as f32 * 9.0, bottom - h, 6.0, h, 1.5, c);
    }
}

impl Front {
    fn logo(&self, cx: &Cx, x: f32, y: f32, w: f32, h: f32) {
        let sk = 14.0;
        // Drop shadow behind parallelogram plate
        cx.para(x - 2.0, y + 3.0, w + 4.0, h, sk, Color::new(0.0, 0.0, 0.0, 0.35));
        cx.para(x, y, w, h, sk, Color::new(0.97, 0.97, 0.98, 0.98));
        cx.para(x, y, 16.0, h, sk, ORANGE);
        cx.para(x + w - 16.0, y, 16.0, h, sk, ORANGE);
        let (a, b, sz) = (self.branding.game_title_prefix.as_str(), self.branding.game_title_suffix.as_str(), 30.0);
        let total = cx.width(true, a, sz) + cx.width(true, b, sz);
        let start = x + sk / 2.0 + (w - total) / 2.0 + 16.0;
        let ty = y + h / 2.0 + 10.5;
        cx.text(true, a, start, ty, sz, DARK);
        cx.text(true, b, start + cx.width(true, a, sz), ty, sz, ORANGE);
        // 3-node connected beam emblem
        let ix = start - 36.0;
        let (p1, p2, p3) = ((ix, y + h * 0.68), (ix + 12.0, y + h * 0.30), (ix + 24.0, y + h * 0.68));
        cx.line(p1.0, p1.1, p2.0, p2.1, 2.4, DARK);
        cx.line(p2.0, p2.1, p3.0, p3.1, 2.4, DARK);
        cx.line(p1.0, p1.1, p3.0, p3.1, 2.4, DARK);
        cx.circle(p1.0, p1.1, 4.2, ORANGE);
        cx.circle(p2.0, p2.1, 3.6, ORANGE);
        cx.circle(p3.0, p3.1, 5.0, ORANGE);
    }

    fn header(&mut self, cx: &Cx, title: &str, sub: &str, x: f32, y: f32, w: f32) {
        let dark = Color::new(0.03, 0.04, 0.06, 0.98);
        let bh = cx.live && cx.hit(13.0, 12.0, 62.0, 26.0);
        cx.rect(13.0, 12.0, 62.0, 26.0, 4.0, if bh { Color::new(0.16, 0.19, 0.25, 1.0) } else { dark });
        cx.text(true, "< Back", 22.0, 30.0, 13.0, WHITE);
        let crumb = format!("{}   /   {}", self.branding.game_title, title);
        let cw = cx.width(false, &crumb, 13.0) + 24.0;
        cx.rect(78.0, 12.0, cw, 26.0, 4.0, dark);
        cx.text(false, &crumb, 90.0, 30.0, 13.0, WHITE);
        cx.rect(x, y, w, 38.0, 0.0, dark);
        cx.para(x + 1.0, y + 3.0, 11.0, 32.0, 8.0, ORANGE);
        cx.text(true, title, x + 29.0, y + 29.0, 26.0, WHITE);
        cx.text_r(false, sub, x + w - 14.0, y + 24.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.55));
        if bh && cx.click { self.go(Screen::Main); }
    }

    fn panel(&self, cx: &Cx, x: f32, y: f32, w: f32, h: f32) {
        cx.rect(x, y, w, h, 10.0, PANEL_BG);
        cx.outline(x, y, w, h, 1.0, Color::new(1.0, 1.0, 1.0, 0.08));
    }

    /// Task 7: Exact layout matching `menu_reference` (`image_e59174.jpg`):
    /// - Top row (indices 0..2): 3 wide visual tiles (`Quick-Start Experiences`, `Freeroam`, `Garage & Tuning`)
    /// - Bottom row left (indices 3..6): 4 square visual tiles (`Career`, `Photo Mode 4K`, `Servers`, `More...`)
    /// - Bottom row right (indices 7..10): 4 stacked horizontal buttons (`Options`, `Repository`, `Credits`, `Quit`)
    fn main_screen(&mut self, cx: &Cx, _menu: &Menu, _t: f32, dt: f32) -> FrontAction {
        let grid_w = 684.0f32;
        let x0 = 640.0 - grid_w / 2.0; // 298.0
        let gap = 6.0f32;
        // Row 1: 3 wide tiles: (684 - 12) / 3 = 224.0 wide x 154.0 high
        let top_w = 224.0f32;
        let top_h = 154.0f32;
        let top_y = 226.0f32;

        // Row 2: 4 square tiles (130.0 wide x 134.0 high) + 1 stacked button column (140.0 wide x 134.0 high)
        // 4 * 130 + 140 + 4 * 6 = 520 + 140 + 24 = 684.0!
        let bot_w = 130.0f32;
        let bot_h = 134.0f32;
        let bot_y = top_y + top_h + gap; // 386.0
        let col5_x = x0 + 4.0 * (bot_w + gap); // 298 + 544 = 842.0
        let col5_w = 140.0f32;
        let btn_h = 30.5f32;
        let btn_gap = 4.0f32;

        // Top center slanted logo plate matching `image_e59174.jpg`
        self.logo(cx, 476.0, 156.0, 328.0, 46.0);

        let rect_of = |i: usize| -> (f32, f32, f32, f32) {
            if i < 3 {
                (x0 + i as f32 * (top_w + gap), top_y, top_w, top_h)
            } else if i < 7 {
                let k = i - 3;
                (x0 + k as f32 * (bot_w + gap), bot_y, bot_w, bot_h)
            } else {
                let k = i - 7;
                (col5_x, bot_y + k as f32 * (btn_h + btn_gap), col5_w, btn_h)
            }
        };

        let mut activate: Option<usize> = None;
        if cx.live {
            for i in 0..11 {
                let (x, y, w, h) = rect_of(i);
                let hovered = cx.hit(x, y, w, h);
                if hovered {
                    self.sel = i;
                    if cx.click { activate = Some(i); }
                }
                let target = if self.sel == i { 1.0 } else { 0.0 };
                self.hover_anim[i] += (target - self.hover_anim[i]) * (dt * 14.0).min(1.0);
            }
            if is_key_pressed(KeyCode::Right) {
                self.sel = match self.sel {
                    0 => 1, 1 => 2, 2 => 7,
                    3 => 4, 4 => 5, 5 => 6, 6 => 7,
                    x => x,
                };
            }
            if is_key_pressed(KeyCode::Left) {
                self.sel = match self.sel {
                    1 => 0, 2 => 1,
                    4 => 3, 5 => 4, 6 => 5,
                    7..=10 => 6,
                    x => x,
                };
            }
            if is_key_pressed(KeyCode::Down) {
                self.sel = match self.sel {
                    0 => 3, 1 => 4, 2 => 6,
                    7 => 8, 8 => 9, 9 => 10,
                    x => x,
                };
            }
            if is_key_pressed(KeyCode::Up) {
                self.sel = match self.sel {
                    3 | 4 => 0, 5 | 6 => 1,
                    7 => 2, 8 => 7, 9 => 8, 10 => 9,
                    x => x,
                };
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
                activate = Some(self.sel);
            }
        }

        // Draw Top Row (3 wide tiles)
        let top_tiles: [(&str, Icon, usize, bool); 3] = [
            ("Quick-Start Experiences", Icon::Key, 0, true),
            ("Freeroam", Icon::Road, 1, false),
            ("Garage & Tuning", Icon::SplitCars, 4, true),
        ];
        for i in 0..3 {
            let (label, icon, thumb_car, is_new) = top_tiles[i];
            let (x, y, w, h) = rect_of(i);
            let on = cx.live && self.sel == i;
            let anim = self.hover_anim[i];
            cx.rect(x, y, w, h, 4.0, if on { TILE_HI } else { TILE_BG });
            // Draw rendered 3D vehicle preview backdrop inside the tile
            if let Some(Some(rt)) = self.thumbs.get(thumb_car) {
                draw_texture_ex(
                    &rt.texture,
                    cx.x(x + 2.0),
                    cx.y(y + 2.0),
                    Color::new(0.85 + 0.15 * anim, 0.85 + 0.15 * anim, 0.90, 0.65 + 0.25 * anim),
                    DrawTextureParams {
                        dest_size: Some(vec2(cx.s(w - 4.0), cx.s(h - 4.0))),
                        flip_y: true,
                        ..Default::default()
                    },
                );
            }
            // Bottom dark gradient strip inside card for crisp icon + text legibility
            grad_rect(cx, x, y + h * 0.45, w, h * 0.55, Color::new(0.04, 0.05, 0.07, 0.10), Color::new(0.03, 0.04, 0.06, 0.88));
            draw_icon(cx, icon, x + 28.0, y + h - 48.0, 34.0, Color::new(1.0, 1.0, 1.0, 0.92));
            cx.text(true, label, x + 12.0, y + h - 11.0, 14.5, WHITE);
            if is_new {
                cx.rect(x + w - 38.0, y + 6.0, 32.0, 15.0, 2.5, BLUE);
                cx.text_c(true, "New!", x + w - 22.0, y + 17.5, 10.5, WHITE);
            }
            if on {
                cx.outline(x, y, w, h, 2.0, ORANGE);
            }
        }

        // Draw Bottom Row (4 square tiles)
        let bot_tiles: [(&str, Icon, usize); 4] = [
            ("Career", Icon::GarageHouse, 2),
            ("Photo Mode 4K", Icon::Controller, 3),
            ("Servers", Icon::Stopwatch, 0),
            ("More...", Icon::Folder, 1),
        ];
        for k in 0..4 {
            let i = 3 + k;
            let (label, icon, thumb_car) = bot_tiles[k];
            let (x, y, w, h) = rect_of(i);
            let on = cx.live && self.sel == i;
            let anim = self.hover_anim[i];
            cx.rect(x, y, w, h, 4.0, if on { TILE_HI } else { TILE_BG });
            if let Some(Some(rt)) = self.thumbs.get(thumb_car) {
                draw_texture_ex(
                    &rt.texture,
                    cx.x(x + 2.0),
                    cx.y(y + 2.0),
                    Color::new(0.75 + 0.20 * anim, 0.78 + 0.18 * anim, 0.85, 0.48 + 0.25 * anim),
                    DrawTextureParams {
                        dest_size: Some(vec2(cx.s(w - 4.0), cx.s(h - 4.0))),
                        flip_y: true,
                        ..Default::default()
                    },
                );
            }
            grad_rect(cx, x, y + h * 0.40, w, h * 0.60, Color::new(0.04, 0.05, 0.07, 0.12), Color::new(0.03, 0.04, 0.06, 0.90));
            draw_icon(cx, icon, x + 28.0, y + h - 46.0, 34.0, Color::new(1.0, 1.0, 1.0, 0.90));
            cx.text(true, label, x + 10.0, y + h - 10.0, 13.5, WHITE);
            if on {
                cx.outline(x, y, w, h, 2.0, ORANGE);
            }
        }

        // Draw Right Column (4 stacked horizontal buttons: Options, Repository, Credits, Quit)
        let stack_labels = ["Options", "Repository", "Credits", "Quit"];
        for k in 0..4 {
            let i = 7 + k;
            let (x, y, w, h) = rect_of(i);
            let on = cx.live && self.sel == i;
            let is_quit = k == 3;
            let base = if is_quit {
                Color::new(0.68, 0.25, 0.20, 0.88)
            } else {
                Color::new(0.22, 0.24, 0.26, 0.82)
            };
            let hi = if is_quit {
                Color::new(0.86, 0.32, 0.26, 0.98)
            } else {
                Color::new(0.34, 0.36, 0.40, 0.95)
            };
            cx.rect(x, y, w, h, 3.5, if on { hi } else { base });
            if on {
                cx.outline(x, y, w, h, 1.8, ORANGE);
            }
            cx.text_c(false, stack_labels[k], x + w / 2.0, y + h / 2.0 + 5.0, 14.0, WHITE);
        }

        if let Some(i) = activate {
            match i {
                0 => return FrontAction::Start, // Quick-Start directly into Official `level.json` Map!
                1 => self.go(Screen::Freeroam),
                2 => self.go(Screen::Garage),
                3 => self.info("Career mode: Complete championship stages in West Coast Urban District"),
                4 => return FrontAction::StartPhotoMode,
                5 => self.go(Screen::Multiplayer),
                6 => self.go(Screen::More),
                7 => self.go(Screen::Options),
                8 => self.go(Screen::Mods),
                9 => self.go(Screen::Credits),
                _ => return FrontAction::Quit,
            }
        }
        FrontAction::None
    }

    fn garage_screen(&mut self, cx: &Cx, menu: &mut Menu) -> FrontAction {
        let mut action = FrontAction::None;
        self.header(cx, "Garage & Tuning", "Customize Clear Coat paint, tire pressure (PSI), and bus passenger load", 820.0, 56.0, 430.0);
        let (px, py, pw, ph) = (820.0, 94.0, 430.0, 560.0);
        self.panel(cx, px, py, pw, ph);

        // Vehicle selector strip
        let cur_name = menu.cars[menu.sel_car].name.clone();
        cx.text(true, "SELECTED VEHICLE", px + 24.0, py + 38.0, 14.0, ORANGE);
        if cx.button("<", px + 24.0, py + 48.0, 44.0, 38.0, Btn::Normal, true) {
            menu.sel_car = if menu.sel_car == 0 { menu.cars.len() - 1 } else { menu.sel_car - 1 };
        }
        cx.rect(px + 76.0, py + 48.0, pw - 152.0, 38.0, 6.0, Color::new(0.12, 0.14, 0.18, 0.95));
        let fit_nm = cx.fit(true, &cur_name, 16.0, pw - 170.0);
        cx.text_c(true, &fit_nm, px + pw / 2.0, py + 72.0, 16.0, WHITE);
        if cx.button(">", px + pw - 68.0, py + 48.0, 44.0, 38.0, Btn::Normal, true) {
            menu.sel_car = (menu.sel_car + 1) % menu.cars.len();
        }

        // Clear Coat Metallic Paint Swatches
        cx.text(true, "CLEAR COAT METALLIC PAINT", px + 24.0, py + 118.0, 14.0, ORANGE);
        for (pi, (pname, rgb)) in GARAGE_PAINTS.iter().enumerate() {
            let col = pi % 4;
            let row = pi / 4;
            let sx = px + 24.0 + col as f32 * 98.0;
            let sy = py + 130.0 + row as f32 * 44.0;
            let sw = 88.0;
            let sh = 34.0;
            let hv = cx.live && cx.hit(sx, sy, sw, sh);
            cx.rect(sx, sy, sw, sh, 6.0, Color::new(rgb[0], rgb[1], rgb[2], 1.0));
            if self.custom_paint_idx == pi || hv {
                cx.outline(sx, sy, sw, sh, 2.5, if self.custom_paint_idx == pi { ORANGE } else { WHITE });
            }
            if hv && cx.click {
                self.custom_paint_idx = pi;
                if let CarKind::Builtin(bi) = menu.cars[menu.sel_car].kind {
                    if let Some(c) = menu.previews.get_mut(&bi) {
                        c.paint = vec3(rgb[0], rgb[1], rgb[2]);
                    }
                }
            }
            if self.custom_paint_idx == pi {
                cx.text(false, pname, px + 24.0, py + 232.0, 13.5, Color::new(0.85, 0.90, 0.98, 0.9));
            }
        }

        // Tire Pressure PSI adjustment (Task 6.4)
        cx.text(true, &format!("TIRE PRESSURE: {:.0} PSI ({:.2} Bar)", self.custom_tire_psi, self.custom_tire_psi * 0.06895), px + 24.0, py + 268.0, 14.0, ORANGE);
        cx.text(false, "Affects tire radial compression, rolling resistance & peak Pacejka grip", px + 24.0, py + 286.0, 12.0, Color::new(1.0, 1.0, 1.0, 0.55));
        if cx.button("- 2 PSI", px + 24.0, py + 296.0, 110.0, 34.0, Btn::Normal, true) {
            self.custom_tire_psi = (self.custom_tire_psi - 2.0).max(18.0);
        }
        if cx.button("Optimal (32 PSI)", px + 148.0, py + 296.0, 134.0, 34.0, Btn::Normal, true) {
            self.custom_tire_psi = 32.0;
        }
        if cx.button("+ 2 PSI", px + 296.0, py + 296.0, 110.0, 34.0, Btn::Normal, true) {
            self.custom_tire_psi = (self.custom_tire_psi + 2.0).min(120.0);
        }

        // Bus Passenger Load System (Task 6.5)
        let extra_kg = self.custom_bus_passengers as f32 * 80.0;
        cx.text(true, &format!("BUS PASSENGER LOAD: {} Passengers (+{:.0} kg)", self.custom_bus_passengers, extra_kg), px + 24.0, py + 360.0, 14.0, ORANGE);
        cx.text(false, "Each passenger adds 80 kg real mass, shifts CoG & compresses suspension", px + 24.0, py + 378.0, 12.0, Color::new(1.0, 1.0, 1.0, 0.55));
        if cx.button("- 5 Pax", px + 24.0, py + 388.0, 90.0, 34.0, Btn::Normal, true) {
            self.custom_bus_passengers = self.custom_bus_passengers.saturating_sub(5);
        }
        if cx.button("Empty (0)", px + 122.0, py + 388.0, 90.0, 34.0, Btn::Normal, true) {
            self.custom_bus_passengers = 0;
        }
        if cx.button("Half (25)", px + 220.0, py + 388.0, 90.0, 34.0, Btn::Normal, true) {
            self.custom_bus_passengers = 25;
        }
        if cx.button("Full (50)", px + 318.0, py + 388.0, 88.0, 34.0, Btn::Normal, true) {
            self.custom_bus_passengers = 50;
        }

        if cx.button("Drive Now in Selected Map", px + 24.0, py + ph - 108.0, pw - 48.0, 44.0, Btn::Primary, true) {
            action = FrontAction::Start;
        }
        if cx.button("Back to Main Menu", px + 24.0, py + ph - 54.0, pw - 48.0, 38.0, Btn::Normal, true) {
            self.go(Screen::Main);
        }
        if cx.live && is_key_pressed(KeyCode::Escape) { self.go(Screen::Main); }
        action
    }

    fn fix_scroll(&mut self) {
        if self.msel < self.mfirst { self.mfirst = self.msel; }
        if self.msel >= self.mfirst + ROWS_VISIBLE { self.mfirst = self.msel + 1 - ROWS_VISIBLE; }
    }

    fn open_dialog(&mut self, d: Dialog) {
        flush_chars();
        self.bs_hold = 0.0;
        self.dialog = Some(d);
    }

    fn join_selected(&mut self) {
        if let Some(s) = self.servers.get(self.msel) {
            let (a, n) = (s.address.clone(), s.name.clone());
            self.start_join(a, n);
        }
    }

    fn mp_screen(&mut self, cx: &Cx, _dt: f32) {
        self.header(cx, "Multiplayer Servers", "Server-Authoritative Worlds, Automatic Map Sync & Permissions", 190.0, 86.0, 900.0);
        let un = format!("Playing as  {}", self.settings.name);
        let cw = cx.width(false, &un, 14.0) + 112.0;
        let (chx, chy) = (1090.0 - cw, 46.0);
        let chover = cx.live && cx.hit(chx, chy, cw, 30.0);
        cx.rect(chx, chy, cw, 30.0, 15.0, if chover { Color::new(1.0, 1.0, 1.0, 0.20) } else { Color::new(1.0, 1.0, 1.0, 0.10) });
        cx.text(false, &un, chx + 16.0, chy + 20.0, 14.0, WHITE);
        cx.text_r(true, "Change", chx + cw - 14.0, chy + 20.0, 13.0, ORANGE);
        if chover && cx.click {
            let b = self.settings.name.clone();
            self.open_dialog(Dialog::Username { buf: b, err: String::new() });
        }

        let (px, py, pw, ph) = (190.0, 124.0, 900.0, 402.0);
        self.panel(cx, px, py, pw, ph);
        let n = self.servers.len();
        let rh = 76.0;

        if cx.live {
            if is_key_pressed(KeyCode::Down) && n > 0 { self.msel = (self.msel + 1).min(n - 1); self.fix_scroll(); }
            if is_key_pressed(KeyCode::Up) { self.msel = self.msel.saturating_sub(1); self.fix_scroll(); }
            let (_, wh) = mouse_wheel();
            if wh != 0.0 && cx.hit(px, py, pw, ph) {
                if wh > 0.0 { self.mfirst = self.mfirst.saturating_sub(1); }
                else { self.mfirst = (self.mfirst + 1).min(n.saturating_sub(ROWS_VISIBLE)); }
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) { self.join_selected(); }
            if is_key_pressed(KeyCode::F5) { self.refresh_all(); }
            if is_key_pressed(KeyCode::Delete) && n > 0 { self.open_dialog(Dialog::Delete(self.msel)); }
            if is_key_pressed(KeyCode::Escape) { self.go(Screen::Main); }
        }
        self.mfirst = self.mfirst.min(n.saturating_sub(ROWS_VISIBLE));

        if n == 0 {
            cx.text_c(true, "No servers saved yet", px + pw / 2.0, py + 180.0, 26.0, Color::new(1.0, 1.0, 1.0, 0.75));
            cx.text_c(false, "Press  Add Server  and enter an IP address or a domain name", px + pw / 2.0, py + 214.0, 17.0, Color::new(1.0, 1.0, 1.0, 0.45));
        }
        let mut clicked: Option<usize> = None;
        let dots = ".".repeat(1 + ((get_time() * 3.0) as usize % 3));
        for k in 0..ROWS_VISIBLE {
            let i = self.mfirst + k;
            if i >= n { break; }
            let (x, y, w, h) = (px + 10.0, py + 11.0 + k as f32 * rh, pw - 28.0, rh - 6.0);
            let hover = cx.live && cx.hit(x, y, w, h);
            if hover && cx.click { clicked = Some(i); }
            let sel = i == self.msel;
            cx.rect(x, y, w, h, 10.0, if sel { Color::new(1.0, 1.0, 1.0, 0.11) } else if hover { Color::new(1.0, 1.0, 1.0, 0.07) } else { Color::new(1.0, 1.0, 1.0, 0.035) });
            if sel { cx.outline(x, y, w, h, 2.0, ORANGE); }
            let s = &self.servers[i];
            let col = server_color(&s.name);
            cx.rect(x + 12.0, y + 7.0, 56.0, 56.0, 9.0, col);
            let letter: String = s.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
            cx.text_c(true, &letter, x + 40.0, y + 46.0, 30.0, WHITE);
            let st = self.status[i].lock().unwrap().clone();
            let max_left = w - 270.0;
            let nm = cx.fit(true, &s.name, 20.0, max_left);
            cx.text(true, &nm, x + 82.0, y + 27.0, 20.0, WHITE);
            cx.text(false, &s.address, x + 82.0, y + 46.0, 14.0, Color::new(1.0, 1.0, 1.0, 0.50));
            let rx = x + w - 14.0;
            match st {
                Status::Pending => {
                    cx.text(false, &format!("Pinging{}", dots), x + 82.0, y + 63.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.45));
                    signal_bars(cx, rx - 36.0, y + 52.0, 0, WHITE);
                }
                Status::Online { ping_ms, name, description, map, players, max_players, known } => {
                    let (level, bc) = if ping_ms < 60 { (4, GREEN) } else if ping_ms < 120 { (3, GREEN) } else if ping_ms < 220 { (2, Color::new(0.95, 0.80, 0.2, 1.0)) } else { (1, ORANGE) };
                    signal_bars(cx, rx - 36.0, y + 52.0, level, bc);
                    if known {
                        cx.text_r(true, &format!("{}/{}", players, max_players), rx - 52.0, y + 31.0, 19.0, WHITE);
                        let mut info = if description.is_empty() { name } else { format!("{}  -  {}", name, description) };
                        if !map.is_empty() { info = format!("{}  -  Map: {}", info, map); }
                        let info = cx.fit(false, &info, 13.0, max_left);
                        cx.text(false, &info, x + 82.0, y + 63.0, 13.0, Color::new(0.55, 0.85, 1.0, 0.9));
                    } else {
                        cx.text(false, "Online  (no server details)", x + 82.0, y + 63.0, 13.0, Color::new(0.55, 0.85, 1.0, 0.9));
                    }
                    cx.text_r(false, &format!("{} ms", ping_ms), rx - 52.0, y + 52.0, 14.0, Color::new(1.0, 1.0, 1.0, 0.55));
                }
                Status::Offline(msg) => {
                    cx.text(false, &format!("Can't connect to server  ({})", msg), x + 82.0, y + 63.0, 13.0, Color::new(0.95, 0.40, 0.35, 1.0));
                    signal_bars(cx, rx - 36.0, y + 52.0, 0, WHITE);
                    cx.text_r(true, "Offline", rx - 52.0, y + 31.0, 16.0, Color::new(0.95, 0.40, 0.35, 0.9));
                }
            }
        }
        if n > ROWS_VISIBLE {
            let th = (ph - 20.0) * ROWS_VISIBLE as f32 / n as f32;
            let ty = py + 10.0 + (ph - 20.0 - th) * self.mfirst as f32 / (n - ROWS_VISIBLE) as f32;
            cx.rect(px + pw - 14.0, ty, 5.0, th, 2.5, Color::new(1.0, 1.0, 1.0, 0.30));
        }
        if let Some(i) = clicked {
            let now = get_time();
            let dbl = self.last_click.0 == i && now - self.last_click.1 < 0.45;
            self.msel = i;
            self.last_click = (i, now);
            if dbl { self.join_selected(); }
        }

        let has = n > 0;
        let (b1y, b2y) = (546.0, 598.0);
        let x1 = 640.0 - (3.0 * 220.0 + 2.0 * 14.0) / 2.0;
        if cx.button("Join Server", x1, b1y, 220.0, 44.0, Btn::Primary, has) { self.join_selected(); }
        if cx.button("Direct Connection", x1 + 234.0, b1y, 220.0, 44.0, Btn::Normal, true) { self.open_dialog(Dialog::Direct { addr: String::new(), err: String::new() }); }
        if cx.button("Add Server", x1 + 468.0, b1y, 220.0, 44.0, Btn::Normal, true) {
            self.open_dialog(Dialog::Server { edit: None, name: String::new(), addr: String::new(), field: 0, err: String::new() });
        }
        let x2 = 640.0 - (4.0 * 160.0 + 3.0 * 12.0) / 2.0;
        if cx.button("Edit", x2, b2y, 160.0, 44.0, Btn::Normal, has) {
            let s = self.servers[self.msel].clone();
            self.open_dialog(Dialog::Server { edit: Some(self.msel), name: s.name, addr: s.address, field: 0, err: String::new() });
        }
        if cx.button("Delete", x2 + 172.0, b2y, 160.0, 44.0, Btn::Normal, has) { self.open_dialog(Dialog::Delete(self.msel)); }
        if cx.button("Refresh", x2 + 344.0, b2y, 160.0, 44.0, Btn::Normal, has) { self.refresh_all(); self.info("Refreshing server list..."); }
        if cx.button("Back", x2 + 516.0, b2y, 160.0, 44.0, Btn::Normal, true) { self.go(Screen::Main); }
    }

    /// Task 1.3: Comprehensive Graphics & Performance Settings Menu (Low/Medium/High, Dynamic Resolution, FPS Cap, Shadows, Clouds, Audio).
    fn options_screen(&mut self, cx: &Cx) {
        self.header(cx, "Graphics & Options", "Optimized for Low-End PCs (Default: Low + Dynamic Resolution)", 210.0, 62.0, 860.0);
        let (px, py, pw, ph) = (210.0, 100.0, 860.0, 510.0);
        self.panel(cx, px, py, pw, ph);

        // Row 1: Graphics Quality Preset (Low / Medium / High)
        cx.text(true, "Graphics Quality Preset", px + 36.0, py + 48.0, 19.0, WHITE);
        cx.text(false, "Adjusts LOD distance, texture compression (512/1024/2048), shadows & physics rate", px + 36.0, py + 68.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.55));
        for (pi, plabel) in [(0u8, "Low (Weak PC)"), (1u8, "Medium"), (2u8, "High (Ultra)")].iter() {
            let bx = px + pw - 420.0 + (*pi as f32) * 132.0;
            let on = self.settings.gfx_preset == *pi;
            if cx.button(plabel, bx, py + 30.0, 124.0, 38.0, if on { Btn::Primary } else { Btn::Normal }, true) {
                self.settings.apply_preset(*pi);
                save_settings(&self.settings);
                self.info(&format!("Graphics preset set to {}", plabel));
            }
        }

        // Row 2: Dynamic Resolution Scaling + Resolution Scale Slider
        cx.rect(px + 30.0, py + 84.0, pw - 60.0, 1.0, 0.0, Color::new(1.0, 1.0, 1.0, 0.08));
        cx.text(true, "Dynamic Resolution Scaling", px + 36.0, py + 122.0, 19.0, WHITE);
        cx.text(false, "Automatically scales 3D render resolution (55%-100%) to prevent FPS drops", px + 36.0, py + 142.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.55));
        let dlbl = if self.settings.dyn_res { "AUTO ON" } else { "MANUAL" };
        if cx.button(dlbl, px + pw - 420.0, py + 104.0, 124.0, 38.0, if self.settings.dyn_res { Btn::Primary } else { Btn::Normal }, true) {
            self.settings.dyn_res = !self.settings.dyn_res;
            save_settings(&self.settings);
        }
        let (rx, ry, rw) = (px + pw - 275.0, py + 123.0, 180.0);
        cx.rect(rx, ry - 4.0, rw, 8.0, 4.0, Color::new(1.0, 1.0, 1.0, 0.16));
        let norm_r = ((self.settings.res_scale - 0.50) / 0.50).clamp(0.0, 1.0);
        cx.rect(rx, ry - 4.0, rw * norm_r, 8.0, 4.0, ORANGE);
        cx.circle(rx + rw * norm_r, ry, 10.0, WHITE);
        cx.text(true, &format!("{:.0}%", self.settings.res_scale * 100.0), rx + rw + 14.0, ry + 6.0, 16.0, WHITE);
        if cx.live && cx.click && cx.hit(rx - 10.0, ry - 16.0, rw + 20.0, 32.0) { self.drag_res = true; }
        if self.drag_res {
            if is_mouse_button_down(MouseButton::Left) {
                let v = ((cx.mx - cx.x(rx)) / cx.s(rw)).clamp(0.0, 1.0);
                self.settings.res_scale = ((0.50 + v * 0.50) * 20.0).round() / 20.0;
            } else {
                self.drag_res = false;
                save_settings(&self.settings);
            }
        }

        // Row 3: FPS target for dynamic resolution; VSync controls presentation.
        cx.rect(px + 30.0, py + 158.0, pw - 60.0, 1.0, 0.0, Color::new(1.0, 1.0, 1.0, 0.08));
        cx.text(true, "Dynamic Resolution Target", px + 36.0, py + 196.0, 19.0, WHITE);
        cx.text(false, "Used only for auto resolution; VSync follows your monitor", px + 36.0, py + 216.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.55));
        for (fi, (cap, clbl)) in [(30u32, "30 target"), (60u32, "60 target"), (120u32, "120 target"), (0u32, "Auto 60")].iter().enumerate() {
            let bx = px + pw - 420.0 + (fi as f32) * 98.0;
            let on = self.settings.fps_cap == *cap;
            if cx.button(clbl, bx, py + 178.0, 92.0, 36.0, if on { Btn::Primary } else { Btn::Normal }, true) {
                self.settings.fps_cap = *cap;
                save_settings(&self.settings);
            }
        }

        // Row 4: Shadows & Volumetric Clouds Toggles
        cx.rect(px + 30.0, py + 232.0, pw - 60.0, 1.0, 0.0, Color::new(1.0, 1.0, 1.0, 0.08));
        cx.text(true, "Planar Shadows & Volumetric Clouds", px + 36.0, py + 270.0, 19.0, WHITE);
        cx.text(false, "Disable on integrated GPUs for a +45% FPS boost", px + 36.0, py + 290.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.55));
        let sh_lbl = if self.settings.shadows_on { "Shadows: ON" } else { "Shadows: OFF" };
        if cx.button(sh_lbl, px + pw - 420.0, py + 252.0, 185.0, 38.0, if self.settings.shadows_on { Btn::Primary } else { Btn::Normal }, true) {
            self.settings.shadows_on = !self.settings.shadows_on;
            save_settings(&self.settings);
        }
        let cl_lbl = if self.settings.clouds_on { "Clouds: ON" } else { "Clouds: OFF" };
        if cx.button(cl_lbl, px + pw - 215.0, py + 252.0, 185.0, 38.0, if self.settings.clouds_on { Btn::Primary } else { Btn::Normal }, true) {
            self.settings.clouds_on = !self.settings.clouds_on;
            save_settings(&self.settings);
        }

        // Row 5: Anti-Aliasing & Forza Cockpit Camera Settings (Phase 5 & Phase 6)
        cx.rect(px + 30.0, py + 308.0, pw - 60.0, 1.0, 0.0, Color::new(1.0, 1.0, 1.0, 0.08));
        cx.text(true, "Anti-Aliasing & Cockpit Camera", px + 36.0, py + 338.0, 18.0, WHITE);
        cx.text(false, &format!("Edge AA + Forza Cockpit FOV ({:.0}°) & Head G-Motion ({:.0}%)", self.settings.cockpit_fov, self.settings.head_motion * 100.0), px + 36.0, py + 358.0, 12.5, Color::new(1.0, 1.0, 1.0, 0.55));
        let aa_lbl = match self.settings.aa_mode { 0 => "AA: Off", 1 => "AA: SMAA", _ => "AA: SMAA+CAS" };
        if cx.button(aa_lbl, px + pw - 420.0, py + 322.0, 132.0, 36.0, if self.settings.aa_mode > 0 { Btn::Primary } else { Btn::Normal }, true) {
            self.settings.aa_mode = (self.settings.aa_mode + 1) % 3;
            save_settings(&self.settings);
        }
        if cx.button(&format!("FOV: {:.0}°", self.settings.cockpit_fov), px + pw - 278.0, py + 322.0, 118.0, 36.0, Btn::Normal, true) {
            self.settings.cockpit_fov = if self.settings.cockpit_fov >= 72.0 { 48.0 } else { self.settings.cockpit_fov + 4.0 };
            save_settings(&self.settings);
        }
        if cx.button(&format!("Motion: {:.0}%", self.settings.head_motion * 100.0), px + pw - 150.0, py + 322.0, 120.0, 36.0, Btn::Normal, true) {
            self.settings.head_motion = if self.settings.head_motion >= 1.25 { 0.0 } else { (self.settings.head_motion + 0.25).min(1.25) };
            save_settings(&self.settings);
        }

        // Row 6: Menu Music & Volume
        cx.rect(px + 30.0, py + 378.0, pw - 60.0, 1.0, 0.0, Color::new(1.0, 1.0, 1.0, 0.08));
        cx.text(true, "Menu Soundtrack", px + 36.0, py + 418.0, 19.0, WHITE);
        let mlbl = if self.settings.music_on { "Music: ON" } else { "Music: OFF" };
        if cx.button(mlbl, px + pw - 420.0, py + 396.0, 124.0, 38.0, if self.settings.music_on { Btn::Primary } else { Btn::Normal }, true) {
            self.settings.music_on = !self.settings.music_on;
            save_settings(&self.settings);
        }
        let (sx, sy, sw) = (px + pw - 275.0, py + 415.0, 180.0);
        cx.rect(sx, sy - 4.0, sw, 8.0, 4.0, Color::new(1.0, 1.0, 1.0, 0.16));
        cx.rect(sx, sy - 4.0, sw * self.settings.music_vol, 8.0, 4.0, ORANGE);
        let kx = sx + sw * self.settings.music_vol;
        cx.circle(kx, sy, 10.0, WHITE);
        cx.text(true, &format!("{}%", (self.settings.music_vol * 100.0).round() as i32), sx + sw + 14.0, sy + 6.0, 16.0, WHITE);
        if cx.live && cx.click && cx.hit(sx - 14.0, sy - 18.0, sw + 28.0, 36.0) { self.drag_vol = true; }
        if self.drag_vol {
            if is_mouse_button_down(MouseButton::Left) {
                let v = ((cx.mx - cx.x(sx)) / cx.s(sw)).clamp(0.0, 1.0);
                self.settings.music_vol = (v * 100.0).round() / 100.0;
            } else {
                self.drag_vol = false;
                save_settings(&self.settings);
            }
        }

        if cx.button("Back to Main Menu", 640.0 - 110.0, py + ph - 52.0, 220.0, 40.0, Btn::Primary, true) { self.go(Screen::Main); }
        if cx.live && is_key_pressed(KeyCode::Escape) { self.go(Screen::Main); }
    }

    fn credits_screen(&mut self, cx: &Cx) {
        let title_sub = format!("{} - v{}", self.branding.game_title, self.branding.version);
        self.header(cx, "Credits & Attributions", &title_sub, 230.0, 92.0, 820.0);
        let (px, py, pw, ph) = (230.0, 130.0, 820.0, 390.0);
        self.panel(cx, px, py, pw, ph);
        let lines: [(&str, &str); 7] = [
            ("Simulation Engine", "APEX.drive - Custom Soft-Body Lattice & Pacejka Tire Simulation"),
            ("Map 1 (Default)", "West Coast Urban District - Official City & Proving Grounds"),
            ("Map 2 Attribution", "West Coast USA (west_coast_usa.zip) by Ahmed Benali (CC-BY 4.0)"),
            ("Map 3 & Studio", "Maseukowice Train Station (masuo) & Amazon Lumberyard Bistro Studio"),
            ("Vehicle Roster", "AMG One, AMG GT Black Series, 2025 BMW M3 CS, 911 GT3 RS, VW Crafter"),
            ("Graphics & Anti-Aliasing", "SMAA + CAS Upscaling, Dual-Layer Clear Coat PBR, 9-Tap PCF Shadows"),
            ("Typeface & Audio", "Poppins (SIL Open Font License) & Custom Main Menu Theme"),
        ];
        for (i, (k, v)) in lines.iter().enumerate() {
            let y = py + 54.0 + i as f32 * 44.0;
            cx.text(true, k, px + 32.0, y, 15.5, ORANGE);
            cx.text(false, v, px + 235.0, y, 15.0, WHITE);
        }
        cx.text_c(false, &format!("Alpha v{}", VERSION), px + pw / 2.0, py + ph - 22.0, 14.0, Color::new(1.0, 1.0, 1.0, 0.4));
        if cx.button("Back", 640.0 - 100.0, py + ph + 24.0, 200.0, 44.0, Btn::Primary, true) { self.go(Screen::Main); }
        if cx.live && is_key_pressed(KeyCode::Escape) { self.go(Screen::Main); }
    }

    fn more_screen(&mut self, cx: &Cx) {
        self.header(cx, "Controls & Dedicated Server Console", "In-Game Shortcuts & Admin Commands", 160.0, 76.0, 960.0);
        let (px, py, pw, ph) = (160.0, 114.0, 960.0, 450.0);
        self.panel(cx, px, py, pw, ph);
        cx.text(true, "DRIVING & PHOTO MODE CONTROLS", px + 32.0, py + 40.0, 16.0, ORANGE);
        let keys: [(&str, &str); 13] = [
            ("W / S / A / D", "Throttle, Brake, Steer"),
            ("Space / I", "Handbrake  /  Engine Start-Stop"),
            ("C", "Cycle Camera (Chase / Cockpit / Orbit)"),
            ("F8 / F12", "4K In-Game Photo Mode  /  Capture 4K Screenshot"),
            ("F10", "World Editor (360 Rotate, Repeat, Copy/Paste)"),
            ("[  /  ]", "Bus Passengers -/+ (Each adds 80 kg & shifts CoG)"),
            ("1 - 5", "1: AMG One | 2: AMG GT BS | 3: BMW M3 CS | 4: 911 GT3 RS | 5: VW Crafter"),
            ("0 / - / =", "Quick Shortcuts for AMG One, AMG GT BS, and Bus/Van"),
            ("V / B", "Toggle Traction Control (TC) / ABS"),
            ("N / M", "Scrub Sunset / Sunrise Time of Day"),
            ("R / T", "Reset All Cars  /  Repair Current Car Here"),
            ("Enter / F5", "In-Game Multiplayer Chat  /  Quick Connect"),
            ("F1 / Esc", "Toggle Profiler Overlay  /  Main Menu"),
        ];
        for (i, (k, v)) in keys.iter().enumerate() {
            let y = py + 72.0 + i as f32 * 27.5;
            cx.rect(px + 32.0, y - 15.0, 118.0, 21.0, 4.0, Color::new(0.85, 0.86, 0.88, 0.92));
            cx.text_c(true, k, px + 91.0, y, 12.0, DARK);
            cx.text(false, v, px + 162.0, y, 14.0, WHITE);
        }
        let hx = px + 510.0;
        cx.text(true, "DEDICATED SERVER CONSOLE (STDIN)", hx, py + 40.0, 16.0, ORANGE);
        let steps = [
            "Run `run_server.bat` to launch headless server (UDP 30814).",
            "Type commands directly into the server terminal:",
            "  op <player> / deop <player>",
            "  build allow <player> / build deny <player>",
            "  time set <day|night|sunset|sunrise|0..24>",
            "  weather set <clear|wet|rain|fog>",
            "  map set <name> / map list",
            "  kick <player> / ban <player> / unban <player>",
            "  whitelist <on|off|add|remove|list>",
            "  save / stop / say <msg> / list / help",
            "World edits by authorized builders are saved & synced live!",
        ];
        for (i, l) in steps.iter().enumerate() {
            cx.text(false, l, hx, py + 74.0 + i as f32 * 29.0, 14.0, Color::new(0.90, 0.93, 0.98, 0.90));
        }
        if cx.button("Back", 640.0 - 100.0, py + ph + 18.0, 200.0, 44.0, Btn::Primary, true) { self.go(Screen::Main); }
        if cx.live && is_key_pressed(KeyCode::Escape) { self.go(Screen::Main); }
    }

    fn mods_screen(&mut self, cx: &Cx, menu: &Menu) {
        self.header(cx, "Repository & Add-ons", "Custom .zip / .glb Vehicle & Map Expansion Packages", 190.0, 86.0, 900.0);
        let (px, py, pw, ph) = (190.0, 124.0, 900.0, 430.0);
        self.panel(cx, px, py, pw, ph);
        if menu.mods.is_empty() {
            cx.text_c(true, "5 Production High-Detail Vehicles + 3 Maps + Studio Ready", px + pw / 2.0, py + 165.0, 24.0, WHITE);
            cx.text_c(false, "Drop external vehicle or map packages (.zip / .glb) into the mods/ directory", px + pw / 2.0, py + 204.0, 16.0, Color::new(1.0, 1.0, 1.0, 0.55));
        } else {
            cx.text(true, &format!("{} mod(s) detected", menu.mods.len()), px + 36.0, py + 44.0, 17.0, ORANGE);
            for (i, m) in menu.mods.iter().take(8).enumerate() {
                let y = py + 62.0 + i as f32 * 44.0;
                cx.rect(px + 24.0, y, pw - 48.0, 38.0, 8.0, Color::new(1.0, 1.0, 1.0, 0.05));
                let kind = if matches!(m.kind, crate::mods::ModKind::Car) { "Vehicle" } else { "Map" };
                let t = cx.fit(true, &m.title, 17.0, 520.0);
                cx.text(true, &t, px + 40.0, y + 25.0, 17.0, WHITE);
                cx.text_r(false, &format!("{}  -  {}", kind, m.author), px + pw - 40.0, y + 25.0, 14.0, Color::new(0.55, 0.85, 1.0, 0.9));
            }
        }
        if cx.button("Back", 640.0 - 100.0, py + ph + 20.0, 200.0, 44.0, Btn::Primary, true) { self.go(Screen::Main); }
        if cx.live && is_key_pressed(KeyCode::Escape) { self.go(Screen::Main); }
    }

    /// Bottom-left slanted version pill + Bottom-right slanted navigation hint pill matching `image_e59174.jpg`
    fn bottom_bar(&self, cx: &Cx) {
        let top = (screen_height() - cx.oy) / cx.u - 34.0;
        let lx = -cx.ox / cx.u + 16.0;
        let rx = (screen_width() - cx.ox) / cx.u - 16.0;

        // Bottom-left slanted pill: `Alpha v0.4.0`
        let ver_str = format!("Alpha v{}", VERSION);
        let vw = cx.width(true, &ver_str, 13.0) + 46.0;
        cx.para(lx, top, vw, 28.0, 8.0, Color::new(0.05, 0.06, 0.08, 0.88));
        cx.para(lx, top, 4.0, 28.0, 8.0, ORANGE);
        cx.para(lx + vw - 4.0, top, 4.0, 28.0, 8.0, ORANGE);
        cx.text(true, &ver_str, lx + 20.0, top + 19.0, 13.0, WHITE);

        // Bottom-right slanted navigation hints pill
        let hints: Vec<(&str, &str)> = if self.join.is_some() {
            vec![("ESC", "Cancel")]
        } else if self.dialog.is_some() {
            vec![("TAB", "Next field"), ("ENTER", "Confirm"), ("ESC", "Cancel")]
        } else {
            match self.screen {
                Screen::Main => vec![("ARROWS / D-PAD", "Navigate"), ("ENTER / A", "OK / Primary action"), ("ESC / B", "Back / Cancel")],
                Screen::Freeroam => vec![("ARROWS", "Navigate"), ("ENTER", "Start driving"), ("F", "Favourite"), ("TAB", "Map tab"), ("ESC", "Back")],
                Screen::Multiplayer => vec![("UP/DOWN", "Select"), ("ENTER", "Join"), ("F5", "Refresh"), ("DEL", "Remove"), ("ESC", "Back")],
                _ => vec![("ESC", "Back")],
            }
        };
        let mut total_hw = 28.0f32;
        for (k, l) in &hints {
            total_hw += cx.width(true, k, 11.0) + 14.0 + cx.width(false, l, 12.5) + 22.0;
        }
        let hx0 = rx - total_hw;
        cx.para(hx0, top, total_hw, 28.0, 8.0, Color::new(0.05, 0.06, 0.08, 0.88));
        cx.para(hx0, top, 4.0, 28.0, 8.0, ORANGE);

        let mut x = rx - 12.0;
        for (k, l) in hints.iter().rev() {
            let lw = cx.width(false, l, 12.5);
            cx.text_r(false, l, x, top + 19.0, 12.5, Color::new(1.0, 1.0, 1.0, 0.90));
            x -= lw + 6.0;
            let kw = cx.width(true, k, 11.0) + 12.0;
            cx.rect(x - kw, top + 5.0, kw, 18.0, 3.0, Color::new(0.85, 0.86, 0.88, 0.95));
            cx.text_c(true, k, x - kw / 2.0, top + 18.0, 11.0, DARK);
            x -= kw + 16.0;
        }
    }

    fn dialog_ui(&mut self, res: &Res, dt: f32) {
        let Some(mut d) = self.dialog.take() else { return };
        let dx = Cx::new(res, true);
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.62));
        let esc = is_key_pressed(KeyCode::Escape);
        let enter = is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter);
        let tab = is_key_pressed(KeyCode::Tab) || is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::Up);
        let mut out = Out::None;
        match &mut d {
            Dialog::Server { edit, name, addr, field, err } => {
                let (bx, by, bw, bh) = (360.0, 190.0, 560.0, 350.0);
                modal(&dx, bx, by, bw, bh, if edit.is_some() { "Edit Server" } else { "Add Server" });
                if tab { *field = 1 - *field; }
                let c0 = input_box(&dx, "SERVER NAME", name, "My CrashNG Server", bx + 30.0, by + 78.0, bw - 60.0, *field == 0);
                let c1 = input_box(&dx, "SERVER ADDRESS  (IP or domain)", addr, "127.0.0.1:30814", bx + 30.0, by + 166.0, bw - 60.0, *field == 1);
                if c0 { *field = 0; }
                if c1 { *field = 1; }
                if *field == 0 { edit_text(name, 40, &mut self.bs_hold, dt); } else { edit_text(addr, 80, &mut self.bs_hold, dt); }
                dx.text(false, "Port is optional (default 30814).", bx + 30.0, by + 252.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.45));
                if !err.is_empty() { dx.text(true, err, bx + 30.0, by + 276.0, 15.0, Color::new(0.95, 0.40, 0.35, 1.0)); }
                let submit_key = enter && *field == 1;
                if enter && *field == 0 { *field = 1; }
                let ok = dx.button(if edit.is_some() { "Save" } else { "Add Server" }, bx + bw - 30.0 - 150.0 - 12.0 - 170.0, by + bh - 66.0, 170.0, 44.0, Btn::Primary, true);
                let cancel = dx.button("Cancel", bx + bw - 30.0 - 150.0, by + bh - 66.0, 150.0, 44.0, Btn::Normal, true);
                if cancel { out = Out::Close; }
                if ok || submit_key {
                    match normalize_address(addr) {
                        Err(e) => *err = e,
                        Ok(a) => {
                            let n = sanitize_name(name, 40);
                            out = Out::Save(*edit, if n.is_empty() { a.clone() } else { n }, a);
                        }
                    }
                }
            }
            Dialog::Direct { addr, err } => {
                let (bx, by, bw, bh) = (360.0, 220.0, 560.0, 270.0);
                modal(&dx, bx, by, bw, bh, "Direct Connection");
                input_box(&dx, "SERVER ADDRESS  (IP or domain)", addr, "127.0.0.1:30814", bx + 30.0, by + 78.0, bw - 60.0, true);
                edit_text(addr, 80, &mut self.bs_hold, dt);
                dx.text(false, "Port is optional (default 30814).", bx + 30.0, by + 164.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.45));
                if !err.is_empty() { dx.text(true, err, bx + 30.0, by + 188.0, 15.0, Color::new(0.95, 0.40, 0.35, 1.0)); }
                let ok = dx.button("Connect", bx + bw - 30.0 - 150.0 - 12.0 - 170.0, by + bh - 66.0, 170.0, 44.0, Btn::Primary, true);
                let cancel = dx.button("Cancel", bx + bw - 30.0 - 150.0, by + bh - 66.0, 150.0, 44.0, Btn::Normal, true);
                if cancel { out = Out::Close; }
                if ok || enter {
                    match normalize_address(addr) { Err(e) => *err = e, Ok(a) => out = Out::Join(a) }
                }
            }
            Dialog::Delete(i) => {
                let (bx, by, bw, bh) = (360.0, 240.0, 560.0, 230.0);
                modal(&dx, bx, by, bw, bh, "Remove Server");
                let nm = self.servers.get(*i).map(|s| s.name.clone()).unwrap_or_default();
                let nm = dx.fit(true, &nm, 19.0, bw - 80.0);
                dx.text(false, "Are you sure you want to remove this server from your list?", bx + 30.0, by + 88.0, 16.0, Color::new(1.0, 1.0, 1.0, 0.8));
                dx.text(true, &nm, bx + 30.0, by + 122.0, 19.0, ORANGE);
                let ok = dx.button("Remove", bx + bw - 30.0 - 150.0 - 12.0 - 150.0, by + bh - 66.0, 150.0, 44.0, Btn::Danger, true);
                let cancel = dx.button("Cancel", bx + bw - 30.0 - 150.0, by + bh - 66.0, 150.0, 44.0, Btn::Normal, true);
                if cancel { out = Out::Close; }
                if ok || enter { out = Out::Remove(*i); }
            }
            Dialog::Username { buf, err } => {
                let (bx, by, bw, bh) = (360.0, 220.0, 560.0, 270.0);
                modal(&dx, bx, by, bw, bh, "Change Username");
                input_box(&dx, "USERNAME  (shown to other players)", buf, "Player", bx + 30.0, by + 78.0, bw - 60.0, true);
                edit_text(buf, 20, &mut self.bs_hold, dt);
                if !err.is_empty() { dx.text(true, err, bx + 30.0, by + 168.0, 15.0, Color::new(0.95, 0.40, 0.35, 1.0)); }
                let ok = dx.button("Save", bx + bw - 30.0 - 150.0 - 12.0 - 150.0, by + bh - 66.0, 150.0, 44.0, Btn::Primary, true);
                let cancel = dx.button("Cancel", bx + bw - 30.0 - 150.0, by + bh - 66.0, 150.0, 44.0, Btn::Normal, true);
                if cancel { out = Out::Close; }
                if ok || enter {
                    let n = sanitize_name(buf, 20);
                    if n.is_empty() { *err = "Username cannot be empty".into(); } else { out = Out::Name(n); }
                }
            }
            Dialog::HeavyMapWarning { map_idx, map_title } => {
                let (bx, by, bw, bh) = (260.0, 165.0, 760.0, 390.0);
                modal(&dx, bx, by, bw, bh, "HEAVY MAP HARDWARE WARNING");
                dx.text(true, &format!("Selected Map: {}", map_title), bx + 30.0, by + 84.0, 19.0, ORANGE);
                dx.text(false, "This massive open-world environment contains dense high-polygon geometry and high-resolution", bx + 30.0, by + 114.0, 15.0, Color::new(0.92, 0.94, 0.98, 0.90));
                dx.text(false, "textures. Please verify that your system meets the hardware requirements below before loading:", bx + 30.0, by + 136.0, 15.0, Color::new(0.92, 0.94, 0.98, 0.90));

                // Minimum vs Recommended Hardware Specification Table
                dx.rect(bx + 30.0, by + 156.0, 340.0, 132.0, 8.0, Color::new(0.05, 0.06, 0.09, 0.95));
                dx.outline(bx + 30.0, by + 156.0, 340.0, 132.0, 1.5, Color::new(0.95, 0.55, 0.15, 0.65));
                dx.text(true, "MINIMUM REQUIREMENTS", bx + 46.0, by + 180.0, 15.0, ORANGE);
                dx.text(false, "• RAM:  16 GB System Memory", bx + 46.0, by + 206.0, 14.0, WHITE);
                dx.text(false, "• GPU:  RTX 2060 / RX 5700 (6 GB VRAM)", bx + 46.0, by + 230.0, 14.0, WHITE);
                dx.text(false, "• CPU:  6-Core / 12-Thread (Core i5 / Ryzen 5)", bx + 46.0, by + 254.0, 14.0, WHITE);

                dx.rect(bx + 390.0, by + 156.0, 340.0, 132.0, 8.0, Color::new(0.05, 0.08, 0.07, 0.95));
                dx.outline(bx + 390.0, by + 156.0, 340.0, 132.0, 1.5, Color::new(0.25, 0.85, 0.45, 0.65));
                dx.text(true, "RECOMMENDED SPECIFICATIONS", bx + 406.0, by + 180.0, 15.0, GREEN);
                dx.text(false, "• RAM:  32 GB Dual-Channel DDR4/DDR5", bx + 406.0, by + 206.0, 14.0, WHITE);
                dx.text(false, "• GPU:  RTX 3080 / RTX 4070 / RX 6800 XT (12 GB)", bx + 406.0, by + 230.0, 14.0, WHITE);
                dx.text(false, "• CPU:  8-Core / 16-Thread (Core i7 / Ryzen 7)", bx + 406.0, by + 254.0, 14.0, WHITE);

                let ok = dx.button("I Understand - Select Map", bx + bw - 30.0 - 150.0 - 14.0 - 250.0, by + bh - 66.0, 250.0, 44.0, Btn::Primary, true);
                let cancel = dx.button("Cancel", bx + bw - 30.0 - 150.0, by + bh - 66.0, 150.0, 44.0, Btn::Normal, true);
                if cancel { out = Out::Close; }
                if ok || enter { out = Out::ConfirmHeavyMap(*map_idx); }
            }
        }
        if esc { out = Out::Close; }
        match out {
            Out::None => self.dialog = Some(d),
            Out::Close => {}
            Out::Save(edit, n, a) => {
                match edit {
                    Some(i) if i < self.servers.len() => {
                        self.servers[i] = ServerEntry { name: n, address: a.clone() };
                        query_status(a, self.status[i].clone());
                        self.msel = i;
                    }
                    _ => {
                        self.servers.push(ServerEntry { name: n, address: a.clone() });
                        let st = Arc::new(Mutex::new(Status::Pending));
                        query_status(a, st.clone());
                        self.status.push(st);
                        self.msel = self.servers.len() - 1;
                    }
                }
                save_servers(&self.servers);
                self.fix_scroll();
            }
            Out::Join(a) => self.start_join(a.clone(), a),
            Out::Remove(i) => {
                if i < self.servers.len() {
                    self.servers.remove(i);
                    self.status.remove(i);
                    save_servers(&self.servers);
                    self.msel = self.msel.min(self.servers.len().saturating_sub(1));
                    self.fix_scroll();
                }
            }
            Out::Name(n) => {
                self.settings.name = n;
                save_settings(&self.settings);
            }
            Out::ConfirmHeavyMap(_idx) => {
                self.info("Heavy map selected and hardware requirements acknowledged.");
            }
        }
    }

    fn join_ui(&mut self, res: &Res) -> FrontAction {
        let Some(j) = self.join.take() else { return FrontAction::None };
        match j.net.status() {
            NetStatus::Connected { .. } => return FrontAction::Connected(j.net),
            NetStatus::Error(e) => {
                j.net.disconnect();
                let msg = if e.contains("unknown host") { "Unknown host - check the IP address or domain name".to_string() } else { e };
                self.error(&msg);
                return FrontAction::None;
            }
            NetStatus::Disconnected => return FrontAction::None,
            NetStatus::Connecting => {}
        }
        if j.started.elapsed().as_secs_f32() > 8.0 {
            j.net.disconnect();
            self.error("No response from the server - check it is running and UDP port 30814 is open");
            return FrontAction::None;
        }
        let dx = Cx::new(res, true);
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.62));
        let (bx, by, bw, bh) = (440.0, 250.0, 400.0, 210.0);
        modal(&dx, bx, by, bw, bh, "Connecting...");
        let label = dx.fit(false, &j.label, 17.0, bw - 120.0);
        dx.text(false, &label, bx + 90.0, by + 96.0, 17.0, WHITE);
        dx.text(false, "Synchronizing authoritative world & map...", bx + 90.0, by + 120.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.5));
        let tt = get_time() as f32;
        for n in 0..8 {
            let a = n as f32 * std::f32::consts::FRAC_PI_4;
            let phase = ((tt * 8.0 - n as f32).rem_euclid(8.0)) / 8.0;
            dx.circle(bx + 50.0 + a.cos() * 18.0, by + 100.0 + a.sin() * 18.0, 3.6, Color::new(0.97, 0.50, 0.10, 1.0 - phase * 0.85));
        }
        let cancel = dx.button("Cancel", bx + bw - 30.0 - 130.0, by + bh - 62.0, 130.0, 40.0, Btn::Normal, true);
        if cancel || is_key_pressed(KeyCode::Escape) {
            j.net.disconnect();
            return FrontAction::None;
        }
        self.join = Some(j);
        FrontAction::None
    }
}

const CARD_W: f32 = 189.0;
const CARD_H: f32 = 112.0;
const CARD_GAP: f32 = 5.0;
const COLS: usize = 5;
const VIEW_TOP: f32 = 83.0;
const VIEW_BOT: f32 = 666.0;

fn grad_rect(cx: &Cx, x: f32, y: f32, w: f32, h: f32, top: Color, bot: Color) {
    let v = |px: f32, py: f32, c: Color| Vertex::new(cx.x(px), cx.y(py), 0.0, 0.0, 0.0, c);
    draw_mesh(&Mesh {
        vertices: vec![v(x, y, top), v(x + w, y, top), v(x + w, y + h, bot), v(x, y + h, bot)],
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: None,
    });
}

fn clip(cx: &Cx, x: f32, y: f32, w: f32, h: f32) {
    let gl = unsafe { get_internal_gl() };
    let (px, py, pw, ph) = (cx.x(x), cx.y(y), cx.s(w), cx.s(h));
    gl.quad_gl.scissor(Some((px as i32, (screen_height() - py - ph) as i32, pw as i32, ph as i32)));
}

fn unclip() {
    let gl = unsafe { get_internal_gl() };
    gl.quad_gl.scissor(None);
}

fn star(cx: &Cx, x: f32, y: f32, r: f32, c: Color) {
    let pts: Vec<Vec2> = (0..10).map(|i| {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        let rr = if i % 2 == 0 { r } else { r * 0.42 };
        vec2(cx.x(x + a.cos() * rr), cx.y(y + a.sin() * rr))
    }).collect();
    let ctr = vec2(cx.x(x), cx.y(y));
    for i in 0..10 { draw_triangle(ctr, pts[i], pts[(i + 1) % 10], c); }
}

fn check(cx: &Cx, x: f32, y: f32, c: Color) {
    cx.line(x - 4.0, y, x - 1.0, y + 3.5, 2.0, c);
    cx.line(x - 1.0, y + 3.5, x + 5.0, y - 4.0, 2.0, c);
}

fn node_icon(cx: &Cx, x: f32, y: f32, c: Color) {
    cx.line(x - 4.0, y, x + 3.0, y - 4.0, 1.2, c);
    cx.line(x - 4.0, y, x + 3.0, y + 4.0, 1.2, c);
    cx.circle(x - 4.0, y, 2.2, c);
    cx.circle(x + 3.0, y - 4.0, 2.2, c);
    cx.circle(x + 3.0, y + 4.0, 2.2, c);
}

impl Front {
    fn ensure_thumbs(&mut self, res: &Res) {
        if self.thumbs.len() < SPECS.len() {
            self.thumbs.resize(SPECS.len(), None);
        }
        let mut made = 0;
        for i in 0..SPECS.len() {
            if self.thumbs[i].is_some() { continue; }
            // Skip heavy Scania Bus GLB decoding on main menu until Freeroam/Garage is opened
            if SPECS[i].hi_kind == HiKind::CrafterBus && self.screen == Screen::Main {
                continue;
            }
            if made >= 1 { break; }
            let rt = render_target_ex(420, 210, RenderTargetParams { sample_count: 1, depth: true });
            rt.texture.set_filter(FilterMode::Linear);
            set_camera(&Camera2D { render_target: Some(rt.clone()), zoom: vec2(1.0, 1.0), target: vec2(0.0, 0.0), ..Default::default() });
            clear_background(Color::new(0.08, 0.11, 0.16, 1.0));
            let mut car = Car::new(i, res.tex.clone(), false);
            car.respawn(vec3(0.0, 0.0, 0.0), 0.0);
            let dist = if SPECS[i].hi_kind == HiKind::CrafterBus { 13.5 } else { 5.4 };
            let focus = vec3(0.0, if SPECS[i].hi_kind == HiKind::CrafterBus { 1.35 } else { 0.55 }, 0.0);
            let yaw = 0.62f32;
            let cpos = focus + vec3(yaw.sin() * dist, dist * 0.22, yaw.cos() * dist);
            let vp = Mat4::perspective_rh_gl(30f32.to_radians(), 420.0 / 210.0, 0.25, 200.0) * Mat4::look_at_rh(cpos, focus, Vec3::Y);
            crate::lit_begin(res, vp, cpos, 0.0);
            crate::draw_car_lit(&mut car, res);
            gl_use_default_material();
            set_default_camera();
            self.thumbs[i] = Some(rt);
            made += 1;
        }
    }

    fn is_fav(&self, name: &str) -> bool { self.settings.favs.iter().any(|f| f == name) }

    fn toggle_fav(&mut self, name: &str) {
        if let Some(p) = self.settings.favs.iter().position(|f| f == name) { self.settings.favs.remove(p); }
        else { self.settings.favs.push(name.to_string()); }
        save_settings(&self.settings);
    }

    fn car_passes(&self, menu: &Menu, i: usize) -> bool {
        let c = &menu.cars[i];
        let q = self.search.trim().to_lowercase();
        if !q.is_empty() && !c.name.to_lowercase().contains(&q) { return false; }
        match &c.kind {
            CarKind::Builtin(s) => {
                if !self.f_builtin { return false; }
                let ok = match SPECS[*s].drive { Drive::Rwd => self.f_rwd, Drive::Fwd => self.f_fwd, Drive::Awd => self.f_awd };
                if !ok { return false; }
            }
            CarKind::Mod { .. } => { if !self.f_mods { return false; } }
        }
        !(self.f_fav_only && !self.is_fav(&c.name))
    }

    fn begin_drive(&mut self, menu: &Menu) -> FrontAction {
        let name = menu.cars[menu.sel_car].name.clone();
        self.settings.recent.retain(|n| n != &name);
        self.settings.recent.insert(0, name);
        self.settings.recent.truncate(5);
        save_settings(&self.settings);
        FrontAction::Start
    }

    fn chip(&mut self, cx: &Cx, label: &str, x: f32, y: f32, w: f32, on: bool) -> bool {
        let hover = cx.live && cx.hit(x, y, w, 20.0);
        cx.rect(x, y, w, 20.0, 3.0, if hover { Color::new(0.20, 0.25, 0.32, 0.98) } else { Color::new(0.13, 0.17, 0.23, 0.96) });
        cx.text(false, label, x + 9.0, y + 14.5, 12.5, if on { WHITE } else { Color::new(1.0, 1.0, 1.0, 0.45) });
        if on { check(cx, x + w - 14.0, y + 10.0, WHITE); }
        hover && cx.click
    }

    fn vehicles_screen(&mut self, cx: &Cx, menu: &mut Menu, dt: f32) -> FrontAction {
        let mut action = FrontAction::None;
        let dark = Color::new(0.04, 0.05, 0.07, 0.96);

        let bh = cx.live && cx.hit(13.0, 12.0, 62.0, 26.0);
        cx.rect(13.0, 12.0, 62.0, 26.0, 4.0, if bh { Color::new(0.16, 0.19, 0.25, 1.0) } else { dark });
        cx.text(true, "< Back", 22.0, 30.0, 13.0, WHITE);
        let crumb = "Freeroam   /   Vehicle Roster & Map Selection";
        let cw = cx.width(false, crumb, 13.0) + 24.0;
        cx.rect(78.0, 12.0, cw, 26.0, 4.0, dark);
        cx.text(false, crumb, 90.0, 30.0, 13.0, WHITE);
        if (bh && cx.click) || (cx.live && !self.search_focus && is_key_pressed(KeyCode::Escape)) {
            self.go(Screen::Main);
            return FrontAction::None;
        }

        let vis_all: Vec<usize> = (0..menu.cars.len()).filter(|&i| self.car_passes(&*menu, i)).collect();
        let recent: Vec<usize> = self.settings.recent.iter()
            .filter_map(|n| menu.cars.iter().position(|c| &c.name == n))
            .filter(|&i| self.car_passes(&*menu, i)).collect();
        let favs: Vec<usize> = vis_all.iter().copied().filter(|&i| self.is_fav(&menu.cars[i].name)).collect();
        let mut secs: Vec<(&str, Vec<usize>)> = Vec::new();
        if !recent.is_empty() { secs.push(("Recent", recent)); }
        if !favs.is_empty() { secs.push(("Favourites", favs)); }
        secs.push(("Official High-Detail Production Fleet", vis_all.clone()));

        if cx.live && !self.search_focus && !vis_all.is_empty() {
            let pos = vis_all.iter().position(|&i| i == menu.sel_car);
            let mut np = pos.unwrap_or(0);
            let n = vis_all.len();
            if pos.is_none() { menu.sel_car = vis_all[0]; self.vfollow = true; }
            if is_key_pressed(KeyCode::Right) { np = (np + 1).min(n - 1); }
            if is_key_pressed(KeyCode::Left) { np = np.saturating_sub(1); }
            if is_key_pressed(KeyCode::Down) { np = (np + COLS).min(n - 1); }
            if is_key_pressed(KeyCode::Up) { np = np.saturating_sub(COLS); }
            if Some(np) != pos && pos.is_some() { menu.sel_car = vis_all[np]; self.vfollow = true; }
            if is_key_pressed(KeyCode::Tab) { self.rtab = 1 - self.rtab; }
            if is_key_pressed(KeyCode::F) { let nm = menu.cars[menu.sel_car].name.clone(); self.toggle_fav(&nm); }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) { action = self.begin_drive(&*menu); }
        } else if cx.live && self.search_focus && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Escape)) {
            self.search_focus = false;
        }

        let (x0, top) = (21.0, VIEW_TOP + 6.0);
        let mut y = top;
        let mut sec_y: Vec<f32> = Vec::new();
        let mut sel_y: Option<f32> = None;
        for (si, (_, items)) in secs.iter().enumerate() {
            sec_y.push(y);
            y += 26.0;
            if si + 1 == secs.len() {
                if let Some(p) = items.iter().position(|&i| i == menu.sel_car) { sel_y = Some(y + (p / COLS) as f32 * (CARD_H + CARD_GAP)); }
            }
            y += ((items.len() + COLS - 1) / COLS) as f32 * (CARD_H + CARD_GAP) + 6.0;
        }
        let max_scroll = (y + 4.0 - VIEW_BOT).max(0.0);
        if self.vfollow {
            if let Some(sy) = sel_y {
                if sy - self.vscroll < top { self.vscroll = sy - top - 22.0; }
                if sy + CARD_H - self.vscroll > VIEW_BOT - 6.0 { self.vscroll = sy + CARD_H - VIEW_BOT + 10.0; }
            }
            self.vfollow = false;
        }
        let (_, wh) = mouse_wheel();
        if wh != 0.0 && cx.live && cx.hit(13.0, VIEW_TOP, 980.0, VIEW_BOT - VIEW_TOP) { self.vscroll -= wh.signum() * 64.0; }
        self.vscroll = self.vscroll.clamp(0.0, max_scroll);
        let sc = self.vscroll;

        cx.rect(13.0, 45.0, 980.0, 621.0, 0.0, Color::new(0.06, 0.08, 0.11, 0.80));
        clip(cx, 13.0, VIEW_TOP, 980.0, VIEW_BOT - VIEW_TOP);
        let mut clicked: Option<usize> = None;
        let mut rclicked: Option<usize> = None;
        for (si, (title, items)) in secs.iter().enumerate() {
            let sy = sec_y[si] - sc;
            if sy > VIEW_BOT || sy + 26.0 + ((items.len() + COLS - 1) / COLS) as f32 * (CARD_H + CARD_GAP) < VIEW_TOP { continue; }
            cx.para(x0 - 2.0, sy + 4.0, 5.0, 15.0, 3.0, ORANGE);
            cx.text(true, title, x0 + 13.0, sy + 17.0, 15.0, WHITE);
            let tw = cx.width(true, title, 15.0);
            cx.rect(x0 + 24.0 + tw, sy + 12.0, 980.0 - 40.0 - tw - 24.0, 1.0, 0.0, Color::new(1.0, 1.0, 1.0, 0.16));
            if items.is_empty() { continue; }
            for (k, &ci) in items.iter().enumerate() {
                let (cxp, cyp) = (x0 + (k % COLS) as f32 * (CARD_W + CARD_GAP), sy + 26.0 + (k / COLS) as f32 * (CARD_H + CARD_GAP));
                if cyp > VIEW_BOT || cyp + CARD_H < VIEW_TOP { continue; }
                let hover = cx.live && cx.hit(cxp, cyp, CARD_W, CARD_H) && cx.my >= cx.y(VIEW_TOP) && cx.my <= cx.y(VIEW_BOT);
                if hover && cx.click { clicked = Some(ci); }
                if hover && is_mouse_button_pressed(MouseButton::Right) { rclicked = Some(ci); }
                let sel = ci == menu.sel_car;
                let tb = CARD_H - 21.0;
                grad_rect(cx, cxp, cyp, CARD_W, tb, if hover || sel { Color::new(0.36, 0.43, 0.52, 0.96) } else { Color::new(0.29, 0.35, 0.43, 0.94) }, if hover || sel { Color::new(0.20, 0.24, 0.30, 0.96) } else { Color::new(0.15, 0.18, 0.23, 0.94) });
                let opt = &menu.cars[ci];
                let tex: Option<Texture2D> = match &opt.kind {
                    CarKind::Builtin(s) => self.thumbs.get(*s).and_then(|r| r.as_ref()).map(|r| r.texture.clone()),
                    CarKind::Mod { .. } => opt.thumb.clone(),
                };
                if let Some(t) = tex {
                    let flip = matches!(&opt.kind, CarKind::Builtin(_));
                    draw_texture_ex(&t, cx.x(cxp), cx.y(cyp), WHITE, DrawTextureParams { dest_size: Some(vec2(cx.s(CARD_W), cx.s(tb))), flip_y: flip, ..Default::default() });
                } else {
                    cx.text_c(true, "3D MODEL READY", cxp + CARD_W / 2.0, cyp + tb / 2.0 + 5.0, 12.0, Color::new(1.0, 1.0, 1.0, 0.45));
                }
                let badge = match &opt.kind { CarKind::Builtin(s) => match SPECS[*s].drive { Drive::Rwd => "RWD", Drive::Fwd => "FWD", Drive::Awd => "AWD" }, CarKind::Mod { .. } => "MOD" };
                cx.text_r(false, badge, cxp + CARD_W - 7.0, cyp + 15.0, 12.0, Color::new(0.90, 0.93, 0.97, 0.85));
                if self.is_fav(&opt.name) { star(cx, cxp + 14.0, cyp + tb - 14.0, 8.0, Color::new(1.0, 0.80, 0.15, 1.0)); }
                cx.rect(cxp, cyp + tb, CARD_W, 21.0, 0.0, dark);
                let nm = cx.fit(false, &opt.name, 13.0, CARD_W - 40.0);
                cx.text(false, &nm, cxp + 8.0, cyp + tb + 15.0, 13.0, WHITE);
                node_icon(cx, cxp + CARD_W - 14.0, cyp + tb + 10.5, Color::new(1.0, 1.0, 1.0, 0.85));
                if sel { cx.outline(cxp, cyp, CARD_W, CARD_H, 2.0, ORANGE); }
            }
        }
        unclip();
        if max_scroll > 0.0 {
            let th = (VIEW_BOT - VIEW_TOP) * (VIEW_BOT - VIEW_TOP) / (VIEW_BOT - VIEW_TOP + max_scroll);
            let ty = VIEW_TOP + (VIEW_BOT - VIEW_TOP - th) * sc / max_scroll;
            cx.rect(986.0, ty, 4.0, th, 2.0, ORANGE);
        }
        cx.rect(13.0, 45.0, 980.0, 38.0, 0.0, Color::new(0.03, 0.04, 0.06, 0.98));
        cx.para(14.0, 48.0, 11.0, 32.0, 8.0, ORANGE);
        cx.text(true, "All Vehicles", 42.0, 74.0, 27.0, WHITE);
        cx.text_r(false, &format!("{} shown", vis_all.len()), 980.0, 70.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.5));

        if let Some(i) = clicked {
            let now = get_time();
            let dbl = self.last_click.0 == i && now - self.last_click.1 < 0.45;
            menu.sel_car = i;
            self.last_click = (i, now);
            self.search_focus = false;
            if dbl { action = self.begin_drive(&*menu); }
        }
        if let Some(i) = rclicked { let nm = menu.cars[i].name.clone(); self.toggle_fav(&nm); }

        let (rx, rw) = (1001.0, 266.0);
        cx.rect(rx, 45.0, rw, 621.0, 0.0, Color::new(0.06, 0.08, 0.11, 0.88));
        let arrow = |cx: &Cx, s: &str, x: f32| -> bool {
            let h = cx.live && cx.hit(x - 12.0, 50.0, 26.0, 24.0);
            cx.text_c(true, s, x, 70.0, 22.0, if h { ORANGE } else { WHITE });
            h && cx.click
        };
        if arrow(cx, "<", rx + 28.0) || arrow(cx, ">", rx + rw - 28.0) { self.rtab = 1 - self.rtab; }
        for t in 0..2usize {
            let tx = rx + 66.0 + t as f32 * 70.0;
            let on = self.rtab == t;
            let hv = cx.live && cx.hit(tx, 50.0, 64.0, 26.0);
            cx.rect(tx, 50.0, 64.0, 24.0, 3.0, if on || hv { Color::new(0.14, 0.18, 0.24, 0.95) } else { Color::new(0.0, 0.0, 0.0, 0.0) });
            if on { cx.rect(tx + 6.0, 72.0, 52.0, 3.0, 0.0, ORANGE); }
            let ic = if on { WHITE } else { Color::new(1.0, 1.0, 1.0, 0.55) };
            if t == 0 {
                for l in 0..3 { cx.rect(tx + 22.0, 57.0 + l as f32 * 6.0, 20.0, 2.5, 1.0, ic); }
            } else {
                cx.circle(tx + 32.0, 60.0, 6.0, ic);
                draw_triangle(vec2(cx.x(tx + 27.0), cx.y(63.0)), vec2(cx.x(tx + 37.0), cx.y(63.0)), vec2(cx.x(tx + 32.0), cx.y(72.0)), ic);
            }
            if hv && cx.click { self.rtab = t; }
        }

        let cur = menu.cars[menu.sel_car].name.clone();
        if self.rtab == 0 {
            let sx = rx + 5.0;
            cx.rect(sx, 89.0, 200.0, 24.0, 2.0, Color::new(0.03, 0.04, 0.06, 0.98));
            cx.outline(sx, 89.0, 200.0, 24.0, 1.5, if self.search_focus { ORANGE } else { Color::new(1.0, 1.0, 1.0, 0.08) });
            if self.search.is_empty() { cx.text(false, "Search...", sx + 6.0, 105.0, 13.0, Color::new(1.0, 1.0, 1.0, 0.45)); }
            else {
                let mut shown = self.search.clone();
                while cx.width(false, &shown, 13.0) > 180.0 && shown.chars().count() > 1 { shown.remove(0); }
                cx.text(false, &shown, sx + 6.0, 105.0, 13.0, WHITE);
            }
            if cx.live && cx.click && cx.hit(sx, 89.0, 200.0, 24.0) { self.search_focus = true; flush_chars(); }
            else if cx.live && cx.click && !cx.hit(sx, 89.0, 200.0, 24.0) { self.search_focus = false; }
            if self.search_focus && cx.live {
                let before = self.search.clone();
                edit_text(&mut self.search, 30, &mut self.bs_hold, dt);
                if before != self.search { self.vfollow = true; self.vscroll = 0.0; }
            }
            let mut yy = 128.0;
            let w2 = (rw - 10.0 - 5.0) / 2.0;
            let w3 = (rw - 10.0 - 10.0) / 3.0;
            if self.chip(cx, "Built-in", rx + 5.0, yy, w2, self.f_builtin) { self.f_builtin = !self.f_builtin; self.vscroll = 0.0; }
            if self.chip(cx, "Mods", rx + 10.0 + w2, yy, w2, self.f_mods) { self.f_mods = !self.f_mods; self.vscroll = 0.0; }
            yy += 25.0;
            if self.chip(cx, "RWD", rx + 5.0, yy, w3, self.f_rwd) { self.f_rwd = !self.f_rwd; self.vscroll = 0.0; }
            if self.chip(cx, "FWD", rx + 10.0 + w3, yy, w3, self.f_fwd) { self.f_fwd = !self.f_fwd; self.vscroll = 0.0; }
            if self.chip(cx, "AWD", rx + 15.0 + 2.0 * w3, yy, w3, self.f_awd) { self.f_awd = !self.f_awd; self.vscroll = 0.0; }
            yy += 32.0;
            cx.text(false, "Current Vehicle:", rx + 8.0, yy + 12.0, 12.5, Color::new(1.0, 1.0, 1.0, 0.55));
            let cn = cx.fit(false, &cur, 12.5, rw - 130.0);
            cx.text_r(false, &cn, rx + rw - 8.0, yy + 12.0, 12.5, WHITE);
            yy += 26.0;
            if cx.button("Start Driving", rx + 5.0, yy, rw - 10.0, 38.0, Btn::Primary, menu.cars[menu.sel_car].playable) {
                action = self.begin_drive(&*menu);
            }
            yy += 46.0;
            if cx.button("Open in Garage & Tuning", rx + 5.0, yy, rw - 10.0, 34.0, Btn::Normal, true) {
                self.go(Screen::Garage);
            }
            yy += 48.0;
            if let CarKind::Builtin(s) = &menu.cars[menu.sel_car].kind {
                let sp = &SPECS[*s];
                cx.text(true, sp.name, rx + 8.0, yy + 12.0, 15.0, WHITE);
                cx.text(false, &format!("{:.0} kg  -  {}", sp.mass, match sp.drive { Drive::Rwd => "Rear-wheel drive", Drive::Fwd => "Front-wheel drive", Drive::Awd => "All-wheel drive" }), rx + 8.0, yy + 30.0, 12.5, Color::new(1.0, 1.0, 1.0, 0.6));
                let bars = [("POWER", sp.power / 3.0), ("GRIP", (sp.grip - 0.6) / 1.0), ("MASS", (sp.mass / 3500.0).min(1.0))];
                for (k, (l, v)) in bars.iter().enumerate() {
                    let by = yy + 46.0 + k as f32 * 22.0;
                    cx.text(false, l, rx + 8.0, by + 9.0, 12.0, Color::new(1.0, 1.0, 1.0, 0.65));
                    cx.rect(rx + 70.0, by, rw - 86.0, 9.0, 4.5, Color::new(1.0, 1.0, 1.0, 0.12));
                    cx.rect(rx + 70.0, by, (rw - 86.0) * v.clamp(0.05, 1.0), 9.0, 4.5, ORANGE);
                }
            }
        } else {
            cx.text(true, "OFFICIAL DEFAULT MAP", rx + 10.0, 100.0, 14.0, ORANGE);
            let mut my = 110.0;
            let mut pick: Option<usize> = None;
            for (i, mp) in menu.maps.iter().enumerate().take(7) {
                let hv = cx.live && cx.hit(rx + 5.0, my, rw - 10.0, 52.0);
                let sel = i == menu.sel_map;
                cx.rect(rx + 5.0, my, rw - 10.0, 52.0, 4.0, if sel { Color::new(0.97, 0.50, 0.10, 0.28) } else if hv { Color::new(1.0, 1.0, 1.0, 0.10) } else { Color::new(1.0, 1.0, 1.0, 0.05) });
                if sel { cx.outline(rx + 5.0, my, rw - 10.0, 52.0, 1.5, ORANGE); }
                let nm = cx.fit(true, &mp.name, 13.5, rw - 24.0);
                cx.text(true, &nm, rx + 12.0, my + 20.0, 13.5, WHITE);
                let sb = cx.fit(false, &mp.sub, 11.5, rw - 24.0);
                cx.text(false, &sb, rx + 12.0, my + 39.0, 11.5, Color::new(1.0, 1.0, 1.0, 0.55));
                if hv && cx.click { pick = Some(i); }
                my += 58.0;
            }
            if let Some(i) = pick {
                menu.sel_map = i;
                if menu.maps[i].heavy_warning {
                    let mtitle = menu.maps[i].name.clone();
                    self.open_dialog(Dialog::HeavyMapWarning { map_idx: i, map_title: mtitle });
                }
            }
            if cx.button("Start Driving", rx + 5.0, 612.0, rw - 10.0, 40.0, Btn::Primary, true) {
                action = self.begin_drive(&*menu);
            }
        }
        action
    }
}
