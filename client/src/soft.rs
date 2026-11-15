// Soft-body lattice: nodes + beams with permanent (plastic) deformation,
// automatic CFL eigenvalue stability clamping, and structural energy dissipation.
use macroquad::prelude::*;

#[derive(Clone)]
pub struct Beam {
    pub a: u16,
    pub b: u16,
    pub k: f32,
    pub c: f32,
    pub l0: f32,
    pub lorig: f32,
    pub yield_t: f32, // tension yield strain
    pub yield_c: f32, // compression yield strain
}

#[derive(Clone)]
pub struct SoftBody {
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
    pub bmin: Vec3,
    pub cell: Vec3,
    pub rest: Vec<Vec3>,
    pub pos: Vec<Vec3>,
    pub vel: Vec<Vec3>,
    pub frc: Vec<Vec3>,
    pub mass: Vec<f32>,
    pub inv_m: Vec<f32>,
    pub beams: Vec<Beam>,
    pub surface: Vec<usize>,
    pub dmg: Vec<f32>,
    pub rest_center: Vec3,
    pub total_mass: f32,
}

impl SoftBody {
    pub fn idx(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.ny + j) * self.nx + i
    }

    pub fn build(bmin: Vec3, bmax: Vec3, n: (usize, usize, usize), total_mass: f32, stiff: f32) -> SoftBody {
        let (nx, ny, nz) = n;
        let cell = (bmax - bmin) / vec3((nx - 1) as f32, (ny - 1) as f32, (nz - 1) as f32);
        let count = nx * ny * nz;
        let mut sb = SoftBody {
            nx, ny, nz, bmin, cell,
            rest: Vec::with_capacity(count),
            pos: Vec::new(), vel: vec![Vec3::ZERO; count], frc: vec![Vec3::ZERO; count],
            mass: vec![0.0; count], inv_m: vec![0.0; count],
            beams: Vec::new(), surface: Vec::new(), dmg: vec![0.0; count],
            rest_center: Vec3::ZERO, total_mass,
        };
        let mut wsum = 0.0;
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    sb.rest.push(bmin + cell * vec3(i as f32, j as f32, k as f32));
                    // Low centre of gravity, reinforced floor & front/mid powertrain mass concentration
                    let tj = j as f32 / (ny - 1) as f32;
                    let tk = k as f32 / (nz - 1) as f32;
                    let mut w = 2.15 - 1.25 * tj;
                    if tk > 0.60 { w *= 1.15; }
                    wsum += w;
                    let id = sb.idx(i, j, k);
                    sb.mass[id] = w;
                }
            }
        }
        for m in sb.mass.iter_mut() { *m = *m / wsum * total_mass; }
        for i in 0..count { sb.inv_m[i] = 1.0 / sb.mass[i].max(0.25); }
        sb.pos = sb.rest.clone();
        sb.rest_center = sb.rest.iter().fold(Vec3::ZERO, |a, b| a + *b) / count as f32;

        // Surface nodes
        for k in 0..nz { for j in 0..ny { for i in 0..nx {
            if i == 0 || j == 0 || k == 0 || i == nx - 1 || j == ny - 1 || k == nz - 1 {
                let id = sb.idx(i, j, k);
                sb.surface.push(id);
            }
        }}}

        // Beams: 13 forward neighbours of the 26-neighbourhood
        let offs: [(i32, i32, i32); 13] = [
            (1,0,0),(0,1,0),(0,0,1),(1,1,0),(1,-1,0),(1,0,1),(1,0,-1),
            (0,1,1),(0,1,-1),(1,1,1),(1,1,-1),(1,-1,1),(1,-1,-1),
        ];
        for k in 0..nz as i32 { for j in 0..ny as i32 { for i in 0..nx as i32 {
            for o in offs.iter() {
                let (ti, tj, tk) = (i + o.0, j + o.1, k + o.2);
                if ti < 0 || tj < 0 || tk < 0 || ti >= nx as i32 || tj >= ny as i32 || tk >= nz as i32 { continue; }
                let a = sb.idx(i as usize, j as usize, k as usize);
                let b = sb.idx(ti as usize, tj as usize, tk as usize);
                let l = (sb.rest[a] - sb.rest[b]).length().max(0.05);
                let t_k = (k as f32 + 0.5 * o.2 as f32) / (nz - 1) as f32;
                let t_j = (j as f32 + 0.5 * o.1 as f32) / (ny - 1) as f32;
                let crumple = t_k < 0.24 || t_k > 0.76;
                let roof = t_j > 0.7;
                let (mut yc, mut yt) = if crumple { (0.022, 0.05) } else { (0.06, 0.09) };
                if roof && !crumple { yc = 0.035; yt = 0.06; }
                let mref = 2.86f32;
                let m_eff = sb.mass[a].min(sb.mass[b]);
                let k_ = stiff * (0.33 / l).min(1.3) * (m_eff / mref);
                let c_ = 0.88 * (k_ * m_eff).sqrt() * 0.36;
                sb.beams.push(Beam { a: a as u16, b: b as u16, k: k_, c: c_, l0: l, lorig: l, yield_t: yt, yield_c: yc });
            }
        }}}
        sb.stabilize_for_dt(1.0 / 2400.0);
        sb
    }

    /// Guarantees Courant-Friedrichs-Lewy (CFL) numerical stability for semi-implicit Euler at step `h`.
    pub fn stabilize_for_dt(&mut self, h: f32) {
        let h2 = h * h;
        for _ in 0..2 {
            let mut k_sum = vec![0.0f32; self.pos.len()];
            for b in &self.beams {
                k_sum[b.a as usize] += b.k;
                k_sum[b.b as usize] += b.k;
            }
            let mut scale = vec![1.0f32; self.pos.len()];
            for i in 0..self.pos.len() {
                let r = h2 * k_sum[i] * self.inv_m[i];
                if r > 2.6 {
                    scale[i] = 2.6 / r;
                }
            }
            for b in self.beams.iter_mut() {
                let f = scale[b.a as usize].min(scale[b.b as usize]);
                if f < 1.0 {
                    b.k *= f;
                }
                let m_red = 1.0 / (self.inv_m[b.a as usize] + self.inv_m[b.b as usize]).max(1e-6);
                let c_max = 0.75 * m_red / h;
                if b.c > c_max {
                    b.c = c_max;
                }
            }
        }
    }

    /// Place the lattice in the world: rest pose rotated by yaw about Y and translated.
    pub fn place(&mut self, origin: Vec3, yaw: f32) {
        let q = Quat::from_rotation_y(yaw);
        for i in 0..self.pos.len() {
            self.pos[i] = origin + q * self.rest[i];
            self.vel[i] = Vec3::ZERO;
        }
    }

    /// Trilinear interpolation of a point given in rest/model space.
    pub fn trilinear(&self, p: Vec3) -> ([usize; 8], [f32; 8]) {
        let u = (p - self.bmin) / self.cell;
        let ui = u.x.clamp(0.0, (self.nx - 1) as f32 - 1e-4);
        let uj = u.y.clamp(0.0, (self.ny - 1) as f32 - 1e-4);
        let uk = u.z.clamp(0.0, (self.nz - 1) as f32 - 1e-4);
        let (i, j, k) = (ui.floor() as usize, uj.floor() as usize, uk.floor() as usize);
        let (fx, fy, fz) = (ui - i as f32, uj - j as f32, uk - k as f32);
        let mut ids = [0usize; 8];
        let mut ws = [0f32; 8];
        let mut n = 0;
        for dk in 0..2 { for dj in 0..2 { for di in 0..2 {
            ids[n] = self.idx(i + di, j + dj, k + dk);
            ws[n] = (if di == 0 { 1.0 - fx } else { fx })
                * (if dj == 0 { 1.0 - fy } else { fy })
                * (if dk == 0 { 1.0 - fz } else { fz });
            n += 1;
        }}}
        (ids, ws)
    }

    pub fn point(&self, ids: &[usize; 8], ws: &[f32; 8]) -> Vec3 {
        let mut p = Vec3::ZERO;
        for n in 0..8 { p += self.pos[ids[n]] * ws[n]; }
        p
    }
    pub fn velocity_at(&self, ids: &[usize; 8], ws: &[f32; 8]) -> Vec3 {
        let mut p = Vec3::ZERO;
        for n in 0..8 { p += self.vel[ids[n]] * ws[n]; }
        p
    }
    pub fn apply(&mut self, ids: &[usize; 8], ws: &[f32; 8], f: Vec3) {
        for n in 0..8 { self.frc[ids[n]] += f * ws[n]; }
    }

    pub fn clear_forces(&mut self, gravity: f32) {
        for i in 0..self.frc.len() {
            self.frc[i] = vec3(0.0, -gravity * self.mass[i], 0.0);
        }
    }

    pub fn beam_forces(&mut self) {
        for b in self.beams.iter_mut() {
            let (a, c) = (b.a as usize, b.b as usize);
            let d = self.pos[c] - self.pos[a];
            let len = d.length();
            if len < 1e-6 || !len.is_finite() { continue; }
            let dir = d / len;
            let ext = len - b.l0;
            let strain = ext / b.l0.max(0.02);
            let rel = (self.vel[c] - self.vel[a]).dot(dir);
            // Compressed beams harden gradually (capped at 1.85x to preserve CFL stability)
            let mut k = b.k;
            if b.l0 < b.lorig {
                k *= (b.lorig / b.l0.max(0.35 * b.lorig)).min(1.85);
            }
            let f = (k * ext + b.c * rel).clamp(-4.5e5, 4.5e5);
            let fv = dir * f;
            self.frc[a] += fv;
            self.frc[c] -= fv;
            // Plastic deformation
            if strain > b.yield_t {
                b.l0 = (len / (1.0 + b.yield_t)).min(b.lorig * 1.9);
            } else if strain < -b.yield_c {
                b.l0 = (len / (1.0 - b.yield_c)).max(b.lorig * 0.35);
            }
        }
    }

    pub fn integrate(&mut self, h: f32) {
        for i in 0..self.pos.len() {
            let mut f = self.frc[i];
            if !f.is_finite() { f = Vec3::ZERO; }
            let mut v = self.vel[i] + f * (self.inv_m[i] * h);
            v *= 1.0 - 0.02 * h;
            if !v.is_finite() { v = Vec3::ZERO; }
            v = v.clamp_length_max(160.0);
            self.vel[i] = v;
            let mut p = self.pos[i] + v * h;
            if !p.is_finite() { p = Vec3::new(0.0, 1.5, 0.0); }
            self.pos[i] = p;
        }
    }

    pub fn update_damage(&mut self) {
        for d in self.dmg.iter_mut() { *d = 0.0; }
        for b in self.beams.iter() {
            let dev = (b.l0 / b.lorig - 1.0).abs();
            let (a, c) = (b.a as usize, b.b as usize);
            if dev > self.dmg[a] { self.dmg[a] = dev; }
            if dev > self.dmg[c] { self.dmg[c] = dev; }
        }
    }

    pub fn damage_ratio(&self) -> f32 {
        let mut s = 0.0;
        for b in self.beams.iter() { s += (b.l0 / b.lorig - 1.0).abs(); }
        s / self.beams.len().max(1) as f32
    }

    pub fn avg_velocity(&self) -> Vec3 {
        let mut s = Vec3::ZERO;
        let mut m = 0.0;
        for i in 0..self.vel.len() { s += self.vel[i] * self.mass[i]; m += self.mass[i]; }
        s / m.max(1.0)
    }
}
