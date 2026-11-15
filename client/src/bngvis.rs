// Visual side of a BeamNG car: flexbody meshes skinned onto the soft-body nodes.
use crate::bng::{to_game, Vehicle};
use crate::dae;
use crate::jbeam::J;
use macroquad::prelude::*;
use std::collections::HashMap;

struct Bind { n: [u32; 4], w: [f32; 4], rest: Vec3 }

pub struct FlexMesh {
    pub name: String,
    nrm_rest: Vec<Vec3>,
    bind: Vec<Bind>,
    pub mesh: Mesh,
}

pub struct WheelState { pub name: String, pub c: Vec3, pub axis: Vec3, pub spin: f32 }

struct Rigid { wheel: String, spins: bool, local: Vec<Vec3>, nloc: Vec<Vec3>, mesh: Mesh }

pub struct Visual {
    pub flex: Vec<FlexMesh>,
    rigid: Vec<Rigid>,
    wheel_rest: HashMap<String, (Vec3, Vec3, Vec3, Vec3)>,
    node_rest: Vec<Vec3>,
    rest_c: Vec3,
    rest_inv: Mat3,
    hubs: Vec<(usize, usize, bool, bool)>, // hub node pair, front, left
    pub verts: usize,
}

fn axes(pos: &[Vec3], hubs: &[(usize, usize, bool, bool)]) -> Mat3 {
    let (mut f, mut r, mut l, mut rt) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
    let (mut nf, mut nr, mut nl, mut nrt) = (0.0, 0.0, 0.0, 0.0);
    for &(a, b, front, left) in hubs {
        let p = (pos[a] + pos[b]) * 0.5;
        if front { f += p; nf += 1.0 } else { r += p; nr += 1.0 }
        if left { l += p; nl += 1.0 } else { rt += p; nrt += 1.0 }
    }
    let fwd = (f / nf - r / nr).normalize_or_zero();
    let left0 = l / nl - rt / nrt;
    let up = fwd.cross(left0).normalize_or_zero();
    let left = up.cross(fwd).normalize_or_zero();
    Mat3::from_cols(left, up, fwd)
}

fn mat_color(j: &J) -> Option<[u8; 4]> {
    let st = j.get("Stages")?.arr()?;
    for s in st {
        for k in ["baseColor", "diffuseColor"] {
            if let Some(a) = s.get(k).and_then(|x| x.arr()) {
                let v: Vec<f64> = a.iter().filter_map(|x| x.num()).collect();
                if v.len() >= 3 { return Some([(v[0].clamp(0.0, 1.0) * 255.0) as u8, (v[1].clamp(0.0, 1.0) * 255.0) as u8, (v[2].clamp(0.0, 1.0) * 255.0) as u8, if v.len() > 3 { (v[3].clamp(0.0, 1.0) * 255.0) as u8 } else { 255 }]); }
            }
        }
    }
    None
}

