// Editable level: every object of the map is an entry in a list (position, 3D rotation, scale, collider...).
// Official default map is embedded from assets/env/default_level.json (the user's level.json).
use crate::jbeam::{self, J};
use crate::world::*;
use macroquad::prelude::*;

pub const DEFAULT_LEVEL_JSON: &str = include_str!("../assets/env/default_level.json");
pub const DEFAULT_MAP_NAME: &str = "level.json";
pub const DEFAULT_MAP_TITLE: &str = "West Coast Urban District";

#[derive(Clone, PartialEq, Debug)]
pub enum Kind {
    Apartment,
    School,
    Box,
    Collider,
    Hazard,
    Ramp,
    Tree(u8),
    Rock(u8),
    Manhole,
    Custom(String),
}

impl Kind {
    pub fn name(&self) -> String {
        match self {
            Kind::Apartment => "apartment".into(),
            Kind::School => "school".into(),
            Kind::Box => "box".into(),
            Kind::Collider => "collider".into(),
            Kind::Hazard => "hazard".into(),
            Kind::Ramp => "ramp".into(),
            Kind::Tree(i) => format!("tree{}", i),
            Kind::Rock(i) => format!("rock{}", i),
            Kind::Manhole => "manhole".into(),
            Kind::Custom(s) => s.clone(),
        }
    }
    pub fn parse(s: &str) -> Option<Kind> {
        let s = s.trim();
        if s.is_empty() { return None; }
        Some(match s {
            "apartment" => Kind::Apartment,
            "school" => Kind::School,
            "box" => Kind::Box,
            "collider" => Kind::Collider,
            "hazard" => Kind::Hazard,
            "ramp" => Kind::Ramp,
            "manhole" => Kind::Manhole,
            _ => {
                if let Some(n) = s.strip_prefix("tree") {
                    if let Some(idx) = n.parse().ok().filter(|x| *x < 5) {
                        return Some(Kind::Tree(idx));
                    }
                }
                if let Some(n) = s.strip_prefix("rock") {
                    if let Some(idx) = n.parse().ok().filter(|x| *x < 4) {
                        return Some(Kind::Rock(idx));
                    }
                }
                Kind::Custom(s.to_string())
            }
        })
    }
}

#[derive(Clone, Debug)]
pub struct Obj {
    pub id: u32,
    pub kind: Kind,
    pub name: String,
    pub pos: Vec3,
    pub yaw: f32,   // Y-axis rotation (radians, full 360 deg)
    pub pitch: f32, // X-axis rotation (radians, full 360 deg)
    pub roll: f32,  // Z-axis rotation (radians, full 360 deg)
    pub scale: f32,
    pub size: Vec3,
    pub p2: f32,
    pub color: [f32; 3],
    pub collide: bool,
    pub visible: bool,
}

#[derive(Clone, Debug)]
pub struct Forest { pub on: bool, pub count: u32, pub inner: f32, pub outer: f32, pub seed: u32, pub mask: [bool; 5], pub scale: f32 }

#[derive(Clone)]
pub struct Level { pub objs: Vec<Obj>, pub forest: Forest, pub next: u32, pub dirty: bool, pub forest_dirty: bool }

pub const APT_HALF: Vec3 = Vec3::new(16.4, 10.0, 7.6);
pub const SCHOOL_HALF: Vec3 = Vec3::new(13.5, 6.0, 10.8);

/// Verified collision-free spawn points inside the official level.json map: (pos, yaw_radians)
pub const OFFICIAL_SPAWNS: [([f32; 3], f32); 8] = [
    ([-15.0, 0.0, 25.0], 0.0),      // Player / Slot 0: Central Plaza between School, Concrete Building & Apartments
    ([-2.5, 0.0, -55.0], 0.0),      // Slot 1: South Main Avenue facing North
    ([-8.5, 0.0, -5.0], 0.0),       // Slot 2: School Front Road
    ([-42.0, 0.0, 73.5], 1.57),     // Slot 3: North Fenced Boulevard facing East
    ([25.0, 0.0, 24.0], 0.0),       // Slot 4: East Parking Bay 1
    ([28.5, 0.0, 24.0], 0.0),       // Slot 5: East Parking Bay 2
    ([32.0, 0.0, 24.0], 0.0),       // Slot 6: East Parking Bay 3
    ([35.5, 0.0, 24.0], 0.0),       // Slot 7: East Parking Bay 4
];

