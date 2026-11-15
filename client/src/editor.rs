// In-game world editor (toggle with F10): fly camera, pick/move/rotate (full 360 deg on X/Y/Z)/scale objects,
// smart directional array repeater, interactive line-draw placement, clipboard copy/paste (Ctrl+C/Ctrl+V),
// undo batch (Ctrl+Z), forest generator, save/load, and a Python console.
use crate::hood::{rescan_custom_assets, sync_custom_bounds, Assets};
use crate::level::*;
use crate::pyscript::{Host, Interp, V};
use crate::{rrect, txt, txt_w, Res, ACCENT};
use macroquad::prelude::*;

pub struct Editor {
    pub active: bool,
    pub cam_pos: Vec3, yaw: f32, pitch: f32, speed: f32,
    pub sel: Option<u32>,
    drag: Option<(usize, f32, Vec3)>,
    rot_drag: Option<(usize, f32, f32, f32)>, // (axis 0=X/Pitch, 1=Y/Yaw, 2=Z/Roll, prev_mouse_ang, accum_delta, start_angle_rad)
    gizmo_mode: u8, // 0 = Move + Rotate 360, 1 = Move Only, 2 = Rotate 360 Only
    pub day_hour: f32,
    pub array_count: u32,
    pub array_gap: f32,
    pub line_mode: bool,
    pub obj_clip: Option<Obj>,
    pub undo_stack: Vec<Vec<u32>>,
    pub internal_clip: String,
    edit: Option<(String, String)>,
    hscroll: f32,
    add_scroll: f32,
    menu: u8, // 0 none, 1 add, 2 forest
    pub console: bool,
    pub script: String,
    pub log: Vec<String>,
    interp: Interp,
    pub msg: (String, f32),
    pub run_request: Option<String>,
}

fn clipboard_read(fallback: &str) -> String {
    if let Some(s) = macroquad::miniquad::window::clipboard_get() {
        if !s.is_empty() {
            return s.replace("\r\n", "\n").replace('\r', "\n");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(out) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", "Get-Clipboard -Raw"])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).to_string();
                let clean = s.replace("\r\n", "\n").replace('\r', "\n");
                let trimmed = clean.strip_suffix('\n').unwrap_or(&clean).to_string();
                if !trimmed.is_empty() {
                    return trimmed;
                }
            }
        }
    }
    fallback.to_string()
}

fn clipboard_write(text: &str, fallback: &mut String) {
    *fallback = text.to_string();
    macroquad::miniquad::window::clipboard_set(text);
}

pub struct LevelHost<'a> { pub level: &'a mut Level }

impl<'a> Host for LevelHost<'a> {
    fn call(&mut self, name: &str, a: &[V]) -> Option<Result<V, String>> {
        let n = |i: usize| -> Result<f32, String> { a.get(i).ok_or_else(|| format!("{}() is missing argument {}", name, i + 1))?.num().map(|x| x as f32) };
        let idn = |i: usize| -> Result<u32, String> { n(i).map(|x| x as u32) };
        let l = &mut *self.level;
        let r: Result<V, String> = (|| Ok(match name {
            "spawn" => {
                let k = a.get(0).map(|v| v.text()).unwrap_or_default();
                let kind = Kind::parse(&k).ok_or_else(|| format!("unknown kind '{}'", k))?;
                let id = l.add(kind.clone(), vec3(n(1)?, 0.0, n(2)?));
                let o = l.get_mut(id).unwrap();
                if matches!(o.kind, Kind::Box | Kind::Collider | Kind::Hazard) { o.pos.y = o.size.y; }
                if matches!(o.kind, Kind::Manhole) { o.pos.y = 0.03; }
                if a.len() > 3 { o.yaw = n(3)?.to_radians(); }
                if a.len() > 4 { o.scale = n(4)?; }
                V::Int(id as i64)
            }
            "spawn_line" => {
                // spawn_line(kind, x1, z1, x2, z2, gap_or_zero)
                let k = a.get(0).map(|v| v.text()).unwrap_or_default();
                let kind = Kind::parse(&k).ok_or_else(|| format!("unknown kind '{}'", k))?;
                let (x1, z1, x2, z2) = (n(1)?, n(2)?, n(3)?, n(4)?);
                let gap = if a.len() > 5 { n(5)? } else { 0.0 };
                let temp = Obj::new(0, kind.clone(), Vec3::ZERO);
                let span_x = (temp.size.x * 2.0).max(0.5);
                let span_z = (temp.size.z * 2.0).max(0.5);
                let use_x_axis = span_x >= span_z;
                let base_span = (if use_x_axis { span_x } else { span_z } + gap).max(0.5);
                let delta = vec3(x2 - x1, 0.0, z2 - z1);
                let dist = delta.length();
                let dir = if dist > 1e-3 { delta / dist } else { Vec3::X };
                let line_yaw = if use_x_axis {
                    (-dir.z).atan2(dir.x)
                } else {
                    dir.x.atan2(dir.z)
                };
                let count = ((dist / base_span).floor() as i32 + 1).clamp(1, 500);
                let mut ids = Vec::new();
                for i in 0..count {
                    let p = vec3(x1, 0.0, z1) + dir * (i as f32 * base_span);
                    let id = l.add(kind.clone(), p);
                    if let Some(o) = l.get_mut(id) {
                        o.yaw = line_yaw;
                        if matches!(o.kind, Kind::Box | Kind::Collider | Kind::Hazard) { o.pos.y = o.size.y; }
                        if matches!(o.kind, Kind::Manhole) { o.pos.y = 0.03; }
                    }
                    ids.push(V::Int(id as i64));
                }
                V::list(ids)
            }
            "repeat" => {
                // repeat(id, count, gap=0.0, axis='x')
                let id = idn(0)?;
                let count = (n(1)? as i32).clamp(1, 500);
                let gap = if a.len() > 2 { n(2)? } else { 0.0 };
                let ax_str = a.get(3).map(|v| v.text().to_lowercase()).unwrap_or_else(|| "x".to_string());
                let base = l.get(id).ok_or("no such object")?.clone();
                let q = base.rot_quat();
                let (local_dir, half_ext) = match ax_str.as_str() {
                    "-x" | "left" => (-Vec3::X, base.size.x),
                    "z" | "+z" | "fwd" => (Vec3::Z, base.size.z),
                    "-z" | "back" => (-Vec3::Z, base.size.z),
                    "y" | "+y" | "up" => (Vec3::Y, base.size.y),
                    _ => (Vec3::X, base.size.x),
                };
                let step = (half_ext * 2.0 * base.scale + gap).max(0.2);
                let world_step = (q * local_dir) * step;
                let mut ids = Vec::new();
                for i in 1..=count {
                    let mut cp_obj = base.clone();
                    let nid = l.next; l.next += 1;
                    cp_obj.id = nid;
                    cp_obj.pos = base.pos + world_step * (i as f32);
                    cp_obj.name = format!("{} {}", cp_obj.kind.name(), nid);
                    l.objs.push(cp_obj);
                    ids.push(V::Int(nid as i64));
                }
                l.dirty = true;
                V::list(ids)
            }
            "spawn_box" => { let id = l.add(Kind::Box, vec3(n(0)?, n(1)?, n(2)?)); let o = l.get_mut(id).unwrap(); o.size = vec3(n(3)?, n(4)?, n(5)?); if a.len() > 6 { o.yaw = n(6)?.to_radians(); } V::Int(id as i64) }
            "spawn_collider" => {
                let id = l.add(Kind::Collider, vec3(n(0)?, n(1)?, n(2)?));
                let o = l.get_mut(id).unwrap();
                o.size = vec3(n(3)?, n(4)?, n(5)?);
                if a.len() > 6 { o.yaw = n(6)?.to_radians(); }
                if a.len() > 7 { o.collide = a.get(7).map(|v| v.truthy()).unwrap_or(true); }
                V::Int(id as i64)
            }
            "move" => { let id = idn(0)?; let p = vec3(n(1)?, n(2)?, n(3)?); l.get_mut(id).ok_or("no such object")?.pos = p; V::None }
            "rotate" => {
                let id = idn(0)?;
                let y = n(1)?.to_radians();
                let p = if a.len() > 2 { n(2)?.to_radians() } else { 0.0 };
                let r_ang = if a.len() > 3 { n(3)?.to_radians() } else { 0.0 };
                let obj = l.get_mut(id).ok_or("no such object")?;
                obj.yaw = y;
                if a.len() > 2 { obj.pitch = p; }
                if a.len() > 3 { obj.roll = r_ang; }
                V::None
            }
            "scale" => { let id = idn(0)?; let s = n(1)?; l.get_mut(id).ok_or("no such object")?.scale = s; V::None }
            "remove" => { l.remove(idn(0)?); V::None }
            "clear" => { let k = a.get(0).map(|v| v.text()).unwrap_or_default(); l.objs.retain(|o| o.kind.name() != k); l.dirty = true; V::None }
            "color" => { let id = idn(0)?; let c = [n(1)?, n(2)?, n(3)?]; l.get_mut(id).ok_or("no such object")?.color = c; V::None }
            "collide" => { let id = idn(0)?; let b = a.get(1).map(|v| v.truthy()).unwrap_or(true); l.get_mut(id).ok_or("no such object")?.collide = b; V::None }
            "visible" => { let id = idn(0)?; let b = a.get(1).map(|v| v.truthy()).unwrap_or(true); l.get_mut(id).ok_or("no such object")?.visible = b; V::None }
            "pos" => { let o = l.get(idn(0)?).ok_or("no such object")?; V::list(vec![V::Float(o.pos.x as f64), V::Float(o.pos.y as f64), V::Float(o.pos.z as f64)]) }
            "yaw" => V::Float(l.get(idn(0)?).ok_or("no such object")?.yaw.to_degrees() as f64),
            "kind" => V::s(&l.get(idn(0)?).ok_or("no such object")?.kind.name()),
            "find" => { let k = a.get(0).map(|v| v.text()).unwrap_or_default(); V::list(l.objs.iter().filter(|o| o.kind.name() == k).map(|o| V::Int(o.id as i64)).collect()) }
            "ids" => V::list(l.objs.iter().map(|o| V::Int(o.id as i64)).collect()),
            "count" => V::Int(l.objs.len() as i64),
            "forest" => { l.forest.on = true; l.forest.count = n(0)? as u32; if a.len() > 1 { l.forest.inner = n(1)?; } if a.len() > 2 { l.forest.outer = n(2)?; } if a.len() > 3 { l.forest.seed = n(3)? as u32; } l.forest_dirty = true; V::None }
            "forest_off" => { l.forest.on = false; l.forest_dirty = true; V::None }
            _ => return Err(String::from("\u{1}")),
        }))();
        match r { Err(e) if e == "\u{1}" => None, other => Some(other) }
    }
}

