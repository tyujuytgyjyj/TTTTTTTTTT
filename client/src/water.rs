// نظام المياه: محيط (WaterPlane) وأنهار (River) بسطح موجي وانعكاس للسماء وتوهج الشمس.
// الملف water.bin تنتجه الأداة tools/bake_water.py. إن فشل تجميع الشيدر لا تتعطل اللعبة (تُتجاهل المياه).
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Comparison, Equation, PipelineParams, UniformDesc, UniformType};
use macroquad::prelude::*;

#[derive(Clone, Copy)]
struct WaterClipPoint {
    x: f32,
    z: f32,
    h: f32,
}

fn clip_water_triangle(tri: [WaterClipPoint; 3], level: f32) -> Vec<WaterClipPoint> {
    let mut polygon = Vec::with_capacity(4);
    for i in 0..3 {
        let a = tri[i];
        let b = tri[(i + 1) % 3];
        let inside_a = a.h < level;
        let inside_b = b.h < level;
        if inside_a && inside_b {
            polygon.push(b);
        } else if inside_a && !inside_b {
            let denom = b.h - a.h;
            if denom.abs() > 1.0e-6 {
                let t = ((level - a.h) / denom).clamp(0.0, 1.0);
                polygon.push(WaterClipPoint { x: a.x + (b.x - a.x) * t, z: a.z + (b.z - a.z) * t, h: level });
            }
        } else if !inside_a && inside_b {
            let denom = b.h - a.h;
            if denom.abs() > 1.0e-6 {
                let t = ((level - a.h) / denom).clamp(0.0, 1.0);
                polygon.push(WaterClipPoint { x: a.x + (b.x - a.x) * t, z: a.z + (b.z - a.z) * t, h: level });
            }
            polygon.push(b);
        }
    }
    polygon
}

fn enqueue_water_cell(
    x: usize, y: usize, width: usize, height: usize,
    submerged: &[bool], connected: &mut [bool],
    queue: &mut std::collections::VecDeque<(usize, usize)>,
) {
    if x >= width || y >= height { return; }
    let index = y * width + x;
    if submerged[index] && !connected[index] {
        connected[index] = true;
        queue.push_back((x, y));
    }
}

fn emit_water_polygon(
    out: &mut Vec<Mesh>, verts: &mut Vec<Vertex>, idx: &mut Vec<u16>,
    polygon: &[WaterClipPoint], level: f32, color: [u8; 4],
) {
    if polygon.len() < 3 { return; }
    // Reject almost-zero shoreline slivers caused by heightfield quantisation.
    let mut twice_area = 0.0f32;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        twice_area += a.x * b.z - b.x * a.z;
    }
    if twice_area.abs() < 0.08 { return; }
    if verts.len() + polygon.len() > 60000 && !verts.is_empty() {
        out.push(Mesh { vertices: std::mem::take(verts), indices: std::mem::take(idx), texture: None });
    }
    let base = verts.len() as u16;
    for p in polygon {
        // Encode the clipped shoreline edge in alpha for the OGRE bridge's
        // foam highlight. The final water draw remains fully opaque.
        let mut vertex_color = color;
        if (p.h - level).abs() < 0.002 { vertex_color[3] = 210; }
        verts.push(Vertex {
            position: vec3(p.x, level + 0.012, p.z),
            uv: vec2(p.x * 0.002, p.z * 0.002),
            color: vertex_color,
            normal: vec4(0.0, 1.0, 0.0, 0.0),
        });
    }
    for i in 1..(polygon.len() - 1) {
        idx.extend_from_slice(&[base, base + i as u16, base + (i + 1) as u16]);
    }
}

