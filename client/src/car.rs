// Vehicle: soft-body chassis + Pacejka "Magic Formula" tire physics + tire thermal/wear/pressure/compression
// + aerodynamic drag/downforce/CoP + passenger/cargo load mass & CoG shift (VW Crafter / Coach Bus)
// + brake rotor thermal & fade modeling + zero-allocation object-pooled mesh buffers.
use crate::hicar::{self, HiModel};
use crate::jbeam;
use crate::model::{self, CarModel, RawMesh};
use crate::soft::SoftBody;
use crate::world::World;
use macroquad::prelude::*;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq)]
pub enum Drive { Rwd, Fwd, Awd }

#[derive(Clone, Copy, PartialEq)]
pub enum HiKind { AmgOne, AmgGt, BmwM3Cs, PorscheGt3Rs, CrafterBus }

pub struct Spec {
    pub name: &'static str,
    pub mass: f32,
    pub power: f32,
    pub drive: Drive,
    pub grip: f32,
    pub top_gear_bias: f32,
    pub hi_kind: HiKind,
    pub drv: &'static Drivetrain,
    pub eye: Vec3,
    pub default_paint: [f32; 3],
}

/// Real-car data: overall gear ratios, engine map (Nm), brakes, aerodynamics & Center of Pressure.
pub struct Drivetrain {
    pub gears: &'static [f32],
    pub final_drive: f32,
    pub idle: f32,
    pub redline: f32,
    pub up_rpm: f32,
    pub down_rpm: f32,
    pub shift_time: f32,
    pub torque: &'static [(f32, f32)],
    pub brake_f: f32,
    pub brake_r: f32,
    pub cd_a: f32,
    pub cl_a: f32,
    /// Aero Center of Pressure longitudinal balance (0.0 = rear axle, 0.5 = center, 0.44 = 44% front / 56% rear)
    pub cop_front: f32,
}

/// 1. Mercedes-AMG One: 1.6 V6 turbo hybrid + 4 e-motors (~782 kW / 1063 hp), 8-speed automated manual, AWD.
pub static AMG_ONE: Drivetrain = Drivetrain {
    gears: &[3.33, 2.81, 2.36, 1.99, 1.67, 1.41, 1.19, 0.96],
    final_drive: 4.2,
    idle: 1200.0,
    redline: 11000.0,
    up_rpm: 10500.0,
    down_rpm: 4800.0,
    shift_time: 0.11,
    torque: &[(0.0, 380.0), (1000.0, 520.0), (2000.0, 650.0), (3000.0, 710.0), (4500.0, 750.0), (6000.0, 780.0), (8000.0, 800.0), (9500.0, 790.0), (10500.0, 700.0), (11000.0, 520.0), (11600.0, 0.0)],
    brake_f: 5400.0,
    brake_r: 3800.0,
    cd_a: 0.62,
    cl_a: 1.55,
    cop_front: 0.43,
};

/// 2. Mercedes-AMG GT Black Series: 4.0L V8 Bi-Turbo flat-plane crank (730 hp / 800 Nm), 7-speed DCT, RWD.
pub static AMG_GT_BS: Drivetrain = Drivetrain {
    gears: &[3.40, 2.19, 1.63, 1.29, 1.03, 0.84, 0.72],
    final_drive: 3.88,
    idle: 950.0,
    redline: 7200.0,
    up_rpm: 7000.0,
    down_rpm: 3100.0,
    shift_time: 0.09,
    torque: &[(0.0, 320.0), (1200.0, 540.0), (2000.0, 800.0), (4000.0, 800.0), (5500.0, 785.0), (6700.0, 720.0), (7200.0, 580.0), (7600.0, 0.0)],
    brake_f: 5100.0,
    brake_r: 3500.0,
    cd_a: 0.74,
    cl_a: 1.72,
    cop_front: 0.41,
};

/// 3. 2025 BMW M3 CS Touring (G81): 3.0L S58 Twin-Turbo Inline-6 (550 hp / 650 Nm), 8-speed M Steptronic, M xDrive AWD.
pub static BMW_M3_CS: Drivetrain = Drivetrain {
    gears: &[5.00, 3.20, 2.14, 1.72, 1.31, 1.00, 0.82, 0.64],
    final_drive: 3.15,
    idle: 850.0,
    redline: 7200.0,
    up_rpm: 6950.0,
    down_rpm: 2800.0,
    shift_time: 0.12,
    torque: &[(0.0, 260.0), (1200.0, 440.0), (2750.0, 650.0), (5950.0, 650.0), (6500.0, 610.0), (7200.0, 540.0), (7500.0, 0.0)],
    brake_f: 4800.0,
    brake_r: 3400.0,
    cd_a: 0.61,
    cl_a: 0.65,
    cop_front: 0.47,
};

/// 4. Porsche 911 GT3 RS (992): 4.0L Naturally Aspirated Flat-6 (525 hp / 465 Nm @ 9000 RPM), 7-speed PDK, RWD, DRS Wing.
pub static PORSCHE_GT3_RS: Drivetrain = Drivetrain {
    gears: &[3.75, 2.38, 1.72, 1.34, 1.11, 0.96, 0.84],
    final_drive: 3.97,
    idle: 1000.0,
    redline: 9000.0,
    up_rpm: 8850.0,
    down_rpm: 4200.0,
    shift_time: 0.08,
    torque: &[(0.0, 210.0), (2000.0, 340.0), (4000.0, 410.0), (6250.0, 465.0), (7500.0, 455.0), (8500.0, 435.0), (9000.0, 405.0), (9300.0, 0.0)],
    brake_f: 5300.0,
    brake_r: 3900.0,
    cd_a: 0.78,
    cl_a: 2.15,
    cop_front: 0.39,
};

/// 5. Volkswagen Crafter / Scania Touring Bus: Turbo-Diesel Commercial Van & Coach Drivetrain, heavy payload & passengers.
pub static CRAFTER_BUS: Drivetrain = Drivetrain {
    gears: &[5.80, 3.70, 2.40, 1.62, 1.15, 0.84],
    final_drive: 3.42,
    idle: 650.0,
    redline: 3200.0,
    up_rpm: 2650.0,
    down_rpm: 1150.0,
    shift_time: 0.32,
    torque: &[(0.0, 750.0), (800.0, 1450.0), (1200.0, 2150.0), (1800.0, 2150.0), (2400.0, 1750.0), (3000.0, 1150.0), (3400.0, 0.0)],
    brake_f: 16500.0,
    brake_r: 14500.0,
    cd_a: 3.45,
    cl_a: 0.18,
    cop_front: 0.52,
};

/// Phase 1 & Phase 3 Production Roster: ONLY the 5 authentic high-detail vehicles!
pub const SPECS: [Spec; 5] = [
    Spec { name: "Mercedes-AMG One",             mass: 1695.0,  power: 3.00, drive: Drive::Awd, grip: 1.50, top_gear_bias: 1.0, hi_kind: HiKind::AmgOne,       drv: &AMG_ONE,        eye: Vec3::new(0.34, 0.96, -0.12), default_paint: [0.80, 0.83, 0.87] },
    Spec { name: "Mercedes-AMG GT Black Series", mass: 1540.0,  power: 2.68, drive: Drive::Rwd, grip: 1.46, top_gear_bias: 1.0, hi_kind: HiKind::AmgGt,        drv: &AMG_GT_BS,      eye: Vec3::new(0.35, 0.93, -0.15), default_paint: [0.98, 0.42, 0.06] },
    Spec { name: "2025 BMW M3 CS Touring",       mass: 1850.0,  power: 2.35, drive: Drive::Awd, grip: 1.40, top_gear_bias: 1.0, hi_kind: HiKind::BmwM3Cs,      drv: &BMW_M3_CS,      eye: Vec3::new(0.36, 1.08,  0.05), default_paint: [0.08, 0.42, 0.24] },
    Spec { name: "Porsche 911 GT3 RS (992)",     mass: 1450.0,  power: 2.55, drive: Drive::Rwd, grip: 1.54, top_gear_bias: 1.0, hi_kind: HiKind::PorscheGt3Rs, drv: &PORSCHE_GT3_RS, eye: Vec3::new(0.34, 0.95, -0.10), default_paint: [0.92, 0.94, 0.96] },
    Spec { name: "Volkswagen Crafter / Coach",   mass: 9800.0,  power: 1.85, drive: Drive::Rwd, grip: 0.98, top_gear_bias: 1.0, hi_kind: HiKind::CrafterBus,   drv: &CRAFTER_BUS,    eye: Vec3::new(0.78, 2.25,  5.45), default_paint: [0.90, 0.92, 0.95] },
];

