//! Saved servers (Minecraft-style list), user & graphics settings (Low/Medium/High, Dynamic Resolution, FPS Cap),
//! and server status pinging.

use crashdrive_shared::Packet;
use std::net::{ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 30814;

#[derive(Clone, Debug)]
pub struct BrandingConfig {
    pub game_title: String,
    pub game_title_prefix: String,
    pub game_title_suffix: String,
    pub subtitle: String,
    pub version: String,
    pub studio: String,
}

impl Default for BrandingConfig {
    fn default() -> Self {
        Self {
            game_title: "APEX.drive".into(),
            game_title_prefix: "APEX.".into(),
            game_title_suffix: "drive".into(),
            subtitle: "Production Soft-Body Vehicle Physics & Driving Simulation".into(),
            version: "1.0.0".into(),
            studio: "Cosmos Studio Engineering".into(),
        }
    }
}

pub fn load_branding() -> BrandingConfig {
    let mut b = BrandingConfig::default();
    for path in ["branding.json", "client/branding.json"] {
        if let Ok(txt) = std::fs::read_to_string(path) {
            if let Ok(j) = crate::jbeam::parse(&txt) {
                if let Some(s) = j.get_str("game_title") { b.game_title = s; }
                if let Some(s) = j.get_str("game_title_prefix") { b.game_title_prefix = s; }
                if let Some(s) = j.get_str("game_title_suffix") { b.game_title_suffix = s; }
                if let Some(s) = j.get_str("subtitle") { b.subtitle = s; }
                if let Some(s) = j.get_str("version") { b.version = s; }
                if let Some(s) = j.get_str("studio") { b.studio = s; }
                break;
            }
        }
    }
    b
}


#[derive(Clone, Debug, PartialEq)]
pub struct ServerEntry {
    pub name: String,
    pub address: String,
}

#[derive(Clone, Debug)]
pub enum Status {
    Pending,
    Online { ping_ms: u32, name: String, description: String, map: String, players: u8, max_players: u8, known: bool },
    Offline(String),
}

pub type SharedStatus = Arc<Mutex<Status>>;

#[derive(Clone, Debug)]
pub struct Settings {
    pub name: String,
    pub music_vol: f32,
    pub music_on: bool,
    pub favs: Vec<String>,
    pub recent: Vec<String>,
    /// Graphics quality preset: 0 = Low (Default for weak PCs!), 1 = Medium, 2 = High
    pub gfx_preset: u8,
    /// Render resolution scale (0.50 .. 1.00)
    pub res_scale: f32,
    /// Dynamic Resolution Scaling enabled (automatically adjusts res_scale between 0.55 and 1.0 to maintain target FPS)
    pub dyn_res: bool,
    /// FPS target for Dynamic Resolution (30, 60, 120; 0 = use 60).
    /// Ogre swapchain VSync controls actual frame presentation.
    pub fps_cap: u32,
    /// Planar shadow map pass enabled
    pub shadows_on: bool,
    /// Volumetric FBM clouds enabled
    pub clouds_on: bool,
    /// Anti-Aliasing & Upscaling mode: 0 = Off, 1 = SMAA, 2 = SMAA + TAA + CAS Upscaling
    pub aa_mode: u8,
    /// Forza Horizon-style Cockpit Camera FOV in degrees (45..80, default 58)
    pub cockpit_fov: f32,
    /// Cockpit head G-force inertia & vibration intensity (0.0..1.5, default 1.0)
    pub head_motion: f32,
}

impl Default for Settings {
    fn default() -> Self {
        // Default preset is Low (0) to ensure high FPS out-of-the-box on weak PCs (Task 1.3)
        Settings {
            name: "Player".into(),
            music_vol: 0.6,
            music_on: true,
            favs: Vec::new(),
            recent: Vec::new(),
            gfx_preset: 2,
            res_scale: 1.0,
            dyn_res: false,
            fps_cap: 60,
            shadows_on: true,
            clouds_on: true,
            aa_mode: 2,
            cockpit_fov: 58.0,
            head_motion: 1.0,
        }
    }
}

impl Settings {
    pub fn apply_preset(&mut self, preset: u8) {
        self.gfx_preset = preset.min(2);
        match self.gfx_preset {
            0 => {
                // Low (Weak PC Friendly)
                self.res_scale = 0.75;
                self.dyn_res = true;
                self.shadows_on = false;
                self.clouds_on = false;
                crate::envload::set_max_tex_dim(512);
            }
            1 => {
                // Medium
                self.res_scale = 0.90;
                self.dyn_res = true;
                self.shadows_on = true;
                self.clouds_on = true;
                crate::envload::set_max_tex_dim(1024);
            }
            _ => {
                // High (Ultra)
                self.res_scale = 1.00;
                self.dyn_res = false;
                self.shadows_on = true;
                self.clouds_on = true;
                crate::envload::set_max_tex_dim(2048);
            }
        }
    }
}

fn data_file(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join(name);
        }
    }
    PathBuf::from(name)
}

