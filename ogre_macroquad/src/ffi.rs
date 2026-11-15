use std::ffi::c_void;
use std::os::raw::{c_char, c_int};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BridgeVertex3D {
    pub px: f32,
    pub py: f32,
    pub pz: f32,
    pub nx: f32,
    pub ny: f32,
    pub nz: f32,
    pub u: f32,
    pub v: f32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    pub spec: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BridgeVertex2D {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

extern "C" {
    pub fn ogre_init(title: *const c_char, width: c_int, height: c_int, msaa: c_int) -> *mut c_void;
    pub fn ogre_begin_frame(ctx: *mut c_void, out_w: *mut c_int, out_h: *mut c_int) -> bool;
    pub fn ogre_end_frame(ctx: *mut c_void) -> bool;
    pub fn ogre_shutdown(ctx: *mut c_void);

    pub fn ogre_set_camera_pose(
        ctx: *mut c_void,
        px: f32, py: f32, pz: f32,
        qx: f32, qy: f32, qz: f32, qw: f32,
        fov_rad: f32, znear: f32, zfar: f32,
    );
    pub fn ogre_set_env(
        ctx: *mut c_void,
        sun_x: f32,
        sun_y: f32,
        sun_z: f32,
        fog_r: f32,
        fog_g: f32,
        fog_b: f32,
        sun_elev: f32,
    );

    pub fn ogre_create_texture(
        ctx: *mut c_void,
        width: u32,
        height: u32,
        rgba: *const u8,
        linear_filter: bool,
    ) -> u32;
    pub fn ogre_update_texture(
        ctx: *mut c_void,
        tex_id: u32,
        width: u32,
        height: u32,
        rgba: *const u8,
    );
    /// Changes the sampler filtering mode for a texture (must be called before first use when possible).
    pub fn ogre_set_texture_filter(ctx: *mut c_void, tex_id: u32, linear_filter: bool);

    pub fn ogre_upload_mesh(
        ctx: *mut c_void,
        verts: *const BridgeVertex3D,
        vcount: u32,
        indices: *const u16,
        icount: u32,
        tex_id: u32,
        is_glass: bool,
        is_water: bool,
    ) -> u32;
    pub fn ogre_update_mesh_vertices(
        ctx: *mut c_void,
        mesh_id: u32,
        verts: *const BridgeVertex3D,
        vcount: u32,
        indices: *const u16,
        icount: u32,
    );
    /// Updates the texture/glass material associated with an already uploaded mesh.
    pub fn ogre_set_mesh_material(ctx: *mut c_void, mesh_id: u32, tex_id: u32, is_glass: bool, is_water: bool);
    pub fn ogre_draw_mesh_trs(
        ctx: *mut c_void, mesh_id: u32,
        px: f32, py: f32, pz: f32,
        qx: f32, qy: f32, qz: f32, qw: f32,
        sx: f32, sy: f32, sz: f32,
        tint_r: f32, tint_g: f32, tint_b: f32,
        is_glass: bool,
    );

    pub fn ogre_draw_2d_batch(
        ctx: *mut c_void,
        verts: *const BridgeVertex2D,
        vcount: u32,
        indices: *const u16,
        icount: u32,
        tex_id: u32,
    );

    pub fn ogre_key_down(vk_code: c_int) -> bool;
    pub fn ogre_mouse_state(
        ctx: *mut c_void,
        mx: *mut f32,
        my: *mut f32,
        wheel_y: *mut f32,
        left_down: *mut bool,
        right_down: *mut bool,
        mid_down: *mut bool,
    );
    pub fn ogre_pop_char(ctx: *mut c_void) -> u32;
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyCode {
    Space, Apostrophe, Comma, Minus, Period, Slash,
    Key0, Key1, Key2, Key3, Key4, Key5, Key6, Key7, Key8, Key9,
    Semicolon, Equal,
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    LeftBracket, Backslash, RightBracket, GraveAccent,
    Escape, Enter, Tab, Backspace, Insert, Delete,
    Right, Left, Down, Up,
    PageUp, PageDown, Home, End,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    KpEnter, LeftShift, LeftControl, LeftAlt, LeftSuper,
    RightShift, RightControl, RightAlt, RightSuper,
}

impl KeyCode {
    pub fn to_vk(self) -> c_int {
        match self {
            KeyCode::Space => 0x20,
            KeyCode::Minus => 0xBD,
            KeyCode::Equal => 0xBB,
            KeyCode::Comma => 0xBC,
            KeyCode::Period => 0xBE,
            KeyCode::Slash => 0xBF,
            KeyCode::Semicolon => 0xBA,
            KeyCode::Apostrophe => 0xDE,
            KeyCode::LeftBracket => 0xDB,
            KeyCode::Backslash => 0xDC,
            KeyCode::RightBracket => 0xDD,
            KeyCode::GraveAccent => 0xC0,
            KeyCode::Key0 => 0x30, KeyCode::Key1 => 0x31, KeyCode::Key2 => 0x32,
            KeyCode::Key3 => 0x33, KeyCode::Key4 => 0x34, KeyCode::Key5 => 0x35,
            KeyCode::Key6 => 0x36, KeyCode::Key7 => 0x37, KeyCode::Key8 => 0x38,
            KeyCode::Key9 => 0x39,
            KeyCode::A => 0x41, KeyCode::B => 0x42, KeyCode::C => 0x43, KeyCode::D => 0x44,
            KeyCode::E => 0x45, KeyCode::F => 0x46, KeyCode::G => 0x47, KeyCode::H => 0x48,
            KeyCode::I => 0x49, KeyCode::J => 0x4A, KeyCode::K => 0x4B, KeyCode::L => 0x4C,
            KeyCode::M => 0x4D, KeyCode::N => 0x4E, KeyCode::O => 0x4F, KeyCode::P => 0x50,
            KeyCode::Q => 0x51, KeyCode::R => 0x52, KeyCode::S => 0x53, KeyCode::T => 0x54,
            KeyCode::U => 0x55, KeyCode::V => 0x56, KeyCode::W => 0x57, KeyCode::X => 0x58,
            KeyCode::Y => 0x59, KeyCode::Z => 0x5A,
            KeyCode::Escape => 0x1B,
            KeyCode::Enter | KeyCode::KpEnter => 0x0D,
            KeyCode::Tab => 0x09,
            KeyCode::Backspace => 0x08,
            KeyCode::Insert => 0x2D,
            KeyCode::Delete => 0x2E,
            KeyCode::Left => 0x25, KeyCode::Up => 0x26, KeyCode::Right => 0x27, KeyCode::Down => 0x28,
            KeyCode::PageUp => 0x21, KeyCode::PageDown => 0x22,
            KeyCode::End => 0x23, KeyCode::Home => 0x24,
            KeyCode::F1 => 0x70, KeyCode::F2 => 0x71, KeyCode::F3 => 0x72, KeyCode::F4 => 0x73,
            KeyCode::F5 => 0x74, KeyCode::F6 => 0x75, KeyCode::F7 => 0x76, KeyCode::F8 => 0x77,
            KeyCode::F9 => 0x78, KeyCode::F10 => 0x79, KeyCode::F11 => 0x7A, KeyCode::F12 => 0x7B,
            KeyCode::LeftShift => 0xA0, KeyCode::RightShift => 0xA1,
            KeyCode::LeftControl => 0xA2, KeyCode::RightControl => 0xA3,
            KeyCode::LeftAlt => 0xA4, KeyCode::RightAlt => 0xA5,
            KeyCode::LeftSuper => 0x5B, KeyCode::RightSuper => 0x5C,
        }
    }
}

pub const ALL_KEYS: &[KeyCode] = &[
    KeyCode::Space, KeyCode::Minus, KeyCode::Equal, KeyCode::Comma, KeyCode::Period, KeyCode::Slash,
    KeyCode::Key0, KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4,
    KeyCode::Key5, KeyCode::Key6, KeyCode::Key7, KeyCode::Key8, KeyCode::Key9,
    KeyCode::A, KeyCode::B, KeyCode::C, KeyCode::D, KeyCode::E, KeyCode::F, KeyCode::G,
    KeyCode::H, KeyCode::I, KeyCode::J, KeyCode::K, KeyCode::L, KeyCode::M, KeyCode::N,
    KeyCode::O, KeyCode::P, KeyCode::Q, KeyCode::R, KeyCode::S, KeyCode::T, KeyCode::U,
    KeyCode::V, KeyCode::W, KeyCode::X, KeyCode::Y, KeyCode::Z,
    KeyCode::LeftBracket, KeyCode::RightBracket, KeyCode::Escape, KeyCode::Enter, KeyCode::Tab,
    KeyCode::Backspace, KeyCode::Delete, KeyCode::Right, KeyCode::Left, KeyCode::Down, KeyCode::Up,
    KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6,
    KeyCode::F7, KeyCode::F8, KeyCode::F9, KeyCode::F10, KeyCode::F11, KeyCode::F12,
    KeyCode::LeftShift, KeyCode::RightShift, KeyCode::LeftControl, KeyCode::RightControl,
    KeyCode::LeftAlt, KeyCode::RightAlt,
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Unknown,
}