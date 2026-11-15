use crashdrive_shared::{FullBodyState, VehicleState, WheelState, WorldCarState};
use macroquad::prelude::*;
use crate::car::Car;

const MM: f32 = 1000.0; // meters → mm

pub fn car_to_state(
    car: &Car,
    vehicle_id: u32,
    owner: u32,
    seq: u32,
    client_time_ms: u32,
    include_full: bool,
) -> VehicleState {
    let f = &car.frame;
    let vel = car.body.avg_velocity();
    let damage = (car.body.damage_ratio() * 25.0).clamp(0.0, 1.0);
    let rot = basis_to_quat(f.fwd, f.up, f.left);

    let mut wheels = [WheelState::default(); 4];
    for (i, w) in car.wheels.iter().take(4).enumerate() {
        wheels[i] = WheelState {
            steer: w.steer_angle,
            spin: w.omega,
            suspension: w.s,
            on_ground: w.grounded,
        };
    }

    let full = if include_full {
        Some(encode_full_body(car))
    } else {
        None
    };

    VehicleState {
        id: vehicle_id,
        owner,
        pos: f.c.to_array(),
        rot: [rot.x, rot.y, rot.z, rot.w],
        vel: vel.to_array(),
        ang_vel: [0.0; 3],
        damage,
        wheels,
        full,
        seq,
        client_time_ms,
    }
}

pub fn encode_full_body(car: &Car) -> FullBodyState {
    let c = car.frame.c;
    let mut nodes_mm = Vec::with_capacity(car.body.pos.len() * 3);
    for p in &car.body.pos {
        let d = *p - c;
        nodes_mm.push((d.x * MM).round().clamp(-32000.0, 32000.0) as i16);
        nodes_mm.push((d.y * MM).round().clamp(-32000.0, 32000.0) as i16);
        nodes_mm.push((d.z * MM).round().clamp(-32000.0, 32000.0) as i16);
    }
    let mut beam_plastic = Vec::new();
    for (i, b) in car.body.beams.iter().enumerate() {
        let ratio = b.l0 / b.lorig.max(1e-6);
        if (ratio - 1.0).abs() > 0.002 {
            let q = (ratio * 10000.0).round().clamp(0.0, 65535.0) as u16;
            beam_plastic.push((i as u16, q));
        }
    }
    FullBodyState {
        nodes_mm,
        beam_plastic,
        node_count: car.body.pos.len() as u16,
    }
}

pub fn apply_full_body(car: &mut Car, pos: Vec3, rot: Quat, full: &FullBodyState) {
    let n = car.body.pos.len().min(full.node_count as usize);
    if full.nodes_mm.len() < n * 3 {
        // fallback rigid
        apply_remote_pose_only(car, pos, rot);
        return;
    }
    for i in 0..n {
        let x = full.nodes_mm[i * 3] as f32 / MM;
        let y = full.nodes_mm[i * 3 + 1] as f32 / MM;
        let z = full.nodes_mm[i * 3 + 2] as f32 / MM;
        // nodes stored relative to center in world-aligned frame at encode time
        // encoder used world delta from center (not rotated). So just pos + delta.
        car.body.pos[i] = pos + vec3(x, y, z);
        car.body.vel[i] = Vec3::ZERO;
    }
    // Plastic beams
    for &(bi, q) in &full.beam_plastic {
        let i = bi as usize;
        if i < car.body.beams.len() {
            let ratio = q as f32 / 10000.0;
            car.body.beams[i].l0 = car.body.beams[i].lorig * ratio;
        }
    }
    // Reset beams not listed to original (optional - keep previous plastic if not sent)
    car.update_frame();
}

fn apply_remote_pose_only(car: &mut Car, pos: Vec3, rot: Quat) {
    let rc = car.body.rest_center;
    for i in 0..car.body.pos.len() {
        car.body.pos[i] = pos + rot * (car.body.rest[i] - rc);
        car.body.vel[i] = Vec3::ZERO;
    }
    car.update_frame();
}

pub fn apply_remote_pose(
    car: &mut Car,
    pos: Vec3,
    rot: Quat,
    wheels: &[crate::net::remote_vehicle::WheelVisual; 4],
) {
    apply_remote_pose_only(car, pos, rot);
    for (i, w) in car.wheels.iter_mut().enumerate().take(4) {
        w.steer_angle = wheels[i].steer;
        w.omega = wheels[i].spin;
        w.spin = wheels[i].spin;
        w.s = wheels[i].suspension.max(car.s_min);
        w.grounded = wheels[i].on_ground;
    }
}

/// Apply exact full state if present, else rigid pose.
pub fn apply_vehicle_state(car: &mut Car, pos: Vec3, rot: Quat, wheels: &[crate::net::remote_vehicle::WheelVisual; 4], full: &Option<FullBodyState>) {
    if let Some(fb) = full {
        apply_full_body(car, pos, rot, fb);
        for (i, w) in car.wheels.iter_mut().enumerate().take(4) {
            w.steer_angle = wheels[i].steer;
            w.omega = wheels[i].spin;
            w.spin = wheels[i].spin;
            w.s = wheels[i].suspension.max(car.s_min);
            w.grounded = wheels[i].on_ground;
        }
    } else {
        apply_remote_pose(car, pos, rot, wheels);
    }
}

