// Scene rendering for the editable level: static ground/roads, pre-batched custom instancing,
// 3-tier LOD (LOD0/LOD1/LOD2 + Shadow Proxy), on-demand asset streaming, frustum & facade occlusion culling.
use crate::envload::{
    box_proxy_epart, bundled_custom_bytes, decimate_eparts, decode_tex, discover_custom_glbs,
    load_glb, load_glb_building, load_glb_custom, load_glb_grid, BUNDLED_CUSTOM_NAMES, EPart,
};
use crate::level::*;
use crate::world::*;
use macroquad::prelude::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
pub struct Frustum {
    pub planes: [Vec4; 6],
}

impl Frustum {
    pub fn from_vp(vp: Mat4) -> Self {
        let r0 = vp.row(0);
        let r1 = vp.row(1);
        let r2 = vp.row(2);
        let r3 = vp.row(3);
        let raw = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r3 + r2, r3 - r2];
        let mut planes = [Vec4::ZERO; 6];
        for i in 0..6 {
            let len = raw[i].truncate().length().max(1e-6);
            planes[i] = raw[i] / len;
        }
        Frustum { planes }
    }

    #[inline]
    pub fn aabb_visible(&self, mn: Vec3, mx: Vec3) -> bool {
        for p in &self.planes {
            let px = if p.x >= 0.0 { mx.x } else { mn.x };
            let py = if p.y >= 0.0 { mx.y } else { mn.y };
            let pz = if p.z >= 0.0 { mx.z } else { mn.z };
            if p.x * px + p.y * py + p.z * pz + p.w < -1.5 {
                return false;
            }
        }
        true
    }

    #[inline]
    pub fn obb_quat_visible(&self, pos: Vec3, q: Quat, scale: f32, bmin: Vec3, bmax: Vec3) -> bool {
        let lc = (bmin + bmax) * (0.5 * scale);
        let lh = (bmax - bmin) * (0.5 * scale);
        let wc = pos + q * lc;
        let ax = (q * Vec3::X).abs();
        let ay = (q * Vec3::Y).abs();
        let az = (q * Vec3::Z).abs();
        let wh = ax * lh.x + ay * lh.y + az * lh.z;
        self.aabb_visible(wc - wh, wc + wh)
    }
}

pub struct Terrain { cell: f32, h: HashMap<(i32, i32), (f32, u32)> }
impl Terrain {
    pub fn from_parts(parts: &[EPart]) -> Terrain {
        let cell = 6.0; let mut h: HashMap<(i32, i32), (f32, u32)> = HashMap::new();
        for p in parts { for v in &p.mesh.vertices { let k = ((v.position.x / cell).floor() as i32, (v.position.z / cell).floor() as i32); let e = h.entry(k).or_insert((0.0, 0)); e.0 += v.position.y; e.1 += 1; } }
        Terrain { cell, h }
    }
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let (i, j) = ((x / self.cell).floor() as i32, (z / self.cell).floor() as i32);
        for r in 0..4 { for di in -r..=r { for dj in -r..=r { if let Some((s, n)) = self.h.get(&(i + di, j + dj)) { return s / *n as f32; } } } }
        0.0
    }
}

pub struct CustomAsset {
    pub name: String,
    pub parts: Vec<EPart>,
    pub lod1: Vec<EPart>,
    pub shadow_proxy: Vec<EPart>,
    pub half: Vec3,
}

impl CustomAsset {
    pub fn new(name: String, parts: Vec<EPart>, half: Vec3) -> Self {
        let total_tris: usize = parts.iter().map(|p| p.mesh.indices.len() / 3).sum();
        let lod1 = if total_tris > 2500 {
            decimate_eparts(&parts, (half.length() * 0.045).clamp(0.18, 0.90))
        } else {
            parts.clone()
        };
        let shadow_proxy = if total_tris > 1200 && half.y > 0.45 {
            box_proxy_epart(half, 0.0, [140, 140, 145, 255])
        } else {
            lod1.clone()
        };
        CustomAsset { name, parts, lod1, shadow_proxy, half }
    }
}

pub struct Assets {
    pub apt_hi: Vec<EPart>,
    pub apt_lod1: Vec<EPart>,
    pub apt_lo: Vec<EPart>,
    pub school: Vec<EPart>,
    pub school_lod1: Vec<EPart>,
    pub school_lo: Vec<EPart>,
    pub trees: Vec<Vec<EPart>>,
    pub rocks: Vec<Vec<EPart>>,
    pub custom: Vec<CustomAsset>,
    pub t_hazard: Texture2D,
    pub t_mh: Texture2D,
    pub terrain: Terrain,
}