impl Visual {
    pub fn build(veh: &Vehicle, dae_text: &str, materials: &HashMap<String, J>, paint: [u8; 4]) -> Visual {
        let geoms = dae::parse(dae_text);
        let scene = dae::parse_scene(dae_text);
        let gmap: HashMap<&str, &dae::Geom> = geoms.iter().map(|g| (g.id.as_str(), g)).collect();
        let smap: HashMap<&str, &dae::SceneNode> = scene.iter().map(|n| (n.name.as_str(), n)).collect();
        let node_rest: Vec<Vec3> = veh.nodes.iter().map(|n| n.pos).collect();
        let find = |nm: &str| veh.nodes.iter().position(|n| n.name == nm);
        let mut hubs = Vec::new();
        let mut zsum = 0.0;
        let mut tmp = Vec::new();
        for w in &veh.wheels { if let (Some(a), Some(b)) = (find(&w.node1), find(&w.node2)) { let p = (node_rest[a] + node_rest[b]) * 0.5; zsum += p.z; tmp.push((a, b, p)); } }
        let zmean = if tmp.is_empty() { 0.0 } else { zsum / tmp.len() as f32 };
        for (a, b, p) in tmp { hubs.push((a, b, p.z > zmean, p.x > 0.0)); }
        let rest_c = node_rest.iter().fold(Vec3::ZERO, |a, b| a + *b) / node_rest.len() as f32;
        let rest_inv = axes(&node_rest, &hubs).inverse();
        let mut flex = Vec::new();
        let mut rigid: Vec<Rigid> = Vec::new();
        let mut wheel_rest: HashMap<String, (Vec3, Vec3, Vec3, Vec3)> = HashMap::new();
        for w in &veh.wheels {
            if let (Some(a), Some(b)) = (find(&w.node1), find(&w.node2)) {
                let c0 = (node_rest[a] + node_rest[b]) * 0.5;
                let a0 = (node_rest[b] - node_rest[a]).normalize_or_zero();
                let u0 = (Vec3::Y - a0 * Vec3::Y.dot(a0)).normalize_or_zero();
                wheel_rest.insert(w.name.clone(), (c0, a0, u0, a0.cross(u0)));
            }
        }
        let mut total = 0usize;
        const HIDDEN: [&str; 25] = ["tubs", "engine", "transmission", "exhaust", "halfshaft", "subframe", "swaybar", "swaymounts", "struts_", "lowarms", "uparms", "tierod", "steerrack", "hubs_", "fueltank", "radiator", "heatshield", "brace_", "_dmg", "$=", "engbay", "drumbrake", "wipers_R_base", "sunvisor", "shifter"];
        for f in &veh.flex {
            let lname = f.mesh.to_lowercase();
            if HIDDEN.iter().any(|h| lname.contains(h)) { continue; }
            // meshes of a wheel (rims, hubcaps, discs) are rigid on that wheel and spin with it
            let mut wmatch: Option<(String, bool)> = None;
            for w in &veh.wheels {
                let suf = &w.name;
                for gname in &f.groups {
                    let g = gname.as_str();
                    if g == w.hub_group || g == w.group || g == format!("hubcap_{}", suf) { wmatch = Some((w.name.clone(), true)); }
                    else if g == format!("wheelhub_{}", suf) && wmatch.is_none() { wmatch = Some((w.name.clone(), false)); }
                }
            }
            let sn = match smap.get(f.mesh.as_str()) { Some(s) => s, None => continue };
            if let Some((wn, spins)) = wmatch.clone() {
                if let Some(&(c0, a0, u0, v0)) = wheel_rest.get(&wn) {
                    let rot = Quat::from_euler(EulerRot::XYZ, f.rot.x.to_radians(), f.rot.y.to_radians(), f.rot.z.to_radians());
                    for gid in &sn.geoms {
                        let g = match gmap.get(gid.as_str()) { Some(g) => g, None => continue };
                        if g.pos.len() > 60000 { continue; }
                        let mut verts = Vec::new(); let mut local = Vec::new(); let mut nloc = Vec::new();
                        let mut vcol = vec![[190u8, 190, 195, 255]; g.pos.len()];
                        let (mut bmn, mut bmx) = (Vec3::splat(1e9), Vec3::splat(-1e9));
                        for i in 0..g.pos.len() { let pb = rot * (sn.m.transform_point3(g.pos[i]) * f.scale) + f.pos; let p = to_game(pb.x as f64, pb.y as f64, pb.z as f64); bmn = bmn.min(p); bmx = bmx.max(p); }
                        let dd = (bmn + bmx) * 0.5 - c0;
                        let lat = dd.dot(a0);
                        let perp = dd - a0 * lat;
                        let shift = if perp.length() > 0.3 { perp + a0 * (lat - lat.clamp(-0.1, 0.1)) } else { Vec3::ZERO };
                        for s in &g.subs { let key = s.material.trim_end_matches("-material").to_string(); let col = materials.get(&key).and_then(mat_color).unwrap_or([190, 190, 195, 255]); for &i in &s.idx { vcol[i as usize] = col; } }
                        for i in 0..g.pos.len() {
                            let pb = rot * (sn.m.transform_point3(g.pos[i]) * f.scale) + f.pos;
                            let nb = sn.m.transform_vector3(g.nrm[i]).normalize_or_zero();
                            let p = to_game(pb.x as f64, pb.y as f64, pb.z as f64) - shift;
                            let n = to_game(nb.x as f64, nb.y as f64, nb.z as f64);
                            let d = p - c0;
                            local.push(vec3(d.dot(a0), d.dot(u0), d.dot(v0)));
                            nloc.push(vec3(n.dot(a0), n.dot(u0), n.dot(v0)));
                            verts.push(Vertex { position: p, uv: vec2(0.01, 0.01), color: vcol[i], normal: vec4(n.x, n.y, n.z, 0.5) });
                        }
                        let mut idx: Vec<u16> = Vec::new();
                        for s in &g.subs { for &i in &s.idx { idx.push(i as u16); } }
                                total += verts.len();
                        rigid.push(Rigid { wheel: wn.clone(), spins, local, nloc, mesh: Mesh { vertices: verts, indices: idx, texture: None } });
                    }
                    continue;
                }
            }
            let cand: Vec<usize> = (0..veh.nodes.len()).filter(|&i| f.groups.is_empty() || f.groups.iter().any(|g| g.is_empty() || veh.nodes[i].groups.contains(g))).collect();
            let cand = if cand.len() >= 4 { cand } else { (0..veh.nodes.len()).collect() };
            let rot = Quat::from_euler(EulerRot::XYZ, f.rot.x.to_radians(), f.rot.y.to_radians(), f.rot.z.to_radians());
            for gid in &sn.geoms {
                let g = match gmap.get(gid.as_str()) { Some(g) => g, None => continue };
                if g.pos.len() > 60000 { continue; }
                let mut verts: Vec<Vertex> = Vec::new();
                let mut idx: Vec<u16> = Vec::new();
                let mut nrm_rest = Vec::new();
                let mut bind = Vec::new();
                let mut vcol = vec![[200u8, 200, 200, 255]; g.pos.len()];
                for s in &g.subs {
                    let key = s.material.trim_end_matches("-material").to_string();
                    let mut col = materials.get(&key).and_then(mat_color).unwrap_or([175, 175, 180, 255]);
                    if key.ends_with("_main") || key == "main" { col = paint; }
                    for &i in &s.idx { vcol[i as usize] = col; }
                }
                for i in 0..g.pos.len() {
                    let pb = sn.m.transform_point3(g.pos[i]);
                    let nb = sn.m.transform_vector3(g.nrm[i]).normalize_or_zero();
                    let pb = rot * (pb * f.scale) + f.pos;
                    let p = to_game(pb.x as f64, pb.y as f64, pb.z as f64);
                    let n = to_game(nb.x as f64, nb.y as f64, nb.z as f64);
                    let mut best: [(f32, usize); 4] = [(f32::MAX, 0); 4];
                    for &ci in &cand {
                        let d = (node_rest[ci] - p).length_squared();
                        if d < best[3].0 {
                            best[3] = (d, ci);
                            best.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                        }
                    }
                    let mut w = [0.0f32; 4];
                    let mut sw = 0.0;
                    for k in 0..4 { let dist = best[k].0.sqrt(); w[k] = 1.0 / ((dist + 0.04) * (dist + 0.04)); sw += w[k]; }
                    for k in 0..4 { w[k] /= sw; }
                    bind.push(Bind { n: [best[0].1 as u32, best[1].1 as u32, best[2].1 as u32, best[3].1 as u32], w, rest: p });
                    nrm_rest.push(n);
                    verts.push(Vertex { position: p, uv: vec2(0.01, 0.01), color: vcol[i], normal: vec4(n.x, n.y, n.z, 0.45) });
                }
                for s in &g.subs { for &i in &s.idx { idx.push(i as u16); } }
                if std::env::var("VISDBG").is_ok() && (f.mesh.contains("door") || f.mesh.contains("quarters") || f.mesh.contains("body")) {
                    let (mut a, mut b) = (Vec3::splat(1e9), Vec3::splat(-1e9)); for q in &bind { a = a.min(q.rest); b = b.max(q.rest); }
                    let (mut na, mut nb) = (Vec3::splat(1e9), Vec3::splat(-1e9)); for &ci in &cand { na = na.min(node_rest[ci]); nb = nb.max(node_rest[ci]); }
                    println!("VIS {} groups={:?} verts={} mesh_bbox=({:.2},{:.2},{:.2})-({:.2},{:.2},{:.2}) nodes={} node_bbox=({:.2},{:.2},{:.2})-({:.2},{:.2},{:.2})", f.mesh, f.groups, verts.len(), a.x, a.y, a.z, b.x, b.y, b.z, cand.len(), na.x, na.y, na.z, nb.x, nb.y, nb.z);
                }
                total += verts.len();
                flex.push(FlexMesh { name: f.mesh.clone(), nrm_rest, bind, mesh: Mesh { vertices: verts, indices: idx, texture: None } });
            }
        }
        Visual { flex, rigid, wheel_rest, node_rest, rest_c, rest_inv, hubs, verts: total }
    }

