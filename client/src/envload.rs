// Static glTF scenery loader with texture compression/downscaling, 3-tier LOD generation,
// on-demand asset streaming, spatial + directional facade partitioning, and custom GLB discovery.
use macroquad::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Maximum texture dimension (512 = Low/Weak PC default, 1024 = Medium, 2048 = High).
static MAX_TEX_DIM: AtomicUsize = AtomicUsize::new(512);

pub fn set_max_tex_dim(dim: usize) {
    MAX_TEX_DIM.store(dim.clamp(256, 4096), Ordering::Relaxed);
}

pub fn max_tex_dim() -> usize {
    MAX_TEX_DIM.load(Ordering::Relaxed)
}

pub struct EPart {
    pub mesh: Mesh,
    pub blend: bool,
    pub bmin: Vec3,
    pub bmax: Vec3,
    pub facade: u8, // 0=Omni/Roof/Base, 1=N(+Z), 2=S(-Z), 3=E(+X), 4=W(-X), 5=NE, 6=NW, 7=SE, 8=SW
    pub cull_x: f32,
    pub cull_z: f32,
}

impl Clone for EPart {
    fn clone(&self) -> Self {
        EPart {
            mesh: Mesh {
                vertices: self.mesh.vertices.clone(),
                indices: self.mesh.indices.clone(),
                texture: self.mesh.texture.clone(),
            },
            blend: self.blend,
            bmin: self.bmin,
            bmax: self.bmax,
            facade: self.facade,
            cull_x: self.cull_x,
            cull_z: self.cull_z,
        }
    }
}

impl EPart {
    pub fn from_mesh(mesh: Mesh, blend: bool) -> Self {
        let (mut bmin, mut bmax) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        for v in &mesh.vertices {
            bmin = bmin.min(v.position);
            bmax = bmax.max(v.position);
        }
        EPart { mesh, blend, bmin, bmax, facade: 0, cull_x: 0.0, cull_z: 0.0 }
    }
}

fn downscale_rgba(mut w: usize, mut h: usize, mut rgba: Vec<u8>, max_dim: usize) -> (u16, u16, Vec<u8>) {
    while (w > max_dim || h > max_dim) && w >= 4 && h >= 4 {
        let nw = (w / 2).max(1);
        let nh = (h / 2).max(1);
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            let y0 = (y * 2) * w;
            let y1 = ((y * 2 + 1).min(h - 1)) * w;
            for x in 0..nw {
                let x0 = x * 2;
                let x1 = (x * 2 + 1).min(w - 1);
                let p00 = (y0 + x0) * 4;
                let p10 = (y0 + x1) * 4;
                let p01 = (y1 + x0) * 4;
                let p11 = (y1 + x1) * 4;
                let dst = (y * nw + x) * 4;
                for c in 0..3 {
                    let sum = rgba[p00 + c] as u32 + rgba[p10 + c] as u32 + rgba[p01 + c] as u32 + rgba[p11 + c] as u32;
                    next[dst + c] = (sum / 4) as u8;
                }
                // Preserve alpha coverage on chain-link fences / foliage cutouts
                let a_max = rgba[p00 + 3].max(rgba[p10 + 3]).max(rgba[p01 + 3]).max(rgba[p11 + 3]);
                let a_avg = ((rgba[p00 + 3] as u32 + rgba[p10 + 3] as u32 + rgba[p01 + 3] as u32 + rgba[p11 + 3] as u32) / 4) as u8;
                next[dst + 3] = ((a_max as u32 + a_avg as u32) / 2) as u8;
            }
        }
        w = nw;
        h = nh;
        rgba = next;
    }
    (w as u16, h as u16, rgba)
}

pub fn decode_tex(b: &[u8]) -> Option<Texture2D> {
    let max_dim = max_tex_dim();
    let is_jpeg = b.len() > 2 && b[0] == 0xFF && b[1] == 0xD8;
    let t = if is_jpeg {
        let mut d = jpeg_decoder::Decoder::new(b);
        let px = d.decode().ok()?;
        let info = d.info()?;
        let (w, h) = (info.width as usize, info.height as usize);
        let mut rgba = Vec::with_capacity(w * h * 4);
        match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => for p in px.chunks(3) { rgba.extend_from_slice(&[p[0], p[1], p[2], 255]); },
            jpeg_decoder::PixelFormat::L8 => for p in px.iter() { rgba.extend_from_slice(&[*p, *p, *p, 255]); },
            _ => return None,
        }
        let (nw, nh, scaled) = downscale_rgba(w, h, rgba, max_dim);
        Texture2D::from_rgba8(nw, nh, &scaled)
    } else {
        if let Ok(img) = Image::from_file_with_format(b, Some(ImageFormat::Png)) {
            let (w, h) = (img.width as usize, img.height as usize);
            let (nw, nh, scaled) = downscale_rgba(w, h, img.bytes, max_dim);
            Texture2D::from_rgba8(nw, nh, &scaled)
        } else {
            Texture2D::from_file_with_format(b, Some(ImageFormat::Png))
        }
    };
    t.set_filter(FilterMode::Linear);
    unsafe {
        let ctx = get_internal_gl().quad_context;
        let id = t.raw_miniquad_id();
        ctx.texture_set_wrap(id, macroquad::miniquad::TextureWrap::Repeat, macroquad::miniquad::TextureWrap::Repeat);
        if is_jpeg {
            ctx.texture_generate_mipmaps(id);
            ctx.texture_set_filter(id, macroquad::miniquad::FilterMode::Linear, macroquad::miniquad::MipmapFilterMode::Linear);
        } else {
            ctx.texture_set_filter(id, macroquad::miniquad::FilterMode::Linear, macroquad::miniquad::MipmapFilterMode::None);
        }
    }
    Some(t)
}