fn plane_cells(hf: &std::sync::Arc<crate::world::HeightField>, lvl: f32, col: [u8; 4]) -> Vec<Mesh> {
    const STEP: usize = 2;
    let n = hf.n;
    if n <= STEP { return Vec::new(); }
    let cells_x = (n - 1) / STEP;
    let cells_y = (n - 1) / STEP;
    if cells_x == 0 || cells_y == 0 { return Vec::new(); }

    // A small shoreline inset avoids the thin, washed-out water film where
    // quantised terrain lies almost exactly at the nominal sea level.
    const SHORELINE_INSET: f32 = 0.30;
    let surface_level = lvl - SHORELINE_INSET;

    // Build the submerged-cell mask. Only cells connected to the heightfield
    // border belong to the open ocean; isolated inland depressions stay dry.
    let mut submerged = vec![false; cells_x * cells_y];
    for cy in 0..cells_y {
        for cx in 0..cells_x {
            let ix = cx * STEP;
            let iy = cy * STEP;
            let h00 = hf.hat(ix, iy);
            let h10 = hf.hat(ix + STEP, iy);
            let h01 = hf.hat(ix, iy + STEP);
            let h11 = hf.hat(ix + STEP, iy + STEP);
            submerged[cy * cells_x + cx] = h00 < surface_level || h10 < surface_level || h01 < surface_level || h11 < surface_level;
        }
    }

    let mut connected = vec![false; submerged.len()];
    let mut queue = std::collections::VecDeque::new();
    for x in 0..cells_x {
        enqueue_water_cell(x, 0, cells_x, cells_y, &submerged, &mut connected, &mut queue);
        enqueue_water_cell(x, cells_y - 1, cells_x, cells_y, &submerged, &mut connected, &mut queue);
    }
    for y in 0..cells_y {
        enqueue_water_cell(0, y, cells_x, cells_y, &submerged, &mut connected, &mut queue);
        enqueue_water_cell(cells_x - 1, y, cells_x, cells_y, &submerged, &mut connected, &mut queue);
    }
    while let Some((x, y)) = queue.pop_front() {
        if x > 0 { enqueue_water_cell(x - 1, y, cells_x, cells_y, &submerged, &mut connected, &mut queue); }
        if x + 1 < cells_x { enqueue_water_cell(x + 1, y, cells_x, cells_y, &submerged, &mut connected, &mut queue); }
        if y > 0 { enqueue_water_cell(x, y - 1, cells_x, cells_y, &submerged, &mut connected, &mut queue); }
        if y + 1 < cells_y { enqueue_water_cell(x, y + 1, cells_x, cells_y, &submerged, &mut connected, &mut queue); }
    }

    // Share the visible ocean mask with tire and chassis physics.
    crate::world::set_water_region(hf.clone(), STEP, surface_level, connected.clone());

    let mut out = Vec::new();
    let mut verts = Vec::new();
    let mut idx = Vec::new();
    for cy in 0..cells_y {
        for cx in 0..cells_x {
            if !connected[cy * cells_x + cx] { continue; }
            let ix = cx * STEP;
            let iy = cy * STEP;
            let x0 = hf.ox + ix as f32 * hf.cell;
            let x1 = hf.ox + (ix + STEP) as f32 * hf.cell;
            let z0 = -(hf.oy + iy as f32 * hf.cell);
            let z1 = -(hf.oy + (iy + STEP) as f32 * hf.cell);
            let p00 = WaterClipPoint { x: x0, z: z0, h: hf.hat(ix, iy) };
            let p10 = WaterClipPoint { x: x1, z: z0, h: hf.hat(ix + STEP, iy) };
            let p01 = WaterClipPoint { x: x0, z: z1, h: hf.hat(ix, iy + STEP) };
            let p11 = WaterClipPoint { x: x1, z: z1, h: hf.hat(ix + STEP, iy + STEP) };

            // Preserve the old diagonal, but cut every triangle at the exact
            // waterline instead of raising the whole 2m cell for one low corner.
            let poly_a = clip_water_triangle([p01, p10, p11], surface_level);
            emit_water_polygon(&mut out, &mut verts, &mut idx, &poly_a, surface_level, col);
            let poly_b = clip_water_triangle([p01, p00, p10], surface_level);
            emit_water_polygon(&mut out, &mut verts, &mut idx, &poly_b, surface_level, col);
        }
    }
    if !verts.is_empty() {
        out.push(Mesh { vertices: verts, indices: idx, texture: None });
    }
    out
}

pub struct Water {
    meshes: Vec<Mesh>,
    mat: Material,
    pub level: f32,
}

