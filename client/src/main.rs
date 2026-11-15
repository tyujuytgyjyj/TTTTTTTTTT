#![allow(dead_code, unused_imports, unused_variables, unused_mut, unreachable_code, non_snake_case, clippy::all)]
mod bng;
mod bngcar;
mod bngvis;
mod dae;
mod editor;
mod envload;
mod hood;
mod level;
mod photo;
mod props;
mod water;
mod grass;
mod pyscript;
mod car;
mod hicar;
mod jbeam;
mod model;
mod mods;
mod soft;
mod world;
mod net;
mod servers;
mod ui;

use car::*;
use crashdrive_shared::VehicleSpawn;
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Comparison, Equation, PipelineParams, UniformDesc};
use macroquad::prelude::*;
use net::{
    apply_vehicle_state, apply_world_car_state, car_to_state, car_to_world_state,
    collide_local_vs_remote, start_net_client, NetStatus, SharedNet,
};
use std::collections::HashMap;
use std::sync::Arc;
use world::*;

const COMMON: &str = include_str!("../assets/shaders/common.glsl");

fn frag(body: &str) -> String { format!("#version 100\nprecision highp float;\n{}\n{}", COMMON, body) }

fn conf() -> Conf {
    // Default MSAA = 0 for weak-PC compatibility; pass `--msaa` for hardware 4x/8x MSAA
    let msaa = std::env::args().any(|a| a == "--msaa");
    Conf {
        window_title: servers::load_branding().game_title,
        window_width: 1280,
        window_height: 720,
        sample_count: if msaa { 4 } else { 0 },
        ..Default::default()
    }
}

struct Res {
    tex: Texture2D,
    font: Font,
    font_b: Font,
    shadow_tex: Texture2D,
    lit: Material,
    lit_cull: Material,
    sky: Material,
    shcast: Material,
    lit_glass: Material,
    shdec: Material,
    post: Material,
    rt: RenderTarget,
    scene_rt: RenderTarget,
    loading_shots: Vec<Texture2D>,
}

fn mat(vs: &str, fs: &str, uniforms: Vec<UniformDesc>, depth_test: Comparison, depth_write: bool, blend: bool) -> Material {
    load_material(
        ShaderSource::Glsl { vertex: vs, fragment: fs },
        MaterialParams {
            pipeline_params: PipelineParams {
                depth_test,
                depth_write,
                color_blend: if blend { Some(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::OneMinusValue(BlendValue::SourceAlpha))) } else { None },
                ..Default::default()
            },
            uniforms,
            textures: vec![],
        },
    )
    .expect("shader failed to compile")
}

fn load_res() -> Res {
    let tex = Texture2D::from_file_with_format(include_bytes!("../assets/cars/colormap.png"), Some(ImageFormat::Png));
    tex.set_filter(FilterMode::Nearest);
    let font = load_ttf_font_from_bytes(include_bytes!("../assets/fonts/Poppins-Medium.ttf")).expect("font");
    let font_b = load_ttf_font_from_bytes(include_bytes!("../assets/fonts/Poppins-Bold.ttf")).expect("font");
    let u = |n: &str, t: UniformType| UniformDesc::new(n, t);
    let lit_uniforms = || vec![
        u("VP", UniformType::Mat4),
        u("MODEL", UniformType::Mat4),
        u("CarM", UniformType::Mat4),
        u("tint", UniformType::Float3),
        u("sun_dir", UniformType::Float3),
        u("cam_pos", UniformType::Float3),
        u("fog_col", UniformType::Float3),
        u("misc", UniformType::Float4),
        u("head_dir", UniformType::Float4),
    ];
    let lit = mat(include_str!("../assets/shaders/lit.vert"), &frag(include_str!("../assets/shaders/lit.frag")),
        lit_uniforms(), Comparison::LessOrEqual, true, true);
    let lit_glass = mat(include_str!("../assets/shaders/lit.vert"), &frag(include_str!("../assets/shaders/lit.frag")),
        lit_uniforms(), Comparison::LessOrEqual, false, true);
    let lit_cull = load_material(
        ShaderSource::Glsl { vertex: include_str!("../assets/shaders/lit.vert"), fragment: &frag(include_str!("../assets/shaders/lit.frag")) },
        MaterialParams {
            pipeline_params: PipelineParams { depth_test: Comparison::LessOrEqual, depth_write: true, cull_face: macroquad::miniquad::CullFace::Back,
                color_blend: Some(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::OneMinusValue(BlendValue::SourceAlpha))), ..Default::default() },
            uniforms: lit_uniforms(),
            textures: vec![],
        }).expect("shader");
    let sky = mat(include_str!("../assets/shaders/sky.vert"), &frag(include_str!("../assets/shaders/sky.frag")),
        vec![u("invVP", UniformType::Mat4), u("sun_dir", UniformType::Float3), u("cam_pos", UniformType::Float3), u("misc", UniformType::Float4)],
        Comparison::Always, false, false);
    let shcast = mat(include_str!("../assets/shaders/shcast.vert"), &frag(include_str!("../assets/shaders/shcast.frag")),
        vec![u("SVP", UniformType::Mat4), u("MODEL", UniformType::Mat4), u("sun_dir", UniformType::Float3), u("misc", UniformType::Float4)],
        Comparison::Always, false, false);
    let shdec = mat(include_str!("../assets/shaders/shdec.vert"), &frag(include_str!("../assets/shaders/shdec.frag")),
        vec![u("VP", UniformType::Mat4), u("SVP", UniformType::Mat4), u("sh_col", UniformType::Float4)],
        Comparison::LessOrEqual, false, true);
    let post = mat(include_str!("../assets/shaders/post.vert"), include_str!("../assets/shaders/post.frag"),
        vec![u("pp_params", UniformType::Float4)], Comparison::Always, false, false);
    let rt = render_target(2048, 2048);
    rt.texture.set_filter(FilterMode::Linear);
    let scene_rt = render_target(1280, 720);
    scene_rt.texture.set_filter(FilterMode::Linear);
    Res { tex, font, font_b, shadow_tex: Texture2D::empty(), lit, lit_glass, lit_cull, sky, shcast, shdec, post, rt, scene_rt, loading_shots: Vec::new() }
}

#[derive(Clone, Copy)]
struct EnvLight {
    true_sun: Vec3,
    key_dir: Vec3,
    sh_dir: Vec3,
    fog: Vec3,
    sh_col: Vec4,
    head_pos: Vec3,
    head_fwd: Vec3,
}

fn env_light(hour: f32, head_pos: Vec3, head_fwd: Vec3) -> EnvLight {
    let ang = ((hour - 6.0) / 24.0) * std::f32::consts::TAU;
    let elev = ang.sin() * 0.88;
    let horiz = ang.cos();
    let true_sun = vec3(horiz * 0.92, elev, 0.38).normalize();
    let sy = true_sun.y;
    let day_f = ((sy + 0.08) / 0.36).clamp(0.0, 1.0);
    let twi_f = ((sy + 0.22) / 0.24).clamp(0.0, 1.0) * (1.0 - ((sy - 0.04) / 0.34).clamp(0.0, 1.0));
    let key_dir = if sy >= -0.06 {
        vec3(true_sun.x, sy.max(0.05), true_sun.z).normalize()
    } else {
        vec3(-true_sun.x, (-sy).max(0.28), -true_sun.z).normalize()
    };
    let sh_dir = vec3(key_dir.x, key_dir.y.max(0.20), key_dir.z).normalize();
    let f_day = vec3(0.52, 0.72, 0.95);
    let f_ngt = vec3(0.018, 0.030, 0.068);
    let f_twi = vec3(0.88, 0.38, 0.20);
    let fog = f_ngt.lerp(f_day, day_f).lerp(f_twi, twi_f * 0.85);
    let sh_alpha = if sy >= -0.06 { 0.54 + 0.12 * twi_f } else { 0.34 };
    let sh_rgb = if sy >= -0.06 {
        vec3(0.015, 0.025, 0.07).lerp(vec3(0.025, 0.025, 0.14), twi_f)
    } else {
        vec3(0.008, 0.012, 0.035)
    };
    EnvLight { true_sun, key_dir, sh_dir, fog, sh_col: vec4(sh_rgb.x, sh_rgb.y, sh_rgb.z, sh_alpha), head_pos, head_fwd }
}

thread_local! {
    static CACHED_STAGE: std::cell::RefCell<Option<(level::Level, World, hood::Scene)>> = std::cell::RefCell::new(None);
}

fn draw_sky_env(res: &Res, vp: Mat4, cp: Vec3, t: f32, env: &EnvLight) {
    gl_use_material(&res.sky);
    res.sky.set_uniform("invVP", vp.inverse());
    res.sky.set_uniform("sun_dir", env.true_sun);
    res.sky.set_uniform("cam_pos", cp);
    res.sky.set_uniform("misc", vec4(t, env.true_sun.y, 0.0, 0.0));
    let v = |x: f32, y: f32| Vertex { position: vec3(x, y, 0.0), uv: vec2(0.0, 0.0), color: [255, 255, 255, 255], normal: vec4(0.0, 0.0, 1.0, 0.0) };
    draw_mesh(&Mesh { vertices: vec![v(-1.0, -1.0), v(1.0, -1.0), v(1.0, 1.0), v(-1.0, 1.0)], indices: vec![0, 1, 2, 0, 2, 3], texture: None });
}

fn lit_set_env(mt: &Material, vp: Mat4, cp: Vec3, t: f32, env: &EnvLight) {
    mt.set_uniform("VP", vp);
    mt.set_uniform("MODEL", Mat4::IDENTITY);
    mt.set_uniform("CarM", Mat4::IDENTITY);
    mt.set_uniform("tint", Vec3::ONE);
    mt.set_uniform("sun_dir", env.key_dir);
    mt.set_uniform("cam_pos", cp);
    mt.set_uniform("fog_col", env.fog);
    mt.set_uniform("misc", vec4(t, env.true_sun.y, env.head_pos.x, env.head_pos.z));
    crate::world::set_env(env.key_dir, env.fog, t, env.true_sun.y);
    mt.set_uniform("head_dir", vec4(env.head_fwd.x, env.head_fwd.y, env.head_fwd.z, env.head_pos.y));
}

fn lit_begin_cull_env(res: &Res, vp: Mat4, cp: Vec3, t: f32, env: &EnvLight) {
    gl_use_material(&res.lit_cull);
    lit_set_env(&res.lit_cull, vp, cp, t, env);
}

fn lit_begin_env(res: &Res, vp: Mat4, cp: Vec3, t: f32, env: &EnvLight) {
    gl_use_material(&res.lit);
    lit_set_env(&res.lit, vp, cp, t, env);
    lit_set_env(&res.lit_glass, vp, cp, t, env);
}

fn draw_sky(res: &Res, vp: Mat4, cp: Vec3, t: f32) {
    let env = env_light(17.55, Vec3::ZERO, Vec3::Z);
    draw_sky_env(res, vp, cp, t, &env);
}

fn lit_begin(res: &Res, vp: Mat4, cp: Vec3, t: f32) {
    let env = env_light(17.55, Vec3::ZERO, Vec3::Z);
    lit_begin_env(res, vp, cp, t, &env);
}

/// Draws one vehicle in the lit pass using zero-allocation pooled wheel meshes (`c.wheel_render`).
fn draw_car_full(res: &Res, c: &Car) {
    if let Some(hm) = c.hi.clone() { hicar::draw_car(res, &hm, c); return; }
    res.lit.set_uniform("tint", c.paint);
    draw_mesh(&c.render);
    res.lit.set_uniform("tint", Vec3::ONE);
    for m in &c.wheel_render { draw_mesh(m); }
}

fn draw_car_lit(c: &mut Car, res: &Res) {
    c.update_render();
    draw_car_full(res, c);
}

fn txt(f: &Font, s: &str, x: f32, y: f32, size: u16, c: Color) {
    draw_text_ex(s, x, y, TextParams { font: Some(f), font_size: size, color: c, ..Default::default() });
}
fn txt_w(f: &Font, s: &str, size: u16) -> f32 { measure_text(s, Some(f), size, 1.0).width }
fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
    let r = r.min(w / 2.0).min(h / 2.0);
    draw_rectangle(x + r, y, w - 2.0 * r, h, c);
    draw_rectangle(x, y + r, r, h - 2.0 * r, c);
    draw_rectangle(x + w - r, y + r, r, h - 2.0 * r, c);
    let steps = 6;
    for (cx, cy, a0) in [(x + r, y + r, 180.0f32), (x + w - r, y + r, 270.0), (x + w - r, y + h - r, 0.0), (x + r, y + h - r, 90.0)] {
        for i in 0..steps {
            let a = (a0 + 90.0 * i as f32 / steps as f32).to_radians();
            let b = (a0 + 90.0 * (i + 1) as f32 / steps as f32).to_radians();
            draw_triangle(vec2(cx, cy), vec2(cx + r * a.cos(), cy + r * a.sin()), vec2(cx + r * b.cos(), cy + r * b.sin()), c);
        }
    }
}
const ACCENT: Color = Color::new(1.0, 0.56, 0.10, 1.0);
const PANEL: Color = Color::new(0.04, 0.06, 0.10, 0.78);

