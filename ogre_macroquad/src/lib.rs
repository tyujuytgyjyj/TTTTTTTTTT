pub mod ffi;
pub mod text_ui;

pub use glam::{
    mat3, mat4, quat, vec2, vec3, vec4, Affine3A, EulerRot, Mat3, Mat4, Quat, Vec2, Vec3, Vec4,
};

pub use ffi::{KeyCode, MouseButton}; use ffi::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_void, CString};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
    pub fn to_rgba8(self) -> (u8, u8, u8, u8) {
        (
            (self.r.clamp(0.0, 1.0) * 255.0) as u8,
            (self.g.clamp(0.0, 1.0) * 255.0) as u8,
            (self.b.clamp(0.0, 1.0) * 255.0) as u8,
            (self.a.clamp(0.0, 1.0) * 255.0) as u8,
        )
    }
}

pub const WHITE: Color = Color::new(1.0, 1.0, 1.0, 1.0);
pub const BLACK: Color = Color::new(0.0, 0.0, 0.0, 1.0);
pub const RED: Color = Color::new(0.95, 0.2, 0.2, 1.0);
pub const GREEN: Color = Color::new(0.2, 0.9, 0.3, 1.0);
pub const BLUE: Color = Color::new(0.2, 0.4, 0.95, 1.0);
pub const YELLOW: Color = Color::new(0.98, 0.88, 0.15, 1.0);
pub const GRAY: Color = Color::new(0.5, 0.5, 0.5, 1.0);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Linear,
    Nearest,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

#[derive(Clone)]
pub struct Image {
    pub bytes: Vec<u8>,
    pub width: u16,
    pub height: u16,
}

impl Image {
    pub fn from_file_with_format(bytes: &[u8], _fmt: Option<ImageFormat>) -> Result<Self, String> {
        let decoded = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
        let (src_w, src_h) = decoded.dimensions();
        if src_w == 0 || src_h == 0 {
            return Err("decoded image has zero dimensions".into());
        }
        let scale = (4096.0f64 / src_w.max(src_h) as f64).min(1.0);
        let (w, h, rgba) = if scale < 1.0 {
            let nw = ((src_w as f64 * scale).round() as u32).max(1);
            let nh = ((src_h as f64 * scale).round() as u32).max(1);
            (nw, nh, image::imageops::resize(&decoded, nw, nh, image::imageops::FilterType::Lanczos3))
        } else {
            (src_w, src_h, decoded)
        };
        Ok(Self { bytes: rgba.into_raw(), width: w as u16, height: h as u16 })
    }
    pub fn width(&self) -> usize { self.width as usize }
    pub fn height(&self) -> usize { self.height as usize }
    pub fn gen_image_color(width: u16, height: u16, color: Color) -> Self {
        let (r, g, b, a) = color.to_rgba8();
        let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            bytes.push(r);
            bytes.push(g);
            bytes.push(b);
            bytes.push(a);
        }
        Self { bytes, width, height }
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width as u32 && y < self.height as u32 {
            let idx = ((y * self.width as u32 + x) * 4) as usize;
            let (r, g, b, a) = color.to_rgba8();
            if let Some(s) = self.bytes.get_mut(idx..idx + 4) {
                s.copy_from_slice(&[r, g, b, a]);
            }
        }
    }

    pub fn export_png(&self, path: &str) {
        let _ = image::save_buffer(
            path,
            &self.bytes,
            self.width as u32,
            self.height as u32,
            image::ColorType::Rgba8,
        );
    }
}

#[derive(Clone)]
pub struct Texture2D {
    pub id: u32,
    pub width: f32,
    pub height: f32,
}

impl Texture2D {
    pub fn empty() -> Self {
        Self { id: 1, width: 1.0, height: 1.0 }
    }

    pub fn from_rgba8(width: u16, height: u16, bytes: &[u8]) -> Self {
        let expected = width as usize * height as usize * 4;
        if width == 0 || height == 0 || bytes.len() != expected {
            return Self::empty();
        }
        let id = with_ctx(|ctx| {
            if ctx.is_null() {
                1
            } else {
                unsafe { ogre_create_texture(ctx, width as u32, height as u32, bytes.as_ptr(), true) }
            }
        });
        Self { id, width: width as f32, height: height as f32 }
    }

    pub fn from_image(img: &Image) -> Self {
        Self::from_rgba8(img.width, img.height, &img.bytes)
    }

    pub fn from_file_with_format(bytes: &[u8], fmt: Option<ImageFormat>) -> Self {
        match Image::from_file_with_format(bytes, fmt) {
            Ok(img) => Self::from_rgba8(img.width, img.height, &img.bytes),
            Err(_) => Self::empty(),
        }
    }

    pub fn set_filter(&self, filter: FilterMode) {
        with_ctx(|ctx| {
            if !ctx.is_null() {
                unsafe { ogre_set_texture_filter(ctx, self.id, matches!(filter, FilterMode::Linear)); }
            }
        });
    }
    pub fn width(&self) -> f32 { self.width }
    pub fn height(&self) -> f32 { self.height }
    pub fn raw_miniquad_id(&self) -> miniquad::TextureId { miniquad::TextureId(self.id) }
    pub fn get_texture_data(&self) -> Image {
        Image::gen_image_color(self.width.max(1.0) as u16, self.height.max(1.0) as u16, BLACK)
    }
}

#[derive(Clone, Default)]
pub struct DrawTextureParams {
    pub dest_size: Option<Vec2>,
    pub source: Option<Rect>,
    pub rotation: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    pub pivot: Option<Vec2>,
}

#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
}

#[derive(Clone)]
pub struct RenderTarget {
    pub texture: Texture2D,
}