/// Tunable physics parameters loaded from `physics.json`.
#[derive(Clone, Copy, Debug)]
pub struct PhysicsConfig {
    pub gravity: f32,
    pub pacejka_bx: f32,
    pub pacejka_cx: f32,
    pub pacejka_ex: f32,
    pub pacejka_by: f32,
    pub pacejka_cy: f32,
    pub pacejka_ey: f32,
    pub mu_asphalt_dry: f32,
    pub mu_asphalt_wet: f32,
    pub mu_dirt: f32,
    pub mu_grass: f32,
    pub default_tire_psi: f32,
    pub passenger_mass_kg: f32,
    pub default_bus_passengers: u32,
    pub substeps_low: u32,
    pub substeps_med: u32,
    pub substeps_high: u32,
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            gravity: 9.81,
            pacejka_bx: 12.2,
            pacejka_cx: 1.68,
            pacejka_ex: 0.32,
            pacejka_by: 10.4,
            pacejka_cy: 1.48,
            pacejka_ey: -0.42,
            mu_asphalt_dry: 1.08,
            mu_asphalt_wet: 0.76,
            mu_dirt: 0.60,
            mu_grass: 0.48,
            default_tire_psi: 32.0,
            passenger_mass_kg: 80.0,
            default_bus_passengers: 24,
            substeps_low: 42,
            substeps_med: 60,
            substeps_high: 90,
        }
    }
}

impl PhysicsConfig {
    pub fn load() -> Self {
        let mut cfg = Self::default();
        for path in ["physics.json", "client/physics.json"] {
            if let Ok(txt) = std::fs::read_to_string(path) {
                if let Ok(j) = jbeam::parse(&txt) {
                    let g = |k: &str, d: f32| j.get(k).and_then(|v| v.num()).map(|x| x as f32).unwrap_or(d);
                    cfg.gravity = g("gravity", cfg.gravity);
                    cfg.pacejka_bx = g("pacejka_bx", cfg.pacejka_bx);
                    cfg.pacejka_cx = g("pacejka_cx", cfg.pacejka_cx);
                    cfg.pacejka_ex = g("pacejka_ex", cfg.pacejka_ex);
                    cfg.pacejka_by = g("pacejka_by", cfg.pacejka_by);
                    cfg.pacejka_cy = g("pacejka_cy", cfg.pacejka_cy);
                    cfg.pacejka_ey = g("pacejka_ey", cfg.pacejka_ey);
                    cfg.mu_asphalt_dry = g("mu_asphalt_dry", cfg.mu_asphalt_dry);
                    cfg.mu_asphalt_wet = g("mu_asphalt_wet", cfg.mu_asphalt_wet);
                    cfg.mu_dirt = g("mu_dirt", cfg.mu_dirt);
                    cfg.mu_grass = g("mu_grass", cfg.mu_grass);
                    cfg.default_tire_psi = g("default_tire_psi", cfg.default_tire_psi);
                    cfg.passenger_mass_kg = g("passenger_mass_kg", cfg.passenger_mass_kg);
                    cfg.default_bus_passengers = g("default_bus_passengers", cfg.default_bus_passengers as f32) as u32;
                    cfg.substeps_low = g("substeps_low", cfg.substeps_low as f32) as u32;
                    cfg.substeps_med = g("substeps_med", cfg.substeps_med as f32) as u32;
                    cfg.substeps_high = g("substeps_high", cfg.substeps_high as f32) as u32;
                    return cfg;
                }
            }
        }
        cfg
    }
}

/// Pacejka "Magic Formula": y(x) = sin(C * atan(B*x - E*(B*x - atan(B*x))))
#[inline]
pub fn pacejka_magic(x: f32, b: f32, c: f32, e: f32) -> f32 {
    let bx = b * x;
    (c * (bx - e * (bx - bx.atan())).atan()).sin()
}

#[derive(Clone, Copy, Default)]
pub struct Input {
    pub throttle: f32, // 0..1 (W)
    pub brake: f32,    // 0..1 (S)
    pub steer: f32,    // -1..1, + = left
    pub handbrake: bool,
}

#[derive(Clone, Copy)]
pub struct Frame { pub c: Vec3, pub fwd: Vec3, pub up: Vec3, pub left: Vec3 }

impl Frame {
    pub fn to_local(&self, p: Vec3) -> Vec3 {
        let d = p - self.c;
        vec3(d.dot(self.left), d.dot(self.up), d.dot(self.fwd))
    }
    pub fn to_world_dir(&self, l: Vec3) -> Vec3 { self.left * l.x + self.up * l.y + self.fwd * l.z }
}

pub struct Wheel {
    pub hub_rest: Vec3,
    pub mount_ids: [usize; 8],
    pub mount_w: [f32; 8],
    pub front: bool,
    pub left: bool,
    pub omega: f32,
    pub spin: f32,
    pub s: f32,             // suspension length (mount -> hub)
    pub grounded: bool,
    pub load: f32,
    pub mesh: RawMesh,
    pub steer_angle: f32,
    pub slip: f32,
    pub slip_ratio: f32,
    pub slip_angle_deg: f32,
    pub tire_deflection: f32, // radial compression of tire sidewall under load (m)
    pub pressure_psi: f32,    // air pressure (PSI, optimal ~32 PSI / 2.2 bar)
    pub temp_c: f32,          // tire tread temperature (°C, optimal 75..98 °C)
    pub brake_temp_c: f32,    // brake rotor temperature (°C, optimal 150..550 °C)
    pub wear: f32,            // 0.0 (new) .. 1.0 (worn out)
    pub surface_id: u8,       // 0=Dry Asphalt, 1=Wet Asphalt, 2=Dirt/Gravel, 3=Grass
    pub mount_rest_local: Vec3,
    pub ka: f32,
    pub kk: f32,
    pub abs_active: bool,
}

