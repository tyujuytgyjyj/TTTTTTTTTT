// World: terrain height, surfaces, obstacles and static scene meshes.
use macroquad::prelude::*;
use std::sync::{Arc, RwLock};

// ---------- التضاريس الحقيقية (خريطة West Coast) ----------
// الملف hf.bin يُنتَج بالأداة tools/bake_west_coast.py. إحداثيات اللعبة: x = bx ، z = -by (BeamNG).
pub struct HeightField {
    pub n: usize,
    pub cell: f32,
    pub ox: f32,
    pub oy: f32,
    pub hscale: f32,
    pub hbase: f32,
    pub h: Vec<u16>,
    pub surf: Vec<u8>, // 0 أسفلت، 2 تراب/حصى، 3 عشب
    pub spawns: Vec<([f32; 3], f32)>,
    pub colliders: Vec<BoxObs>,
}

impl HeightField {
    pub fn parse(b: &[u8]) -> Option<HeightField> {
        if b.len() < 24 { return None; }
        let v2 = &b[0..4] == b"HF02";
        if !v2 && &b[0..4] != b"HF01" { return None; }
        let n = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
        let f = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let (cell, ox, oy, hscale) = (f(8), f(12), f(16), f(20));
        let (hbase, off) = if v2 { (f(24), 32usize) } else { (0.0f32, 24usize) };
        let need = off + n * n * 3;
        if n < 2 || b.len() < need { return None; }
        let mut h = Vec::with_capacity(n * n);
        for i in 0..n * n { h.push(u16::from_le_bytes([b[off + i * 2], b[off + 1 + i * 2]])); }
        let surf = b[off + n * n * 2..need].to_vec();
        Some(HeightField { n, cell, ox, oy, hscale, hbase, h, surf, spawns: Vec::new(), colliders: Vec::new() })
    }
    #[inline]
    pub fn hat(&self, ix: usize, iy: usize) -> f32 { self.h[iy * self.n + ix] as f32 * self.hscale + self.hbase }
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let m = (self.n - 1) as f32 - 0.001;
        let fx = ((x - self.ox) / self.cell).clamp(0.0, m);
        let fy = ((-z - self.oy) / self.cell).clamp(0.0, m);
        let (ix, iy) = (fx as usize, fy as usize);
        let (tx, ty) = (fx - ix as f32, fy - iy as f32);
        let (h00, h10, h01, h11) = (self.hat(ix, iy), self.hat(ix + 1, iy), self.hat(ix, iy + 1), self.hat(ix + 1, iy + 1));
        (h00 * (1.0 - tx) + h10 * tx) * (1.0 - ty) + (h01 * (1.0 - tx) + h11 * tx) * ty
    }
    pub fn surface(&self, x: f32, z: f32) -> u8 {
        let m = (self.n - 1) as f32;
        let ix = (((x - self.ox) / self.cell).round()).clamp(0.0, m) as usize;
        let iy = (((-z - self.oy) / self.cell).round()).clamp(0.0, m) as usize;
        self.surf[iy * self.n + ix]
    }
    pub fn half_extent(&self) -> f32 { (self.n - 1) as f32 * self.cell * 0.5 }
}

pub struct WaterRegion {
    heightfield: Arc<HeightField>,
    step: usize,
    cells_x: usize,
    cells_y: usize,
    connected: Vec<bool>,
    level: f32,
}

impl WaterRegion {
    pub fn depth_at(&self, x: f32, z: f32, y: f32) -> f32 {
        if self.cells_x == 0 || self.cells_y == 0 || self.step == 0 || self.level <= -1.0e8 {
            return 0.0;
        }
        let fx = (x - self.heightfield.ox) / self.heightfield.cell;
        let fy = (-z - self.heightfield.oy) / self.heightfield.cell;
        if fx < 0.0 || fy < 0.0 ||
           fx > (self.cells_x * self.step) as f32 ||
           fy > (self.cells_y * self.step) as f32 {
            return 0.0;
        }
        let cx = ((fx / self.step as f32).floor() as usize).min(self.cells_x - 1);
        let cy = ((fy / self.step as f32).floor() as usize).min(self.cells_y - 1);
        let index = cy * self.cells_x + cx;
        if !self.connected.get(index).copied().unwrap_or(false) {
            return 0.0;
        }
        if self.heightfield.height(x, z) >= self.level {
            return 0.0;
        }
        (self.level - y).max(0.0)
    }
}