pub fn shared_white_tex() -> Texture2D {
    thread_local! {
        static W: Texture2D = {
            let t = Texture2D::from_rgba8(1, 1, &[255, 255, 255, 255]);
            t.set_filter(FilterMode::Nearest);
            t
        };
    }
    W.with(|t| t.clone())
}

/// Generates a simplified LOD1 mesh list using spatial vertex clustering per EPart.
pub fn decimate_eparts(parts: &[EPart], cell: f32) -> Vec<EPart> {
    let mut out = Vec::new();
    let inv_cell = 1.0 / cell.max(0.05);
    for p in parts {
        if p.blend { continue; }
        let mut cell_map: HashMap<(i32, i32, i32), u16> = HashMap::new();
        let mut verts: Vec<Vertex> = Vec::new();
        let mut remap: Vec<u16> = Vec::with_capacity(p.mesh.vertices.len());
        for v in &p.mesh.vertices {
            let key = (
                (v.position.x * inv_cell).floor() as i32,
                (v.position.y * inv_cell).floor() as i32,
                (v.position.z * inv_cell).floor() as i32,
            );
            let idx = *cell_map.entry(key).or_insert_with(|| {
                let id = verts.len() as u16;
                verts.push(*v);
                id
            });
            remap.push(idx);
        }
        let mut ind: Vec<u16> = Vec::new();
        for tri in p.mesh.indices.chunks(3) {
            if tri.len() < 3 { break; }
            let (a, b, c) = (remap[tri[0] as usize], remap[tri[1] as usize], remap[tri[2] as usize]);
            if a != b && b != c && a != c {
                ind.extend_from_slice(&[a, b, c]);
            }
        }
        if !ind.is_empty() {
            let mut ep = EPart::from_mesh(Mesh { vertices: verts, indices: ind, texture: p.mesh.texture.clone() }, false);
            ep.facade = p.facade;
            ep.cull_x = p.cull_x;
            ep.cull_z = p.cull_z;
            out.push(ep);
        }
    }
    out
}