fn ray(mx: f32, my: f32, sw: f32, sh: f32, inv: Mat4, cp: Vec3) -> (Vec3, Vec3) {
    let p = inv * vec4(mx / sw * 2.0 - 1.0, 1.0 - my / sh * 2.0, 1.0, 1.0);
    (cp, (p.truncate() / p.w - cp).normalize())
}
fn project(vp: Mat4, p: Vec3, sw: f32, sh: f32) -> Option<Vec2> {
    let c = vp * vec4(p.x, p.y, p.z, 1.0);
    if c.w < 0.05 { return None; }
    Some(vec2((c.x / c.w * 0.5 + 0.5) * sw, (1.0 - (c.y / c.w * 0.5 + 0.5)) * sh))
}
fn ray_obb_quat(o: Vec3, d: Vec3, c: Vec3, h: Vec3, q: Quat) -> Option<f32> {
    let inv_q = q.inverse();
    let (lo, ld) = (inv_q * (o - c), inv_q * d);
    let (mut t0, mut t1) = (0.0f32, 1e9f32);
    for i in 0..3 {
        let (oi, di, hi) = (lo[i], ld[i], h[i]);
        if di.abs() < 1e-8 { if oi.abs() > hi { return None; } continue; }
        let (a, b) = ((-hi - oi) / di, (hi - oi) / di);
        let (a, b) = if a < b { (a, b) } else { (b, a) };
        t0 = t0.max(a); t1 = t1.min(b);
        if t0 > t1 { return None; }
    }
    Some(t0)
}
fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

const RING_SEGS: usize = 44;

fn ring_basis(axis: usize) -> (Vec3, Vec3) {
    match axis {
        0 => (Vec3::Y, Vec3::Z), // X-axis rotation (Pitch) in YZ plane
        1 => (Vec3::X, Vec3::Z), // Y-axis rotation (Yaw) in XZ plane
        _ => (Vec3::X, Vec3::Y), // Z-axis rotation (Roll) in XY plane
    }
}

fn ring_screen_dist(p: Vec2, vp: Mat4, origin: Vec3, radius: f32, axis: usize, sw: f32, sh: f32) -> f32 {
    let (u, v) = ring_basis(axis);
    let mut best = 1e9f32;
    let mut prev: Option<Vec2> = None;
    for i in 0..=RING_SEGS {
        let a = (i as f32 / RING_SEGS as f32) * std::f32::consts::TAU;
        let wp = origin + (u * a.cos() + v * a.sin()) * radius;
        let cur = project(vp, wp, sw, sh);
        if let (Some(p0), Some(p1)) = (prev, cur) {
            best = best.min(seg_dist(p, p0, p1));
        }
        prev = cur;
    }
    best
}

fn wrap_deg_360(rad: f32) -> f32 {
    let mut d = rad.to_degrees() % 360.0;
    if d < 0.0 { d += 360.0; }
    if (d - 360.0).abs() < 1e-3 { 0.0 } else { d }
}

pub fn list_scripts() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("scripts").map(|r| r.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".py")).collect()).unwrap_or_default();
    v.sort(); v
}

impl Editor {
    pub fn new() -> Editor {
        Editor {
            active: false, cam_pos: vec3(0.0, 20.0, -110.0), yaw: 0.0, pitch: -0.3, speed: 35.0,
            sel: None, drag: None, rot_drag: None, gizmo_mode: 0,
            day_hour: 17.35, array_count: 10, array_gap: 0.0, line_mode: false, obj_clip: None, undo_stack: Vec::new(), internal_clip: String::new(),
            edit: None, hscroll: 0.0, add_scroll: 0.0, menu: 0, console: false,
            script: "# Python console - Ctrl+V paste, Ctrl+C copy, Ctrl+Enter run\n# Example: spawn_line('fence_concrete-_15mb', -30, 20, 40, 20, 0)\n".into(),
            log: vec![], interp: Interp::new(), msg: (String::new(), 0.0), run_request: None,
        }
    }
    pub fn enter(&mut self, pos: Vec3, target: Vec3) {
        self.active = true; self.cam_pos = pos;
        let d = (target - pos).normalize_or_zero();
        self.yaw = d.x.atan2(d.z); self.pitch = d.y.asin().clamp(-1.4, 1.4);
    }
    fn typing(&self) -> bool { self.edit.is_some() || self.console }
    pub fn cam(&mut self, dt: f32) -> (Vec3, Vec3) {
        let (_, wheel) = mouse_wheel();
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        if is_mouse_button_down(MouseButton::Right) {
            let d = mouse_delta_position();
            self.yaw -= d.x * 2.8; self.pitch = (self.pitch + d.y * 2.0).clamp(-1.5, 1.5);
            if wheel != 0.0 { self.speed = (self.speed * (1.0 + wheel * 0.1)).clamp(2.0, 600.0); }
        }
        let fwd = vec3(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos());
        let right = fwd.cross(Vec3::Y).normalize_or_zero();
        if !self.typing() && is_mouse_button_down(MouseButton::Right) {
            let sp = self.speed * if shift { 3.5 } else { 1.0 } * dt;
            if is_key_down(KeyCode::W) { self.cam_pos += fwd * sp; }
            if is_key_down(KeyCode::S) { self.cam_pos -= fwd * sp; }
            if is_key_down(KeyCode::D) { self.cam_pos += right * sp; }
            if is_key_down(KeyCode::A) { self.cam_pos -= right * sp; }
            if is_key_down(KeyCode::E) { self.cam_pos += Vec3::Y * sp; }
            if is_key_down(KeyCode::Q) { self.cam_pos -= Vec3::Y * sp; }
        }
        (self.cam_pos, self.cam_pos + fwd)
    }
    fn say(&mut self, s: &str) { self.msg = (s.to_string(), 3.2); }

    fn repeat_selected(&mut self, level: &mut Level, local_dir: Vec3, half_ext: f32) {
        let id = match self.sel { Some(id) => id, None => return };
        let base = match level.get(id).cloned() { Some(o) => o, None => return };
        let count = self.array_count.clamp(1, 500);
        let step = (half_ext * 2.0 * base.scale + self.array_gap).max(0.15);
        let world_step = (base.rot_quat() * local_dir) * step;
        let mut batch = Vec::with_capacity(count as usize);
        let mut last_id = id;
        for i in 1..=count {
            let mut cp_obj = base.clone();
            let nid = level.next; level.next += 1;
            cp_obj.id = nid;
            cp_obj.pos = base.pos + world_step * (i as f32);
            cp_obj.name = format!("{} {}", cp_obj.kind.name(), nid);
            level.objs.push(cp_obj);
            batch.push(nid);
            last_id = nid;
        }
        level.dirty = true;
        self.sel = Some(last_id);
        self.undo_stack.push(batch);
        self.say(&format!("repeated {} copies (Ctrl+Z to undo batch)", count));
    }

    fn undo_last_batch(&mut self, level: &mut Level) {
        if let Some(batch) = self.undo_stack.pop() {
            let n = batch.len();
            level.objs.retain(|o| !batch.contains(&o.id));
            level.dirty = true;
            if let Some(sel) = self.sel {
                if batch.contains(&sel) {
                    self.sel = level.objs.last().map(|o| o.id);
                }
            }
            self.say(&format!("undid {} object(s)", n));
        } else {
            self.say("nothing to undo");
        }
    }