pub struct Scene {
    pub ground: Vec<EPart>,
    pub far: Vec<EPart>,
    pub assets: Assets,
    pub dynm: Vec<EPart>,
    pub forest: Vec<EPart>,
    /// Pre-transformed static batches for custom objects (collapses 83 fences + props into a few draw calls!)
    pub batched_custom: Vec<EPart>,
    pub batched_custom_glass: Vec<EPart>,
    /// عند true تُرسم التضاريس الحقيقية بدل الأرضية الافتراضية
    pub terrain_on: bool,
    pub terrain_name: String,
    pub ground_backup: Vec<EPart>,
    pub far_backup: Vec<EPart>,
    /// أشجار وأسوار وأعمدة الخريطة الحقيقية (streaming)
    pub props: Option<crate::props::Props>,
    pub water: Option<crate::water::Water>,
    pub grass: Option<crate::grass::Grass>,
}

fn rng(seed: &mut u32) -> f32 { *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); ((*seed >> 8) & 0xFFFF) as f32 / 65535.0 }

fn meshes_to_parts(meshes: Vec<Mesh>) -> Vec<EPart> {
    meshes.into_iter().map(|m| EPart::from_mesh(m, false)).collect()
}

/// Streams in a single custom GLB model on demand if not already in `assets.custom`.
pub fn ensure_custom_loaded(assets: &mut Assets, name: &str) {
    if assets.custom.iter().any(|c| c.name == name) {
        return;
    }
    if let Some(bytes) = bundled_custom_bytes(name) {
        let (parts, half) = load_glb_custom(bytes);
        if !parts.is_empty() {
            assets.custom.push(CustomAsset::new(name.to_string(), parts, half));
            assets.custom.sort_by(|a, b| a.name.cmp(&b.name));
            return;
        }
    }
    // Check disk folders
    let loaded: HashSet<String> = assets.custom.iter().map(|c| c.name.clone()).collect();
    for (n, parts, half) in discover_custom_glbs(&loaded) {
        if !assets.custom.iter().any(|c| c.name == n) {
            assets.custom.push(CustomAsset::new(n, parts, half));
        }
    }
    assets.custom.sort_by(|a, b| a.name.cmp(&b.name));
}

/// Loads all bundled + disk custom GLB assets (called when opening the World Editor `+ Add` catalog).
pub fn rescan_custom_assets(assets: &mut Assets) {
    for &bname in &BUNDLED_CUSTOM_NAMES {
        ensure_custom_loaded(assets, bname);
    }
    let mut loaded: HashSet<String> = assets.custom.iter().map(|c| c.name.clone()).collect();
    for (name, parts, half) in discover_custom_glbs(&loaded) {
        loaded.insert(name.clone());
        assets.custom.push(CustomAsset::new(name, parts, half));
    }
    assets.custom.sort_by(|a, b| a.name.cmp(&b.name));
}

pub fn stream_custom_bounds(assets: &mut Assets, level: &mut Level) {
    let used_names: Vec<String> = level.objs.iter().filter_map(|o| {
        if let Kind::Custom(ref name) = o.kind { Some(name.clone()) } else { None }
    }).collect();
    for name in used_names {
        ensure_custom_loaded(assets, &name);
    }
    sync_custom_bounds(assets, level);
}

pub fn sync_custom_bounds(assets: &Assets, level: &mut Level) {
    for o in level.objs.iter_mut() {
        if let Kind::Custom(ref name) = o.kind {
            if let Some(ca) = assets.custom.iter().find(|c| &c.name == name) {
                if (o.size - ca.half).length() > 0.05 {
                    o.size = ca.half;
                    level.dirty = true;
                }
            }
        }
    }
}