#[derive(Clone, Copy)]
pub struct RenderTargetParams {
    pub sample_count: i32,
    pub depth: bool,
}

pub fn render_target(width: u32, height: u32) -> RenderTarget {
    RenderTarget {
        texture: Texture2D { id: 1, width: width as f32, height: height as f32 },
    }
}

pub fn render_target_ex(width: u32, height: u32, _params: RenderTargetParams) -> RenderTarget {
    render_target(width, height)
}

#[derive(Clone, Default)]
pub struct Camera2D {
    pub rotation: f32,
    pub zoom: Vec2,
    pub target: Vec2,
    pub offset: Vec2,
    pub render_target: Option<RenderTarget>,
}

pub fn set_camera(cam: &Camera2D) {
    STATE.with(|st| {
        st.borrow_mut().offscreen_skip = cam.render_target.is_some();
    });
}

pub fn set_default_camera() {
    STATE.with(|st| {
        st.borrow_mut().offscreen_skip = false;
    });
}

#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: Vec3,
    pub uv: Vec2,
    pub color: [u8; 4],
    pub normal: Vec4,
}

impl Vertex {
    pub fn new(x: f32, y: f32, z: f32, u: f32, v: f32, color: Color) -> Self {
        let (r, g, b, a) = color.to_rgba8();
        Self {
            position: vec3(x, y, z),
            uv: vec2(u, v),
            color: [r, g, b, a],
            normal: vec4(0.0, 1.0, 0.0, 0.0),
        }
    }
}

#[derive(Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    pub texture: Option<Texture2D>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UniformType {
    Float1,
    Float2,
    Float3,
    Float4,
    Int1,
    Mat4,
}

pub enum ShaderSource<'a> {
    Glsl { vertex: &'a str, fragment: &'a str },
}

pub struct MaterialParams {
    pub pipeline_params: miniquad::PipelineParams,
    pub uniforms: Vec<miniquad::UniformDesc>,
    pub textures: Vec<String>,
}

static NEXT_MAT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct Material {
    pub id: u64,
    pub is_sky: bool,
    pub is_shadow_or_post: bool,
    pub is_glass: bool,
    pub is_water: bool,
    pub model_mat: Rc<RefCell<Mat4>>,
    pub car_mat: Rc<RefCell<Mat4>>,
    pub tint: Rc<RefCell<Vec3>>,
    pub vp: Rc<RefCell<Mat4>>,
    pub cam_pos: Rc<RefCell<Vec3>>,
    pub sun_dir: Rc<RefCell<Vec3>>,
    pub fog_col: Rc<RefCell<Vec3>>,
    pub sun_elev: Rc<RefCell<f32>>,
}

pub trait UniformVal {
    fn apply(&self, name: &str, mat: &Material);
}

impl UniformVal for Mat4 {
    fn apply(&self, name: &str, mat: &Material) {
        match name {
            "MODEL" => *mat.model_mat.borrow_mut() = *self,
            "CarM" => *mat.car_mat.borrow_mut() = *self,
            "VP" => *mat.vp.borrow_mut() = *self,
            _ => {}
        }
    }
}

impl UniformVal for Vec3 {
    fn apply(&self, name: &str, mat: &Material) {
        match name {
            "tint" => *mat.tint.borrow_mut() = *self,
            "cam_pos" => *mat.cam_pos.borrow_mut() = *self,
            "sun_dir" => *mat.sun_dir.borrow_mut() = *self,
            "fog_col" => *mat.fog_col.borrow_mut() = *self,
            _ => {}
        }
    }
}

impl UniformVal for Vec4 {
    fn apply(&self, name: &str, mat: &Material) {
        if name == "misc" {
            *mat.sun_elev.borrow_mut() = self.y;
        }
    }
}

impl UniformVal for f32 {
    fn apply(&self, _name: &str, _mat: &Material) {}
}

impl UniformVal for (f32, f32, f32, f32) {
    fn apply(&self, _name: &str, _mat: &Material) {}
}

impl Material {
    pub fn set_uniform<T: UniformVal>(&self, name: &str, val: T) {
        val.apply(name, self);
        if name == "VP" || name == "fog_col" || name == "sun_dir" {
            sync_active_camera_and_env(self);
        }
    }
    pub fn set_texture(&self, _name: &str, _tex: Texture2D) {}
}

pub fn load_material(src: ShaderSource<'_>, params: MaterialParams) -> Result<Material, String> {
    let ShaderSource::Glsl { vertex, fragment } = src;
    let is_sky = fragment.contains("invVP") || vertex.contains("invVP");
    let is_shadow_or_post = vertex.contains("SVP") || fragment.contains("pp_params");
    // Identify water separately from vehicle glass so the Ogre bridge can select
    // a dedicated water PBS datablock.
    let is_water = vertex.contains("varying highp vec3 wp") && fragment.contains("vec2 p = wp.xz");
    let is_glass = !is_water && !params.pipeline_params.depth_write && params.pipeline_params.color_blend.is_some() && !is_sky && !is_shadow_or_post;
    Ok(Material {
        id: NEXT_MAT_ID.fetch_add(1, Ordering::Relaxed),
        is_sky,
        is_shadow_or_post,
        is_glass,
        is_water,
        model_mat: Rc::new(RefCell::new(Mat4::IDENTITY)),
        car_mat: Rc::new(RefCell::new(Mat4::IDENTITY)),
        tint: Rc::new(RefCell::new(Vec3::ONE)),
        vp: Rc::new(RefCell::new(Mat4::IDENTITY)),
        cam_pos: Rc::new(RefCell::new(Vec3::ZERO)),
        sun_dir: Rc::new(RefCell::new(vec3(0.4, 0.8, 0.3))),
        fog_col: Rc::new(RefCell::new(vec3(0.52, 0.72, 0.95))),
        sun_elev: Rc::new(RefCell::new(0.6)),
    })
}