    fn run_script(&mut self, level: &mut Level, assets: &Assets, src: &str) {
        let mut host = LevelHost { level };
        let before = self.interp.out.len();
        let r = self.interp.run(src, &mut host);
        sync_custom_bounds(assets, level);
        for l in self.interp.out[before..].iter() { self.log.push(l.clone()); }
        if let Err(e) = r { self.log.push(format!("Error: {}", e)); }
        if self.log.len() > 200 { let n = self.log.len() - 200; self.log.drain(0..n); }
    }

    fn field(&mut self, res: &Res, key: &str, label: &str, val: f32, x: f32, y: f32, w: f32, mx: f32, my: f32, click: bool) -> Option<f32> {
        let lw = 92.0;
        txt(&res.font, label, x, y + 14.5, 14, Color::new(1.0, 1.0, 1.0, 0.78));
        let (bx, bw) = (x + lw, w - lw);
        let hover = mx > bx && mx < bx + bw && my > y && my < y + 20.0;
        let editing = matches!(&self.edit, Some((k, _)) if k == key);
        rrect(bx, y, bw, 20.0, 5.0, if editing { Color::new(1.0, 0.56, 0.1, 0.35) } else if hover { Color::new(1.0, 1.0, 1.0, 0.16) } else { Color::new(1.0, 1.0, 1.0, 0.08) });
        let shown = if editing { format!("{}|", self.edit.as_ref().unwrap().1) } else { format!("{:.2}", val) };
        txt(&res.font, &shown, bx + 7.0, y + 14.5, 14, WHITE);
        if hover && click && !editing { self.edit = Some((key.to_string(), format!("{}", (val * 100.0).round() / 100.0))); return None; }
        if editing && is_key_pressed(KeyCode::Enter) { let v = self.edit.take().unwrap().1.parse::<f32>().ok(); return v; }
        None
    }

    /// draws the whole editor UI and handles all interaction.
    pub fn ui(&mut self, res: &Res, level: &mut Level, assets: &mut Assets, vp: Mat4, cp: Vec3, sw: f32, sh: f32, dt: f32) {
        let (mx, my) = mouse_position();
        let click = is_mouse_button_pressed(MouseButton::Left);
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);

        // Clipboard shortcuts for Python Console and Numeric Edit fields
        let mut pasted_this_frame = false;
        if ctrl && is_key_pressed(KeyCode::V) {
            let clip = clipboard_read(&self.internal_clip);
            if let Some((_, buf)) = &mut self.edit {
                for ch in clip.chars() {
                    if ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == 'e' { buf.push(ch); }
                }
                pasted_this_frame = true;
            } else if self.console {
                self.script.push_str(&clip);
                self.say("pasted from clipboard");
                pasted_this_frame = true;
            }
        }
        if ctrl && is_key_pressed(KeyCode::C) && self.console {
            let s = self.script.clone();
            clipboard_write(&s, &mut self.internal_clip);
            self.say("copied script to clipboard");
        }
        if ctrl && is_key_pressed(KeyCode::X) && self.console {
            let s = self.script.clone();
            clipboard_write(&s, &mut self.internal_clip);
            self.script.clear();
            self.say("cut script to clipboard");
        }

        // text input
        while let Some(c) = get_char_pressed() {
            // Handle control char paste (\u{16} = Ctrl+V) if key layout didn't trigger KeyCode::V
            if c == '\u{16}' && !pasted_this_frame {
                let clip = clipboard_read(&self.internal_clip);
                if let Some((_, buf)) = &mut self.edit {
                    for ch in clip.chars() {
                        if ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == 'e' { buf.push(ch); }
                    }
                } else if self.console {
                    self.script.push_str(&clip);
                    self.say("pasted from clipboard");
                }
                pasted_this_frame = true;
                continue;
            }
            if let Some((_, buf)) = &mut self.edit {
                if !ctrl && (c.is_ascii_digit() || c == '.' || c == '-' || c == 'e') { buf.push(c); }
            } else if self.console && !ctrl {
                if c == '\r' || c == '\n' { self.script.push('\n'); }
                else if c == '\t' { self.script.push_str("    "); }
                else if !c.is_control() { self.script.push(c); }
            }
        }
        if is_key_pressed(KeyCode::Backspace) {
            if let Some((_, b)) = &mut self.edit { b.pop(); }
            else if self.console {
                if ctrl { self.script.clear(); } else { self.script.pop(); }
            }
        }
        if is_key_pressed(KeyCode::Escape) { self.edit = None; self.menu = 0; self.line_mode = false; }
        if self.console && ctrl && is_key_pressed(KeyCode::Enter) { let s = self.script.clone(); self.run_script(level, assets, &s); }

        let (lp, rp) = (250.0f32, 310.0f32);
        let top = 46.0;
        let console_h = if self.console { 250.0 } else { 0.0 };
        let menu_x0 = 190.0 + 105.0 + 60.0 + 60.0 + 100.0 + 32.0;
        let in_dropdown = (self.menu == 1 && mx >= menu_x0 - 10.0 && mx <= menu_x0 + 560.0 && my >= top && my <= top + 430.0)
            || (self.menu == 2 && mx >= menu_x0 + 40.0 && mx <= menu_x0 + 360.0 && my >= top && my <= top + 320.0);
        let over_ui = mx < lp || mx > sw - rp || my < top || (self.console && my > sh - console_h) || in_dropdown;
        let inv = vp.inverse();

        let show_move = self.gizmo_mode == 0 || self.gizmo_mode == 1;
        let show_rot = self.gizmo_mode == 0 || self.gizmo_mode == 2;

        // Calculate mouse ground intersection point for Line Mode and 3D Paste
        let (ro, rd) = ray(mx, my, sw, sh, inv, cp);
        let cursor_ground = if rd.y < -0.01 {
            ro + rd * (-ro.y / rd.y).clamp(1.0, 600.0)
        } else {
            vec3(ro.x + rd.x * 40.0, 0.0, ro.z + rd.z * 40.0)
        };

        // ---------------- Interactive Line-Draw Placement Mode (L)
        if self.line_mode && !self.typing() && !over_ui && !is_mouse_button_down(MouseButton::Right) {
            if let Some(id) = self.sel {
                if let Some(base) = level.get(id).cloned() {
                    let p_start = vec3(base.pos.x, 0.0, base.pos.z);
                    let p_end = vec3(cursor_ground.x, 0.0, cursor_ground.z);
                    let delta = p_end - p_start;
                    let dist = delta.length();
                    let span_x = (base.size.x * 2.0 * base.scale).max(0.5);
                    let span_z = (base.size.z * 2.0 * base.scale).max(0.5);
                    let use_x = span_x >= span_z;
                    let step = (if use_x { span_x } else { span_z } + self.array_gap).max(0.4);
                    let dir = if dist > 1e-3 { delta / dist } else { Vec3::X };
                    let line_yaw = if use_x { (-dir.z).atan2(dir.x) } else { dir.x.atan2(dir.z) };
                    let count = ((dist / step).floor() as i32).clamp(0, 300);

                    // Draw live preview line & piece markers from selected object to cursor
                    if let (Some(s2d), Some(e2d)) = (project(vp, base.pos + Vec3::Y * 0.5, sw, sh), project(vp, p_end + Vec3::Y * 0.5, sw, sh)) {
                        draw_line(s2d.x, s2d.y, e2d.x, e2d.y, 3.0, Color::new(0.2, 1.0, 0.5, 0.9));
                    }
                    for i in 1..=count {
                        let wp = base.pos + dir * (i as f32 * step);
                        if let Some(p2d) = project(vp, wp + Vec3::Y * 0.5, sw, sh) {
                            draw_circle(p2d.x, p2d.y, 5.5, Color::new(0.2, 1.0, 0.5, 0.95));
                        }
                    }
                    let badge = format!("Line Repeat: +{} copies (Step {:.2}m) - LMB to place, Esc/L to cancel", count, step);
                    let bw = txt_w(&res.font_b, &badge, 15) + 24.0;
                    rrect(mx + 16.0, my - 34.0, bw, 28.0, 6.0, Color::new(0.04, 0.12, 0.08, 0.94));
                    txt(&res.font_b, &badge, mx + 28.0, my - 15.0, 15, Color::new(0.4, 1.0, 0.6, 1.0));

                    if click && count > 0 {
                        // Rotate the first piece to align with the line as well
                        if let Some(first_o) = level.get_mut(id) {
                            first_o.yaw = line_yaw;
                        }
                        let mut batch = Vec::with_capacity(count as usize);
                        let mut last_id = id;
                        for i in 1..=count {
                            let mut cp_obj = base.clone();
                            let nid = level.next; level.next += 1;
                            cp_obj.id = nid;
                            cp_obj.yaw = line_yaw;
                            cp_obj.pos = base.pos + dir * (i as f32 * step);
                            cp_obj.name = format!("{} {}", cp_obj.kind.name(), nid);
                            level.objs.push(cp_obj);
                            batch.push(nid);
                            last_id = nid;
                        }
                        level.dirty = true;
                        self.sel = Some(last_id);
                        self.undo_stack.push(batch);
                        self.say(&format!("placed line of {} copies! (Click again to continue line, or press L to exit)", count));
                    }
                }
            } else {
                self.line_mode = false;
            }
        }

