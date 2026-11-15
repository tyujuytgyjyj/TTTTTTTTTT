use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleState {
    pub id: u32,
    pub owner: u32,
    pub pos: [f32; 3],
    pub rot: [f32; 4],
    pub vel: [f32; 3],
    pub ang_vel: [f32; 3],
    pub damage: f32,
    pub wheels: [WheelState; 4],
    /// Full soft-body snapshot (exact damage). Optional on lightweight updates.
    pub full: Option<FullBodyState>,
    pub seq: u32,
    pub client_time_ms: u32,
}

/// Exact soft-body state for true damage replication (no simplification).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullBodyState {
    /// Node positions relative to vehicle center, quantized (mm).
    /// Packed as xyz xyz ... length = node_count * 3
    pub nodes_mm: Vec<i16>,
    /// Plastic beam rest lengths as ratio l0/lorig * 10000 (u16).
    /// Only entries where |ratio-1| > threshold, as (beam_index, qratio)
    pub beam_plastic: Vec<(u16, u16)>,
    pub node_count: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct WheelState {
    pub steer: f32,
    pub spin: f32,
    pub suspension: f32,
    pub on_ground: bool,
}

impl Default for VehicleState {
    fn default() -> Self {
        Self {
            id: 0,
            owner: 0,
            pos: [0.0; 3],
            rot: [0.0, 0.0, 0.0, 1.0],
            vel: [0.0; 3],
            ang_vel: [0.0; 3],
            damage: 0.0,
            wheels: [WheelState::default(); 4],
            full: None,
            seq: 0,
            client_time_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleSpawn {
    pub vehicle_id: u32,
    pub owner: u32,
    pub model: String,
    pub pos: [f32; 3],
    pub rot_y: f32,
    pub color: [f32; 3],
}

/// World/NPC car (parked bots etc.) – same full body when damaged/moved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldCarState {
    /// Stable index in spawn_scene (1.. = bots)
    pub slot: u16,
    pub model_spec: u16,
    pub pos: [f32; 3],
    pub rot: [f32; 4],
    pub vel: [f32; 3],
    pub asleep: bool,
    pub damage: f32,
    pub full: Option<FullBodyState>,
    pub wheels: [WheelState; 4],
    pub seq: u32,
    pub authority: u32, // player id who last simulated this car
}