pub fn build_ground(arena: f32) -> Scene {
    let t_road = decode_tex(include_bytes!("../assets/env/road.png")).unwrap();
    let t_roadb = decode_tex(include_bytes!("../assets/env/road_b.png")).unwrap();
    let t_asph = decode_tex(include_bytes!("../assets/env/asphalt.png")).unwrap();
    let t_conc = decode_tex(include_bytes!("../assets/env/concrete.png")).unwrap();
    let t_fa = decode_tex(include_bytes!("../assets/env/floor_a.jpg")).unwrap();
    let t_fb = decode_tex(include_bytes!("../assets/env/floor_b.jpg")).unwrap();
    let white = Color::new(1.0, 1.0, 1.0, 1.0);
    let stone = Color::new(0.62, 0.62, 0.65, 1.0);
    let mut ground_meshes: Vec<Mesh> = Vec::new();
    let a = arena;
    let mut mb = MeshBuilder::new();
    let tile = 3.0;
    mb.ground_quad(-a, -a, a, a, 0.0, [vec2(-a / tile, -a / tile), vec2(a / tile, -a / tile), vec2(a / tile, a / tile), vec2(-a / tile, a / tile)], white);
    mb.flush(&mut ground_meshes, Some(t_asph));
    mb.ground_quad(60.0, -95.0, 95.0, -60.0, 0.012, [Vec2::ZERO; 4], Color::new(0.42, 0.37, 0.30, 1.0));
    mb.flush(&mut ground_meshes, None);
    mb.ground_quad(30.0, -64.0, 58.0, -34.0, 0.01, [vec2(0.0, 0.0), vec2(7.0, 0.0), vec2(7.0, 7.5), vec2(0.0, 7.5)], white);
    mb.flush(&mut ground_meshes, Some(t_conc));
    let rw = 13.1f32; let hw = rw / 2.0;
    mb.ground_quad(-hw, -a, hw, a, 0.014, [vec2(0.0, -a / rw), vec2(1.0, -a / rw), vec2(1.0, a / rw), vec2(0.0, a / rw)], white);
    mb.flush(&mut ground_meshes, Some(t_road));
    let zc = 30.0; let cw = 7.0; let tl = 29.0;
    for (xa, xb) in [(-a, -hw), (hw, a)] {
        mb.ground_quad(xa, zc - cw, xb, zc + cw, 0.016, [vec2(xa / tl, 0.0), vec2(xb / tl, 0.0), vec2(xb / tl, 1.0), vec2(xa / tl, 1.0)], white);
    }
    mb.flush(&mut ground_meshes, Some(t_roadb));
    let cob = 0.75f32;
    let mut seed = 12345u32;
    let mut ma = MeshBuilder::new(); let mut mbb = MeshBuilder::new();
    let seg = |x0: f32, z0: f32, x1: f32, z1: f32, idx: usize, seed: &mut u32, ma: &mut MeshBuilder, mbb: &mut MeshBuilder| {
        let off = vec2(rng(seed) * 4.0, rng(seed) * 4.0); let swap = rng(seed) > 0.5;
        let corner = |x: f32, z: f32| { let u = (x - x0) / cob + off.x; let v = (z - z0) / cob + off.y; if swap { vec2(v, u) } else { vec2(u, v) } };
        let uv = [corner(x0, z0), corner(x1, z0), corner(x1, z1), corner(x0, z1)];
        if idx % 2 == 0 { ma.ground_quad(x0, z0, x1, z1, 0.02, uv, stone); } else { mbb.ground_quad(x0, z0, x1, z1, 0.02, uv, stone); }
    };
    let mut n = 0usize;
    let mut z = -a;
    while z < a - 0.1 { let z1 = (z + 8.0).min(a); if !(z1 > zc - cw - 3.2 && z < zc + cw + 3.2) { seg(hw + 0.1, z, hw + 3.2, z1, n, &mut seed, &mut ma, &mut mbb); n += 1; seg(-hw - 3.2, z, -hw - 0.1, z1, n, &mut seed, &mut ma, &mut mbb); n += 1; } z = z1; }
    for side in [-1.0f32, 1.0] {
        let (za, zb) = if side < 0.0 { (zc - cw - 3.2, zc - cw - 0.1) } else { (zc + cw + 0.1, zc + cw + 3.2) };
        let mut x = hw + 3.3;
        while x < a - 0.1 { let x1 = (x + 8.0).min(a); seg(x, za, x1, zb, n, &mut seed, &mut ma, &mut mbb); n += 1; seg(-x1, za, -x, zb, n, &mut seed, &mut ma, &mut mbb); n += 1; x = x1; }
    }
    let mut x = -a + 4.0; while x < a - 4.0 { let x1 = (x + 10.0).min(a - 4.0); seg(x, 66.0, x1, 77.0, n, &mut seed, &mut ma, &mut mbb); n += 1; x = x1; }
    let mut x = -a + 4.0; while x < a - 4.0 { let x1 = (x + 10.0).min(a - 4.0); if !(x1 > -hw - 14.0 && x < hw + 14.0) { seg(x, -77.0, x1, -66.0, n, &mut seed, &mut ma, &mut mbb); n += 1; } x = x1; }
    let mut zz = -30.0; while zz < 30.0 { let z1 = zz + 10.0; seg(-68.0, zz, -58.0, z1, n, &mut seed, &mut ma, &mut mbb); n += 1; zz = z1; }
    ma.flush(&mut ground_meshes, Some(t_fa)); mbb.flush(&mut ground_meshes, Some(t_fb));
    let ground = meshes_to_parts(ground_meshes);

    let far = load_glb_grid(include_bytes!("../assets/env/forest_ground.glb"), 8);
    let terrain = Terrain::from_parts(&far);
    let tree_all = load_glb(include_bytes!("../assets/env/tree_types.glb"));
    let trees: Vec<Vec<EPart>> = tree_all.into_iter().map(|p| vec![p]).collect();
    let rocks: Vec<Vec<EPart>> = load_glb(include_bytes!("../assets/env/mountains.glb")).into_iter().map(|mut p| {
        let (mut mn, mut mx) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        for v in &p.mesh.vertices { mn = mn.min(v.position); mx = mx.max(v.position); }
        let c = vec3((mn.x + mx.x) / 2.0, mn.y, (mn.z + mx.z) / 2.0);
        for v in p.mesh.vertices.iter_mut() { v.position -= c; }
        p.bmin -= c;
        p.bmax -= c;
        vec![p]
    }).collect();

    let apt_hi = load_glb_building(include_bytes!("../assets/env/apartment_hi.glb"), 15.2, 3.8, -6.5, 16.0);
    let apt_lod1 = decimate_eparts(&apt_hi, 0.72);
    let apt_lo = box_proxy_epart(APT_HALF, 0.0, [158, 156, 150, 255]);

    let school = load_glb_building(include_bytes!("../assets/env/school.glb"), 11.8, 7.2, -7.2, 9.8);
    let school_lod1 = decimate_eparts(&school, 0.68);
    let school_lo = box_proxy_epart(SCHOOL_HALF, 0.0, [135, 160, 185, 255]);

    // Custom models are streamed on demand via sync_custom_bounds() when referenced in level.objs!
    let custom: Vec<CustomAsset> = Vec::new();

    let assets = Assets {
        apt_hi,
        apt_lod1,
        apt_lo,
        school,
        school_lod1,
        school_lo,
        trees,
        rocks,
        custom,
        t_hazard: decode_tex(include_bytes!("../assets/env/hazard.png")).unwrap(),
        t_mh: decode_tex(include_bytes!("../assets/env/manhole.png")).unwrap(),
        terrain,
    };
    let lite = std::env::args().any(|a| a == "--lite");
    let mut sc = Scene {
        ground,
        far,
        assets,
        dynm: vec![],
        forest: vec![],
        batched_custom: vec![],
        batched_custom_glass: vec![],
        terrain_on: false,
        terrain_name: String::new(),
        ground_backup: vec![],
        far_backup: vec![],
        props: None,
        water: None,
        grass: None,
    };
    if lite { sc.assets.rocks.clear(); }
    sc
}

