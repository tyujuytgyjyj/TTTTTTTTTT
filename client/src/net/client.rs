//! Network client – background tokio thread + SharedNet for the game loop.
//! Supports Server-Authoritative time/weather, permission checks (`op` / `can_build`),
//! and secure custom map transfers with FNV-1a 64-bit hash verification.

use crashdrive_shared::{
    fnv1a_64, validate_map_json, Packet, Reassembler, VehicleSpawn, VehicleState, WorldCarState,
    PROTOCOL_VERSION,
};
use log::{error, info, warn};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::remote_vehicle::RemoteVehicle;

#[derive(Clone, Debug)]
pub enum NetStatus {
    Disconnected,
    Connecting,
    Connected { player_id: u32, server_name: String },
    Error(String),
}

#[derive(Clone, Debug)]
pub struct AuthEnv {
    pub day_hour: f32,
    pub time_cycle_on: bool,
    pub weather: String,
    pub is_op: bool,
    pub can_build: bool,
}

impl Default for AuthEnv {
    fn default() -> Self {
        Self {
            day_hour: 17.5,
            time_cycle_on: true,
            weather: "clear".into(),
            is_op: false,
            can_build: false,
        }
    }
}

pub struct SharedNet {
    pub status: Mutex<NetStatus>,
    pub remote_vehicles: Mutex<HashMap<u32, RemoteVehicle>>,
    pub players: Mutex<HashMap<u32, String>>,
    pub chat: Mutex<Vec<(String, String)>>,
    pub my_vehicle_id: Mutex<Option<u32>>,
    pub world_cars: Mutex<HashMap<u16, WorldCarState>>,
    pub auth_env: Mutex<AuthEnv>,
    /// Newly received authoritative map `(map_name, map_hash, map_json)` waiting to be applied by the game loop
    pub incoming_map: Mutex<Option<(String, u64, String)>>,
    outgoing: mpsc::UnboundedSender<Packet>,
}

impl SharedNet {
    pub fn status(&self) -> NetStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn is_connected(&self) -> bool {
        matches!(*self.status.lock().unwrap(), NetStatus::Connected { .. })
    }

    pub fn player_id(&self) -> Option<u32> {
        match *self.status.lock().unwrap() {
            NetStatus::Connected { player_id, .. } => Some(player_id),
            _ => None,
        }
    }

    pub fn can_build(&self) -> bool {
        let env = self.auth_env.lock().unwrap();
        env.is_op || env.can_build
    }

    pub fn is_op(&self) -> bool {
        self.auth_env.lock().unwrap().is_op
    }

    pub fn send(&self, packet: Packet) {
        let _ = self.outgoing.send(packet);
    }

    pub fn send_vehicle_state(&self, state: VehicleState) {
        self.send(Packet::VehicleUpdate(state));
    }

    pub fn send_world_car(&self, state: WorldCarState) {
        self.send(Packet::WorldCarUpdate(state));
    }

    pub fn request_spawn(&self, spawn: VehicleSpawn) {
        self.send(Packet::RequestSpawn(spawn));
    }

    pub fn request_time_set(&self, day_hour: f32) {
        self.send(Packet::RequestTimeSet { day_hour });
    }

    pub fn request_world_edit(&self, map_json: String) {
        let map_hash = fnv1a_64(map_json.as_bytes());
        self.send(Packet::RequestWorldEdit { map_json, map_hash });
    }

    pub fn chat(&self, text: &str) {
        self.send(Packet::Chat { text: text.to_string() });
    }

    pub fn disconnect(&self) {
        self.send(Packet::Disconnect);
        *self.status.lock().unwrap() = NetStatus::Disconnected;
    }

    pub fn snapshot_remotes(&self) -> Vec<RemoteVehicle> {
        self.remote_vehicles.lock().unwrap().values().cloned().collect()
    }

    pub fn update_remotes(&self, dt: f32) {
        for v in self.remote_vehicles.lock().unwrap().values_mut() {
            v.update(dt);
        }
    }
}