pub struct Car {
    pub spec_idx: usize,
    pub body: SoftBody,
    pub wheels: Vec<Wheel>,
    pub radius: f32,
    pub wheel_scale: f32,
    pub rest_body: SoftBody,
    // suspension
    pub k_spring: f32,
    pub c_damp: f32,
    pub s_eq: f32,
    pub s_free: f32,
    pub s_min: f32,
    pub s_max: f32,
    // render data + object-pooled scratch buffers
    pub mesh_src: RawMesh,
    pub bind_ids: Vec<[usize; 8]>,
    pub bind_w: Vec<[f32; 8]>,
    pub render: Mesh,
    pub wheel_render: Vec<Mesh>,
    pub scratch_pos: Vec<Vec3>,
    pub scratch_nrm: Vec<Vec3>,
    pub render_dirty: bool,
    // drivetrain & thermal
    pub gear: i32,
    pub rpm: f32,
    pub clutch: f32,     // 0.0 (disengaged) .. 1.0 (fully locked)
    pub shift_timer: f32,
    pub input: Input,
    pub steer_cur: f32,
    pub throttle_cur: f32,
    pub boost_bar: f32,
    pub fuel: f32,       // 0.0 .. 1.0
    pub eng_temp_c: f32, // °C
    pub engine_on: bool,
    // bus passenger & cargo load system
    pub passengers: u32,
    /// فترة سماح بعد الإنشاء: تُتجاهل فيها صناديق التصادم (مبانٍ/أشجار) حتى لا تُسحق السيارة إن نشأت قرب مجسم
    pub grace: f32,
    pub passenger_mass_kg: f32,
    pub cog_height_m: f32,
    // state & kinematics
    pub asleep: bool,
    pub still_time: f32,
    pub frame: Frame,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    pub aabb: (Vec3, Vec3),
    pub speed: f32,
    pub accel_long_g: f32, // smoothed longitudinal acceleration (G) for cockpit camera pitch
    pub accel_lat_g: f32,  // smoothed lateral acceleration (G) for cockpit camera roll/lean
    pub impact_g: f32,
    pub peak_g: f32,
    pub last_vel: Vec3,
    pub is_player: bool,
    pub toe_bias: f32,
    pub tc: bool,
    pub abs: bool,
    pub hi: Option<Rc<HiModel>>,
    pub paint: Vec3,
    pub hi_def: Option<hicar::HiDef>,
    pub hi_active: bool,
    pub pcfg: PhysicsConfig,
}

pub const WHEEL_I: f32 = 2.4;

impl Car {
    pub fn new(spec_idx: usize, texture: Texture2D, is_player: bool) -> Car {
        let pcfg = PhysicsConfig::load();
        let spec_idx = spec_idx.min(SPECS.len() - 1);
        let spec = &SPECS[spec_idx];
        let hi: Option<Rc<HiModel>> = match spec.hi_kind {
            HiKind::AmgOne => hicar::amg(),
            HiKind::AmgGt => hicar::amg_gt(),
            HiKind::BmwM3Cs => hicar::bmw_m3_cs(),
            HiKind::PorscheGt3Rs => hicar::porsche_gt3_rs(),
            HiKind::CrafterBus => hicar::scania_bus(),
        };
        let model: CarModel = match &hi {
            Some(h) => {
                let r = (h.radii[0] + h.radii[2]) * 0.5;
                CarModel {
                    body: h.proxy.clone(),
                    wheel_hub: h.hubs,
                    wheel_mesh: [
                        model::cylinder(r, 0.30),
                        model::cylinder(r, 0.30),
                        model::cylinder(r, 0.30),
                        model::cylinder(r, 0.30),
                    ],
                }
            }
            None => CarModel {
                body: model::box_mesh(vec3(-1.0, 0.15, -2.2), vec3(1.0, 1.25, 2.2)),
                wheel_hub: [vec3(0.85, 0.35, 1.35), vec3(-0.85, 0.35, 1.35), vec3(0.85, 0.35, -1.35), vec3(-0.85, 0.35, -1.35)],
                wheel_mesh: [model::cylinder(0.35, 0.30), model::cylinder(0.35, 0.30), model::cylinder(0.35, 0.30), model::cylinder(0.35, 0.30)],
            },
        };
        let raw = model.body.clone();
        let (mut bmin, mut bmax) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        for p in &raw.pos { bmin = bmin.min(*p); bmax = bmax.max(*p); }
        bmin -= Vec3::splat(0.02);
        bmax += Vec3::splat(0.02);
        bmin.y = bmin.y.max(0.13);
        let stiff = match spec.hi_kind {
            HiKind::CrafterBus => 3.5e5,
            HiKind::AmgOne | HiKind::AmgGt | HiKind::PorscheGt3Rs | HiKind::BmwM3Cs => 3.9e5,
        };
        let mut body = SoftBody::build(bmin, bmax, (8, 6, 15), spec.mass, stiff);
        body.place(vec3(0.0, 0.0, 0.0), 0.0);

        let wm = &model.wheel_mesh[0];
        let (mut ymin, mut ymax) = (1e9f32, -1e9f32);
        for p in &wm.pos { ymin = ymin.min(p.y); ymax = ymax.max(p.y); }
        let radius = ((ymax - ymin) * 0.5).max(0.28);

        let s_eq = if spec.hi_kind == HiKind::CrafterBus { 0.38f32 } else { 0.30f32 };
        let k_spring = 54000.0 * spec.mass / 1300.0;
        let c_damp = 0.64 * 2.0 * (k_spring * spec.mass / 4.0).sqrt();
        let s_free = s_eq + spec.mass * pcfg.gravity / 4.0 / k_spring;

        let mut wheels = Vec::new();
        let mut wheel_render = Vec::new();
        for w in 0..4 {
            let hub_m = model.wheel_hub[w];
            // Enforce exact lateral & longitudinal axle symmetry for zero false toe/camber pull
            let sym_x = if w % 2 == 0 { hub_m.x.abs() } else { -hub_m.x.abs() };
            let sym_z = if w < 2 {
                (model.wheel_hub[0].z + model.wheel_hub[1].z) * 0.5
            } else {
                (model.wheel_hub[2].z + model.wheel_hub[3].z) * 0.5
            };
            let hub = vec3(sym_x, radius, sym_z);
            let mount = hub + vec3(0.0, s_eq, 0.0);
            let (ids, ws) = body.trilinear(mount);
            let mesh = model.wheel_mesh[w].clone();
            let w_verts: Vec<Vertex> = (0..mesh.pos.len()).map(|i| Vertex {
                position: mesh.pos[i],
                uv: mesh.uv[i],
                color: [255, 255, 255, 255],
                normal: vec4(mesh.nrm[i].x, mesh.nrm[i].y, mesh.nrm[i].z, -1.0),
            }).collect();
            wheel_render.push(Mesh { vertices: w_verts, indices: mesh.idx.clone(), texture: Some(texture.clone()) });
            wheels.push(Wheel {
                hub_rest: hub,
                mount_ids: ids,
                mount_w: ws,
                front: w < 2,
                left: w % 2 == 0,
                omega: 0.0,
                spin: 0.0,
                s: s_eq,
                grounded: false,
                load: 0.0,
                mesh,
                steer_angle: 0.0,
                slip: 0.0,
                slip_ratio: 0.0,
                slip_angle_deg: 0.0,
                tire_deflection: 0.0,
                pressure_psi: if spec.hi_kind == HiKind::CrafterBus { 95.0 } else { pcfg.default_tire_psi },
                temp_c: 36.0,
                brake_temp_c: 65.0,
                wear: 0.0,
                surface_id: 0,
                mount_rest_local: mount,
                ka: 0.0,
                kk: 0.0,
                abs_active: false,
            });
        }

        let mut bind_ids = Vec::new();
        let mut bind_w = Vec::new();
        for p in &raw.pos {
            let (i, w) = body.trilinear(*p);
            bind_ids.push(i);
            bind_w.push(w);
        }
        let verts: Vec<Vertex> = (0..raw.pos.len()).map(|i| Vertex {
            position: raw.pos[i],
            uv: raw.uv[i],
            color: [255, 255, 255, 255],
            normal: vec4(raw.nrm[i].x, raw.nrm[i].y, raw.nrm[i].z, 2.22),
        }).collect();
        let n_verts = raw.pos.len();
        let render = Mesh { vertices: verts, indices: raw.idx.clone(), texture: Some(texture) };
        let rest_body = body.clone();
        let initial_passengers = if spec.hi_kind == HiKind::CrafterBus { pcfg.default_bus_passengers } else { 0 };
        let dp = spec.default_paint;
        let mut car = Car {
            spec_idx,
            body,
            wheels,
            radius,
            wheel_scale: 1.0,
            rest_body,
            k_spring,
            c_damp,
            s_eq,
            s_free,
            s_min: s_eq - 0.17,
            s_max: s_eq + 0.18,
            mesh_src: raw,
            bind_ids,
            bind_w,
            render,
            wheel_render,
            scratch_pos: vec![Vec3::ZERO; n_verts],
            scratch_nrm: vec![Vec3::ZERO; n_verts],
            render_dirty: true,
            gear: 0,
            rpm: spec.drv.idle,
            clutch: 1.0,
            shift_timer: 0.0,
            input: Input::default(),
            steer_cur: 0.0,
            throttle_cur: 0.0,
            boost_bar: 0.0,
            fuel: 0.94,
            eng_temp_c: 88.0,
            engine_on: true,
            passengers: 0,
            grace: 0.0,
            passenger_mass_kg: pcfg.passenger_mass_kg,
            cog_height_m: 0.48,
            asleep: false,
            still_time: 0.0,
            frame: Frame { c: Vec3::ZERO, fwd: Vec3::Z, up: Vec3::Y, left: Vec3::X },
            bounds_min: Vec3::ZERO,
            bounds_max: Vec3::ZERO,
            aabb: (Vec3::ZERO, Vec3::ZERO),
            speed: 0.0,
            accel_long_g: 0.0,
            accel_lat_g: 0.0,
            impact_g: 0.0,
            peak_g: 0.0,
            last_vel: Vec3::ZERO,
            is_player,
            toe_bias: 0.0,
            tc: true,
            abs: true,
            hi,
            paint: vec3(dp[0], dp[1], dp[2]),
            hi_def: None,
            hi_active: false,
            pcfg,
        };
        if initial_passengers > 0 {
            car.set_passengers(initial_passengers);
        }
        car.update_frame();
        car
    }