/// Rebuilds dynamic level geometry AND pre-batches repeated custom objects (e.g. 83 fences) into static world chunks.
/// يبني مجسم التضاريس الحقيقية (شرائح 64x64 خلية مع culling) وينسّقه بنسيج الألوان المخبوز.
fn build_terrain_parts() -> Option<Vec<EPart>> {
    let hf = crate::world::terrain_hf()?;
    let tex = crate::world::map_path("terrain_albedo.jpg")
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| decode_tex(&b))?;
    let n = hf.n;
    let chunk = 64usize;
    let ext = (n - 1) as f32;
    let white = Color::new(1.0, 1.0, 1.0, 1.0);
    let families = [10.0f32, 11.0, 12.0, 13.0]; // sand, dirt, grass, asphalt
    let water_level = crate::world::water_level();
    let mut meshes: Vec<Mesh> = Vec::new();
    let mut cy = 0usize;
    while cy < n - 1 {
        let ey = (cy + chunk).min(n - 1);
        let mut cx = 0usize;
        while cx < n - 1 {
            let ex = (cx + chunk).min(n - 1);
            let w = ex - cx + 1;
            let mut layers: Vec<MeshBuilder> = (0..4).map(|_| MeshBuilder::new()).collect();
            for iy in cy..=ey {
                for ix in cx..=ex {
                    let x = hf.ox + ix as f32 * hf.cell;
                    let z = -(hf.oy + iy as f32 * hf.cell);
                    let y = hf.hat(ix, iy);
                    let hl = hf.hat(ix.saturating_sub(1), iy);
                    let hr = hf.hat((ix + 1).min(n - 1), iy);
                    let hd = hf.hat(ix, iy.saturating_sub(1));
                    let hu = hf.hat(ix, (iy + 1).min(n - 1));
                    let nrm = vec3(-(hr - hl) / (2.0 * hf.cell), 1.0,
                                   (hu - hd) / (2.0 * hf.cell)).normalize();
                    let uv = vec2(ix as f32 / ext, iy as f32 / ext);
                    for layer in 0..4 {
                        layers[layer].vert_uv(vec3(x, y, z), uv, nrm, white, families[layer]);
                    }
                }
            }
            for iy in 0..(ey - cy) {
                for ix in 0..(ex - cx) {
                    let d = (iy * w + ix) as u16;
                    let c = d + 1;
                    let a = d + w as u16;
                    let b = a + 1;
                    let world_x = hf.ox + (cx as f32 + ix as f32 + 0.5) * hf.cell;
                    let world_z = -(hf.oy + (cy as f32 + iy as f32 + 0.5) * hf.cell);
                    let y = (hf.hat(cx + ix, cy + iy) + hf.hat(cx + ix + 1, cy + iy)
                        + hf.hat(cx + ix, cy + iy + 1) + hf.hat(cx + ix + 1, cy + iy + 1)) * 0.25;
                    let surface = hf.surface(world_x, world_z);
                    let material = match surface {
                        4 => 0, // explicit baked beach sand
                        2 if water_level > -1.0e8 && y >= water_level - 0.35 && y <= water_level + 5.0 => 0,
                        2 => 1, // dirt / gravel
                        3 => 2, // grass
                        0 => 3, // asphalt
                        _ => 1,
                    };
                    layers[material].i.extend_from_slice(&[a, c, b, a, d, c]);
                }
            }
            for layer in 0..4 {
                if !layers[layer].i.is_empty() {
                    layers[layer].flush(&mut meshes, Some(tex.clone()));
                }
            }
            cx = ex;
        }
        cy = ey;
    }
    Some(meshes_to_parts(meshes))
}
/// طرق الخريطة الحقيقية (أسفلت، خطوط، حواف، مجاري) كشرائح فوق التضاريس. الملفات من tools/bake_roads.py
fn build_road_parts() -> Vec<EPart> {
    let mut out: Vec<EPart> = Vec::new();
    let Some(dir) = crate::world::map_path("roads") else { return out };
    let dir = dir.as_str();
    let Ok(b) = std::fs::read(format!("{}/roads.bin", dir)) else { return out };
    if b.len() < 8 || &b[0..4] != b"RDS1" { return out; }
    let rd = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let rf = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let n = rd(4) as usize;
    let mut texs: HashMap<u32, Option<Texture2D>> = HashMap::new();
    let mut p = 8usize;
    for _ in 0..n {
        if p + 12 > b.len() { break; }
        let (t, nv, ni) = (rd(p), rd(p + 4) as usize, rd(p + 8) as usize);
        p += 12;
        let vend = p + nv * 20;
        let iend = vend + ni * 2;
        if iend > b.len() { break; }
        let mut verts = Vec::with_capacity(nv);
        for i in 0..nv {
            let o = p + i * 20;
            verts.push(Vertex {
                position: vec3(rf(o), rf(o + 4), rf(o + 8)),
                uv: vec2(rf(o + 12), rf(o + 16)),
                color: [255, 255, 255, 255],
                normal: vec4(0.0, 1.0, 0.0, 14.0),
            });
        }
        let mut idx = Vec::with_capacity(ni);
        for i in 0..ni { idx.push(u16::from_le_bytes([b[vend + i * 2], b[vend + i * 2 + 1]])); }
        p = iend;
        let tex = texs
            .entry(t)
            .or_insert_with(|| std::fs::read(format!("{}/tex/{}.png", dir, t)).ok().and_then(|bytes| decode_tex(&bytes)))
            .clone();
        out.push(EPart::from_mesh(Mesh { vertices: verts, indices: idx, texture: tex }, false));
    }
    out
}