    /// true when the vehicle's own model provides a rim/hubcap mesh for this wheel
    pub fn has_rim_for(&self, wheel: &str, all_wheels: &[&str]) -> bool {
        if self.rigid.iter().any(|r| r.wheel == wheel) { return true; }
        // naming mismatch guard: if no rigid mesh matches ANY wheel name we cannot tell, so only treat as "missing" when there are none at all
        let any_match = self.rigid.iter().any(|r| all_wheels.iter().any(|w| *w == r.wheel));
        !self.rigid.is_empty() && !any_match
    }

    pub fn rigid_meshes(&self) -> impl Iterator<Item = &Mesh> { self.rigid.iter().map(|r| &r.mesh) }

    pub fn update(&mut self, pos: &[Vec3], wheels: &[WheelState]) {
        let r = axes(pos, &self.hubs) * self.rest_inv;
        let c_now = pos.iter().fold(Vec3::ZERO, |a, b| a + *b) / pos.len() as f32;
        // residual displacement of every node beyond the rigid motion
        let res: Vec<Vec3> = (0..self.node_rest.len()).map(|i| pos[i] - (c_now + r * (self.node_rest[i] - self.rest_c))).collect();
        for f in self.flex.iter_mut() {
            for (vi, b) in f.bind.iter().enumerate() {
                let mut d = Vec3::ZERO;
                for k in 0..4 { d += res[b.n[k] as usize] * b.w[k]; }
                let p = c_now + r * (b.rest - self.rest_c) + d;
                let n = r * f.nrm_rest[vi];
                let v = &mut f.mesh.vertices[vi];
                v.position = p;
                v.normal = vec4(n.x, n.y, n.z, v.normal.w);
            }
        }
        let up_now = r * Vec3::Y;
        for rg in self.rigid.iter_mut() {
            let ws = match wheels.iter().find(|w| w.name == rg.wheel) { Some(w) => w, None => continue };
            let a = ws.axis;
            let u = (up_now - a * up_now.dot(a)).normalize_or_zero();
            let v = a.cross(u);
            let (sp, cp) = if rg.spins { ws.spin.sin_cos() } else { (0.0, 1.0) };
            let (u2, v2) = (u * cp + v * sp, v * cp - u * sp);
            for i in 0..rg.local.len() {
                let l = rg.local[i];
                let nl = rg.nloc[i];
                let p = ws.c + a * l.x + u2 * l.y + v2 * l.z;
                let n = a * nl.x + u2 * nl.y + v2 * nl.z;
                let vv = &mut rg.mesh.vertices[i];
                vv.position = p;
                vv.normal = vec4(n.x, n.y, n.z, vv.normal.w);
            }
        }
    }
}