        // ---------------- picking + gizmo (3D move arrows & 360-degree 3-axis rotation rings)
        if !self.line_mode && !is_mouse_button_down(MouseButton::Right) && !self.typing() {
            if let (Some(id), Some((axis, s0, start))) = (self.sel, self.drag) {
                if is_mouse_button_down(MouseButton::Left) {
                    let a = [Vec3::X, Vec3::Y, Vec3::Z][axis];
                    let (b, c, e) = (a.dot(rd), a.dot(start - ro), rd.dot(start - ro));
                    let den = 1.0 - b * b;
                    if den.abs() > 1e-4 {
                        let s = (b * e - c) / den;
                        let mut np = start + a * (s - s0);
                        if shift { np = (np * 0.5).round() * 2.0; if axis != 1 { np.y = start.y; } }
                        if let Some(o) = level.get_mut(id) { o.pos = np; }
                    }
                } else {
                    self.drag = None;
                }
            } else if let (Some(id), Some((axis, mut prev_ang, mut accum, start_val))) = (self.sel, self.rot_drag) {
                if is_mouse_button_down(MouseButton::Left) {
                    if let Some(o) = level.get(id).cloned() {
                        let origin = o.pos;
                        let delta_ang = if let Some(c2d) = project(vp, origin, sw, sh) {
                            let v = vec2(mx, my) - c2d;
                            if v.length() > 12.0 {
                                let cur = v.y.atan2(v.x);
                                let mut da = cur - prev_ang;
                                while da > std::f32::consts::PI { da -= std::f32::consts::TAU; }
                                while da < -std::f32::consts::PI { da += std::f32::consts::TAU; }
                                prev_ang = cur;
                                let axis_vec = [Vec3::X, Vec3::Y, Vec3::Z][axis];
                                let side = if (cp - origin).dot(axis_vec) >= 0.0 { -1.0 } else { 1.0 };
                                da * side
                            } else {
                                let md = mouse_delta_position();
                                (md.x - md.y) * 2.5
                            }
                        } else {
                            let md = mouse_delta_position();
                            (md.x - md.y) * 2.5
                        };
                        accum += delta_ang;
                        let mut new_ang = start_val + accum;
                        if shift {
                            let step = 15.0f32.to_radians();
                            new_ang = (new_ang / step).round() * step;
                        }
                        if let Some(om) = level.get_mut(id) {
                            match axis {
                                0 => om.pitch = new_ang,
                                1 => om.yaw = new_ang,
                                _ => om.roll = new_ang,
                            }
                        }
                        self.rot_drag = Some((axis, prev_ang, accum, start_val));
                    }
                } else {
                    self.rot_drag = None;
                }
            } else if click && !over_ui {
                let mut hit_gizmo = false;
                if let Some(id) = self.sel {
                    if let Some(o) = level.get(id) {
                        let origin = o.pos;
                        let base_len = ((origin - cp).length() * 0.16).clamp(1.5, 45.0);
                        let arrow_len = base_len * 1.15;
                        let ring_r = base_len * 0.72;
                        if show_move {
                            if let Some(p0) = project(vp, origin, sw, sh) {
                                for (axis, dir) in [Vec3::X, Vec3::Y, Vec3::Z].iter().enumerate() {
                                    if let Some(p1) = project(vp, origin + *dir * arrow_len, sw, sh) {
                                        let tip_dist = (vec2(mx, my) - p1).length();
                                        let shaft_dist = seg_dist(vec2(mx, my), p0, p1);
                                        if tip_dist < 13.0 || (!show_rot && shaft_dist < 10.0) {
                                            let (b, c, e) = (dir.dot(rd), dir.dot(origin - ro), rd.dot(origin - ro));
                                            let den = 1.0 - b * b;
                                            let s0 = if den.abs() > 1e-4 { (b * e - c) / den } else { 0.0 };
                                            self.drag = Some((axis, s0, origin));
                                            hit_gizmo = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if !hit_gizmo && show_rot {
                            let mut best_ring: Option<(f32, usize)> = None;
                            for axis in 0..3 {
                                let d = ring_screen_dist(vec2(mx, my), vp, origin, ring_r, axis, sw, sh);
                                if d < 11.5 && best_ring.map(|b| d < b.0).unwrap_or(true) {
                                    best_ring = Some((d, axis));
                                }
                            }
                            if let Some((_, axis)) = best_ring {
                                let c2d = project(vp, origin, sw, sh).unwrap_or(vec2(mx, my));
                                let init_ang = (my - c2d.y).atan2(mx - c2d.x);
                                let start_val = match axis { 0 => o.pitch, 1 => o.yaw, _ => o.roll };
                                self.rot_drag = Some((axis, init_ang, 0.0, start_val));
                                hit_gizmo = true;
                            }
                        }
                        if !hit_gizmo && show_move {
                            if let Some(p0) = project(vp, origin, sw, sh) {
                                for (axis, dir) in [Vec3::X, Vec3::Y, Vec3::Z].iter().enumerate() {
                                    if let Some(p1) = project(vp, origin + *dir * arrow_len, sw, sh) {
                                        if seg_dist(vec2(mx, my), p0, p1) < 10.5 {
                                            let (b, c, e) = (dir.dot(rd), dir.dot(origin - ro), rd.dot(origin - ro));
                                            let den = 1.0 - b * b;
                                            let s0 = if den.abs() > 1e-4 { (b * e - c) / den } else { 0.0 };
                                            self.drag = Some((axis, s0, origin));
                                            hit_gizmo = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if !hit_gizmo {
                    let mut best: Option<(f32, u32)> = None;
                    for o in level.objs.iter().filter(|o| o.visible || o.kind == Kind::Collider) {
                        let (c, h) = o.pick_box();
                        if let Some(t) = ray_obb_quat(ro, rd, c, h, o.rot_quat()) {
                            if best.map(|b| t < b.0).unwrap_or(true) { best = Some((t, o.id)); }
                        }
                    }
                    self.sel = best.map(|b| b.1);
                }
            }
        }
        // ---------------- keyboard shortcuts (when not typing in console or field)
        if !self.typing() {
            if is_key_pressed(KeyCode::Tab) { self.gizmo_mode = (self.gizmo_mode + 1) % 3; }
            if is_key_pressed(KeyCode::L) && self.sel.is_some() {
                self.line_mode = !self.line_mode;
                self.say(if self.line_mode { "Line Repeat ON: click on ground to fill a line of objects!" } else { "Line Repeat OFF" });
            }
            if ctrl && is_key_pressed(KeyCode::Z) {
                self.undo_last_batch(level);
            }
            if ctrl && is_key_pressed(KeyCode::C) {
                if let Some(id) = self.sel {
                    if let Some(o) = level.get(id).cloned() {
                        let snippet = format!("spawn('{}', {:.2}, {:.2}, {:.1}, {:.2})", o.kind.name(), o.pos.x, o.pos.z, o.yaw.to_degrees(), o.scale);
                        clipboard_write(&snippet, &mut self.internal_clip);
                        self.obj_clip = Some(o);
                        self.say("copied object (Ctrl+V to paste at cursor)");
                    }
                }
            }
            if ctrl && is_key_pressed(KeyCode::V) {
                let template = self.obj_clip.clone().or_else(|| self.sel.and_then(|id| level.get(id).cloned()));
                if let Some(mut cp_obj) = template {
                    let nid = level.next; level.next += 1;
                    cp_obj.id = nid;
                    cp_obj.pos = vec3(cursor_ground.x, cp_obj.pos.y, cursor_ground.z);
                    cp_obj.name = format!("{} {}", cp_obj.kind.name(), nid);
                    level.objs.push(cp_obj);
                    level.dirty = true;
                    self.sel = Some(nid);
                    self.undo_stack.push(vec![nid]);
                    self.say("pasted object at cursor (Ctrl+Z to undo)");
                }
            }
            if let Some(id) = self.sel {
                if is_key_pressed(KeyCode::Delete) { level.remove(id); self.sel = None; }
                else if ctrl && is_key_pressed(KeyCode::D) {
                    if let Some(nid) = level.duplicate(id) {
                        self.sel = Some(nid);
                        self.undo_stack.push(vec![nid]);
                    }
                } else {
                    let step = if shift { 1.0f32.to_radians() } else { 15.0f32.to_radians() };
                    if is_key_pressed(KeyCode::LeftBracket) { if let Some(o) = level.get_mut(id) { o.yaw -= step; } }
                    if is_key_pressed(KeyCode::RightBracket) { if let Some(o) = level.get_mut(id) { o.yaw += step; } }
                    if is_key_pressed(KeyCode::Comma) { if let Some(o) = level.get_mut(id) { o.pitch -= step; } }
                    if is_key_pressed(KeyCode::Period) { if let Some(o) = level.get_mut(id) { o.pitch += step; } }
                    if is_key_pressed(KeyCode::Semicolon) { if let Some(o) = level.get_mut(id) { o.roll -= step; } }
                    if is_key_pressed(KeyCode::Apostrophe) { if let Some(o) = level.get_mut(id) { o.roll += step; } }
                    if is_key_pressed(KeyCode::Equal) { if let Some(o) = level.get_mut(id) { o.scale *= 1.1; } }
                    if is_key_pressed(KeyCode::Minus) { if let Some(o) = level.get_mut(id) { o.scale /= 1.1; } }
                    if is_key_pressed(KeyCode::F) { if let Some(o) = level.get(id) { let (c, h) = o.pick_box(); let d = (h.length() * 1.6).clamp(8.0, 380.0); let fwd = vec3(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos()); self.cam_pos = c - fwd * d; } }
                }
            }
            if ctrl && is_key_pressed(KeyCode::S) { self.save(level); }
        }

        // ---------------- draw selection box + Move Arrows + 360-degree 3-Axis Rotation Rings
        if let Some(id) = self.sel {
            if let Some(o) = level.get(id) {
                let (c, h) = o.pick_box();
                let q = o.rot_quat();
                let corners: Vec<Option<Vec2>> = (0..8).map(|i| {
                    let s = vec3(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 });
                    project(vp, c + q * (s * h), sw, sh)
                }).collect();
                for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                    if let (Some(p), Some(r)) = (corners[a], corners[b]) {
                        draw_line(p.x, p.y, r.x, r.y, 1.5, Color::new(1.0, 0.8, 0.2, 0.9));
                    }
                }
                let origin = o.pos;
                let base_len = ((origin - cp).length() * 0.16).clamp(1.5, 45.0);
                let arrow_len = base_len * 1.15;
                let ring_r = base_len * 0.72;
                let axis_colors = [
                    Color::new(1.0, 0.25, 0.25, 0.95), // X (Red)
                    Color::new(0.25, 0.95, 0.35, 0.95), // Y (Green)
                    Color::new(0.30, 0.60, 1.0, 0.95),  // Z (Blue)
                ];

                // Draw 360-degree 3-axis rotation rings
                if show_rot && !self.line_mode {
                    let mut hovered_ring: Option<usize> = None;
                    if self.drag.is_none() && self.rot_drag.is_none() && !over_ui {
                        let mut best_d = 11.5f32;
                        for axis in 0..3 {
                            let d = ring_screen_dist(vec2(mx, my), vp, origin, ring_r, axis, sw, sh);
                            if d < best_d {
                                best_d = d;
                                hovered_ring = Some(axis);
                            }
                        }
                    }
                    for axis in 0..3 {
                        let (u, v) = ring_basis(axis);
                        let hot = matches!(self.rot_drag, Some((a, _, _, _)) if a == axis) || hovered_ring == Some(axis);
                        let col = if hot { YELLOW } else { axis_colors[axis] };
                        let thick = if hot { 4.5 } else { 2.6 };
                        let mut prev: Option<Vec2> = None;
                        for i in 0..=RING_SEGS {
                            let a = (i as f32 / RING_SEGS as f32) * std::f32::consts::TAU;
                            let wp = origin + (u * a.cos() + v * a.sin()) * ring_r;
                            let cur = project(vp, wp, sw, sh);
                            if let (Some(p0), Some(p1)) = (prev, cur) {
                                draw_line(p0.x, p0.y, p1.x, p1.y, thick, col);
                            }
                            prev = cur;
                        }
                    }
                }

                // Draw 3-axis move arrows
                if show_move && !self.line_mode {
                    if let Some(p0) = project(vp, origin, sw, sh) {
                        for (axis, dir) in [Vec3::X, Vec3::Y, Vec3::Z].iter().enumerate() {
                            if let Some(p1) = project(vp, origin + *dir * arrow_len, sw, sh) {
                                let hot = matches!(self.drag, Some((a, _, _)) if a == axis)
                                    || (self.rot_drag.is_none() && ((vec2(mx, my) - p1).length() < 13.0 || (!show_rot && seg_dist(vec2(mx, my), p0, p1) < 10.5)));
                                let c2 = if hot { YELLOW } else { axis_colors[axis] };
                                draw_line(p0.x, p0.y, p1.x, p1.y, if hot { 5.0 } else { 3.0 }, c2);
                                draw_circle(p1.x, p1.y, 7.5, c2);
                            }
                        }
                        draw_circle(p0.x, p0.y, 4.0, WHITE);
                    }
                }

                // Live angle badge while rotating
                if let Some((axis, _, _, _)) = self.rot_drag {
                    let (label, deg) = match axis {
                        0 => ("Rotate X (Pitch)", wrap_deg_360(o.pitch)),
                        1 => ("Rotate Y (Yaw)", wrap_deg_360(o.yaw)),
                        _ => ("Rotate Z (Roll)", wrap_deg_360(o.roll)),
                    };
                    let badge = format!("{}: {:.1} deg (Shift = 15 deg snap)", label, deg);
                    let bw = txt_w(&res.font_b, &badge, 16) + 24.0;
                    rrect(mx + 16.0, my - 34.0, bw, 28.0, 6.0, Color::new(0.05, 0.08, 0.14, 0.94));
                    txt(&res.font_b, &badge, mx + 28.0, my - 15.0, 16, YELLOW);
                }
            }
        }

        // ---------------- top toolbar
        rrect(0.0, 0.0, sw, top, 0.0, Color::new(0.03, 0.05, 0.09, 0.92));
        txt(&res.font_b, "WORLD EDITOR", 14.0, 30.0, 22, ACCENT);
        let mut bx = 190.0;
        let mut btn = |label: &str, w: f32, on: bool, res: &Res| -> bool {
            let hover = mx > bx && mx < bx + w && my > 7.0 && my < 39.0;
            rrect(bx, 7.0, w, 32.0, 8.0, if on { ACCENT } else if hover { Color::new(1.0, 1.0, 1.0, 0.22) } else { Color::new(1.0, 1.0, 1.0, 0.1) });
            txt(&res.font_b, label, bx + 10.0, 29.0, 15, if on { BLACK } else { WHITE });
            bx += w + 8.0; hover && click
        };
        if btn("PLAY (F10)", 100.0, false, res) { self.active = false; self.line_mode = false; }
        if btn("Save", 58.0, false, res) { self.save(level); }
        if btn("Load", 58.0, false, res) { self.load(level, assets); }
        if btn("Reset map", 95.0, false, res) { *level = Level::default_level(); level.forest_dirty = true; self.sel = None; self.undo_stack.clear(); self.say("map reset to default"); }
        if btn("+ Add", 68.0, self.menu == 1, res) {
            if self.menu != 1 {
                rescan_custom_assets(assets);
                self.menu = 1;
            } else {
                self.menu = 0;
            }
        }
        if btn("Forest", 70.0, self.menu == 2, res) { self.menu = if self.menu == 2 { 0 } else { 2 }; }
        let gizmo_label = match self.gizmo_mode {
            0 => "Gizmo: Move+Rot 360",
            1 => "Gizmo: Move Only",
            _ => "Gizmo: Rotate 360",
        };
        if btn(gizmo_label, 175.0, self.gizmo_mode == 2, res) {
            self.gizmo_mode = (self.gizmo_mode + 1) % 3;
        }
        if btn("+ Collision", 104.0, false, res) {
            let (ro, rd) = ray(sw / 2.0, sh / 2.0, sw, sh, inv, cp);
            let mut p = if rd.y < -0.02 { ro + rd * (-ro.y / rd.y).min(120.0) } else { ro + rd * 25.0 };
            let id = level.add(Kind::Collider, vec3(p.x, 1.5, p.z));
            if let Some(o) = level.get_mut(id) {
                p.y = o.size.y;
                o.pos.y = p.y;
            }
            self.sel = Some(id);
            self.undo_stack.push(vec![id]);
            self.say("Spawned manual Collision Volume! Resize Half X/Y/Z, rotate 360, or press L to draw a collision wall.");
        }
        if btn("Line Draw (L)", 114.0, self.line_mode, res) {
            if self.sel.is_some() {
                self.line_mode = !self.line_mode;
                self.say(if self.line_mode { "Line Repeat ON: click on ground to fill a line of objects!" } else { "Line Repeat OFF" });
            } else {
                self.say("Select an object first, then click Line Draw (L)");
            }
        }
        let hh = (self.day_hour as u32) % 24;
        let mm = ((self.day_hour.fract() * 60.0) as u32) % 60;
        let phase = if self.day_hour >= 5.2 && self.day_hour < 7.5 { "Sunrise" }
            else if self.day_hour >= 7.5 && self.day_hour < 16.8 { "Day" }
            else if self.day_hour >= 16.8 && self.day_hour < 19.3 { "Sunset" }
            else { "Night" };
        if btn(&format!("{:02}:{:02} {}", hh, mm, phase), 118.0, false, res) {
            self.day_hour = if self.day_hour < 7.0 { 12.5 }
                else if self.day_hour < 16.5 { 17.85 }
                else if self.day_hour < 20.0 { 22.5 }
                else { 6.15 };
            self.say(&format!("Time set to {:02}:{:02}", (self.day_hour as u32)%24, ((self.day_hour.fract()*60.0) as u32)%60));
        }
        if btn("Python", 78.0, self.console, res) { self.console = !self.console; }
        let _ = bx;

        // ---------------- left: hierarchy
        let hh = sh - top - console_h;
        rrect(0.0, top, lp, hh, 0.0, Color::new(0.04, 0.06, 0.10, 0.97));
        txt(&res.font_b, &format!("Hierarchy ({})", level.objs.len()), 12.0, top + 24.0, 17, WHITE);
        if mx < lp && my > top && my < top + hh { let (_, w) = mouse_wheel(); if !is_mouse_button_down(MouseButton::Right) { self.hscroll = (self.hscroll - w * 0.5).max(0.0); } }
        let rows = ((hh - 40.0) / 22.0) as usize;
        let start = (self.hscroll as usize).min(level.objs.len().saturating_sub(1));
        let mut pick: Option<u32> = None;
        for (ri, o) in level.objs.iter().skip(start).take(rows).enumerate() {
            let y = top + 36.0 + ri as f32 * 22.0;
            let on = self.sel == Some(o.id);
            let hover = mx < lp && my > y && my < y + 21.0;
            if on || hover { rrect(6.0, y, lp - 12.0, 21.0, 5.0, if on { Color::new(1.0, 0.56, 0.1, 0.35) } else { Color::new(1.0, 1.0, 1.0, 0.1) }); }
            let mut nm = o.name.clone(); while txt_w(&res.font, &nm, 15) > lp - 40.0 && nm.len() > 3 { nm.pop(); }
            txt(&res.font, &nm, 14.0, y + 15.0, 15, if o.visible { WHITE } else { GRAY });
            if hover && click { pick = Some(o.id); }
        }
        if let Some(id) = pick { self.sel = Some(id); }

        // ---------------- right: inspector + Smart Array / Repeater
        rrect(sw - rp, top, rp, hh, 0.0, Color::new(0.04, 0.06, 0.10, 0.97));
        txt(&res.font_b, "Inspector", sw - rp + 12.0, top + 22.0, 17, WHITE);
        let mut y = top + 32.0;
        let fx = sw - rp + 10.0; let fw = rp - 20.0;
        if let Some(id) = self.sel {
            if let Some(o) = level.get(id).cloned() {
                txt(&res.font, &o.name, fx, y + 12.0, 15, ACCENT); y += 20.0;
                let mut o2 = o.clone();
                macro_rules! num { ($key:expr, $label:expr, $val:expr, $set:expr) => { if let Some(v) = self.field(res, $key, $label, $val, fx, y, fw, mx, my, click) { $set(&mut o2, v); } y += 22.0; }; }
                num!("px", "Pos X", o.pos.x, |o: &mut Obj, v: f32| o.pos.x = v);
                num!("py", "Pos Y", o.pos.y, |o: &mut Obj, v: f32| o.pos.y = v);
                num!("pz", "Pos Z", o.pos.z, |o: &mut Obj, v: f32| o.pos.z = v);
                num!("yaw", "Rot Y (360)", wrap_deg_360(o.yaw), |o: &mut Obj, v: f32| o.yaw = v.to_radians());
                num!("pitch", "Rot X (360)", wrap_deg_360(o.pitch), |o: &mut Obj, v: f32| o.pitch = v.to_radians());
                num!("roll", "Rot Z (360)", wrap_deg_360(o.roll), |o: &mut Obj, v: f32| o.roll = v.to_radians());
                let r_hover = mx > fx && mx < fx + fw && my > y && my < y + 20.0;
                rrect(fx, y, fw, 20.0, 5.0, if r_hover { ACCENT } else { Color::new(1.0, 1.0, 1.0, 0.10) });
                txt(&res.font, "Reset Rotation (0, 0, 0)", fx + 10.0, y + 14.5, 13, if r_hover { BLACK } else { WHITE });
                if r_hover && click { o2.yaw = 0.0; o2.pitch = 0.0; o2.roll = 0.0; }
                y += 23.0;

                num!("sc", "Scale", o.scale, |o: &mut Obj, v: f32| o.scale = v.max(0.01));
                match o.kind {
                    Kind::Box | Kind::Collider | Kind::Hazard => { num!("sx", "Half X", o.size.x, |o: &mut Obj, v: f32| o.size.x = v.max(0.05)); num!("sy", "Half Y", o.size.y, |o: &mut Obj, v: f32| o.size.y = v.max(0.05)); num!("sz", "Half Z", o.size.z, |o: &mut Obj, v: f32| o.size.z = v.max(0.05));
                        if o.kind == Kind::Box { num!("cr", "Red", o.color[0], |o: &mut Obj, v: f32| o.color[0] = v.clamp(0.0, 1.0)); num!("cg", "Green", o.color[1], |o: &mut Obj, v: f32| o.color[1] = v.clamp(0.0, 1.0)); num!("cb", "Blue", o.color[2], |o: &mut Obj, v: f32| o.color[2] = v.clamp(0.0, 1.0)); } }
                    Kind::Ramp => { num!("sx", "Half width", o.size.x, |o: &mut Obj, v: f32| o.size.x = v.max(0.5)); num!("sy", "Height", o.size.y, |o: &mut Obj, v: f32| o.size.y = v.max(0.1)); num!("sz", "Up length", o.size.z, |o: &mut Obj, v: f32| o.size.z = v.max(1.0)); num!("p2", "Top length", o.p2, |o: &mut Obj, v: f32| o.p2 = v.max(0.0)); }
                    _ => {}
                }
                let toggle = |label: &str, on: bool, y: f32| -> bool { let hover = mx > fx && mx < fx + fw && my > y && my < y + 20.0; rrect(fx, y, fw, 20.0, 5.0, if hover { Color::new(1.0, 1.0, 1.0, 0.18) } else { Color::new(1.0, 1.0, 1.0, 0.08) }); rrect(fx + 6.0, y + 3.0, 14.0, 14.0, 3.0, if on { ACCENT } else { Color::new(1.0, 1.0, 1.0, 0.2) }); txt(&res.font, label, fx + 26.0, y + 14.5, 14, WHITE); hover && click };
                if toggle("Collider (solid)", o.collide, y) { o2.collide = !o2.collide; } y += 22.0;
                if toggle("Visible", o.visible, y) { o2.visible = !o2.visible; } y += 25.0;

                if o2.pos != o.pos || o2.yaw != o.yaw || o2.pitch != o.pitch || o2.roll != o.roll || o2.scale != o.scale || o2.size != o.size || o2.p2 != o.p2 || o2.color != o.color || o2.collide != o.collide || o2.visible != o.visible {
                    if let Some(t) = level.get_mut(id) { *t = o2.clone(); }
                }

                // -------- SMART DIRECTIONAL REPEATER / ARRAY SECTION --------
                rrect(fx - 4.0, y, fw + 8.0, 162.0, 8.0, Color::new(0.08, 0.14, 0.24, 0.85));
                txt(&res.font_b, "Smart Repeat / Array (تكرار منتظم)", fx + 4.0, y + 17.0, 14, ACCENT);
                y += 22.0;
                if let Some(v) = self.field(res, "arr_n", "Copies", self.array_count as f32, fx, y, fw, mx, my, click) {
                    self.array_count = (v.round() as i32).clamp(1, 500) as u32;
                }
                y += 22.0;
                if let Some(v) = self.field(res, "arr_gap", "Gap (0=Touch)", self.array_gap, fx, y, fw, mx, my, click) {
                    self.array_gap = v.clamp(-20.0, 200.0);
                }
                y += 24.0;

                // 2x2 grid of directional repeat buttons along the object's local rotated axes
                let hw_btn = (fw - 6.0) * 0.5;
                let mut rep_dir: Option<(Vec3, f32)> = None;
                for (row, ((l1, d1, h1), (l2, d2, h2))) in [
                    (("Repeat +X (Right)", Vec3::X, o2.size.x), ("Repeat -X (Left)", -Vec3::X, o2.size.x)),
                    (("Repeat +Z (Fwd)", Vec3::Z, o2.size.z), ("Repeat -Z (Back)", -Vec3::Z, o2.size.z)),
                ].iter().enumerate() {
                    let yy = y + row as f32 * 26.0;
                    let h_a = mx >= fx && mx <= fx + hw_btn && my >= yy && my <= yy + 23.0;
                    rrect(fx, yy, hw_btn, 23.0, 5.0, if h_a { ACCENT } else { Color::new(0.2, 0.55, 0.95, 0.28) });
                    txt(&res.font_b, l1, fx + 7.0, yy + 16.0, 13, if h_a { BLACK } else { WHITE });
                    if h_a && click { rep_dir = Some((*d1, *h1)); }

                    let fx2 = fx + hw_btn + 6.0;
                    let h_b = mx >= fx2 && mx <= fx2 + hw_btn && my >= yy && my <= yy + 23.0;
                    rrect(fx2, yy, hw_btn, 23.0, 5.0, if h_b { ACCENT } else { Color::new(0.2, 0.55, 0.95, 0.28) });
                    txt(&res.font_b, l2, fx2 + 7.0, yy + 16.0, 13, if h_b { BLACK } else { WHITE });
                    if h_b && click { rep_dir = Some((*d2, *h2)); }
                }
                y += 54.0;

                // Draw Line with Mouse + Undo Batch row
                let h_line = mx >= fx && mx <= fx + hw_btn && my >= y && my <= y + 24.0;
                rrect(fx, y, hw_btn, 24.0, 5.0, if self.line_mode || h_line { Color::new(0.25, 0.9, 0.45, 0.9) } else { Color::new(0.15, 0.6, 0.3, 0.35) });
                txt(&res.font_b, if self.line_mode { "Line Mode: ON (L)" } else { "Draw Line (L)" }, fx + 7.0, y + 16.5, 13, if self.line_mode || h_line { BLACK } else { WHITE });
                if h_line && click { self.line_mode = !self.line_mode; }

                let fx2 = fx + hw_btn + 6.0;
                let h_undo = mx >= fx2 && mx <= fx2 + hw_btn && my >= y && my <= y + 24.0;
                rrect(fx2, y, hw_btn, 24.0, 5.0, if h_undo { ACCENT } else { Color::new(0.9, 0.3, 0.2, 0.28) });
                txt(&res.font_b, "Undo Batch (Ctrl+Z)", fx2 + 6.0, y + 16.5, 13, if h_undo { BLACK } else { WHITE });
                if h_undo && click { self.undo_last_batch(level); }
                y += 32.0;

                if let Some((dir, half_ext)) = rep_dir {
                    self.repeat_selected(level, dir, half_ext);
                }

                // Standard object actions
                let mut act = 0;
                for (i, l) in ["Duplicate 1x (Ctrl+D)", "Copy / Paste (Ctrl+C / V)", "Delete (Del)", "Focus (F)"].iter().enumerate() {
                    let hover = mx > fx && mx < fx + fw && my > y && my < y + 23.0;
                    rrect(fx, y, fw, 23.0, 6.0, if hover { ACCENT } else { Color::new(1.0, 1.0, 1.0, 0.12) });
                    txt(&res.font_b, l, fx + 10.0, y + 16.0, 13, if hover { BLACK } else { WHITE });
                    if hover && click { act = i + 1; }
                    y += 26.0;
                }
                if act == 1 {
                    if let Some(nid) = level.duplicate(id) {
                        self.sel = Some(nid);
                        self.undo_stack.push(vec![nid]);
                    }
                } else if act == 2 {
                    self.obj_clip = Some(o2.clone());
                    if let Some(nid) = level.duplicate(id) {
                        self.sel = Some(nid);
                        self.undo_stack.push(vec![nid]);
                        self.say("copied & pasted object (or use Ctrl+C / Ctrl+V at cursor)");
                    }
                } else if act == 3 {
                    level.remove(id);
                    self.sel = None;
                } else if act == 4 {
                    let (c, h) = o.pick_box();
                    let d = (h.length() * 1.6).clamp(8.0, 380.0);
                    let fwd = vec3(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos());
                    self.cam_pos = c - fwd * d;
                }
            }
        } else {
            txt(&res.font, "Click an object in the world", fx, y + 14.0, 15, Color::new(1.0, 1.0, 1.0, 0.6));
            txt(&res.font, "or in the Hierarchy to edit it.", fx, y + 36.0, 15, Color::new(1.0, 1.0, 1.0, 0.6));
            txt(&res.font, "Drag arrows = Move X/Y/Z", fx, y + 68.0, 14, Color::new(0.75, 0.9, 1.0, 0.8));
            txt(&res.font, "Drag rings = Rotate 360 X/Y/Z", fx, y + 90.0, 14, Color::new(0.75, 0.9, 1.0, 0.8));
            txt(&res.font, "Smart Repeat / Line Draw (L):", fx, y + 122.0, 14, ACCENT);
            txt(&res.font, "Select a fence/lamp & click", fx, y + 142.0, 14, Color::new(0.8, 0.95, 0.8, 0.85));
            txt(&res.font, "Repeat +X/-X or Draw Line (L)!", fx, y + 162.0, 14, Color::new(0.8, 0.95, 0.8, 0.85));
        }

        // ---------------- dropdown menus
        if self.menu == 1 {
            let panel_w = 540.0;
            let panel_h = 415.0;
            rrect(menu_x0, top, panel_w, panel_h, 10.0, Color::new(0.04, 0.06, 0.10, 0.97));
            let col1_x = menu_x0 + 10.0;
            let col1_w = 290.0;
            let col2_x = menu_x0 + 310.0;
            let col2_w = 220.0;

            txt(&res.font_b, "3D Models & Buildings (.glb)", col1_x + 6.0, top + 22.0, 15, ACCENT);
            txt(&res.font_b, "Props, Nature & Shapes", col2_x + 6.0, top + 22.0, 15, ACCENT);

            let mut left_items: Vec<String> = vec!["apartment".into(), "school".into()];
            for c in &assets.custom {
                left_items.push(c.name.clone());
            }
            if mx >= col1_x && mx <= col1_x + col1_w && my >= top && my <= top + panel_h {
                let (_, w) = mouse_wheel();
                if !is_mouse_button_down(MouseButton::Right) && w != 0.0 {
                    self.add_scroll = (self.add_scroll - w * 0.5).clamp(0.0, left_items.len().saturating_sub(14) as f32);
                }
            }
            let max_left_rows = 14usize;
            let start_left = (self.add_scroll as usize).min(left_items.len().saturating_sub(1));
            let mut chosen: Option<String> = None;

            for (ri, k) in left_items.iter().skip(start_left).take(max_left_rows).enumerate() {
                let yy = top + 30.0 + ri as f32 * 24.0;
                let hover = mx >= col1_x && mx <= col1_x + col1_w && my >= yy && my <= yy + 23.0;
                let is_custom = ri + start_left >= 2;
                if hover {
                    rrect(col1_x, yy, col1_w, 23.0, 5.0, Color::new(1.0, 0.56, 0.1, 0.42));
                } else if is_custom {
                    rrect(col1_x, yy, col1_w, 23.0, 5.0, Color::new(0.2, 0.45, 0.85, 0.14));
                }
                let mut label = k.clone();
                while txt_w(&res.font, &label, 15) > col1_w - 16.0 && label.len() > 4 { label.pop(); }
                txt(&res.font, &label, col1_x + 8.0, yy + 16.5, 15, if is_custom { Color::new(0.75, 0.92, 1.0, 1.0) } else { WHITE });
                if hover && click { chosen = Some(k.clone()); }
            }

            let ry = top + panel_h - 36.0;
            let rhover = mx >= col1_x && mx <= col1_x + col1_w && my >= ry && my <= ry + 28.0;
            rrect(col1_x, ry, col1_w, 28.0, 6.0, if rhover { ACCENT } else { Color::new(1.0, 1.0, 1.0, 0.12) });
            txt(&res.font_b, "Rescan .glb folder (assets / mods)", col1_x + 12.0, ry + 19.0, 14, if rhover { BLACK } else { WHITE });
            if rhover && click {
                let before = assets.custom.len();
                rescan_custom_assets(assets);
                sync_custom_bounds(assets, level);
                let added = assets.custom.len().saturating_sub(before);
                self.say(&format!("scanned .glb models ({} total, +{} new)", assets.custom.len(), added));
            }

            let right_items = ["collider", "box", "hazard", "ramp", "manhole", "tree0", "tree1", "tree2", "tree3", "tree4", "rock0", "rock1", "rock2", "rock3"];
            for (ri, k) in right_items.iter().enumerate() {
                let yy = top + 30.0 + ri as f32 * 24.0;
                let hover = mx >= col2_x && mx <= col2_x + col2_w && my >= yy && my <= yy + 23.0;
                if hover { rrect(col2_x, yy, col2_w, 23.0, 5.0, Color::new(1.0, 0.56, 0.1, 0.42)); }
                txt(&res.font, k, col2_x + 8.0, yy + 16.5, 15, WHITE);
                if hover && click { chosen = Some((*k).to_string()); }
            }

            if let Some(k) = chosen {
                let (ro, rd) = ray(sw / 2.0, sh / 2.0, sw, sh, inv, cp);
                let mut p = if rd.y < -0.02 { ro + rd * (-ro.y / rd.y).min(120.0) } else { ro + rd * 25.0 };
                p.y = 0.0;
                if let Some(kind) = Kind::parse(&k) {
                    let id = level.add(kind, p);
                    if let Some(o) = level.get_mut(id) {
                        if matches!(o.kind, Kind::Box | Kind::Collider | Kind::Hazard) { o.pos.y = o.size.y; }
                        if matches!(o.kind, Kind::Manhole) { o.pos.y = 0.03; }
                        if let Kind::Custom(ref cname) = o.kind {
                            if let Some(ca) = assets.custom.iter().find(|c| &c.name == cname) {
                                o.size = ca.half;
                                if ca.half.x > 60.0 || ca.half.z > 60.0 || cname.contains("road") || cname.contains("street") {
                                    o.collide = false;
                                }
                            }
                        }
                    }
                    self.sel = Some(id);
                    self.undo_stack.push(vec![id]);
                    self.menu = 0;
                    self.say(&format!("added {}", k));
                }
            }
        }
        if self.menu == 2 {
            let x0 = 190.0 + 105.0 + 60.0 + 60.0 + 100.0 + 70.0 + 24.0;
            rrect(x0, top, 280.0, 300.0, 10.0, Color::new(0.04, 0.06, 0.10, 0.96));
            let f = level.forest.clone(); let mut f2 = f.clone();
            let mut yy = top + 12.0;
            let hover = mx > x0 + 8.0 && mx < x0 + 272.0 && my > yy && my < yy + 24.0;
            rrect(x0 + 8.0, yy, 264.0, 24.0, 6.0, if hover { Color::new(1.0, 1.0, 1.0, 0.18) } else { Color::new(1.0, 1.0, 1.0, 0.08) });
            rrect(x0 + 14.0, yy + 5.0, 14.0, 14.0, 3.0, if f.on { ACCENT } else { Color::new(1.0, 1.0, 1.0, 0.2) }); txt(&res.font, "Forest enabled", x0 + 36.0, yy + 17.0, 15, WHITE);
            if hover && click { f2.on = !f.on; } yy += 32.0;
            macro_rules! fnum { ($key:expr, $label:expr, $val:expr, $set:expr) => { if let Some(v) = self.field(res, $key, $label, $val, x0 + 10.0, yy, 260.0, mx, my, click) { $set(&mut f2, v); } yy += 26.0; }; }
            fnum!("f_count", "Trees", f.count as f32, |f: &mut Forest, v: f32| f.count = v.clamp(0.0, 8000.0) as u32);
            fnum!("f_inner", "Clear radius", f.inner, |f: &mut Forest, v: f32| f.inner = v.max(100.0));
            fnum!("f_outer", "Outer radius", f.outer, |f: &mut Forest, v: f32| f.outer = v.max(f.inner + 10.0));
            fnum!("f_seed", "Random seed", f.seed as f32, |f: &mut Forest, v: f32| f.seed = v as u32);
            fnum!("f_scale", "Tree scale", f.scale, |f: &mut Forest, v: f32| f.scale = v.clamp(0.2, 4.0));
            txt(&res.font, "Tree types:", x0 + 12.0, yy + 14.0, 15, Color::new(1.0, 1.0, 1.0, 0.75)); yy += 22.0;
            for k in 0..5 {
                let bxk = x0 + 12.0 + k as f32 * 50.0;
                let hover = mx > bxk && mx < bxk + 44.0 && my > yy && my < yy + 26.0;
                rrect(bxk, yy, 44.0, 26.0, 6.0, if f.mask[k] { ACCENT } else if hover { Color::new(1.0, 1.0, 1.0, 0.2) } else { Color::new(1.0, 1.0, 1.0, 0.1) });
                txt(&res.font_b, &format!("{}", k), bxk + 17.0, yy + 19.0, 16, if f.mask[k] { BLACK } else { WHITE });
                if hover && click { f2.mask[k] = !f.mask[k]; }
            }
            if f2.on != f.on || f2.count != f.count || f2.inner != f.inner || f2.outer != f.outer || f2.seed != f.seed || f2.scale != f.scale || f2.mask != f.mask { level.forest = f2; level.forest_dirty = true; }
        }

        // ---------------- python console (with Paste Ctrl+V & Copy Ctrl+C buttons)
        if self.console {
            let cy = sh - console_h;
            rrect(lp, cy, sw - lp - rp, console_h, 0.0, Color::new(0.03, 0.04, 0.07, 0.98));
            txt(&res.font_b, "Python console (Ctrl+V Paste | Ctrl+C Copy | Ctrl+Enter Run)", lp + 12.0, cy + 22.0, 15, ACCENT);
            let w2 = (sw - lp - rp) * 0.58;
            // script area
            rrect(lp + 8.0, cy + 30.0, w2 - 12.0, console_h - 76.0, 6.0, Color::new(1.0, 1.0, 1.0, 0.05));
            let lines: Vec<&str> = self.script.split('\n').collect();
            let maxl = ((console_h - 90.0) / 17.0) as usize;
            let first = lines.len().saturating_sub(maxl);
            for (i, l) in lines.iter().enumerate().skip(first) { let yy = cy + 48.0 + (i - first) as f32 * 17.0; let cur = if i + 1 == lines.len() && (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" }; txt(&res.font, &format!("{}{}", l, cur), lp + 14.0, yy, 15, Color::new(0.85, 1.0, 0.85, 1.0)); }
            // buttons
            let mut bxx = lp + 8.0; let byy = cy + console_h - 38.0;
            let mut cbtn = |label: &str, w: f32, res: &Res| -> bool { let hover = mx > bxx && mx < bxx + w && my > byy && my < byy + 28.0; rrect(bxx, byy, w, 28.0, 7.0, if hover { ACCENT } else { Color::new(1.0, 1.0, 1.0, 0.12) }); txt(&res.font_b, label, bxx + 8.0, byy + 19.5, 14, if hover { BLACK } else { WHITE }); bxx += w + 6.0; hover && click };
            if cbtn("Run (Ctrl+Enter)", 140.0, res) { let s = self.script.clone(); self.run_script(level, assets, &s); }
            if cbtn("Paste (Ctrl+V)", 118.0, res) {
                let clip = clipboard_read(&self.internal_clip);
                self.script.push_str(&clip);
                self.say("pasted from clipboard");
            }
            if cbtn("Copy (Ctrl+C)", 112.0, res) {
                let s = self.script.clone();
                clipboard_write(&s, &mut self.internal_clip);
                self.say("copied script to clipboard");
            }
            if cbtn("Clear", 56.0, res) { self.script.clear(); }
            if cbtn("Save", 56.0, res) { let _ = std::fs::create_dir_all("scripts"); let ok = std::fs::write("scripts/user.py", &self.script).is_ok(); self.say(if ok { "saved scripts/user.py" } else { "could not save script" }); }
            for s in list_scripts().iter().take(2) { if cbtn(s, txt_w(&res.font_b, s, 14) + 18.0, res) { if let Ok(t) = std::fs::read_to_string(format!("scripts/{}", s)) { self.script = t; } } }
            // output
            let ox = lp + w2 + 8.0;
            rrect(ox, cy + 30.0, sw - lp - rp - w2 - 16.0, console_h - 40.0, 6.0, Color::new(0.0, 0.0, 0.0, 0.35));
            txt(&res.font, "Output", ox + 8.0, cy + 46.0, 14, Color::new(1.0, 1.0, 1.0, 0.5));
            let mo = ((console_h - 70.0) / 17.0) as usize;
            for (i, l) in self.log.iter().rev().take(mo).collect::<Vec<_>>().into_iter().rev().enumerate() { txt(&res.font, l, ox + 8.0, cy + 66.0 + i as f32 * 17.0, 14, if l.starts_with("Error") { Color::new(1.0, 0.5, 0.4, 1.0) } else { WHITE }); }
        }

        // status
        if self.msg.1 > 0.0 { self.msg.1 -= dt; let w = txt_w(&res.font, &self.msg.0, 18) + 40.0; rrect(sw / 2.0 - w / 2.0, top + 10.0, w, 34.0, 10.0, Color::new(0.1, 0.5, 0.2, 0.92)); txt(&res.font, &self.msg.0, sw / 2.0 - w / 2.0 + 20.0, top + 33.0, 18, WHITE); }
        txt(&res.font, "RMB+WASD fly | Arrows=Move / Rings=Rotate 360 | L=Draw Line | Ctrl+C/V Copy/Paste | Ctrl+Z Undo Batch", lp + 10.0, sh - console_h - 8.0, 13, Color::new(1.0, 1.0, 1.0, 0.60));
    }

    pub fn save(&mut self, level: &Level) {
        let _ = std::fs::create_dir_all("scenes");
        let ok = std::fs::write("scenes/level.json", level.to_json()).is_ok();
        self.say(if ok { "saved scenes/level.json" } else { "could not save (folder not writable)" });
    }
    pub fn load(&mut self, level: &mut Level, assets: &Assets) {
        match std::fs::read_to_string("scenes/level.json").ok().and_then(|t| Level::from_json(&t)) {
            Some(mut l) => { sync_custom_bounds(assets, &mut l); *level = l; level.forest_dirty = true; self.sel = None; self.undo_stack.clear(); self.say("loaded scenes/level.json"); }
            None => self.say("no valid scenes/level.json found"),
        }
    }
}