pub fn default_custom_half(name: &str) -> (Vec3, bool) {
    let low = name.to_lowercase();
    match name {
        "arabic_temple_diorama" => (vec3(4.04, 2.64, 3.98), true),
        "fence_concrete-_15mb" => (vec3(1.78, 1.24, 0.20), true),
        "gdr_park_lamp" => (vec3(0.59, 3.01, 0.59), true),
        "free_city_road_layout_mockup" => (vec3(222.18, 0.25, 296.84), false),
        "tree_low_poly_lowpoly" => (vec3(5.37, 4.50, 2.61), true),
        "abandoned_city_street_for_games" => (vec3(18.21, 4.05, 22.40), false),
        "old_concrete_building_pack__lowpoly" => (vec3(25.60, 6.74, 8.75), true),
        "abadoned_building__lowpoly" => (vec3(9.84, 6.41, 9.83), true),
        "scania_touring_2026" => (vec3(1.34, 1.92, 7.43), true),
        _ => {
            let is_drivable = low.contains("road") || low.contains("street") || low.contains("ground");
            (vec3(4.0, 3.0, 4.0), !is_drivable)
        }
    }
}

impl Obj {
    pub fn new(id: u32, kind: Kind, pos: Vec3) -> Obj {
        let mut collide = true;
        let (size, p2, color) = match &kind {
            Kind::Box => (vec3(1.5, 0.5, 0.5), 0.0, [0.62, 0.62, 0.60]),
            Kind::Collider => (vec3(3.0, 1.5, 0.5), 0.0, [0.15, 0.95, 0.45]),
            Kind::Hazard => (vec3(1.5, 0.5, 0.35), 0.0, [1.0, 1.0, 1.0]),
            Kind::Ramp => (vec3(3.0, 1.7, 9.0), 3.0, [0.55, 0.50, 0.30]),
            Kind::Custom(name) => {
                let (h, col_flag) = default_custom_half(name);
                collide = col_flag;
                (h, 0.0, [1.0, 1.0, 1.0])
            }
            _ => (Vec3::ONE, 0.0, [1.0, 1.0, 1.0]),
        };
        Obj { id, name: format!("{} {}", kind.name(), id), kind, pos, yaw: 0.0, pitch: 0.0, roll: 0.0, scale: 1.0, size, p2, color, collide, visible: true }
    }

    #[inline]
    pub fn rot_quat(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, self.roll)
    }

    /// oriented bounding box used for picking: centre, half extents (local)
    pub fn pick_box(&self) -> (Vec3, Vec3) {
        let s = self.scale;
        let q = self.rot_quat();
        match self.kind {
            Kind::Apartment => (self.pos + q * vec3(0.0, 10.0 * s, 0.0), APT_HALF * s),
            Kind::School => (self.pos + q * vec3(0.0, 6.0 * s, 0.0), SCHOOL_HALF * s),
            Kind::Box | Kind::Collider | Kind::Hazard => (self.pos, self.size * s),
            Kind::Ramp => (self.pos + q * vec3(0.0, self.size.y * 0.5, 0.0), vec3(self.size.x, self.size.y * 0.5 + 0.2, (self.size.z + self.p2 + self.size.y / 0.9) * 0.5)),
            Kind::Tree(_) => (self.pos + q * vec3(0.0, 9.0 * s, 0.0), vec3(5.0 * s, 9.0 * s, 5.0 * s)),
            Kind::Rock(_) => (self.pos + q * vec3(0.0, 15.0 * s, 0.0), vec3(25.0 * s, 15.0 * s, 25.0 * s)),
            Kind::Manhole => (self.pos + q * vec3(0.0, 0.05, 0.0), vec3(0.5, 0.1, 0.5) * s),
            Kind::Custom(_) => {
                let h = self.size.max(Vec3::splat(0.20)) * s;
                (self.pos + q * vec3(0.0, h.y, 0.0), h)
            }
        }
    }
}

/// نقطة البداية رقم i: من الخريطة الحقيقية إن وُجدت، وإلا النقاط الرسمية.
pub fn spawn(i: usize) -> ([f32; 3], f32) {
    if let Some(s) = crate::world::terrain_spawn(i) { return s; }
    OFFICIAL_SPAWNS[i % OFFICIAL_SPAWNS.len()]
}

impl Level {
    pub fn add(&mut self, kind: Kind, pos: Vec3) -> u32 {
        let id = self.next; self.next += 1;
        self.objs.push(Obj::new(id, kind, pos)); self.dirty = true; id
    }
    pub fn get(&self, id: u32) -> Option<&Obj> { self.objs.iter().find(|o| o.id == id) }
    pub fn get_mut(&mut self, id: u32) -> Option<&mut Obj> { self.dirty = true; self.objs.iter_mut().find(|o| o.id == id) }
    pub fn remove(&mut self, id: u32) { self.objs.retain(|o| o.id != id); self.dirty = true; }
    pub fn duplicate(&mut self, id: u32) -> Option<u32> {
        let mut o = self.get(id)?.clone(); o.id = self.next; self.next += 1; o.pos += vec3(2.0, 0.0, 2.0); o.name = format!("{} {}", o.kind.name(), o.id);
        let nid = o.id; self.objs.push(o); self.dirty = true; Some(nid)
    }