pub fn gl_use_material(mat: &Material) {
    STATE.with(|st| {
        st.borrow_mut().active_mat = Some(mat.clone());
    });
    sync_active_camera_and_env(mat);
}

pub fn gl_use_default_material() {
    STATE.with(|st| {
        st.borrow_mut().active_mat = None;
    });
}

pub fn gl_set_drawcall_buffer_capacity(_verts: usize, _indices: usize) {}

fn sync_active_camera_and_env(mat: &Material) {
    if mat.is_sky || mat.is_shadow_or_post {
        return;
    }
    STATE.with(|st| {
        let s = st.borrow();

if s.ctx.is_null() || s.offscreen_skip {
return;
}
let sun = *mat.sun_dir.borrow();
let fog = *mat.fog_col.borrow();
let elev = *mat.sun_elev.borrow();
unsafe {
ogre_set_env(s.ctx, sun.x, sun.y, sun.z, fog.x, fog.y, fog.z, elev);
}
let vp = *mat.vp.borrow();
if vp != Mat4::IDENTITY {
// Ø·Â§Ø·Â³Ø·Ú¾Ø·Â®Ø·Â±Ø·Â§Ø·Â¬ FOV Ø¸ËØ¸â¦Ø·ÂµØ¸Ù¾Ø¸ËØ¸Ù¾Ø·Â© Ø·Â§Ø¸âØ·Â±Ø·Â¤Ø¸Ù¹Ø·Â© Ø¸â¦Ø¸â  Ø¸â¦Ø·ÂµØ¸Ù¾Ø¸ËØ¸Ù¾Ø·Â© VP Ø·Â§Ø¸âØ·Â®Ø·Â§Ø·ÂµØ·Â© Ø·Â¨Ø¸â¬ APEX.drive
let cp = *mat.cam_pos.borrow();
let inv_vp = vp.inverse();
let far_ndc = inv_vp.project_point3(vec3(0.0, 0.0, 0.5));
let fwd = (far_ndc - cp).normalize_or_zero();
if fwd.length_squared() > 0.1 {
let view = Mat4::look_to_rh(cp, fwd, Vec3::Y);
let proj = vp * view.inverse();
let m11 = proj.col(1).y.abs().max(0.2);
let fov_rad = 2.0 * (1.0 / m11).atan();
// ÙÙØ¶Ø¹ ÙØ¯ÙØ±Ø§Ù Ø§ÙÙØ§ÙÙØ±Ø§ ÙÙ Ø§ÙØ¹Ø§ÙÙ (ÙØ¹ÙÙØ³ ÙØµÙÙÙØ© View)
let cam_world = view.inverse();
let (_, rot, pos) = cam_world.to_scale_rotation_translation();
unsafe {
ogre_set_camera_pose(
s.ctx,
pos.x, pos.y, pos.z,
rot.x, rot.y, rot.z, rot.w,
fov_rad, 0.12, 2200.0,
);
}
}
}
});
}
struct CachedGpuMesh {
mesh_id: u32,
vertex_count: usize,
index_count: usize,
fingerprint: u64,
texture_id: u32,
is_glass: bool,
is_water: bool,
}

// Hash every geometry attribute, not just vector lengths or a pair of sampled positions.
// Mesh vertices are mutated in-place by the soft-body renderer, so pointer/length checks are insufficient.
fn mesh_fingerprint(mesh: &Mesh, texture_id: u32, is_glass: bool) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    };
    mix(texture_id as u64);
    mix(is_glass as u64);
    mix(mesh.vertices.len() as u64);
    mix(mesh.indices.len() as u64);
    for v in &mesh.vertices {
        for f in [v.position.x, v.position.y, v.position.z, v.normal.x, v.normal.y, v.normal.z, v.normal.w, v.uv.x, v.uv.y] {
            mix(f.to_bits() as u64);
        }
        mix(u32::from_le_bytes([v.color[0], v.color[1], v.color[2], v.color[3]]) as u64);
    }
    for &i in &mesh.indices { mix(i as u64); }
    h
}

