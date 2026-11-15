// Minimal COLLADA (.dae) reader: geometries -> triangle lists (one sub-mesh per material).
use macroquad::prelude::*;
use std::collections::HashMap;

pub struct SubMesh { pub material: String, pub idx: Vec<u32> }
pub struct Geom {
    pub id: String,
    pub name: String,
    pub pos: Vec<Vec3>,   // BeamNG space (x left, y back, z up) converted to game space later
    pub nrm: Vec<Vec3>,
    pub uv: Vec<Vec2>,
    pub subs: Vec<SubMesh>,
}

/// Ear-clipping triangulation of a (possibly concave) planar polygon given in 3D.
fn triangulate(pts: &[Vec3]) -> Vec<[usize; 3]> {
    let n = pts.len();
    let mut out = Vec::new();
    if n < 3 { return out; }
    if n == 3 { return vec![[0, 1, 2]]; }
    let mut nrm = Vec3::ZERO;
    for i in 0..n { let a = pts[i]; let b = pts[(i + 1) % n]; nrm += vec3((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y)); }
    let ax = if nrm.x.abs() >= nrm.y.abs() && nrm.x.abs() >= nrm.z.abs() { 0 } else if nrm.y.abs() >= nrm.z.abs() { 1 } else { 2 };
    let flip = match ax { 0 => nrm.x < 0.0, 1 => nrm.y < 0.0, _ => nrm.z < 0.0 };
    let p2: Vec<Vec2> = pts.iter().map(|p| { let (u, v) = match ax { 0 => (p.y, p.z), 1 => (p.z, p.x), _ => (p.x, p.y) }; vec2(u, v) }).collect();
    let mut idx: Vec<usize> = (0..n).collect();
    if flip { idx.reverse(); }
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let mut guard = 0;
    while idx.len() > 3 && guard < 10000 {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (p2[ia], p2[ib], p2[ic]);
            if cross(a, b, c) <= 1e-12 { continue; }
            let mut inside = false;
            for &k in &idx {
                if k == ia || k == ib || k == ic { continue; }
                let p = p2[k];
                if cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0 { inside = true; break; }
            }
            if inside { continue; }
            out.push(if flip { [ic, ib, ia] } else { [ia, ib, ic] });
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped { break; }
    }
    if idx.len() == 3 { out.push(if flip { [idx[2], idx[1], idx[0]] } else { [idx[0], idx[1], idx[2]] }); }
    else if idx.len() > 3 { for k in 1..idx.len() - 1 { out.push([idx[0], idx[k], idx[k + 1]]); } }
    out
}

fn find_from(s: &str, pat: &str, from: usize) -> Option<usize> { s[from..].find(pat).map(|i| i + from) }

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{}=\"", name);
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')? + i;
    Some(&tag[i..j])
}

fn floats(s: &str) -> Vec<f32> { s.split_ascii_whitespace().filter_map(|x| x.parse().ok()).collect() }
fn ints(s: &str) -> Vec<u32> { s.split_ascii_whitespace().filter_map(|x| x.parse().ok()).collect() }