    /// Loads the official default level (`level.json`), checking disk paths first and falling back to the embedded `DEFAULT_LEVEL_JSON`.
    /// Loads Map 1 (0 = Official Default), Map 2 (1 = West Coast USA), Map 3 (2 = Metropolis Megacity / City Sample), or Studio (3 = Lumberyard Bistro Studio).
    pub fn load_by_id(map_id: u8) -> Level {
        let map_name = match map_id { 1 => "west_coast", 2 => "masuostt", _ => "" };
        crate::world::set_map_dir(map_name);
        crate::world::set_water_level(-1.0e9);
        crate::world::set_terrain(if map_name.is_empty() { None } else { crate::world::load_terrain_map(map_name) });
        let candidates: &[&str] = match map_id {
            1 => &["scenes/west_coast_usa.json", "client/scenes/west_coast_usa.json", "server/maps/west_coast_usa.json"],
            2 => &["scenes/masuostt.json", "client/scenes/masuostt.json", "scenes/city_sample_megacity.json", "client/scenes/city_sample_megacity.json", "server/maps/city_sample_megacity.json"],
            3 => &["scenes/lumberyard_bistro_studio.json", "client/scenes/lumberyard_bistro_studio.json"],
            _ => &["scenes/level.json", "client/scenes/level.json", "server/maps/default.json"],
        };
        for path in candidates {
            if let Ok(txt) = std::fs::read_to_string(path) {
                if let Some(mut l) = Self::from_json(&txt) {
                    l.dirty = true;
                    l.forest_dirty = true;
                    return l;
                }
            }
        }
        Self::load_official_or_disk()
    }

    pub fn load_official_or_disk() -> Level {
        for path in ["scenes/level.json", "client/scenes/level.json", "server/maps/default.json"] {
            if let Ok(txt) = std::fs::read_to_string(path) {
                if let Some(l) = Self::from_json(&txt) {
                    return l;
                }
            }
        }
        Self::default_level()
    }

    pub fn default_level() -> Level {
        if let Some(mut l) = Self::from_json(DEFAULT_LEVEL_JSON) {
            l.dirty = true;
            l.forest_dirty = true;
            return l;
        }
        Level { objs: vec![], forest: Forest { on: false, count: 2600, inner: 390.0, outer: 1040.0, seed: 7, mask: [true; 5], scale: 1.0 }, next: 1, dirty: true, forest_dirty: true }
    }

    pub fn world(&self) -> World {
        let mut boxes = Vec::new();
        let mut ramps = Vec::new();
        // Outer arena boundary walls so cars stay within the 400m x 400m map
        let tr = crate::world::terrain_active();
        let a = if tr { crate::world::terrain_hf().map(|t| t.half_extent() - 3.0).unwrap_or(1020.0) } else { 200.0f32 };
        let (wy, wh) = if tr { (200.0f32, 200.0f32) } else { (1.0f32, 1.0f32) }; // جدران عالية فوق التضاريس
        boxes.push(BoxObs { c: vec3(0.0, wy, a + 0.5), h: vec3(a, wh, 0.5), yaw: 0.0 });
        boxes.push(BoxObs { c: vec3(0.0, wy, -a - 0.5), h: vec3(a, wh, 0.5), yaw: 0.0 });
        boxes.push(BoxObs { c: vec3(a + 0.5, wy, 0.0), h: vec3(0.5, wh, a), yaw: 0.0 });
        boxes.push(BoxObs { c: vec3(-a - 0.5, wy, 0.0), h: vec3(0.5, wh, a), yaw: 0.0 });

        for o in self.objs.iter().filter(|o| o.collide && (o.visible || o.kind == Kind::Collider)) {
            match o.kind {
                Kind::Box | Kind::Collider | Kind::Hazard => boxes.push(BoxObs { c: o.pos, h: o.size * o.scale, yaw: o.yaw }),
                Kind::Apartment => { let (c, h) = o.pick_box(); boxes.push(BoxObs { c, h: vec3(h.x, h.y, h.z * 0.95), yaw: o.yaw }); }
                Kind::School => { let (c, h) = o.pick_box(); boxes.push(BoxObs { c, h, yaw: o.yaw }); }
                Kind::Custom(_) => {
                    let (c, h) = o.pick_box();
                    if h.y > 0.4 { boxes.push(BoxObs { c, h, yaw: o.yaw }); }
                }
                Kind::Ramp => ramps.push(Ramp { c: vec2(o.pos.x, o.pos.z), half_w: o.size.x, up_len: o.size.z, top_len: o.p2, height: o.size.y, yaw: o.yaw }),
                _ => {}
            }
        }
        let n = boxes.len();
        if tr {
            if let Some(t) = crate::world::terrain_hf() { boxes.extend(t.colliders.iter().cloned()); }
        }
        let grid = BoxGrid::build(&boxes);
        World { vis_boxes: 0, boxes, ramps, arena: a, grid }.with_count(n)
    }