/// يبدّل بين الأرضية الافتراضية والتضاريس الحقيقية حسب الخريطة المحمّلة.
pub fn sync_terrain(sc: &mut Scene) {
    let want = crate::world::terrain_active();
    let cur = crate::world::map_dir();
    if want && (!sc.terrain_on || sc.terrain_name != cur) {
        // Load water metadata first; terrain materials use its sea level to
        // distinguish the existing beach band from inland dirt/gravel.
        let loaded_water = crate::water::Water::load();
        if loaded_water.is_none() {
            crate::world::set_water_level(-1.0e9);
        }
        if let Some(mut parts) = build_terrain_parts() {
            parts.extend(build_road_parts());
            if !sc.terrain_on {
                sc.ground_backup = std::mem::take(&mut sc.ground);
                sc.far_backup = std::mem::take(&mut sc.far);
            }
            sc.ground = parts;
            sc.props = crate::props::Props::load();
            sc.water = loaded_water;
            sc.grass = Some(crate::grass::Grass::new());
            sc.terrain_on = true;
            sc.terrain_name = cur;
        } else {
            crate::world::set_water_level(-1.0e9);
        }
    } else if !want && sc.terrain_on {
        sc.ground = std::mem::take(&mut sc.ground_backup);
        sc.far = std::mem::take(&mut sc.far_backup);
        sc.props = None;
        sc.water = None;
        sc.grass = None;
        crate::world::set_water_level(-1.0e9);
        sc.terrain_on = false;
        sc.terrain_name = String::new();
    }
}