// -------------------------------------------------------------------------
// Task 9: Non-Blocking Progressive Loading Screen (`loading_reference` / `image_e58d76.png`)
// -------------------------------------------------------------------------
const LOADING_TIPS: [&str; 6] = [
    "Tip: Press F8 at any time to open the 4K In-Game Photo Mode & Golden Hour Camera Sequencer.",
    "Tip: On the Scania Touring 2026 Bus, press [ and ] to add up to 50 passengers (+4,000 kg real mass).",
    "Tip: Lower tire pressure in the Garage increases contact patch grip on dirt & wet asphalt.",
    "Tip: Server operators can grant World Editor (F10) permissions via `build allow <player>` in console.",
    "Tip: Press I (or click Engine Start/Stop on the HUD) to toggle the ignition.",
    "Tip: Press F1 in-game to inspect real-time FPS, frame time, draw calls, and Pacejka tire telemetry.",
];

fn draw_loading_screen(res: &Res, step: usize, total_steps: usize, status_text: &str, tip_idx: usize) {
    let (sw, sh) = (screen_width(), screen_height());
    clear_background(Color::new(0.015, 0.018, 0.024, 1.0));
    // Phase 10: Cyclic static cinematic background shot (rotates by tip_idx per loading entry, stays static during the loading screen!)
    if !res.loading_shots.is_empty() {
        let shot_tex = &res.loading_shots[tip_idx % res.loading_shots.len()];
        draw_texture_ex(
            shot_tex,
            0.0,
            0.0,
            Color::new(0.72, 0.74, 0.78, 1.0),
            DrawTextureParams {
                dest_size: Some(vec2(sw, sh)),
                flip_y: true,
                ..Default::default()
            },
        );
        draw_rectangle(0.0, sh - 135.0, sw, 135.0, Color::new(0.01, 0.015, 0.025, 0.82));
    }

    // Bottom-left CrashNG 3-node connected triangle emblem (`image_e58d76.png`)
    let orange = Color::new(0.98, 0.44, 0.04, 1.0);
    let orange_dim = Color::new(0.76, 0.32, 0.03, 1.0);
    let slate = Color::new(0.11, 0.17, 0.24, 0.95);
    let p_left = vec2(46.0, sh - 74.0);
    let p_top = vec2(74.0, sh - 92.0);
    let p_bot = vec2(80.0, sh - 54.0);
    draw_line(p_left.x, p_left.y, p_top.x, p_top.y, 4.5, WHITE);
    draw_line(p_top.x, p_top.y, p_bot.x, p_bot.y, 4.5, WHITE);
    draw_line(p_left.x, p_left.y, p_bot.x, p_bot.y, 4.5, WHITE);
    draw_circle(p_left.x, p_left.y, 13.5, orange);
    draw_circle(p_top.x, p_top.y, 8.0, orange);
    draw_circle(p_bot.x, p_bot.y, 11.5, orange);

    // "Loading..." + Stage text on the same baseline
    let lx = 108.0;
    let ly = sh - 68.0;
    txt(&res.font_b, "Loading...", lx, ly, 42, WHITE);
    let lw = txt_w(&res.font_b, "Loading...", 42);
    txt(&res.font, status_text, lx + lw + 16.0, ly - 2.0, 18, Color::new(0.84, 0.87, 0.92, 0.90));

    // 7 slanted parallelogram progress segments directly below "Loading..."
    let seg_count = 7usize;
    let seg_w = 42.0f32;
    let seg_h = 10.0f32;
    let seg_gap = 5.0f32;
    let sk = 5.0f32;
    let sy = sh - 52.0;
    let filled = ((step as f32 / total_steps.max(1) as f32) * seg_count as f32).ceil() as usize;
    for i in 0..seg_count {
        let sx = lx + i as f32 * (seg_w + seg_gap);
        let col = if i + 1 < filled {
            orange
        } else if i + 1 == filled {
            orange_dim
        } else {
            slate
        };
        let p1 = vec2(sx + sk, sy);
        let p2 = vec2(sx + seg_w + sk, sy);
        let p3 = vec2(sx + seg_w, sy + seg_h);
        let p4 = vec2(sx, sy + seg_h);
        draw_triangle(p1, p2, p3, col);
        draw_triangle(p1, p3, p4, col);
    }

    // Rotating gameplay tip on the bottom-right
    let tip = LOADING_TIPS[tip_idx % LOADING_TIPS.len()];
    let tw = txt_w(&res.font, tip, 15);
    txt(&res.font, tip, (sw - tw - 36.0).max(460.0), sy + 8.0, 15, Color::new(0.65, 0.72, 0.82, 0.78));
}

fn build_world() -> World {
    level::Level::load_official_or_disk().world()
}

fn stage_sync(res: &Res, level: &mut level::Level, world: &mut World, scene: &mut hood::Scene) {
    hood::stream_custom_bounds(&mut scene.assets, level);
    if level.dirty { *world = level.world(); hood::rebuild(scene, level, world, res.tex.clone()); level.dirty = false; }
    if level.forest_dirty { hood::rebuild_forest(scene, level); level.forest_dirty = false; }
}

async fn new_stage_async(res: &Res, map_id: u8, tip_idx: usize) -> (level::Level, World, hood::Scene) {
    if let Some((_old_lvl, mut world, mut scene)) = CACHED_STAGE.with(|cs| cs.borrow_mut().take()) {
        draw_loading_screen(res, 3, 7, "Synchronizing world geometry & collision volumes", tip_idx);
        next_frame().await;
        let mut level = level::Level::load_by_id(map_id);
        level.dirty = true;
        level.forest_dirty = true;
        stage_sync(res, &mut level, &mut world, &mut scene);
        return (level, world, scene);
    }
    draw_loading_screen(res, 2, 7, "Loading world environment & road layout", tip_idx);
    next_frame().await;
    let mut level = level::Level::load_by_id(map_id);
    let mut world = level.world();

    draw_loading_screen(res, 4, 7, "Building terrain, LOD meshes & road network", tip_idx);
    next_frame().await;
    let mut scene = hood::build_ground(world.arena);

    draw_loading_screen(res, 6, 7, "Streaming custom GLB models & batching static instances", tip_idx);
    next_frame().await;
    level.dirty = true;
    level.forest_dirty = true;
    stage_sync(res, &mut level, &mut world, &mut scene);
    if let Ok(src) = std::fs::read_to_string("scripts/startup.py") {
        let mut it = pyscript::Interp::new();
        let mut host = editor::LevelHost { level: &mut level };
        let _ = it.run(&src, &mut host);
        stage_sync(res, &mut level, &mut world, &mut scene);
    }
    (level, world, scene)
}

fn new_stage_sync(res: &Res) -> (level::Level, World, hood::Scene) {
    if let Some((_old_lvl, mut world, mut scene)) = CACHED_STAGE.with(|cs| cs.borrow_mut().take()) {
        let mut level = level::Level::load_official_or_disk();
        level.dirty = true;
        level.forest_dirty = true;
        stage_sync(res, &mut level, &mut world, &mut scene);
        return (level, world, scene);
    }
    let mut level = level::Level::load_official_or_disk();
    let mut world = level.world();
    let mut scene = hood::build_ground(world.arena);
    level.dirty = true;
    level.forest_dirty = true;
    stage_sync(res, &mut level, &mut world, &mut scene);
    (level, world, scene)
}

#[derive(Clone)]
pub enum CarKind { Builtin(usize), Mod { m: usize, cfg: usize } }
#[derive(Clone)]
pub enum MapKind { BuiltinMap(u8), Mod(usize) }

pub struct CarOpt { pub name: String, pub sub: String, pub thumb: Option<Texture2D>, pub kind: CarKind, pub playable: bool }
pub struct MapOpt { pub name: String, pub sub: String, pub thumb: Option<Texture2D>, pub kind: MapKind, pub heavy_warning: bool }

pub struct Menu {
    pub tab: usize,
    pub sel_car: usize,
    pub sel_map: usize,
    pub scroll_car: f32,
    pub scroll_map: f32,
    pub toast: Option<(String, f32)>,
    pub previews: HashMap<usize, Car>,
    pub cars: Vec<CarOpt>,
    pub maps: Vec<MapOpt>,
    pub mods: Vec<mods::ModInfo>,
}

fn thumb(b: &Option<Vec<u8>>) -> Option<Texture2D> {
    let b = b.as_ref()?;
    let t = if b.len() > 2 && b[0] == 0xFF && b[1] == 0xD8 {
        let mut d = jpeg_decoder::Decoder::new(&b[..]);
        let px = d.decode().ok()?;
        let info = d.info()?;
        let (w, h) = (info.width as usize, info.height as usize);
        let mut rgba = Vec::with_capacity(w * h * 4);
        match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => for p in px.chunks(3) { rgba.extend_from_slice(&[p[0], p[1], p[2], 255]); },
            jpeg_decoder::PixelFormat::L8 => for p in px.iter() { rgba.extend_from_slice(&[*p, *p, *p, 255]); },
            _ => return None,
        }
        Texture2D::from_rgba8(w as u16, h as u16, &rgba)
    } else {
        Texture2D::from_file_with_format(b, None)
    };
    t.set_filter(FilterMode::Linear);
    Some(t)
}

fn build_menu(mods_dir: &std::path::Path) -> Menu {
    let mut found = mods::scan(mods_dir);
    for m in found.iter_mut() {
        if m.kind != mods::ModKind::Car { continue; }
        let parts = bng::load_parts(&m.path, &m.model);
        for c in m.configs.iter_mut() {
            if let Some(b) = mods::read_zip_file(&m.path, &format!("vehicles/{}/{}.pc", m.model, c.id)) {
                if let Ok(cfg) = jbeam::parse(&String::from_utf8_lossy(&b)) {
                    let v = bng::assemble(&parts, &cfg);
                    let ok = v.wheels.iter().filter(|w| v.nodes.iter().any(|n| n.name == w.node1) && v.nodes.iter().any(|n| n.name == w.node2)).count() >= 4;
                    c.playable = ok;
                }
            }
        }
    }
    let mut cars = Vec::new();
    for (i, s) in SPECS.iter().enumerate() {
        cars.push(CarOpt { name: s.name.to_string(), sub: format!("{:.0} kg  -  {}", s.mass, match s.drive { Drive::Rwd => "RWD", Drive::Fwd => "FWD", Drive::Awd => "AWD" }), thumb: None, kind: CarKind::Builtin(i), playable: true });
    }
    let mut maps = vec![
        MapOpt {
            name: "Map 1: West Coast Urban District (Default)".into(),
            sub: "Official Lightweight Proving Grounds  -  Fast Load, Full 60+ FPS on All PCs".into(),
            thumb: None,
            kind: MapKind::BuiltinMap(0),
            heavy_warning: false,
        },
        MapOpt {
            name: "Map 2: West Coast USA (by Ahmed Benali)".into(),
            sub: "Heavy Expansion Map (CC-BY 4.0)  -  Coastal Highway, Overpass & Urban Blocks".into(),
            thumb: None,
            kind: MapKind::BuiltinMap(1),
            heavy_warning: true,
        },
        MapOpt {
            name: "Map 3: Maseukowice Train Station (by masuo)".into(),
            sub: "Large Open Map 4x4 km  -  Railway Station, Rail Yard & Countryside Roads".into(),
            thumb: None,
            kind: MapKind::BuiltinMap(2),
            heavy_warning: true,
        },
    ];
    for (mi, m) in found.iter().enumerate() {
        match m.kind {
            mods::ModKind::Car => {
                let mt = thumb(&m.thumb);
                for (ci, c) in m.configs.iter().enumerate() {
                    let sub = if c.playable { format!("Custom Vehicle Package  -  {}", m.author) } else { "Custom Vehicle Package".to_string() };
                    cars.push(CarOpt { name: format!("{} - {}", m.title, c.name), sub, thumb: thumb(&c.thumb).or(mt.clone()), kind: CarKind::Mod { m: mi, cfg: ci }, playable: c.playable });
                }
            }
            mods::ModKind::Map => maps.push(MapOpt {
                name: m.title.clone(),
                sub: format!("External Map Package  -  {}", if m.author.is_empty() { "Custom".to_string() } else { m.author.clone() }),
                thumb: thumb(&m.thumb),
                kind: MapKind::Mod(mi),
                heavy_warning: true,
            }),
        }
    }
    // Default selected vehicle: Mercedes-AMG One (index 0)
    Menu { tab: 0, sel_car: 0, sel_map: 0, scroll_car: 0.0, scroll_map: 0.0, toast: None, previews: HashMap::new(), cars, maps, mods: found }
}

const SHOT_LEN: f32 = 10.0;

