use crashdrive_shared::{
    fnv1a_64, validate_map_json, Packet, ServerConfig, VehicleSpawn, VehicleState,
};
use dashmap::DashMap;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::player::Player;
use crate::vehicle::ServerVehicle;

const FALLBACK_DEFAULT_MAP: &str = include_str!("../../client/assets/env/default_level.json");

pub struct World {
    pub config: Mutex<ServerConfig>,
    players: DashMap<SocketAddr, Player>,
    players_by_id: DashMap<u32, SocketAddr>,
    last_seen: DashMap<SocketAddr, Instant>,
    vehicles: DashMap<u32, ServerVehicle>,
    next_player_id: AtomicU32,
    next_vehicle_id: AtomicU32,
    start: Instant,
    // Authoritative world state
    pub map_name: Mutex<String>,
    pub map_json: Mutex<String>,
    pub map_hash: Mutex<u64>,
    pub day_hour: Mutex<f32>,
    pub time_cycle_on: Mutex<bool>,
    pub weather: Mutex<String>,
    // Permissions & moderation sets (persisted to disk)
    pub ops: Mutex<HashSet<String>>,
    pub builders: Mutex<HashSet<String>>,
    pub banned: Mutex<HashSet<String>>,
    pub whitelist: Mutex<HashSet<String>>,
}

impl World {
    pub fn new(config: ServerConfig) -> Self {
        let _ = fs::create_dir_all("server/maps");
        let _ = fs::create_dir_all("maps");
        let map_name = config.map.clone();
        let map_json = Self::read_map_file(&map_name).unwrap_or_else(|| FALLBACK_DEFAULT_MAP.to_string());
        let map_hash = fnv1a_64(map_json.as_bytes());
        let ops = Self::load_set("ops.txt");
        let builders = Self::load_set("builders.txt");
        let banned = Self::load_set("banned-players.txt");
        let whitelist = Self::load_set("whitelist.txt");

        Self {
            day_hour: Mutex::new(config.initial_time_hour),
            time_cycle_on: Mutex::new(config.time_cycle_on),
            weather: Mutex::new(config.weather.clone()),
            config: Mutex::new(config),
            players: DashMap::new(),
            players_by_id: DashMap::new(),
            last_seen: DashMap::new(),
            vehicles: DashMap::new(),
            next_player_id: AtomicU32::new(1),
            next_vehicle_id: AtomicU32::new(1),
            start: Instant::now(),
            map_name: Mutex::new(map_name),
            map_json: Mutex::new(map_json),
            map_hash: Mutex::new(map_hash),
            ops: Mutex::new(ops),
            builders: Mutex::new(builders),
            banned: Mutex::new(banned),
            whitelist: Mutex::new(whitelist),
        }
    }