pub fn sanitize_name(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(max).collect()
}

pub fn normalize_address(input: &str) -> Result<String, String> {
    let mut s = input.trim().to_string();
    for scheme in ["crashdrive://", "crashng://", "udp://", "http://", "https://"] {
        if s.to_lowercase().starts_with(scheme) {
            s = s[scheme.len()..].to_string();
        }
    }
    let s = s.trim_end_matches('/').trim();
    if s.is_empty() {
        return Err("Enter an IP address or a domain name".into());
    }
    if s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("The address cannot contain spaces".into());
    }
    let (host, port): (String, u16) = if let Some(rest) = s.strip_prefix('[') {
        let end = rest.find(']').ok_or("Missing ']' in IPv6 address")?;
        let host = &rest[..end];
        let tail = &rest[end + 1..];
        let port = if tail.is_empty() {
            DEFAULT_PORT
        } else if let Some(p) = tail.strip_prefix(':') {
            p.parse::<u16>().map_err(|_| "Invalid port number".to_string())?
        } else {
            return Err("Invalid address".into());
        };
        (format!("[{}]", host), port)
    } else {
        match s.matches(':').count() {
            0 => (s.to_string(), DEFAULT_PORT),
            1 => {
                let (h, p) = s.split_once(':').unwrap();
                let port = p.parse::<u16>().map_err(|_| "Invalid port number".to_string())?;
                (h.to_string(), port)
            }
            _ => return Err("For IPv6 use the form [address]:port".into()),
        }
    };
    if port == 0 {
        return Err("Port must be between 1 and 65535".into());
    }
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare.is_empty() || bare.len() > 253 {
        return Err("Invalid host name".into());
    }
    if !bare.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == ':') {
        return Err("The host name contains invalid characters".into());
    }
    if bare.starts_with('.') || bare.ends_with('.') || bare.contains("..") {
        return Err("Invalid host name".into());
    }
    Ok(format!("{}:{}", host, port))
}

pub fn load_servers() -> Vec<ServerEntry> {
    let path = data_file("servers.txt");
    match std::fs::read_to_string(&path) {
        Ok(txt) => parse_servers(&txt),
        Err(_) => vec![ServerEntry { name: "APEX Official Dedicated Server".into(), address: format!("127.0.0.1:{}", DEFAULT_PORT) }],
    }
}

pub fn parse_servers(txt: &str) -> Vec<ServerEntry> {
    let mut v = Vec::new();
    for line in txt.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((n, a)) = line.split_once('\t') {
            if let Ok(addr) = normalize_address(a) {
                let name = sanitize_name(n, 40);
                v.push(ServerEntry { name: if name.is_empty() { addr.clone() } else { name }, address: addr });
            }
        }
    }
    v
}

pub fn serialize_servers(list: &[ServerEntry]) -> String {
    let mut out = String::from("# CrashNG Drive - saved servers (name<TAB>address)\n");
    for s in list {
        out.push_str(&format!("{}\t{}\n", s.name.replace('\t', " "), s.address));
    }
    out
}

pub fn save_servers(list: &[ServerEntry]) {
    let _ = std::fs::write(data_file("servers.txt"), serialize_servers(list));
}

pub fn load_settings() -> Settings {
    let mut st = Settings::default();
    if let Ok(txt) = std::fs::read_to_string(data_file("settings.txt")) {
        for line in txt.lines() {
            if let Some((k, v)) = line.split_once('=') {
                match k.trim() {
                    "name" => {
                        let n = sanitize_name(v, 20);
                        if !n.is_empty() { st.name = n; }
                    }
                    "music_vol" => { if let Ok(f) = v.trim().parse::<f32>() { st.music_vol = f.clamp(0.0, 1.0); } }
                    "music_on" => st.music_on = v.trim() != "0",
                    "favs" => st.favs = split_list(v),
                    "recent" => st.recent = split_list(v),
                    "gfx_preset" => if let Ok(p) = v.trim().parse::<u8>() { st.gfx_preset = p.min(2); },
                    "res_scale" => if let Ok(r) = v.trim().parse::<f32>() { st.res_scale = r.clamp(0.50, 1.00); },
                    "dyn_res" => st.dyn_res = v.trim() != "0",
                    "fps_cap" => if let Ok(c) = v.trim().parse::<u32>() { st.fps_cap = c; },
                    "shadows_on" => st.shadows_on = v.trim() != "0",
                    "clouds_on" => st.clouds_on = v.trim() != "0",
                    "aa_mode" => if let Ok(a) = v.trim().parse::<u8>() { st.aa_mode = a.min(2); },
                    "cockpit_fov" => if let Ok(f) = v.trim().parse::<f32>() { st.cockpit_fov = f.clamp(45.0, 80.0); },
                    "head_motion" => if let Ok(m) = v.trim().parse::<f32>() { st.head_motion = m.clamp(0.0, 1.5); },
                    _ => {}
                }
            }
        }
    }
    crate::envload::set_max_tex_dim(match st.gfx_preset { 0 => 512, 1 => 1024, _ => 2048 });
    st
}

