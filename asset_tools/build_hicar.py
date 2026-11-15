#!/usr/bin/env python3
"""Converts the Sketchfab Mercedes-AMG One .glb and the wheel pack into compact CrashNG model files (.cdm).
Usage: build_hicar.py amg.glb wheelpack.glb out_dir
Game coordinates: x = left(+), y = up, z = forward(+), metres, ground at y=0, origin at bbox centre (x,z)."""
import sys, io, struct, json
import numpy as np
from PIL import Image

def load_glb(path):
    b = open(path, 'rb').read()
    l = struct.unpack('<I', b[12:16])[0]
    j = json.loads(b[20:20 + l])
    off = 20 + l
    bl = struct.unpack('<I', b[off:off + 4])[0]
    return j, b[off + 8:off + 8 + bl]

CT = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
NC = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4}

def acc(j, blob, i):
    a = j['accessors'][i]; bv = j['bufferViews'][a['bufferView']]
    dt = np.dtype(CT[a['componentType']]); n = NC[a['type']]
    stride = bv.get('byteStride', dt.itemsize * n)
    start = bv.get('byteOffset', 0) + a.get('byteOffset', 0)
    if stride == dt.itemsize * n:
        arr = np.frombuffer(blob, dtype=dt, count=a['count'] * n, offset=start).reshape(a['count'], n)
    else:
        arr = np.stack([np.frombuffer(blob, dtype=dt, count=n, offset=start + k * stride) for k in range(a['count'])])
    if a.get('normalized'):
        arr = arr.astype(np.float32) / np.iinfo(dt).max
    return arr

def srgb(c):  # linear -> sRGB (our shader expects sRGB inputs and linearises itself)
    c = np.clip(np.asarray(c, dtype=np.float64), 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)

class Part:
    def __init__(s, group, flags, tex, color, spec, emis):
        s.group, s.flags, s.tex, s.color, s.spec, s.emis = group, flags, tex, color, spec, emis
        s.P = []; s.N = []; s.U = []; s.I = []; s.n = 0

FL_GLASS, FL_PAINT, FL_SPIN, FL_DECAL, FL_TYRE = 1, 2, 4, 8, 16

def encode_tex(img_bytes):
    im = Image.open(io.BytesIO(img_bytes))
    if im.mode == 'P': im = im.convert('RGBA' if 'transparency' in im.info else 'RGB')
    elif im.mode in ('1', 'L', 'LA'): im = im.convert('RGBA' if im.mode == 'LA' else 'RGB')
    has_alpha = im.mode == 'RGBA' and im.getchannel('A').getextrema()[0] < 250
    if im.mode == 'RGBA' and not has_alpha: im = im.convert('RGB')
    out = io.BytesIO()
    if has_alpha or im.size[0] * im.size[1] <= 64:
        im.save(out, 'PNG', optimize=True); return 1, out.getvalue()
    im.save(out, 'JPEG', quality=90); return 0, out.getvalue()

def write_cdm(path, header, textures, parts, proxy):
    f = io.BytesIO()
    f.write(b'CDM1')
    f.write(struct.pack('<II', len(parts), len(textures)))
    f.write(struct.pack('<I', len(header)))
    f.write(struct.pack('<%df' % len(header), *header))
    for fmt, data in textures:
        f.write(struct.pack('<BI', fmt, len(data))); f.write(data)
    for p in parts:
        P = np.concatenate(p.P).astype('<f4'); N = np.concatenate(p.N).astype('<f4'); U = np.concatenate(p.U).astype('<f4')
        I = np.concatenate(p.I).astype('<u2')
        f.write(struct.pack('<bBi4ffII', p.group, p.flags, p.tex, *p.color, p.spec, len(P), len(I)))
        f.write(struct.pack('<f', p.emis))
        inter = np.concatenate([P, N, U], axis=1).astype('<f4')
        f.write(inter.tobytes()); f.write(I.tobytes())
    pp, pn, pi = proxy
    f.write(struct.pack('<II', len(pp), len(pi)))
    f.write(np.concatenate([pp, pn], axis=1).astype('<f4').tobytes()); f.write(np.asarray(pi, '<u2').tobytes())
    open(path, 'wb').write(f.getvalue())
    print(path, len(f.getvalue()) // 1024, 'KB', 'parts', len(parts), 'tex', len(textures))

def cluster_proxy(P, I, cell):
    """vertex-clustering decimation -> coarse proxy mesh (shadow + soft-body binding)."""
    key = np.floor(P / cell).astype(np.int64)
    k = key[:, 0] * 73856093 ^ key[:, 1] * 19349663 ^ key[:, 2] * 83492791
    uniq, inv = np.unique(k, return_inverse=True)
    cp = np.zeros((len(uniq), 3)); cnt = np.zeros(len(uniq))
    np.add.at(cp, inv, P); np.add.at(cnt, inv, 1); cp /= cnt[:, None]
    T = inv[I.reshape(-1, 3)]
    ok = (T[:, 0] != T[:, 1]) & (T[:, 1] != T[:, 2]) & (T[:, 0] != T[:, 2])
    T = T[ok]
    nrm = np.zeros_like(cp)
    fn = np.cross(cp[T[:, 1]] - cp[T[:, 0]], cp[T[:, 2]] - cp[T[:, 0]])
    for c in range(3): np.add.at(nrm, T[:, c], fn)
    nrm /= np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-9)
    return cp, nrm, T.reshape(-1)

