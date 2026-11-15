#![allow(dead_code, unused_imports, unused_variables, unused_mut, unreachable_code, non_snake_case, clippy::all)]
mod player;
mod vehicle;
mod world;

use anyhow::Result;
use crashdrive_shared::{fnv1a_64, validate_vehicle_state, Packet, Reassembler, ServerConfig, PROTOCOL_VERSION};
use log::{debug, error, info, warn};
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use world::World;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Load or create server.properties (or server.json fallback)
    let config = if let Ok(txt) = std::fs::read_to_string("server.properties") {
        ServerConfig::from_properties(&txt)

    } else {
        let cfg = ServerConfig::default();
        let _ = std::fs::write("server.properties", cfg.to_properties());
        cfg
    };

    let addr = format!("0.0.0.0:{}", config.port);
    let socket = Arc::new(UdpSocket::bind(&addr).await?);
    let world = Arc::new(World::new(config.clone()));

    world.log_line(&format!(
        "=== CrashNG Drive Dedicated Server (Authoritative Mode) listening on UDP {} ===",
        addr
    ));
    world.log_line(&format!(
        "Name: '{}' | Map: '{}' (hash={:016x}) | MaxPlayers: {} | Whitelist: {}",
        config.name,
        *world.map_name.lock().unwrap(),
        *world.map_hash.lock().unwrap(),
        config.max_players,
        config.whitelist_enabled
    ));
    world.log_line("Type 'help' in this console for admin commands (op, build allow, time set, map set, kick, ban, save, stop...).");

    let (tx, mut rx) = mpsc::unbounded_channel::<(SocketAddr, Packet)>();

    // Outbound sender task
    {
        let socket = socket.clone();
        tokio::spawn(async move {
            while let Some((addr, packet)) = rx.recv().await {
                match packet.encode_datagrams() {
                    Ok(datagrams) => {
                        for bytes in datagrams {
                            if let Err(e) = socket.send_to(&bytes, addr).await {
                                warn!("Failed to send to {}: {}", addr, e);
                            }
                        }
                    }
                    Err(e) => error!("Encode error: {}", e),
                }
            }
        });
    }

    // Authoritative Environment (Time & Weather) tick + Idle player reaper
    {
        let world = world.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(std::time::Duration::from_secs(2));
            loop {
                iv.tick().await;
                let (day_hour, time_cycle_on, weather) = world.step_environment(2.0);
                world.broadcast(
                    Packet::EnvSync {
                        day_hour,
                        time_cycle_on,
                        weather,
                    },
                    &tx,
                );
                for addr in world.idle_players(20) {
                    if let Some(player) = world.remove_player(addr) {
                        world.log_line(&format!("Player {} (#{}) timed out", player.name, player.id));
                        world.broadcast(Packet::PlayerLeft { player_id: player.id }, &tx);
                        for vid in world.vehicles_of(player.id) {
                            world.despawn_vehicle(vid);
                            world.broadcast(Packet::VehicleDespawned { vehicle_id: vid }, &tx);
                        }
                    }
                }
            }
        });
    }

    // Minecraft-style Interactive Terminal Console on a blocking stdin thread
    {
        let world = world.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines().flatten() {
                let cmd = line.trim();
                if cmd.is_empty() {
                    continue;
                }
                if handle_console_command(cmd, &world, &tx) {
                    world.log_line("Saving world and shutting down server...");
                    world.save_map_to_disk();
                    world.persist_acl();
                    std::process::exit(0);
                }
            }
        });
    }

    let mut buf = vec![0u8; 65535];
    let mut reasm = Reassembler::new();

    loop {
        let (len, addr) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                debug!("recv error: {}", e);
                continue;
            }
        };
        let data = &buf[..len];

        let packet = match reasm.feed(addr, data) {
            Some(p) => p,
            None => continue,
        };

        world.touch(addr);
        handle_packet(packet, addr, &world, &tx).await;
    }
}