/// Generates an ultra-lightweight 12-triangle box proxy for LOD2 (distant view & shadow map casting).
pub fn box_proxy_epart(half: Vec3, base_y: f32, col: [u8; 4]) -> Vec<EPart> {
    let (hx, hy, hz) = (half.x, half.y, half.z);
    let y0 = base_y;
    let y1 = base_y + hy * 2.0;
    let corners = [
        vec3(-hx, y0, -hz), vec3( hx, y0, -hz), vec3( hx, y1, -hz), vec3(-hx, y1, -hz),
        vec3(-hx, y0,  hz), vec3( hx, y0,  hz), vec3( hx, y1,  hz), vec3(-hx, y1,  hz),
    ];
    let faces: [([usize; 4], Vec3); 5] = [
        ([4, 5, 6, 7], Vec3::Z),
        ([1, 0, 3, 2], -Vec3::Z),
        ([5, 1, 2, 6], Vec3::X),
        ([0, 4, 7, 3], -Vec3::X),
        ([3, 7, 6, 2], Vec3::Y),
    ];
    let mut verts = Vec::with_capacity(20);
    let mut indices = Vec::with_capacity(30);
    for (quad, n) in faces {
        let b = verts.len() as u16;
        for &ci in &quad {
            verts.push(Vertex {
                position: corners[ci],
                uv: vec2(0.0, 0.0),
                color: col,
                normal: vec4(n.x, n.y, n.z, 0.05),
            });
        }
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    vec![EPart::from_mesh(Mesh { vertices: verts, indices, texture: Some(shared_white_tex()) }, false)]
}

/// Standard loader for multi-object GLBs (e.g. tree_types.glb, mountains.glb) where each primitive is a separate asset.
pub fn load_glb(bytes: &[u8]) -> Vec<EPart> {
    let g = match gltf::Gltf::from_slice(bytes) { Ok(g) => g, Err(_) => return vec![] };
    let blob = g.blob.clone();
    let mut tex_cache: HashMap<usize, Option<Texture2D>> = HashMap::new();
    let white = shared_white_tex();
    let mut out = Vec::new();
    let mut parent_of: HashMap<usize, usize> = HashMap::new();
    let local_mats: Vec<Mat4> = g.nodes().map(|n| Mat4::from_cols_array_2d(&n.transform().matrix())).collect();
    for n in g.nodes() {
        for ch in n.children() {
            parent_of.insert(ch.index(), n.index());
        }
    }
    let world_mat = |mut idx: usize| -> Mat4 {
        let mut m = local_mats[idx];
        while let Some(&p) = parent_of.get(&idx) {
            m = local_mats[p] * m;
            idx = p;
        }
        m
    };
    for node in g.nodes() {
        let mesh = match node.mesh() { Some(m) => m, None => continue };
        let xf = world_mat(node.index());
        let nm = Mat3::from_mat4(xf.inverse().transpose());
        for prim in mesh.primitives() {
            let reader = prim.reader(|_b| blob.as_deref());
            let pos: Vec<[f32; 3]> = match reader.read_positions() { Some(p) => p.collect(), None => continue };
            let nrm: Vec<[f32; 3]> = reader.read_normals().map(|n| n.collect()).unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; pos.len()]);
            let mat = prim.material();
            let pbr = mat.pbr_metallic_roughness();
            let f = pbr.base_color_factor();
            let mut texture = None;
            if let Some(info) = pbr.base_color_texture() {
                let img = info.texture().source();
                let key = img.index();
                texture = tex_cache.entry(key).or_insert_with(|| {
                    if let gltf::image::Source::View { view, .. } = img.source() {
                        if let Some(bl) = &blob { let s = view.offset(); let e = s + view.length(); return decode_tex(&bl[s..e]); }
                    }
                    None
                }).clone();
            }
            let has_tex = texture.is_some();
            let uv: Vec<[f32; 2]> = if has_tex {
                reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_else(|| vec![[0.0, 0.0]; pos.len()])
            } else {
                vec![[0.0, 0.0]; pos.len()]
            };
            let idx: Vec<u32> = reader.read_indices().map(|i| i.into_u32().collect()).unwrap_or_else(|| (0..pos.len() as u32).collect());
            let blend = f[3] < 0.98 && !has_tex;
            let col = if has_tex {
                [255u8, 255, 255, 255]
            } else {
                [(f[0].powf(1.0 / 2.2) * 255.0) as u8, (f[1].powf(1.0 / 2.2) * 255.0) as u8, (f[2].powf(1.0 / 2.2) * 255.0) as u8, (f[3] * 255.0) as u8]
            };
            let chunk_tex = if has_tex { texture } else { Some(white.clone()) };
            if pos.len() <= 45000 && idx.len() <= 135000 {
                let vertices: Vec<Vertex> = (0..pos.len()).map(|i| {
                    let p = xf.transform_point3(Vec3::from(pos[i]));
                    let n = (nm * Vec3::from(nrm[i])).normalize_or_zero();
                    Vertex { position: p, uv: Vec2::from(uv[i]), color: col, normal: vec4(n.x, n.y, n.z, 0.0) }
                }).collect();
                let indices: Vec<u16> = idx.iter().map(|&i| i as u16).collect();
                out.push(EPart::from_mesh(Mesh { vertices, indices, texture: chunk_tex }, blend));
            } else {
                let mut vertices: Vec<Vertex> = Vec::new();
                let mut indices: Vec<u16> = Vec::new();
                let mut vmap: HashMap<u32, u16> = HashMap::new();
                let mut flush_chunk = |vertices: &mut Vec<Vertex>, indices: &mut Vec<u16>, vmap: &mut HashMap<u32, u16>| {
                    if vertices.is_empty() { return; }
                    out.push(EPart::from_mesh(Mesh {
                        vertices: std::mem::take(vertices),
                        indices: std::mem::take(indices),
                        texture: chunk_tex.clone(),
                    }, blend));
                    vmap.clear();
                };
                for tri in idx.chunks(3) {
                    if tri.len() < 3 { break; }
                    if vertices.len() + 3 > 45000 || indices.len() + 3 > 135000 {
                        flush_chunk(&mut vertices, &mut indices, &mut vmap);
                    }
                    for &orig_i in tri {
                        let new_i = *vmap.entry(orig_i).or_insert_with(|| {
                            let i = orig_i as usize;
                            let p = xf.transform_point3(Vec3::from(pos[i]));
                            let n = (nm * Vec3::from(nrm[i])).normalize_or_zero();
                            let local_idx = vertices.len() as u16;
                            vertices.push(Vertex { position: p, uv: Vec2::from(uv[i]), color: col, normal: vec4(n.x, n.y, n.z, 0.0) });
                            local_idx
                        });
                        indices.push(new_i);
                    }
                }
                flush_chunk(&mut vertices, &mut indices, &mut vmap);
            }
        }
    }
    out
}