fn menu_cinematic(res: &Res, m: &mut Menu, t: f32, gfx_quality: u8) {
    let (sw, sh) = (screen_width(), screen_height());
    CACHED_STAGE.with(|cs| {
        if cs.borrow().is_none() {
            let st = new_stage_sync(res);
            *cs.borrow_mut() = Some(st);
        }
    });
    let idx = ((t / SHOT_LEN) as usize) % photo::OFFICIAL_MAP_SHOTS.len();
    let u = (t % SHOT_LEN) / SHOT_LEN;
    let e = u * u * (3.0 - 2.0 * u);
    let shot = &photo::OFFICIAL_MAP_SHOTS[idx];
    let pcar = shot.car_pos;
    let cpos = shot.cam_pos + vec3((e - 0.5) * 1.8, (t * 0.6).sin() * 0.06, (e - 0.5) * 1.4);
    let ct = shot.cam_look;
    let vp = Mat4::perspective_rh_gl(shot.fov_deg.to_radians(), sw / sh, 0.15, 1800.0) * Mat4::look_at_rh(cpos, ct, Vec3::Y);
    let env = env_light(shot.day_hour, pcar + vec3(0.0, 0.65, 1.8), Vec3::Z);
    clear_background(BLACK);
    draw_sky_env(res, vp, cpos, t, &env);
    lit_begin_env(res, vp, cpos, t, &env);
    CACHED_STAGE.with(|cs| {
        if let Some((lvl, _, sc)) = cs.borrow().as_ref() {
            hood::draw_ex(sc, lvl, &res.lit, vp, cpos, (ct - cpos).normalize_or_zero(), false, gfx_quality);
        }
    });
    let car_spec = shot.car_spec.min(SPECS.len() - 1);
    // Avoid decoding the 20MB Scania GLB on the main menu cinematic loop on Low preset
    let eff_spec = if gfx_quality == 0 && car_spec == 4 { 0 } else { car_spec };
    if !m.previews.contains_key(&eff_spec) {
        let mut c = Car::new(eff_spec, res.tex.clone(), false);
        c.respawn(pcar, shot.car_yaw);
        m.previews.insert(eff_spec, c);
    }
    if let Some(c) = m.previews.get_mut(&eff_spec) {
        c.respawn(pcar, shot.car_yaw);
        draw_car_lit(c, res);
    }
    gl_use_default_material();
    set_default_camera();
    let fade = ((0.05 - u) / 0.05).max((u - 0.95) / 0.05).clamp(0.0, 1.0);
    if fade > 0.0 { draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, fade)); }
}

fn draw_bg(res: &Res, m: &mut Menu, t: f32, mode: u8, gfx_quality: u8) {
    if false { menu_cinematic(res, m, t, gfx_quality); return; }
    let (sw, sh) = (screen_width(), screen_height());
    let aspect = sw / sh;
    let car_idx = if let CarKind::Builtin(i) = m.cars[m.sel_car].kind { Some(i) } else { None };
    let is_bus = car_idx == Some(4);
    let focus = vec3(if mode == 3 { -1.8 } else { -2.2 }, if is_bus { 1.35 } else { 0.55 }, 0.0);
    let yaw = t * 0.22;
    let height = if is_bus { 2.6 } else { 1.65 + 0.25 * (t * 0.2).sin() };
    let dist = if is_bus { 14.5 } else { 6.8 };
    let cpos = focus + vec3(yaw.sin() * dist, height, yaw.cos() * dist);
    let fwd = (focus - cpos).normalize();
    let right = fwd.cross(Vec3::Y).normalize();
    let ctarget = if mode == 3 { focus + right * 1.4 } else { focus + vec3(0.0, 0.25, 0.0) };
    let vp = Mat4::perspective_rh_gl(42f32.to_radians(), aspect, 0.25, 1800.0) * Mat4::look_at_rh(cpos, ctarget, Vec3::Y);
    clear_background(BLACK);
    draw_sky(res, vp, cpos, t);
    lit_begin(res, vp, cpos, t);
    let mut mb = MeshBuilder::new();
    mb.add_box(vec3(0.0, -0.5, 0.0), vec3(300.0, 0.5, 300.0), 0.0, Color::new(0.20, 0.21, 0.23, 1.0), 0.06);
    mb.add_box(vec3(-2.2, -0.04, 0.0), vec3(7.2, 0.04, 7.2), 0.0, Color::new(0.12, 0.12, 0.14, 1.0), 0.45);
    let mut ms = Vec::new();
    mb.flush(&mut ms, None);
    for s in &ms { draw_mesh(s); }
    if let Some(i) = car_idx {
        if !m.previews.contains_key(&i) {
            let mut c = Car::new(i, res.tex.clone(), false);
            c.respawn(vec3(-2.2, 0.0, 0.0), 0.6);
            m.previews.insert(i, c);
        }
        if let Some(c) = m.previews.get_mut(&i) { draw_car_lit(c, res); }
    }
    gl_use_default_material();
    set_default_camera();
}

/// Task 2: Spawns player and parked NPC cars at verified collision-free coordinates (`level::OFFICIAL_SPAWNS`) inside `level.json`.
fn spawn_scene(tex: &Texture2D) -> Vec<Car> {
    let mut cars = Vec::new();
    let (p0, yaw0) = level::spawn(0);
    let mut p = Car::new(0, tex.clone(), true); // Default player vehicle: Mercedes-AMG One (index 0)
    p.respawn(Vec3::from_array(p0), yaw0);
    cars.push(p);

    let parked_specs: [usize; 4] = [1, 2, 3, 4];
    for (k, &spec_i) in parked_specs.iter().enumerate() {
        let (pos_arr, yaw) = level::spawn(k + 1);
        let mut c = Car::new(spec_i, tex.clone(), false);
        c.respawn(Vec3::from_array(pos_arr), yaw);
        c.update_render();
        c.asleep = true;
        c.render_dirty = false;
        cars.push(c);
    }
    cars
}

fn aabb_overlap(a: &(Vec3, Vec3), b: &(Vec3, Vec3), pad: f32) -> bool {
    a.1.x + pad >= b.0.x && a.0.x - pad <= b.1.x && a.1.y + pad >= b.0.y && a.0.y - pad <= b.1.y && a.1.z + pad >= b.0.z && a.0.z - pad <= b.1.z
}

fn sim_step(cars: &mut Vec<Car>, world: &World, h: f32) {
    for c in cars.iter_mut() {
        if !c.asleep { c.update_frame(); }
    }
    for c in cars.iter_mut() {
        if !c.asleep { c.forces(h, world); }
    }
    let n = cars.len();
    for i in 0..n {
        for j in 0..n {
            if i == j { continue; }
            if cars[i].asleep && cars[j].asleep { continue; }
            if cars[i].asleep { continue; }
            if !aabb_overlap(&cars[i].aabb, &cars[j].aabb, 0.25) { continue; }
            if cars[j].asleep { cars[j].update_frame(); }
            let (a, b) = if i < j {
                let (l, r) = cars.split_at_mut(j);
                (&mut l[i], &mut r[0])
            } else {
                let (l, r) = cars.split_at_mut(i);
                (&mut r[0], &mut l[j])
            };
            collide_into(a, b, h);
        }
    }
    for c in cars.iter_mut() {
        if !c.asleep { c.integrate(h); }
    }
}

fn frame_housekeeping(cars: &mut Vec<Car>, dt: f32) {
    let n = cars.len();
    for j in 0..n {
        if !cars[j].asleep { continue; }
        for i in 0..n {
            if i == j || cars[i].asleep { continue; }
            if aabb_overlap(&cars[i].aabb, &cars[j].aabb, 0.8) && (cars[i].is_player || cars[i].body.avg_velocity().length() > 0.5) {
                cars[j].asleep = false;
                cars[j].still_time = 0.0;
                cars[j].render_dirty = true;
            }
        }
    }
    for c in cars.iter_mut() {
        if c.asleep { continue; }
        let v = c.body.avg_velocity();
        let dv = (v - c.last_vel) / dt.max(1e-4);
        let a = dv.length();
        let long_g = (dv.dot(c.frame.fwd) / 9.81).clamp(-2.5, 2.5);
        let lat_g = (dv.dot(c.frame.left) / 9.81).clamp(-2.5, 2.5);
        let g_blend = (dt * 9.0).min(1.0);
        c.accel_long_g += (long_g - c.accel_long_g) * g_blend;
        c.accel_lat_g += (lat_g - c.accel_lat_g) * g_blend;
        c.impact_g += (a / 9.81 - c.impact_g) * 0.5;
        if c.impact_g > c.peak_g { c.peak_g = c.impact_g; }
        c.last_vel = v;
        if !c.is_player {
            let maxv = c.body.vel.iter().fold(0.0f32, |m, v| m.max(v.length()));
            if maxv < 0.25 { c.still_time += dt; } else { c.still_time = 0.0; }
            if c.still_time > 1.2 { c.asleep = true; for v in c.body.vel.iter_mut() { *v = Vec3::ZERO; } }
        }
    }
}

struct Cam {
    pos: Vec3,
    target: Vec3,
    heading: Vec3,
    mode: i32,
    orbit: f32,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    idle: f32,
}

struct CamTarget {
    c: Vec3,
    fwd: Vec3,
    up: Vec3,
    left: Vec3,
    vel: Vec3,
    eye: Option<Vec3>,
    is_bus: bool,
    accel_long_g: f32,
    accel_lat_g: f32,
    steer_cur: f32,
    head_motion: f32,
}

fn update_camera(cam: &mut Cam, car: &Car, head_motion: f32, dt: f32) -> (Vec3, Vec3) {
    let f = car.frame;
    let is_bus = SPECS[car.spec_idx].hi_kind == HiKind::CrafterBus;
    let t = CamTarget {
        c: f.c,
        fwd: f.fwd,
        up: f.up,
        left: f.left,
        vel: car.body.avg_velocity(),
        eye: car.cockpit_eye(),
        is_bus,
        accel_long_g: car.accel_long_g,
        accel_lat_g: car.accel_lat_g,
        steer_cur: car.steer_cur,
        head_motion,
    };
    update_camera_t(cam, &t, dt)
}

fn update_camera_t(cam: &mut Cam, f: &CamTarget, dt: f32) -> (Vec3, Vec3) {
    let vel = f.vel;
    if is_mouse_button_down(MouseButton::Right) {
        let d = mouse_delta_position();
        cam.yaw += d.x * 2.6;
        cam.pitch = (cam.pitch - d.y * 1.9).clamp(-0.45, 1.15);
        cam.idle = 0.0;
    } else {
        cam.idle += dt;
        if cam.idle > 2.2 && cam.mode != 2 {
            let k = 1.0 - (-2.0 * dt).exp();
            cam.yaw -= cam.yaw * k;
            cam.pitch -= cam.pitch * k;
        }
    }
    let (_, wheel) = mouse_wheel();
    if wheel != 0.0 { cam.zoom = (cam.zoom * (1.0 - wheel * 0.08)).clamp(0.45, 3.2); }
    let flat_fwd = vec3(f.fwd.x, 0.0, f.fwd.z).normalize_or_zero();
    let want = if vel.length() > 2.5 { vec3(vel.x, 0.0, vel.z).normalize_or_zero() * 0.45 + flat_fwd * 0.55 } else { flat_fwd };
    let want = want.normalize_or_zero();
    let t = 1.0 - (-4.5 * dt).exp();
    cam.heading = (cam.heading + (want - cam.heading) * t).normalize_or_zero();
    if cam.heading.length() < 0.1 { cam.heading = Vec3::Z; }

    // Phase 2 & Phase 5: Calibrated exterior chase & orbit distances and heights (comfortable full road view over the VW Crafter / Bus roof)
    let base_dist = if f.is_bus { 14.8 } else { 7.4 };
    let base_lift = if f.is_bus { 2.55 } else { 0.88 };
    let base_elev = if f.is_bus { 0.27 } else { 0.29 };
    match cam.mode {
        0 => {
            let base = base_dist + vel.length() * 0.032;
            let dist = base * cam.zoom;
            let elev = base_elev + cam.pitch;
            let hy = Quat::from_rotation_y(cam.yaw) * (-cam.heading);
            let off = hy * (dist * elev.cos()) + Vec3::Y * (dist * elev.sin() + if f.is_bus { 1.15 } else { 0.38 });
            let desired = f.c + off;
            cam.pos += (desired - cam.pos) * (1.0 - (-9.5 * dt).exp());
            let look = f.c + Vec3::Y * base_lift + cam.heading * (if f.is_bus { 5.2 } else { 3.2 } * (1.0 - cam.yaw.abs().min(1.0)));
            cam.target += (look - cam.target) * (1.0 - (-12.0 * dt).exp());
        }
        1 => {
            // Phase 5: Forza Horizon-style Cockpit Driver Camera:
            // - Natural head position with subtle longitudinal surge on acceleration/braking
            // - Subtle lateral lean & apex look-ahead on cornering
            let hm = f.head_motion;
            let base_eye = f.eye.unwrap_or(f.c + f.up * 0.54 + f.fwd * 0.15);
            let surge_fwd = (-f.accel_long_g * 0.038 * hm).clamp(-0.065, 0.075);
            let sway_lat = (-f.accel_lat_g * 0.032 * hm).clamp(-0.055, 0.055);
            let dip_up = (-f.accel_long_g.abs() * 0.012 * hm).clamp(-0.025, 0.01);
            let desired_eye = base_eye + f.fwd * surge_fwd + f.left * sway_lat + f.up * dip_up;
            let eye_blend = (dt * 22.0).min(1.0);
            if (cam.pos - desired_eye).length() > 1.2 {
                cam.pos = desired_eye;
            } else {
                cam.pos += (desired_eye - cam.pos) * eye_blend;
            }
            let pitch_g = (-f.accel_long_g * 0.028 * hm).clamp(-0.06, 0.06);
            let apex_yaw = (f.steer_cur * 0.18 * hm).clamp(-0.14, 0.14);
            let q = Quat::from_rotation_y(cam.yaw + apex_yaw) * Quat::from_axis_angle(-f.left, (cam.pitch + pitch_g) * 0.65);
            cam.target = cam.pos + (q * f.fwd) * 12.0;
        }
        _ => {
            if is_key_down(KeyCode::Left) { cam.orbit -= 1.5 * dt; }
            if is_key_down(KeyCode::Right) { cam.orbit += 1.5 * dt; }
            let a = cam.orbit + cam.yaw;
            let dist = (base_dist + 1.2) * cam.zoom;
            let elev = (base_elev + 0.04) + cam.pitch;
            let desired = f.c + vec3(a.sin() * dist * elev.cos(), dist * elev.sin() + if f.is_bus { 1.2 } else { 0.5 }, a.cos() * dist * elev.cos());
            cam.pos += (desired - cam.pos) * (1.0 - (-10.0 * dt).exp());
            cam.target = f.c + Vec3::Y * base_lift;
        }
    }
    (cam.pos, cam.target)
}