pub fn rebuild(sc: &mut Scene, level: &Level, world: &World, tex: Texture2D) {
    sync_terrain(sc);
    let mut dynm_meshes = build_scene(world, tex);
    let mut mb = MeshBuilder::new();
    for o in level.objs.iter().filter(|o| o.visible && o.kind == Kind::Box) {
        mb.add_box_quat(o.pos, o.size * o.scale, o.rot_quat(), Color::new(o.color[0], o.color[1], o.color[2], 1.0), 0.05);
        if !mb.room() { mb.flush(&mut dynm_meshes, None); }
    }
    mb.flush(&mut dynm_meshes, None);
    for o in level.objs.iter().filter(|o| o.visible && o.kind == Kind::Hazard) {
        mb.add_box_uv_quat(o.pos, o.size * o.scale, o.rot_quat(), Color::new(o.color[0], o.color[1], o.color[2], 1.0), 1.0);
    }
    mb.flush(&mut dynm_meshes, Some(sc.assets.t_hazard.clone()));
    for o in level.objs.iter().filter(|o| o.visible && o.kind == Kind::Manhole) {
        let r = 0.45 * o.scale;
        mb.ground_quad(o.pos.x - r, o.pos.z - r, o.pos.x + r, o.pos.z + r, o.pos.y.max(0.03), [vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0)], Color::new(1.0, 1.0, 1.0, 1.0));
    }
    mb.flush(&mut dynm_meshes, Some(sc.assets.t_mh.clone()));
    sc.dynm = meshes_to_parts(dynm_meshes);

    // Pre-batch all static custom objects by (blend, raw_texture_id, spatial_sector) so 83 fences become 2 draw calls!
    sc.batched_custom.clear();
    sc.batched_custom_glass.clear();
    let mut batches: HashMap<(bool, String, usize, i32, i32), (Option<Texture2D>, Vec<Vertex>, Vec<u16>)> = HashMap::new();
    for o in level.objs.iter().filter(|o| o.visible) {
        let Kind::Custom(ref name) = o.kind else { continue };
        let Some(ca) = sc.assets.custom.iter().find(|c| &c.name == name) else { continue };
        let m = model_matrix(o);
        let q = o.rot_quat();
        let sx = (o.pos.x / 90.0).floor() as i32;
        let sz = (o.pos.z / 90.0).floor() as i32;
        for (pi, p) in ca.parts.iter().enumerate() {
            let key = (p.blend, name.clone(), pi, sx, sz);
            let entry = batches.entry(key).or_insert_with(|| (p.mesh.texture.clone(), Vec::new(), Vec::new()));
            if entry.1.len() + p.mesh.vertices.len() > 38000 || entry.2.len() + p.mesh.indices.len() > 120000 {
                let finished = EPart::from_mesh(Mesh {
                    vertices: std::mem::take(&mut entry.1),
                    indices: std::mem::take(&mut entry.2),
                    texture: entry.0.clone(),
                }, p.blend);
                if p.blend { sc.batched_custom_glass.push(finished); } else { sc.batched_custom.push(finished); }
            }
            let base = entry.1.len() as u16;
            for v in &p.mesh.vertices {
                let wp = m.transform_point3(v.position);
                let wn = q * vec3(v.normal.x, v.normal.y, v.normal.z);
                entry.1.push(Vertex {
                    position: wp,
                    uv: v.uv,
                    color: v.color,
                    normal: vec4(wn.x, wn.y, wn.z, v.normal.w),
                });
            }
            for &idx in &p.mesh.indices {
                entry.2.push(base + idx);
            }
        }
    }
    let mut keys: Vec<(bool, String, usize, i32, i32)> = batches.keys().cloned().collect();
    keys.sort();
    for k in keys {
        let (blend, _, _, _, _) = k;
        let (tex_opt, vs, is) = batches.remove(&k).unwrap();
        if vs.is_empty() { continue; }
        let ep = EPart::from_mesh(Mesh { vertices: vs, indices: is, texture: tex_opt }, blend);
        if blend { sc.batched_custom_glass.push(ep); } else { sc.batched_custom.push(ep); }
    }
}

pub fn rebuild_forest(sc: &mut Scene, level: &Level) {
    sc.forest.clear();
    let f = &level.forest;
    if !f.on || f.count == 0 { return; }
    let kinds: Vec<usize> = (0..5).filter(|k| f.mask[*k] && *k < sc.assets.trees.len()).collect();
    if kinds.is_empty() { return; }
    let mut seed = f.seed.wrapping_mul(2654435761).wrapping_add(17);
    const SECTORS: usize = 16;
    let mut bufs: Vec<Vec<(Vec<Vertex>, Vec<u16>)>> = kinds
        .iter()
        .map(|_| (0..SECTORS).map(|_| (Vec::new(), Vec::new())).collect())
        .collect();
    for _ in 0..f.count {
        let ki = (rng(&mut seed) * kinds.len() as f32) as usize % kinds.len();
        let r = (f.inner * f.inner + rng(&mut seed) * (f.outer * f.outer - f.inner * f.inner)).sqrt();
        let ang = rng(&mut seed) * std::f32::consts::TAU;
        let (x, z) = (r * ang.cos(), r * ang.sin());
        let yaw = rng(&mut seed) * std::f32::consts::TAU;
        let sc_ = f.scale * (0.7 + rng(&mut seed) * 0.7);
        let y = sc.assets.terrain.height(x, z);
        let q = Quat::from_rotation_y(yaw);
        let part = &sc.assets.trees[kinds[ki]][0].mesh;
        let sec = ((ang / std::f32::consts::TAU) * SECTORS as f32) as usize % SECTORS;
        let (vs, is) = &mut bufs[ki][sec];
        if vs.len() + part.vertices.len() > 45000 { continue; }
        let base = vs.len() as u16;
        for v in &part.vertices { let p = vec3(x, y, z) + q * (v.position * sc_); let n = q * vec3(v.normal.x, v.normal.y, v.normal.z); vs.push(Vertex { position: p, uv: v.uv, color: v.color, normal: vec4(n.x, n.y, v.normal.z, 0.0) }); }
        for i in &part.indices { is.push(base + *i); }
    }
    for (ki, sec_bufs) in bufs.into_iter().enumerate() {
        let tex = sc.assets.trees[kinds[ki]][0].mesh.texture.clone();
        for (vs, is) in sec_bufs {
            if vs.is_empty() { continue; }
            sc.forest.push(EPart::from_mesh(Mesh { vertices: vs, indices: is, texture: tex.clone() }, false));
        }
    }
}