fn decompose_affine_for_trs(m: Mat4) -> Option<(Vec3, Quat, Vec3)> {
    let pos = m.w_axis.truncate();
    let mut x = m.x_axis.truncate();
    let y = m.y_axis.truncate();
    let z = m.z_axis.truncate();
    let mut sx = x.length();
    let sy = y.length();
    let sz = z.length();
    if !pos.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite()
        || sx < 1.0e-6 || sy < 1.0e-6 || sz < 1.0e-6 {
        return None;
    }
    // Keep reflections in the scale vector while giving the quaternion a right-handed basis.
    if Mat3::from_cols(x, y, z).determinant() < 0.0 {
        sx = -sx;
        x = -x;
    }
    let rx = x.normalize();
    let y0 = y / sy;
    let ry = (y0 - rx * rx.dot(y0)).normalize_or_zero();
    if ry.length_squared() < 0.5 { return None; }
    let rz = rx.cross(ry).normalize_or_zero();
    if rz.length_squared() < 0.5 { return None; }
    let rot = Quat::from_mat3(&Mat3::from_cols(rx, ry, rz)).normalize();
    if !rot.is_finite() { return None; }
    Some((Vec3::new(sx, sy, sz), rot, pos))
}
struct EngineState {
ctx: *mut c_void,
win_w: i32,
win_h: i32,
active_mat: Option<Material>,
offscreen_skip: bool,
mesh_cache: HashMap<usize, CachedGpuMesh>,
start_time: Instant,
last_frame: Instant,
dt: f32,
fps: i32,
keys_down: HashSet<KeyCode>,
keys_prev: HashSet<KeyCode>,
mouse_pos: (f32, f32),
mouse_prev: (f32, f32),
wheel_y: f32,
mb_down: (bool, bool, bool),
mb_prev: (bool, bool, bool),
}
thread_local! {
static STATE: RefCell<EngineState> = RefCell::new(EngineState {
ctx: std::ptr::null_mut(),
win_w: 1280,
win_h: 720,
active_mat: None,
offscreen_skip: false,
mesh_cache: HashMap::new(),
start_time: Instant::now(),
last_frame: Instant::now(),
dt: 1.0 / 60.0,
fps: 60,
keys_down: HashSet::new(),
keys_prev: HashSet::new(),
mouse_pos: (0.0, 0.0),
mouse_prev: (0.0, 0.0),
wheel_y: 0.0,
mb_down: (false, false, false),
mb_prev: (false, false, false),
});
}
fn with_ctx<R>(f: impl FnOnce(*mut c_void) -> R) -> R {
STATE.with(|st| f(st.borrow().ctx))
}
pub fn draw_mesh(mesh: &Mesh) {
if mesh.vertices.is_empty() || mesh.indices.is_empty() {
return;
}
STATE.with(|st| {
let mut s = st.borrow_mut();
if s.ctx.is_null() || s.offscreen_skip {
return;
}
// Ø·Â¥Ø·Â°Ø·Â§ Ø¸âØ¸â¦ Ø·Ú¾Ø¸ÆØ¸â  Ø¸â¡Ø¸â Ø·Â§Ø¸Æ Ø¸â¦Ø·Â§Ø·Â¯Ø·Â© Ø·Â«Ø¸âØ·Â§Ø·Â«Ø¸Ù¹Ø·Â© Ø·Â§Ø¸âØ·Â£Ø·Â¨Ø·Â¹Ø·Â§Ø·Â¯ Ø¸â¦Ø¸Ù¾Ø·Â¹Ø¸âØ·Â©Ø·Å Ø¸Ù¾Ø¸â¡Ø·Â°Ø·Â§ Ø·Â±Ø·Â³Ø¸â¦ Ø¸ËØ·Â§Ø·Â¬Ø¸â¡Ø·Â© Ø·Â«Ø¸â Ø·Â§Ø·Â¦Ø¸Ù¹Ø·Â© Ø·Â§Ø¸âØ·Â£Ø·Â¨Ø·Â¹Ø·Â§Ø·Â¯ (Ø¸â¦Ø·Â«Ø¸â grad_rect Ø¸Ù¾Ø¸Ù¹ Ø·Â§Ø¸âØ¸âØ¸ËØ·Â§Ø·Â¦Ø¸â¦)
let Some(mat) = s.active_mat.clone() else {
let mut v2d = Vec::with_capacity(mesh.vertices.len());
for v in &mesh.vertices {
let r = *v.color.first().unwrap_or(&255);
let g = *v.color.get(1).unwrap_or(&255);
let b = *v.color.get(2).unwrap_or(&255);
let a = *v.color.get(3).unwrap_or(&255);
v2d.push(BridgeVertex2D {
x: v.position.x,
y: v.position.y,
u: v.uv.x,
v: v.uv.y,
r, g, b, a,
});
}
let tid = mesh.texture.as_ref().map(|t| t.id).unwrap_or(1);
unsafe {
ogre_draw_2d_batch(s.ctx, v2d.as_ptr(), v2d.len() as u32, mesh.indices.as_ptr(), mesh.indices.len() as u32, tid);
}
return;
};
if mat.is_sky || mat.is_shadow_or_post {
return;
}
let key = mesh.vertices.as_ptr() as usize;
let tex_id = mesh.texture.as_ref().map(|t| t.id).unwrap_or(1);
let fingerprint = mesh_fingerprint(mesh, tex_id, mat.is_glass);
let to_bridge_verts = |verts: &[Vertex]| -> Vec<BridgeVertex3D> {
verts.iter().map(|v| BridgeVertex3D {
px: v.position.x, py: v.position.y, pz: v.position.z,
nx: v.normal.x,   ny: v.normal.y,   nz: v.normal.z,
u: v.uv.x,        v: v.uv.y,
r: *v.color.first().unwrap_or(&255),
g: *v.color.get(1).unwrap_or(&255),
b: *v.color.get(2).unwrap_or(&255),
a: *v.color.get(3).unwrap_or(&255),
spec: v.normal.w,
}).collect()
};
let ctx_ptr = s.ctx;
let mesh_id = if let Some(cached) = s.mesh_cache.get_mut(&key) {
if cached.fingerprint != fingerprint
    || cached.vertex_count != mesh.vertices.len()
    || cached.index_count != mesh.indices.len() {
    let bverts = to_bridge_verts(&mesh.vertices);
    unsafe {
        ogre_update_mesh_vertices(
            ctx_ptr,
            cached.mesh_id,
            bverts.as_ptr(),
            bverts.len() as u32,
            mesh.indices.as_ptr(),
            mesh.indices.len() as u32,
        );
    }
    cached.fingerprint = fingerprint;
    cached.vertex_count = mesh.vertices.len();
    cached.index_count = mesh.indices.len();
}
if cached.texture_id != tex_id || cached.is_glass != mat.is_glass || cached.is_water != mat.is_water {
     unsafe { ogre_set_mesh_material(ctx_ptr, cached.mesh_id, tex_id, mat.is_glass, mat.is_water); }
     cached.texture_id = tex_id;
     cached.is_glass = mat.is_glass;
     cached.is_water = mat.is_water;
 }
cached.mesh_id
} else {
let bverts = to_bridge_verts(&mesh.vertices);
let id = unsafe {
ogre_upload_mesh(
ctx_ptr,
bverts.as_ptr(),
bverts.len() as u32,
mesh.indices.as_ptr(),
mesh.indices.len() as u32,
tex_id,
mat.is_glass,
            mat.is_water,
)
};
s.mesh_cache.insert(key, CachedGpuMesh {
mesh_id: id,
vertex_count: mesh.vertices.len(),
index_count: mesh.indices.len(),
fingerprint,
texture_id: tex_id,
is_glass: mat.is_glass,
            is_water: mat.is_water,
});
id
};
if mesh_id != 0 {
let world_m = (*mat.model_mat.borrow()) * (*mat.car_mat.borrow());
let Some((scale, rot, pos)) = decompose_affine_for_trs(world_m) else { return; };
let tint = *mat.tint.borrow();
unsafe {
    ogre_draw_mesh_trs(
        ctx_ptr, mesh_id,
        pos.x, pos.y, pos.z,
        rot.x, rot.y, rot.z, rot.w,
        scale.x, scale.y, scale.z,
        tint.x, tint.y, tint.z,
        mat.is_glass,
    );
}
}
});
}
// ---------------- Ø·Â£Ø¸ËØ·Â§Ø¸â¦Ø·Â± Ø·Â±Ø·Â³Ø¸â¦ Ø·Â§Ø¸âØ¸ËØ·Â§Ø·Â¬Ø¸â¡Ø·Â§Ø·Ú¾ Ø¸ËØ·Â§Ø¸âØ·Â¹Ø·Â¯Ø·Â§Ø·Â¯Ø·Â§Ø·Ú¾ 2D ----------------
pub fn clear_background(_color: Color) {}
pub fn draw_rectangle(x: f32, y: f32, w: f32, h: f32, color: Color) {
draw_textured_quad_2d(1, x, y, w, h, color, false);
}
pub fn draw_rectangle_lines(x: f32, y: f32, w: f32, h: f32, thickness: f32, color: Color) {
let t = thickness.max(1.0);
draw_rectangle(x, y, w, t, color);
draw_rectangle(x, y + h - t, w, t, color);
draw_rectangle(x, y + t, t, (h - 2.0 * t).max(0.0), color);
draw_rectangle(x + w - t, y + t, t, (h - 2.0 * t).max(0.0), color);
}
pub fn draw_line(x1: f32, y1: f32, x2: f32, y2: f32, thickness: f32, color: Color) {
let dx = x2 - x1;
let dy = y2 - y1;
let len = (dx * dx + dy * dy).sqrt();
if len < 0.001 {
return;
}
let nx = -dy / len * (thickness * 0.5);
let ny = dx / len * (thickness * 0.5);
let (r, g, b, a) = color.to_rgba8();
let verts = [
BridgeVertex2D { x: x1 + nx, y: y1 + ny, u: 0.0, v: 0.0, r, g, b, a },
BridgeVertex2D { x: x2 + nx, y: y2 + ny, u: 1.0, v: 0.0, r, g, b, a },
BridgeVertex2D { x: x2 - nx, y: y2 - ny, u: 1.0, v: 1.0, r, g, b, a },
BridgeVertex2D { x: x1 - nx, y: y1 - ny, u: 0.0, v: 1.0, r, g, b, a },
];
let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];
submit_2d(&verts, &idx, 1);
}
pub fn draw_triangle(v1: Vec2, v2: Vec2, v3: Vec2, color: Color) {
let (r, g, b, a) = color.to_rgba8();
let verts = [
BridgeVertex2D { x: v1.x, y: v1.y, u: 0.0, v: 0.0, r, g, b, a },
BridgeVertex2D { x: v2.x, y: v2.y, u: 1.0, v: 0.0, r, g, b, a },
BridgeVertex2D { x: v3.x, y: v3.y, u: 0.5, v: 1.0, r, g, b, a },
];
let idx: [u16; 3] = [0, 1, 2];
submit_2d(&verts, &idx, 1);
}
pub fn draw_circle(x: f32, y: f32, radius: f32, color: Color) {
let steps = 24usize;
let (r, g, b, a) = color.to_rgba8();
let mut verts = Vec::with_capacity(steps + 1);
let mut idx = Vec::with_capacity(steps * 3);
verts.push(BridgeVertex2D { x, y, u: 0.5, v: 0.5, r, g, b, a });
for i in 0..steps {
let ang = (i as f32 / steps as f32) * std::f32::consts::TAU;
verts.push(BridgeVertex2D {
x: x + ang.cos() * radius,
y: y + ang.sin() * radius,
u: 0.5,
v: 0.5,
r, g, b, a,
});
}
for i in 0..steps {
let n = if i + 1 == steps { 1 } else { (i + 2) as u16 };
idx.extend_from_slice(&[0, (i + 1) as u16, n]);
}
submit_2d(&verts, &idx, 1);
}
pub fn draw_circle_lines(x: f32, y: f32, radius: f32, thickness: f32, color: Color) {
let steps = 28usize;
for i in 0..steps {
let a0 = (i as f32 / steps as f32) * std::f32::consts::TAU;
let a1 = ((i + 1) as f32 / steps as f32) * std::f32::consts::TAU;
draw_line(
x + a0.cos() * radius,
y + a0.sin() * radius,
x + a1.cos() * radius,
y + a1.sin() * radius,
thickness,
color,
);
}
}
pub fn draw_texture_ex(tex: &Texture2D, x: f32, y: f32, color: Color, params: DrawTextureParams) {
if tex.id <= 1 {
return;
}
let sz = params.dest_size.unwrap_or(vec2(tex.width, tex.height));
draw_textured_quad_2d(tex.id, x, y, sz.x, sz.y, color, params.flip_y);
}
fn draw_textured_quad_2d(tex_id: u32, x: f32, y: f32, w: f32, h: f32, color: Color, flip_y: bool) {
let (r, g, b, a) = color.to_rgba8();
let (v0, v1) = if flip_y { (1.0, 0.0) } else { (0.0, 1.0) };
let verts = [
BridgeVertex2D { x, y, u: 0.0, v: v0, r, g, b, a },
BridgeVertex2D { x: x + w, y, u: 1.0, v: v0, r, g, b, a },
BridgeVertex2D { x: x + w, y: y + h, u: 1.0, v: v1, r, g, b, a },
BridgeVertex2D { x, y: y + h, u: 0.0, v: v1, r, g, b, a },
];
let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];
submit_2d(&verts, &idx, tex_id);
}
fn submit_2d(verts: &[BridgeVertex2D], idx: &[u16], tex_id: u32) {
STATE.with(|st| {
let s = st.borrow();
if !s.ctx.is_null() && !s.offscreen_skip {
unsafe {
ogre_draw_2d_batch(s.ctx, verts.as_ptr(), verts.len() as u32, idx.as_ptr(), idx.len() as u32, tex_id);
}
}
});
}
// ---------------- Ø·Â§Ø¸âØ¸â Ø·ÂµØ¸ËØ·Âµ Ø¸ËØ·Â§Ø¸âØ·Â®Ø·Â·Ø¸ËØ·Â· ----------------
pub use text_ui::{Font, TextDimensions};
pub struct TextParams<'a> {
pub font: Option<&'a Font>,
pub font_size: u16,
pub font_scale: f32,
pub font_scale_aspect: f32,
pub rotation: f32,
pub color: Color,
}
impl<'a> Default for TextParams<'a> {
fn default() -> Self {
Self {
font: None,
font_size: 20,
font_scale: 1.0,
font_scale_aspect: 1.0,
rotation: 0.0,
color: WHITE,
}
}
}
pub fn load_ttf_font_from_bytes(bytes: &[u8]) -> Result<Font, String> {
Font::from_bytes(bytes)
}
pub fn measure_text(text: &str, font: Option<&Font>, font_size: u16, font_scale: f32) -> TextDimensions {
text_ui::measure_font_text(text, font, font_size, font_scale)
}
pub fn draw_text_ex(text: &str, x: f32, y: f32, params: TextParams<'_>) -> TextDimensions {
let dims = measure_text(text, params.font, params.font_size, params.font_scale);
let (r, g, b, a) = params.color.to_rgba8();
let built = with_ctx(|ctx| {
if ctx.is_null() {
None
} else {
text_ui::build_text_quads(ctx, text, x, y, params.font, params.font_size, params.font_scale, [r, g, b, a])
}
});
if let Some((tid, verts, idx)) = built {
submit_2d(&verts, &idx, tid);
}
dims
}
// ---------------- Ø·Â§Ø¸âØ·Â´Ø·Â§Ø·Â´Ø·Â© Ø¸ËØ·Â§Ø¸âØ¸ËØ¸âØ·Ú¾ Ø¸ËØ·Â§Ø¸âØ¸â¦Ø·Â¯Ø·Â®Ø¸âØ·Â§Ø·Ú¾ ----------------
pub fn screen_width() -> f32 {
STATE.with(|st| st.borrow().win_w.max(1) as f32)
}
pub fn screen_height() -> f32 {
STATE.with(|st| st.borrow().win_h.max(1) as f32)
}
pub fn get_frame_time() -> f32 {
STATE.with(|st| st.borrow().dt)
}
pub fn get_fps() -> i32 {
STATE.with(|st| st.borrow().fps)
}
pub fn get_time() -> f64 {
STATE.with(|st| st.borrow().start_time.elapsed().as_secs_f64())
}
pub fn is_key_down(key: KeyCode) -> bool {
STATE.with(|st| st.borrow().keys_down.contains(&key))
}
pub fn is_key_pressed(key: KeyCode) -> bool {
STATE.with(|st| {
let s = st.borrow();
s.keys_down.contains(&key) && !s.keys_prev.contains(&key)
})
}
pub fn get_char_pressed() -> Option<char> {
with_ctx(|ctx| {
if ctx.is_null() {
return None;
}
let code = unsafe { ogre_pop_char(ctx) };
if code == 0 { None } else { char::from_u32(code) }
})
}
pub fn mouse_position() -> (f32, f32) {
STATE.with(|st| st.borrow().mouse_pos)
}
pub fn mouse_delta_position() -> Vec2 {
STATE.with(|st| {
let s = st.borrow();
let w = s.win_w.max(1) as f32;
let h = s.win_h.max(1) as f32;
vec2((s.mouse_pos.0 - s.mouse_prev.0) / w, (s.mouse_pos.1 - s.mouse_prev.1) / h)
})
}
pub fn mouse_wheel() -> (f32, f32) {
STATE.with(|st| (0.0, st.borrow().wheel_y))
}
pub fn is_mouse_button_down(btn: MouseButton) -> bool {
STATE.with(|st| {
let b = st.borrow().mb_down;
match btn {
MouseButton::Left => b.0,
MouseButton::Right => b.1,
MouseButton::Middle => b.2,
MouseButton::Unknown => false,
}
})
}
pub fn is_mouse_button_pressed(btn: MouseButton) -> bool {
STATE.with(|st| {
let s = st.borrow();
match btn {
MouseButton::Left => s.mb_down.0 && !s.mb_prev.0,
MouseButton::Right => s.mb_down.1 && !s.mb_prev.1,
MouseButton::Middle => s.mb_down.2 && !s.mb_prev.2,
MouseButton::Unknown => false,
}
})
}
// ---------------- Ø·Â§Ø¸âØ¸â Ø·Â§Ø¸Ù¾Ø·Â°Ø·Â© Ø¸ËØ·Â­Ø¸âØ¸âØ·Â© Ø·Â§Ø¸âØ·Ú¾Ø·Â´Ø·ØØ¸Ù¹Ø¸â Async ----------------
pub mod conf {
#[derive(Clone)]
pub struct Conf {
pub window_title: String,
pub window_width: i32,
pub window_height: i32,
pub sample_count: i32,
pub fullscreen: bool,
pub window_resizable: bool,
pub draw_call_vertex_capacity: usize,
pub draw_call_index_capacity: usize,
}
impl Default for Conf {
fn default() -> Self {
Self {
window_title: "APEX.drive (Ogre-Next 3.0)".into(),
window_width: 1280,
window_height: 720,
sample_count: 0,
fullscreen: false,
window_resizable: true,
draw_call_vertex_capacity: 55000,
draw_call_index_capacity: 190000,
}
}
}
}
pub use conf::Conf;
struct NextFrameFuture {
yielded: bool,
}
impl Future for NextFrameFuture {
type Output = ();
fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {

if self.yielded {
Poll::Ready(())
} else {
self.yielded = true;
Poll::Pending
}
}
}
pub fn next_frame() -> impl Future<Output = ()> {
NextFrameFuture { yielded: false }
}
pub struct Window;
impl Window {
pub fn from_config<F: Future<Output = ()>>(config: Conf, mut fut: F) {
let title = CString::new(format!("{} [Ogre-Next 3.0 Direct3D11]", config.window_title)).unwrap();
let ctx = unsafe { ogre_init(title.as_ptr(), config.window_width, config.window_height, config.sample_count) };
assert!(!ctx.is_null(), "Failed to initialize Ogre-Next 3.0!");
STATE.with(|st| {
let mut s = st.borrow_mut();
s.ctx = ctx;
s.win_w = config.window_width;
s.win_h = config.window_height;
s.start_time = Instant::now();
s.last_frame = Instant::now();
});
fn dummy_raw_waker() -> RawWaker {
fn no_op(_p: *const ()) {}
fn clone(_p: *const ()) -> RawWaker { dummy_raw_waker() }
static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
RawWaker::new(std::ptr::null(), &VTABLE)
}
let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
let mut task_cx = Context::from_waker(&waker);
let mut pinned = unsafe { Pin::new_unchecked(&mut fut) };
loop {
let mut w = 1280;
let mut h = 720;
let alive = unsafe { ogre_begin_frame(ctx, &mut w, &mut h) };
if !alive {
break;
}
STATE.with(|st| {
let mut s = st.borrow_mut();
let now = Instant::now();
let dt = now.duration_since(s.last_frame).as_secs_f32().clamp(0.0001, 0.1);
s.last_frame = now;
s.dt = dt;
s.fps = (1.0 / dt).round() as i32;
s.win_w = w.max(1);
s.win_h = h.max(1);
s.keys_prev = s.keys_down.clone();
s.keys_down.clear();
for &k in ALL_KEYS {
if unsafe { ogre_key_down(k.to_vk()) } {
s.keys_down.insert(k);
}
}
s.mouse_prev = s.mouse_pos;
s.mb_prev = s.mb_down;
let (mut mx, mut my, mut wy) = (0.0, 0.0, 0.0);
let (mut l, mut r, mut m) = (false, false, false);
unsafe {
ogre_mouse_state(ctx, &mut mx, &mut my, &mut wy, &mut l, &mut r, &mut m);
}
s.mouse_pos = (mx, my);
s.wheel_y = wy;
s.mb_down = (l, r, m);
});
match pinned.as_mut().poll(&mut task_cx) {
Poll::Ready(()) => break,
Poll::Pending => {}
}
if !unsafe { ogre_end_frame(ctx) } {
break;
}
}
unsafe {
ogre_shutdown(ctx);
}
}
}
// ---------------- Ø¸ËØ·Â­Ø·Â¯Ø·Â§Ø·Ú¾ Ø·Â§Ø¸âØ·Ú¾Ø¸ËØ·Â§Ø¸Ù¾Ø¸â Ø·Â§Ø¸âØ¸Ù¾Ø·Â±Ø·Â¹Ø¸Ù¹Ø·Â© (miniquad & audio & prelude) ----------------
pub struct QuadGl;
impl QuadGl {
pub fn scissor(&mut self, _rect: Option<(i32, i32, i32, i32)>) {}
}
pub struct InternalGlContext<'a> {
pub quad_gl: &'a mut QuadGl,
pub quad_context: &'a mut miniquad::DummyCtx,
}
static mut DUMMY_QGL: QuadGl = QuadGl;
static mut DUMMY_CTX: miniquad::DummyCtx = miniquad::DummyCtx;
pub unsafe fn get_internal_gl<'a>() -> InternalGlContext<'a> {
InternalGlContext {
quad_gl: &mut *std::ptr::addr_of_mut!(DUMMY_QGL),
quad_context: &mut *std::ptr::addr_of_mut!(DUMMY_CTX),
}
}
pub mod miniquad {
pub use crate::UniformType;
#[derive(Clone, Copy)]
pub struct TextureId(pub u32);
#[derive(Clone, Copy)]
pub enum TextureWrap { Repeat, Clamp, Mirror }
#[derive(Clone, Copy)]
pub enum FilterMode { Linear, Nearest }
#[derive(Clone, Copy)]
pub enum MipmapFilterMode { None, Linear }
pub struct DummyCtx;
impl DummyCtx {
pub fn texture_generate_mipmaps(&mut self, _id: TextureId) {} pub fn texture_set_wrap(&mut self, _id: TextureId, _x: TextureWrap, _y: TextureWrap) {}
pub fn texture_set_filter(&mut self, _id: TextureId, _f: FilterMode, _m: MipmapFilterMode) {}
}
#[derive(Clone, Copy, Default)]
pub enum Comparison { #[default] Never, Less, Equal, LessOrEqual, Greater, NotEqual, GreaterOrEqual, Always }
#[derive(Clone, Copy, Default)]
pub enum CullFace { #[default] Nothing, Front, Back }
#[derive(Clone, Copy)]
pub enum BlendValue { SourceColor, SourceAlpha, DestinationColor, DestinationAlpha }
#[derive(Clone, Copy)]
pub enum BlendFactor { Zero, One, Value(BlendValue), OneMinusValue(BlendValue) }
#[derive(Clone, Copy)]
pub enum Equation { Add, Subtract, ReverseSubtract }
#[derive(Clone, Copy)]
pub struct BlendState;
impl BlendState {
pub fn new(_eq: Equation, _src: BlendFactor, _dst: BlendFactor) -> Self { Self }
}
#[derive(Clone, Copy, Default)]
pub struct PipelineParams {
pub depth_test: Comparison,
pub depth_write: bool,
pub cull_face: CullFace,
pub color_blend: Option<BlendState>,
}
#[derive(Clone)]
pub struct UniformDesc {
pub name: String,
pub uniform_type: UniformType,
}
impl UniformDesc {
pub fn new(name: &str, uniform_type: UniformType) -> Self {
Self { name: name.to_string(), uniform_type }
}
}
pub mod window {
pub fn clipboard_get() -> Option<String> { None }
pub fn clipboard_set(_s: &str) {}
}
}
pub mod audio {
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::rc::Rc;
    use std::sync::Arc;

