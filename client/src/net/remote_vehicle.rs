use crashdrive_shared::{FullBodyState, VehicleSpawn, VehicleState};
use macroquad::prelude::*;

#[derive(Clone, Copy, Default)]
pub struct WheelVisual {
    pub steer: f32,
    pub spin: f32,
    pub suspension: f32,
    pub on_ground: bool,
}

#[derive(Clone)]
pub struct RemoteVehicle {
    pub id: u32,
    pub owner: u32,
    pub model: String,
    pub color: [f32; 3],
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    pub damage: f32,
    pub full: Option<FullBodyState>,
    pub wheels: [WheelVisual; 4],
    prev_pos: Vec3,
    prev_vel: Vec3,
    prev_rot: Quat,
    target_pos: Vec3,
    target_vel: Vec3,
    target_rot: Quat,
    interp_t: f32,
    last_seq: u32,
}

impl RemoteVehicle {
    pub fn from_spawn(spawn: &VehicleSpawn) -> Self {
        let pos = Vec3::from_array(spawn.pos);
        let rot = Quat::from_rotation_y(spawn.rot_y);
        Self {
            id: spawn.vehicle_id,
            owner: spawn.owner,
            model: spawn.model.clone(),
            color: spawn.color,
            pos,
            rot,
            vel: Vec3::ZERO,
            damage: 0.0,
            full: None,
            wheels: [WheelVisual::default(); 4],
            prev_pos: pos,
            prev_vel: Vec3::ZERO,
            prev_rot: rot,
            target_pos: pos,
            target_vel: Vec3::ZERO,
            target_rot: rot,
            interp_t: 1.0,
            last_seq: 0,
        }
    }

    pub fn apply_state(&mut self, state: &VehicleState) {
        if state.seq <= self.last_seq && self.last_seq - state.seq < 10000 {
            return;
        }
        self.last_seq = state.seq;
        let new_pos = Vec3::from_array(state.pos);
        let new_vel = Vec3::from_array(state.vel);
        if !new_pos.is_finite() || !new_vel.is_finite() {
            return;
        }
        self.prev_pos = self.pos;
        self.prev_vel = self.vel;
        self.prev_rot = self.rot;
        self.target_pos = new_pos;
        self.target_vel = new_vel.clamp_length_max(140.0);
        let mut q = Quat::from_array(state.rot).normalize();
        if !q.is_finite() {
            q = Quat::IDENTITY;
        }
        if self.prev_rot.dot(q) < 0.0 {
            q = -q;
        }
        self.target_rot = q;
        self.vel = self.target_vel;
        self.damage = state.damage.clamp(0.0, 1.0);
        if state.full.is_some() {
            self.full = state.full.clone();
            self.pos = self.target_pos;
            self.rot = self.target_rot;
            self.prev_pos = self.pos;
            self.prev_rot = self.rot;
            self.interp_t = 1.0;
        } else {
            self.interp_t = 0.0;
        }
        for (i, w) in state.wheels.iter().enumerate().take(4) {
            self.wheels[i] = WheelVisual {
                steer: if w.steer.is_finite() { w.steer.clamp(-0.9, 0.9) } else { 0.0 },
                spin: if w.spin.is_finite() { w.spin } else { 0.0 },
                suspension: if w.suspension.is_finite() { w.suspension.clamp(0.05, 0.85) } else { 0.30 },
                on_ground: w.on_ground,
            };
        }
    }

    /// Lag-compensated Cubic Hermite Spline interpolation + bounded dead-reckoning extrapolation
    pub fn update(&mut self, dt: f32) {
        if self.full.is_some() {
            self.pos = self.pos.lerp(self.target_pos, (dt * 18.0).min(1.0));
            self.rot = self.rot.slerp(self.target_rot, (dt * 20.0).min(1.0));
            return;
        }
        let window = 0.055f32; // ~55ms interpolation buffer for 30Hz network tick
        self.interp_t += dt / window;
        if self.interp_t <= 1.0 {
            let t = self.interp_t.clamp(0.0, 1.0);
            let t2 = t * t;
            let t3 = t2 * t;
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            self.pos = self.prev_pos * h00
                + self.prev_vel * (window * h10)
                + self.target_pos * h01
                + self.target_vel * (window * h11);
            self.rot = self.prev_rot.slerp(self.target_rot, t);
        } else {
            // Bounded dead-reckoning up to 120ms if a packet is delayed
            let over_t = ((self.interp_t - 1.0) * window).min(0.12);
            self.pos = self.target_pos + self.target_vel * over_t;
            self.rot = self.target_rot;
        }
        for w in self.wheels.iter_mut() {
            w.spin += w.spin * dt * 0.0; // spin is updated from omega
        }
    }
}