// -------------------------------------------------------------------------
// Task 8: Circular Tachometer, Boost Sub-Gauge, Engine Start/Stop & Circular Minimap (`hud_reference` / `image_e5faee.jpg`)
// -------------------------------------------------------------------------
fn draw_arc_band(cx: f32, cy: f32, r_in: f32, r_out: f32, a0_deg: f32, a1_deg: f32, steps: usize, col: Color) {
    if steps == 0 || (a1_deg - a0_deg).abs() < 0.1 { return; }
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let a0 = (a0_deg + (a1_deg - a0_deg) * t0).to_radians();
        let a1 = (a0_deg + (a1_deg - a0_deg) * t1).to_radians();
        let p0_in = vec2(cx + a0.cos() * r_in, cy + a0.sin() * r_in);
        let p0_out = vec2(cx + a0.cos() * r_out, cy + a0.sin() * r_out);
        let p1_in = vec2(cx + a1.cos() * r_in, cy + a1.sin() * r_in);
        let p1_out = vec2(cx + a1.cos() * r_out, cy + a1.sin() * r_out);
        draw_triangle(p0_in, p0_out, p1_out, col);
        draw_triangle(p0_in, p1_out, p1_in, col);
    }
}

fn zone_name_at(pos: Vec3) -> &'static str {
    if pos.z > 135.0 {
        "Arabic Temple Avenue"
    } else if pos.z > 62.0 {
        "North Fence Boulevard"
    } else if pos.x < -15.0 && pos.z > -25.0 && pos.z <= 62.0 {
        "Central School Plaza"
    } else if pos.x > 18.0 && pos.z > 10.0 && pos.z < 50.0 {
        "East Residential Parking"
    } else {
        "West Coast Downtown Avenue"
    }
}

