// Generic BeamNG-style soft body car: nodes + beams (spring/damp/deform/break), node-built tyres,
// node-ground friction, and a simple powertrain driven by the mod's own engine/gearbox data.
use crate::bng::{BKind, Vehicle};
use crate::jbeam::J;
use crate::world::World;
use macroquad::prelude::*;

#[derive(Clone)]
pub struct SBeam {
    pub a: usize, pub b: usize,
    pub k: f32, pub c: f32,
    pub l0: f32, pub lorig: f32,
    pub deform: f32, pub strength: f32,
    pub broken: bool,
    pub kind: BKind,
    pub long_b: f32, pub short_b: f32,
    pub hydro: Option<(f32, f32, f32)>, // factor, out, in
    pub lim_k: f32, pub lim_c: f32, pub lim_c_reb: f32,
    pub long_r: f32, pub short_r: f32,
    pub damp_reb: f32, pub damp_fast: f32, pub damp_reb_fast: f32, pub vsplit: f32,
}

pub struct Tire {
    pub name: String,
    pub spin: f32,
    pub tq_f: f32,
    pub tc_f: f32,
    pub hub1: usize, pub hub2: usize, pub arm: usize,
    pub tread: Vec<usize>, // 2*n nodes (ring L then ring R)
    pub n: usize,
    pub radius: f32,
    pub front: bool, pub right: bool,
    pub omega: f32,        // forward-rolling angular velocity
    pub inertia: f32,
    pub sgn: f32,
    pub drive: bool,
}

pub struct BngCar {
    pub pos: Vec<Vec3>, pub vel: Vec<Vec3>, pub frc: Vec<Vec3>,
    pub mass: Vec<f32>, pub inv_m: Vec<f32>,
    pub collide: Vec<bool>,
    pub tread_mu: Vec<f32>,   // >0 for tyre nodes
    pub beams: Vec<SBeam>,
    pub tris: Vec<[usize; 3]>,
    pub tires: Vec<Tire>,
    pub names: Vec<String>,
    pub n_body: usize,
    // contact bookkeeping
    pub cnorm: Vec<Vec3>, pub cforce: Vec<f32>, pub cmu: Vec<f32>,
    // powertrain
    pub torque: Vec<(f32, f32)>,
    pub ratios: Vec<f32>,     // index 0 = reverse, 1 = neutral, 2.. = forward
    pub final_drive: f32,
    pub idle: f32, pub max_rpm: f32,
    pub gear: usize,
    pub rpm: f32,
    pub shift_t: f32,
    pub throttle: f32, pub brake: f32, pub steer: f32, pub hand: bool,
    pub steer_cur: f32,
    pub softened: usize,
    pub steer_gain: f32,
    pub dbg: u32,
    pub rack_u: f32,
    pub rack_fixed: Option<f32>,
    pub rest_phi: f32,
    pub yaw_now: f32,
    pub phi_now: f32,
    pub step_count: u32,
    pub rack_sign: f32,
    pub total_mass: f32,
    pub h: f32,
}

fn table(j: &J) -> Vec<(f32, f32)> {
    let mut v = Vec::new();
    if let Some(a) = j.arr() {
        for r in a.iter().skip(1) {
            if let Some(r) = r.arr() { if r.len() >= 2 { if let (Some(x), Some(y)) = (r[0].num(), r[1].num()) { v.push((x as f32, y as f32)); } } }
        }
    }
    v
}

