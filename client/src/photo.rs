// Task 3: Built-in 4K In-Game Photo Mode, Cinematic Camera Sequencer & High-Res Screenshot Exporter.
// Strictly uses the in-game 3D camera inside `level.json` (Zero AI image generation).
use crate::{rrect, txt, txt_w, Res};
use macroquad::prelude::*;

pub struct CinematicShot {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub cam_pos: Vec3,
    pub cam_look: Vec3,
    pub car_pos: Vec3,
    pub car_yaw: f32,
    pub car_spec: usize,
    pub fov_deg: f32,
    pub day_hour: f32,
    pub dof_strength: f32,
}

/// 6 curated Golden-Hour & Sunset cinematic presets inside the official `level.json` map.
pub const OFFICIAL_MAP_SHOTS: [CinematicShot; 6] = [
    CinematicShot {
        title: "01 - Golden Hour at the Central Plaza & School",
        subtitle: "Mercedes-AMG One framed against the School & Sunset Sky",
        cam_pos: Vec3::new(-10.2, 1.35, 17.8),
        cam_look: Vec3::new(-17.8, 1.45, 22.4),
        car_pos: Vec3::new(-15.0, 0.0, 21.5),
        car_yaw: -0.45,
        car_spec: 0,
        fov_deg: 34.0,
        day_hour: 17.65,
        dof_strength: 0.45,
    },
    CinematicShot {
        title: "02 - Concrete Fence Corridor & North Boulevard",
        subtitle: "Low-angle tracking shot along the 83-fence corridor",
        cam_pos: Vec3::new(-33.5, 1.10, 70.2),
        cam_look: Vec3::new(-43.0, 1.25, 73.8),
        car_pos: Vec3::new(-40.5, 0.0, 73.2),
        car_yaw: 1.57,
        car_spec: 1,
        fov_deg: 30.0,
        day_hour: 17.80,
        dof_strength: 0.50,
    },
    CinematicShot {
        title: "03 - Scania Touring 2026 Coach Bus on Main Avenue",
        subtitle: "Sunset reflection on the Scania Touring luxury coach",
        cam_pos: Vec3::new(4.2, 1.85, -44.5),
        cam_look: Vec3::new(-2.5, 1.75, -54.0),
        car_pos: Vec3::new(-2.5, 0.0, -54.0),
        car_yaw: 0.15,
        car_spec: 4,
        fov_deg: 36.0,
        day_hour: 17.50,
        dof_strength: 0.35,
    },
    CinematicShot {
        title: "04 - 2025 BMW M3 CS Touring at Northern Monument",
        subtitle: "Twilight warm glow & rule-of-thirds framing",
        cam_pos: Vec3::new(-6.2, 1.45, 168.5),
        cam_look: Vec3::new(-12.8, 1.65, 181.0),
        car_pos: Vec3::new(-10.5, 0.0, 174.5),
        car_yaw: -0.52,
        car_spec: 2,
        fov_deg: 35.0,
        day_hour: 18.05,
        dof_strength: 0.40,
    },
    CinematicShot {
        title: "05 - Porsche 911 GT3 RS (992) on Boulevard Corner",
        subtitle: "Rule-of-thirds composition with dramatic Golden Hour shadows",
        cam_pos: Vec3::new(-22.0, 1.60, 56.5),
        cam_look: Vec3::new(-34.5, 2.20, 49.5),
        car_pos: Vec3::new(-28.0, 0.0, 54.0),
        car_yaw: -2.35,
        car_spec: 3,
        fov_deg: 32.0,
        day_hour: 17.40,
        dof_strength: 0.45,
    },
    CinematicShot {
        title: "06 - Aerial Golden Hour Panorama of West Coast Urban District",
        subtitle: "Wide architectural overview of apartments, school & boulevard",
        cam_pos: Vec3::new(28.0, 26.0, -36.0),
        cam_look: Vec3::new(-18.0, 3.0, 38.0),
        car_pos: Vec3::new(-15.0, 0.0, 25.0),
        car_yaw: 0.0,
        car_spec: 0,
        fov_deg: 46.0,
        day_hour: 17.60,
        dof_strength: 0.20,
    },
];

pub struct PhotoMode {
    pub active: bool,
    pub preset_idx: usize,
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_deg: f32,
    pub dof_strength: f32,
    pub show_grid: bool,
    pub hide_ui: bool,
    pub status_toast: Option<(String, f32)>,
    pub request_4k_capture: bool,
}

impl PhotoMode {
    pub fn new() -> Self {
        let s = &OFFICIAL_MAP_SHOTS[0];
        let d = (s.cam_look - s.cam_pos).normalize_or_zero();
        PhotoMode {
            active: false,
            preset_idx: 0,
            pos: s.cam_pos,
            yaw: d.x.atan2(d.z),
            pitch: d.y.asin(),
            fov_deg: s.fov_deg,
            dof_strength: s.dof_strength,
            show_grid: true,
            hide_ui: false,
            status_toast: None,
            request_4k_capture: false,
        }
    }

    pub fn apply_preset(&mut self, idx: usize, day_hour: &mut f32) -> (Vec3, f32, usize) {
        let i = idx % OFFICIAL_MAP_SHOTS.len();
        self.preset_idx = i;
        let s = &OFFICIAL_MAP_SHOTS[i];
        self.pos = s.cam_pos;
        let d = (s.cam_look - s.cam_pos).normalize_or_zero();
        self.yaw = d.x.atan2(d.z);
        self.pitch = d.y.asin();
        self.fov_deg = s.fov_deg;
        self.dof_strength = s.dof_strength;
        *day_hour = s.day_hour;
        (s.car_pos, s.car_yaw, s.car_spec)
    }