def build_amg(src, out):
    j, blob = load_glb(src)
    S = 2.725 / 11.5            # model units -> metres (wheelbase of the real car: 2.725 m)
    XC, YC = -15.57, (-7.77 + 12.30) / 2
    def conv_p(P): return np.stack([-(P[:, 0] - XC), P[:, 2], (P[:, 1] - YC)], 1) * S
    def conv_n(N): return np.stack([-N[:, 0], N[:, 2], N[:, 1]], 1)
    # wheel centres [FL, FR, RL, RR] (game coords)
    cen_o = [(-19.16, 1.46, -7.95), (-11.98, 1.46, -7.95), (-19.17, 1.52, 3.55), (-11.98, 1.52, 3.55)]
    # original (x, y_up=z, z' = -y)  ->  game
    cg = np.array([[-(x - XC) * S, y * S, (-zp - YC) * S] for (x, y, zp) in cen_o])
    cg[0, 0] = abs(cg[0, 0]); cg[1, 0] = -abs(cg[1, 0]); cg[2, 0] = abs(cg[2, 0]); cg[3, 0] = -abs(cg[3, 0])
    R = 1.46 * S; HW = 0.95 * S
    print('wheel centres', cg.round(3).tolist())
    # image index per texture
    texsrc = [t['source'] for t in j['textures']]
    bv = j['bufferViews']
    used = {}
    textures = []
    def tex_id(ti):
        if ti is None: return -1
        if ti in used: return used[ti]
        im = j['images'][texsrc[ti]]; v = bv[im['bufferView']]
        data = blob[v.get('byteOffset', 0):v.get('byteOffset', 0) + v['byteLength']]
        used[ti] = len(textures); textures.append(encode_tex(data)); return used[ti]
    parts = {}
    allP = []; allI = []; base = 0
    for mi, m in enumerate(j['meshes']):
        pr = m['primitives'][0]
        P = conv_p(acc(j, blob, pr['attributes']['POSITION']).astype(np.float64))
        N = conv_n(acc(j, blob, pr['attributes']['NORMAL']).astype(np.float64))
        U = acc(j, blob, pr['attributes']['TEXCOORD_0']).astype(np.float64) if 'TEXCOORD_0' in pr['attributes'] else np.zeros((len(P), 2))
        T = acc(j, blob, pr['indices']).astype(np.int64).reshape(-1, 3)
        mat = j['materials'][pr['material']]; pbr = mat.get('pbrMetallicRoughness', {})
        base_c = pbr.get('baseColorFactor', [1, 1, 1, 1]); tex = pbr.get('baseColorTexture', {}).get('index')
        rough = pbr.get('roughnessFactor', 0.6); name = mat['name']
        blend = mat.get('alphaMode') == 'BLEND'
        glass = blend and base_c[3] <= 0.35
        paint = tex is None and not blend and min(base_c[:3]) > 0.95
        emis = 1.0 if mat.get('emissiveFactor') else 0.0
        spin = name.startswith('Wheel')
        spec = 0.9 if glass else (0.75 if paint else float(np.clip(0.55 - rough * 0.5, 0.08, 0.5)))
        if 'Interior' in name or 'Fsteer0031' in name: spec = 0.22
        rgb = srgb(base_c[:3]); alpha = 0.5 if glass else 1.0
        if glass: rgb = np.array([0.05, 0.075, 0.085])
        # triangle -> wheel group
        grp = np.full(len(P), -1)
        for wi in range(4):
            d = P - cg[wi]; r = np.hypot(d[:, 1], d[:, 2]); ok = (r <= R) & (np.abs(d[:, 0]) <= HW)
            grp[ok] = wi
        tg = np.where((grp[T] == grp[T][:, :1]).all(1), grp[T][:, 0], -1)
        for g in range(-1, 4):
            sel = T[tg == g]
            if len(sel) == 0: continue
            flags = (FL_GLASS if glass else 0) | (FL_PAINT if paint else 0) | (FL_SPIN if (spin and g >= 0) else 0)
            if blend and not glass: flags |= FL_DECAL
            key = (g, pr['material'], flags)
            if key not in parts:
                parts[key] = Part(g, flags, tex_id(tex), [*rgb, alpha], spec, emis)
            p = parts[key]
            used_v, inv = np.unique(sel, return_inverse=True)
            Pv = P[used_v] - (cg[g] if g >= 0 else 0)
            Nv = N[used_v]; Uv = U[used_v]
            idx = inv.reshape(-1, 3)
            # split into <=65535-vertex chunks (u16 indices)
            if p.n + len(Pv) > 65000:
                k2 = (key, len(parts)); parts[k2] = Part(g, flags, p.tex, p.color, p.spec, p.emis); p = parts[k2]
            p.P.append(Pv); p.N.append(Nv); p.U.append(Uv); p.I.append((idx + p.n).reshape(-1)); p.n += len(Pv)
            if g == -1 and not glass and not (blend and not glass):
                allP.append(P[used_v]); allI.append((idx + base).reshape(-1)); base += len(used_v)
    plist = []
    for p in parts.values():
        if p.n == 0 or p.n > 65535: continue
        plist.append(p)
    plist.sort(key=lambda p: (p.flags & FL_GLASS != 0, p.group))
    body = np.concatenate(allP); bi = np.concatenate(allI)
    bmin, bmax = body.min(0), body.max(0)
    print('body bbox', bmin.round(3), bmax.round(3))
    proxy = cluster_proxy(body, bi, 0.17)
    print('proxy verts', len(proxy[0]), 'tris', len(proxy[2]) // 3)
    rr = [1.46 * S, 1.46 * S, 1.52 * S - 0.0, 1.52 * S]
    radii = [float(cg[i, 1]) for i in range(4)]
    header = [*cg.reshape(-1), *radii, *bmin, *bmax]
    write_cdm(out + '/amg_one.cdm', header, textures, plist, proxy)

def build_wheel(src, out):
    j, blob = load_glb(src)
    # Wheel01: tyre + rim + silver; axis = X, radius ~0.57 -> normalise so tyre outer radius = 1
    parts = []; allv = []
    meshes = {n['name']: n for n in j['nodes']}
    node = meshes['Wheel01']; m = j['meshes'][node['mesh']]
    cols = []
    for pr in m['primitives']:
        P = acc(j, blob, pr['attributes']['POSITION']).astype(np.float64)
        allv.append(P)
    allP = np.concatenate(allv); rad = np.hypot(allP[:, 1], allP[:, 2]).max()
    ctr_x = (allP[:, 0].min() + allP[:, 0].max()) / 2
    for pr in m['primitives']:
        mat = j['materials'][pr['material']]; pbr = mat['pbrMetallicRoughness']
        P = acc(j, blob, pr['attributes']['POSITION']).astype(np.float64)
        N = acc(j, blob, pr['attributes']['NORMAL']).astype(np.float64)
        I = acc(j, blob, pr['indices']).astype(np.int64).reshape(-1)
        name = mat['name']
        col = srgb(pbr['baseColorFactor'][:3]); spec = 0.55 if name.startswith('silver') else 0.12
        if name.startswith('Rim'): col = np.array([0.18, 0.18, 0.19])
        if name.startswith('silver'): col = np.array([0.62, 0.63, 0.66])
        if name.startswith('Tyre'): col = np.array([0.045, 0.045, 0.048])
        p = Part(0, FL_SPIN | (FL_TYRE if name.startswith('Tyre') else 0), -1, [*col, 1.0], spec, 0.0)
        Pn = (P - [ctr_x, 0, 0]) / rad
        p.P.append(Pn); p.N.append(N); p.U.append(np.zeros((len(P), 2))); p.I.append(I); p.n = len(P)
        parts.append(p)
    allp = np.concatenate([q.P[0] for q in parts])
    width = allp[:, 0].max() - allp[:, 0].min()
    header = [1.0, width, 0, 0]
    pr_ = (np.zeros((0, 3)), np.zeros((0, 3)), np.zeros(0))
    write_cdm(out + '/default_wheel.cdm', header, [], parts, pr_)

if __name__ == '__main__':
    import os
    os.makedirs(sys.argv[3], exist_ok=True)
    build_amg(sys.argv[1], sys.argv[3])
    build_wheel(sys.argv[2], sys.argv[3])