/// Spatially partition a large terrain GLB (like forest_ground.glb) into an NxN grid of frustum-culled EParts.
pub fn load_glb_grid(bytes: &[u8], grid_n: usize) -> Vec<EPart> {
    let base_parts = load_glb(bytes);
    if base_parts.is_empty() || grid_n <= 1 { return base_parts; }
    let (mut mn, mut mx) = (Vec3::splat(1e9), Vec3::splat(-1e9));
    for p in &base_parts {
        mn = mn.min(p.bmin);
        mx = mx.max(p.bmax);
    }
    let sx = (mx.x - mn.x).max(1.0);
    let sz = (mx.z - mn.z).max(1.0);
    let mut out = Vec::new();
    for p in base_parts {
        let mut cells: Vec<(Vec<Vertex>, Vec<u16>, HashMap<u16, u16>)> =
            (0..grid_n * grid_n).map(|_| (Vec::new(), Vec::new(), HashMap::new())).collect();
        for tri in p.mesh.indices.chunks(3) {
            if tri.len() < 3 { break; }
            let v0 = &p.mesh.vertices[tri[0] as usize];
            let v1 = &p.mesh.vertices[tri[1] as usize];
            let v2 = &p.mesh.vertices[tri[2] as usize];
            let cx = (v0.position.x + v1.position.x + v2.position.x) / 3.0;
            let cz = (v0.position.z + v1.position.z + v2.position.z) / 3.0;
            let gx = (((cx - mn.x) / sx * grid_n as f32) as usize).min(grid_n - 1);
            let gz = (((cz - mn.z) / sz * grid_n as f32) as usize).min(grid_n - 1);
            let cell = &mut cells[gz * grid_n + gx];
            for &vi in tri {
                let ni = *cell.2.entry(vi).or_insert_with(|| {
                    let id = cell.0.len() as u16;
                    cell.0.push(p.mesh.vertices[vi as usize]);
                    id
                });
                cell.1.push(ni);
            }
        }
        for (vs, is, _) in cells {
            if !vs.is_empty() {
                out.push(EPart::from_mesh(Mesh { vertices: vs, indices: is, texture: p.mesh.texture.clone() }, p.blend));
            }
        }
    }
    out
}

struct BucketBuf {
    verts: Vec<Vertex>,
    ind: Vec<u16>,
    vmap: HashMap<(usize, u32), u16>,
}

impl BucketBuf {
    fn new() -> Self {
        BucketBuf { verts: Vec::new(), ind: Vec::new(), vmap: HashMap::new() }
    }
}