fn draw_hud(
    res: &Res,
    car: &mut Car,
    level: &level::Level,
    spec_name: &str,
    cam_mode: i32,
    day_hour: f32,
    show_profiler: bool,
    show_help: bool,
    toast: &Option<(String, f32)>,
) {
    let (sw, sh) = (screen_width(), screen_height());
    let u = (sh / 720.0).min(sw / 1280.0).clamp(0.75, 2.2);
    let f = &res.font;
    let fb = &res.font_b;

    // -----------------------------------------------------------------
    // 1. Bottom-Left Circular Minimap + Zone Banner (`image_e5faee.jpg`)
    // -----------------------------------------------------------------
    let mm_r = 76.0 * u;
    let mm_cx = 22.0 * u + mm_r;
    let mm_cy = sh - 22.0 * u - mm_r;

    // Zone Banner Pill above Minimap
    let zname = zone_name_at(car.frame.c);
    let zfs = (12.5 * u).round() as u16;
    let zw = txt_w(fb, zname, zfs) + 22.0 * u;
    let zh = 22.0 * u;
    rrect(mm_cx - zw * 0.5, mm_cy - mm_r - zh - 8.0 * u, zw, zh, 4.0 * u, Color::new(0.06, 0.07, 0.09, 0.86));
    txt(fb, zname, mm_cx - zw * 0.5 + 11.0 * u, mm_cy - mm_r - 12.5 * u, zfs, Color::new(0.92, 0.94, 0.97, 0.95));

    // Minimap circular base & subtle grid
    draw_circle(mm_cx, mm_cy, mm_r + 3.0 * u, Color::new(0.05, 0.06, 0.08, 0.90));
    draw_circle(mm_cx, mm_cy, mm_r, Color::new(0.13, 0.15, 0.18, 0.85));

    let cam_yaw = car.frame.fwd.x.atan2(car.frame.fwd.z);
    let (sy, cy_rot) = cam_yaw.sin_cos();
    let map_scale = mm_r / 95.0; // 95m radius radar view

    let to_mm = |wx: f32, wz: f32| -> Vec2 {
        let dx = wx - car.frame.c.x;
        let dz = wz - car.frame.c.z;
        // Rotate relative to car heading so forward is UP on the minimap
        let rx = -(dx * cy_rot - dz * sy) * map_scale;
        let ry = -(dx * sy + dz * cy_rot) * map_scale;
        vec2(mm_cx + rx, mm_cy + ry)
    };

    // Draw diagonal grid lines inside minimap circle
    let grid_step = 25.0f32;
    let gx0 = ((car.frame.c.x - 90.0) / grid_step).floor() as i32;
    let gz0 = ((car.frame.c.z - 90.0) / grid_step).floor() as i32;
    for gi in 0..8 {
        let wx = (gx0 + gi) as f32 * grid_step;
        let p1 = to_mm(wx, car.frame.c.z - 85.0);
        let p2 = to_mm(wx, car.frame.c.z + 85.0);
        if (p1 - vec2(mm_cx, mm_cy)).length() < mm_r && (p2 - vec2(mm_cx, mm_cy)).length() < mm_r {
            draw_line(p1.x, p1.y, p2.x, p2.y, 1.0, Color::new(1.0, 1.0, 1.0, 0.14));
        }
        let wz = (gz0 + gi) as f32 * grid_step;
        let q1 = to_mm(car.frame.c.x - 85.0, wz);
        let q2 = to_mm(car.frame.c.x + 85.0, wz);
        if (q1 - vec2(mm_cx, mm_cy)).length() < mm_r && (q2 - vec2(mm_cx, mm_cy)).length() < mm_r {
            draw_line(q1.x, q1.y, q2.x, q2.y, 1.0, Color::new(1.0, 1.0, 1.0, 0.14));
        }
    }

    // Draw buildings & fences from `level.json` on the minimap
    for o in level.objs.iter().filter(|o| o.visible) {
        let mp = to_mm(o.pos.x, o.pos.z);
        if (mp - vec2(mm_cx, mm_cy)).length() > mm_r - 4.0 * u { continue; }
        match o.kind {
            level::Kind::Apartment | level::Kind::School => {
                draw_circle(mp.x, mp.y, 5.5 * u, Color::new(0.42, 0.48, 0.56, 0.85));
            }
            level::Kind::Custom(ref nm) if nm.contains("fence") => {
                draw_circle(mp.x, mp.y, 1.8 * u, Color::new(0.75, 0.78, 0.82, 0.70));
            }
            _ => {}
        }
    }

    draw_circle_lines(mm_cx, mm_cy, mm_r, 2.5 * u, Color::new(0.04, 0.05, 0.07, 0.95));

    // North `(N)` badge on the minimap perimeter
    let north_ang = -std::f32::consts::FRAC_PI_2 - cam_yaw;
    let nx = mm_cx + north_ang.cos() * (mm_r - 10.0 * u);
    let ny = mm_cy + north_ang.sin() * (mm_r - 10.0 * u);
    draw_circle(nx, ny, 9.5 * u, Color::new(0.95, 0.96, 0.98, 0.95));
    let nfs = (11.0 * u).round() as u16;
    txt(fb, "N", nx - txt_w(fb, "N", nfs) * 0.5, ny + 4.0 * u, nfs, Color::new(0.08, 0.09, 0.11, 1.0));

    // Orange triangular player arrow with white border in the lower-center of minimap
    let py = mm_cy + 18.0 * u;
    let p_tip = vec2(mm_cx, py - 14.0 * u);
    let p_l = vec2(mm_cx - 9.5 * u, py + 9.0 * u);
    let p_r = vec2(mm_cx + 9.5 * u, py + 9.0 * u);
    draw_triangle(vec2(mm_cx, py - 16.5 * u), vec2(mm_cx - 11.5 * u, py + 10.5 * u), vec2(mm_cx + 11.5 * u, py + 10.5 * u), WHITE);
    draw_triangle(p_tip, p_l, p_r, Color::new(0.98, 0.50, 0.08, 1.0));

    // -----------------------------------------------------------------
    // 2. Bottom-Right Circular Tachometer + Speedometer + Boost Gauge (`image_e5faee.jpg`)
    // -----------------------------------------------------------------
    let tach_r = 86.0 * u;
    let tach_cx = sw - 22.0 * u - tach_r;
    let tach_cy = sh - 22.0 * u - tach_r;

    // Outer dark bezel & dial face
    draw_circle(tach_cx, tach_cy, tach_r + 4.0 * u, Color::new(0.08, 0.09, 0.11, 0.88));
    draw_circle(tach_cx, tach_cy, tach_r, Color::new(0.14, 0.16, 0.19, 0.88));
    draw_circle(tach_cx, tach_cy, tach_r * 0.72, Color::new(0.10, 0.12, 0.15, 0.92));

    // RPM arc angles: 135 deg (bottom-left = 0 RPM) clockwise to 395 deg (bottom-right = max RPM)
    let (_, redline, _, _, _, _) = car.drv_cfg();
    let max_dial_rpm = (redline * 1.12).max(8000.0);
    let rpm_frac = (car.rpm / max_dial_rpm).clamp(0.0, 1.0);
    let a_start = 135.0f32;
    let a_span = 250.0f32;
    let a_red = a_start + a_span * (redline / max_dial_rpm).clamp(0.75, 0.92);

    // Background track arc + redline zone arc + live RPM progress arc
    draw_arc_band(tach_cx, tach_cy, tach_r * 0.76, tach_r * 0.95, a_start, a_start + a_span, 36, Color::new(0.06, 0.07, 0.09, 0.85));
    draw_arc_band(tach_cx, tach_cy, tach_r * 0.76, tach_r * 0.95, a_red, a_start + a_span, 10, Color::new(0.72, 0.14, 0.12, 0.78));
    let rpm_end = a_start + a_span * rpm_frac;
    let rpm_col = if car.rpm >= redline * 0.94 {
        Color::new(0.96, 0.22, 0.18, 0.96)
    } else {
        Color::new(0.94, 0.96, 0.98, 0.94)
    };
    draw_arc_band(tach_cx, tach_cy, tach_r * 0.84, tach_r * 0.95, a_start, rpm_end, 28, rpm_col);

    // Inner right blue fuel level bar arc (`image_e5faee.jpg`)
    let fuel_a0 = 35.0f32;
    let fuel_a1 = -35.0f32;
    draw_arc_band(tach_cx, tach_cy, tach_r * 0.63, tach_r * 0.69, fuel_a1, fuel_a0, 14, Color::new(0.08, 0.10, 0.14, 0.90));
    let fuel_top = fuel_a0 + (fuel_a1 - fuel_a0) * car.fuel.clamp(0.0, 1.0);
    draw_arc_band(tach_cx, tach_cy, tach_r * 0.63, tach_r * 0.69, fuel_top, fuel_a0, 14, Color::new(0.18, 0.56, 0.98, 0.96));

    // Radial RPM tick marks and numbers `1 .. 7`
    let max_k = (max_dial_rpm / 1000.0).round() as i32;
    let tick_fs = (12.5 * u).round() as u16;
    for k in 0..=max_k {
        let frac = k as f32 / max_k as f32;
        let ang = (a_start + a_span * frac).to_radians();
        let p_in = vec2(tach_cx + ang.cos() * (tach_r * 0.82), tach_cy + ang.sin() * (tach_r * 0.82));
        let p_out = vec2(tach_cx + ang.cos() * (tach_r * 0.95), tach_cy + ang.sin() * (tach_r * 0.95));
        draw_line(p_in.x, p_in.y, p_out.x, p_out.y, 2.0 * u, Color::new(1.0, 1.0, 1.0, 0.78));
        if k >= 1 {
            let lbl = format!("{}", k);
            let tx = tach_cx + ang.cos() * (tach_r * 0.69) - txt_w(fb, &lbl, tick_fs) * 0.5;
            let ty = tach_cy + ang.sin() * (tach_r * 0.69) + 4.5 * u;
            txt(fb, &lbl, tx, ty, tick_fs, WHITE);
        }
    }

    // Center Digital Speed Readout (`0 km/h`)
    let kmh = (car.speed.abs() * 3.6).round() as i32;
    let spd_str = format!("{}", kmh);
    let spd_fs = (38.0 * u).round() as u16;
    txt(fb, &spd_str, tach_cx - txt_w(fb, &spd_str, spd_fs) * 0.5, tach_cy - 4.0 * u, spd_fs, WHITE);
    let u_fs = (13.0 * u).round() as u16;
    txt(fb, "km/h", tach_cx - txt_w(fb, "km/h", u_fs) * 0.5, tach_cy + 13.0 * u, u_fs, Color::new(0.85, 0.88, 0.92, 0.90));

    // Coolant Temp & Fuel icons beside `km/h`
    let ic_fs = (10.5 * u).round() as u16;
    txt(fb, "T", tach_cx - 34.0 * u, tach_cy + 13.0 * u, ic_fs, Color::new(0.88, 0.90, 0.94, 0.85));
    txt(fb, "F", tach_cx + 26.0 * u, tach_cy + 13.0 * u, ic_fs, Color::new(0.35, 0.72, 1.0, 0.95));

    // Gear Indicator (`R`, `N`, `1..8`) + `x1000 RPM` at bottom center
    let g_str = if !car.engine_on {
        "OFF".to_string()
    } else if car.gear < 0 {
        "R".to_string()
    } else if car.speed.abs() < 0.4 && car.input.throttle < 0.02 {
        "N".to_string()
    } else {
        format!("{}", car.gear + 1)
    };
    let g_fs = (26.0 * u).round() as u16;
    txt(fb, &g_str, tach_cx - txt_w(fb, &g_str, g_fs) * 0.5, tach_cy + 42.0 * u, g_fs, WHITE);
    let r_fs = (9.5 * u).round() as u16;
    txt(f, "x1000 RPM", tach_cx - txt_w(f, "x1000 RPM", r_fs) * 0.5, tach_cy + 55.0 * u, r_fs, Color::new(0.78, 0.82, 0.88, 0.80));

    // Perimeter circular indicator badges (Headlights, Handbrake (P), TC, ABS)
    let badge_items: [(&str, bool, Color); 4] = [
        ("D", day_hour < 6.5 || day_hour > 17.2, Color::new(0.25, 0.85, 0.45, 1.0)),
        ("P", car.input.handbrake, Color::new(0.95, 0.25, 0.22, 1.0)),
        ("TC", car.tc, Color::new(0.98, 0.68, 0.15, 1.0)),
        ("ABS", car.abs, Color::new(0.98, 0.68, 0.15, 1.0)),
    ];
    for (bi, (blbl, bon, bcol)) in badge_items.iter().enumerate() {
        let bang = (18.0 + bi as f32 * 22.0).to_radians();
        let bx = tach_cx + bang.cos() * (tach_r * 0.86);
        let by = tach_cy + bang.sin() * (tach_r * 0.86);
        draw_circle(bx, by, 9.0 * u, Color::new(0.10, 0.12, 0.15, 0.95));
        draw_circle_lines(bx, by, 9.0 * u, 1.4 * u, if *bon { *bcol } else { Color::new(1.0, 1.0, 1.0, 0.25) });
        let bfs = (8.5 * u).round() as u16;
        txt(fb, blbl, bx - txt_w(fb, blbl, bfs) * 0.5, by + 3.0 * u, bfs, if *bon { *bcol } else { Color::new(1.0, 1.0, 1.0, 0.40) });
    }

    // -----------------------------------------------------------------
    // 3. Circular Turbo / Boost Bar Sub-Gauge (`0.0 Bar`) above Tachometer (`image_e5faee.jpg`)
    // -----------------------------------------------------------------
    let boost_r = 35.0 * u;
    let boost_cx = sw - 18.0 * u - boost_r;
    let boost_cy = tach_cy - tach_r - boost_r - 10.0 * u;
    draw_circle(boost_cx, boost_cy, boost_r + 2.5 * u, Color::new(0.07, 0.08, 0.10, 0.88));
    draw_circle(boost_cx, boost_cy, boost_r, Color::new(0.13, 0.15, 0.18, 0.90));
    draw_arc_band(boost_cx, boost_cy, boost_r * 0.74, boost_r * 0.94, 135.0, 385.0, 18, Color::new(0.06, 0.07, 0.09, 0.88));
    draw_arc_band(boost_cx, boost_cy, boost_r * 0.74, boost_r * 0.94, 315.0, 385.0, 8, Color::new(0.75, 0.15, 0.12, 0.80));
    let b_frac = ((car.boost_bar + 0.4) / 1.9).clamp(0.0, 1.0);
    draw_arc_band(boost_cx, boost_cy, boost_r * 0.78, boost_r * 0.94, 135.0, 135.0 + 250.0 * b_frac, 16, WHITE);
    let b_str = format!("{:.1}", car.boost_bar.max(0.0));
    let b_fs = (16.5 * u).round() as u16;
    txt(fb, &b_str, boost_cx - txt_w(fb, &b_str, b_fs) * 0.5, boost_cy + 3.0 * u, b_fs, WHITE);
    let bar_fs = (9.0 * u).round() as u16;
    txt(f, "Bar", boost_cx - txt_w(f, "Bar", bar_fs) * 0.5, boost_cy + 14.0 * u, bar_fs, Color::new(0.80, 0.84, 0.90, 0.85));

    // -----------------------------------------------------------------
    // 4. Interactive Circular `ENGINE START STOP` Button (`image_e5faee.jpg`)
    // -----------------------------------------------------------------
    let eng_r = 20.0 * u;
    let eng_cx = tach_cx - tach_r - eng_r - 12.0 * u;
    let eng_cy = sh - 22.0 * u - eng_r;
    let (mx, my) = mouse_position();
    let eng_hover = vec2(mx - eng_cx, my - eng_cy).length() <= eng_r;
    if (eng_hover && is_mouse_button_pressed(MouseButton::Left)) || is_key_pressed(KeyCode::I) {
        car.engine_on = !car.engine_on;
    }
    draw_circle(eng_cx, eng_cy, eng_r + 2.0 * u, Color::new(0.25, 0.27, 0.30, 0.90));
    draw_circle(eng_cx, eng_cy, eng_r, if car.engine_on { Color::new(0.12, 0.14, 0.17, 0.95) } else { Color::new(0.35, 0.12, 0.12, 0.95) });
    draw_circle_lines(eng_cx, eng_cy, eng_r - 2.0 * u, 1.5 * u, if car.engine_on { Color::new(0.98, 0.52, 0.10, 0.90) } else { Color::new(0.95, 0.30, 0.25, 0.90) });
    let efs = (7.5 * u).round() as u16;
    txt(fb, "ENGINE", eng_cx - txt_w(fb, "ENGINE", efs) * 0.5, eng_cy - 4.0 * u, efs, WHITE);
    txt(fb, "START", eng_cx - txt_w(fb, "START", efs) * 0.5, eng_cy + 3.5 * u, efs, Color::new(0.98, 0.56, 0.14, 1.0));
    txt(fb, "STOP", eng_cx - txt_w(fb, "STOP", efs) * 0.5, eng_cy + 11.0 * u, efs, WHITE);

    // Bus Passenger Badge when driving Scania Touring 2026 Bus (or when passengers > 0)
    if SPECS[car.spec_idx].hi_kind == HiKind::CrafterBus || car.passengers > 0 {
        let pax_txt = format!(
            "BUS LOAD: {} Pax (+{:.0} kg)  |  Total Mass: {:.0} kg  |  CoG Y: {:.2}m  |  Tire Defl: {:.1}mm  ([ / ] Adjust)",
            car.passengers,
            car.passengers as f32 * car.passenger_mass_kg,
            car.body.total_mass,
            car.cog_height_m,
            car.wheels[0].tire_deflection * 1000.0
        );
        let pfs = (13.0 * u).round() as u16;
        let pw = txt_w(fb, &pax_txt, pfs) + 28.0 * u;
        rrect(sw * 0.5 - pw * 0.5, sh - 38.0 * u, pw, 26.0 * u, 6.0 * u, Color::new(0.05, 0.06, 0.08, 0.86));
        txt(fb, &pax_txt, sw * 0.5 - pw * 0.5 + 14.0 * u, sh - 20.5 * u, pfs, Color::new(0.98, 0.62, 0.18, 1.0));
    }

    // Non-intrusive top-center notification toast
    if let Some((msg, _)) = toast {
        let tfs = (15.0 * u).round() as u16;
        let tw = txt_w(fb, msg, tfs) + 36.0 * u;
        rrect(sw * 0.5 - tw * 0.5, 16.0 * u, tw, 32.0 * u, 6.0 * u, Color::new(0.05, 0.06, 0.08, 0.88));
        txt(fb, msg, sw * 0.5 - tw * 0.5 + 18.0 * u, 37.0 * u, tfs, WHITE);
    }

    // Optional Profiler & Telemetry Panel (`F1`)
    if show_profiler {
        rrect(18.0, 16.0, 560.0, 96.0, 10.0, PANEL);
        txt(fb, &format!("{}  |  {} FPS ({:.1} ms)", spec_name, get_fps(), get_frame_time() * 1000.0), 32.0, 42.0, 19, Color::new(0.35, 0.95, 0.55, 1.0));
        let w0 = &car.wheels[0];
        let surf_nm = match w0.surface_id { 0 => "Dry Asphalt", 1 => "Wet Asphalt", 2 => "Dirt/Gravel", _ => "Grass" };
        txt(f, &format!(
            "Pacejka Tire [FL]: Load {:.0}N | SlipRatio {:+.2} | SlipAng {:+.1}آ° | Temp {:.0}آ°C | Wear {:.1}% | {:.0} PSI ({})",
            w0.load, w0.slip_ratio, w0.slip_angle_deg, w0.temp_c, w0.wear * 100.0, w0.pressure_psi, surf_nm
        ), 32.0, 66.0, 13, WHITE);
        let hh = (day_hour as u32) % 24;
        let mm = ((day_hour.fract() * 60.0) as u32) % 60;
        txt(f, &format!(
            "Mass: {:.0} kg | CoG Y: {:.2}m | Impact: {:.1}G (Peak {:.1}G) | Time {:02}:{:02} | Cam {}",
            car.body.total_mass, car.cog_height_m, car.impact_g, car.peak_g, hh, mm, cam_mode + 1
        ), 32.0, 88.0, 13, Color::new(0.80, 0.85, 0.92, 0.85));
    }

    // Optional Controls Help (`H`)
    if show_help {
        rrect(18.0, 120.0, 580.0, 74.0, 10.0, PANEL);
        let lines = [
            "W/S Throttle/Brake | A/D Steer | SPACE Handbrake | C Camera | I Ignition | [ / ] Bus Passengers",
            "0 AMG One | - AMG GT Black Series | = Scania Touring Bus | 1-9 Cars | F8 4K Photo Mode | F10 Editor",
            "F1 Profiler & Pacejka Telemetry | F5 Online | N/M Time | R Reset | T Repair | H Hide Help | ESC Menu",
        ];
        for (i, l) in lines.iter().enumerate() {
            txt(f, l, 30.0, 141.0 + i as f32 * 21.0, 12, Color::new(0.92, 0.94, 0.98, 0.88));
        }
    }
}

fn yaw_from_quat(q: Quat) -> f32 {
    let fwd = q * Vec3::Z;
    fwd.x.atan2(fwd.z)
}

fn model_to_spec(model: &str) -> usize {
    for (i, s) in SPECS.iter().enumerate() {
        if s.name.eq_ignore_ascii_case(model) || model.to_lowercase().contains(&s.name.to_lowercase()) {
            return i;
        }
    }
    let m = model.to_lowercase();
    if m.contains("black") || (m.contains("gt") && m.contains("amg")) { return 1; }
    if m.contains("bmw") || m.contains("m3") || m.contains("touring") { return 2; }
    if m.contains("porsche") || m.contains("gt3") || m.contains("911") { return 3; }
    if m.contains("crafter") || m.contains("volkswagen") || m.contains("bus") || m.contains("scania") { return 4; }
    0
}

fn collide_player_vs_all_remotes(cars: &mut Vec<Car>, remotes: &HashMap<u32, Car>, h: f32) {
    if cars.is_empty() || remotes.is_empty() { return; }
    for remote in remotes.values() {
        collide_local_vs_remote(&mut cars[0], remote, h);
    }
}

struct Game {
    level: level::Level,
    ed: editor::Editor,
    photo: photo::PhotoMode,
    world: World,
    scene: hood::Scene,
    cars: Vec<Car>,
    cam: Cam,
    acc: f32,
    time_scale: f32,
    paused: bool,
    help: bool,
    show_profiler: bool,
    hud_toast: Option<(String, f32)>,
    spawn_pos: Vec3,
    // multiplayer
    net: Option<Arc<SharedNet>>,
    net_seq: u32,
    net_spawn_sent: bool,
    net_send_acc: f32,
    net_status_msg: String,
    remote_cars: HashMap<u32, Car>,
    chat_open: bool,
    chat_input: String,
    world_seq: u32,
    full_send_acc: f32,
    last_synced_hash: u64,
}