pub fn parse(text: &str) -> Vec<Geom> {
    let mut out = Vec::new();
    let mut at = match text.find("<library_geometries") { Some(i) => i, None => return out };
    while let Some(gs) = find_from(text, "<geometry ", at) {
        let ge = match find_from(text, "</geometry>", gs) { Some(e) => e, None => break };
        let g = &text[gs..ge];
        at = ge + 11;
        let head_end = g.find('>').unwrap_or(0);
        let head = &g[..head_end];
        let name = attr(head, "name").or(attr(head, "id")).unwrap_or("").to_string();
        let gid = attr(head, "id").unwrap_or("").to_string();
        // sources
        let mut sources: HashMap<String, (Vec<f32>, usize)> = HashMap::new();
        let mut p = 0;
        while let Some(si) = find_from(g, "<source ", p) {
            let se = match find_from(g, "</source>", si) { Some(e) => e, None => break };
            let sx = &g[si..se];
            p = se + 9;
            let id = attr(&sx[..sx.find('>').unwrap_or(0)], "id").unwrap_or("").to_string();
            if let Some(fa) = sx.find("<float_array") {
                let fe = sx[fa..].find('>').map(|i| i + fa + 1).unwrap_or(fa);
                let fend = sx[fe..].find("</float_array>").map(|i| i + fe).unwrap_or(sx.len());
                let vals = floats(&sx[fe..fend]);
                let stride = sx.find("<accessor").and_then(|a| attr(&sx[a..], "stride")).and_then(|s| s.parse().ok()).unwrap_or(3);
                sources.insert(id, (vals, stride));
            }
        }
        // vertices -> position source
        let mut vert_src = String::new();
        let mut vert_id = String::new();
        if let Some(vi) = g.find("<vertices") {
            let ve = g[vi..].find("</vertices>").map(|i| i + vi).unwrap_or(g.len());
            vert_id = attr(&g[vi..], "id").unwrap_or("").to_string();
            if let Some(ii) = g[vi..ve].find("semantic=\"POSITION\"") {
                if let Some(s) = attr(&g[vi + ii..], "source") { vert_src = s.trim_start_matches('#').to_string(); }
            }
        }
        let pos_data = match sources.get(&vert_src) { Some(v) => v, None => continue };
        // primitive groups
        let mut subs_raw: Vec<(String, Vec<(String, usize, String)>, Vec<u32>, Vec<u32>, bool)> = Vec::new();
        let mut q = 0;
        loop {
            let tri = find_from(g, "<triangles", q);
            let pl = find_from(g, "<polylist", q);
            let (st, is_poly) = match (tri, pl) { (Some(a), Some(b)) => if a < b { (a, false) } else { (b, true) }, (Some(a), None) => (a, false), (None, Some(b)) => (b, true), _ => break };
            let endtag = if is_poly { "</polylist>" } else { "</triangles>" };
            let en = match find_from(g, endtag, st) { Some(e) => e, None => break };
            let blk = &g[st..en];
            q = en + endtag.len();
            let head = &blk[..blk.find('>').unwrap_or(0)];
            let material = attr(head, "material").unwrap_or("").to_string();
            let mut inputs = Vec::new();
            let mut ip = 0;
            while let Some(ii) = find_from(blk, "<input ", ip) {
                let ie = blk[ii..].find('>').map(|i| i + ii).unwrap_or(blk.len());
                let tag = &blk[ii..ie];
                ip = ie;
                let sem = attr(tag, "semantic").unwrap_or("").to_string();
                let off: usize = attr(tag, "offset").and_then(|s| s.parse().ok()).unwrap_or(0);
                let src = attr(tag, "source").unwrap_or("").trim_start_matches('#').to_string();
                inputs.push((sem, off, src));
            }
            let vcount = blk.find("<vcount>").map(|i| { let e = blk[i..].find("</vcount>").map(|x| x + i).unwrap_or(blk.len()); ints(&blk[i + 8..e]) }).unwrap_or_default();
            let pdata = blk.find("<p>").map(|i| { let e = blk[i..].find("</p>").map(|x| x + i).unwrap_or(blk.len()); ints(&blk[i + 3..e]) }).unwrap_or_default();
            subs_raw.push((material, inputs, vcount, pdata, is_poly));
        }
        if subs_raw.is_empty() { continue; }
        let mut geom = Geom { id: gid.clone(), name: name.clone(), pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), subs: Vec::new() };
        let mut cache: HashMap<(u32, u32, u32), u32> = HashMap::new();
        for (material, inputs, vcount, pdata, is_poly) in subs_raw {
            let stride = inputs.iter().map(|i| i.1).max().unwrap_or(0) + 1;
            let find_in = |sem: &str| inputs.iter().find(|i| i.0 == sem).cloned();
            let vi = find_in("VERTEX");
            let ni = find_in("NORMAL");
            let ti = find_in("TEXCOORD");
            let vi = match vi { Some(v) => v, None => continue };
            let nsrc = ni.as_ref().and_then(|n| sources.get(&n.2));
            let tsrc = ti.as_ref().and_then(|t| sources.get(&t.2));
            let _ = &vert_id;
            let corner = |c: usize, geom: &mut Geom, cache: &mut HashMap<(u32, u32, u32), u32>| -> u32 {
                let base = c * stride;
                let pi = pdata[base + vi.1];
                let ni_ = ni.as_ref().map(|n| pdata[base + n.1]).unwrap_or(u32::MAX);
                let ti_ = ti.as_ref().map(|t| pdata[base + t.1]).unwrap_or(u32::MAX);
                *cache.entry((pi, ni_, ti_)).or_insert_with(|| {
                    let ps = pos_data.1;
                    let p = vec3(pos_data.0[pi as usize * ps], pos_data.0[pi as usize * ps + 1], pos_data.0[pi as usize * ps + 2]);
                    geom.pos.push(p);
                    let n = match nsrc { Some((d, s)) if ni_ != u32::MAX => vec3(d[ni_ as usize * s], d[ni_ as usize * s + 1], d[ni_ as usize * s + 2]), _ => Vec3::Z };
                    geom.nrm.push(n);
                    let t = match tsrc { Some((d, s)) if ti_ != u32::MAX => vec2(d[ti_ as usize * s], 1.0 - d[ti_ as usize * s + 1]), _ => Vec2::ZERO };
                    geom.uv.push(t);
                    (geom.pos.len() - 1) as u32
                })
            };
            let mut idx: Vec<u32> = Vec::new();
            let total_corners = pdata.len() / stride.max(1);
            if !is_poly {
                let mut c = 0;
                while c + 2 < total_corners { let a = corner(c, &mut geom, &mut cache); let b = corner(c + 1, &mut geom, &mut cache); let d = corner(c + 2, &mut geom, &mut cache); idx.extend_from_slice(&[a, b, d]); c += 3; }
            } else {
                let mut c = 0;
                for vc in vcount {
                    let vc = vc as usize;
                    if c + vc > total_corners { break; }
                    let ids: Vec<u32> = (0..vc).map(|k| corner(c + k, &mut geom, &mut cache)).collect();
                    if vc == 3 { idx.extend_from_slice(&ids); }
                    else if vc == 4 {
                        // split along the shorter diagonal
                        let p = |i: usize| geom.pos[ids[i] as usize];
                        if (p(0) - p(2)).length_squared() <= (p(1) - p(3)).length_squared() { idx.extend_from_slice(&[ids[0], ids[1], ids[2], ids[0], ids[2], ids[3]]); }
                        else { idx.extend_from_slice(&[ids[0], ids[1], ids[3], ids[1], ids[2], ids[3]]); }
                    } else {
                        let pts: Vec<Vec3> = ids.iter().map(|&i| geom.pos[i as usize]).collect();
                        for t in triangulate(&pts) { idx.extend_from_slice(&[ids[t[0]], ids[t[1]], ids[t[2]]]); }
                    }
                    c += vc;
                }
            }
            geom.subs.push(SubMesh { material, idx });
        }
        out.push(geom);
    }
    out
}