impl BngCar {
    pub fn new(veh: &Vehicle, origin: Vec3, yaw: f32, h: f32) -> BngCar {
        let q = Quat::from_rotation_y(yaw);
        let mut pos: Vec<Vec3> = veh.nodes.iter().map(|n| origin + q * n.pos).collect();
        let mut mass: Vec<f32> = veh.nodes.iter().map(|n| n.mass.max(1.0)).collect();
        let mut collide: Vec<bool> = veh.nodes.iter().map(|n| n.static_col).collect();
        let n_body = pos.len();
        let mut names: Vec<String> = veh.nodes.iter().map(|n| n.name.clone()).collect();
        let mut tread_mu = vec![0.0f32; n_body];
        let find = |nm: &str| veh.nodes.iter().position(|n| n.name == nm);
        let mut beams: Vec<SBeam> = Vec::new();
        for b in &veh.beams {
            let l = (pos[b.a] - pos[b.b]).length();
            if l < 1e-4 { continue; }
            let hyd = if b.kind == BKind::Hydro && b.hydro_src == "steering" { Some((b.hydro_factor, b.hydro_out, b.hydro_in)) } else { None };
            beams.push(SBeam { a: b.a, b: b.b, k: b.spring, c: b.damp, l0: l * if b.precomp > 0.0 { b.precomp } else { 1.0 } + b.pre_range, lorig: l, deform: b.deform, strength: b.strength, broken: false, kind: b.kind, long_b: b.long_bound, short_b: b.short_bound, hydro: hyd, lim_k: b.lim_k, lim_c: b.lim_c, lim_c_reb: b.lim_c_reb, long_r: b.long_r, short_r: b.short_r, damp_reb: b.damp_reb, damp_fast: b.damp_fast, damp_reb_fast: b.damp_reb_fast, vsplit: b.vsplit });
        }
        // ---- tyres: 16 rays x 2 shoulders, built from the two hub nodes of every wheel
        let mut tires = Vec::new();
        let tire_r = 0.2875f32;      // 175/70 R13
        let tire_w = 0.175f32;
        let nray = 32usize;
        for w in &veh.wheels {
            let (i1, i2, ia) = match (find(&w.node1), find(&w.node2), find(&w.node_arm)) { (Some(a), Some(b), Some(c)) => (a, b, c), (Some(a), Some(b), None) => (a, b, a), _ => continue };
            let c = (pos[i1] + pos[i2]) * 0.5;
            let axis = (pos[i2] - pos[i1]).normalize();
            let up = Vec3::Y;
            let u = (up - axis * up.dot(axis)).normalize();
            let v = axis.cross(u);
            let first = pos.len();
            let tread_m = 0.28f32;
            for side in 0..2 {
                for r in 0..nray {
                    let th = std::f32::consts::TAU * r as f32 / nray as f32;
                    let d = u * th.cos() + v * th.sin();
                    let off = axis * (if side == 0 { -tire_w * 0.45 } else { tire_w * 0.45 });
                    pos.push(c + d * tire_r + off);
                    mass.push(tread_m);
                    collide.push(true);
                    names.push(format!("{}_t{}_{}", w.name, side, r));
                    tread_mu.push(1.0);
                }
            }
            let idx = |side: usize, r: usize| first + side * nray + (r % nray);
            let kk = 1.1e5f32; // spoke stiffness
            let add = |beams: &mut Vec<SBeam>, a: usize, b: usize, k: f32, cdamp: f32, pos: &Vec<Vec3>| {
                let l = (pos[a] - pos[b]).length();
                beams.push(SBeam { a, b, k, c: cdamp, l0: l, lorig: l, deform: f32::MAX, strength: 1e9, broken: false, kind: BKind::Normal, long_b: 1.0, short_b: 1.0, hydro: None, lim_k: 0.0, lim_c: 0.0, lim_c_reb: -1.0, long_r: 0.0, short_r: 0.0, damp_reb: -1.0, damp_fast: -1.0, damp_reb_fast: -1.0, vsplit: 1e9 });
            };
            for side in 0..2 {
                let hub = if side == 0 { i1 } else { i2 };
                for r in 0..nray {
                    add(&mut beams, idx(side, r), hub, kk, 12.0, &pos);                 // spoke to own hub
                    add(&mut beams, idx(side, r), if side == 0 { i2 } else { i1 }, kk * 0.6, 9.0, &pos); // cross spoke
                    add(&mut beams, idx(side, r), idx(side, r + 1), 2.0e5, 10.0, &pos);  // tread ring
                    add(&mut beams, idx(side, r), idx(side, r + 2), 3.0e4, 7.0, &pos);   // bending
                }
            }
            for r in 0..nray {
                add(&mut beams, idx(0, r), idx(1, r), 0.8e5, 10.0, &pos);
                add(&mut beams, idx(0, r), idx(1, r + 1), 3.5e4, 6.0, &pos);
                add(&mut beams, idx(1, r), idx(0, r + 1), 3.5e4, 6.0, &pos);
            }
            let sgn = axis.cross(Vec3::Y).dot(Vec3::Z).signum();
            let tread: Vec<usize> = (first..first + 2 * nray).collect();
            let inertia = 0.6 + tread.iter().map(|&i| mass[i] * tire_r * tire_r).sum::<f32>() + 0.8;
            tires.push(Tire { name: w.name.clone(), spin: 0.0, tq_f: 0.0, tc_f: 1.0, hub1: i1, hub2: i2, arm: ia, tread, n: nray, radius: tire_r, front: c.z > origin.z + 0.5 * 0.0 + (q.inverse() * (c - origin)).z.signum() * 0.0 + 0.0 && (q.inverse() * (c - origin)).z > 0.0, right: (q.inverse() * (c - origin)).x < 0.0, omega: 0.0, inertia, sgn, drive: (q.inverse() * (c - origin)).z > 0.0 });
        }
        let nn = pos.len();
        let total_mass: f32 = mass.iter().sum();
        // powertrain data from the mod
        let mut torque = vec![(0.0, 0.0), (1000.0, 105.0), (3000.0, 148.0), (5000.0, 162.0), (7000.0, 128.0)];
        let (mut idle, mut max_rpm) = (800.0, 7000.0);
        if let Some(e) = &veh.engine {
            if let Some(t) = e.get("torque") { let tt = table(t); if tt.len() > 2 { torque = tt; } }
            if let Some(x) = e.get("idleRPM").and_then(|x| x.num()) { idle = x as f32; }
            if let Some(x) = e.get("maxRPM").and_then(|x| x.num()) { max_rpm = x as f32; }
        }
        let mut ratios = vec![-3.2, 0.0, 3.5, 2.0, 1.4, 1.0, 0.8];
        for g in &veh.gearbox {
            if let Some(a) = g.get("gearRatios").and_then(|x| x.arr()) { let v: Vec<f32> = a.iter().filter_map(|x| x.num()).map(|x| x as f32).collect(); if v.len() >= 3 { ratios = v; } }
        }
        let mut car = BngCar {
            pos, vel: vec![Vec3::ZERO; nn], frc: vec![Vec3::ZERO; nn],
            inv_m: mass.iter().map(|m| 1.0 / m).collect(), mass, collide, tread_mu, beams, tris: veh.tris.clone(), tires, names, n_body,
            cnorm: vec![Vec3::Y; nn], cforce: vec![0.0; nn], cmu: vec![0.0; nn],
            torque, ratios, final_drive: 4.1, idle, max_rpm, gear: 2, rpm: idle, shift_t: 0.0,
            throttle: 0.0, brake: 0.0, steer: 0.0, hand: false, steer_cur: 0.0, softened: 0, dbg: std::env::var("BNGDBG").ok().and_then(|s| s.parse().ok()).unwrap_or(0), steer_gain: std::env::var("STEERGAIN").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0), rack_u: 0.0, rack_fixed: None, rest_phi: 0.0, yaw_now: 0.0, phi_now: 0.0, step_count: 0, rack_sign: std::env::var("RACKSIGN").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0), total_mass, h,
        };
        car.stabilize();
        car
    }

    /// Soften only the beams that would be numerically unstable at this time step.
    fn stabilize(&mut self) {
        let h2 = self.h * self.h;
        for _ in 0..3 {
            let mut s = vec![0.0f32; self.pos.len()];
            for b in &self.beams { let kk = b.k.max(if b.kind == BKind::Bounded { b.lim_k } else { 0.0 }); s[b.a] += kk; s[b.b] += kk; }
            let mut scale = vec![1.0f32; self.pos.len()];
            for i in 0..self.pos.len() {
                let r = h2 * s[i] * self.inv_m[i];
                if r > 3.0 { scale[i] = 3.0 / r; }
            }
            for b in self.beams.iter_mut() {
                let f = scale[b.a].min(scale[b.b]);
                if f < 1.0 { b.k *= f; b.lim_k *= f; self.softened += 1; }
            }
        }
        // damping limit: c*h/m < 0.8
        for b in self.beams.iter_mut() {
            let m = 1.0 / (self.inv_m[b.a] + self.inv_m[b.b]);
            let cmax = 0.8 * m / self.h;
            if b.c > cmax { b.c = cmax; }
        }
    }

    pub fn front_phis(&self) -> Vec<f32> {
        let fwd = self.forward();
        let (mut l, mut nl, mut r, mut nr) = (Vec3::ZERO, 0.0, Vec3::ZERO, 0.0);
        for t in &self.tires { let p = (self.pos[t.hub1] + self.pos[t.hub2]) * 0.5; if t.right { r += p; nr += 1.0 } else { l += p; nl += 1.0 } }
        let left = (l / nl - r / nr).normalize_or_zero();
        let up = fwd.cross(left).normalize_or_zero();
        let left = up.cross(fwd).normalize_or_zero();
        self.tires.iter().filter(|t| t.front).map(|t| { let a = self.pos[t.hub2] - self.pos[t.hub1]; let al = if a.dot(left) >= 0.0 { a } else { -a }.normalize_or_zero(); (-al.dot(fwd)).atan2(al.dot(left)).to_degrees() }).collect()
    }

    pub fn center(&self) -> Vec3 {
        let mut c = Vec3::ZERO; let mut m = 0.0;
        for i in 0..self.n_body { c += self.pos[i] * self.mass[i]; m += self.mass[i]; }
        c / m
    }
    pub fn velocity(&self) -> Vec3 {
        let mut c = Vec3::ZERO; let mut m = 0.0;
        for i in 0..self.n_body { c += self.vel[i] * self.mass[i]; m += self.mass[i]; }
        c / m
    }
    pub fn forward(&self) -> Vec3 {
        // from average of rear tyres to average of front tyres
        let (mut f, mut r, mut nf, mut nr) = (Vec3::ZERO, Vec3::ZERO, 0.0, 0.0);
        for t in &self.tires { let c = (self.pos[t.hub1] + self.pos[t.hub2]) * 0.5; if t.front { f += c; nf += 1.0; } else { r += c; nr += 1.0; } }
        if nf == 0.0 || nr == 0.0 { return Vec3::Z; }
        (f / nf - r / nr).normalize_or_zero()
    }

    fn torque_at(&self, rpm: f32) -> f32 {
        let t = &self.torque;
        if rpm <= t[0].0 { return t[0].1; }
        for w in t.windows(2) { if rpm <= w[1].0 { let k = (rpm - w[0].0) / (w[1].0 - w[0].0); return w[0].1 + (w[1].1 - w[0].1) * k; } }
        0.0
    }

    pub fn step(&mut self, world: &World) {
        let h = self.h;
        self.step_count += 1;
        for i in 0..self.pos.len() { self.frc[i] = vec3(0.0, -9.81 * self.mass[i], 0.0); self.cforce[i] = 0.0; }
        // steering hydros
        let _st = self.steer_cur;
        // beams
        for bi in 0..self.beams.len() {
            let b = &mut self.beams[bi];
            if b.broken { continue; }
            let d = self.pos[b.b] - self.pos[b.a];
            let len = d.length();
            if len < 1e-6 { continue; }
            let dir = d / len;
            let mut target = b.l0;
            if let Some((fac, _out, _inn)) = b.hydro {
                target = b.lorig; let _ = fac;
            }
            let ext = len - target;
            let rel = (self.vel[b.b] - self.vel[b.a]).dot(dir);
            // damping: bump (shortening) vs rebound (extending), with optional fast-velocity split
            let mut cd = if rel < 0.0 { b.c } else if b.damp_reb >= 0.0 { b.damp_reb } else { b.c };
            if rel.abs() > b.vsplit {
                if rel < 0.0 && b.damp_fast >= 0.0 { cd = b.damp_fast; }
                if rel > 0.0 && b.damp_reb_fast >= 0.0 { cd = b.damp_reb_fast; }
            }
            let mut f = b.k * ext + cd * rel;
            if b.kind == BKind::Support { if ext > 0.0 { continue; } else { f = f.min(0.0); } }
            if b.kind == BKind::Bounded {
                // free travel: [-short_r, +long_r]; beyond that a stiff limit spring/damper acts
                let e = len - b.lorig;
                if e > b.long_r {
                    let cl = if b.lim_c_reb >= 0.0 { b.lim_c_reb } else { b.lim_c };
                    f += b.lim_k * (e - b.long_r) + cl * rel.max(0.0);
                } else if e < -b.short_r {
                    f += b.lim_k * (e + b.short_r) + b.lim_c * rel.min(0.0);
                }
            }
            let sf = b.k * ext;
            if b.hydro.is_none() {
                if sf.abs() > b.deform { b.l0 += (sf.abs() - b.deform) / b.k * sf.signum() * 0.6; }
                if sf.abs() > b.strength { b.broken = true; continue; }
            }
            let fv = dir * f;
            self.frc[b.a] += fv;
            self.frc[b.b] -= fv;
        }
        // powertrain / brakes -> tangential forces on tyre nodes
        self.powertrain(h);
        // steering: yaw-rate controller (bicycle model). A lateral force couple acts on the axles; the tyres' own
        // grip limit decides whether the car turns or slides.
        {
            let fwd = self.forward();
            let (mut fp, mut rp, mut nf, mut nr) = (Vec3::ZERO, Vec3::ZERO, 0.0, 0.0);
            let (mut fv, mut rv) = (Vec3::ZERO, Vec3::ZERO);
            let (mut l, mut nl, mut r, mut nrr) = (Vec3::ZERO, 0.0, Vec3::ZERO, 0.0);
            for t in &self.tires {
                let p = (self.pos[t.hub1] + self.pos[t.hub2]) * 0.5;
                let v = (self.vel[t.hub1] + self.vel[t.hub2]) * 0.5;
                if t.front { fp += p; fv += v; nf += 1.0 } else { rp += p; rv += v; nr += 1.0 }
                if t.right { r += p; nrr += 1.0 } else { l += p; nl += 1.0 }
            }
            if nf > 0.0 && nr > 0.0 && nl > 0.0 && nrr > 0.0 {
                let (fp, rp, fv, rv) = (fp / nf, rp / nr, fv / nf, rv / nr);
                let left = (l / nl - r / nrr).normalize_or_zero();
                let up = fwd.cross(left).normalize_or_zero();
                let left = up.cross(fwd).normalize_or_zero();
                let wb = (fp - rp).dot(fwd).max(1.0);
                let spd = self.velocity().dot(fwd);
                let yaw_now = (fv - rv).dot(left) / wb; // + = turning left
                let delta = self.steer_cur * 0.5;
                let mut yaw_des = spd / wb * delta.tan();
                // cap the lateral acceleration (~0.7 g) so high-speed turns slide instead of rolling the car over
                if spd.abs() > 1.0 { let lim = 7.0 / spd.abs(); yaw_des = yaw_des.clamp(-lim, lim); }
                let m = self.total_mass;
                let tmax = 0.55 * m * 9.81 * 0.5 * wb;
                let t_yaw = if self.dbg & 1 != 0 { 0.0 } else { (m * 5.0 * (yaw_des - yaw_now) * wb).clamp(-tmax, tmax) };
                self.yaw_now = yaw_now;
                // pure yaw couple spread over all chassis nodes (no sideways pushes on the wheels)
                let com = self.center();
                let mut inertia = 0.0;
                for i in 0..self.n_body { let r = self.pos[i] - com; let rp = r - up * r.dot(up); inertia += self.mass[i] * rp.length_squared(); }
                if inertia > 1.0 && t_yaw != 0.0 {
                    let tv = up * t_yaw;
                    for i in 0..self.n_body { let r = self.pos[i] - com; self.frc[i] += tv.cross(r) * (self.mass[i] / inertia); }
                }
            }
        }
        // aerodynamic drag
        {
            let v = self.velocity();
            let sp = v.length();
            let d = v * (-0.5 * 1.2 * 0.62 * 2.0 * sp) / self.n_body as f32;
            for i in 0..self.n_body { self.frc[i] += d; }
        }
        // collisions: ground + static boxes
        for i in 0..self.pos.len() {
            if !self.collide[i] { continue; }
            let p = self.pos[i]; let v = self.vel[i];
            let r = if self.tread_mu[i] > 0.0 { 0.012 } else { 0.03 };
            let gh = world.ground_h(p.x, p.z);
            let pen = gh + r - p.y;
            if pen > 0.0 {
                let n = world.ground_normal(p.x, p.z);
                let kc = (0.3 * self.mass[i] / (h * h)).min(3.0e5);
                let vn = v.dot(n);
                let fnm = (kc * pen - 0.3 * (kc * self.mass[i]).sqrt() * vn).max(0.0);
                self.frc[i] += n * fnm;
                self.cnorm[i] = n; self.cforce[i] = fnm;
                self.cmu[i] = if self.tread_mu[i] > 0.0 { self.tread_mu[i] * world.mu(p.x, p.z) } else { 0.45 };
            }
        }
        let (mut amin, mut amax) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        for i in 0..self.n_body { amin = amin.min(self.pos[i]); amax = amax.max(self.pos[i]); }
        let mut grid_c: Vec<usize> = Vec::new();
        world.grid.query(amin - Vec3::splat(0.6), amax + Vec3::splat(0.6), &mut grid_c);
        for &bi_ in grid_c.iter() {
            let b = &world.boxes[bi_];
            let (bmin, bmax) = b.aabb();
            if bmax.x + 0.5 < amin.x || bmin.x - 0.5 > amax.x || bmax.z + 0.5 < amin.z || bmin.z - 0.5 > amax.z || bmax.y + 0.5 < amin.y || bmin.y - 0.5 > amax.y { continue; }
            for i in 0..self.pos.len() {
                if !self.collide[i] { continue; }
                let p = self.pos[i];
                let l = b.local(p);
                let r = 0.04;
                let (dx, dy, dz) = (b.h.x + r - l.x.abs(), b.h.y + r - l.y.abs(), b.h.z + r - l.z.abs());
                if dx <= 0.0 || dy <= 0.0 || dz <= 0.0 { continue; }
                let (d, nl) = if dx <= dy && dx <= dz { (dx, vec3(l.x.signum(), 0.0, 0.0)) } else if dy <= dz { (dy, vec3(0.0, l.y.signum(), 0.0)) } else { (dz, vec3(0.0, 0.0, l.z.signum())) };
                let n = b.to_world_dir(nl);
                let kc = (0.3 * self.mass[i] / (h * h)).min(2.5e5);
                let vn = self.vel[i].dot(n);
                let fnm = (kc * d - 0.3 * (kc * self.mass[i]).sqrt() * vn).max(0.0);
                self.frc[i] += n * fnm;
                // Coulomb friction as a force with stability cap
                let vt = self.vel[i] - n * vn;
                let sp = vt.length();
                if sp > 1e-4 { self.frc[i] -= vt / sp * (0.4 * fnm).min(self.mass[i] * sp / h * 0.5); }
            }
        }
        // integrate (semi-implicit) + friction as velocity constraint for ground contacts
        for i in 0..self.pos.len() {
            let mut v = self.vel[i] + self.frc[i] * (self.inv_m[i] * h);
            if self.cforce[i] > 0.0 {
                let n = self.cnorm[i];
                let vn = v.dot(n);
                let vt = v - n * vn;
                let sp = vt.length();
                let dv = self.cmu[i] * self.cforce[i] * h * self.inv_m[i];
                if sp <= dv { v = n * vn; } else if sp > 1e-6 { v -= vt / sp * dv; }
            }
            v *= 1.0 - 0.01 * h;
            self.vel[i] = v;
            self.pos[i] += v * h;
        }
    }

    fn powertrain(&mut self, h: f32) {
        // wheel speeds
        let mut omegas = Vec::new();
        for t in self.tires.iter_mut() {
            let c = (self.pos[t.hub1] + self.pos[t.hub2]) * 0.5;
            let vc = (self.vel[t.hub1] + self.vel[t.hub2]) * 0.5;
            let axis = (self.pos[t.hub2] - self.pos[t.hub1]).normalize_or_zero();
            let mut s = 0.0;
            for &i in &t.tread {
                let r = self.pos[i] - c;
                let rr = r - axis * r.dot(axis);
                let rl = rr.length().max(1e-3);
                let tang = axis.cross(rr / rl);
                s += (self.vel[i] - vc).dot(tang) / rl;
            }
            let om = s / t.tread.len() as f32 * t.sgn;
            t.omega = om;
            t.spin += om * t.sgn * h;
            omegas.push(om);
        }
        let fwd = self.forward();
        let speed = self.velocity().dot(fwd);
        let drv: Vec<usize> = (0..self.tires.len()).filter(|&i| self.tires[i].drive).collect();
        let w_drv = if drv.is_empty() { 0.0 } else { drv.iter().map(|&i| omegas[i]).sum::<f32>() / drv.len() as f32 };
        // automatic gearbox
        let ratio = self.ratios.get(self.gear).copied().unwrap_or(3.5);
        let wheel_rpm = w_drv.abs() * 60.0 / std::f32::consts::TAU;
        self.shift_t -= h;
        let rad = self.tires.first().map(|t| t.radius).unwrap_or(0.29);
        let ground_rpm = speed.abs() / rad * 60.0 / std::f32::consts::TAU;
        if self.gear >= 2 && self.shift_t <= 0.0 {
            let r = ground_rpm * ratio.abs() * self.final_drive;
            if r > self.max_rpm * 0.82 && self.gear + 1 < self.ratios.len() { self.gear += 1; self.shift_t = 0.5; }
            else if r < self.max_rpm * 0.30 && self.gear > 2 { self.gear -= 1; self.shift_t = 0.5; }
        }
        let rev = speed < 1.0 && self.brake > 0.1 && self.throttle < 0.05;
        let mut thr = self.throttle;
        let mut brk = self.brake;
        if rev { self.gear = 0; thr = self.brake; brk = 0.0; }
        else if self.gear == 0 && (self.throttle > 0.05 || speed > 0.5) { self.gear = 2; }
        if self.gear == 0 && speed > 1.5 { brk = self.brake; thr = 0.0; }
        let ratio = self.ratios.get(self.gear).copied().unwrap_or(3.5) * self.final_drive;
        let slip_rpm = self.idle + thr * 2200.0;
        let rpm = (wheel_rpm * ratio.abs()).max(slip_rpm).min(self.max_rpm * 1.05);
        self.rpm = rpm;
        let eng_t = if rpm > self.max_rpm { 0.0 } else { self.torque_at(rpm) * thr };
        let drive_total = if self.dbg & 2 != 0 { 0.0 } else { eng_t * ratio * 0.9 };
        let per = if drv.is_empty() { 0.0 } else { drive_total / drv.len() as f32 };
        let mut react = Vec3::ZERO;
        for ti in 0..self.tires.len() {
            let t = &self.tires[ti];
            let mut tq = if t.drive { per } else { 0.0 };
            let mut tcf = 1.0f32;
            if t.drive && tq > 0.0 {
                // traction control: back off the torque when the driven wheel spins much faster than the ground
                let vg = ((self.vel[t.hub1] + self.vel[t.hub2]) * 0.5).dot(fwd).abs();
                let slip = t.omega.abs() * t.radius - vg;
                let lim = 3.0 + 0.25 * vg;
                if slip > lim { tcf = (1.0 - (slip - lim) / 6.0).clamp(0.30, 1.0); }
            }
            {
                let tm = &mut self.tires[ti];
                tm.tc_f += (tcf - tm.tc_f) * (h / 0.12).min(1.0);       // TC changes slowly (no on/off chatter)
                tm.tq_f += (tq * tm.tc_f - tm.tq_f) * (h / 0.05).min(1.0); // drive torque ramps over ~50 ms
                tq = tm.tq_f;
            }
            let t = &self.tires[ti];
            let mut tb = brk * if t.front { 2000.0 } else { 900.0 };
            if self.hand && !t.front { tb += 1800.0; }
            if tb > 0.0 {
                let lim = t.inertia * t.omega.abs() / h;
                tq += -t.omega.signum() * tb.min(lim);
            }
            if tq == 0.0 { continue; }
            let tq_a = tq * t.sgn; // torque about +axis
            let c = (self.pos[t.hub1] + self.pos[t.hub2]) * 0.5;
            let axis = (self.pos[t.hub2] - self.pos[t.hub1]).normalize_or_zero();
            let nt = t.tread.len() as f32;
            let tread = t.tread.clone();
            for &i in &tread {
                let r = self.pos[i] - c;
                let rr = r - axis * r.dot(axis);
                let rl = rr.length().max(1e-3);
                let tang = axis.cross(rr / rl);
                self.frc[i] += tang * (tq_a / (rl * nt));
            }
            // reaction torque is spread over the whole chassis as a pure couple (no violent point forces)
            react += -axis * tq_a;
        }
        if react.length() > 1e-3 {
            let com = self.center();
            let that = react.normalize();
            let mut inertia = 0.0;
            for i in 0..self.n_body { let r = self.pos[i] - com; let rp = r - that * r.dot(that); inertia += self.mass[i] * rp.length_squared(); }
            if inertia > 1.0 {
                for i in 0..self.n_body { let r = self.pos[i] - com; self.frc[i] += react.cross(r) * (self.mass[i] / inertia); }
            }
        }
    }
}