static WATER_REGION: RwLock<Option<Arc<WaterRegion>>> = RwLock::new(None);

pub fn set_water_region(
    heightfield: Arc<HeightField>,
    step: usize,
    level: f32,
    connected: Vec<bool>,
) {
    if step == 0 { return; }
    let cells_x = (heightfield.n.saturating_sub(1)) / step;
    let cells_y = cells_x;
    if cells_x == 0 || cells_y == 0 || connected.len() != cells_x * cells_y {
        return;
    }
    let region = WaterRegion { heightfield, step, cells_x, cells_y, connected, level };
    if let Ok(mut g) = WATER_REGION.write() {
        *g = Some(Arc::new(region));
    }
}

pub fn water_region() -> Option<Arc<WaterRegion>> {
    WATER_REGION.read().ok().and_then(|g| g.clone())
}

static TERRAIN: RwLock<Option<Arc<HeightField>>> = RwLock::new(None);

static ENV: RwLock<[f32; 8]> = RwLock::new([0.0; 8]);
/// قيم البيئة (الشمس والضباب والزمن) تُحدَّث كل إطار لتستعملها شيدرات المياه وغيرها.
pub fn set_env(sun: Vec3, fog: Vec3, t: f32, sy: f32) {
    if let Ok(mut g) = ENV.write() { *g = [sun.x, sun.y, sun.z, fog.x, fog.y, fog.z, t, sy]; }
}
pub fn get_env() -> [f32; 8] { ENV.read().map(|g| *g).unwrap_or([0.0; 8]) }

static WATER_LEVEL: RwLock<f32> = RwLock::new(-1.0e9);
pub fn set_water_level(v: f32) {
    if let Ok(mut g) = WATER_LEVEL.write() { *g = v; }
    if v < -1.0e8 {
        if let Ok(mut g) = WATER_REGION.write() { *g = None; }
    }
}
/// مستوى سطح الماء (المحيط) أو -1e9 إن لم يوجد.
pub fn water_level() -> f32 { WATER_LEVEL.read().map(|g| *g).unwrap_or(-1.0e9) }

pub fn set_terrain(t: Option<Arc<HeightField>>) { if let Ok(mut g) = TERRAIN.write() { *g = t; } }
pub fn terrain_hf() -> Option<Arc<HeightField>> { TERRAIN.read().ok().and_then(|g| g.clone()) }
pub fn terrain_active() -> bool { TERRAIN.read().map(|g| g.is_some()).unwrap_or(false) }
pub fn terrain_surface(x: f32, z: f32) -> Option<u8> {
    let g = TERRAIN.read().ok()?;
    g.as_ref().map(|t| t.surface(x, z))
}
pub fn terrain_spawn(i: usize) -> Option<([f32; 3], f32)> {
    let g = TERRAIN.read().ok()?;
    let t = g.as_ref()?;
    if t.spawns.is_empty() { return None; }
    let (p, yaw) = t.spawns[i % t.spawns.len()];
    // ارفع السيارة فوق سطح الأرض الفعلي حتى لا تنشأ داخل التضاريس فتتحطم عند الإنشاء
    // مثل النقاط الافتراضية: السيارة تُوضع عند سطح الأرض نفسه (بلا سقوط) وعلى أعلى نقطة حولها
    let mut gy = t.height(p[0], p[2]);
    for (dx, dz) in [(2.0f32, 0.0f32), (-2.0, 0.0), (0.0, 2.0), (0.0, -2.0)] {
        gy = gy.max(t.height(p[0] + dx, p[2] + dz));
    }
    Some(([p[0], gy + 0.05, p[2]], yaw))
}
static TERRAIN_CACHE: std::sync::Mutex<Vec<(String, Option<Arc<HeightField>>)>> = std::sync::Mutex::new(Vec::new());
static MAP_DIR: RwLock<String> = RwLock::new(String::new());

