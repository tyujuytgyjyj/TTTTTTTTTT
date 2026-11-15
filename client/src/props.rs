// Props: أشجار وشجيرات وأسوار وأعمدة حقيقية بمواقعها الأصلية في خريطة West Coast.
// الملفات تنتجها الأداة tools/bake_props.py. تُبنى المجسمات على شكل خلايا 128م قرب الكاميرا فقط (streaming).
use crate::envload::{decode_tex, EPart};
use macroquad::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

const CELL: f32 = 128.0;

struct SubMesh {
    tex: u32,
    verts: Vec<Vertex>,
    idx: Vec<u16>,
}

struct Species {
    subs: Vec<SubMesh>,
    small: bool,
}

struct Placement {
    sp: u16,
    pos: Vec3,
    yaw: f32,
    scale: Vec3,
}

struct Inner {
    species: Vec<Species>,
    textures: Vec<Option<Texture2D>>,
    cells: HashMap<(i32, i32, u8), Vec<Placement>>,
    built: HashMap<(i32, i32, u8), Vec<EPart>>,
}

pub struct Props {
    inner: RefCell<Inner>,
}

fn rd_u32(b: &[u8], p: &mut usize) -> Option<u32> {
    let v = u32::from_le_bytes(b.get(*p..*p + 4)?.try_into().ok()?);
    *p += 4;
    Some(v)
}
fn rd_f32(b: &[u8], p: usize) -> f32 {
    f32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}

impl Props {
    pub fn load() -> Option<Props> {
        let dir = crate::world::map_path("props")?;
        let dir = dir.as_str();
        let pb = std::fs::read(format!("{}/props.bin", dir)).ok()?;
        let plb = std::fs::read(format!("{}/placements.bin", dir)).ok()?;
        if pb.len() < 8 || &pb[0..4] != b"PRP1" || plb.len() < 8 || &plb[0..4] != b"PLC2" {
            return None;
        }
        let mut p = 4usize;
        let ns = rd_u32(&pb, &mut p)? as usize;
        let mut species = Vec::with_capacity(ns);
        let mut max_tex = 0u32;
        for _ in 0..ns {
            let l = rd_u32(&pb, &mut p)? as usize;
            let name = String::from_utf8_lossy(pb.get(p..p + l)?).to_string();
            p += l;
            let nsub = rd_u32(&pb, &mut p)? as usize;
            let mut subs = Vec::with_capacity(nsub);
            let mut tris = 0usize;
            for _ in 0..nsub {
                let tex = rd_u32(&pb, &mut p)?;
                let nv = rd_u32(&pb, &mut p)? as usize;
                let ni = rd_u32(&pb, &mut p)? as usize;
                if tex != u32::MAX { max_tex = max_tex.max(tex + 1); }
                let vend = p + nv * 32;
                let iend = vend + ni * 2;
                if pb.len() < iend { return None; }
                let tint = if tex == u32::MAX { [170u8, 170, 175, 255] } else { [255u8, 255, 255, 255] };
                let mut verts = Vec::with_capacity(nv);
                for i in 0..nv {
                    let o = p + i * 32;
                    verts.push(Vertex {
                        position: vec3(rd_f32(&pb, o), rd_f32(&pb, o + 4), rd_f32(&pb, o + 8)),
                        uv: vec2(rd_f32(&pb, o + 24), rd_f32(&pb, o + 28)),
                        color: tint,
                        normal: vec4(rd_f32(&pb, o + 12), rd_f32(&pb, o + 16), rd_f32(&pb, o + 20), 0.0),
                    });
                }
                let mut idx = Vec::with_capacity(ni);
                for i in 0..ni { idx.push(u16::from_le_bytes([pb[vend + i * 2], pb[vend + i * 2 + 1]])); }
                p = iend;
                tris += ni / 3;
                subs.push(SubMesh { tex, verts, idx });
            }
            let lname = name.to_lowercase();
            let small = lname.starts_with("shrub") || lname.contains("grass") || lname.starts_with("spanish") || tris < 120;
            species.push(Species { subs, small });
        }
        let mut textures = Vec::new();
        for i in 0..max_tex {
            let t = std::fs::read(format!("{}/tex/{}.png", dir, i)).ok().and_then(|b| decode_tex(&b));
            textures.push(t);
        }
        let mut cells: HashMap<(i32, i32, u8), Vec<Placement>> = HashMap::new();
        let n = u32::from_le_bytes([plb[4], plb[5], plb[6], plb[7]]) as usize;
        for i in 0..n {
            let o = 8 + i * 30;
            if o + 30 > plb.len() { break; }
            let sp = u16::from_le_bytes([plb[o], plb[o + 1]]);
            if sp as usize >= species.len() { continue; }
            let pos = vec3(rd_f32(&plb, o + 2), rd_f32(&plb, o + 6), rd_f32(&plb, o + 10));
            let yaw = rd_f32(&plb, o + 14);
            let scale = vec3(rd_f32(&plb, o + 18), rd_f32(&plb, o + 22), rd_f32(&plb, o + 26));
            let tier = if species[sp as usize].small { 0u8 } else { 1u8 };
            let key = ((pos.x / CELL).floor() as i32, (pos.z / CELL).floor() as i32, tier);
            cells.entry(key).or_default().push(Placement { sp, pos, yaw, scale });
        }
        Some(Props { inner: RefCell::new(Inner { species, textures, cells, built: HashMap::new() }) })
    }