    pub fn step_camera(&mut self, dt: f32) -> (Vec3, Vec3) {
        if is_mouse_button_down(MouseButton::Right) {
            let md = mouse_delta_position();
            self.yaw -= md.x * 2.2;
            self.pitch = (self.pitch - md.y * 1.8).clamp(-1.45, 1.45);
        }
        let (_, wh) = mouse_wheel();
        if wh != 0.0 {
            self.fov_deg = (self.fov_deg - wh * 2.5).clamp(16.0, 85.0);
        }
        let fwd = vec3(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        ).normalize_or_zero();
        let right = fwd.cross(Vec3::Y).normalize_or_zero();
        let spd = if is_key_down(KeyCode::LeftShift) { 28.0 } else { 9.0 } * dt;
        if is_key_down(KeyCode::W) { self.pos += fwd * spd; }
        if is_key_down(KeyCode::S) { self.pos -= fwd * spd; }
        if is_key_down(KeyCode::D) { self.pos += right * spd; }
        if is_key_down(KeyCode::A) { self.pos -= right * spd; }
        if is_key_down(KeyCode::E) { self.pos.y += spd; }
        if is_key_down(KeyCode::Q) { self.pos.y = (self.pos.y - spd).max(0.25); }
        if is_key_pressed(KeyCode::G) { self.show_grid = !self.show_grid; }
        if is_key_pressed(KeyCode::H) { self.hide_ui = !self.hide_ui; }
        if is_key_pressed(KeyCode::F12) || is_key_pressed(KeyCode::Enter) {
            self.request_4k_capture = true;
        }
        (self.pos, self.pos + fwd * 10.0)
    }

    /// Draws optical Depth-of-Field edge softness & cinematic Golden-Hour color grading, plus Photo Mode UI.
    pub fn draw_overlay(&mut self, res: &Res, day_hour: f32, dt: f32) {
        let (sw, sh) = (screen_width(), screen_height());
        // Subtle optical Depth-of-Field peripheral blur/vignette rings
        if self.dof_strength > 0.02 {
            let bands = 6;
            for b in 0..bands {
                let frac = (b + 1) as f32 / bands as f32;
                let pad_x = sw * 0.16 * (1.0 - frac);
                let pad_y = sh * 0.18 * (1.0 - frac);
                let alpha = self.dof_strength * 0.065 * frac;
                let warm = Color::new(0.08, 0.04, 0.02, alpha);
                draw_rectangle(0.0, 0.0, sw, pad_y, warm);
                draw_rectangle(0.0, sh - pad_y, sw, pad_y, warm);
                draw_rectangle(0.0, pad_y, pad_x, sh - 2.0 * pad_y, warm);
                draw_rectangle(sw - pad_x, pad_y, pad_x, sh - 2.0 * pad_y, warm);
            }
        }
        if self.hide_ui { return; }

        if self.show_grid {
            let gc = Color::new(1.0, 1.0, 1.0, 0.22);
            draw_line(sw / 3.0, 0.0, sw / 3.0, sh, 1.0, gc);
            draw_line(sw * 2.0 / 3.0, 0.0, sw * 2.0 / 3.0, sh, 1.0, gc);
            draw_line(0.0, sh / 3.0, sw, sh / 3.0, 1.0, gc);
            draw_line(0.0, sh * 2.0 / 3.0, sw, sh * 2.0 / 3.0, 1.0, gc);
        }

        let s = &OFFICIAL_MAP_SHOTS[self.preset_idx % OFFICIAL_MAP_SHOTS.len()];
        rrect(20.0, 16.0, 660.0, 86.0, 10.0, Color::new(0.03, 0.04, 0.06, 0.86));
        txt(&res.font_b, "IN-GAME 4K PHOTO MODE & CINEMATIC SEQUENCER (F8)", 36.0, 42.0, 19, Color::new(0.98, 0.54, 0.12, 1.0));
        txt(&res.font_b, s.title, 36.0, 66.0, 17, WHITE);
        txt(&res.font, &format!("{}  |  FOV {:.0}°  |  DoF {:.0}%  |  Time {:02}:{:02}",
            s.subtitle, self.fov_deg, self.dof_strength * 100.0, (day_hour as u32) % 24, ((day_hour.fract() * 60.0) as u32) % 60),
            36.0, 88.0, 14, Color::new(0.85, 0.88, 0.94, 0.85));

        rrect(20.0, sh - 62.0, sw - 40.0, 44.0, 8.0, Color::new(0.03, 0.04, 0.06, 0.86));
        let help = "Keys [1..6] Cinematic Shot Presets  |  WASD+QE Move  |  RMB Look  |  Wheel FOV  |  N/M Golden Hour  |  G Rule-of-Thirds  |  [F12 / Enter] Save 4K Screenshot";
        txt(&res.font, help, 36.0, sh - 34.0, 15, WHITE);

        if let Some((msg, ttl)) = &mut self.status_toast {
            *ttl -= dt;
            let w = txt_w(&res.font_b, msg, 18) + 48.0;
            rrect(sw / 2.0 - w / 2.0, 114.0, w, 42.0, 8.0, Color::new(0.12, 0.58, 0.26, 0.95));
            txt(&res.font_b, msg, sw / 2.0 - w / 2.0 + 24.0, 141.0, 18, WHITE);
        }
        if matches!(&self.status_toast, Some((_, t)) if *t <= 0.0) {
            self.status_toast = None;
        }
    }
}