pub fn set_map_dir(name: &str) { if let Ok(mut g) = MAP_DIR.write() { *g = name.to_string(); } }
pub fn map_dir() -> String { MAP_DIR.read().map(|g| g.clone()).unwrap_or_default() }
/// مسار ملف/مجلد من أصول الخريطة الحالية (assets/maps/<map>/<sub>) إن وُجد.
pub fn map_path(sub: &str) -> Option<String> {
    let m = map_dir();
    if m.is_empty() { return None; }
    for base in ["assets/maps", "client/assets/maps"] {
        let p = format!("{}/{}/{}", base, m, sub);
        if std::path::Path::new(&p).exists() { return Some(p); }
    }
    None
}
pub fn load_west_coast() -> Option<Arc<HeightField>> { load_terrain_map("west_coast") }
pub fn load_terrain_map(name: &str) -> Option<Arc<HeightField>> {
    if let Ok(g) = TERRAIN_CACHE.lock() {
        for (k, v) in g.iter() { if k == name { return v.clone(); } }
    }
    let mut result: Option<Arc<HeightField>> = None;
    for base in ["assets/maps", "client/assets/maps"] {
        let p = format!("{}/{}/hf.bin", base, name);
        if let Ok(b) = std::fs::read(&p) {
            if let Some(mut t) = HeightField::parse(&b) {
                if let Ok(txt) = std::fs::read_to_string(p.replace("hf.bin", "spawns.txt")) {
                    for line in txt.lines() {
                        let v: Vec<f32> = line.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                        if v.len() == 4 { t.spawns.push(([v[0], v[1] + 0.6, v[2]], v[3])); }
                    }
                }
                if let Ok(cb) = std::fs::read(p.replace("hf.bin", "props/colliders.bin")) {
                    if cb.len() >= 8 && &cb[0..4] == b"COL1" {
                        let n = u32::from_le_bytes([cb[4], cb[5], cb[6], cb[7]]) as usize;
                        let f = |o: usize| f32::from_le_bytes([cb[o], cb[o + 1], cb[o + 2], cb[o + 3]]);
                        for i in 0..n {
                            let o = 8 + i * 28;
                            if o + 28 > cb.len() { break; }
                            t.colliders.push(BoxObs { c: vec3(f(o), f(o + 4), f(o + 8)), h: vec3(f(o + 12), f(o + 16), f(o + 20)), yaw: f(o + 24) });
                        }
                    }
                }
                result = Some(Arc::new(t));
                break;
            }
        }
    }
    if let Ok(mut g) = TERRAIN_CACHE.lock() { g.push((name.to_string(), result.clone())); }
    result
}

#[derive(Clone)]
pub struct BoxObs {
    pub c: Vec3,
    pub h: Vec3, // half extents
    pub yaw: f32,
}

impl BoxObs {
    pub fn local(&self, p: Vec3) -> Vec3 {
        let d = p - self.c;
        let (s, c) = self.yaw.sin_cos();
        vec3(d.x * c - d.z * s, d.y, d.x * s + d.z * c)
    }
    pub fn to_world_dir(&self, n: Vec3) -> Vec3 {
        let (s, c) = self.yaw.sin_cos();
        vec3(n.x * c + n.z * s, n.y, -n.x * s + n.z * c)
    }
    pub fn aabb(&self) -> (Vec3, Vec3) {
        let (s, c) = self.yaw.sin_cos();
        let ex = self.h.x * c.abs() + self.h.z * s.abs();
        let ez = self.h.x * s.abs() + self.h.z * c.abs();
        let e = vec3(ex, self.h.y, ez);
        (self.c - e, self.c + e)
    }
}

/// Ramp rising along +Z (local), placed with centre/yaw.
#[derive(Clone)]
pub struct Ramp {
    pub c: Vec2,
    pub half_w: f32,
    pub up_len: f32,
    pub top_len: f32,
    pub height: f32,
    pub yaw: f32,
}

