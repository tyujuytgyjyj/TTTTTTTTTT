// glTF (.glb) car model loader: body mesh + 4 wheels.
use macroquad::prelude::*;

#[derive(Clone, Default)]
pub struct RawMesh {
    pub pos: Vec<Vec3>,
    pub nrm: Vec<Vec3>,
    pub uv: Vec<Vec2>,
    pub idx: Vec<u16>,
}

pub struct CarModel {
    pub body: RawMesh,
    /// order: front-left, front-right, back-left, back-right
    pub wheel_hub: [Vec3; 4],
    pub wheel_mesh: [RawMesh; 4],
}

const WHEEL_NAMES: [&str; 4] = [
    "wheel-front-left",
    "wheel-front-right",
    "wheel-back-left",
    "wheel-back-right",
];

fn read_prim(
    prim: &gltf::Primitive,
    blob: &Option<Vec<u8>>,
    xf: &Mat4,
    out: &mut RawMesh,
) {
    let reader = prim.reader(|_b| blob.as_deref());
    let base = out.pos.len() as u32;
    let pos: Vec<[f32; 3]> = match reader.read_positions() {
        Some(p) => p.collect(),
        None => return,
    };
    let n = pos.len();
    let nrm: Vec<[f32; 3]> = reader
        .read_normals()
        .map(|i| i.collect())
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; n]);
    let uv: Vec<[f32; 2]> = reader
        .read_tex_coords(0)
        .map(|t| t.into_f32().collect())
        .unwrap_or_else(|| vec![[0.0, 0.0]; n]);
    let nm = Mat3::from_mat4(*xf);
    for i in 0..n {
        out.pos.push(xf.transform_point3(Vec3::from(pos[i])));
        out.nrm.push((nm * Vec3::from(nrm[i])).normalize_or_zero());
        out.uv.push(Vec2::from(uv[i]));
    }
    if let Some(ind) = reader.read_indices() {
        for i in ind.into_u32() {
            out.idx.push((base + i) as u16);
        }
    }
}

pub fn load(bytes: &[u8]) -> CarModel {
    let Ok(g) = gltf::Gltf::from_slice(bytes) else { eprintln!("[WARN] Invalid GLB skipped"); return CarModel { body: RawMesh::default(), wheel_hub: [Vec3::ZERO; 4], wheel_mesh: Default::default() }; };
    let blob = g.blob.clone();
    let mut body = RawMesh::default();
    let mut wheel_hub = [Vec3::ZERO; 4];
    let mut wheel_mesh: [RawMesh; 4] = Default::default();
    for node in g.nodes() {
        let mesh = match node.mesh() {
            Some(m) => m,
            None => continue,
        };
        let name = node.name().unwrap_or("");
        let m = Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(wi) = WHEEL_NAMES.iter().position(|w| *w == name) {
            // keep wheel mesh in local space (origin = hub)
            wheel_hub[wi] = m.transform_point3(Vec3::ZERO);
            for p in mesh.primitives() {
                read_prim(&p, &blob, &Mat4::IDENTITY, &mut wheel_mesh[wi]);
            }
        } else {
            for p in mesh.primitives() {
                read_prim(&p, &blob, &m, &mut body);
            }
        }
    }
    CarModel { body, wheel_hub, wheel_mesh }
}

/// Uniformly split every triangle into 4 (shared edge midpoints, so no cracks).
/// More vertices = smoother crumpling when the soft body deforms the surface.
pub fn subdivide(m: &RawMesh) -> RawMesh {
    use std::collections::HashMap;
    let mut out = m.clone();
    let mut cache: HashMap<(u16, u16), u16> = HashMap::new();
    let mut idx = Vec::new();
    let mut mid = |out: &mut RawMesh, a: u16, b: u16| -> u16 {
        let key = if a < b { (a, b) } else { (b, a) };
        if let Some(&v) = cache.get(&key) {
            return v;
        }
        let (a_, b_) = (a as usize, b as usize);
        let p = (out.pos[a_] + out.pos[b_]) * 0.5;
        let n = (out.nrm[a_] + out.nrm[b_]).normalize_or_zero();
        let uv = (out.uv[a_] + out.uv[b_]) * 0.5;
        out.pos.push(p);
        out.nrm.push(n);
        out.uv.push(uv);
        let id = (out.pos.len() - 1) as u16;
        cache.insert(key, id);
        id
    };
    for t in m.idx.chunks(3) {
        let (a, b, c) = (t[0], t[1], t[2]);
        let ab = mid(&mut out, a, b);
        let bc = mid(&mut out, b, c);
        let ca = mid(&mut out, c, a);
        idx.extend_from_slice(&[a, ab, ca, ab, b, bc, ca, bc, c, ab, bc, ca]);
    }
    out.idx = idx;
    out
}


/// Simple wheel stand-in (axis = x, centred on the hub): used as physics/shadow proxy for high-detail cars.
pub fn cylinder(radius: f32, width: f32) -> RawMesh {
    let mut m = RawMesh::default();
    let n = 14u16;
    for side in [-1.0f32, 1.0] {
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            m.pos.push(vec3(side * width * 0.5, s * radius, c * radius));
            m.nrm.push(vec3(0.0, s, c));
            m.uv.push(vec2(0.02, 0.02));
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b, c, d) = (i, j, i + n, j + n);
        m.idx.extend_from_slice(&[a, b, c, b, d, c]);
    }
    for i in 1..n - 1 {
        m.idx.extend_from_slice(&[0, i + 1, i, n, n + i, n + i + 1]);
    }
    m
}

pub fn box_mesh(bmin: Vec3, bmax: Vec3) -> RawMesh {
    let corners = [
        vec3(bmin.x, bmin.y, bmin.z), vec3(bmax.x, bmin.y, bmin.z), vec3(bmax.x, bmax.y, bmin.z), vec3(bmin.x, bmax.y, bmin.z),
        vec3(bmin.x, bmin.y, bmax.z), vec3(bmax.x, bmin.y, bmax.z), vec3(bmax.x, bmax.y, bmax.z), vec3(bmin.x, bmax.y, bmax.z),
    ];
    let faces: [([usize; 4], Vec3); 6] = [
        ([4, 5, 6, 7], Vec3::Z),
        ([1, 0, 3, 2], -Vec3::Z),
        ([5, 1, 2, 6], Vec3::X),
        ([0, 4, 7, 3], -Vec3::X),
        ([3, 7, 6, 2], Vec3::Y),
        ([0, 1, 5, 4], -Vec3::Y),
    ];
    let mut rm = RawMesh::default();
    for (q, n) in faces {
        let b = rm.pos.len() as u16;
        for &ci in &q {
            rm.pos.push(corners[ci]);
            rm.nrm.push(n);
            rm.uv.push(vec2(0.02, 0.02));
        }
        rm.idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    rm
}