fn split_list(v: &str) -> Vec<String> {
    v.split('|').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

fn join_list(l: &[String]) -> String {
    l.iter().map(|x| x.replace('|', " ")).collect::<Vec<_>>().join("|")
}

pub fn save_settings(st: &Settings) {
    let txt = format!(
        "name={}\nmusic_vol={:.2}\nmusic_on={}\nfavs={}\nrecent={}\ngfx_preset={}\nres_scale={:.2}\ndyn_res={}\nfps_cap={}\nshadows_on={}\nclouds_on={}\naa_mode={}\ncockpit_fov={:.1}\nhead_motion={:.2}\n",
        st.name,
        st.music_vol,
        if st.music_on { 1 } else { 0 },
        join_list(&st.favs),
        join_list(&st.recent),
        st.gfx_preset,
        st.res_scale,
        if st.dyn_res { 1 } else { 0 },
        st.fps_cap,
        if st.shadows_on { 1 } else { 0 },
        if st.clouds_on { 1 } else { 0 },
        st.aa_mode,
        st.cockpit_fov,
        st.head_motion
    );
    let _ = std::fs::write(data_file("settings.txt"), txt);
}

pub fn query_status(address: String, out: SharedStatus) {
    *out.lock().unwrap() = Status::Pending;
    std::thread::spawn(move || {
        let res = query_blocking(&address);
        *out.lock().unwrap() = res;
    });
}

fn query_blocking(address: &str) -> Status {
    let addr = match address.to_socket_addrs() {
        Ok(it) => {
            let all: Vec<_> = it.collect();
            match all.iter().copied().find(|a| a.is_ipv4()).or_else(|| all.first().copied()) {
                Some(a) => a,
                None => return Status::Offline("Unknown host".into()),
            }
        }
        Err(_) => return Status::Offline("Unknown host".into()),
    };
    let sock = match UdpSocket::bind(if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }) {
        Ok(s) => s,
        Err(_) => return Status::Offline("Network unavailable".into()),
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(250)));
    let start = Instant::now();
    let ping = Packet::Ping { client_time_ms: 0 }.encode().unwrap_or_default();
    let req = Packet::StatusRequest.encode().unwrap_or_default();
    let _ = sock.send_to(&ping, addr);
    let _ = sock.send_to(&req, addr);

    let mut ping_ms: Option<u32> = None;
    let mut info: Option<(String, String, String, u8, u8)> = None;
    let mut buf = vec![0u8; 4096];
    let deadline = Duration::from_millis(3000);
    let mut grace: Option<Instant> = None;
    while start.elapsed() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) if from.port() != addr.port() => { let _ = n; }
            Ok((n, _)) => match Packet::decode(&buf[..n]) {
                Ok(Packet::Pong { .. }) => {
                    if ping_ms.is_none() {
                        ping_ms = Some(start.elapsed().as_millis().max(1) as u32);
                        grace = Some(Instant::now());
                    }
                }
                Ok(Packet::StatusResponse { name, description, map, players, max_players, .. }) => {
                    info = Some((name, description, map, players, max_players));
                }
                _ => {}
            },
            Err(_) => {
                if ping_ms.is_none() && start.elapsed() > Duration::from_millis(900) && start.elapsed() < Duration::from_millis(1200) {
                    let _ = sock.send_to(&ping, addr);
                    let _ = sock.send_to(&req, addr);
                }
            }
        }
        if info.is_some() && ping_ms.is_some() {
            break;
        }
        if let Some(g) = grace {
            if g.elapsed() > Duration::from_millis(600) {
                break;
            }
        }
    }
    match (ping_ms, info) {
        (Some(ms), Some((name, description, map, players, max_players))) =>
            Status::Online { ping_ms: ms, name, description, map, players, max_players, known: true },
        (Some(ms), None) =>
            Status::Online { ping_ms: ms, name: address.to_string(), description: String::new(), map: String::new(), players: 0, max_players: 0, known: false },
        _ => Status::Offline("No response".into()),
    }
}