impl Ramp {
    fn down_len(&self) -> f32 { self.height / 0.9 }
    pub fn total_len(&self) -> f32 { self.up_len + self.top_len + self.down_len() }
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        let (s, c) = self.yaw.sin_cos();
        let (dx, dz) = (x - self.c.x, z - self.c.y);
        let lx = dx * c - dz * s;
        let lz = dx * s + dz * c;
        if lx.abs() > self.half_w { return 0.0; }
        let u = lz + self.total_len() * 0.5;
        if u < 0.0 { return 0.0; }
        if u < self.up_len { return self.height * u / self.up_len; }
        if u < self.up_len + self.top_len { return self.height; }
        let d = u - self.up_len - self.top_len;
        (self.height - d * 0.9).max(0.0)
    }
    pub fn to_world(&self, lx: f32, lz: f32) -> Vec2 {
        let (s, c) = self.yaw.sin_cos();
        vec2(self.c.x + lx * c + lz * s, self.c.y - lx * s + lz * c)
    }
}

pub struct World {
    pub vis_boxes: usize,
    pub boxes: Vec<BoxObs>,
    pub ramps: Vec<Ramp>,
    pub arena: f32, // half size of the flat asphalt area
    pub grid: BoxGrid,
}

/// شبكة مكانية لصناديق التصادم (broad-phase) حتى لا نفحص عشرات الآلاف منها كل خطوة فيزياء.
#[derive(Default)]
pub struct BoxGrid {
    cell: f32,
    map: std::collections::HashMap<(i32, i32), Vec<u32>>,
    big: Vec<u32>,
}