pub fn model_matrix(o: &Obj) -> Mat4 {
    Mat4::from_translation(o.pos) * Mat4::from_quat(o.rot_quat()) * Mat4::from_scale(Vec3::splat(o.scale))
}

pub fn draw(sc: &Scene, level: &Level, lit: &Material, vp: Mat4, cp: Vec3, fwd: Vec3) {
    draw_ex(sc, level, lit, vp, cp, fwd, false, 1);
}

/// Extended scene draw supporting static batching (when `!editor_active`) and 3-tier LOD (`gfx_quality`: 0=Low, 1=Medium, 2=High).
pub fn draw_ex(sc: &Scene, level: &Level, lit: &Material, vp: Mat4, cp: Vec3, _fwd: Vec3, editor_active: bool, gfx_quality: u8) {
    let frustum = Frustum::from_vp(vp);
    lit.set_uniform("MODEL", Mat4::IDENTITY);
    for p in &sc.ground {
        if frustum.aabb_visible(p.bmin, p.bmax) { draw_mesh(&p.mesh); }
    }
    for p in &sc.far {
        if frustum.aabb_visible(p.bmin, p.bmax) { draw_mesh(&p.mesh); }
    }
    for p in &sc.dynm {
        if frustum.aabb_visible(p.bmin, p.bmax) { draw_mesh(&p.mesh); }
    }
    if let Some(pr) = &sc.props {
        pr.draw(cp, gfx_quality, &|mn, mx| frustum.aabb_visible(mn, mx));
    }
    if let Some(gr) = &sc.grass {
        let wl = sc.water.as_ref().map(|w| w.level).unwrap_or(-1.0e9);
        gr.draw(cp, gfx_quality, wl, &|mn, mx| frustum.aabb_visible(mn, mx));
    }
    if gfx_quality > 0 || level.forest.on {
        for p in &sc.forest {
            if frustum.aabb_visible(p.bmin, p.bmax) { draw_mesh(&p.mesh); }
        }
    }
    if let Some(w) = &sc.water {
        w.draw(vp, cp);
        gl_use_material(lit);
    }

    // Phase 8: When World Editor is active, draw manual Collision Volumes (`Kind::Collider`) as translucent green/red wireframes & boxes
    if editor_active {
        let mut col_mb = MeshBuilder::new();
        for o in level.objs.iter().filter(|o| o.kind == Kind::Collider && o.visible) {
            let c = if o.collide {
                Color::new(0.15, 0.95, 0.45, 0.38)
            } else {
                Color::new(0.95, 0.25, 0.20, 0.28)
            };
            col_mb.add_box_quat(o.pos, o.size * o.scale, o.rot_quat(), c, 0.15);
        }
        let mut col_meshes = Vec::new();
        col_mb.flush(&mut col_meshes, None);
        for m in &col_meshes {
            draw_mesh(m);
        }
    }

    // Fast path during gameplay: draw pre-batched custom objects with MODEL = IDENTITY (collapses 83 fences into 2 draw calls!)
    if !editor_active {
        for p in &sc.batched_custom {
            if frustum.aabb_visible(p.bmin, p.bmax) {
                draw_mesh(&p.mesh);
            }
        }
    }

    let mut glass: Vec<(Mat4, &Mesh)> = Vec::new();
    let lod0_dist = match gfx_quality { 0 => 58.0f32, 1 => 115.0, _ => 210.0 };
    let lod1_dist = match gfx_quality { 0 => 155.0f32, 1 => 260.0, _ => 420.0 };
    let max_bldg_dist = match gfx_quality { 0 => 380.0f32, 1 => 560.0, _ => 780.0 };

    for o in level.objs.iter().filter(|o| o.visible) {
        if !editor_active && matches!(o.kind, Kind::Custom(_)) {
            continue;
        }
        let (box_c, box_h) = o.pick_box();
        let d = o.pos - cp;
        let dist_sq = d.length_squared();
        let obj_radius = box_h.length();
        let max_dist = match o.kind {
            Kind::Apartment | Kind::School => max_bldg_dist,
            _ => (650.0f32 + obj_radius * 2.0).min(1800.0),
        };
        if dist_sq > max_dist * max_dist { continue; }

        let parts: &Vec<EPart> = match &o.kind {
            Kind::Apartment => {
                if dist_sq > lod1_dist * lod1_dist { &sc.assets.apt_lo }
                else if dist_sq > lod0_dist * lod0_dist { &sc.assets.apt_lod1 }
                else { &sc.assets.apt_hi }
            }
            Kind::School => {
                if dist_sq > lod1_dist * lod1_dist { &sc.assets.school_lo }
                else if dist_sq > lod0_dist * lod0_dist { &sc.assets.school_lod1 }
                else { &sc.assets.school }
            }
            Kind::Tree(k) => match sc.assets.trees.get(*k as usize) { Some(p) => p, None => continue },
            Kind::Rock(k) => match sc.assets.rocks.get(*k as usize) { Some(p) => p, None => continue },
            Kind::Custom(name) => match sc.assets.custom.iter().find(|c| &c.name == name) {
                Some(ca) => {
                    if dist_sq > lod0_dist * lod0_dist { &ca.lod1 } else { &ca.parts }
                }
                None => continue,
            },
            _ => continue,
        };

        let q = o.rot_quat();
        let ax = (q * Vec3::X).abs();
        let ay = (q * Vec3::Y).abs();
        let az = (q * Vec3::Z).abs();
        let wh = ax * box_h.x + ay * box_h.y + az * box_h.z + Vec3::splat(3.0);
        if !frustum.aabb_visible(box_c - wh, box_c + wh) { continue; }

        let inv_s = 1.0 / o.scale.max(0.01);
        let local_cam = (q.inverse() * (cp - o.pos)) * inv_s;
        let (lx, ly, lz) = (local_cam.x, local_cam.y, local_cam.z);
        let use_facade_cull = matches!(o.kind, Kind::Apartment | Kind::School) && o.pitch.abs() < 0.05 && o.roll.abs() < 0.05 && ly < 22.0;

        let m = model_matrix(o);
        let mut model_set = false;
        for p in parts {
            if use_facade_cull {
                let occluded = match p.facade {
                    1 => lz < p.cull_z,
                    2 => lz > p.cull_z,
                    3 => lx < p.cull_x,
                    4 => lx > p.cull_x,
                    5 => lx < p.cull_x && lz < p.cull_z,
                    6 => lx > p.cull_x && lz < p.cull_z,
                    7 => lx < p.cull_x && lz > p.cull_z,
                    8 => lx > p.cull_x && lz > p.cull_z,
                    _ => false,
                };
                if occluded { continue; }
            }
            if !frustum.obb_quat_visible(o.pos, q, o.scale, p.bmin, p.bmax) {
                continue;
            }
            if p.blend {
                glass.push((m, &p.mesh));
            } else {
                if !model_set {
                    lit.set_uniform("MODEL", m);
                    model_set = true;
                }
                draw_mesh(&p.mesh);
            }
        }
    }
    for (m, mesh) in glass {
        lit.set_uniform("MODEL", m);
        draw_mesh(mesh);
    }
    if !editor_active && !sc.batched_custom_glass.is_empty() {
        lit.set_uniform("MODEL", Mat4::IDENTITY);
        for p in &sc.batched_custom_glass {
            if frustum.aabb_visible(p.bmin, p.bmax) {
                draw_mesh(&p.mesh);
            }
        }
    }
    lit.set_uniform("MODEL", Mat4::IDENTITY);
}

