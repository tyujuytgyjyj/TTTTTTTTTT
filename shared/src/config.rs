use serde::{Deserialize, Serialize};

/// Server configuration (loaded from / saved to `server.properties` or `server.json`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub description: String,
    pub max_players: u8,
    pub max_cars_per_player: u8,
    pub map: String,
    pub port: u16,
    pub private: bool,
    pub whitelist_enabled: bool,
    pub initial_time_hour: f32,
    pub time_cycle_on: bool,
    pub weather: String,
    /// How often clients should send vehicle state (ms)
    pub tick_rate_ms: u16,
    /// Interest radius in meters – cars farther than this are not sent
    pub interest_radius: f32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "CrashNG Official Dedicated Server".into(),
            description: "Server-Authoritative Soft-Body Driving & World Building".into(),
            max_players: 16,
            max_cars_per_player: 2,
            map: "default".into(),
            port: 30814,
            private: false,
            whitelist_enabled: false,
            initial_time_hour: 17.5,
            time_cycle_on: true,
            weather: "clear".into(),
            tick_rate_ms: 33, // ~30 Hz
            interest_radius: 450.0,
        }
    }
}

impl ServerConfig {
    pub fn from_properties(txt: &str) -> Self {
        let mut c = Self::default();
        for line in txt.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                match k {
                    "server-name" | "name" => c.name = v.to_string(),
                    "motd" | "description" => c.description = v.to_string(),
                    "server-port" | "port" => if let Ok(p) = v.parse() { c.port = p; },
                    "max-players" => if let Ok(m) = v.parse() { c.max_players = m; },
                    "max-cars-per-player" => if let Ok(m) = v.parse() { c.max_cars_per_player = m; },
                    "level-name" | "map" => c.map = v.to_string(),
                    "white-list" | "whitelist" => c.whitelist_enabled = v.eq_ignore_ascii_case("true") || v == "1",
                    "initial-time" => if let Ok(t) = v.parse() { c.initial_time_hour = t; },
                    "do-daylight-cycle" => c.time_cycle_on = v.eq_ignore_ascii_case("true") || v == "1",
                    "weather" => c.weather = v.to_string(),
                    "tick-rate-ms" => if let Ok(t) = v.parse() { c.tick_rate_ms = t; },
                    "interest-radius" => if let Ok(r) = v.parse() { c.interest_radius = r; },
                    _ => {}
                }
            }
        }
        c
    }

    pub fn to_properties(&self) -> String {
        format!(
            "# CrashNG Drive Dedicated Server Properties (Minecraft-style)\n\
             server-name={}\n\
             motd={}\n\
             server-port={}\n\
             level-name={}\n\
             max-players={}\n\
             max-cars-per-player={}\n\
             white-list={}\n\
             initial-time={:.2}\n\
             do-daylight-cycle={}\n\
             weather={}\n\
             tick-rate-ms={}\n\
             interest-radius={:.1}\n",
            self.name, self.description, self.port, self.map,
            self.max_players, self.max_cars_per_player, self.whitelist_enabled,
            self.initial_time_hour, self.time_cycle_on, self.weather,
            self.tick_rate_ms, self.interest_radius
        )
    }
}