/// Building loader that merges solid-color materials and partitions geometry into 11 spatial/directional facade chunks.
pub fn load_glb_building(bytes: &[u8], x_thr: f32, z_n: f32, z_s: f32, y_roof: f32) -> Vec<EPart> {
    let g = match gltf::Gltf::from_slice(bytes) { Ok(g) => g, Err(_) => return vec![] };
    let blob = g.blob.clone();
    let mut tex_cache: HashMap<usize, Option<Texture2D>> = HashMap::new();
    let white = shared_white_tex();

    let mut parent_of: HashMap<usize, usize> = HashMap::new();
    let local_mats: Vec<Mat4> = g.nodes().map(|n| Mat4::from_cols_array_2d(&n.transform().matrix())).collect();
    for n in g.nodes() {
        for ch in n.children() {
            parent_of.insert(ch.index(), n.index());
        }
    }
    let world_mat = |mut idx: usize| -> Mat4 {
        let mut m = local_mats[idx];
        while let Some(&p) = parent_of.get(&idx) {
            m = local_mats[p] * m;
            idx = p;
        }
        m
    };

    const NUM_BUCKETS: usize = 11;
    let bucket_facade: [u8; NUM_BUCKETS] = [0, 1, 1, 2, 2, 3, 4, 5, 6, 7, 8];
    let cull_z_pos = z_n - 1.8;
    let cull_z_neg = z_s + 1.8;
    let cull_x_pos = x_thr - 1.8;
    let cull_x_neg = -x_thr + 1.8;

    let mut groups: HashMap<(bool, Option<usize>), (Option<Texture2D>, Vec<BucketBuf>)> = HashMap::new();
    let mut prim_counter = 0usize;

    for node in g.nodes() {
        let mesh = match node.mesh() { Some(m) => m, None => continue };
        let xf = world_mat(node.index());
        let nm = Mat3::from_mat4(xf.inverse().transpose());
        for prim in mesh.primitives() {
            prim_counter += 1;
            let reader = prim.reader(|_b| blob.as_deref());
            let pos: Vec<[f32; 3]> = match reader.read_positions() { Some(p) => p.collect(), None => continue };
            let nrm: Vec<[f32; 3]> = reader.read_normals().map(|n| n.collect()).unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; pos.len()]);
            let mat = prim.material();
            let pbr = mat.pbr_metallic_roughness();
            let f = pbr.base_color_factor();
            let mut tex_key: Option<usize> = None;
            let mut texture: Option<Texture2D> = None;
            if let Some(info) = pbr.base_color_texture() {
                let img = info.texture().source();
                let key = img.index();
                tex_key = Some(key);
                texture = tex_cache.entry(key).or_insert_with(|| {
                    if let gltf::image::Source::View { view, .. } = img.source() {
                        if let Some(bl) = &blob { let s = view.offset(); let e = s + view.length(); return decode_tex(&bl[s..e]); }
                    }
                    None
                }).clone();
            }
            let has_tex = texture.is_some();
            if !has_tex { tex_key = None; }
            let uv: Vec<[f32; 2]> = if has_tex {
                reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_else(|| vec![[0.0, 0.0]; pos.len()])
            } else {
                vec![[0.0, 0.0]; pos.len()]
            };
            let idx: Vec<u32> = reader.read_indices().map(|i| i.into_u32().collect()).unwrap_or_else(|| (0..pos.len() as u32).collect());
            let blend = f[3] < 0.98 && !has_tex;
            let col = if has_tex {
                [255u8, 255, 255, 255]
            } else {
                [(f[0].powf(1.0 / 2.2) * 255.0) as u8, (f[1].powf(1.0 / 2.2) * 255.0) as u8, (f[2].powf(1.0 / 2.2) * 255.0) as u8, (f[3] * 255.0) as u8]
            };
            let chunk_tex = if has_tex { texture.clone() } else { Some(white.clone()) };
            let entry = groups.entry((blend, tex_key)).or_insert_with(|| {
                let mut v = Vec::with_capacity(NUM_BUCKETS);
                for _ in 0..NUM_BUCKETS { v.push(BucketBuf::new()); }
                (chunk_tex, v)
            });

            for tri in idx.chunks(3) {
                if tri.len() < 3 { break; }
                let p0 = xf.transform_point3(Vec3::from(pos[tri[0] as usize]));
                let p1 = xf.transform_point3(Vec3::from(pos[tri[1] as usize]));
                let p2 = xf.transform_point3(Vec3::from(pos[tri[2] as usize]));
                let c = (p0 + p1 + p2) * (1.0 / 3.0);
                let ymx = p0.y.max(p1.y).max(p2.y);

                let b_idx = if ymx > y_roof || c.y < 1.25 {
                    0
                } else if c.x < -x_thr && c.z > z_n {
                    8
                } else if c.x > x_thr && c.z > z_n {
                    7
                } else if c.x > x_thr && c.z < z_s {
                    9
                } else if c.x < -x_thr && c.z < z_s {
                    10
                } else if c.x < -x_thr {
                    6
                } else if c.x > x_thr {
                    5
                } else if c.z > z_n {
                    if c.x < 0.0 { 1 } else { 2 }
                } else if c.z < z_s {
                    if c.x < 0.0 { 3 } else { 4 }
                } else {
                    0
                };

                let buf = &mut entry.1[b_idx];
                for &orig_i in tri {
                    let local_i = *buf.vmap.entry((prim_counter, orig_i)).or_insert_with(|| {
                        let i = orig_i as usize;
                        let p = xf.transform_point3(Vec3::from(pos[i]));
                        let n = (nm * Vec3::from(nrm[i])).normalize_or_zero();
                        let id = buf.verts.len() as u16;
                        buf.verts.push(Vertex { position: p, uv: Vec2::from(uv[i]), color: col, normal: vec4(n.x, n.y, n.z, 0.0) });
                        id
                    });
                    buf.ind.push(local_i);
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut keys: Vec<(bool, Option<usize>)> = groups.keys().cloned().collect();
    keys.sort_by_key(|&(blend, tk)| (blend, tk.unwrap_or(9999)));
    for k in keys {
        let (blend, _) = k;
        let (chunk_tex, buckets) = groups.remove(&k).unwrap();
        for (b_idx, buf) in buckets.into_iter().enumerate() {
            if buf.ind.is_empty() { continue; }
            let facade = bucket_facade[b_idx];
            let (cx, cz) = match facade {
                1 => (0.0, cull_z_pos),
                2 => (0.0, cull_z_neg),
                3 => (cull_x_pos, 0.0),
                4 => (cull_x_neg, 0.0),
                5 => (cull_x_pos, cull_z_pos),
                6 => (cull_x_neg, cull_z_pos),
                7 => (cull_x_pos, cull_z_neg),
                8 => (cull_x_neg, cull_z_neg),
                _ => (0.0, 0.0),
            };
            if buf.verts.len() <= 42000 && buf.ind.len() <= 130000 {
                let mut ep = EPart::from_mesh(Mesh { vertices: buf.verts, indices: buf.ind, texture: chunk_tex.clone() }, blend);
                ep.facade = facade;
                ep.cull_x = cx;
                ep.cull_z = cz;
                out.push(ep);
            } else {
                let mut vs: Vec<Vertex> = Vec::new();
                let mut is: Vec<u16> = Vec::new();
                let mut remap: HashMap<u16, u16> = HashMap::new();
                let mut flush_sub = |vs: &mut Vec<Vertex>, is: &mut Vec<u16>, remap: &mut HashMap<u16, u16>| {
                    if vs.is_empty() { return; }
                    let mut ep = EPart::from_mesh(Mesh { vertices: std::mem::take(vs), indices: std::mem::take(is), texture: chunk_tex.clone() }, blend);
                    ep.facade = facade;
                    ep.cull_x = cx;
                    ep.cull_z = cz;
                    out.push(ep);
                    remap.clear();
                };
                for tri in buf.ind.chunks(3) {
                    if tri.len() < 3 { break; }
                    if vs.len() + 3 > 42000 || is.len() + 3 > 130000 {
                        flush_sub(&mut vs, &mut is, &mut remap);
                    }
                    for &old_i in tri {
                        let new_i = *remap.entry(old_i).or_insert_with(|| {
                            let id = vs.len() as u16;
                            vs.push(buf.verts[old_i as usize]);
                            id
                        });
                        is.push(new_i);
                    }
                }
                flush_sub(&mut vs, &mut is, &mut remap);
            }
        }
    }
    out
}

/// Universal loader for custom GLB assets added to the game.
pub fn load_glb_custom(bytes: &[u8]) -> (Vec<EPart>, Vec3) {
    let g = match gltf::Gltf::from_slice(bytes) { Ok(g) => g, Err(_) => return (vec![], Vec3::ONE) };
    let blob = g.blob.clone();
    let mut tex_cache: HashMap<usize, Option<Texture2D>> = HashMap::new();
    let white = shared_white_tex();

    let mut parent_of: HashMap<usize, usize> = HashMap::new();
    let local_mats: Vec<Mat4> = g.nodes().map(|n| Mat4::from_cols_array_2d(&n.transform().matrix())).collect();
    for n in g.nodes() {
        for ch in n.children() {
            parent_of.insert(ch.index(), n.index());
        }
    }
    let world_mat = |mut idx: usize| -> Mat4 {
        let mut m = local_mats[idx];
        while let Some(&p) = parent_of.get(&idx) {
            m = local_mats[p] * m;
            idx = p;
        }
        m
    };

    let (mut all_min, mut all_max) = (Vec3::splat(1e9), Vec3::splat(-1e9));
    for node in g.nodes() {
        let mesh = match node.mesh() { Some(m) => m, None => continue };
        let xf = world_mat(node.index());
        for prim in mesh.primitives() {
            let reader = prim.reader(|_b| blob.as_deref());
            if let Some(pos) = reader.read_positions() {
                for p in pos {
                    let wp = xf.transform_point3(Vec3::from(p));
                    all_min = all_min.min(wp);
                    all_max = all_max.max(wp);
                }
            }
        }
    }
    if all_min.x > all_max.x {
        return (vec![], Vec3::ONE);
    }
    let cx = if (all_min.x + all_max.x).abs() < 0.25 { 0.0 } else { (all_min.x + all_max.x) * 0.5 };
    let cz = if (all_min.z + all_max.z).abs() < 0.25 { 0.0 } else { (all_min.z + all_max.z) * 0.5 };
    let y0 = if all_min.y >= 0.0 && all_min.y <= 0.12 { 0.0 } else { all_min.y };
    let shift = vec3(cx, y0, cz);
    let ext = (all_max - all_min).max(Vec3::splat(0.4));
    let half = ext * 0.5;
    let is_large = ext.x > 80.0 || ext.z > 80.0;
    let grid_n: usize = if is_large { 4 } else { 1 };
    let min_shifted = all_min - shift;

    let mut groups: HashMap<(bool, Option<usize>), (Option<Texture2D>, Vec<BucketBuf>)> = HashMap::new();
    let mut prim_counter = 0usize;

    for node in g.nodes() {
        let mesh = match node.mesh() { Some(m) => m, None => continue };
        let xf = world_mat(node.index());
        let nm = Mat3::from_mat4(xf.inverse().transpose());
        for prim in mesh.primitives() {
            prim_counter += 1;
            let reader = prim.reader(|_b| blob.as_deref());
            let pos: Vec<[f32; 3]> = match reader.read_positions() { Some(p) => p.collect(), None => continue };
            let nrm: Vec<[f32; 3]> = reader.read_normals().map(|n| n.collect()).unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; pos.len()]);
            let mat = prim.material();
            let pbr = mat.pbr_metallic_roughness();
            let f = pbr.base_color_factor();
            let rough = pbr.roughness_factor();
            let spec_val = (0.75 - rough * 0.65).clamp(0.06, 0.85);
            let mut tex_key: Option<usize> = None;
            let mut texture: Option<Texture2D> = None;
            if let Some(info) = pbr.base_color_texture() {
                let img = info.texture().source();
                let key = img.index();
                tex_key = Some(key);
                texture = tex_cache.entry(key).or_insert_with(|| {
                    if let gltf::image::Source::View { view, .. } = img.source() {
                        if let Some(bl) = &blob { let s = view.offset(); let e = s + view.length(); return decode_tex(&bl[s..e]); }
                    }
                    None
                }).clone();
            }
            let has_tex = texture.is_some();
            if !has_tex { tex_key = None; }
            let uv: Vec<[f32; 2]> = if has_tex {
                reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_else(|| vec![[0.0, 0.0]; pos.len()])
            } else {
                vec![[0.0, 0.0]; pos.len()]
            };
            let idx: Vec<u32> = reader.read_indices().map(|i| i.into_u32().collect()).unwrap_or_else(|| (0..pos.len() as u32).collect());
            let is_blend_mode = matches!(mat.alpha_mode(), gltf::material::AlphaMode::Blend) || f[3] < 0.98;
            let blend = is_blend_mode;
            let alpha_u8 = if is_blend_mode { (f[3].min(0.96) * 255.0) as u8 } else { 255u8 };
            let col = [(f[0].powf(1.0 / 2.2) * 255.0) as u8, (f[1].powf(1.0 / 2.2) * 255.0) as u8, (f[2].powf(1.0 / 2.2) * 255.0) as u8, alpha_u8];
            let chunk_tex = if has_tex { texture.clone() } else { Some(white.clone()) };
            let entry = groups.entry((blend, tex_key)).or_insert_with(|| {
                let mut v = Vec::with_capacity(grid_n * grid_n);
                for _ in 0..(grid_n * grid_n) { v.push(BucketBuf::new()); }
                (chunk_tex, v)
            });

            for tri in idx.chunks(3) {
                if tri.len() < 3 { break; }
                let b_idx = if grid_n > 1 {
                    let p0 = xf.transform_point3(Vec3::from(pos[tri[0] as usize])) - shift;
                    let p1 = xf.transform_point3(Vec3::from(pos[tri[1] as usize])) - shift;
                    let p2 = xf.transform_point3(Vec3::from(pos[tri[2] as usize])) - shift;
                    let c = (p0 + p1 + p2) * (1.0 / 3.0);
                    let gx = (((c.x - min_shifted.x) / ext.x * grid_n as f32) as usize).min(grid_n - 1);
                    let gz = (((c.z - min_shifted.z) / ext.z * grid_n as f32) as usize).min(grid_n - 1);
                    gz * grid_n + gx
                } else {
                    0
                };
                let buf = &mut entry.1[b_idx];
                for &orig_i in tri {
                    let local_i = *buf.vmap.entry((prim_counter, orig_i)).or_insert_with(|| {
                        let i = orig_i as usize;
                        let p = xf.transform_point3(Vec3::from(pos[i])) - shift;
                        let n = (nm * Vec3::from(nrm[i])).normalize_or_zero();
                        let id = buf.verts.len() as u16;
                        buf.verts.push(Vertex { position: p, uv: Vec2::from(uv[i]), color: col, normal: vec4(n.x, n.y, n.z, spec_val) });
                        id
                    });
                    buf.ind.push(local_i);
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut keys: Vec<(bool, Option<usize>)> = groups.keys().cloned().collect();
    keys.sort_by_key(|&(blend, tk)| (blend, tk.unwrap_or(9999)));
    for k in keys {
        let (blend, _) = k;
        let (chunk_tex, buckets) = groups.remove(&k).unwrap();
        for buf in buckets {
            if buf.ind.is_empty() { continue; }
            if buf.verts.len() <= 42000 && buf.ind.len() <= 130000 {
                out.push(EPart::from_mesh(Mesh { vertices: buf.verts, indices: buf.ind, texture: chunk_tex.clone() }, blend));
            } else {
                let mut vs: Vec<Vertex> = Vec::new();
                let mut is: Vec<u16> = Vec::new();
                let mut remap: HashMap<u16, u16> = HashMap::new();
                let mut flush_sub = |vs: &mut Vec<Vertex>, is: &mut Vec<u16>, remap: &mut HashMap<u16, u16>| {
                    if vs.is_empty() { return; }
                    out.push(EPart::from_mesh(Mesh { vertices: std::mem::take(vs), indices: std::mem::take(is), texture: chunk_tex.clone() }, blend));
                    remap.clear();
                };
                for tri in buf.ind.chunks(3) {
                    if tri.len() < 3 { break; }
                    if vs.len() + 3 > 42000 || is.len() + 3 > 130000 {
                        flush_sub(&mut vs, &mut is, &mut remap);
                    }
                    for &old_i in tri {
                        let new_i = *remap.entry(old_i).or_insert_with(|| {
                            let id = vs.len() as u16;
                            vs.push(buf.verts[old_i as usize]);
                            id
                        });
                        is.push(new_i);
                    }
                }
                flush_sub(&mut vs, &mut is, &mut remap);
            }
        }
    }
    (out, half)
}

pub const BUNDLED_CUSTOM_NAMES: [&str; 9] = [
    "abadoned_building__lowpoly",
    "abandoned_city_street_for_games",
    "arabic_temple_diorama",
    "fence_concrete-_15mb",
    "free_city_road_layout_mockup",
    "gdr_park_lamp",
    "old_concrete_building_pack__lowpoly",
    "scania_touring_2026",
    "tree_low_poly_lowpoly",
];

pub fn bundled_custom_bytes(name: &str) -> Option<&'static [u8]> {
    match name {
        "abadoned_building__lowpoly" => Some(include_bytes!("../assets/models/abadoned_building__lowpoly.glb")),
        "abandoned_city_street_for_games" => Some(include_bytes!("../assets/models/abandoned_city_street_for_games.glb")),
        "arabic_temple_diorama" => Some(include_bytes!("../assets/models/arabic_temple_diorama.glb")),
        "fence_concrete-_15mb" => Some(include_bytes!("../assets/models/fence_concrete-_15mb.glb")),
        "free_city_road_layout_mockup" => Some(include_bytes!("../assets/models/free_city_road_layout_mockup.glb")),
        "gdr_park_lamp" => Some(include_bytes!("../assets/models/gdr_park_lamp.glb")),
        "old_concrete_building_pack__lowpoly" => Some(include_bytes!("../assets/models/old_concrete_building_pack__lowpoly.glb")),
        "scania_touring_2026" => Some(include_bytes!("../assets/models/scania_touring_2026.glb")),
        "tree_low_poly_lowpoly" => Some(include_bytes!("../assets/models/tree_low_poly_lowpoly.glb")),
        _ => None,
    }
}

fn collect_glb_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) { Ok(e) => e, Err(_) => return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let dname = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            if dname == "cars" || dname == "target" || dname == ".git" {
                continue;
            }
            collect_glb_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("glb")).unwrap_or(false) {
            out.push(path);
        }
    }
}

