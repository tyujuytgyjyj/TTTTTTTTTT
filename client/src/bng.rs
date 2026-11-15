// BeamNG vehicle loader (jbeam): parts -> slots -> nodes/beams/triangles/wheels/flexbodies.
use crate::jbeam::{self, J};
use macroquad::prelude::*;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct BNode {
    pub name: String,
    pub pos: Vec3, // game space: x left, y up, z forward
    pub mass: f32,
    pub fric: f32,
    pub slide: f32,
    pub collision: bool,
    pub static_col: bool,
    pub fixed: bool,
    pub groups: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BKind { Normal, Bounded, Support, Hydro, Anisotropic, Pressured }

#[derive(Clone, Debug)]
pub struct BBeam {
    pub a: usize,
    pub b: usize,
    pub spring: f32,
    pub damp: f32,
    pub deform: f32,
    pub strength: f32,
    pub kind: BKind,
    pub long_bound: f32,
    pub short_bound: f32,
    pub precomp: f32,
    pub hydro_src: String,
    pub hydro_factor: f32,
    pub hydro_out: f32,
    pub hydro_in: f32,
    pub lim_k: f32,
    pub lim_c: f32,
    pub lim_c_reb: f32,
    pub long_r: f32,
    pub short_r: f32,
    pub damp_reb: f32,
    pub damp_fast: f32,
    pub damp_reb_fast: f32,
    pub vsplit: f32,
    pub pre_range: f32,
}

#[derive(Clone, Debug, Default)]
pub struct BWheel {
    pub name: String,
    pub node1: String,
    pub node2: String,
    pub node_s: String,
    pub node_arm: String,
    pub hub_group: String,
    pub group: String,
    pub dir: f32,
    pub params: HashMap<String, f64>,
}

#[derive(Clone, Debug)]
pub struct BFlex {
    pub mesh: String,
    pub groups: Vec<String>,
    pub pos: Vec3,
    pub rot: Vec3,
    pub scale: Vec3,
    pub part: String,
}

#[derive(Default)]
pub struct Vehicle {
    pub nodes: Vec<BNode>,
    pub beams: Vec<BBeam>,
    pub tris: Vec<[usize; 3]>,
    pub wheels: Vec<BWheel>,
    pub flex: Vec<BFlex>,
    pub parts: Vec<String>,
    pub missing: Vec<String>,
    pub engine: Option<J>,
    pub gearbox: Vec<J>,
    pub skipped_beams: usize,
}

pub fn to_game(x: f64, y: f64, z: f64) -> Vec3 { vec3(x as f32, z as f32, -y as f32) }

/// read every jbeam file of the model (and "common") from a mod zip
pub fn load_parts(zip: &Path, model: &str) -> HashMap<String, J> {
    let mut parts = HashMap::new();
    let f = match std::fs::File::open(zip) { Ok(f) => f, Err(_) => return parts };
    let mut z = match zip::ZipArchive::new(std::io::BufReader::new(f)) { Ok(z) => z, Err(_) => return parts };
    let pre1 = format!("vehicles/{}/", model);
    for i in 0..z.len() {
        let mut e = match z.by_index(i) { Ok(e) => e, Err(_) => continue };
        let name = e.name().replace('\\', "/");
        if !name.ends_with(".jbeam") || !(name.starts_with(&pre1) || name.starts_with("vehicles/common/")) { continue; }
        let mut b = Vec::new();
        if e.read_to_end(&mut b).is_err() { continue; }
        if let Ok(J::Obj(items)) = jbeam::parse(&String::from_utf8_lossy(&b)) {
            for (k, v) in items { parts.insert(k, v); }
        }
    }
    parts
}

struct Ctx { vars: HashMap<String, f64> }

impl Ctx {
    fn num(&self, j: &J) -> f64 {
        match j {
            J::Num(n) => *n,
            J::Bool(b) => *b as i32 as f64,
            J::Str(s) => self.eval(s),
            _ => 0.0,
        }
    }
    fn eval(&self, s: &str) -> f64 {
        let t = s.trim();
        if t == "FLT_MAX" { return f32::MAX as f64; }
        if let Ok(n) = t.parse::<f64>() { return n; }
        if let Some(rest) = t.strip_prefix("$=") { return self.expr(rest); }
        if t.starts_with('$') { return *self.vars.get(t).unwrap_or(&0.0); }
        0.0
    }
    // tiny arithmetic evaluator: + - * / ( ) numbers $vars
    fn expr(&self, s: &str) -> f64 {
        let b: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
        let mut i = 0;
        self.e_add(&b, &mut i)
    }
    fn e_add(&self, b: &[char], i: &mut usize) -> f64 {
        let mut v = self.e_mul(b, i);
        while *i < b.len() && (b[*i] == '+' || b[*i] == '-') {
            let op = b[*i]; *i += 1;
            let r = self.e_mul(b, i);
            if op == '+' { v += r } else { v -= r }
        }
        v
    }
    fn e_mul(&self, b: &[char], i: &mut usize) -> f64 {
        let mut v = self.e_atom(b, i);
        while *i < b.len() && (b[*i] == '*' || b[*i] == '/') {
            let op = b[*i]; *i += 1;
            let r = self.e_atom(b, i);
            if op == '*' { v *= r } else if r != 0.0 { v /= r }
        }
        v
    }
    fn e_atom(&self, b: &[char], i: &mut usize) -> f64 {
        if *i >= b.len() { return 0.0; }
        if b[*i] == '-' { *i += 1; return -self.e_atom(b, i); }
        if b[*i] == '(' { *i += 1; let v = self.e_add(b, i); if *i < b.len() && b[*i] == ')' { *i += 1; } return v; }
        let st = *i;
        if b[*i] == '$' {
            *i += 1;
            while *i < b.len() && (b[*i].is_alphanumeric() || b[*i] == '_' || b[*i] == '.') { *i += 1; }
            let name: String = b[st..*i].iter().collect();
            return *self.vars.get(&name).unwrap_or(&0.0);
        }
        while *i < b.len() && (b[*i].is_ascii_digit() || b[*i] == '.' || b[*i] == 'e') { *i += 1; }
        let t: String = b[st..*i].iter().collect();
        if t.is_empty() { *i += 1; return 0.0; }
        t.parse().unwrap_or(0.0)
    }
}

fn rows(p: &J, key: &str) -> Vec<J> { p.get(key).and_then(|x| x.arr()).cloned().unwrap_or_default() }

fn list_str(j: &J) -> Vec<String> {
    match j {
        J::Str(s) => vec![s.clone()],
        J::Arr(a) => a.iter().filter_map(|x| x.str().map(|s| s.to_string())).collect(),
        _ => vec![],
    }
}

pub fn assemble(all: &HashMap<String, J>, cfg: &J) -> Vehicle {
    let mut veh = Vehicle::default();
    let chosen: HashMap<String, String> = cfg.get("parts").and_then(|p| p.obj()).map(|o| o.iter().filter_map(|(k, v)| v.str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default();
    let main = cfg.get_str("mainPartName").unwrap_or_default();
    // --- select active parts (depth-first through slots)
    let mut active: Vec<String> = Vec::new();
    let mut offs: Vec<Vec<J>> = Vec::new();
    fn add(name: &str, all: &HashMap<String, J>, chosen: &HashMap<String, String>, active: &mut Vec<String>, offs: &mut Vec<Vec<J>>, chain: &Vec<J>, missing: &mut Vec<String>) {
        if active.iter().any(|a| a == name) { return; }
        let p = match all.get(name) { Some(p) => p, None => { missing.push(name.to_string()); return; } };
        active.push(name.to_string());
        offs.push(chain.clone());
        for key in ["slots", "slots2"] {
            for r in rows(p, key).iter().skip(1) {
                let a = match r.arr() { Some(a) => a, None => continue };
                let slot = a.get(0).and_then(|x| x.str()).unwrap_or("").to_string();
                let def = if key == "slots" { a.get(1) } else { a.get(3) }.and_then(|x| x.str()).unwrap_or("").to_string();
                let pick = chosen.get(&slot).cloned().unwrap_or(def);
                let mut ch = chain.clone();
                if let Some(J::Obj(o)) = a.last() { if let Some((_, v)) = o.iter().find(|(k, _)| k == "nodeOffset") { ch.push(v.clone()); } }
                if !pick.is_empty() { add(&pick, all, chosen, active, offs, &ch, missing); }
            }
        }
    }
    add(&main, all, &chosen, &mut active, &mut offs, &Vec::new(), &mut veh.missing);
    veh.parts = active.clone();

    // --- variables
    let mut ctx = Ctx { vars: HashMap::new() };
    for pn in &active {
        let p = &all[pn];
        for r in rows(p, "variables").iter().skip(1) {
            if let Some(a) = r.arr() {
                if let (Some(n), Some(d)) = (a.get(0).and_then(|x| x.str()), a.get(4)) {
                    ctx.vars.insert(n.to_string(), ctx.num(d));
                }
            }
        }
    }
    fn flat(prefix: &str, j: &J, out: &mut HashMap<String, f64>) {
        match j {
            J::Num(n) => { out.insert(prefix.to_string(), *n); }
            J::Obj(o) => for (k, v) in o { flat(&format!("{}.{}", prefix, k), v, out); },
            _ => {}
        }
    }
    for pn in &active {
        if let Some(c) = all[pn].get("components") { flat("$components", c, &mut ctx.vars); }
    }
    if let Some(J::Obj(v)) = cfg.get("vars") { for (k, x) in v { ctx.vars.insert(format!("${}", k), ctx.num(x)); } }

    let mut index: HashMap<String, usize> = HashMap::new();
    let mut wheel_map: HashMap<String, usize> = HashMap::new();

    for (pi, pn) in active.iter().enumerate() {
        let p = &all[pn];
        let mut off = [0.0f64; 3];
        for o in &offs[pi] {
            for (ax, k) in ["x", "y", "z"].iter().enumerate() { if let Some(v) = o.get(k) { off[ax] += ctx.num(v); } }
        }
        // ---- nodes
        let mut m: HashMap<String, J> = HashMap::new();
        for r in rows(p, "nodes").iter().skip(1) {
            match r {
                J::Obj(o) => for (k, v) in o { m.insert(k.clone(), v.clone()); },
                J::Arr(a) if a.len() >= 4 => {
                    let mut local = m.clone();
                    if let Some(J::Obj(o)) = a.get(4) { for (k, v) in o { local.insert(k.clone(), v.clone()); } }
                    let name = a[0].str().unwrap_or("").to_string();
                    let g = |k: &str, d: f64| local.get(k).map(|v| ctx.num(v)).unwrap_or(d);
                    let groups = local.get("group").map(list_str).unwrap_or_default();
                    let node = BNode {
                        name: name.clone(),
                        pos: to_game(ctx.num(&a[1]) + off[0], ctx.num(&a[2]) + off[1], ctx.num(&a[3]) + off[2]),
                        mass: g("nodeWeight", 25.0) as f32,
                        fric: g("frictionCoef", 1.0) as f32,
                        slide: g("slidingFrictionCoef", 1.0) as f32,
                        collision: local.get("collision").map(|v| ctx.num(v) != 0.0 || matches!(v, J::Bool(true))).unwrap_or(true),
                        static_col: local.get("staticCollision").map(|v| ctx.num(v) != 0.0 || matches!(v, J::Bool(true))).unwrap_or(true),
                        fixed: matches!(local.get("fixed"), Some(J::Bool(true))),
                        groups,
                    };
                    if index.contains_key(&name) { continue; } // same node name in two parts = same node
                    index.insert(name, veh.nodes.len());
                    veh.nodes.push(node);
                }
                _ => {}
            }
        }
    }
    for pn in &active {
        let p = &all[pn];
        // ---- beams (and hydros)
        for (key, is_hydro) in [("beams", false), ("hydros", true)] {
            let mut m: HashMap<String, J> = HashMap::new();
            for r in rows(p, key).iter().skip(1) {
                match r {
                    J::Obj(o) => for (k, v) in o { m.insert(k.clone(), v.clone()); },
                    J::Arr(a) if a.len() >= 2 => {
                        let mut local = m.clone();
                        if let Some(J::Obj(o)) = a.last() { for (k, v) in o { local.insert(k.clone(), v.clone()); } }
                        let (na, nb) = (a[0].str().unwrap_or(""), a[1].str().unwrap_or(""));
                        let (ia, ib) = match (index.get(na), index.get(nb)) { (Some(x), Some(y)) => (*x, *y), _ => { veh.skipped_beams += 1; continue; } };
                        let g = |k: &str, d: f64| local.get(k).map(|v| ctx.num(v)).unwrap_or(d);
                        let kind = match local.get("beamType").and_then(|x| x.str()).unwrap_or("|NORMAL") {
                            "|BOUNDED" => BKind::Bounded, "|SUPPORT" => BKind::Support, "|ANISOTROPIC" => BKind::Anisotropic,
                            "|PRESSURED" => BKind::Pressured, _ => if is_hydro { BKind::Hydro } else { BKind::Normal },
                        };
                        veh.beams.push(BBeam {
                            a: ia, b: ib,
                            spring: g("beamSpring", 4300000.0) as f32, damp: g("beamDamp", 580.0) as f32,
                            deform: g("beamDeform", 220000.0).min(1e12) as f32, strength: g("beamStrength", 1e6).min(1e12) as f32,
                            kind, long_bound: g("beamLongBound", 1.0) as f32, short_bound: g("beamShortBound", 1.0) as f32,
                            precomp: g("beamPrecompression", 1.0) as f32,
                            hydro_src: if is_hydro { local.get("inputSource").and_then(|x| x.str()).unwrap_or("steering").to_string() } else { String::new() },
                            hydro_factor: g("factor", g("inputFactor", 1.0)) as f32,
                            hydro_out: g("outLimit", 1.0) as f32,
                            hydro_in: g("inLimit", 1.0) as f32,
                            lim_k: g("beamLimitSpring", 1.0e6) as f32,
                            lim_c: g("beamLimitDamp", 1000.0) as f32,
                            lim_c_reb: g("beamLimitDampRebound", -1.0) as f32,
                            long_r: g("longBoundRange", 0.0) as f32,
                            short_r: g("shortBoundRange", 0.0) as f32,
                            damp_reb: g("beamDampRebound", -1.0) as f32,
                            damp_fast: g("beamDampFast", -1.0) as f32,
                            damp_reb_fast: g("beamDampReboundFast", -1.0) as f32,
                            vsplit: g("beamDampVelocitySplit", 1e9) as f32,
                            pre_range: g("precompressionRange", 0.0) as f32,
                        });
                    }
                    _ => {}
                }
            }
        }
        // ---- triangles
        for r in rows(p, "triangles").iter().skip(1) {
            if let Some(a) = r.arr() {
                if a.len() >= 3 {
                    if let (Some(x), Some(y), Some(z)) = (index.get(a[0].str().unwrap_or("")), index.get(a[1].str().unwrap_or("")), index.get(a[2].str().unwrap_or(""))) {
                        veh.tris.push([*x, *y, *z]);
                    }
                }
            }
        }
        // ---- wheels (rows from several parts with the same name are merged)
        let mut m: HashMap<String, J> = HashMap::new();
        for r in rows(p, "pressureWheels").iter().skip(1) {
            match r {
                J::Obj(o) => for (k, v) in o { m.insert(k.clone(), v.clone()); },
                J::Arr(a) if !a.is_empty() => {
                    let name = a[0].str().unwrap_or("").to_string();
                    let wi = *wheel_map.entry(name.clone()).or_insert_with(|| { veh.wheels.push(BWheel { name: name.clone(), ..Default::default() }); veh.wheels.len() - 1 });
                    let w = &mut veh.wheels[wi];
                    let mut local = m.clone();
                    if let Some(J::Obj(o)) = a.last() { for (k, v) in o { local.insert(k.clone(), v.clone()); } }
                    if let Some(s) = a.get(1).and_then(|x| x.str()) { if !s.is_empty() { w.hub_group = s.to_string(); } }
                    if let Some(s) = a.get(2).and_then(|x| x.str()) { if !s.is_empty() { w.group = s.to_string(); } }
                    if let Some(s) = a.get(3).and_then(|x| x.str()) { w.node1 = s.to_string(); }
                    if let Some(s) = a.get(4).and_then(|x| x.str()) { w.node2 = s.to_string(); }
                    if let Some(s) = a.get(5).and_then(|x| x.str()) { w.node_s = s.to_string(); }
                    if let Some(s) = a.get(6).and_then(|x| x.str()) { w.node_arm = s.to_string(); }
                    if let Some(n) = a.get(7).and_then(|x| x.num()) { w.dir = n as f32; }
                    for (k, v) in local { if matches!(v, J::Num(_) | J::Str(_)) { w.params.insert(k, ctx.num(&v)); } }
                }
                _ => {}
            }
        }
        // ---- flexbodies
        let mut m: HashMap<String, J> = HashMap::new();
        for r in rows(p, "flexbodies").iter().skip(1) {
            match r {
                J::Obj(o) => for (k, v) in o { m.insert(k.clone(), v.clone()); },
                J::Arr(a) if a.len() >= 2 => {
                    let mut local = m.clone();
                    if let Some(J::Obj(o)) = a.last() { for (k, v) in o { local.insert(k.clone(), v.clone()); } }
                    let v3 = |k: &str, d: f32| -> Vec3 {
                        match local.get(k) { Some(o) => vec3(o.get("x").map(|x| ctx.num(x) as f32).unwrap_or(d), o.get("y").map(|x| ctx.num(x) as f32).unwrap_or(d), o.get("z").map(|x| ctx.num(x) as f32).unwrap_or(d)), None => Vec3::splat(d) }
                    };
                    let p0 = v3("pos", 0.0);
                    veh.flex.push(BFlex { mesh: a[0].str().unwrap_or("").to_string(), groups: list_str(&a[1]), pos: p0, rot: v3("rot", 0.0), scale: v3("scale", 1.0), part: pn.clone() });
                }
                _ => {}
            }
        }
        if let Some(J::Obj(e)) = p.get("mainEngine") {
            let mut cur = match veh.engine.take() { Some(J::Obj(v)) => v, _ => Vec::new() };
            for (k, v) in e { cur.retain(|(a, _)| a != k); cur.push((k.clone(), v.clone())); }
            veh.engine = Some(J::Obj(cur));
        }
        if let Some(g) = p.get("gearbox") { veh.gearbox.push(g.clone()); }
    }
    veh
}

pub fn summary(v: &Vehicle) -> String {
    format!("parts={} nodes={} beams={} (skipped {}) tris={} wheels={} flex={} missing={:?}", v.parts.len(), v.nodes.len(), v.beams.len(), v.skipped_beams, v.tris.len(), v.wheels.len(), v.flex.len(), v.missing)
}
