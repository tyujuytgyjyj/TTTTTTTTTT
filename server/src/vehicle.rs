use crashdrive_shared::{VehicleSpawn, VehicleState};

#[derive(Debug, Clone)]
pub struct ServerVehicle {
    pub spawn: VehicleSpawn,
    pub last_state: VehicleState,
    pub last_update_ms: u32,
}

impl ServerVehicle {
    pub fn new(spawn: VehicleSpawn) -> Self {
        let mut state = VehicleState::default();
        state.id = spawn.vehicle_id;
        state.owner = spawn.owner;
        state.pos = spawn.pos;
        state.rot = [0.0, 0.0, 0.0, 1.0]; // will be updated

        Self {
            spawn,
            last_state: state,
            last_update_ms: 0,
        }
    }
}