    pub fn log_line(&self, msg: &str) {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let line = format!("[{}s] {}\n", secs, msg);
        print!("{}", line);
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("server.log") {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn load_set(filename: &str) -> HashSet<String> {
        fs::read_to_string(filename)
            .unwrap_or_default()
            .lines()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .collect()
    }

    fn save_set(filename: &str, set: &HashSet<String>) {
        let mut list: Vec<&str> = set.iter().map(|s| s.as_str()).collect();
        list.sort();
        let _ = fs::write(filename, list.join("\n") + "\n");
    }

    pub fn persist_acl(&self) {
        Self::save_set("ops.txt", &self.ops.lock().unwrap());
        Self::save_set("builders.txt", &self.builders.lock().unwrap());
        Self::save_set("banned-players.txt", &self.banned.lock().unwrap());
        Self::save_set("whitelist.txt", &self.whitelist.lock().unwrap());
        let cfg = self.config.lock().unwrap().clone();
        let _ = fs::write("server.properties", cfg.to_properties());
    }

    pub fn map_paths(name: &str) -> Vec<PathBuf> {
        let clean = name.trim_end_matches(".json");
        vec![
            PathBuf::from(format!("server/maps/{}.json", clean)),
            PathBuf::from(format!("maps/{}.json", clean)),
            PathBuf::from(format!("scenes/{}.json", clean)),
            PathBuf::from(format!("client/scenes/{}.json", clean)),
            PathBuf::from("scenes/level.json"),
            PathBuf::from("client/scenes/level.json"),
        ]
    }

    pub fn read_map_file(name: &str) -> Option<String> {
        for p in Self::map_paths(name) {
            if let Ok(txt) = fs::read_to_string(&p) {
                if validate_map_json(&txt).is_ok() {
                    return Some(txt);
                }
            }
        }
        None
    }

    pub fn list_available_maps() -> Vec<String> {
        let mut out = vec!["default".to_string(), "level.json".to_string()];
        for dir in ["server/maps", "maps", "scenes", "client/scenes"] {
            if let Ok(entries) = fs::read_dir(dir) {
                for e in entries.flatten() {
                    if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) {
                        let s = stem.to_string();
                        if !out.contains(&s) {
                            out.push(s);
                        }
                    }
                }
            }
        }
        out
    }

    pub fn set_map(&self, name: &str) -> Result<(String, u64, String), String> {
        let clean = name.trim().trim_end_matches(".json");
        let json = if clean.eq_ignore_ascii_case("default") || clean.eq_ignore_ascii_case("level") {
            Self::read_map_file("default").unwrap_or_else(|| FALLBACK_DEFAULT_MAP.to_string())
        } else {
            Self::read_map_file(clean).ok_or_else(|| format!("Map '{}' not found in server/maps/ or scenes/", clean))?
        };
        validate_map_json(&json)?;
        let h = fnv1a_64(json.as_bytes());
        *self.map_name.lock().unwrap() = clean.to_string();
        *self.map_json.lock().unwrap() = json.clone();
        *self.map_hash.lock().unwrap() = h;
        self.config.lock().unwrap().map = clean.to_string();
        self.persist_acl();
        Ok((clean.to_string(), h, json))
    }

    pub fn apply_world_edit(&self, new_json: String) -> Result<u64, String> {
        validate_map_json(&new_json)?;
        let h = fnv1a_64(new_json.as_bytes());
        *self.map_json.lock().unwrap() = new_json;
        *self.map_hash.lock().unwrap() = h;
        self.save_map_to_disk();
        Ok(h)
    }

    pub fn save_map_to_disk(&self) {
        let name = self.map_name.lock().unwrap().clone();
        let clean = name.trim_end_matches(".json");
        let json = self.map_json.lock().unwrap().clone();
        let _ = fs::create_dir_all("server/maps");
        let _ = fs::write(format!("server/maps/{}.json", clean), &json);
        let _ = fs::write("scenes/level.json", &json);
        let _ = fs::write("client/scenes/level.json", &json);
    }

    pub fn step_environment(&self, dt_sec: f32) -> (f32, bool, String) {
        let cycle = *self.time_cycle_on.lock().unwrap();
        let mut h = *self.day_hour.lock().unwrap();
        if cycle {
            // 24-minute real-time day/night cycle (1440s = 24h -> 1 min = 1 game hour)
            h = (h + dt_sec / 60.0) % 24.0;
            *self.day_hour.lock().unwrap() = h;
        }
        let w = self.weather.lock().unwrap().clone();
        (h, cycle, w)
    }

    pub fn touch(&self, addr: SocketAddr) {
        if self.players.contains_key(&addr) {
            self.last_seen.insert(addr, Instant::now());
        }
    }

    pub fn idle_players(&self, secs: u64) -> Vec<SocketAddr> {
        self.players
            .iter()
            .map(|p| *p.key())
            .filter(|a| self.last_seen.get(a).map_or(true, |t| t.elapsed().as_secs() >= secs))
            .collect()
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    pub fn config(&self) -> ServerConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn now_ms(&self) -> u32 {
        self.start.elapsed().as_millis() as u32
    }

    pub fn add_player(&self, addr: SocketAddr, name: String, client_id: Uuid) -> Result<Player, String> {
        let lower = name.trim().to_lowercase();
        if self.banned.lock().unwrap().contains(&lower) {
            return Err("You are banned from this server.".into());
        }
        let cfg = self.config();
        if cfg.whitelist_enabled && !self.whitelist.lock().unwrap().contains(&lower) && !self.ops.lock().unwrap().contains(&lower) {
            return Err("Server is whitelisted. Ask the admin to run: whitelist add <your_name>".into());
        }
        if self.players.len() >= cfg.max_players as usize {
            return Err("Server full".into());
        }
        if self.players.contains_key(&addr) {
            return Err("Already connected".into());
        }

        let is_op = self.ops.lock().unwrap().contains(&lower);
        let can_build = is_op || self.builders.lock().unwrap().contains(&lower);
        let id = self.next_player_id.fetch_add(1, Ordering::SeqCst);
        let player = Player::new(id, name, addr, client_id, is_op, can_build);
        self.players.insert(addr, player.clone());
        self.last_seen.insert(addr, Instant::now());
        self.players_by_id.insert(id, addr);
        Ok(player)
    }

    pub fn remove_player(&self, addr: SocketAddr) -> Option<Player> {
        self.last_seen.remove(&addr);
        if let Some((_, player)) = self.players.remove(&addr) {
            self.players_by_id.remove(&player.id);
            Some(player)
        } else {
            None
        }
    }

    pub fn find_player_by_name(&self, name: &str) -> Option<Player> {
        let target = name.trim().to_lowercase();
        self.players
            .iter()
            .find(|p| p.name.to_lowercase() == target || format!("{}", p.id) == target)
            .map(|p| p.clone())
    }

    pub fn set_op(&self, name: &str, op: bool) -> Option<Player> {
        let lower = name.trim().to_lowercase();
        if op {
            self.ops.lock().unwrap().insert(lower.clone());
            self.builders.lock().unwrap().insert(lower.clone());
        } else {
            self.ops.lock().unwrap().remove(&lower);
        }
        self.persist_acl();
        if let Some(p) = self.find_player_by_name(name) {
            if let Some(mut entry) = self.players.get_mut(&p.addr) {
                entry.is_op = op;
                if op { entry.can_build = true; }
                return Some(entry.clone());
            }
        }
        None
    }

    pub fn set_build(&self, name: &str, allow: bool) -> Option<Player> {
        let lower = name.trim().to_lowercase();
        if allow {
            self.builders.lock().unwrap().insert(lower.clone());
        } else {
            self.builders.lock().unwrap().remove(&lower);
        }
        self.persist_acl();
        if let Some(p) = self.find_player_by_name(name) {
            if let Some(mut entry) = self.players.get_mut(&p.addr) {
                entry.can_build = entry.is_op || allow;
                return Some(entry.clone());
            }
        }
        None
    }

    pub fn player_by_addr(&self, addr: SocketAddr) -> Option<Player> {
        self.players.get(&addr).map(|p| p.clone())
    }

    pub fn player_by_id(&self, id: u32) -> Option<Player> {
        self.players_by_id.get(&id)
            .and_then(|addr| self.players.get(&addr).map(|p| p.clone()))
    }

    pub fn all_players(&self) -> Vec<Player> {
        self.players.iter().map(|p| p.clone()).collect()
    }

    pub fn spawn_vehicle(&self, owner: u32, mut spawn: VehicleSpawn) -> Result<VehicleSpawn, String> {
        let player = self.player_by_id(owner).ok_or("Player not found")?;
        let max_cars = self.config().max_cars_per_player as usize;
        if player.cars.len() >= max_cars {
            // Replace previous vehicle automatically so switching cars in-game works seamlessly
            for old_vid in player.cars.clone() {
                self.despawn_vehicle(old_vid);
            }
        }

        let vid = self.next_vehicle_id.fetch_add(1, Ordering::SeqCst);
        spawn.vehicle_id = vid;
        spawn.owner = owner;

        let vehicle = ServerVehicle::new(spawn.clone());
        self.vehicles.insert(vid, vehicle);

        if let Some(mut p) = self.players.get_mut(&player.addr) {
            p.cars.push(vid);
        }

        Ok(spawn)
    }

    pub fn despawn_vehicle(&self, vehicle_id: u32) {
        if let Some((_, v)) = self.vehicles.remove(&vehicle_id) {
            if let Some(addr) = self.players_by_id.get(&v.spawn.owner) {
                if let Some(mut p) = self.players.get_mut(&addr) {
                    p.cars.retain(|&id| id != vehicle_id);
                }
            }
        }
    }

    pub fn update_vehicle(&self, state: VehicleState) {
        if let Some(mut v) = self.vehicles.get_mut(&state.id) {
            v.last_state = state;
            v.last_update_ms = self.now_ms();
        }
    }

    pub fn vehicle_spawns(&self) -> Vec<VehicleSpawn> {
        self.vehicles.iter().map(|v| v.spawn.clone()).collect()
    }

    pub fn vehicles_of(&self, owner: u32) -> Vec<u32> {
        self.vehicles.iter()
            .filter(|v| v.spawn.owner == owner)
            .map(|v| *v.key())
            .collect()
    }

    pub fn broadcast(&self, packet: Packet, tx: &mpsc::UnboundedSender<(SocketAddr, Packet)>) {
        for entry in self.players.iter() {
            let _ = tx.send((*entry.key(), packet.clone()));
        }
    }

    pub fn broadcast_except(
        &self,
        except_player_id: u32,
        packet: Packet,
        tx: &mpsc::UnboundedSender<(SocketAddr, Packet)>,
    ) {
        for entry in self.players.iter() {
            if entry.id != except_player_id {
                let _ = tx.send((*entry.key(), packet.clone()));
            }
        }
    }
}