    /// Adjusts passenger & cargo load (0..55) on the Bus/Van (or 0..4 on sports cars),
    /// adding real mass (`80 kg` per passenger) to the upper cabin nodes, shifting CoG,
    /// and re-stabilizing the soft-body lattice for numerical stability.
    pub fn set_passengers(&mut self, count: u32) {
        let max_p = if SPECS[self.spec_idx].hi_kind == HiKind::CrafterBus { 55 } else { 4 };
        let count = count.min(max_p);
        self.passengers = count;
        let base_mass = SPECS[self.spec_idx].mass;
        let extra_mass = count as f32 * self.passenger_mass_kg;
        let (nx, ny, nz) = (self.body.nx, self.body.ny, self.body.nz);

        let mut cabin_nodes = 0usize;
        for _k in 1..(nz - 1) {
            for _j in (ny / 2)..(ny - 1) {
                for _i in 0..nx {
                    cabin_nodes += 1;
                }
            }
        }
        let per_node = if cabin_nodes > 0 { extra_mass / cabin_nodes as f32 } else { 0.0 };
        let mut total_m = 0.0f32;
        let mut cog_y = 0.0f32;
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let id = self.body.idx(i, j, k);
                    let mut m = self.rest_body.mass[id];
                    if k >= 1 && k + 1 < nz && j >= ny / 2 && j + 1 < ny {
                        m += per_node;
                    }
                    self.body.mass[id] = m;
                    self.body.inv_m[id] = 1.0 / m.max(0.25);
                    total_m += m;
                    cog_y += self.rest_body.rest[id].y * m;
                }
            }
        }
        self.body.total_mass = total_m.max(base_mass);
        self.cog_height_m = cog_y / self.body.total_mass;
        // Dynamically adjust equilibrium spring free length so loaded bus compresses suspension realistically
        self.s_free = self.s_eq + self.body.total_mass * self.pcfg.gravity / 4.0 / self.k_spring;
        self.render_dirty = true;
    }

    pub fn respawn(&mut self, pos: Vec3, yaw: f32) {
        let cur_passengers = self.passengers;
        self.body = self.rest_body.clone();
        self.body.place(pos, yaw);
        self.grace = 2.5;
        if cur_passengers > 0 {
            self.set_passengers(cur_passengers);
        }
        for w in self.wheels.iter_mut() {
            w.omega = 0.0;
            w.spin = 0.0;
            w.s = self.s_eq;
            w.tire_deflection = 0.0;
            w.temp_c = 36.0;
            w.brake_temp_c = 65.0;
            w.ka = 0.0;
            w.kk = 0.0;
            w.steer_angle = 0.0;
        }
        self.gear = 0;
        self.rpm = SPECS[self.spec_idx].drv.idle;
        self.clutch = 1.0;
        self.asleep = false;
        self.still_time = 0.0;
        self.steer_cur = 0.0;
        self.toe_bias = 0.0;
        self.accel_long_g = 0.0;
        self.accel_lat_g = 0.0;
        self.peak_g = 0.0;
        self.last_vel = Vec3::ZERO;
        self.render_dirty = true;
        self.update_frame();
    }

    pub fn update_frame(&mut self) {
        let b = &self.body;
        let mut c = Vec3::ZERO;
        let mut msum = 0.0f32;
        for i in 0..b.pos.len() {
            c += b.pos[i] * b.mass[i];
            msum += b.mass[i];
        }
        c /= msum.max(1.0);
        let (nx, ny, nz) = (b.nx, b.ny, b.nz);
        let avg_plane = |f: &dyn Fn(usize, usize, usize) -> bool| {
            let mut s = Vec3::ZERO; let mut n = 0.0;
            for k in 0..nz { for j in 0..ny { for i in 0..nx { if f(i, j, k) { s += b.pos[b.idx(i, j, k)]; n += 1.0; } } } }
            s / n
        };
        let front = avg_plane(&|_, _, k| k == nz - 1);
        let rear = avg_plane(&|_, _, k| k == 0);
        let left = avg_plane(&|i, _, _| i == nx - 1);
        let right = avg_plane(&|i, _, _| i == 0);
        let fwd = (front - rear).normalize_or_zero();
        let l0 = left - right;
        let up = fwd.cross(l0).normalize_or_zero();
        let leftv = up.cross(fwd).normalize_or_zero();
        self.frame = Frame { c, fwd, up, left: leftv };
        let (mut lmin, mut lmax) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        let (mut wmin, mut wmax) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        for &i in &b.surface {
            let l = self.frame.to_local(b.pos[i]);
            lmin = lmin.min(l); lmax = lmax.max(l);
            wmin = wmin.min(b.pos[i]); wmax = wmax.max(b.pos[i]);
        }
        self.bounds_min = lmin; self.bounds_max = lmax;
        self.aabb = (wmin, wmax);
        let v = self.body.avg_velocity();
        self.speed = v.dot(self.frame.fwd);
    }

    pub fn set_input(&mut self, inp: Input) { self.input = inp; }

    pub fn control(&mut self, keys: Input, dt: f32) {
        let spd = self.speed;
        let mut thr = 0.0;
        let mut brk = 0.0;
        if keys.throttle > 0.0 {
            if spd < -1.5 { brk = 1.0; } else { thr = keys.throttle; if self.gear < 0 { self.gear = 0; } }
        } else if keys.brake > 0.0 {
            if spd > 1.5 { brk = keys.brake; } else { thr = keys.brake; self.gear = -1; }
        }
        if !self.engine_on { thr = 0.0; }
        let rate = 6.5;
        self.throttle_cur += (thr - self.throttle_cur) * (1.0 - (-rate * dt).exp());
        let max_steer = 0.60 / (1.0 + (spd.abs() / 15.0).powf(1.55));
        let target = keys.steer * max_steer.max(0.045);
        // Smooth critically damped steering actuator (zero high-frequency step jitter)
        let steer_rate = if target.abs() > self.steer_cur.abs() { 7.5 } else { 11.0 };
        self.steer_cur += (target - self.steer_cur) * (1.0 - (-steer_rate * dt).exp());
        if self.steer_cur.abs() < 1e-4 && target.abs() < 1e-4 {
            self.steer_cur = 0.0;
        }
        self.input = Input { throttle: self.throttle_cur, brake: brk, steer: self.steer_cur, handbrake: keys.handbrake };

        // Update turbo boost bar & engine thermal cooling loop
        let redline = SPECS[self.spec_idx].drv.redline;
        let is_na = SPECS[self.spec_idx].hi_kind == HiKind::PorscheGt3Rs;
        let target_boost = if !self.engine_on || is_na {
            0.0
        } else {
            (self.throttle_cur * 1.48 * (self.rpm / (redline * 0.72)).clamp(0.2, 1.2) - 0.22).clamp(-0.3, 1.5)
        };
        self.boost_bar += (target_boost - self.boost_bar) * (1.0 - (-5.5 * dt).exp());
        if self.engine_on {
            self.fuel = (self.fuel - (0.000018 + 0.00011 * self.throttle_cur) * dt).max(0.0);
            let airflow_cool = (spd.abs() * 0.12).min(12.0);
            let heat_load = 86.0 + 22.0 * self.throttle_cur * (self.rpm / redline).clamp(0.3, 1.1) - airflow_cool;
            self.eng_temp_c += (heat_load.clamp(78.0, 118.0) - self.eng_temp_c) * (dt * 0.12).min(1.0);
        }
    }

    fn ratio(&self) -> f32 {
        let d = SPECS[self.spec_idx].drv;
        if self.gear < 0 {
            -3.35 * d.final_drive
        } else {
            d.gears[(self.gear as usize).min(d.gears.len() - 1)] * d.final_drive
        }
    }

    pub fn cockpit_eye(&self) -> Option<Vec3> {
        let mut e = SPECS[self.spec_idx].eye;
        if let Ok(s) = std::env::var("CAMEYE") { let v: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect(); if v.len() == 3 { e = vec3(v[0], v[1], v[2]); } }
        if e == Vec3::ZERO { return None; }
        let f = self.frame;
        let r = e - self.body.rest_center;
        Some(f.c + f.left * r.x + f.up * r.y + f.fwd * r.z)
    }

    pub fn drv_cfg(&self) -> (f32, f32, f32, f32, i32, f32) {
        let d = SPECS[self.spec_idx].drv;
        (d.idle, d.redline, d.up_rpm, d.down_rpm, d.gears.len() as i32 - 1, d.shift_time)
    }

    fn engine_torque(&self, rpm: f32) -> f32 {
        let d = SPECS[self.spec_idx].drv;
        for w in d.torque.windows(2) {
            if rpm <= w[1].0 {
                let t = (rpm - w[0].0) / (w[1].0 - w[0].0).max(1.0);
                return w[0].1 + (w[1].1 - w[0].1) * t;
            }
        }
        0.0
    }

    pub fn forces(&mut self, h: f32, world: &World) {
        let spec = &SPECS[self.spec_idx];
        let pcfg = self.pcfg;
        let grav = pcfg.gravity;
        let f = self.frame;
        self.body.clear_forces(grav);

        // ---- Aerodynamics (Drag, Speed-Squared Downforce, and Center of Pressure Balance)
        {
            let v = self.body.avg_velocity();
            let sp = v.length();
            let v_fwd = v.dot(f.fwd).max(0.0);
            let rho_air = 1.225f32;
            let cda = spec.drv.cd_a;
            let cla = spec.drv.cl_a;
            let cop_front = spec.drv.cop_front;

            let drag = v * (-0.5 * rho_air * cda * sp);
            let df_total = 0.5 * rho_air * cla * v_fwd * v_fwd;
            let tm = self.body.total_mass.max(1.0);
            let nz = self.body.nz;
            for i in 0..self.body.pos.len() {
                let k_idx = i / (self.body.nx * self.body.ny);
                let is_front_half = k_idx >= nz / 2;
                let cop_weight = if is_front_half { cop_front * 2.0 } else { (1.0 - cop_front) * 2.0 };
                let w_mass = self.body.mass[i] / tm;
                self.body.frc[i] += drag * w_mass - f.up * (df_total * w_mass * cop_weight);
            }
        }

        // ---- Drivetrain: Progressive Clutch Engagement, Gear Shifting & Engine RPM
        let driven: Vec<usize> = (0..4).filter(|&i| match spec.drive {
            Drive::Rwd => !self.wheels[i].front, Drive::Fwd => self.wheels[i].front, Drive::Awd => true }).collect();
        let w_avg: f32 = driven.iter().map(|&i| self.wheels[i].omega).sum::<f32>() / driven.len() as f32;
        let ratio = self.ratio();
        let wheel_rpm = w_avg.abs() * 60.0 / (2.0 * std::f32::consts::PI);
        let (idle, redline, up_rpm, down_rpm, top_gear, shift_time) = self.drv_cfg();
        let k = redline / 7000.0;
        let slip_rpm = |thr: f32, gear: i32| if gear <= 0 { idle + thr * 2800.0 * k } else if gear == 1 { idle + thr * 1900.0 * k } else { idle + thr * 1100.0 * k };

        self.shift_timer = (self.shift_timer - h).max(0.0);
        if self.gear >= 0 && self.shift_timer <= 0.0 {
            let ground_rpm = self.speed.abs() / self.radius * 60.0 / (2.0 * std::f32::consts::PI);
            let wr = ground_rpm * ratio.abs();
            if wr > up_rpm && self.gear < top_gear {
                self.gear += 1;
                self.shift_timer = shift_time;
            } else if wr < down_rpm && self.gear > 0 {
                self.gear -= 1;
                self.shift_timer = shift_time;
            }
        }
        // Clutch engagement factor (0 during shift cut, smoothly ramps to 1.0 as speed increases from launch)
        let launch_clutch = ((self.speed.abs() - 0.5) / 4.5).clamp(0.35 + 0.65 * self.input.throttle, 1.0);
        self.clutch = if self.shift_timer > shift_time * 0.45 { 0.15 } else { launch_clutch };

        let ratio = self.ratio();
        let rpm = if self.engine_on {
            (wheel_rpm * ratio.abs()).max(slip_rpm(self.input.throttle, self.gear))
        } else {
            0.0
        };
        self.rpm = rpm.min(redline + 350.0);
        let mut drive_total = 0.0;
        if self.engine_on && (self.shift_timer <= shift_time * 0.55 || self.gear < 0) {
            let t = if rpm > redline { 0.0 } else { self.engine_torque(rpm) };
            drive_total = self.input.throttle * t * ratio * 0.93 * self.clutch;
        }
        if self.input.throttle < 0.05 && self.gear >= 0 {
            drive_total = -w_avg.signum() * (24.0 + 0.018 * rpm) * ratio.abs() * 0.48;
        }
        let drive_per_wheel = drive_total / driven.len() as f32;

        // ---- Phase 2 Fix: Alignment pull ONLY from permanent plastic crash deformation (never from elastic suspension squat/dive!)
        let front_plastic: f32 = self.wheels[0].mount_ids.iter().chain(self.wheels[1].mount_ids.iter())
            .map(|&id| self.body.dmg[id])
            .fold(0.0f32, f32::max);
        let mut local_off = [Vec3::ZERO; 4];
        for (wi, w) in self.wheels.iter().enumerate() {
            let m = self.body.point(&w.mount_ids, &w.mount_w);
            let l = f.to_local(m);
            let r = w.mount_rest_local - self.body.rest_center;
            local_off[wi] = l - r;
        }
        let dz = local_off[0].z - local_off[1].z;
        let target_toe = if front_plastic > 0.028 {
            ((dz.abs() - 0.055).max(0.0) * dz.signum() * 0.65).clamp(-0.22, 0.22)
        } else {
            0.0
        };
        self.toe_bias += (target_toe - self.toe_bias) * (h * 8.0).min(1.0);
        let toe = self.toe_bias;

        let mut dmg_grip = 1.0f32;
        if front_plastic > 0.03 {
            let total_off: f32 = local_off.iter().map(|o| o.length()).sum();
            dmg_grip -= (total_off * 0.25).min(0.40);
        }

        // ---- Suspension (Spring / Asymmetric Damper / Anti-Roll Bars) + Tire Radial Compression + Pacejka Magic Formula
        let comp: Vec<f32> = self.wheels.iter().map(|w| if w.grounded { self.s_free - w.s } else { 0.0 }).collect();
        let arb_f = 0.56 * self.k_spring;
        let arb_r = 0.44 * self.k_spring;

        for wi in 0..4 {
            let (ids, ws) = (self.wheels[wi].mount_ids, self.wheels[wi].mount_w);
            let m = self.body.point(&ids, &ws);
            let vm = self.body.velocity_at(&ids, &ws);
            let up = f.up;
            let hg = world.ground_h(m.x - up.x * self.s_eq, m.z - up.z * self.s_eq);
            let upy = up.y.max(0.15);
            let g = (m.y - hg) / upy;
            let s_contact = g - self.radius;
            let w = &mut self.wheels[wi];
            let in_reach = s_contact < self.s_max && up.y > 0.1;
            let mut fn_;
            w.grounded = in_reach;
            let s = s_contact.clamp(self.s_min, self.s_max);
            w.s = if in_reach { s } else { self.s_max };
            let mut fx_react = 0.0;

            if in_reach {
                let ds = vm.dot(up);
                let cd = if ds < 0.0 { self.c_damp * 0.88 } else { self.c_damp * 1.28 };
                fn_ = self.k_spring * (self.s_free - s) - cd * ds;
                let other = wi ^ 1;
                fn_ += (if wi < 2 { arb_f } else { arb_r }) * (comp[wi] - comp[other]);
                if s_contact < self.s_min { fn_ += 240_000.0 * (self.s_min - s_contact); }
                fn_ = fn_.max(0.0);
                w.load = fn_;

                let opt_psi = if spec.hi_kind == HiKind::CrafterBus { 95.0 } else { pcfg.default_tire_psi };
                let psi_ratio = (w.pressure_psi / opt_psi).clamp(0.55, 1.45);
                let k_tire = (if spec.hi_kind == HiKind::CrafterBus { 740_000.0 } else { 220_000.0 }) * psi_ratio;
                w.tire_deflection = (fn_ / k_tire).clamp(0.0, self.radius * 0.22);
                let eff_radius = (self.radius - w.tire_deflection * 0.35).max(0.18);

                let n = world.ground_normal(m.x - up.x * g, m.z - up.z * g);
                // Subtle Ackermann geometry: inner front wheel turns ~6% sharper than outer wheel in tight turns
                let ackermann = if w.front {
                    if (w.left && self.input.steer > 0.0) || (!w.left && self.input.steer < 0.0) { 1.04 } else { 0.96 }
                } else { 0.0 };
                let steer = if w.front { self.input.steer * ackermann + toe } else { 0.0 };
                // Smooth wheel steer angle visual & physical transition
                w.steer_angle += (steer - w.steer_angle) * (h * 45.0).min(1.0);
                let (ss, cs) = w.steer_angle.sin_cos();
                let fw = f.fwd * cs + f.left * ss;
                let fw = (fw - n * fw.dot(n)).normalize_or_zero();
                let lat = n.cross(fw);
                let vlong = vm.dot(fw);
                let vlat = vm.dot(lat);
                let denom = vlong.abs().max(2.8);
                let kappa_raw = (w.omega * eff_radius - vlong) / denom;
                let alpha_raw = (vlat / denom).atan();

                // Speed-adaptive tire relaxation length filter (eliminates low-speed lateral oscillation)
                let rel_alpha = (h * (vlong.abs() + 1.8) / 0.42).min(1.0);
                let rel_kappa = (h * (vlong.abs() + 2.5) / 0.28).min(1.0);
                w.ka += (alpha_raw - w.ka) * rel_alpha;
                w.kk += (kappa_raw - w.kk) * rel_kappa;
                let (kappa, alpha) = (w.kk, w.ka);
                w.slip_ratio = kappa;
                w.slip_angle_deg = alpha.to_degrees();

                // Surface classification: 0=Dry Asphalt, 1=Wet Asphalt, 2=Dirt/Gravel, 3=Grass
                let (surf_id, surf_mu, roll_res) = if let Some(sid) = crate::world::terrain_surface(m.x, m.z) {
                    match sid {
                        0 => (0u8, pcfg.mu_asphalt_dry, 0.012),
                        2 => (2u8, pcfg.mu_dirt, 0.028),
                        _ => (3u8, pcfg.mu_grass, 0.042),
                    }
                } else if m.x.abs() < world.arena && m.z.abs() < world.arena {
                    if m.x > 60.0 && m.x < 95.0 && m.z > -95.0 && m.z < -60.0 {
                        (2u8, pcfg.mu_dirt, 0.028)
                    } else {
                        (0u8, pcfg.mu_asphalt_dry, 0.012)
                    }
                } else {
                    (3u8, pcfg.mu_grass, 0.042)
                };
                w.surface_id = surf_id;
                // الماء: يقل التماسك (hydroplaning) وتزداد مقاومة الدحرجة عند غمر العجلة
                let wet_d = ((crate::world::water_level() - m.y).max(0.0) / 0.25).min(1.0);
                let surf_mu = surf_mu * (1.0 - 0.8 * wet_d);
                let roll_res = roll_res + 0.35 * wet_d;

                let psi_err = (psi_ratio - 1.0).abs();
                let psi_grip_mod = (1.0 - 0.18 * psi_err).clamp(0.82, 1.0);
                let temp_opt = 1.0 - ((w.temp_c - 86.0).abs() / 95.0).powi(2) * 0.14;
                let wear_mod = (1.0 - 0.28 * w.wear).clamp(0.70, 1.0);
                let nominal = self.body.total_mass * grav / 4.0;
                let load_sens = (1.0 - 0.14 * (fn_ / nominal - 1.0)).clamp(0.62, 1.12);

                let mu = surf_mu * spec.grip * dmg_grip.max(0.4) * load_sens * psi_grip_mod * temp_opt.clamp(0.84, 1.03) * wear_mod;

                let sx = kappa / 0.12;
                let sa = alpha / if w.front { 0.155 } else { 0.125 };
                let rho = (sx * sx + sa * sa).sqrt();
                let mut fxw = 0.0;
                let mut fyw = 0.0;
                if rho > 1e-5 {
                    let fx_pure = pacejka_magic(kappa, pcfg.pacejka_bx, pcfg.pacejka_cx, pcfg.pacejka_ex);
                    let fy_pure = pacejka_magic(alpha, pcfg.pacejka_by, pcfg.pacejka_cy, pcfg.pacejka_ey);
                    let gf_comb = pacejka_magic(rho * 0.12, pcfg.pacejka_bx, pcfg.pacejka_cx, pcfg.pacejka_ex).abs();
                    let blend_mag = (fx_pure.hypot(fy_pure) * 0.5 + gf_comb * 0.5).min(1.05);
                    fxw = mu * fn_ * blend_mag * (sx / rho);
                    fyw = -mu * fn_ * blend_mag * (sa / rho);
                }
                let eff_roll = roll_res * (1.0 + 0.35 * (1.0 - psi_ratio).max(0.0));
                fxw -= eff_roll * fn_ * (vlong * 2.0).tanh();

                let slip_speed = (w.omega * eff_radius - vlong).hypot(vlat);
                let slip_power = (fxw.hypot(fyw)) * slip_speed;
                let heat_in = slip_power * 0.000018;
                let cool_out = (w.temp_c - 26.0) * (0.015 + 0.0025 * vlong.abs());
                w.temp_c = (w.temp_c + (heat_in - cool_out) * h).clamp(24.0, 165.0);
                w.wear = (w.wear + slip_power * 1.2e-9 * h).min(1.0);

                // Smooth lateral stability clamp + lateral velocity damping (prevents front-wheel micro-jitter)
                let mloc: f32 = ids.iter().map(|&i| self.body.mass[i]).sum();
                let lim = 0.56 * mloc / h;
                fyw = fyw.clamp(-lim * vlat.abs(), lim * vlat.abs());
                let low_speed = (1.0 - vlong.abs() / 10.0).clamp(0.0, 1.0);
                fyw -= vlat * (mloc * 2.4).min(1200.0) * (0.12 + 0.88 * low_speed);

                let total = up * fn_ + fw * fxw + lat * fyw;
                for n_ in 0..8 { self.body.frc[ids[n_]] += total * ws[n_]; }
                fx_react = fxw;
                w.slip = rho;
            } else {
                w.load = 0.0;
                w.tire_deflection = 0.0;
                w.steer_angle += (if w.front { self.input.steer } else { 0.0 } - w.steer_angle) * (h * 35.0).min(1.0);
                w.slip = 0.0;
                w.slip_ratio = 0.0;
                w.slip_angle_deg = 0.0;
                w.temp_c = (w.temp_c - (w.temp_c - 26.0) * 0.01 * h).max(24.0);
            }

            let mut drv = if driven.contains(&wi) { drive_per_wheel } else { 0.0 };
            if self.tc && drv > 0.0 {
                let sl = self.wheels[wi].slip;
                drv *= ((1.55 - sl) / 0.75).clamp(0.12, 1.0);
            }
            let w = &mut self.wheels[wi];
            let wheel_inertia = if spec.hi_kind == HiKind::CrafterBus { 12.5 } else { WHEEL_I };
            w.omega += (drv - fx_react * self.radius) / wheel_inertia * h;

            let mut bt = 0.0;
            if self.input.brake > 0.0 {
                let (bf, br) = (spec.drv.brake_f, spec.drv.brake_r);
                // Brake fade when rotor temperature exceeds 620 °C
                let fade = if w.brake_temp_c > 620.0 {
                    (1.0 - (w.brake_temp_c - 620.0) / 500.0).clamp(0.55, 1.0)
                } else {
                    1.0
                };
                let mut b = self.input.brake * (if w.front { bf } else { br }) * fade;
                // EBD: توزيع الكبح حسب الحمل الفعلي على كل عجلة (العجلات الخلفية تخف عند الفرملة فتقل قوة كبحها)
                let static_load = (self.body.total_mass * 9.81 / 4.0).max(1.0);
                let ebd = (w.load / static_load).clamp(0.55, 1.25);
                b *= 0.5 + 0.5 * ebd;
                if self.abs {
                    let k = w.kk;
                    if k < -0.15 && w.grounded {
                        b *= (1.0 - (-k - 0.15) * 7.0).clamp(0.18, 1.0);
                        w.abs_active = true;
                    } else {
                        w.abs_active = false;
                    }
                }
                bt += b;
                // Brake rotor heating under braking power
                let brk_power = b * w.omega.abs();
                w.brake_temp_c = (w.brake_temp_c + brk_power * 0.000045 * h).min(950.0);
            } else {
                w.abs_active = false;
            }
            // Brake rotor convective cooling
            let cool_b = (w.brake_temp_c - 35.0) * (0.012 + 0.003 * self.speed.abs());
            w.brake_temp_c = (w.brake_temp_c - cool_b * h).max(35.0);

            if self.input.handbrake && !w.front { bt += if spec.hi_kind == HiKind::CrafterBus { 19500.0 } else { 4200.0 }; }
            if self.asleep { bt += 4500.0; }
            if bt > 0.0 {
                let dw = bt / wheel_inertia * h;
                if w.omega.abs() <= dw { w.omega = 0.0 } else { w.omega -= w.omega.signum() * dw }
            }
            w.omega *= 1.0 - 0.15 * h;
            w.spin += w.omega * h;
        }

        self.collide_world(h, world);
        self.apply_water(h);
        self.body.beam_forces();
    }

    /// فيزياء الماء: طفو + مقاومة لزجة/تربيعية على العقد المغمورة، وانقطاع الدخان عند غمر المحرك.
    fn apply_water(&mut self, h: f32) {
        let wl = crate::world::water_level();
        if wl < -1.0e8 { return; }
        let n = self.body.pos.len();
        if n == 0 { return; }
        let mut sum_y = 0.0f32;
        for i in 0..n {
            sum_y += self.body.pos[i].y;
            let y = self.body.pos[i].y;
            if y < wl {
                let d = ((wl - y) / 0.4).min(1.0);
                let m = self.body.mass[i];
                // الكثافة الفعالة للسيارة أعلى بقليل من الماء بعد امتلاء الهيكل، فتطفو لحظات ثم تغرق ببطء
                self.body.frc[i].y += m * 9.81 * 0.92 * d;
                let v = self.body.vel[i];
                let drag = m * (5.0 + 2.2 * v.length()) * d;
                self.body.frc[i] -= v * drag;
            }
        }
        let _ = h;
        // المحرك يتوقف إذا غمر الماء منتصف الهيكل
        if sum_y / (n as f32) < wl - 0.25 {
            self.input.throttle = 0.0;
        }
    }

    fn collide_world(&mut self, h: f32, world: &World) {
        let r = 0.10f32;
        let kc = 2.6e5f32;
        let (amin, amax) = self.aabb;
        let mut cand: Vec<usize> = Vec::new();
        let mut grid_c: Vec<usize> = Vec::new();
        world.grid.query(amin - Vec3::splat(0.6), amax + Vec3::splat(0.6), &mut grid_c);
        for &bi in grid_c.iter() {
            let b = &world.boxes[bi];
            let (bmin, bmax) = b.aabb();
            if bmax.x + 0.5 < amin.x || bmin.x - 0.5 > amax.x || bmax.z + 0.5 < amin.z || bmin.z - 0.5 > amax.z || bmax.y + 0.5 < amin.y || bmin.y - 0.5 > amax.y { continue; }
            cand.push(bi);
        }
        if self.grace > 0.0 {
            self.grace -= h;
            cand.clear();
        }
        for si in 0..self.body.surface.len() {
            let i = self.body.surface[si];
            let p = self.body.pos[i];
            let v = self.body.vel[i];
            let m = self.body.mass[i];
            let gh = world.ground_h(p.x, p.z);
            let pen = gh + r - p.y;
            if pen > 0.0 {
                let n = world.ground_normal(p.x, p.z);
                let vn = v.dot(n);
                let cc = 380.0;
                let fnm = (kc * pen - cc * vn).max(0.0);
                let vt = v - n * vn;
                let sp = vt.length();
                let mut ff = Vec3::ZERO;
                if sp > 1e-4 {
                    let mu = 0.55;
                    let mag = (mu * fnm).min(m * sp / h * 0.5);
                    ff = -vt / sp * mag;
                }
                self.body.frc[i] += n * fnm + ff;
            }
            for &bi in &cand {
                let b = &world.boxes[bi];
                let l = b.local(p);
                let dx = b.h.x + r - l.x.abs();
                let dy = b.h.y + r - l.y.abs();
                let dz = b.h.z + r - l.z.abs();
                if dx <= 0.0 || dy <= 0.0 || dz <= 0.0 { continue; }
                let (d, nl) = if dx <= dy && dx <= dz { (dx, vec3(l.x.signum(), 0.0, 0.0)) }
                    else if dy <= dz { (dy, vec3(0.0, l.y.signum(), 0.0)) }
                    else { (dz, vec3(0.0, 0.0, l.z.signum())) };
                let n = b.to_world_dir(nl);
                let vn = v.dot(n);
                let fnm = (kc * d - 420.0 * vn).max(0.0);
                let vt = v - n * vn;
                let sp = vt.length();
                let mut ff = Vec3::ZERO;
                if sp > 1e-4 { ff = -vt / sp * (0.35 * fnm).min(m * sp / h * 0.5); }
                self.body.frc[i] += n * fnm + ff;
            }
        }
    }

    pub fn integrate(&mut self, h: f32) {
        self.body.integrate(h);
        self.render_dirty = true;
    }

    pub fn update_render_visual(&mut self) {
        self.render_dirty = true;
        self.update_render_inner(false);
    }

    pub fn update_render(&mut self) {
        if self.asleep && !self.render_dirty {
            return;
        }
        self.update_render_inner(true);
        self.render_dirty = false;
    }

    fn update_render_inner(&mut self, refresh_damage: bool) {
        if refresh_damage {
            self.body.update_damage();
        }
        let n = self.mesh_src.pos.len();
        if self.scratch_pos.len() != n {
            self.scratch_pos.resize(n, Vec3::ZERO);
            self.scratch_nrm.resize(n, Vec3::ZERO);
        }
        for i in 0..n {
            let (ids, ws) = (&self.bind_ids[i], &self.bind_w[i]);
            self.scratch_pos[i] = self.body.point(ids, ws);
            self.scratch_nrm[i] = Vec3::ZERO;
        }
        for t in self.mesh_src.idx.chunks(3) {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let fnv = (self.scratch_pos[b] - self.scratch_pos[a]).cross(self.scratch_pos[c] - self.scratch_pos[a]);
            self.scratch_nrm[a] += fnv;
            self.scratch_nrm[b] += fnv;
            self.scratch_nrm[c] += fnv;
        }
        for i in 0..n {
            let nn = self.scratch_nrm[i].normalize_or_zero();
            let (ids, ws) = (&self.bind_ids[i], &self.bind_w[i]);
            let mut d = 0.0;
            for k in 0..8 { d += self.body.dmg[ids[k]] * ws[k]; }
            let dark = 1.0 - (d * 7.0).min(0.62);
            let v = &mut self.render.vertices[i];
            v.position = self.scratch_pos[i];
            let spec_mode = if d > 0.08 { 0.45 } else { 2.22 };
            v.normal = vec4(nn.x, nn.y, nn.z, spec_mode);
            let c = (255.0 * dark) as u8;
            let tint = (255.0 * (dark * (1.0 - (d * 3.0).min(0.2)))) as u8;
            v.color = [c, tint, tint, 255];
        }
        for wi in 0..4 {
            let (hub, left, up, fwd) = self.wheel_world_pose(wi);
            let w = &self.wheels[wi];
            let (sp, cp) = w.spin.sin_cos();
            let rad_scale = ((self.radius - w.tire_deflection * 0.5) / self.radius.max(0.1)).clamp(0.75, 1.05);
            let wm = &mut self.wheel_render[wi];
            for i in 0..w.mesh.pos.len() {
                let p = w.mesh.pos[i] * vec3(1.0, rad_scale, rad_scale);
                let n = w.mesh.nrm[i];
                let rot = |q: Vec3| -> Vec3 {
                    let y = q.y * cp - q.z * sp;
                    let z = q.y * sp + q.z * cp;
                    left * q.x + up * y + fwd * z
                };
                let wp = hub + rot(p);
                let wn = rot(n);
                wm.vertices[i].position = wp;
                wm.vertices[i].normal = vec4(wn.x, wn.y, wn.z, -1.0);
            }
        }
        if self.hi.is_some() { hicar::update_deform(self); }
    }

    pub fn wheel_world_pose(&self, wi: usize) -> (Vec3, Vec3, Vec3, Vec3) {
        let f = self.frame;
        let w = &self.wheels[wi];
        let m = self.body.point(&w.mount_ids, &w.mount_w);
        let hub = m - f.up * (w.s - w.tire_deflection * 0.45);
        let (ss, cs) = w.steer_angle.sin_cos();
        let fwd = f.fwd * cs + f.left * ss;
        let left = f.left * cs - f.fwd * ss;
        (hub, left, f.up, fwd)
    }

    pub fn wheel_meshes(&self, out: &mut Vec<Mesh>, _texture: &Texture2D) {
        for wm in &self.wheel_render {
            out.push(Mesh {
                vertices: wm.vertices.clone(),
                indices: wm.indices.clone(),
                texture: wm.texture.clone(),
            });
        }
    }
}

