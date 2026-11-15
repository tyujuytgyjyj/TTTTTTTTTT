// BeamNG-style mod discovery: zips (or folders) placed in the `mods` directory.
use crate::jbeam;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Debug)]
pub enum ModKind { Car, Map }

#[derive(Clone)]
pub struct ModConfig {
    pub id: String,        // e.g. "facelift_gxl"
    pub name: String,
    pub thumb: Option<Vec<u8>>,
    pub info: String,
    pub playable: bool,
}

#[derive(Clone)]
pub struct ModInfo {
    pub path: PathBuf,
    pub kind: ModKind,
    pub title: String,
    pub author: String,
    pub version: String,
    pub thumb: Option<Vec<u8>>,
    pub model: String,            // car model dir or map dir
    pub configs: Vec<ModConfig>,  // for cars
    pub files: usize,
}

pub fn read_zip_file(path: &Path, name: &str) -> Option<Vec<u8>> {
    let f = std::fs::File::open(path).ok()?;
    let mut z = zip::ZipArchive::new(std::io::BufReader::new(f)).ok()?;
    let mut e = z.by_name(name).ok()?;
    let mut v = Vec::new();
    e.read_to_end(&mut v).ok()?;
    Some(v)
}

fn text(b: &[u8]) -> String { String::from_utf8_lossy(b).into_owned() }

pub fn scan(dir: &Path) -> Vec<ModInfo> {
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(dir) { Ok(r) => r, Err(_) => return out };
    let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.extension().map(|e| e.to_ascii_lowercase() == "zip").unwrap_or(false) {
            if let Some(m) = inspect_zip(&p) { out.push(m); }
        }
    }
    out
}

fn inspect_zip(path: &Path) -> Option<ModInfo> {
    let f = std::fs::File::open(path).ok()?;
    let mut z = zip::ZipArchive::new(std::io::BufReader::new(f)).ok()?;
    let names: Vec<String> = (0..z.len()).filter_map(|i| z.by_index(i).ok().map(|e| e.name().replace('\\', "/"))).collect();
    let files = names.len();
    // kind + model
    let mut model = String::new();
    let mut kind = None;
    for n in &names {
        let parts: Vec<&str> = n.split('/').collect();
        if parts.len() >= 3 && parts[0] == "vehicles" && parts[1] != "common" && n.ends_with(".pc") { kind = Some(ModKind::Car); model = parts[1].to_string(); break; }
        if parts.len() >= 3 && parts[0] == "levels" { kind = Some(ModKind::Map); model = parts[1].to_string(); }
    }
    let kind = kind?;
    // mod_info metadata
    let mut title = model.clone();
    let mut author = String::new();
    let mut version = String::new();
    let mut thumb = None;
    for n in &names {
        if n.starts_with("mod_info/") && n.ends_with("/info.json") {
            drop(z);
            if let Some(b) = read_zip_file(path, n) {
                if let Ok(j) = jbeam::parse(&text(&b)) {
                    if let Some(t) = j.get_str("title") { title = t; }
                    if let Some(t) = j.get_str("username") { author = t; }
                    if let Some(t) = j.get_str("version_string") { version = t; }
                }
            }
            z = zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(path).ok()?)).ok()?;
            break;
        }
    }
    for n in &names {
        if n.starts_with("mod_info/") && n.ends_with("/icon.jpg") { thumb = read_zip_file(path, n); break; }
    }
    drop(z);
    let mut configs = Vec::new();
    if kind == ModKind::Car {
        let pre = format!("vehicles/{}/", model);
        for n in &names {
            if n.starts_with(&pre) && n.ends_with(".pc") && n[pre.len()..].find('/').is_none() {
                let id = n[pre.len()..n.len() - 3].to_string();
                let mut name = id.clone();
                let mut info = String::new();
                if let Some(b) = read_zip_file(path, &format!("{}info_{}.json", pre, id)) {
                    if let Ok(j) = jbeam::parse(&text(&b)) {
                        if let Some(t) = j.get_str("Configuration") { name = t; }
                        if let Some(t) = j.get_str("Description") { info = t; }
                    }
                }
                let th = read_zip_file(path, &format!("{}{}.jpg", pre, id));
                configs.push(ModConfig { id, name, thumb: th, info, playable: true });
            }
        }
        configs.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(b) = read_zip_file(path, &format!("{}info.json", pre)) {
            if let Ok(j) = jbeam::parse(&text(&b)) {
                let brand = j.get_str("Brand").unwrap_or_default();
                let nm = j.get_str("Name").unwrap_or_default();
                if !nm.is_empty() { title = format!("{} {}", brand, nm).trim().to_string(); }
            }
        }
    } else {
        let pre = format!("levels/{}/", model);
        if let Some(b) = read_zip_file(path, &format!("{}info.json", pre)) {
            if let Ok(j) = jbeam::parse(&text(&b)) {
                if let Some(t) = j.get_str("title") { title = t; }
            }
        }
        for n in &names {
            if n.starts_with(&pre) && n.contains("preview") && n.ends_with(".jpg") { thumb = read_zip_file(path, n).or(thumb); break; }
        }
    }
    Some(ModInfo { path: path.to_path_buf(), kind, title, author, version, thumb, model, configs, files })
}