pub fn start_net_client(server: &str, name: &str) -> Arc<SharedNet> {
    let (tx, rx) = mpsc::unbounded_channel::<Packet>();
    let shared = Arc::new(SharedNet {
        status: Mutex::new(NetStatus::Connecting),
        remote_vehicles: Mutex::new(HashMap::new()),
        players: Mutex::new(HashMap::new()),
        chat: Mutex::new(Vec::new()),
        my_vehicle_id: Mutex::new(None),
        world_cars: Mutex::new(HashMap::new()),
        auth_env: Mutex::new(AuthEnv::default()),
        incoming_map: Mutex::new(None),
        outgoing: tx,
    });
    let shared2 = shared.clone();
    let shared_err = shared.clone();
    let server = server.to_string();
    let name = name.to_string();
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio rt");
        rt.block_on(async move {
            if let Err(e) = run_client(&server, &name, shared2, rx).await {
                error!("Net client exited: {}", e);
                let mut st = shared_err.status.lock().unwrap();
                if !matches!(*st, NetStatus::Disconnected) {
                    *st = NetStatus::Error("Connection timed out or server unreachable".into());
                }
            }
        });
    });
    shared
}

async fn run_client(
    server: &str,
    name: &str,
    shared: Arc<SharedNet>,
    mut outgoing_rx: mpsc::UnboundedReceiver<Packet>,
) -> anyhow::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let server_addr: SocketAddr = {
        let all: Vec<SocketAddr> = tokio::net::lookup_host(server)
            .await
            .map_err(|_| anyhow::anyhow!("unknown host '{}'", server))?
            .collect();
        all.iter().copied().find(|a| a.is_ipv4()).or_else(|| all.first().copied())
            .ok_or_else(|| anyhow::anyhow!("unknown host '{}'", server))?
    };
    info!("Connecting to {} ({})", server, server_addr);
    let socket = Arc::new(socket);
    let client_id = Uuid::new_v4();

    let hello = Packet::Hello {
        name: name.to_string(),
        version: PROTOCOL_VERSION,
        client_id,
    };
    let hello_bytes = hello.encode()?;
    let _ = socket.send_to(&hello_bytes, server_addr).await;

    {
        let socket = socket.clone();
        let shared = shared.clone();
        let hello_bytes = hello_bytes.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            for _ in 0..24 {
                interval.tick().await;
                if !matches!(shared.status(), NetStatus::Connecting) {
                    break;
                }
                let _ = socket.send_to(&hello_bytes, server_addr).await;
            }
        });
    }

    {
        let socket = socket.clone();
        tokio::spawn(async move {
            while let Some(packet) = outgoing_rx.recv().await {
                if let Ok(datagrams) = packet.encode_datagrams() {
                    for bytes in datagrams {
                        let _ = socket.send_to(&bytes, server_addr).await;
                    }
                }
            }
        });
    }

    {
        let socket = socket.clone();
        let start = Instant::now();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            loop {
                interval.tick().await;
                let ping = Packet::Ping {
                    client_time_ms: start.elapsed().as_millis() as u32,
                };
                if let Ok(bytes) = ping.encode() {
                    let _ = socket.send_to(&bytes, server_addr).await;
                }
            }
        });
    }

    let mut buf = vec![0u8; 65535];
    let mut reasm = Reassembler::new();
    loop {
        match tokio::time::timeout(Duration::from_millis(500), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, from))) => {
                if from.port() == server_addr.port() {
                    if let Some(packet) = reasm.feed(from, &buf[..len]) {
                        handle_packet(packet, &shared);
                    }
                }
            }
            Ok(Err(e)) if matches!(e.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionRefused) => {}
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => {}
        }
        if matches!(shared.status(), NetStatus::Disconnected) {
            return Ok(());
        }
    }
}