impl BoxGrid {
    pub fn build(boxes: &[BoxObs]) -> BoxGrid {
        let cell = 32.0f32;
        let mut g = BoxGrid { cell, map: std::collections::HashMap::new(), big: Vec::new() };
        for (i, b) in boxes.iter().enumerate() {
            let (mn, mx) = b.aabb();
            let (x0, x1) = ((mn.x / cell).floor() as i32, (mx.x / cell).floor() as i32);
            let (z0, z1) = ((mn.z / cell).floor() as i32, (mx.z / cell).floor() as i32);
            if (x1 - x0 + 1) * (z1 - z0 + 1) > 36 { g.big.push(i as u32); continue; }
            for gz in z0..=z1 {
                for gx in x0..=x1 { g.map.entry((gx, gz)).or_default().push(i as u32); }
            }
        }
        g
    }
    /// يعيد فهارس الصناديق المحتملة ضمن المربع (بدون تكرار).
    pub fn query(&self, amin: Vec3, amax: Vec3, out: &mut Vec<usize>) {
        out.clear();
        for &i in &self.big { out.push(i as usize); }
        if self.cell <= 0.0 { return; }
        let (x0, x1) = ((amin.x / self.cell).floor() as i32, (amax.x / self.cell).floor() as i32);
        let (z0, z1) = ((amin.z / self.cell).floor() as i32, (amax.z / self.cell).floor() as i32);
        for gz in z0..=z1 {
            for gx in x0..=x1 {
                if let Some(v) = self.map.get(&(gx, gz)) {
                    for &i in v { out.push(i as usize); }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

impl World {
    pub fn terrain(&self, x: f32, z: f32) -> f32 {
        if let Ok(g) = TERRAIN.read() {
            if let Some(t) = g.as_ref() { return t.height(x, z); }
        }
        0.0
    }
    pub fn ground_h(&self, x: f32, z: f32) -> f32 {
        let mut h = self.terrain(x, z);
        for r in &self.ramps { h = h.max(r.height_at(x, z)); }
        h
    }
    pub fn ground_normal(&self, x: f32, z: f32) -> Vec3 {
        let e = 0.3;
        let dx = self.ground_h(x + e, z) - self.ground_h(x - e, z);
        let dz = self.ground_h(x, z + e) - self.ground_h(x, z - e);
        vec3(-dx / (2.0 * e), 1.0, -dz / (2.0 * e)).normalize()
    }
    pub fn mu(&self, x: f32, z: f32) -> f32 {
        if let Some(s) = terrain_surface(x, z) { return match s { 0 => 1.0, 2 => 0.58, _ => 0.5 }; }
        if x.abs() < self.arena && z.abs() < self.arena {
            if x > 60.0 && x < 95.0 && z > -95.0 && z < -60.0 { 0.55 } else { 1.0 } // gravel patch
        } else { 0.62 }
    }
}

// ---------- static mesh building ----------
pub struct MeshBuilder {
    pub v: Vec<Vertex>,
    pub i: Vec<u16>,
}

fn col(c: Color) -> Color { c }

impl MeshBuilder {
    pub fn new() -> Self { MeshBuilder { v: Vec::new(), i: Vec::new() } }
    pub fn vert(&mut self, p: Vec3, n: Vec3, c: Color, spec: f32) -> u16 {
        self.v.push(Vertex {
            position: p,
            uv: vec2(0.02, 0.02),
            color: [(c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, 255],
            normal: vec4(n.x, n.y, n.z, spec),
        });
        (self.v.len() - 1) as u16
    }
    pub fn quad(&mut self, p: [Vec3; 4], n: Vec3, c: Color, spec: f32) {
        let a = self.vert(p[0], n, col(c), spec);
        let b = self.vert(p[1], n, col(c), spec);
        let cc = self.vert(p[2], n, col(c), spec);
        let d = self.vert(p[3], n, col(c), spec);
        self.i.extend_from_slice(&[a, b, cc, a, cc, d]);
    }
    pub fn add_box_quat(&mut self, c: Vec3, h: Vec3, rot_q: Quat, color: Color, spec: f32) {
        let corner = |sx: f32, sy: f32, sz: f32| c + rot_q * vec3(sx * h.x, sy * h.y, sz * h.z);
        let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
            ([1., 0., 0.], [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]]),
            ([-1., 0., 0.], [[-1., -1., 1.], [-1., 1., 1.], [-1., 1., -1.], [-1., -1., -1.]]),
            ([0., 1., 0.], [[-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.], [1., 1., -1.]]),
            ([0., -1., 0.], [[-1., -1., 1.], [-1., -1., -1.], [1., -1., -1.], [1., -1., 1.]]),
            ([0., 0., 1.], [[1., -1., 1.], [1., 1., 1.], [-1., 1., 1.], [-1., -1., 1.]]),
            ([0., 0., -1.], [[-1., -1., -1.], [-1., 1., -1.], [1., 1., -1.], [1., -1., -1.]]),
        ];
        for (n, q) in faces.iter() {
            let nn = rot_q * vec3(n[0], n[1], n[2]);
            let pts = [
                corner(q[0][0], q[0][1], q[0][2]), corner(q[1][0], q[1][1], q[1][2]),
                corner(q[2][0], q[2][1], q[2][2]), corner(q[3][0], q[3][1], q[3][2]),
            ];
            self.quad(pts, nn, color, spec);
        }
    }
    pub fn add_box_uv_quat(&mut self, c: Vec3, h: Vec3, rot_q: Quat, color: Color, tile: f32) {
        let corner = |sx: f32, sy: f32, sz: f32| c + rot_q * vec3(sx * h.x, sy * h.y, sz * h.z);
        let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
            ([1., 0., 0.], [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]]), ([-1., 0., 0.], [[-1., -1., 1.], [-1., 1., 1.], [-1., 1., -1.], [-1., -1., -1.]]),
            ([0., 1., 0.], [[-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.], [1., 1., -1.]]), ([0., -1., 0.], [[-1., -1., 1.], [-1., -1., -1.], [1., -1., -1.], [1., -1., 1.]]),
            ([0., 0., 1.], [[1., -1., 1.], [1., 1., 1.], [-1., 1., 1.], [-1., -1., 1.]]), ([0., 0., -1.], [[-1., -1., -1.], [-1., 1., -1.], [1., 1., -1.], [1., -1., -1.]]),
        ];
        for (n, q) in faces.iter() {
            let nn = rot_q * vec3(n[0], n[1], n[2]);
            let p: Vec<Vec3> = q.iter().map(|k| corner(k[0], k[1], k[2])).collect();
            let (l1, l2) = ((p[1] - p[0]).length() / tile, (p[3] - p[0]).length() / tile);
            let uv = [vec2(0.0, 0.0), vec2(l1, 0.0), vec2(l1, l2), vec2(0.0, l2)];
            let ids: Vec<u16> = (0..4).map(|i| self.vert_uv(p[i], uv[i], nn, color, 0.05)).collect();
            self.i.extend_from_slice(&[ids[0], ids[1], ids[2], ids[0], ids[2], ids[3]]);
        }
    }
    pub fn add_box(&mut self, c: Vec3, h: Vec3, yaw: f32, color: Color, spec: f32) {
        let (s, co) = yaw.sin_cos();
        let rot = |p: Vec3| vec3(p.x * co + p.z * s, p.y, -p.x * s + p.z * co);
        let corner = |sx: f32, sy: f32, sz: f32| c + rot(vec3(sx * h.x, sy * h.y, sz * h.z));
        let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
            ([1., 0., 0.], [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]]),
            ([-1., 0., 0.], [[-1., -1., 1.], [-1., 1., 1.], [-1., 1., -1.], [-1., -1., -1.]]),
            ([0., 1., 0.], [[-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.], [1., 1., -1.]]),
            ([0., -1., 0.], [[-1., -1., 1.], [-1., -1., -1.], [1., -1., -1.], [1., -1., 1.]]),
            ([0., 0., 1.], [[1., -1., 1.], [1., 1., 1.], [-1., 1., 1.], [-1., -1., 1.]]),
            ([0., 0., -1.], [[-1., -1., -1.], [-1., 1., -1.], [1., 1., -1.], [1., -1., -1.]]),
        ];
        for (n, q) in faces.iter() {
            let nn = rot(vec3(n[0], n[1], n[2]));
            let pts = [
                corner(q[0][0], q[0][1], q[0][2]), corner(q[1][0], q[1][1], q[1][2]),
                corner(q[2][0], q[2][1], q[2][2]), corner(q[3][0], q[3][1], q[3][2]),
            ];
            // shade stripe: darker toward the bottom via two-tone colour
            self.quad(pts, nn, color, spec);
        }
    }
    pub fn vert_uv(&mut self, p: Vec3, uv: Vec2, n: Vec3, c: Color, spec: f32) -> u16 {
        self.v.push(Vertex { position: p, uv, color: [(c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, (c.a * 255.0) as u8], normal: vec4(n.x, n.y, n.z, spec) });
        (self.v.len() - 1) as u16
    }
    /// flat horizontal quad with tiling UVs: corners in order (x0,z0)-(x1,z1)
    pub fn ground_quad(&mut self, x0: f32, z0: f32, x1: f32, z1: f32, y: f32, uv: [Vec2; 4], c: Color) {
        let a = self.vert_uv(vec3(x0, y, z0), uv[0], Vec3::Y, c, 0.0);
        let b = self.vert_uv(vec3(x1, y, z0), uv[1], Vec3::Y, c, 0.0);
        let cc = self.vert_uv(vec3(x1, y, z1), uv[2], Vec3::Y, c, 0.0);
        let d = self.vert_uv(vec3(x0, y, z1), uv[3], Vec3::Y, c, 0.0);
        self.i.extend_from_slice(&[a, cc, b, a, d, cc]);
    }
    /// box whose faces carry tiling UVs (tile metres per texture repeat)
    pub fn add_box_uv(&mut self, c: Vec3, h: Vec3, yaw: f32, color: Color, tile: f32) {
        let (s, co) = yaw.sin_cos();
        let rot = |p: Vec3| vec3(p.x * co + p.z * s, p.y, -p.x * s + p.z * co);
        let corner = |sx: f32, sy: f32, sz: f32| c + rot(vec3(sx * h.x, sy * h.y, sz * h.z));
        let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
            ([1., 0., 0.], [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]]), ([-1., 0., 0.], [[-1., -1., 1.], [-1., 1., 1.], [-1., 1., -1.], [-1., -1., -1.]]),
            ([0., 1., 0.], [[-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.], [1., 1., -1.]]), ([0., -1., 0.], [[-1., -1., 1.], [-1., -1., -1.], [1., -1., -1.], [1., -1., 1.]]),
            ([0., 0., 1.], [[1., -1., 1.], [1., 1., 1.], [-1., 1., 1.], [-1., -1., 1.]]), ([0., 0., -1.], [[-1., -1., -1.], [-1., 1., -1.], [1., 1., -1.], [1., -1., -1.]]),
        ];
        for (n, q) in faces.iter() {
            let nn = rot(vec3(n[0], n[1], n[2]));
            let p: Vec<Vec3> = q.iter().map(|k| corner(k[0], k[1], k[2])).collect();
            let (l1, l2) = ((p[1] - p[0]).length() / tile, (p[3] - p[0]).length() / tile);
            let uv = [vec2(0.0, 0.0), vec2(l1, 0.0), vec2(l1, l2), vec2(0.0, l2)];
            let ids: Vec<u16> = (0..4).map(|i| self.vert_uv(p[i], uv[i], nn, color, 0.05)).collect();
            self.i.extend_from_slice(&[ids[0], ids[1], ids[2], ids[0], ids[2], ids[3]]);
        }
    }
    pub fn flush(&mut self, out: &mut Vec<Mesh>, tex: Option<Texture2D>) {
        if self.v.is_empty() { return; }
        out.push(Mesh { vertices: std::mem::take(&mut self.v), indices: std::mem::take(&mut self.i), texture: tex });
    }
    pub fn room(&self) -> bool { self.v.len() < 50000 }
}