    /// يبني الخلايا القريبة (حد أقصى لكل إطار) ويحذف البعيدة ثم يرسم المرئي منها.
    pub fn draw(&self, cp: Vec3, quality: u8, vis: &dyn Fn(Vec3, Vec3) -> bool) {
        let big_r = match quality { 0 => 200.0f32, 1 => 320.0, _ => 420.0 };
        let radius = |tier: u8| if tier == 0 { big_r * 0.4 } else { big_r };
        {
            let mut inn = self.inner.borrow_mut();
            // حذف البعيد
            inn.built.retain(|k, _| {
                let c = vec2((k.0 as f32 + 0.5) * CELL, (k.1 as f32 + 0.5) * CELL);
                vec2(cp.x, cp.z).distance(c) < radius(k.2) + CELL * 1.6
            });
            // الخلايا المطلوبة وغير المبنية
            let mut need: Vec<((i32, i32, u8), f32)> = Vec::new();
            let r = (big_r / CELL).ceil() as i32 + 1;
            let (ccx, ccz) = ((cp.x / CELL).floor() as i32, (cp.z / CELL).floor() as i32);
            for dz in -r..=r {
                for dx in -r..=r {
                    for tier in 0..2u8 {
                        let k = (ccx + dx, ccz + dz, tier);
                        if inn.built.contains_key(&k) || !inn.cells.contains_key(&k) { continue; }
                        let c = vec2((k.0 as f32 + 0.5) * CELL, (k.1 as f32 + 0.5) * CELL);
                        let d = vec2(cp.x, cp.z).distance(c);
                        if d < radius(tier) + CELL * 0.8 { need.push((k, d)); }
                    }
                }
            }
            need.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            for (k, _) in need.into_iter().take(3) {
                let parts = Self::build_cell(&inn, &k);
                inn.built.insert(k, parts);
            }
        }
        let inn = self.inner.borrow();
        for (k, parts) in inn.built.iter() {
            let c = vec2((k.0 as f32 + 0.5) * CELL, (k.1 as f32 + 0.5) * CELL);
            if vec2(cp.x, cp.z).distance(c) > radius(k.2) + CELL * 0.8 { continue; }
            for p in parts {
                if vis(p.bmin, p.bmax) { draw_mesh(&p.mesh); }
            }
        }
    }

    fn build_cell(inn: &Inner, key: &(i32, i32, u8)) -> Vec<EPart> {
        let mut out: Vec<EPart> = Vec::new();
        let Some(list) = inn.cells.get(key) else { return out };
        let mut bufs: HashMap<u32, (Vec<Vertex>, Vec<u16>)> = HashMap::new();
        let flush = |tex_id: u32, vs: Vec<Vertex>, is: Vec<u16>, out: &mut Vec<EPart>| {
            if vs.is_empty() { return; }
            let tex = if tex_id == u32::MAX { None } else { inn.textures.get(tex_id as usize).cloned().flatten() };
            out.push(EPart::from_mesh(Mesh { vertices: vs, indices: is, texture: tex }, false));
        };
        for pl in list {
            let q = Quat::from_rotation_y(pl.yaw);
            for sub in &inn.species[pl.sp as usize].subs {
                let e = bufs.entry(sub.tex).or_insert_with(|| (Vec::new(), Vec::new()));
                if e.0.len() + sub.verts.len() > 60000 {
                    let (vs, is) = std::mem::take(e);
                    flush(sub.tex, vs, is, &mut out);
                }
                let base = e.0.len() as u16;
                for v in &sub.verts {
                    let n = q * vec3(v.normal.x, v.normal.y, v.normal.z);
                    e.0.push(Vertex {
                        position: pl.pos + q * (v.position * pl.scale),
                        uv: v.uv,
                        color: v.color,
                        normal: vec4(n.x, n.y, n.z, v.normal.w),
                    });
                }
                for i in &sub.idx { e.1.push(base + *i); }
            }
        }
        for (tid, (vs, is)) in bufs.into_iter() { flush(tid, vs, is, &mut out); }
        out
    }
}