fn handle_packet(packet: Packet, shared: &SharedNet) {
    match packet {
        Packet::Welcome {
            player_id,
            server_name,
            map,
            max_players,
            tick_rate_ms,
            day_hour,
            weather,
            is_op,
            can_build,
        } => {
            info!(
                "Joined '{}' map={} max={} tick={} -> id={} [op={}, build={}]",
                server_name, map, max_players, tick_rate_ms, player_id, is_op, can_build
            );
            {
                let mut env = shared.auth_env.lock().unwrap();
                env.day_hour = day_hour;
                env.weather = weather;
                env.is_op = is_op;
                env.can_build = can_build;
            }
            *shared.status.lock().unwrap() = NetStatus::Connected {
                player_id,
                server_name,
            };
        }
        Packet::EnvSync {
            day_hour,
            time_cycle_on,
            weather,
        } => {
            let mut env = shared.auth_env.lock().unwrap();
            env.day_hour = day_hour;
            env.time_cycle_on = time_cycle_on;
            env.weather = weather;
        }
        Packet::PermissionUpdate { is_op, can_build } => {
            let mut env = shared.auth_env.lock().unwrap();
            env.is_op = is_op;
            env.can_build = can_build;
        }
        Packet::MapTransfer {
            map_name,
            map_hash,
            map_json,
        } => {
            let calc_hash = fnv1a_64(map_json.as_bytes());
            if calc_hash != map_hash {
                warn!("Rejected server map '{}': FNV-1a hash mismatch ({:016x} != {:016x})", map_name, calc_hash, map_hash);
                return;
            }
            if let Err(e) = validate_map_json(&map_json) {
                warn!("Rejected server map '{}': security check failed ({})", map_name, e);
                return;
            }
            let _ = std::fs::create_dir_all("scenes");
            let cache_path = format!("scenes/server_cache_{:016x}.json", map_hash);
            let _ = std::fs::write(&cache_path, &map_json);
            *shared.incoming_map.lock().unwrap() = Some((map_name, map_hash, map_json));
        }
        Packet::PlayerJoined { player_id, name } => {
            info!("Player joined: {} ({})", name, player_id);
            shared.players.lock().unwrap().insert(player_id, name);
        }
        Packet::PlayerLeft { player_id } => {
            info!("Player left: {}", player_id);
            shared.players.lock().unwrap().remove(&player_id);
            shared.remote_vehicles.lock().unwrap().retain(|_, v| v.owner != player_id);
        }
        Packet::VehicleList(list) => {
            let mut map = shared.remote_vehicles.lock().unwrap();
            for spawn in list {
                if let Some(pid) = shared.player_id() {
                    if spawn.owner == pid {
                        *shared.my_vehicle_id.lock().unwrap() = Some(spawn.vehicle_id);
                        continue;
                    }
                }
                map.insert(spawn.vehicle_id, RemoteVehicle::from_spawn(&spawn));
            }
        }
        Packet::VehicleSpawned(spawn) => {
            if let Some(pid) = shared.player_id() {
                if spawn.owner == pid {
                    *shared.my_vehicle_id.lock().unwrap() = Some(spawn.vehicle_id);
                    return;
                }
            }
            shared.remote_vehicles.lock().unwrap()
                .insert(spawn.vehicle_id, RemoteVehicle::from_spawn(&spawn));
        }
        Packet::VehicleDespawned { vehicle_id } => {
            shared.remote_vehicles.lock().unwrap().remove(&vehicle_id);
            let mut mine = shared.my_vehicle_id.lock().unwrap();
            if *mine == Some(vehicle_id) { *mine = None; }
        }
        Packet::VehicleStates(states) => {
            let my_id = *shared.my_vehicle_id.lock().unwrap();
            let mut map = shared.remote_vehicles.lock().unwrap();
            for state in states {
                if Some(state.id) == my_id { continue; }
                if let Some(v) = map.get_mut(&state.id) {
                    v.apply_state(&state);
                }
            }
        }
        Packet::WorldCarStates(states) => {
            let mut map = shared.world_cars.lock().unwrap();
            for st in states {
                let insert = match map.get(&st.slot) {
                    None => true,
                    Some(old) => st.seq >= old.seq,
                };
                if insert {
                    map.insert(st.slot, st);
                }
            }
        }
        Packet::ChatMessage { name, text, .. } => {
            info!("[CHAT] {}: {}", name, text);
            let mut chat = shared.chat.lock().unwrap();
            chat.push((name, text));
            if chat.len() > 40 { chat.remove(0); }
        }
        Packet::Error { message } => {
            error!("Server error: {}", message);
            *shared.status.lock().unwrap() = NetStatus::Error(message);
        }
        Packet::Pong { .. } => {}
        _ => {}
    }
}