    struct AudioEngine {
        _stream: rodio::OutputStream,
        handle: rodio::OutputStreamHandle,
    }

    thread_local! {
        static AUDIO: RefCell<Option<AudioEngine>> = RefCell::new(None);
    }

    fn ensure_audio<R>(f: impl FnOnce(&rodio::OutputStreamHandle) -> R) -> Option<R> {
        AUDIO.with(|a| {
            let mut opt = a.borrow_mut();
            if opt.is_none() {
                if let Ok((stream, handle)) = rodio::OutputStream::try_default() {
                    *opt = Some(AudioEngine { _stream: stream, handle });
                }
            }
            opt.as_ref().map(|eng| f(&eng.handle))
        })
    }

    #[derive(Clone)]
    pub struct Sound {
        bytes: Arc<Vec<u8>>,
        sink: Rc<RefCell<Option<rodio::Sink>>>,
    }

    #[derive(Clone, Copy, Default)]
    pub struct PlaySoundParams {
        pub looped: bool,
        pub volume: f32,
    }

    pub async fn load_sound_from_bytes(bytes: &[u8]) -> Result<Sound, String> {
        Ok(Sound {
            bytes: Arc::new(bytes.to_vec()),
            sink: Rc::new(RefCell::new(None)),
        })
    }

    pub fn play_sound(sound: &Sound, params: PlaySoundParams) {
        stop_sound(sound);
        let data = sound.bytes.clone();
        ensure_audio(|handle| {
            if let Ok(sink) = rodio::Sink::try_new(handle) {
                sink.set_volume(params.volume.clamp(0.0, 2.0));
                let cursor = Cursor::new((*data).clone());
                if let Ok(decoder) = rodio::Decoder::new(cursor) {
                    if params.looped {
                        use rodio::Source;
                        sink.append(decoder.repeat_infinite());
                    } else {
                        sink.append(decoder);
                    }
                    *sound.sink.borrow_mut() = Some(sink);
                }
            }
        });
    }