pub fn discover_custom_glbs(already_loaded: &HashSet<String>) -> Vec<(String, Vec<EPart>, Vec3)> {
    let builtin_skip: HashSet<&str> = [
        "apartment", "apartment_hi", "school", "tree_types", "trees", "mountains", "forest_ground",
        "sedan", "suv", "suv-luxury", "police", "race", "taxi", "van", "hatchback-sports", "sedan-sports", "wheel-default",
    ].into_iter().collect();

    let mut search_roots: Vec<PathBuf> = vec![
        PathBuf::from("assets/models"),
        PathBuf::from("assets/env"),
        PathBuf::from("assets"),
        PathBuf::from("mods"),
        PathBuf::from("client/assets/models"),
        PathBuf::from("client/assets/env"),
        PathBuf::from("client/assets"),
        PathBuf::from("client/mods"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            search_roots.push(p.join("assets/models"));
            search_roots.push(p.join("assets/env"));
            search_roots.push(p.join("assets"));
            search_roots.push(p.join("mods"));
            if let Some(pp) = p.parent().and_then(|x| x.parent()) {
                search_roots.push(pp.join("assets/models"));
                search_roots.push(pp.join("assets/env"));
                search_roots.push(pp.join("assets"));
            }
        }
    }

    let mut files = Vec::new();
    for root in search_roots {
        collect_glb_files(&root, &mut files);
    }

    let mut seen_names: HashSet<String> = already_loaded.clone();
    let mut candidates: Vec<(String, PathBuf)> = Vec::new();
    for path in files {
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let lower = stem.to_lowercase();
        if builtin_skip.contains(lower.as_str()) || seen_names.contains(&stem) {
            continue;
        }
        seen_names.insert(stem.clone());
        candidates.push((stem, path));
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0));

    let mut results = Vec::new();
    for (name, path) in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            let (parts, half) = load_glb_custom(&bytes);
            if !parts.is_empty() {
                results.push((name, parts, half));
            }
        }
    }
    results
}