/// Returns true if the server should stop (`stop` command).
fn handle_console_command(
    line: &str,
    world: &Arc<World>,
    tx: &mpsc::UnboundedSender<(SocketAddr, Packet)>,
) -> bool {
    let line = line.trim_start_matches('/');
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return false;
    }
    match parts[0].to_lowercase().as_str() {
        "help" | "?" => {
            println!("──────────────── CrashNG Dedicated Server Console Commands ────────────────");
            println!("  op <player>                 Grant full operator (admin + build + time) rights");
            println!("  deop <player>               Revoke operator rights");
            println!("  build allow <player>        Allow player to edit & save the world map (F10)");
            println!("  build deny <player>         Revoke world-building permission from player");
            println!("  time set <day|night|sunset|sunrise|0..24>   Set authoritative server time");
            println!("  time cycle <on|off>         Enable/disable 24-minute daylight progression");
            println!("  weather set <clear|wet|rain|fog>            Set authoritative weather & road grip");
            println!("  map list                    List all available maps on the server");
            println!("  map set <name>              Switch server map and push it to all connected players");
            println!("  kick <player> [reason]      Disconnect a player immediately");
            println!("  ban <player>                Ban a player and disconnect them");
            println!("  unban <player>              Remove a player from the ban list");
            println!("  whitelist <on|off|add|remove|list> [player] Manage server whitelist");
            println!("  say <message>               Broadcast a server announcement to all players");
            println!("  list                        List connected players, IDs, and permissions");
            println!("  save                        Save current world map & permissions to disk");
            println!("  stop                        Save everything and shut down the server");
            println!("───────────────────────────────────────────────────────────────────────────");
        }
        "op" => {
            if let Some(&target) = parts.get(1) {
                let updated = world.set_op(target, true);
                world.log_line(&format!("[ADMIN] Opped player '{}'", target));
                if let Some(p) = updated {
                    let _ = tx.send((
                        p.addr,
                        Packet::PermissionUpdate {
                            is_op: p.is_op,
                            can_build: p.can_build,
                        },
                    ));
                    let _ = tx.send((
                        p.addr,
                        Packet::ChatMessage {
                            player_id: 0,
                            name: "SERVER".into(),
                            text: "You are now an Operator (OP) and can edit the world (F10).".into(),
                        },
                    ));
                }
            } else {
                println!("Usage: op <player>");
            }
        }
        "deop" => {
            if let Some(&target) = parts.get(1) {
                let updated = world.set_op(target, false);
                world.log_line(&format!("[ADMIN] De-opped player '{}'", target));
                if let Some(p) = updated {
                    let _ = tx.send((
                        p.addr,
                        Packet::PermissionUpdate {
                            is_op: p.is_op,
                            can_build: p.can_build,
                        },
                    ));
                    let _ = tx.send((
                        p.addr,
                        Packet::ChatMessage {
                            player_id: 0,
                            name: "SERVER".into(),
                            text: "Your Operator (OP) status was revoked.".into(),
                        },
                    ));
                }
            } else {
                println!("Usage: deop <player>");
            }
        }
        "build" => {
            if parts.len() >= 3 {
                let allow = parts[1].eq_ignore_ascii_case("allow");
                let target = parts[2];
                let updated = world.set_build(target, allow);
                world.log_line(&format!(
                    "[ADMIN] World build permission for '{}' set to {}",
                    target, allow
                ));
                if let Some(p) = updated {
                    let _ = tx.send((
                        p.addr,
                        Packet::PermissionUpdate {
                            is_op: p.is_op,
                            can_build: p.can_build,
                        },
                    ));
                    let _ = tx.send((
                        p.addr,
                        Packet::ChatMessage {
                            player_id: 0,
                            name: "SERVER".into(),
                            text: if allow {
                                "Build permission GRANTED! Press F10 to edit the world.".into()
                            } else {
                                "Build permission REVOKED by server admin.".into()
                            },
                        },
                    ));
                }
            } else {
                println!("Usage: build <allow|deny> <player>");
            }
        }
        "time" => {
            if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("set") {
                let val = match parts[2].to_lowercase().as_str() {
                    "day" | "noon" => Some(12.5f32),
                    "sunset" | "golden" | "dusk" => Some(17.65f32),
                    "sunrise" | "dawn" => Some(6.25f32),
                    "night" | "midnight" => Some(0.5f32),
                    other => other.parse::<f32>().ok().map(|v| v.rem_euclid(24.0)),
                };
                if let Some(h) = val {
                    *world.day_hour.lock().unwrap() = h;
                    let cycle = *world.time_cycle_on.lock().unwrap();
                    let w = world.weather.lock().unwrap().clone();
                    world.broadcast(
                        Packet::EnvSync {
                            day_hour: h,
                            time_cycle_on: cycle,
                            weather: w,
                        },
                        tx,
                    );
                    world.log_line(&format!("[TIME] Set server time to {:02}:{:02} ({:.2}h)", (h as u32) % 24, ((h.fract() * 60.0) as u32) % 60, h));
                } else {
                    println!("Usage: time set <day|night|sunset|sunrise|0..24>");
                }
            } else if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("cycle") {
                let on = matches!(parts[2].to_lowercase().as_str(), "on" | "true" | "1");
                *world.time_cycle_on.lock().unwrap() = on;
                world.config.lock().unwrap().time_cycle_on = on;
                world.persist_acl();
                let h = *world.day_hour.lock().unwrap();
                let w = world.weather.lock().unwrap().clone();
                world.broadcast(
                    Packet::EnvSync {
                        day_hour: h,
                        time_cycle_on: on,
                        weather: w,
                    },
                    tx,
                );
                world.log_line(&format!("[TIME] Daylight cycle set to {}", on));
            } else {
                println!("Usage: time set <day|night|sunset|sunrise|0..24> | time cycle <on|off>");
            }
        }
        "weather" => {
            if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("set") {
                let w = parts[2].to_lowercase();
                *world.weather.lock().unwrap() = w.clone();
                world.config.lock().unwrap().weather = w.clone();
                world.persist_acl();
                let h = *world.day_hour.lock().unwrap();
                let cycle = *world.time_cycle_on.lock().unwrap();
                world.broadcast(
                    Packet::EnvSync {
                        day_hour: h,
                        time_cycle_on: cycle,
                        weather: w.clone(),
                    },
                    tx,
                );
                world.log_line(&format!("[WEATHER] Weather set to '{}'", w));
            } else {
                println!("Usage: weather set <clear|wet|rain|fog>");
            }
        }
        "map" => {
            if parts.get(1).map(|s| s.eq_ignore_ascii_case("list")).unwrap_or(false) {
                let maps = World::list_available_maps();
                let cur = world.map_name.lock().unwrap().clone();
                println!("Available maps (active = '{}'): {}", cur, maps.join(", "));
            } else if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("set") {
                match world.set_map(parts[2]) {
                    Ok((mname, mhash, mjson)) => {
                        world.log_line(&format!(
                            "[MAP] Switched active map to '{}' (hash={:016x}, {} bytes) and broadcasting to all clients",
                            mname, mhash, mjson.len()
                        ));
                        world.broadcast(
                            Packet::MapTransfer {
                                map_name: mname,
                                map_hash: mhash,
                                map_json: mjson,
                            },
                            tx,
                        );
                    }
                    Err(e) => println!("Error setting map: {}", e),
                }
            } else {
                println!("Usage: map list | map set <name>");
            }
        }
        "kick" => {
            if let Some(&target) = parts.get(1) {
                let reason = if parts.len() > 2 {
                    parts[2..].join(" ")
                } else {
                    "Kicked by admin".into()
                };
                if let Some(p) = world.find_player_by_name(target) {
                    let _ = tx.send((
                        p.addr,
                        Packet::Error {
                            message: format!("Kicked: {}", reason),
                        },
                    ));
                    world.remove_player(p.addr);
                    world.broadcast(Packet::PlayerLeft { player_id: p.id }, tx);
                    for vid in world.vehicles_of(p.id) {
                        world.despawn_vehicle(vid);
                        world.broadcast(Packet::VehicleDespawned { vehicle_id: vid }, tx);
                    }
                    world.log_line(&format!("[MOD] Kicked {} ({})", p.name, reason));
                } else {
                    println!("Player '{}' not found online", target);
                }
            } else {
                println!("Usage: kick <player> [reason]");
            }
        }
        "ban" => {
            if let Some(&target) = parts.get(1) {
                world.banned.lock().unwrap().insert(target.to_lowercase());
                world.persist_acl();
                if let Some(p) = world.find_player_by_name(target) {
                    let _ = tx.send((
                        p.addr,
                        Packet::Error {
                            message: "You have been banned from this server.".into(),
                        },
                    ));
                    world.remove_player(p.addr);
                    world.broadcast(Packet::PlayerLeft { player_id: p.id }, tx);
                    for vid in world.vehicles_of(p.id) {
                        world.despawn_vehicle(vid);
                        world.broadcast(Packet::VehicleDespawned { vehicle_id: vid }, tx);
                    }
                }
                world.log_line(&format!("[MOD] Banned player '{}'", target));
            } else {
                println!("Usage: ban <player>");
            }
        }
        "unban" => {
            if let Some(&target) = parts.get(1) {
                world.banned.lock().unwrap().remove(&target.to_lowercase());
                world.persist_acl();
                world.log_line(&format!("[MOD] Unbanned player '{}'", target));
            } else {
                println!("Usage: unban <player>");
            }
        }
        "whitelist" => {
            match parts.get(1).map(|s| s.to_lowercase()).as_deref() {
                Some("on") => {
                    world.config.lock().unwrap().whitelist_enabled = true;
                    world.persist_acl();
                    world.log_line("[WHITELIST] Whitelist enabled");
                }
                Some("off") => {
                    world.config.lock().unwrap().whitelist_enabled = false;
                    world.persist_acl();
                    world.log_line("[WHITELIST] Whitelist disabled");
                }
                Some("add") if parts.len() >= 3 => {
                    world.whitelist.lock().unwrap().insert(parts[2].to_lowercase());
                    world.persist_acl();
                    world.log_line(&format!("[WHITELIST] Added '{}'", parts[2]));
                }
                Some("remove") if parts.len() >= 3 => {
                    world.whitelist.lock().unwrap().remove(&parts[2].to_lowercase());
                    world.persist_acl();
                    world.log_line(&format!("[WHITELIST] Removed '{}'", parts[2]));
                }
                Some("list") => {
                    let wl: Vec<String> = world.whitelist.lock().unwrap().iter().cloned().collect();
                    println!("Whitelisted players: {:?}", wl);
                }
                _ => println!("Usage: whitelist <on|off|add|remove|list> [player]"),
            }
        }
        "say" => {
            if parts.len() >= 2 {
                let msg = parts[1..].join(" ");
                world.log_line(&format!("[SERVER] {}", msg));
                world.broadcast(
                    Packet::ChatMessage {
                        player_id: 0,
                        name: "SERVER".into(),
                        text: msg,
                    },
                    tx,
                );
            } else {
                println!("Usage: say <message>");
            }
        }
        "list" => {
            let players = world.all_players();
            println!("Online players ({}/{}):", players.len(), world.config().max_players);
            for p in players {
                println!(
                    "  #{} '{}' @ {} [op={}, build={}]",
                    p.id, p.name, p.addr, p.is_op, p.can_build
                );
            }
        }
        "save" => {
            world.save_map_to_disk();
            world.persist_acl();
            world.log_line("[SAVE] Saved world map and server configuration to disk.");
        }
        "stop" | "exit" | "quit" => return true,
        other => println!("Unknown command '{}'. Type 'help' for a list of commands.", other),
    }
    let _ = std::io::stdout().flush();
    false
}

