use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::vehicle_state::{VehicleSpawn, VehicleState, WorldCarState};

pub const MAX_MAP_BYTES: usize = 512 * 1024; // 512 KB security cap for custom map JSON
pub const MAX_MAP_OBJECTS: usize = 2500;

/// Validates a client VehicleState or VehicleSpawn for NaN/Inf, speed hacks, and out-of-bounds coordinates.
pub fn validate_vehicle_state(st: &VehicleState) -> Result<(), &'static str> {
    for &x in &st.pos {
        if !x.is_finite() || x.abs() > 25000.0 {
            return Err("Invalid or out-of-bounds vehicle position");
        }
    }
    for &q in &st.rot {
        if !q.is_finite() || q.abs() > 2.0 {
            return Err("Invalid vehicle rotation quaternion");
        }
    }
    let mut v2 = 0.0f32;
    for &v in &st.vel {
        if !v.is_finite() {
            return Err("Invalid vehicle velocity (NaN/Inf)");
        }
        v2 += v * v;
    }
    // Reject speed hacks exceeding 165 m/s (~594 km/h)
    if v2 > 165.0 * 165.0 {
        return Err("Vehicle velocity exceeds physical server anti-cheat limit");
    }
    if let Some(ref fb) = st.full {
        if fb.node_count > 1200 || fb.nodes_mm.len() > 3600 || fb.beam_plastic.len() > 4000 {
            return Err("Soft-body state exceeds node/beam limit");
        }
    }
    Ok(())
}


/// FNV-1a 64-bit hash for map manifest & payload verification.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Validates a map JSON payload in pure Rust (zero extra crate dependencies) for size, structure, object count, and safe identifiers.
pub fn validate_map_json(json_str: &str) -> Result<(), String> {
    if json_str.len() > MAX_MAP_BYTES {
        return Err(format!("Map exceeds max allowed size ({} > {} bytes)", json_str.len(), MAX_MAP_BYTES));
    }
    if json_str.contains("..") || json_str.contains('\\') {
        return Err("Map contains forbidden path traversal characters".into());
    }
    let trimmed = json_str.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err("Invalid map JSON: must be a JSON object {...}".into());
    }
    if !trimmed.contains("\"objects\"") {
        return Err("Map JSON missing 'objects' array".into());
    }
    let kind_count = trimmed.matches("\"kind\"").count();
    if kind_count > MAX_MAP_OBJECTS {
        return Err(format!("Map has too many objects ({} > {})", kind_count, MAX_MAP_OBJECTS));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Packet {
    Hello {
        name: String,
        version: u32,
        client_id: Uuid,
    },
    RequestSpawn(VehicleSpawn),
    VehicleUpdate(VehicleState),
    WorldCarUpdate(WorldCarState),
    Chat { text: String },
    Disconnect,

    /// Client -> Server request to change time (only accepted if player is OP)
    RequestTimeSet {
        day_hour: f32,
    },
    /// Client -> Server request to update the world map (only accepted if player has `op` or `build allow`)
    RequestWorldEdit {
        map_json: String,
        map_hash: u64,
    },

    Welcome {
        player_id: u32,
        server_name: String,
        map: String,
        max_players: u8,
        tick_rate_ms: u16,
        day_hour: f32,
        weather: String,
        is_op: bool,
        can_build: bool,
    },
    /// Server -> Client authoritative environment sync (time of day, weather, permissions)
    EnvSync {
        day_hour: f32,
        time_cycle_on: bool,
        weather: String,
    },
    /// Server -> Client permission update (`op`/`deop` or `build allow`/`build deny`)
    PermissionUpdate {
        is_op: bool,
        can_build: bool,
    },
    /// Server -> Client authoritative map manifest + full JSON transfer with FNV-1a 64-bit hash verification
    MapTransfer {
        map_name: String,
        map_hash: u64,
        map_json: String,
    },
    PlayerJoined {
        player_id: u32,
        name: String,
    },
    PlayerLeft {
        player_id: u32,
    },
    VehicleList(Vec<VehicleSpawn>),
    VehicleSpawned(VehicleSpawn),
    VehicleDespawned {
        vehicle_id: u32,
    },
    VehicleStates(Vec<VehicleState>),
    WorldCarStates(Vec<WorldCarState>),
    ChatMessage {
        player_id: u32,
        name: String,
        text: String,
    },
    Error {
        message: String,
    },
    Ping {
        client_time_ms: u32,
    },
    Pong {
        client_time_ms: u32,
        server_time_ms: u32,
    },
    StatusRequest,
    StatusResponse {
        name: String,
        description: String,
        map: String,
        players: u8,
        max_players: u8,
        protocol: u32,
    },
    /// One piece of a packet that was too large for a single internet-safe datagram (supports up to 65535 chunks)
    Fragment {
        id: u32,
        idx: u16,
        count: u16,
        data: Vec<u8>,
    },
}

impl Packet {
    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

pub const PROTOCOL_VERSION: u32 = 3;