/// Renders lightweight LOD2 shadow proxies into the planar shadow map (cuts shadow vertex load by >96%).
pub fn draw_shadows(sc: &Scene, level: &Level, shcast: &Material, center: Vec3, radius: f32) {
    let r_sq = (radius + 32.0) * (radius + 32.0);
    for o in level.objs.iter().filter(|o| o.visible) {
        let (box_c, box_h) = o.pick_box();
        if box_h.y < 0.85 || box_h.x > 65.0 || box_h.z > 65.0 { continue; }
        let dx = box_c.x - center.x;
        let dz = box_c.z - center.z;
        if dx * dx + dz * dz > r_sq { continue; }
        let parts: &Vec<EPart> = match &o.kind {
            Kind::Apartment => &sc.assets.apt_lo,
            Kind::School => &sc.assets.school_lo,
            Kind::Tree(k) => match sc.assets.trees.get(*k as usize) { Some(p) => p, None => continue },
            Kind::Custom(name) => match sc.assets.custom.iter().find(|c| &c.name == name) {
                Some(ca) => &ca.shadow_proxy,
                None => continue,
            },
            _ => continue,
        };
        let m = model_matrix(o);
        shcast.set_uniform("MODEL", m);
        for p in parts {
            if !p.blend {
                draw_mesh(&p.mesh);
            }
        }
    }
    shcast.set_uniform("MODEL", Mat4::IDENTITY);
    for p in &sc.dynm {
        if p.bmax.y > 0.35 {
            draw_mesh(&p.mesh);
        }
    }
}