async fn handle_packet(
    packet: Packet,
    addr: SocketAddr,
    world: &Arc<World>,
    tx: &mpsc::UnboundedSender<(SocketAddr, Packet)>,
) {
    match packet {
        Packet::Hello {
            name,
            version,
            client_id,
        } => {
            if version != PROTOCOL_VERSION {
                let _ = tx.send((
                    addr,
                    Packet::Error {
                        message: format!(
                            "Protocol mismatch: server={}, client={}",
                            PROTOCOL_VERSION, version
                        ),
                    },
                ));
                return;
            }

            let cfg = world.config();
            let day_hour = *world.day_hour.lock().unwrap();
            let weather = world.weather.lock().unwrap().clone();
            let map_name = world.map_name.lock().unwrap().clone();
            let map_hash = *world.map_hash.lock().unwrap();
            let map_json = world.map_json.lock().unwrap().clone();

            if let Some(existing) = world.player_by_addr(addr) {
                let _ = tx.send((
                    addr,
                    Packet::Welcome {
                        player_id: existing.id,
                        server_name: cfg.name.clone(),
                        map: map_name.clone(),
                        max_players: cfg.max_players,
                        tick_rate_ms: cfg.tick_rate_ms,
                        day_hour,
                        weather,
                        is_op: existing.is_op,
                        can_build: existing.can_build,
                    },
                ));
                let _ = tx.send((
                    addr,
                    Packet::MapTransfer {
                        map_name,
                        map_hash,
                        map_json,
                    },
                ));
                return;
            }

            match world.add_player(addr, name.clone(), client_id) {
                Ok(player) => {
                    world.log_line(&format!(
                        "Player '{}' (#{}) joined from {} [op={}, build={}]",
                        player.name, player.id, addr, player.is_op, player.can_build
                    ));

                    let _ = tx.send((
                        addr,
                        Packet::Welcome {
                            player_id: player.id,
                            server_name: cfg.name.clone(),
                            map: map_name.clone(),
                            max_players: cfg.max_players,
                            tick_rate_ms: cfg.tick_rate_ms,
                            day_hour,
                            weather,
                            is_op: player.is_op,
                            can_build: player.can_build,
                        },
                    ));

                    // Send authoritative map manifest + JSON payload
                    let _ = tx.send((
                        addr,
                        Packet::MapTransfer {
                            map_name,
                            map_hash,
                            map_json,
                        },
                    ));

                    world.broadcast_except(
                        player.id,
                        Packet::PlayerJoined {
                            player_id: player.id,
                            name: name.clone(),
                        },
                        tx,
                    );

                    let vehicles = world.vehicle_spawns();
                    if !vehicles.is_empty() {
                        let _ = tx.send((addr, Packet::VehicleList(vehicles)));
                    }
                }
                Err(e) => {
                    let _ = tx.send((addr, Packet::Error { message: e }));
                }
            }
        }

        Packet::RequestTimeSet { day_hour } => {
            if let Some(player) = world.player_by_addr(addr) {
                if !player.is_op {
                    let _ = tx.send((
                        addr,
                        Packet::ChatMessage {
                            player_id: 0,
                            name: "SERVER".into(),
                            text: "Denied: Only server Operators (OP) can change the server time.".into(),
                        },
                    ));
                    return;
                }
                let h = day_hour.rem_euclid(24.0);
                *world.day_hour.lock().unwrap() = h;
                let cycle = *world.time_cycle_on.lock().unwrap();
                let weather = world.weather.lock().unwrap().clone();
                world.broadcast(
                    Packet::EnvSync {
                        day_hour: h,
                        time_cycle_on: cycle,
                        weather,
                    },
                    tx,
                );
            }
        }

        Packet::RequestWorldEdit { map_json, map_hash } => {
            if let Some(player) = world.player_by_addr(addr) {
                if !player.can_build && !player.is_op {
                    let _ = tx.send((
                        addr,
                        Packet::ChatMessage {
                            player_id: 0,
                            name: "SERVER".into(),
                            text: "Denied: You do not have permission to edit the world. Ask admin to run: build allow <name>".into(),
                        },
                    ));
                    return;
                }
                let calc_hash = fnv1a_64(map_json.as_bytes());
                if calc_hash != map_hash {
                    let _ = tx.send((
                        addr,
                        Packet::ChatMessage {
                            player_id: 0,
                            name: "SERVER".into(),
                            text: "World edit rejected: Map hash mismatch.".into(),
                        },
                    ));
                    return;
                }
                match world.apply_world_edit(map_json.clone()) {
                    Ok(new_hash) => {
                        let map_name = world.map_name.lock().unwrap().clone();
                        world.log_line(&format!(
                            "[WORLD EDIT] Player '{}' (#{}) updated map '{}' (hash={:016x}, {} bytes) — saved & synced",
                            player.name, player.id, map_name, new_hash, map_json.len()
                        ));
                        world.broadcast(
                            Packet::MapTransfer {
                                map_name,
                                map_hash: new_hash,
                                map_json,
                            },
                            tx,
                        );
                    }
                    Err(e) => {
                        let _ = tx.send((
                            addr,
                            Packet::ChatMessage {
                                player_id: 0,
                                name: "SERVER".into(),
                                text: format!("World edit rejected: {}", e),
                            },
                        ));
                    }
                }
            }
        }

        Packet::RequestSpawn(spawn) => {
            if let Some(player) = world.player_by_addr(addr) {
                match world.spawn_vehicle(player.id, spawn) {
                    Ok(spawned) => {
                        info!("Vehicle {} spawned by player {}", spawned.vehicle_id, player.id);
                        world.broadcast(Packet::VehicleSpawned(spawned), tx);
                    }
                    Err(e) => {
                        let _ = tx.send((addr, Packet::Error { message: e }));
                    }
                }
            }
        }

        Packet::VehicleUpdate(state) => {
            if let Some(player) = world.player_by_addr(addr) {
                if state.owner != player.id {
                    return;
                }
                if validate_vehicle_state(&state).is_err() {
                    return;
                }
                world.update_vehicle(state.clone());
                world.broadcast_except(player.id, Packet::VehicleStates(vec![state]), tx);
            }
        }

        Packet::Chat { text } => {
            if let Some(player) = world.player_by_addr(addr) {
                let clean: String = text.chars().filter(|c| !c.is_control()).take(140).collect();
                let clean = clean.trim().to_string();
                if clean.is_empty() { return; }
                let msg = Packet::ChatMessage {
                    player_id: player.id,
                    name: player.name.clone(),
                    text: clean,
                };
                world.broadcast(msg, tx);
            }
        }

        Packet::WorldCarUpdate(state) => {
            if let Some(player) = world.player_by_addr(addr) {
                let mut s = state;
                s.authority = player.id;
                world.broadcast_except(player.id, Packet::WorldCarStates(vec![s]), tx);
            }
        }

        Packet::Disconnect => {
            if let Some(player) = world.remove_player(addr) {
                world.log_line(&format!("Player '{}' (#{}) disconnected", player.name, player.id));
                world.broadcast(Packet::PlayerLeft { player_id: player.id }, tx);
                for vid in world.vehicles_of(player.id) {
                    world.despawn_vehicle(vid);
                    world.broadcast(Packet::VehicleDespawned { vehicle_id: vid }, tx);
                }
            }
        }

        Packet::Ping { client_time_ms } => {
            let server_time = world.now_ms();
            let _ = tx.send((
                addr,
                Packet::Pong {
                    client_time_ms,
                    server_time_ms: server_time,
                },
            ));
        }

        Packet::StatusRequest => {
            let cfg = world.config();
            let map_name = world.map_name.lock().unwrap().clone();
            let _ = tx.send((
                addr,
                Packet::StatusResponse {
                    name: cfg.name.clone(),
                    description: cfg.description.clone(),
                    map: map_name,
                    players: world.player_count().min(255) as u8,
                    max_players: cfg.max_players,
                    protocol: PROTOCOL_VERSION,
                },
            ));
        }

        _ => {}
    }
}