fn rgb(r: f32, g: f32, b: f32) -> Color { Color::new(r, g, b, 1.0) }

pub fn build_scene(w: &World, _tex: Texture2D) -> Vec<Mesh> {
    let mut meshes = Vec::new();
    let mut mb = MeshBuilder::new();
    if false {

    // terrain grid: 4 m cells over 480 m
    let half = 240.0f32;
    let cell = 4.0f32;
    let n = (half * 2.0 / cell) as usize;
    let mut grid: Vec<u16> = Vec::new();
    let mut base = 0usize;
    let mut chunk_rows = 0;
    for gz in 0..=n {
        for gx in 0..=n {
            let x = -half + gx as f32 * cell;
            let z = -half + gz as f32 * cell;
            let y = w.terrain(x, z);
            let nn = {
                let e = 1.5;
                vec3(-(w.terrain(x + e, z) - w.terrain(x - e, z)) / (2.0 * e), 1.0, -(w.terrain(x, z + e) - w.terrain(x, z - e)) / (2.0 * e)).normalize()
            };
            let noise = (((gx * 7 + gz * 13) % 5) as f32) * 0.012 + (((gx / 3 + gz / 3) % 2) as f32) * 0.02;
            let c = rgb(0.22 + noise, 0.40 + noise * 1.5, 0.17 + noise);
            let _ = &mut grid;
            mb.vert(vec3(x, y - 0.03, z), nn, c, 0.0);
        }
        chunk_rows += 1;
        let _ = (&mut base, chunk_rows);
    }
    let stride = n + 1;
    for gz in 0..n {
        for gx in 0..n {
            let a = (gz * stride + gx) as u32;
            let b = a + 1;
            let c = a + stride as u32;
            let d = c + 1;
            for v in [a, c, b, b, c, d] { mb.i.push(v as u16); }
        }
    }
    // (u16 index limit: 121x121 = 14641 vertices OK)
    mb.flush(&mut meshes, None);

    // asphalt arena
    let a = w.arena;
    mb.quad([vec3(-a, 0.0, a), vec3(a, 0.0, a), vec3(a, 0.0, -a), vec3(-a, 0.0, -a)].map(|p| p + vec3(0.0, 0.0, 0.0)), Vec3::Y, rgb(0.20, 0.20, 0.22), 0.03);
    // gravel patch
    mb.quad([vec3(60.0, 0.015, -60.0), vec3(95.0, 0.015, -60.0), vec3(95.0, 0.015, -95.0), vec3(60.0, 0.015, -95.0)], Vec3::Y, rgb(0.45, 0.40, 0.33), 0.0);
    // lane markings
    let white = rgb(0.85, 0.85, 0.80);
    let yel = rgb(0.9, 0.75, 0.15);
    let mut z = -a + 5.0;
    while z < a - 5.0 {
        mb.quad([vec3(-0.15, 0.02, z), vec3(0.15, 0.02, z), vec3(0.15, 0.02, z + 4.0), vec3(-0.15, 0.02, z + 4.0)].map(|p| p), Vec3::Y, yel, 0.04);
        z += 9.0;
    }
    for sx in [-8.0f32, 8.0] {
        mb.quad([vec3(sx - 0.12, 0.02, -a + 2.0), vec3(sx + 0.12, 0.02, -a + 2.0), vec3(sx + 0.12, 0.02, a - 2.0), vec3(sx - 0.12, 0.02, a - 2.0)], Vec3::Y, white, 0.04);
    }
    // parking bays
    for k in 0..8 {
        let x = 30.0 + k as f32 * 3.2;
        mb.quad([vec3(x - 0.08, 0.02, 20.0), vec3(x + 0.08, 0.02, 20.0), vec3(x + 0.08, 0.02, 28.0), vec3(x - 0.08, 0.02, 28.0)], Vec3::Y, white, 0.04);
    }
    mb.flush(&mut meshes, None);

    }
    // ramps
    for r in &w.ramps {
        let hl = r.total_len() * 0.5;
        let p = |lx: f32, lz: f32, y: f32| { let q = r.to_world(lx, lz); vec3(q.x, y, q.y) };
        let (hw, ul, tl, hh) = (r.half_w, r.up_len, r.top_len, r.height);
        let z0 = -hl; let z1 = -hl + ul; let z2 = z1 + tl; let z3 = hl;
        let slope_n = |rise: f32, run: f32| { let l = r.to_world(0.0, 0.0); let _ = l; let n = vec3(0.0, run, -rise).normalize(); let (s, c) = r.yaw.sin_cos(); vec3(n.x * c + n.z * s, n.y, -n.x * s + n.z * c) };
        let ramp_c = rgb(0.55, 0.50, 0.30);
        mb.quad([p(-hw, z0, 0.0), p(hw, z0, 0.0), p(hw, z1, hh), p(-hw, z1, hh)].map(|q| q), slope_n(hh, ul), ramp_c, 0.1);
        mb.quad([p(-hw, z1, hh), p(hw, z1, hh), p(hw, z2, hh), p(-hw, z2, hh)], Vec3::Y, ramp_c, 0.1);
        mb.quad([p(-hw, z2, hh), p(hw, z2, hh), p(hw, z3, 0.0), p(-hw, z3, 0.0)], slope_n(-hh, hh / 0.9), rgb(0.5, 0.46, 0.28), 0.1);
        let side_c = rgb(0.4, 0.37, 0.25);
        for s in [-1.0f32, 1.0] {
            let nrm = { let (sn, c) = r.yaw.sin_cos(); vec3(c * s, 0.0, -sn * s) };
            mb.quad([p(s * hw, z0, 0.0), p(s * hw, z1, hh), p(s * hw, z2, hh), p(s * hw, z3, 0.0)], nrm, side_c, 0.0);
        }
    }
    mb.flush(&mut meshes, None);

    // boxes (barriers, walls, pillars) -- colour by size
    for b in w.boxes.iter().take(w.vis_boxes) {
        let c = if b.h.y > 3.0 { rgb(0.55, 0.40, 0.33) }
            else if b.h.y < 0.5 { rgb(0.72, 0.72, 0.70) }
            else if b.h.x < 0.6 && b.h.z < 0.6 { rgb(0.85, 0.35, 0.12) }
            else { rgb(0.62, 0.62, 0.60) };
        mb.add_box(b.c, b.h, b.yaw, c, 0.05);
        if !mb.room() { mb.flush(&mut meshes, None); }
    }
    mb.flush(&mut meshes, None);
    meshes
}