async fn new_game_async(res: &Res, spec_idx: usize, map_id: u8, front: &ui::Front, tip_idx: usize) -> Game {
    let (level, world, scene) = new_stage_async(res, map_id, tip_idx).await;
    draw_loading_screen(res, 7, 7, "Initializing Pacejka tire model & soft-body lattice", tip_idx);
    next_frame().await;
    let mut cars = spawn_scene(&res.tex);
    let (p0, yaw0) = level::spawn(0);
    let mut p = Car::new(spec_idx, res.tex.clone(), true);
    let (_, rgb) = ui::GARAGE_PAINTS[front.custom_paint_idx % ui::GARAGE_PAINTS.len()];
    p.paint = vec3(rgb[0], rgb[1], rgb[2]);
    for w in p.wheels.iter_mut() {
        w.pressure_psi = front.custom_tire_psi;
    }
    if SPECS[spec_idx].hi_kind == HiKind::CrafterBus {
        p.set_passengers(front.custom_bus_passengers);
    }
    p.respawn(Vec3::from_array(p0), yaw0);
    cars[0] = p;
    let init_hash = crashdrive_shared::fnv1a_64(level.to_json().as_bytes());
    Game {
        level,
        ed: editor::Editor::new(),
        photo: photo::PhotoMode::new(),
        world,
        scene,
        cars,
        cam: Cam { pos: vec3(-15.0, 4.0, 15.0), target: vec3(-15.0, 1.0, 25.0), heading: Vec3::Z, mode: 0, orbit: 0.6, yaw: 0.0, pitch: 0.0, zoom: 1.0, idle: 0.0 },
        acc: 0.0,
        time_scale: 1.0,
        paused: false,
        help: false,
        show_profiler: false,
        hud_toast: Some((format!("Driving {} in {}", SPECS[spec_idx].name, level::DEFAULT_MAP_TITLE), 3.5)),
        spawn_pos: Vec3::from_array(p0),
        net: None,
        net_seq: 0,
        net_spawn_sent: false,
        net_send_acc: 0.0,
        net_status_msg: String::new(),
        remote_cars: HashMap::new(),
        chat_open: false,
        chat_input: String::new(),
        world_seq: 0,
        full_send_acc: 0.0,
        last_synced_hash: init_hash,
    }
}

/// Captures a high-resolution 4K (`3840x2160`) offscreen screenshot directly from the in-game camera (`Task 3`).
fn capture_4k_in_game(res: &Res, g: &Game, cp: Vec3, ct: Vec3, fov_deg: f32, t: f32, env: &EnvLight, out_path: &str) {
    let (w, h) = (3840u32, 2160u32);
    let rt_4k = render_target(w, h);
    rt_4k.texture.set_filter(FilterMode::Linear);
    let vp = Mat4::perspective_rh_gl(fov_deg.to_radians(), w as f32 / h as f32, 0.15, 2000.0) * Mat4::look_at_rh(cp, ct, Vec3::Y);
    set_camera(&Camera2D { render_target: Some(rt_4k.clone()), zoom: vec2(1.0, 1.0), target: vec2(0.0, 0.0), ..Default::default() });
    clear_background(BLACK);
    draw_sky_env(res, vp, cp, t, env);
    lit_begin_env(res, vp, cp, t, env);
    hood::draw_ex(&g.scene, &g.level, &res.lit, vp, cp, (ct - cp).normalize_or_zero(), false, 2);
    for c in g.cars.iter() {
        draw_car_full(res, c);
    }
    gl_use_default_material();
    set_default_camera();
    let img = rt_4k.texture.get_texture_data();
    let _ = std::fs::create_dir_all("screenshots");
    img.export_png(out_path);
}