pub struct SceneNode { pub name: String, pub geoms: Vec<String>, pub m: Mat4 }

pub fn parse_scene(text: &str) -> Vec<SceneNode> {
    let mut out = Vec::new();
    let start = match text.find("<library_visual_scenes") { Some(i) => i, None => return out };
    let mut at = start;
    while let Some(ns) = find_from(text, "<node ", at) {
        let ne = match find_from(text, "</node>", ns) { Some(e) => e, None => break };
        let blk = &text[ns..ne];
        at = ns + 6;
        let head = &blk[..blk.find('>').unwrap_or(0)];
        let name = attr(head, "name").or(attr(head, "id")).unwrap_or("").to_string();
        let mut m = Mat4::IDENTITY;
        if let Some(mi) = blk.find("<matrix") {
            let s = blk[mi..].find('>').map(|i| i + mi + 1).unwrap_or(mi);
            let e = blk[s..].find("</matrix>").map(|i| i + s).unwrap_or(s);
            let f = floats(&blk[s..e]);
            if f.len() == 16 { m = Mat4::from_cols_array(&[f[0], f[4], f[8], f[12], f[1], f[5], f[9], f[13], f[2], f[6], f[10], f[14], f[3], f[7], f[11], f[15]]); }
        }
        let mut geoms = Vec::new();
        let mut p = 0;
        while let Some(i) = find_from(blk, "<instance_geometry", p) {
            let e = blk[i..].find('>').map(|x| x + i).unwrap_or(blk.len());
            if let Some(u) = attr(&blk[i..e], "url") { geoms.push(u.trim_start_matches('#').to_string()); }
            p = e;
        }
        if !geoms.is_empty() { out.push(SceneNode { name, geoms, m }); }
    }
    out
}