fn basis_to_quat(fwd: Vec3, up: Vec3, _left: Vec3) -> Quat {
    let fwd = fwd.normalize_or_zero();
    let mut up = up.normalize_or_zero();
    let right = up.cross(fwd).normalize_or_zero();
    up = fwd.cross(right).normalize_or_zero();
    Quat::from_mat3(&Mat3::from_cols(right, up, fwd)).normalize()
}

pub fn car_to_world_state(car: &Car, slot: u16, authority: u32, seq: u32) -> WorldCarState {
    let f = &car.frame;
    let vel = car.body.avg_velocity();
    let rot = basis_to_quat(f.fwd, f.up, f.left);
    let damage = (car.body.damage_ratio() * 25.0).clamp(0.0, 1.0);
    let mut wheels = [WheelState::default(); 4];
    for (i, w) in car.wheels.iter().take(4).enumerate() {
        wheels[i] = WheelState {
            steer: w.steer_angle,
            spin: w.omega,
            suspension: w.s,
            on_ground: w.grounded,
        };
    }
    let full = if !car.asleep || damage > 0.01 {
        Some(encode_full_body(car))
    } else {
        None
    };
    WorldCarState {
        slot,
        model_spec: car.spec_idx as u16,
        pos: f.c.to_array(),
        rot: [rot.x, rot.y, rot.z, rot.w],
        vel: vel.to_array(),
        asleep: car.asleep,
        damage,
        full,
        wheels,
        seq,
        authority,
    }
}

pub fn apply_world_car_state(car: &mut Car, st: &WorldCarState) {
    let pos = Vec3::from_array(st.pos);
    let rot = Quat::from_array(st.rot).normalize();
    let wheels = [
        crate::net::remote_vehicle::WheelVisual {
            steer: st.wheels[0].steer,
            spin: st.wheels[0].spin,
            suspension: st.wheels[0].suspension,
            on_ground: st.wheels[0].on_ground,
        },
        crate::net::remote_vehicle::WheelVisual {
            steer: st.wheels[1].steer,
            spin: st.wheels[1].spin,
            suspension: st.wheels[1].suspension,
            on_ground: st.wheels[1].on_ground,
        },
        crate::net::remote_vehicle::WheelVisual {
            steer: st.wheels[2].steer,
            spin: st.wheels[2].spin,
            suspension: st.wheels[2].suspension,
            on_ground: st.wheels[2].on_ground,
        },
        crate::net::remote_vehicle::WheelVisual {
            steer: st.wheels[3].steer,
            spin: st.wheels[3].spin,
            suspension: st.wheels[3].suspension,
            on_ground: st.wheels[3].on_ground,
        },
    ];
    apply_vehicle_state(car, pos, rot, &wheels, &st.full);
    // apply velocity for smoother motion
    let v = Vec3::from_array(st.vel);
    for vel in car.body.vel.iter_mut() {
        *vel = v;
    }
    car.asleep = st.asleep;
    if !st.asleep {
        car.still_time = 0.0;
    }
}

pub fn collide_local_vs_remote(local: &mut Car, remote: &Car, h: f32) {
    if !aabb_overlap_simple(&local.aabb, &remote.aabb, 0.3) {
        return;
    }
    let r = 0.12f32;
    let kc = 1.8e5f32;
    let fb = remote.frame;
    let (bmin, bmax) = (
        remote.bounds_min - Vec3::splat(r),
        remote.bounds_max + Vec3::splat(r),
    );
    for si in 0..local.body.surface.len() {
        let i = local.body.surface[si];
        let p = local.body.pos[i];
        let l = fb.to_local(p);
        let dx0 = l.x - bmin.x;
        let dx1 = bmax.x - l.x;
        let dy0 = l.y - bmin.y;
        let dy1 = bmax.y - l.y;
        let dz0 = l.z - bmin.z;
        let dz1 = bmax.z - l.z;
        if dx0 <= 0.0 || dx1 <= 0.0 || dy0 <= 0.0 || dy1 <= 0.0 || dz0 <= 0.0 || dz1 <= 0.0 {
            continue;
        }
        let cands = [
            (dx0, vec3(-1.0, 0.0, 0.0)),
            (dx1, vec3(1.0, 0.0, 0.0)),
            (dy0, vec3(0.0, -1.0, 0.0)),
            (dy1, vec3(0.0, 1.0, 0.0)),
            (dz0, vec3(0.0, 0.0, -1.0)),
            (dz1, vec3(0.0, 0.0, 1.0)),
        ];
        let mut best = cands[0];
        for c in &cands {
            if c.0 < best.0 {
                best = *c;
            }
        }
        let (depth, nl) = best;
        let n = fb.to_world_dir(nl);
        let va = local.body.vel[i];
        let vn = va.dot(n);
        let fnm = (kc * depth - 400.0 * vn).max(0.0);
        let vt = va - n * vn;
        let sp = vt.length();
        let mut ff = Vec3::ZERO;
        if sp > 1e-4 {
            ff = -vt / sp * (0.4 * fnm).min(local.body.mass[i] * sp / h * 0.3);
        }
        local.body.frc[i] += n * fnm + ff;
    }
}

fn aabb_overlap_simple(a: &(Vec3, Vec3), b: &(Vec3, Vec3), pad: f32) -> bool {
    a.0.x <= b.1.x + pad
        && a.1.x + pad >= b.0.x
        && a.0.y <= b.1.y + pad
        && a.1.y + pad >= b.0.y
        && a.0.z <= b.1.z + pad
        && a.1.z + pad >= b.0.z
}