fn game_frame(res: &Res, g: &mut Game, settings: &mut servers::Settings, t: f32, dt_real: f32, keys_override: Option<Input>) -> bool {
    if let Some((_, ttl)) = &mut g.hud_toast {
        *ttl -= dt_real;
    }
    if matches!(&g.hud_toast, Some((_, ttl)) if *ttl <= 0.0) {
        g.hud_toast = None;
    }

    // Toggle 4K Photo Mode (`F8`)
    if is_key_pressed(KeyCode::F8) && !g.chat_open && keys_override.is_none() {
        g.photo.active = !g.photo.active;
        if g.photo.active {
            g.ed.active = false;
            let (cpos, cyaw, cspec) = g.photo.apply_preset(g.photo.preset_idx, &mut g.ed.day_hour);
            if g.net.is_none() {
                if g.cars[0].spec_idx != cspec {
                    g.cars[0] = Car::new(cspec, res.tex.clone(), true);
                }
                g.cars[0].respawn(cpos, cyaw);
            }
        }
    }

    // Toggle World Editor (`F10`) â€” Server-Authoritative Permission Check (`Task 4.4`)
    if is_key_pressed(KeyCode::F10) && !g.chat_open && keys_override.is_none() {
        if let Some(net) = g.net.as_ref() {
            if net.is_connected() && !net.can_build() {
                g.hud_toast = Some(("Server Denied: Ask admin to run `build allow <your_name>` or `op <your_name>` in console".into(), 4.5));
            } else if g.ed.active {
                g.ed.active = false;
            } else {
                g.photo.active = false;
                hood::rescan_custom_assets(&mut g.scene.assets);
                g.ed.enter(g.cam.pos, g.cam.target);
            }
        } else if g.ed.active {
            g.ed.active = false;
        } else {
            g.photo.active = false;
            hood::rescan_custom_assets(&mut g.scene.assets);
            g.ed.enter(g.cam.pos, g.cam.target);
        }
    }

    // Adaptive fixed physics timestep based on Graphics/Performance Preset (`Task 1.2 & Task 6.6`)
    // Rock-solid 2400 Hz physics integration rate across all quality presets (prevents any CFL instability or black screen)
    let (h, quality_substeps): (f32, usize) = match settings.gfx_preset {
        0 => (1.0 / 2400.0, g.cars[0].pcfg.substeps_low as usize),
        1 => (1.0 / 2400.0, g.cars[0].pcfg.substeps_med as usize),
        _ => (1.0 / 2400.0, g.cars[0].pcfg.substeps_high as usize),
    };
    // Keep the fixed 2400 Hz solver synchronized with real elapsed frame time.
    const MAX_CATCHUP_SUBSTEPS: usize = 240;
    let required_substeps = (((dt_real * g.time_scale + g.acc).max(0.0) / h).ceil() as usize)
        .clamp(1, MAX_CATCHUP_SUBSTEPS);
    let max_substeps = quality_substeps.max(required_substeps);

    let mut keys = Input::default();
    let mut back = false;

    let mut chat_blocking = false;
    if !g.ed.active && !g.photo.active {
        if let Some(net) = g.net.as_ref() {
            if g.chat_open {
                chat_blocking = true;
                while let Some(c) = get_char_pressed() {
                    if c.is_ascii() && !c.is_control() && g.chat_input.len() < 120 {
                        g.chat_input.push(c);
                    }
                }
                if is_key_pressed(KeyCode::Backspace) {
                    g.chat_input.pop();
                }
                if is_key_pressed(KeyCode::Enter) {
                    let msg = g.chat_input.trim().to_string();
                    if !msg.is_empty() {
                        net.chat(&msg);
                        net.chat.lock().unwrap().push(("You".into(), msg));
                    }
                    g.chat_input.clear();
                    g.chat_open = false;
                }
                if is_key_pressed(KeyCode::Escape) {
                    g.chat_input.clear();
                    g.chat_open = false;
                    back = false;
                }
            } else if is_key_pressed(KeyCode::Enter) && net.is_connected() {
                g.chat_open = true;
                g.chat_input.clear();
            }
        }
    }

    if g.photo.active {
        let preset_keys = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4, KeyCode::Key5, KeyCode::Key6];
        for (pi, &pk) in preset_keys.iter().enumerate() {
            if is_key_pressed(pk) {
                let (cpos, cyaw, cspec) = g.photo.apply_preset(pi, &mut g.ed.day_hour);
                if g.net.is_none() {
                    if g.cars[0].spec_idx != cspec {
                        g.cars[0] = Car::new(cspec, res.tex.clone(), true);
                    }
                    g.cars[0].respawn(cpos, cyaw);
                }
            }
        }
        if is_key_pressed(KeyCode::Escape) {
            g.photo.active = false;
        }
    } else if chat_blocking {
        keys = Input::default();
    } else if let Some(k) = keys_override {
        keys = k;
    } else if !g.ed.active {
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) { keys.throttle = 1.0; }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) { keys.brake = 1.0; }
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) && g.cam.mode != 2 { keys.steer += 1.0; }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) && g.cam.mode != 2 { keys.steer -= 1.0; }
        keys.handbrake = is_key_down(KeyCode::Space);
        if is_key_pressed(KeyCode::C) { g.cam.mode = (g.cam.mode + 1) % 3; g.cam.yaw = 0.0; g.cam.pitch = 0.0; }
        if is_key_pressed(KeyCode::G) { g.time_scale = if g.time_scale < 0.5 { 1.0 } else { 0.2 }; }
        if is_key_pressed(KeyCode::P) { g.paused = !g.paused; }
        if is_key_pressed(KeyCode::H) { g.help = !g.help; }
        if is_key_pressed(KeyCode::F1) { g.show_profiler = !g.show_profiler; }
        if is_key_pressed(KeyCode::V) { g.cars[0].tc = !g.cars[0].tc; }
        if is_key_pressed(KeyCode::B) { g.cars[0].abs = !g.cars[0].abs; }
        if is_key_pressed(KeyCode::Escape) && !g.chat_open { back = true; }

        // Bus Passenger Load Adjustment (`[` and `]` keys â€” Task 6.5)
        if is_key_pressed(KeyCode::LeftBracket) {
            let np = g.cars[0].passengers.saturating_sub(5);
            g.cars[0].set_passengers(np);
            g.hud_toast = Some((format!("Passengers: {} (+{:.0} kg, Total {:.0} kg, CoG {:.2}m)", np, np as f32 * 80.0, g.cars[0].body.total_mass, g.cars[0].cog_height_m), 3.0));
        }
        if is_key_pressed(KeyCode::RightBracket) {
            let np = g.cars[0].passengers + 5;
            g.cars[0].set_passengers(np);
            g.hud_toast = Some((format!("Passengers: {} (+{:.0} kg, Total {:.0} kg, CoG {:.2}m)", g.cars[0].passengers, g.cars[0].passengers as f32 * 80.0, g.cars[0].body.total_mass, g.cars[0].cog_height_m), 3.0));
        }

        if is_key_pressed(KeyCode::F5) {
            if g.net.is_none() {
                g.net_status_msg = "Connecting to 127.0.0.1:30814 ...".into();
                g.net = Some(start_net_client("127.0.0.1:30814", &settings.name));
                g.net_spawn_sent = false;
                g.net_seq = 0;
            } else {
                if let Some(n) = g.net.take() {
                    n.disconnect();
                }
                g.net_status_msg = "Disconnected".into();
            }
        }
        if is_key_pressed(KeyCode::R) {
            let m = g.cars[0].spec_idx;
            let (p0, yaw0) = level::spawn(0);
            g.cars = spawn_scene(&res.tex);
            let mut p = Car::new(m, res.tex.clone(), true);
            p.respawn(Vec3::from_array(p0), yaw0);
            g.cars[0] = p;
        }
        if is_key_pressed(KeyCode::T) {
            let f = g.cars[0].frame;
            let yaw = f.fwd.x.atan2(f.fwd.z);
            let c = f.c;
            let gh = g.world.ground_h(c.x, c.z);
            g.cars[0].respawn(vec3(c.x, gh + 0.15, c.z), yaw);
        }
        let switch_keys: [(KeyCode, usize); 8] = [
            (KeyCode::Key1, 0), (KeyCode::Key2, 1), (KeyCode::Key3, 2), (KeyCode::Key4, 3),
            (KeyCode::Key5, 4), (KeyCode::Key0, 0), (KeyCode::Minus, 1), (KeyCode::Equal, 4),
        ];
        for (k, i) in switch_keys {
            if is_key_pressed(k) && i < SPECS.len() {
                let f = g.cars[0].frame;
                let yaw = f.fwd.x.atan2(f.fwd.z);
                let c = f.c;
                let mut p = Car::new(i, res.tex.clone(), true);
                let gh = g.world.ground_h(c.x, c.z);
                p.respawn(vec3(c.x, gh + 0.12, c.z), yaw);
                g.cars[0] = p;
                g.hud_toast = Some((format!("Switched to {}", SPECS[i].name), 2.5));
            }
        }
    }
    g.cars[0].control(keys, dt_real);

    // ---- Server-Authoritative Map & Environment Sync (`Task 4.2 & 4.3`)
    if let Some(net) = g.net.as_ref() {
        if net.is_connected() {
            let env_state = net.auth_env.lock().unwrap().clone();
            g.ed.day_hour = env_state.day_hour;
            if !env_state.can_build && !env_state.is_op && g.ed.active {
                g.ed.active = false;
                g.hud_toast = Some(("Build permission revoked by server admin".into(), 3.5));
            }
            if let Some((mname, mhash, mjson)) = net.incoming_map.lock().unwrap().take() {
                if mhash != g.last_synced_hash {
                    if let Some(new_lvl) = level::Level::from_json(&mjson) {
                        g.level = new_lvl;
                        g.level.dirty = true;
                        g.level.forest_dirty = true;
                        stage_sync(res, &mut g.level, &mut g.world, &mut g.scene);
                        g.last_synced_hash = mhash;
                        g.hud_toast = Some((format!("Synchronized server world map: {}", mname), 3.5));
                    }
                }
            }
        }
        net.update_remotes(dt_real);
        let remotes = net.snapshot_remotes();
        for rv in &remotes {
            if !g.remote_cars.contains_key(&rv.id) {
                let spec_idx = model_to_spec(&rv.model);
                let mut c = Car::new(spec_idx, res.tex.clone(), false);
                c.respawn(rv.pos, yaw_from_quat(rv.rot));
                g.remote_cars.insert(rv.id, c);
            }
        }
        g.remote_cars.retain(|id, _| remotes.iter().any(|r| r.id == *id));
        for rv in &remotes {
            if let Some(c) = g.remote_cars.get_mut(&rv.id) {
                apply_vehicle_state(c, rv.pos, rv.rot, &rv.wheels, &rv.full);
            }
        }
    }

    if !g.paused && !g.ed.active && !g.photo.active {
        g.acc += dt_real * g.time_scale;
        let mut guard = 0;
        while g.acc >= h && guard < max_substeps {
            for c in g.cars.iter_mut() {
                if !c.asleep { c.update_frame(); }
            }
            for c in g.cars.iter_mut() {
                if !c.asleep { c.forces(h, &g.world); }
            }
            let n = g.cars.len();
            for i in 0..n {
                for j in 0..n {
                    if i == j { continue; }
                    if g.cars[i].asleep { continue; }
                    if !aabb_overlap(&g.cars[i].aabb, &g.cars[j].aabb, 0.25) { continue; }
                    if g.cars[j].asleep { g.cars[j].update_frame(); }
                    let (a, b) = if i < j {
                        let (l, r) = cars_split_at_mut_helper(&mut g.cars, i, j);
                        (l, r)
                    } else {
                        let (r, l) = cars_split_at_mut_helper(&mut g.cars, j, i);
                        (l, r)
                    };
                    collide_into(a, b, h);
                }
            }
            collide_player_vs_all_remotes(&mut g.cars, &g.remote_cars, h);
            for c in g.cars.iter_mut() {
                if !c.asleep { c.integrate(h); }
            }
            g.acc -= h;
            guard += 1;
        }
        if guard >= max_substeps { g.acc = 0.0; }
        frame_housekeeping(&mut g.cars, dt_real * g.time_scale);
        for c in g.cars.iter_mut() { if !c.asleep { c.update_frame(); } }
    }

    let (mut cp, mut ct) = if g.photo.active {
        g.photo.step_camera(dt_real)
    } else if g.ed.active {
        g.ed.cam(dt_real)
    } else {
        update_camera(&mut g.cam, &g.cars[0], settings.head_motion, dt_real)
    };
    if let (Ok(a), Ok(b)) = (std::env::var("CAMPOS"), std::env::var("CAMLOOK")) {
        let p = |s: &str| { let v: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect(); vec3(v[0], v[1], v[2]) };
        cp = p(&a); ct = p(&b);
    }
    let aspect = screen_width() / screen_height();
    let cockpit = !g.ed.active && !g.photo.active && g.cam.mode == 1;
    hicar::COCKPIT.with(|c| c.set(cockpit));
    let fov_deg = if g.photo.active { g.photo.fov_deg } else if cockpit { settings.cockpit_fov.clamp(45.0, 80.0) } else { 60.0 };
    let vp = Mat4::perspective_rh_gl(fov_deg.to_radians(), aspect, if cockpit { 0.06 } else { 0.25 }, 1800.0) * Mat4::look_at_rh(cp, ct, Vec3::Y);

    // ---- Time of Day: Local 24-minute cycle offline, or Server-Authoritative online (`Task 4.2`)
    let online_connected = g.net.as_ref().map_or(false, |n| n.is_connected());
    if !online_connected {
        if !g.paused && !g.photo.active {
            g.ed.day_hour = (g.ed.day_hour + dt_real / 60.0) % 24.0;
        }
        if !g.ed.active && !chat_blocking {
            if is_key_down(KeyCode::N) { g.ed.day_hour = (g.ed.day_hour + 24.0 - dt_real * 2.8) % 24.0; }
            if is_key_down(KeyCode::M) { g.ed.day_hour = (g.ed.day_hour + dt_real * 2.8) % 24.0; }
        }
    } else if !g.ed.active && !chat_blocking && (is_key_pressed(KeyCode::N) || is_key_pressed(KeyCode::M)) {
        if let Some(net) = g.net.as_ref() {
            if net.is_op() {
                let target_h = if is_key_pressed(KeyCode::N) { (g.ed.day_hour + 23.0) % 24.0 } else { (g.ed.day_hour + 1.0) % 24.0 };
                net.request_time_set(target_h);
            } else {
                g.hud_toast = Some(("Server-Authoritative Time: Only OPs can change time online".into(), 3.0));
            }
        }
    }

    let pf = g.cars[0].frame;
    let env = env_light(g.ed.day_hour, pf.c + pf.fwd * 1.8 + pf.up * 0.65, pf.fwd);

    // Only update render meshes for awake/dirty cars (sleeping cars cost 0 CPU!)
    for c in g.cars.iter_mut() { c.update_render(); }

    // ---- Optional Planar Shadow Pass (Skipped on Low preset for huge FPS boost on weak PCs!)
    let pc = if g.ed.active || g.photo.active { vec3(cp.x, 0.0, cp.z) } else { g.cars[0].frame.c };
    let gy = g.world.ground_h(pc.x, pc.z);
    let center = vec3(pc.x, gy, pc.z);
    let sh_r = 68.0f32;
    let svp = Mat4::orthographic_rh_gl(-sh_r, sh_r, -sh_r, sh_r, 1.0, 220.0) * Mat4::look_at_rh(center + Vec3::Y * 100.0, center, Vec3::Z);

    if settings.shadows_on {
        set_camera(&Camera2D { render_target: Some(res.rt.clone()), zoom: vec2(1.0, 1.0), target: vec2(0.0, 0.0), ..Default::default() });
        clear_background(WHITE);
        gl_use_material(&res.shcast);
        res.shcast.set_uniform("SVP", svp);
        res.shcast.set_uniform("MODEL", Mat4::IDENTITY);
        res.shcast.set_uniform("sun_dir", env.sh_dir);
        res.shcast.set_uniform("misc", vec4(0.0, gy + 0.02, 0.0, 0.0));
        hood::draw_shadows(&g.scene, &g.level, &res.shcast, center, sh_r);
        res.shcast.set_uniform("MODEL", Mat4::IDENTITY);
        for c in g.cars.iter() {
            if (c.frame.c - pc).length() > sh_r + 10.0 { continue; }
            draw_mesh(&c.render);
        }
        gl_use_default_material();
        set_default_camera();
    }

    // ---- Main 3D Pass (with Static Batching, 3-Tier LOD & Frustum/Facade Culling)
    clear_background(BLACK);
    draw_sky_env(res, vp, cp, t, &env);
    lit_begin_env(res, vp, cp, t, &env);
    hood::draw_ex(&g.scene, &g.level, &res.lit, vp, cp, (ct - cp).normalize_or_zero(), g.ed.active, settings.gfx_preset);

    if settings.shadows_on {
        gl_use_material(&res.shdec);
        res.shdec.set_uniform("VP", vp);
        res.shdec.set_uniform("SVP", svp);
        res.shdec.set_uniform("sh_col", env.sh_col);
        let y = gy + 0.042;
        let v = |x: f32, z: f32| Vertex { position: vec3(pc.x + x, y, pc.z + z), uv: vec2(0.0, 0.0), color: [255, 255, 255, 255], normal: vec4(0.0, 1.0, 0.0, 0.0) };
        draw_mesh(&Mesh { vertices: vec![v(-sh_r, -sh_r), v(sh_r, -sh_r), v(sh_r, sh_r), v(-sh_r, sh_r)], indices: vec![0, 1, 2, 0, 2, 3], texture: Some(res.rt.texture.clone()) });
    }

    lit_begin_env(res, vp, cp, t, &env);
    let cam_frustum = hood::Frustum::from_vp(vp);
    for c in g.cars.iter() {
        if !c.is_player && !cam_frustum.aabb_visible(c.aabb.0 - Vec3::splat(2.0), c.aabb.1 + Vec3::splat(2.0)) {
            continue;
        }
        draw_car_full(res, c);
    }

    if let Some(net) = g.net.as_ref() {
        for rv in net.snapshot_remotes() {
            if let Some(c) = g.remote_cars.get_mut(&rv.id) {
                apply_vehicle_state(c, rv.pos, rv.rot, &rv.wheels, &rv.full);
                c.update_render_visual();
                draw_car_full(res, c);
            }
        }
    }

    gl_use_default_material();
    set_default_camera();

    // ---- 4K Screenshot Capture (`F12` or Photo Mode capture)
    if g.photo.request_4k_capture || (is_key_pressed(KeyCode::F12) && !g.photo.active) {
        g.photo.request_4k_capture = false;
        let out_file = format!("screenshots/shot_{:02}_4k.png", g.photo.preset_idx + 1);
        capture_4k_in_game(res, g, cp, ct, fov_deg, t, &env, &out_file);
        g.photo.status_toast = Some((format!("Saved 4K In-Game Screenshot: {}", out_file), 4.0));
        g.hud_toast = Some((format!("Saved 4K In-Game Screenshot: {}", out_file), 4.0));
    }

    // ---- Multiplayer network tick
    if let Some(net) = g.net.as_ref() {
        match net.status() {
            NetStatus::Connecting => g.net_status_msg = "Connecting...".into(),
            NetStatus::Connected { player_id, ref server_name } => {
                let n_remotes = net.remote_vehicles.lock().unwrap().len();
                let role = if net.is_op() { "OP" } else if net.can_build() { "BUILDER" } else { "PLAYER" };
                g.net_status_msg = format!("Online: {} | #{} ({}) | Players: {}", server_name, player_id, role, n_remotes + 1);
                if !g.net_spawn_sent {
                    let f = g.cars[0].frame;
                    let yaw = f.fwd.x.atan2(f.fwd.z);
                    let slot = (player_id as usize) % level::OFFICIAL_SPAWNS.len();
                    let (sp_pos, _) = level::spawn(slot);
                    let spawn = VehicleSpawn {
                        vehicle_id: 0,
                        owner: player_id,
                        model: SPECS[g.cars[0].spec_idx].name.to_string(),
                        pos: if slot == 0 { f.c.to_array() } else { sp_pos },
                        rot_y: yaw,
                        color: g.cars[0].paint.to_array(),
                    };
                    net.request_spawn(spawn);
                    g.net_spawn_sent = true;
                }
            }
            NetStatus::Error(ref e) => g.net_status_msg = format!("Net error: {}", e),
            NetStatus::Disconnected => g.net_status_msg = "Disconnected".into(),
        }

        g.net_send_acc += dt_real;
        g.full_send_acc += dt_real;
        if g.net_send_acc >= 0.033 && net.is_connected() {
            g.net_send_acc = 0.0;
            g.net_seq = g.net_seq.wrapping_add(1);
            let vid = net.my_vehicle_id.lock().unwrap().unwrap_or(0);
            let pid = net.player_id().unwrap_or(0);
            let include_full = g.full_send_acc >= 0.10 || g.cars[0].body.damage_ratio() > 0.002;
            if include_full { g.full_send_acc = 0.0; }
            let state = car_to_state(&g.cars[0], vid, pid, g.net_seq, (t * 1000.0) as u32, include_full);
            net.send_vehicle_state(state);

            if include_full {
                for (slot, car) in g.cars.iter().enumerate().skip(1) {
                    if car.asleep && car.body.damage_ratio() < 0.001 { continue; }
                    g.world_seq = g.world_seq.wrapping_add(1);
                    let st = car_to_world_state(car, slot as u16, pid, g.world_seq);
                    net.send_world_car(st);
                }
            }
        }

        if let Ok(map) = net.world_cars.lock() {
            for (slot, st) in map.iter() {
                let i = *slot as usize;
                if i == 0 || i >= g.cars.len() { continue; }
                let pid = net.player_id().unwrap_or(0);
                if st.authority == pid && !g.cars[i].asleep { continue; }
                apply_world_car_state(&mut g.cars[i], st);
            }
        }
    }

    if g.photo.active {
        g.photo.draw_overlay(res, g.ed.day_hour, dt_real);
    } else if g.ed.active {
        let was_dirty = g.level.dirty;
        g.ed.ui(res, &mut g.level, &mut g.scene.assets, vp, cp, screen_width(), screen_height(), dt_real);
        stage_sync(res, &mut g.level, &mut g.world, &mut g.scene);
        // If connected to an authoritative server and the builder modified the level, push the edit live!
        if was_dirty {
            if let Some(net) = g.net.as_ref() {
                if net.is_connected() && net.can_build() {
                    let json = g.level.to_json();
                    let h = crashdrive_shared::fnv1a_64(json.as_bytes());
                    if h != g.last_synced_hash {
                        g.last_synced_hash = h;
                        net.request_world_edit(json);
                    }
                }
            }
        }
    } else {
        let spec_name = SPECS[g.cars[0].spec_idx].name;
        draw_hud(res, &mut g.cars[0], &g.level, spec_name, g.cam.mode, g.ed.day_hour, g.show_profiler, g.help, &g.hud_toast);
        if !g.net_status_msg.is_empty() {
            let f = &res.font;
            let w = measure_text(&g.net_status_msg, Some(f), 16, 1.0).width;
            draw_text_ex(&g.net_status_msg, screen_width() - w - 20.0, 28.0,
                TextParams { font: Some(f), font_size: 16, color: Color::new(0.4, 1.0, 0.5, 0.95), ..Default::default() });
        }
        if g.net.as_ref().map(|n| n.is_connected()).unwrap_or(false) {
            draw_chat_ui(res, g);
        }
    }

    back
}