    // ---------------- save / load (JSON text)
    pub fn to_json(&self) -> String {
        let mut s = String::from("{\n  \"forest\": ");
        let f = &self.forest;
        s += &format!("{{\"on\": {}, \"count\": {}, \"inner\": {}, \"outer\": {}, \"seed\": {}, \"scale\": {}, \"mask\": [{}]}},\n  \"objects\": [\n", f.on, f.count, f.inner, f.outer, f.seed, f.scale, f.mask.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","));
        for (i, o) in self.objs.iter().enumerate() {
            s += &format!("    {{\"kind\":\"{}\",\"name\":\"{}\",\"pos\":[{},{},{}],\"yaw\":{},\"pitch\":{},\"roll\":{},\"scale\":{},\"size\":[{},{},{}],\"p2\":{},\"color\":[{},{},{}],\"collide\":{},\"visible\":{}}}{}\n",
                o.kind.name(), o.name.replace('"', "'"), o.pos.x, o.pos.y, o.pos.z, o.yaw, o.pitch, o.roll, o.scale, o.size.x, o.size.y, o.size.z, o.p2, o.color[0], o.color[1], o.color[2], o.collide, o.visible, if i + 1 < self.objs.len() { "," } else { "" });
        }
        s += "  ]\n}\n"; s
    }
    pub fn from_json(text: &str) -> Option<Level> {
        let j = jbeam::parse(text).ok()?;
        let num = |x: Option<&J>, d: f64| x.and_then(|v| v.num()).unwrap_or(d);
        let flag = |x: Option<&J>, d: bool| match x { Some(J::Bool(b)) => *b, _ => d };
        let mut l = Level { objs: vec![], forest: Forest { on: false, count: 2600, inner: 390.0, outer: 1040.0, seed: 7, mask: [true; 5], scale: 1.0 }, next: 1, dirty: true, forest_dirty: true };
        if let Some(f) = j.get("forest") {
            l.forest.on = flag(f.get("on"), false); l.forest.count = num(f.get("count"), 2600.0) as u32; l.forest.inner = num(f.get("inner"), 390.0).max(390.0) as f32; l.forest.outer = num(f.get("outer"), 1040.0).max(1040.0) as f32; l.forest.seed = num(f.get("seed"), 7.0) as u32; l.forest.scale = num(f.get("scale"), 1.0) as f32;
            if let Some(m) = f.get("mask").and_then(|m| m.arr()) { for (i, b) in m.iter().enumerate().take(5) { l.forest.mask[i] = matches!(b, J::Bool(true)); } }
        }
        for o in j.get("objects")?.arr()? {
            let kind = Kind::parse(o.get("kind")?.str()?)?;
            let v3 = |k: &str, d: Vec3| o.get(k).and_then(|a| a.arr()).map(|a| vec3(num(a.get(0), d.x as f64) as f32, num(a.get(1), d.y as f64) as f32, num(a.get(2), d.z as f64) as f32)).unwrap_or(d);
            let id = l.next; l.next += 1;
            let mut ob = Obj::new(id, kind, v3("pos", Vec3::ZERO));
            ob.name = o.get_str("name").unwrap_or(ob.name);
            ob.yaw = num(o.get("yaw"), 0.0) as f32;
            ob.pitch = num(o.get("pitch"), 0.0) as f32;
            ob.roll = num(o.get("roll"), 0.0) as f32;
            ob.scale = num(o.get("scale"), 1.0) as f32; ob.size = v3("size", ob.size); ob.p2 = num(o.get("p2"), ob.p2 as f64) as f32;
            let c = v3("color", vec3(ob.color[0], ob.color[1], ob.color[2])); ob.color = [c.x, c.y, c.z];
            ob.collide = flag(o.get("collide"), ob.collide); ob.visible = flag(o.get("visible"), true);
            l.objs.push(ob);
        }
        Some(l)
    }
}

trait WithCount { fn with_count(self, n: usize) -> Self; }
impl WithCount for World { fn with_count(mut self, _n: usize) -> Self { self.vis_boxes = 0; self } }