impl Water {
    pub fn load() -> Option<Water> {
        let path = crate::world::map_path("water.bin")?;
        let b = std::fs::read(path).ok()?;
        if b.len() < 8 || &b[0..4] != b"WAT1" { return None; }
        let rd = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let rf = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let n = rd(4) as usize;
        let mut p = 8usize;
        let mut meshes = Vec::new();
        let mut level = -1.0e9f32;
        let mut physics_level = -1.0e9f32;
        let mut has_terrain_ocean = false;
        for _ in 0..n {
            if p + 12 > b.len() { break; }
            let (nv, ni, lvl) = (rd(p) as usize, rd(p + 4) as usize, rf(p + 8));
            p += 12;
            let vend = p + nv * 16;
            let iend = vend + ni * 2;
            if iend > b.len() { break; }
            if lvl > -1.0e8 { level = level.max(lvl); }
            let mut verts = Vec::with_capacity(nv);
            for i in 0..nv {
                let o = p + i * 16;
                verts.push(Vertex {
                    position: vec3(rf(o), rf(o + 4), rf(o + 8)),
                    uv: vec2(0.0, 0.0),
                    color: [b[o + 12], b[o + 13], b[o + 14], b[o + 15]],
                    normal: vec4(0.0, 1.0, 0.0, 0.0),
                });
            }
            let mut idx = Vec::with_capacity(ni);
            for i in 0..ni { idx.push(u16::from_le_bytes([b[vend + i * 2], b[vend + i * 2 + 1]])); }
            p = iend;
            if nv == 4 && lvl > -1.0e8 {
                // المحيط: نبني سطح الماء فقط فوق خلايا التضاريس التي تقع تحت مستواه (لا مستوى لانهائي)
                let col = [verts[0].color[0], verts[0].color[1], verts[0].color[2], 255];
                if let Some(hf) = crate::world::terrain_hf() {
                    let effective_level = lvl - 0.30;
                    meshes.extend(plane_cells(&hf, lvl, col));
                    physics_level = physics_level.max(effective_level);
                    has_terrain_ocean = true;
                    continue;
                }
            }
            meshes.push(Mesh { vertices: verts, indices: idx, texture: None });
        }
        if meshes.is_empty() { return None; }
        let active_level = if has_terrain_ocean { physics_level } else { level };
        crate::world::set_water_level(active_level);
        let fs = format!(
            "#version 100\nprecision highp float;\n{}\n{}",
            include_str!("../assets/shaders/common.glsl"),
            include_str!("../assets/shaders/water.frag")
        );
        let mat = load_material(
            ShaderSource::Glsl { vertex: include_str!("../assets/shaders/water.vert"), fragment: &fs },
            MaterialParams {
                pipeline_params: PipelineParams {
                    depth_test: Comparison::LessOrEqual,
                    depth_write: false,
                    color_blend: Some(BlendState::new(
                        Equation::Add,
                        BlendFactor::Value(BlendValue::SourceAlpha),
                        BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                    )),
                    ..Default::default()
                },
                uniforms: vec![
                    UniformDesc::new("VP", UniformType::Mat4),
                    UniformDesc::new("sun_dir", UniformType::Float3),
                    UniformDesc::new("cam_pos", UniformType::Float3),
                    UniformDesc::new("fog_col", UniformType::Float3),
                    UniformDesc::new("misc", UniformType::Float4),
                ],
                textures: vec![],
            },
        )
        .ok()?;
        Some(Water { meshes, mat, level: active_level })
    }

    pub fn draw(&self, vp: Mat4, cp: Vec3) {
        let e = crate::world::get_env();
        gl_use_material(&self.mat);
        self.mat.set_uniform("VP", vp);
        self.mat.set_uniform("sun_dir", vec3(e[0], e[1], e[2]));
        self.mat.set_uniform("cam_pos", cp);
        self.mat.set_uniform("fog_col", vec3(e[3], e[4], e[5]));
        self.mat.set_uniform("misc", vec4(e[6], e[7], 0.0, 0.0));
        for m in &self.meshes { draw_mesh(m); }
    }
}
