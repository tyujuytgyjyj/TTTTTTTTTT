// نظام العشب: خصل عشب (شرائح متقاطعة) تُولَّد حول الكاميرا فقط فوق أسطح العشب في التضاريس (streaming).
// النسيج يُرسم برمجيًا فلا يحتاج ملفات. بلا حركة رياح (المحرك لا يدعمها حاليًا).
use crate::envload::EPart;
use macroquad::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

const CELL: f32 = 32.0;

pub struct Grass {
    tex: Texture2D,
    cells: RefCell<HashMap<(i32, i32, u8), Vec<EPart>>>,
}

fn hash2(a: i32, b: i32, s: u32) -> u32 {
    let mut h = (a as u32).wrapping_mul(0x9E37_79B1) ^ (b as u32).wrapping_mul(0x85EB_CA77) ^ s.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}
fn rnd(h: &mut u32) -> f32 {
    *h = h.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    ((*h >> 8) & 0x00FF_FFFF) as f32 / 16_777_216.0
}

fn make_texture() -> Texture2D {
    let mut img = Image::gen_image_color(128, 128, Color::new(0.0, 0.0, 0.0, 0.0));
    let mut s = 12345u32;
    for k in 0..10 {
        let base_x = 8.0 + k as f32 * 11.5 + rnd(&mut s) * 6.0;
        let height = 70.0 + rnd(&mut s) * 55.0;
        let lean = (rnd(&mut s) - 0.5) * 36.0;
        let shade = 0.8 + rnd(&mut s) * 0.4;
        let steps = height as i32;
        for st in 0..steps {
            let t = st as f32 / height;
            let y = 127 - st;
            let x = base_x + lean * t * t;
            let half_w = 3.4 * (1.0 - t).powf(0.7) + 0.5;
            let x0 = (x - half_w).floor() as i32;
            let x1 = (x + half_w).ceil() as i32;
            let r = (0.10 + 0.32 * t) * shade;
            let g = (0.26 + 0.38 * t) * shade;
            let b = (0.05 + 0.10 * t) * shade;
            for xx in x0..=x1 {
                if xx >= 0 && xx < 128 && y >= 0 && y < 128 {
                    img.set_pixel(xx as u32, y as u32, Color::new(r, g, b, 1.0));
                }
            }
        }
    }
    let tex = Texture2D::from_image(&img);
    tex.set_filter(FilterMode::Linear);
    tex
}

impl Grass {
    pub fn new() -> Grass {
        Grass { tex: make_texture(), cells: RefCell::new(HashMap::new()) }
    }

    /// طبقتان: قريبة كثيفة، وبعيدة أخف بخصل أكبر حتى مسافة 150م (في الجودة العالية).
    pub fn draw(&self, cp: Vec3, quality: u8, min_h: f32, vis: &dyn Fn(Vec3, Vec3) -> bool) {
        let (r_far, r_near, d_near, d_far) = match quality {
            0 => (60.0f32, 18.0f32, 0.9f32, 0.12f32),
            1 => (100.0, 32.0, 1.4, 0.22),
            _ => (150.0, 48.0, 2.0, 0.32),
        };
        let Some(hf) = crate::world::terrain_hf() else { return };
        let dist_to = |k: &(i32, i32, u8)| {
            let c = vec2((k.0 as f32 + 0.5) * CELL, (k.1 as f32 + 0.5) * CELL);
            vec2(cp.x, cp.z).distance(c)
        };
        {
            let mut cells = self.cells.borrow_mut();
            cells.retain(|k, _| dist_to(k) < (if k.2 == 0 { r_near } else { r_far }) + CELL * 1.5);
            let r = (r_far / CELL).ceil() as i32 + 1;
            let (ccx, ccz) = ((cp.x / CELL).floor() as i32, (cp.z / CELL).floor() as i32);
            let mut need: Vec<((i32, i32, u8), f32)> = Vec::new();
            for dz in -r..=r {
                for dx in -r..=r {
                    for tier in 0..2u8 {
                        let k = (ccx + dx, ccz + dz, tier);
                        if cells.contains_key(&k) { continue; }
                        let d = dist_to(&k);
                        let ok = if tier == 0 { d < r_near + CELL * 0.7 } else { d > r_near - CELL * 1.5 && d < r_far + CELL * 0.7 };
                        if ok { need.push((k, d)); }
                    }
                }
            }
            need.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            for (k, _) in need.into_iter().take(3) {
                let parts = if k.2 == 0 { self.build_cell(&hf, (k.0, k.1), d_near, 1.0, min_h) } else { self.build_cell(&hf, (k.0, k.1), d_far, 1.7, min_h) };
                cells.insert(k, parts);
            }
        }
        let cells = self.cells.borrow();
        for (k, parts) in cells.iter() {
            let d = dist_to(k);
            let show = if k.2 == 0 { d < r_near + CELL * 0.7 } else { d > r_near - CELL * 1.5 && d < r_far + CELL * 0.7 };
            if !show { continue; }
            for p in parts {
                if vis(p.bmin, p.bmax) { draw_mesh(&p.mesh); }
            }
        }
    }