fn cars_split_at_mut_helper(cars: &mut [Car], i: usize, j: usize) -> (&mut Car, &mut Car) {
    let (l, r) = cars.split_at_mut(j);
    (&mut l[i], &mut r[0])
}

fn draw_chat_ui(res: &Res, g: &Game) {
    let f = &res.font;
    let fb = &res.font_b;
    let sh = screen_height();
    let panel_w = 380.0;
    let panel_x = 18.0;
    let line_h = 20.0;
    let max_lines = 6;

    let messages: Vec<(String, String)> = g.net.as_ref()
        .map(|n| n.chat.lock().unwrap().clone())
        .unwrap_or_default();

    if messages.is_empty() && !g.chat_open {
        return;
    }

    let visible = messages.len().min(max_lines);
    let hist_h = 16.0 + visible as f32 * line_h;
    let panel_h = hist_h + if g.chat_open { 46.0 } else { 8.0 };
    let panel_y = sh - 230.0 - panel_h;

    draw_rectangle(panel_x, panel_y, panel_w, panel_h, Color::new(0.02, 0.03, 0.05, 0.72));
    draw_rectangle_lines(panel_x, panel_y, panel_w, panel_h, 1.5, Color::new(0.98, 0.52, 0.10, 0.45));

    let start = messages.len().saturating_sub(max_lines);
    for (i, (name, msg)) in messages[start..].iter().enumerate() {
        let y = panel_y + 22.0 + i as f32 * line_h;
        draw_text_ex(name, panel_x + 10.0, y,
            TextParams { font: Some(fb), font_size: 14, color: Color::new(1.0, 0.72, 0.25, 1.0), ..Default::default() });
        let nw = measure_text(name, Some(fb), 14, 1.0).width;
        draw_text_ex(&format!(": {}", msg), panel_x + 12.0 + nw, y,
            TextParams { font: Some(f), font_size: 14, color: WHITE, ..Default::default() });
    }

    if g.chat_open {
        let box_y = panel_y + panel_h - 38.0;
        draw_rectangle(panel_x + 8.0, box_y, panel_w - 16.0, 28.0, Color::new(0.0, 0.0, 0.0, 0.85));
        draw_rectangle_lines(panel_x + 8.0, box_y, panel_w - 16.0, 28.0, 1.5, Color::new(0.98, 0.52, 0.10, 0.85));
        let caret = if ((get_time() * 2.0) as i32) % 2 == 0 { "|" } else { " " };
        let shown = if g.chat_input.is_empty() {
            format!("Type message... {}", caret)
        } else {
            format!("{}{}", g.chat_input, caret)
        };
        draw_text_ex(&shown, panel_x + 14.0, box_y + 19.0,
            TextParams { font: Some(f), font_size: 14, color: WHITE, ..Default::default() });
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("APEX.drive Panic: {}\n", info);
        eprintln!("{}", msg);
        let _ = std::fs::write("crash_log.txt", &msg);
    }));
    let mut c: macroquad::conf::Conf = conf().into();
    c.draw_call_vertex_capacity = 55000;
    c.draw_call_index_capacity = 190000;
    macroquad::Window::from_config(c, amain());
}

async fn amain() {
    let args: Vec<String> = std::env::args().collect();
    let capture_4k = args.iter().any(|a| a == "--capture-4k");
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    gl_set_drawcall_buffer_capacity(55000, 190000);
    let mods_dir = arg("--mods").map(std::path::PathBuf::from).unwrap_or_else(|| {
        let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.join("mods")));
        match exe { Some(p) if p.exists() => p, _ => std::path::PathBuf::from("mods") }
    });

    // Progressive Non-Blocking Startup Loading Screen (`Task 9` / `image_e58d76.png`)
    let mut res = load_res();
    res.shadow_tex = shadow_texture();
    let mut front = ui::Front::new();

    draw_loading_screen(&res, 1, 7, "Loading main resources", 0);
    next_frame().await;

    draw_loading_screen(&res, 3, 7, "Initializing high-detail vehicle catalog & physics engine", 1);
    next_frame().await;
    let mut menu = build_menu(&mods_dir);

    draw_loading_screen(&res, 5, 7, "Loading menu soundtrack & audio engine", 2);
    next_frame().await;
    let mut music = ui::Music::load().await;

    draw_loading_screen(&res, 7, 7, "Preparing Lumberyard & Urban District cinematic studio", 3);
    next_frame().await;

    // Pre-render 4 cyclic cinematic loading screen backdrops from the studio so loading screens display a static shot that cycles on each entry
    {
        println!("-- Main Menu directly!"); if false { let st = new_stage_sync(&res);
        for i in 0..4 {
            let shot = &photo::OFFICIAL_MAP_SHOTS[i % photo::OFFICIAL_MAP_SHOTS.len()];
            let rt_shot = render_target(960, 540);
            rt_shot.texture.set_filter(FilterMode::Linear);
            let vp = Mat4::perspective_rh_gl(shot.fov_deg.to_radians(), 960.0 / 540.0, 0.15, 1800.0) * Mat4::look_at_rh(shot.cam_pos, shot.cam_look, Vec3::Y);
            let env = env_light(shot.day_hour, shot.car_pos + vec3(0.0, 0.65, 1.8), Vec3::Z);
            set_camera(&Camera2D { render_target: Some(rt_shot.clone()), zoom: vec2(1.0, 1.0), target: vec2(0.0, 0.0), ..Default::default() });
            clear_background(BLACK);
            draw_sky_env(&res, vp, shot.cam_pos, 0.0, &env);
            lit_begin_env(&res, vp, shot.cam_pos, 0.0, &env);
            hood::draw_ex(&st.2, &st.0, &res.lit, vp, shot.cam_pos, (shot.cam_look - shot.cam_pos).normalize_or_zero(), false, 1);
            let cspec = shot.car_spec.min(SPECS.len() - 1);
            let mut c = Car::new(cspec, res.tex.clone(), false);
            c.respawn(shot.car_pos, shot.car_yaw);
            draw_car_lit(&mut c, &res);
            gl_use_default_material();
            set_default_camera();
            res.loading_shots.push(rt_shot.texture);
        }
        CACHED_STAGE.with(|cs| *cs.borrow_mut() = Some(st)); }
    }

    // Optional automated 4K screenshot batch exporter (`--capture-4k` / `capture_4k.bat`)
    if false {
        envload::set_max_tex_dim(2048);
        let mut g = new_game_async(&res, 0, 0, &front, 0).await;
        for i in 0..photo::OFFICIAL_MAP_SHOTS.len() {
            let (cpos, cyaw, cspec) = g.photo.apply_preset(i, &mut g.ed.day_hour);
            g.cars[0] = Car::new(cspec, res.tex.clone(), true);
            g.cars[0].respawn(cpos, cyaw);
            g.cars[0].update_render();
            let s = &photo::OFFICIAL_MAP_SHOTS[i];
            let env = env_light(s.day_hour, cpos + vec3(0.0, 0.65, 1.8), Vec3::Z);
            let out_path = format!("screenshots/shot_{:02}_4k.png", i + 1);
            capture_4k_in_game(&res, &g, s.cam_pos, s.cam_look, s.fov_deg, 0.0, &env, &out_path);
            println!("Captured 4K screenshot: {}", out_path);
        }
        return;
    }

    let mut game: Option<Game> = None;
    let mut t = 0.0f32;
    let mut tip_counter = 0usize;
    // Enable disk diagnostics only when explicitly requested. Synchronous file
    // writes during normal menu rendering can introduce avoidable frame-time spikes.
    let debug_log_enabled = std::env::var_os("APEX_DEBUG_LOG").is_some();
    let mut log_acc = 0.0f32;
    if debug_log_enabled {
        let _ = std::fs::write("debug_log.txt", "APEX.drive debug log (fps every 5s)\n");
    }

    loop {
        // Frame presentation is paced by Ogre swapchain VSync.
        let dt = get_frame_time().min(1.0 / 20.0);
        t += dt;
        if debug_log_enabled {
            log_acc += dt;
            if log_acc >= 5.0 {
                log_acc = 0.0;
                if let Ok(mut lf) = std::fs::OpenOptions::new().create(true).append(true).open("debug_log.txt") {
                    use std::io::Write;
                    let _ = writeln!(lf, "fps={} frame_ms={:.1} res_scale={:.2} in_game={}", get_fps(), get_frame_time() * 1000.0, front.settings.res_scale, game.is_some());
                }
            }
        }

        // Dynamic Resolution Adjustment (`Task 1.3`): automatically adjust `res_scale` to maintain smooth FPS
        if front.settings.dyn_res && game.is_some() {
            let fps = get_fps();
            let target = if front.settings.fps_cap > 0 { front.settings.fps_cap as i32 } else { 60 };
            if fps < target - 8 && front.settings.res_scale > 0.55 {
                front.settings.res_scale = (front.settings.res_scale - 0.05 * dt).max(0.55);
            } else if fps >= target - 2 && front.settings.res_scale < 1.0 {
                front.settings.res_scale = (front.settings.res_scale + 0.02 * dt).min(1.0);
            }
        }

        if let Some(g) = game.as_mut() {
            let back = game_frame(&res, g, &mut front.settings, t, dt, None);
            if back {
                let was_online = g.net.is_some();
                if let Some(mut g_old) = game.take() {
                    if let Some(n) = g_old.net.take() { n.disconnect(); }
                    CACHED_STAGE.with(|cs| *cs.borrow_mut() = Some((g_old.level, g_old.world, g_old.scene)));
                }
                if was_online { front.go(ui::Screen::Multiplayer); }
            }
        } else {
            let bg_mode = match front.screen {
                ui::Screen::Freeroam => 2,
                ui::Screen::Garage => 3,
                _ => 1,
            };
            // Keep stdout out of the per-frame rendering path. Console I/O here
            // was executed only while menus were visible and could stall UI frames.
            draw_bg(&res, &mut menu, t, bg_mode, front.settings.gfx_preset);
            match front.draw(&res, &mut menu, t, dt) {
                ui::FrontAction::Quit => break,
                ui::FrontAction::Connected(net) => {
                    tip_counter += 1;
                    let idx = match menu.cars[menu.sel_car].kind { CarKind::Builtin(i) => i, _ => 0 };
                    let map_id = match menu.maps[menu.sel_map].kind { MapKind::BuiltinMap(m) => m, _ => 0 };
                    let mut g = new_game_async(&res, idx, map_id, &front, tip_counter).await;
                    g.net = Some(net);
                    g.net_spawn_sent = false;
                    g.net_seq = 0;
                    game = Some(g);
                }
                ui::FrontAction::Start => {
                    tip_counter += 1;
                    let idx = match menu.cars[menu.sel_car].kind { CarKind::Builtin(i) => i, _ => 0 };
                    let map_id = match menu.maps[menu.sel_map].kind { MapKind::BuiltinMap(m) => m, _ => 0 };
                    let g = new_game_async(&res, idx, map_id, &front, tip_counter).await;
                    game = Some(g);
                }
                ui::FrontAction::StartPhotoMode => {
                    tip_counter += 1;
                    let idx = match menu.cars[menu.sel_car].kind { CarKind::Builtin(i) => i, _ => 0 };
                    let mut g = new_game_async(&res, idx, 3, &front, tip_counter).await;
                    g.photo.active = true;
                    let (cpos, cyaw, _) = g.photo.apply_preset(0, &mut g.ed.day_hour);
                    g.cars[0].respawn(cpos, cyaw);
                    game = Some(g);
                }
                ui::FrontAction::None => {}
            }
        }

        music.sync(game.is_none(), &front.settings);

        // No second CPU-side FPS sleep/spin: VSync in the Ogre swapchain keeps
        // presentation aligned with the display refresh. fps_cap is only the
        // dynamic-resolution target above, not a second frame limiter.
        next_frame().await;
    }
}

fn shadow_texture() -> Texture2D {
    let n = 64u16;
    let mut img = Image::gen_image_color(n, n, Color::new(1.0, 1.0, 1.0, 0.0));
    for y in 0..n { for x in 0..n {
        let dx = (x as f32 / (n - 1) as f32 - 0.5) * 2.0;
        let dy = (y as f32 / (n - 1) as f32 - 0.5) * 2.0;
        let a = (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
        img.set_pixel(x as u32, y as u32, Color::new(1.0, 1.0, 1.0, a * a * 0.85));
    }}
    Texture2D::from_image(&img)
}