pub fn collide_into(a: &mut Car, b: &mut Car, h: f32) {
    let r = 0.10f32;
    let kc = 1.6e5f32;
    let fb = b.frame;
    let (bmin, bmax) = (b.bounds_min - Vec3::splat(r), b.bounds_max + Vec3::splat(r));
    for si in 0..a.body.surface.len() {
        let i = a.body.surface[si];
        let p = a.body.pos[i];
        let l = fb.to_local(p);
        let dx0 = l.x - bmin.x; let dx1 = bmax.x - l.x;
        let dy0 = l.y - bmin.y; let dy1 = bmax.y - l.y;
        let dz0 = l.z - bmin.z; let dz1 = bmax.z - l.z;
        if dx0 <= 0.0 || dx1 <= 0.0 || dy0 <= 0.0 || dy1 <= 0.0 || dz0 <= 0.0 || dz1 <= 0.0 { continue; }
        let cands = [(dx0, vec3(-1.0, 0.0, 0.0)), (dx1, vec3(1.0, 0.0, 0.0)), (dy0, vec3(0.0, -1.0, 0.0)), (dy1, vec3(0.0, 1.0, 0.0)), (dz0, vec3(0.0, 0.0, -1.0)), (dz1, vec3(0.0, 0.0, 1.0))];
        let mut best = cands[0];
        for c in cands.iter() { if c.0 < best.0 { best = *c; } }
        let (d, nl) = best;
        let n = fb.to_world_dir(nl);
        let (ids, ws) = b.body.trilinear(l + b.body.rest_center);
        let vb = b.body.velocity_at(&ids, &ws);
        let va = a.body.vel[i];
        let rel = va - vb;
        let vn = rel.dot(n);
        let fnm = (kc * d - 350.0 * vn).max(0.0);
        let vt = rel - n * vn;
        let sp = vt.length();
        let mut ff = Vec3::ZERO;
        if sp > 1e-4 { ff = -vt / sp * (0.35 * fnm).min(a.body.mass[i] * sp / h * 0.25); }
        let ftot = n * fnm + ff;
        a.body.frc[i] += ftot;
        b.body.apply(&ids, &ws, -ftot);
    }
}