    fn build_cell(&self, hf: &crate::world::HeightField, k: (i32, i32), dens: f32, size: f32, min_h: f32) -> Vec<EPart> {
        let mut out: Vec<EPart> = Vec::new();
        let mut rng = hash2(k.0, k.1, 77 + (size * 10.0) as u32);
        let n = (CELL * CELL * dens) as usize;
        let (cx0, cz0) = (k.0 as f32 * CELL, k.1 as f32 * CELL);
        let mut verts: Vec<Vertex> = Vec::new();
        let mut idx: Vec<u16> = Vec::new();
        for _ in 0..n {
            let x = cx0 + rnd(&mut rng) * CELL;
            let z = cz0 + rnd(&mut rng) * CELL;
            let w = (0.55 + rnd(&mut rng) * 0.5) * size;
            let ht = (0.40 + rnd(&mut rng) * 0.55) * size;
            let ang = rnd(&mut rng) * 3.14159;
            let var = 0.78 + rnd(&mut rng) * 0.22;
            if hf.surface(x, z) != 3 { continue; }
            let y = hf.height(x, z);
            if y < min_h + 0.15 { continue; }
            if (hf.height(x + 1.0, z) - y).abs() > 0.9 || (hf.height(x, z + 1.0) - y).abs() > 0.9 { continue; }
            let dry = (hash2((x / 45.0).floor() as i32, (z / 45.0).floor() as i32, 5) % 100) as f32 / 100.0;
            let cr = ((0.80 + 0.20 * dry) * var * 255.0) as u8;
            let cg = ((0.95 - 0.12 * dry) * var * 255.0) as u8;
            let cb = ((0.70 - 0.25 * dry) * var * 255.0) as u8;
            for q in 0..2 {
                let a = ang + q as f32 * 1.5708;
                let (dx, dz) = (a.cos() * w * 0.5, a.sin() * w * 0.5);
                let base = verts.len() as u16;
                let mk = |px: f32, py: f32, pz: f32, u: f32, v: f32| Vertex {
                    position: vec3(px, py, pz),
                    uv: vec2(u, v),
                    color: [cr, cg, cb, 255],
                    normal: vec4(0.0, 1.0, 0.0, 0.0),
                };
                verts.push(mk(x - dx, y - 0.02, z - dz, 0.0, 1.0));
                verts.push(mk(x + dx, y - 0.02, z + dz, 1.0, 1.0));
                verts.push(mk(x + dx, y + ht, z + dz, 1.0, 0.0));
                verts.push(mk(x - dx, y + ht, z - dz, 0.0, 0.0));
                idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            if verts.len() > 59000 {
                let vs = std::mem::take(&mut verts);
                let is = std::mem::take(&mut idx);
                out.push(EPart::from_mesh(Mesh { vertices: vs, indices: is, texture: Some(self.tex.clone()) }, false));
            }
        }
        if !verts.is_empty() {
            out.push(EPart::from_mesh(Mesh { vertices: verts, indices: idx, texture: Some(self.tex.clone()) }, false));
        }
        out
    }
}