    pub fn stop_sound(sound: &Sound) {
        if let Some(sink) = sound.sink.borrow_mut().take() {
            sink.stop();
        }
    }

    pub fn set_sound_volume(sound: &Sound, vol: f32) {
        if let Some(sink) = sound.sink.borrow().as_ref() {
            sink.set_volume(vol.clamp(0.0, 2.0));
        }
    }
}
pub mod prelude {
pub use crate::{
clear_background, draw_circle, draw_circle_lines, draw_line, draw_mesh, draw_rectangle,
draw_rectangle_lines, draw_text_ex, draw_texture_ex, draw_triangle, get_char_pressed,
get_fps, get_frame_time, get_internal_gl, get_time, gl_set_drawcall_buffer_capacity,
gl_use_default_material, gl_use_material, is_key_down, is_key_pressed,
is_mouse_button_down, is_mouse_button_pressed, load_material, load_ttf_font_from_bytes,
mat3, mat4, measure_text, mouse_delta_position, mouse_position, mouse_wheel, next_frame,
quat, render_target, render_target_ex, screen_height, screen_width, set_camera,
set_default_camera, vec2, vec3, vec4, Affine3A, Camera2D, Color, Conf, DrawTextureParams,
EulerRot, FilterMode, Font, Image, ImageFormat, KeyCode, Mat3, Mat4, Material, MaterialParams, Mesh,
MouseButton, Quat, Rect, RenderTarget, RenderTargetParams, ShaderSource, TextDimensions,
TextParams, Texture2D, UniformType, Vec2, Vec3, Vec4, Vertex, BLACK, BLUE, GRAY, GREEN,
RED, WHITE, YELLOW,
};
}