import json, struct, io, os
import numpy as np
from PIL import Image

def write_glb(parts, out_path):
    """
    parts: list of dict(
        pos: (N, 3) float32,
        nrm: (N, 3) float32,
        uv: (N, 2) float32 or None,
        idx: (M, 3) uint32,
        col: [r, g, b, a] float (0..1),
        img_bytes: bytes (PNG/JPEG) or None,
        img_mime: str ('image/png' or 'image/jpeg'),
        name: str
    )
    """
    buffer_views = []
    accessors = []
    meshes = []
    nodes = []
    materials = []
    textures = []
    images = []
    blob_parts = []
    offset = 0

    def add_bytes(b, target=None):
        nonlocal offset
        pad = (4 - (len(b) % 4)) % 4
        bv = {"bufferView": len(buffer_views), "byteOffset": offset, "byteLength": len(b)}
        if target is not None:
            bv["target"] = target
        buffer_views.append(bv)
        blob_parts.append(b + b'\x00' * pad)
        offset += len(b) + pad
        return len(buffer_views) - 1

    img_cache = {}
    for i, p in enumerate(parts):
        pos = np.ascontiguousarray(p['pos'], dtype=np.float32)
        nrm = np.ascontiguousarray(p['nrm'], dtype=np.float32)
        idx = np.ascontiguousarray(p['idx'].reshape(-1), dtype=np.uint32)

        bv_pos = add_bytes(pos.tobytes(), 34962)
        acc_pos = len(accessors)
        accessors.append({
            "bufferView": bv_pos,
            "componentType": 5126,
            "count": len(pos),
            "type": "VEC3",
            "min": [float(x) for x in pos.min(0)],
            "max": [float(x) for x in pos.max(0)],
        })

        bv_nrm = add_bytes(nrm.tobytes(), 34962)
        acc_nrm = len(accessors)
        accessors.append({
            "bufferView": bv_nrm,
            "componentType": 5126,
            "count": len(nrm),
            "type": "VEC3",
        })

        attrs = {"POSITION": acc_pos, "NORMAL": acc_nrm}
        if p.get('uv') is not None and p.get('img_bytes') is not None:
            uv = np.ascontiguousarray(p['uv'], dtype=np.float32)
            bv_uv = add_bytes(uv.tobytes(), 34962)
            acc_uv = len(accessors)
            accessors.append({
                "bufferView": bv_uv,
                "componentType": 5126,
                "count": len(uv),
                "type": "VEC2",
            })
            attrs["TEXCOORD_0"] = acc_uv

        bv_idx = add_bytes(idx.tobytes(), 34963)
        acc_idx = len(accessors)
        accessors.append({
            "bufferView": bv_idx,
            "componentType": 5125,
            "count": len(idx),
            "type": "SCALAR",
        })

        col = [float(x) for x in p['col']]
        pbr = {
            "baseColorFactor": col,
            "metallicFactor": 0.0,
            "roughnessFactor": 0.85,
        }
        if p.get('img_bytes') is not None and p.get('uv') is not None:
            ib = p['img_bytes']
            k = hash(ib)
            if k not in img_cache:
                bv_img = add_bytes(ib)
                im_idx = len(images)
                images.append({"bufferView": bv_img, "mimeType": p.get('img_mime', 'image/png')})
                tx_idx = len(textures)
                textures.append({"source": im_idx})
                img_cache[k] = tx_idx
            pbr["baseColorTexture"] = {"index": img_cache[k]}

        mat_idx = len(materials)
        mat_obj = {
            "name": p.get('name', f'mat_{i}'),
            "pbrMetallicRoughness": pbr,
            "doubleSided": True,
        }
        if col[3] < 0.98 and p.get('img_bytes') is None:
            mat_obj["alphaMode"] = "BLEND"
        materials.append(mat_obj)

        mesh_idx = len(meshes)
        meshes.append({
            "name": p.get('name', f'part{i}'),
            "primitives": [{
                "attributes": attrs,
                "indices": acc_idx,
                "material": mat_idx,
                "mode": 4,
            }]
        })
        nodes.append({"name": p.get('name', f'part{i}'), "mesh": mesh_idx})

    gltf = {
        "asset": {"version": "2.0", "generator": "CrashDrive FullQuality Builder"},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": [
            {k: v for k, v in bv.items() if k != "bufferView"} | {"buffer": 0}
            for bv in buffer_views
        ],
        "buffers": [{"byteLength": offset}],
    }
    if textures:
        gltf["textures"] = textures
        gltf["images"] = images

    jb = json.dumps(gltf, separators=(',', ':')).encode('utf-8')
    jpad = (4 - (len(jb) % 4)) % 4
    jb += b' ' * jpad
    blob = b''.join(blob_parts)
    total_len = 12 + 8 + len(jb) + 8 + len(blob)

    with open(out_path, 'wb') as f:
        f.write(struct.pack('<4sII', b'glTF', 2, total_len))
        f.write(struct.pack('<I4s', len(jb), b'JSON'))
        f.write(jb)
        f.write(struct.pack('<I4s', len(blob), b'BIN\x00'))
        f.write(blob)
    print(f"Wrote {out_path}: {len(parts)} parts, {sum(len(p['idx']) for p in parts)} tris, {os.path.getsize(out_path)//1024} KB")

# ---------------------------------------------------------
# 1. Build FULL ORIGINAL QUALITY school.glb from 1793983218242_the_urban_estate_of_n__g__grigoriev.glb
# ---------------------------------------------------------
orig_school_path = '/working_dir/c_ba0227500f3bc57f/user_uploaded_files/ttl=63d/user_uploaded/1793983218242_the_urban_estate_of_n__g__grigoriev.glb'
b = open(orig_school_path, 'rb').read()
l = struct.unpack('<I', b[12:16])[0]
j = json.loads(b[20:20+l])
blob = b[20+l+8:]

def get_acc_raw(j, blob, idx):
    acc = j['accessors'][idx]
    bv = j['bufferViews'][acc['bufferView']]
    off = bv.get('byteOffset', 0) + acc.get('byteOffset', 0)
    cnt = acc['count']
    ct = acc['componentType']
    tp = acc['type']
    ncomp = {'SCALAR':1, 'VEC2':2, 'VEC3':3, 'VEC4':4}[tp]
    dt = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}[ct]
    stride = bv.get('byteStride', 0)
    if stride and stride != np.dtype(dt).itemsize * ncomp:
        raw = np.frombuffer(blob, dtype=np.uint8, count=cnt*stride, offset=off).reshape(cnt, stride)
        return np.frombuffer(raw[:, :np.dtype(dt).itemsize*ncomp].tobytes(), dtype=dt).reshape(cnt, ncomp)
    arr = np.frombuffer(blob, dtype=dt, count=cnt*ncomp, offset=off)
    return arr.reshape(cnt, ncomp) if ncomp > 1 else arr

nodes = j.get('nodes', [])
parent_map = {}
for ni, n in enumerate(nodes):
    for ch in n.get('children', []):
        parent_map[ch] = ni

def node_mat(ni):
    n = nodes[ni]
    M = np.array(n['matrix'], float).reshape(4, 4).T if 'matrix' in n else np.eye(4)
    return node_mat(parent_map[ni]) @ M if ni in parent_map else M

raw_school_items = []
for ni, n in enumerate(nodes):
    if 'mesh' not in n: continue
    m = j['meshes'][n['mesh']]
    mname = m.get('name', '')
    if mname == 'Plane__0' or 'Signs' in mname:
        continue
    M = node_mat(ni)
    NM = np.linalg.inv(M[:3, :3]).T
    for p in m['primitives']:
        pos = get_acc_raw(j, blob, p['attributes']['POSITION']).astype(np.float64)
        pos = (pos @ M[:3, :3].T) + M[:3, 3]
        nrm = get_acc_raw(j, blob, p['attributes']['NORMAL']).astype(np.float64)
        nrm = nrm @ NM.T
        nl = np.linalg.norm(nrm, axis=1, keepdims=True); nl[nl == 0] = 1.0
        nrm = nrm / nl
        uv = get_acc_raw(j, blob, p['attributes']['TEXCOORD_0']).astype(np.float64) if 'TEXCOORD_0' in p['attributes'] else None
        idx = get_acc_raw(j, blob, p['indices']).reshape(-1, 3)
        mat = j['materials'][p['material']]
        mat_name = mat.get('name', '')
        pbr = mat.get('pbrMetallicRoughness', {})
        col = list(pbr.get('baseColorFactor', [1.0, 1.0, 1.0, 1.0]))
        if mat_name == 'ParalaxMaterial.001':
            # Interior window glass plane: dark slate blue-grey instead of solid white
            col = [0.12, 0.15, 0.20, 1.0]
            uv = None
        tex_info = pbr.get('baseColorTexture')
        img_bytes = None
        img_mime = 'image/png'
        if tex_info is not None and uv is not None:
            # Check KHR_texture_transform
            ext_tt = tex_info.get('extensions', {}).get('KHR_texture_transform')
            if ext_tt:
                uv = uv.copy()
                if 'offset' in ext_tt:
                    uv += np.array(ext_tt['offset'])
                if 'rotation' in ext_tt:
                    r = ext_tt['rotation']
                    c, s = np.cos(r), np.sin(r)
                    R = np.array([[c, s], [-s, c]])
                    uv = uv @ R.T
                if 'scale' in ext_tt:
                    uv *= np.array(ext_tt['scale'])
            tex_idx = tex_info['index']
            im_idx = j['textures'][tex_idx]['source']
            im_obj = j['images'][im_idx]
            bv = j['bufferViews'][im_obj['bufferView']]
            off = bv.get('byteOffset', 0)
            raw_im_bytes = blob[off:off+bv['byteLength']]
            img_bytes = raw_im_bytes
            img_mime = im_obj.get('mimeType', 'image/png')
            col = [1.0, 1.0, 1.0, 1.0]
        v0, v1, v2 = pos[idx[:,0]], pos[idx[:,1]], pos[idx[:,2]]
        area = 0.5 * np.linalg.norm(np.cross(v1 - v0, v2 - v0), axis=1)
        if 'BlackMetal' in mname or 'GrayMetal' in mname:
            keep = area >= 0.00015
        elif 'WhiteGipsym' in mname:
            keep = area >= 0.00010
        else:
            keep = np.ones(len(idx), dtype=bool)
        idx = idx[keep]
        used, inv = np.unique(idx, return_inverse=True)
        pos = pos[used]
        nrm = nrm[used]
        if uv is not None:
            uv = uv[used]
        idx = inv.reshape(-1, 3)
        raw_school_items.append(dict(
            pos=pos, nrm=nrm, uv=uv if img_bytes is not None else None, idx=idx, col=col,
            img_bytes=img_bytes, img_mime=img_mime, name=mname
        ))

all_school_v = np.vstack([it['pos'] for it in raw_school_items])
mn_s, mx_s = all_school_v.min(0), all_school_v.max(0)
cx_s, cz_s = 0.5 * (mn_s[0] + mx_s[0]), 0.5 * (mn_s[2] + mx_s[2])
shift_s = np.array([cx_s, mn_s[1], cz_s])
for it in raw_school_items:
    it['pos'] = (it['pos'] - shift_s).astype(np.float32)
    it['nrm'] = it['nrm'].astype(np.float32)
    if it['uv'] is not None:
        it['uv'] = it['uv'].astype(np.float32)

write_glb(raw_school_items, '/tmp/inspect/crashdrive/assets/env/school.glb')

# ---------------------------------------------------------
# 2. Build FULL REPAIRED apartment_hi.glb and apartment.glb
# ---------------------------------------------------------
b_hi = open('assets/env/apartment_hi.glb', 'rb').read()
l_hi = struct.unpack('<I', b_hi[12:16])[0]
j_hi = json.loads(b_hi[20:20+l_hi])
blob_hi = b_hi[20+l_hi+8:]

b_lo = open('assets/env/apartment.glb', 'rb').read()
l_lo = struct.unpack('<I', b_lo[12:16])[0]
j_lo = json.loads(b_lo[20:20+l_lo])
blob_lo = b_lo[20+l_lo+8:]

def load_apt_raw(j, blob):
    imgs_bytes = []
    imgs_mime = []
    for im in j.get('images', []):
        bv = j['bufferViews'][im['bufferView']]
        off = bv.get('byteOffset', 0)
        imgs_bytes.append(blob[off:off+bv['byteLength']])
        imgs_mime.append(im.get('mimeType', 'image/png'))
    parts = []
    for mi, m in enumerate(j['meshes']):
        p = m['primitives'][0]
        pos = get_acc_raw(j, blob, p['attributes']['POSITION']).astype(np.float32)
        uv = get_acc_raw(j, blob, p['attributes']['TEXCOORD_0']).astype(np.float32) if 'TEXCOORD_0' in p['attributes'] else None
        idx = get_acc_raw(j, blob, p['indices']).reshape(-1, 3).astype(np.uint32)
        mat = j['materials'][p['material']]
        pbr = mat['pbrMetallicRoughness']
        col = list(pbr.get('baseColorFactor', [1.0, 1.0, 1.0, 1.0]))
        tex_idx = pbr.get('baseColorTexture', {}).get('index')
        ib = imgs_bytes[j['textures'][tex_idx]['source']] if tex_idx is not None else None
        imime = imgs_mime[j['textures'][tex_idx]['source']] if tex_idx is not None else 'image/png'
        v0, v1, v2 = pos[idx[:,0]], pos[idx[:,1]], pos[idx[:,2]]
        fn = np.cross(v1 - v0, v2 - v0)
        fn_u = fn / np.maximum(np.linalg.norm(fn, axis=1, keepdims=True), 1e-9)
        nrm = np.zeros_like(pos)
        nrm[idx[:,0]] = fn_u; nrm[idx[:,1]] = fn_u; nrm[idx[:,2]] = fn_u
        parts.append(dict(pos=pos, nrm=nrm, uv=uv, idx=idx, col=col, img_bytes=ib, img_mime=imime, name=f'part{mi}'))
    return parts

hi_apt = load_apt_raw(j_hi, blob_hi)
lo_apt = load_apt_raw(j_lo, blob_lo)

def add_box3d(v_list, n_list, uv_list, x0, x1, y0, y1, z0, z1):
    faces = [
        ([x0,y0,z1], [x1,y0,z1], [x1,y1,z1], [x0,y1,z1], [ 0, 0, 1]),
        ([x1,y0,z0], [x0,y0,z0], [x0,y1,z0], [x1,y1,z0], [ 0, 0,-1]),
        ([x0,y0,z0], [x0,y0,z1], [x0,y1,z1], [x0,y1,z0], [-1, 0, 0]),
        ([x1,y0,z1], [x1,y0,z0], [x1,y1,z0], [x1,y1,z1], [ 1, 0, 0]),
        ([x0,y1,z1], [x1,y1,z1], [x1,y1,z0], [x0,y1,z0], [ 0, 1, 0]),
        ([x0,y0,z0], [x1,y0,z0], [x1,y0,z1], [x0,y0,z1], [ 0,-1, 0]),
    ]
    uvs_q = [[0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.8]]
    for p0, p1, p2, p3, n in faces:
        for (pa, ua), (pb, ub), (pc, uc) in [((p0,uvs_q[0]),(p1,uvs_q[1]),(p2,uvs_q[2])), ((p0,uvs_q[0]),(p2,uvs_q[2]),(p3,uvs_q[3]))]:
            v_list.append(pa); n_list.append(n); uv_list.append(ua)
            v_list.append(pb); n_list.append(n); uv_list.append(ub)
            v_list.append(pc); n_list.append(n); uv_list.append(uc)

cols_x = [(-14.8, -13.2), (-11.8, -10.2), (-8.8, -7.2), (-5.8, -4.2), (-2.8, -1.2),
          (1.2, 2.8), (4.2, 5.8), (7.2, 8.8), (10.2, 11.8), (13.2, 14.8)]
floors_y = [(1.50, 3.25), (4.60, 6.35), (7.70, 9.45), (10.80, 12.55), (13.90, 15.65)]
side_z = [(-4.57, -2.97), (0.43, 2.03)]

def get_front_holes():
    holes = []
    for ci, (x0, x1) in enumerate(cols_x):
        for fi, (y0, y1) in enumerate(floors_y):
            if ci in (2, 7) and fi == 0: holes.append((x0, x1, 0.65, 3.17))
            elif ci in (1, 3, 6, 8) and fi > 0: holes.append((x0, x1, y0 - 0.79, y1))
            else: holes.append((x0, x1, y0, y1))
    return holes
def get_back_holes(): return [(x0, x1, y0, y1) for (x0, x1) in cols_x for (y0, y1) in floors_y]
def get_side_holes(): return [(z0, z1, y0, y1) for (z0, z1) in side_z for (y0, y1) in floors_y]

def build_wall_with_holes(u_min, u_max, v_min, v_max, holes, to_3d, uv_fn, normal):
    us = sorted(set([u_min, u_max] + [h[0] for h in holes] + [h[1] for h in holes]))
    vs = sorted(set([v_min, v_max] + [h[2] for h in holes] + [h[3] for h in holes]))
    verts, nrms, uvs = [], [], []
    for i in range(len(us) - 1):
        u0, u1 = us[i], us[i+1]; um = 0.5 * (u0 + u1)
        for j in range(len(vs) - 1):
            v0, v1 = vs[j], vs[j+1]; vm = 0.5 * (v0 + v1)
            if any(h[0] - 1e-4 <= um <= h[1] + 1e-4 and h[2] - 1e-4 <= vm <= h[3] + 1e-4 for h in holes): continue
            p00 = to_3d(u0, v0); p10 = to_3d(u1, v0); p11 = to_3d(u1, v1); p01 = to_3d(u0, v1)
            for tri in [(p00, p10, p11), (p00, p11, p01)]:
                for p in tri: verts.append(p); nrms.append(normal); uvs.append(uv_fn(p))
    return verts, nrms, uvs

v0_list, n0_list, uv0_list = [], [], []
def add_wall(u_min, u_max, v_min, v_max, holes, to_3d, uv_fn, normal):
    vv, nn, uu = build_wall_with_holes(u_min, u_max, v_min, v_max, holes, to_3d, uv_fn, normal)
    v0_list.extend(vv); n0_list.extend(nn); uv0_list.extend(uu)

fh, bh, sh = get_front_holes(), get_back_holes(), get_side_holes()
uv_front_out = lambda p: [-0.010734*p[1] + 0.194923, -0.010738*p[0] + 0.679950]
uv_front_in  = lambda p: [-0.010741*p[1] + 0.569737,  0.010733*p[0] + 0.813179]
uv_back_out  = lambda p: [-0.010737*p[1] + 0.382378,  0.010734*p[0] + 0.808748]
uv_back_in   = lambda p: [-0.010749*p[1] + 0.569884, -0.010730*p[0] + 0.459873]
uv_left_out  = lambda p: [-0.010709*p[1] + 0.194932, -0.010692*p[2] + 0.902284]
uv_left_in   = lambda p: [ 0.010762*p[2] + 0.655216,  0.010743*p[1] + 0.442481]
uv_right_out = lambda p: [ 0.010687*p[2] + 0.097219,  0.010660*p[1] + 0.314270]
uv_right_in  = lambda p: [-0.010767*p[2] + 0.627725,  0.010744*p[1] + 0.255078]

add_wall(-16.0, 16.0, 0.65, 16.35, fh, lambda x, y: [x, y, 4.725], uv_front_out, [0, 0, 1])
# add_wall(-15.58, 15.58, 0.65, 16.35, fh, lambda x, y: [x, y, 4.305], uv_front_in, [0, 0, -1])
add_wall(-16.0, 16.0, 0.65, 16.35, bh, lambda x, y: [x, y, -7.275], uv_back_out, [0, 0, -1])
# add_wall(-15.58, 15.58, 0.65, 16.35, bh, lambda x, y: [x, y, -6.855], uv_back_in, [0, 0, 1])
add_wall(-7.275, 4.725, 0.65, 16.35, sh, lambda z, y: [-16.0, y, z], uv_left_out, [-1, 0, 0])
# add_wall(-6.855, 4.305, 0.65, 16.35, sh, lambda z, y: [-15.58, y, z], uv_left_in, [1, 0, 0])
add_wall(-7.275, 4.725, 0.65, 16.35, sh, lambda z, y: [16.0, y, z], uv_right_out, [1, 0, 0])
# add_wall(-6.855, 4.305, 0.65, 16.35, sh, lambda z, y: [15.58, y, z], uv_right_in, [-1, 0, 0])

def add_quad0(p0, p1, p2, p3, n, uv_fn):
    for tri in [(p0, p1, p2), (p0, p2, p3)]:
        for p in tri: v0_list.append(p); n0_list.append(n); uv0_list.append(uv_fn(p))

for x0, x1, y0, y1 in fh:
    add_quad0([x0, y0, 4.305], [x0, y0, 4.725], [x0, y1, 4.725], [x0, y1, 4.305], [ 1, 0, 0], uv_front_out)
    add_quad0([x1, y0, 4.725], [x1, y0, 4.305], [x1, y1, 4.305], [x1, y1, 4.725], [-1, 0, 0], uv_front_out)
    add_quad0([x0, y0, 4.305], [x1, y0, 4.305], [x1, y0, 4.725], [x0, y0, 4.725], [ 0, 1, 0], uv_front_out)
    add_quad0([x0, y1, 4.725], [x1, y1, 4.725], [x1, y1, 4.305], [x0, y1, 4.305], [ 0,-1, 0], uv_front_out)
for x0, x1, y0, y1 in bh:
    add_quad0([x0, y0, -7.275], [x0, y0, -6.855], [x0, y1, -6.855], [x0, y1, -7.275], [ 1, 0, 0], uv_back_out)
    add_quad0([x1, y0, -6.855], [x1, y0, -7.275], [x1, y1, -7.275], [x1, y1, -6.855], [-1, 0, 0], uv_back_out)
    add_quad0([x0, y0, -7.275], [x1, y0, -7.275], [x1, y0, -6.855], [x0, y0, -6.855], [ 0, 1, 0], uv_back_out)
    add_quad0([x0, y1, -6.855], [x1, y1, -6.855], [x1, y1, -7.275], [x0, y1, -7.275], [ 0,-1, 0], uv_back_out)
for z0, z1, y0, y1 in sh:
    for x_out, x_in, uv_fn in [(-16.0, -15.58, uv_left_out), (15.58, 16.0, uv_right_out)]:
        add_quad0([x_out, y0, z0], [x_in, y0, z0], [x_in, y1, z0], [x_out, y1, z0], [0, 0, 1], uv_fn)
        add_quad0([x_in, y0, z1], [x_out, y0, z1], [x_out, y1, z1], [x_in, y1, z1], [0, 0,-1], uv_fn)
        add_quad0([x_out, y0, z0], [x_out, y0, z1], [x_in, y0, z1], [x_in, y0, z0], [0, 1, 0], uv_fn)
        add_quad0([x_in, y1, z0], [x_in, y1, z1], [x_out, y1, z1], [x_out, y1, z0], [0,-1, 0], uv_fn)
for cx0, cx1 in [(-9.7, -8.9), (-3.0, -2.2), (6.3, 7.1), (11.6, 12.4)]:
    cy0, cy1, cz0, cz1 = 18.0, 20.0, -2.72, -1.82
    add_quad0([cx0, cy0, cz1], [cx1, cy0, cz1], [cx1, cy1, cz1], [cx0, cy1, cz1], [0, 0, 1], uv_front_out)
    add_quad0([cx1, cy0, cz0], [cx0, cy0, cz0], [cx0, cy1, cz0], [cx1, cy1, cz0], [0, 0,-1], uv_back_out)
    add_quad0([cx0, cy0, cz0], [cx0, cy0, cz1], [cx0, cy1, cz1], [cx0, cy1, cz0], [-1,0, 0], uv_left_out)
    add_quad0([cx1, cy0, cz1], [cx1, cy0, cz0], [cx1, cy1, cz0], [cx1, cy1, cz1], [ 1,0, 0], uv_right_out)

pos0_new = np.array(v0_list, dtype=np.float32)
nrm0_new = np.array(n0_list, dtype=np.float32)
uv0_new  = np.array(uv0_list, dtype=np.float32)
idx0_new = np.arange(len(pos0_new), dtype=np.uint32).reshape(-1, 3)

# Build Part 4: Blinds from hi_apt[4] + exact 3D T-frames
pos4_h = hi_apt[4]['pos'].reshape(-1, 3, 3)
uv4_h  = hi_apt[4]['uv'].reshape(-1, 3, 2)
dy4 = np.ptp(pos4_h[:,:,1], axis=1); dx4 = np.ptp(pos4_h[:,:,0], axis=1); dz4 = np.ptp(pos4_h[:,:,2], axis=1)
blinds_mask = (dy4 <= 0.04) & (np.minimum(dx4, dz4) <= 0.06)
v4_list = list(pos4_h[blinds_mask].reshape(-1, 3))
uv4_list = list(uv4_h[blinds_mask].reshape(-1, 2))
fn_b = np.cross(pos4_h[blinds_mask,1]-pos4_h[blinds_mask,0], pos4_h[blinds_mask,2]-pos4_h[blinds_mask,0])
fn_b /= np.maximum(np.linalg.norm(fn_b, axis=1, keepdims=True), 1e-9)
n4_list = list(np.repeat(fn_b, 3, axis=0))

def add_window_frame_z(x0, x1, y0, y1, z0, z1, is_balcony=False):
    yt = y0 + (2.14 if is_balcony else 1.35)
    add_box3d(v4_list, n4_list, uv4_list, x0, x0+0.09, y0, y1, z0, z1)
    add_box3d(v4_list, n4_list, uv4_list, x1-0.09, x1, y0, y1, z0, z1)
    add_box3d(v4_list, n4_list, uv4_list, x0+0.09, x1-0.09, y0, y0+0.09, z0, z1)
    add_box3d(v4_list, n4_list, uv4_list, x0+0.09, x1-0.09, y1-0.09, y1, z0, z1)
    add_box3d(v4_list, n4_list, uv4_list, x0+0.09, x1-0.09, yt, yt+0.06, z0, z1)
    xm = 0.5 * (x0 + x1)
    add_box3d(v4_list, n4_list, uv4_list, xm-0.03, xm+0.03, y0+0.09, yt, z0, z1)

def add_window_frame_x(z0, z1, y0, y1, x0, x1):
    yt = y0 + 1.35
    add_box3d(v4_list, n4_list, uv4_list, x0, x1, y0, y1, z0, z0+0.09)
    add_box3d(v4_list, n4_list, uv4_list, x0, x1, y0, y1, z1-0.09, z1)
    add_box3d(v4_list, n4_list, uv4_list, x0, x1, y0, y0+0.09, z0+0.09, z1-0.09)
    add_box3d(v4_list, n4_list, uv4_list, x0, x1, y1-0.09, y1, z0+0.09, z1-0.09)
    add_box3d(v4_list, n4_list, uv4_list, x0, x1, yt, yt+0.06, z0+0.09, z1-0.09)
    zm = 0.5 * (z0 + z1)
    add_box3d(v4_list, n4_list, uv4_list, x0, x1, y0+0.09, yt, zm-0.03, zm+0.03)

for ci, (x0, x1) in enumerate(cols_x):
    for fi, (y0, y1) in enumerate(floors_y):
        if ci in (2, 7) and fi == 0:
            add_box3d(v4_list, n4_list, uv4_list, x0, x0+0.09, 0.65, 3.17, 4.52, 4.65)
            add_box3d(v4_list, n4_list, uv4_list, x1-0.09, x1, 0.65, 3.17, 4.52, 4.65)
            add_box3d(v4_list, n4_list, uv4_list, x0, x1, 3.08, 3.17, 4.52, 4.65)
        elif ci in (1, 3, 6, 8) and fi > 0:
            add_window_frame_z(x0, x1, y0 - 0.79, y1, 4.52, 4.63, is_balcony=True)
        else:
            add_window_frame_z(x0, x1, y0, y1, 4.52, 4.63, is_balcony=False)
        add_window_frame_z(x0, x1, y0, y1, -7.18, -7.07, is_balcony=False)

for (z0, z1) in side_z:
    for (y0, y1) in floors_y:
        add_window_frame_x(z0, z1, y0, y1, -15.91, -15.80)
        add_window_frame_x(z0, z1, y0, y1,  15.80,  15.91)

pos4_new = np.array(v4_list, dtype=np.float32)
nrm4_new = np.array(n4_list, dtype=np.float32)
uv4_new  = np.array(uv4_list, dtype=np.float32)
idx4_new = np.arange(len(pos4_new), dtype=np.uint32).reshape(-1, 3)

# Build Part 2: lo_apt[2] + crisp 3-sided window architraves
v2_list = list(lo_apt[2]['pos']); n2_list = list(lo_apt[2]['nrm']); uv2_list = list(lo_apt[2]['uv'])
for ci, (x0, x1) in enumerate(cols_x):
    for fi, (y0, y1) in enumerate(floors_y):
        yb = 0.65 if (ci in (2, 7) and fi == 0) else (y0 - 0.79 if (ci in (1, 3, 6, 8) and fi > 0) else y0)
        add_box3d(v2_list, n2_list, uv2_list, x0-0.11, x0, yb-0.05, y1+0.11, 4.725, 4.855)
        add_box3d(v2_list, n2_list, uv2_list, x1, x1+0.11, yb-0.05, y1+0.11, 4.725, 4.855)
        add_box3d(v2_list, n2_list, uv2_list, x0-0.11, x1+0.11, y1, y1+0.11, 4.725, 4.855)
        add_box3d(v2_list, n2_list, uv2_list, x0-0.11, x0, y0-0.05, y1+0.11, -7.405, -7.275)
        add_box3d(v2_list, n2_list, uv2_list, x1, x1+0.11, y0-0.05, y1+0.11, -7.405, -7.275)
        add_box3d(v2_list, n2_list, uv2_list, x0-0.11, x1+0.11, y1, y1+0.11, -7.405, -7.275)
for (z0, z1) in side_z:
    for (y0, y1) in floors_y:
        add_box3d(v2_list, n2_list, uv2_list, -16.13, -16.00, y0-0.05, y1+0.11, z0-0.11, z0)
        add_box3d(v2_list, n2_list, uv2_list, -16.13, -16.00, y0-0.05, y1+0.11, z1, z1+0.11)
        add_box3d(v2_list, n2_list, uv2_list, -16.13, -16.00, y1, y1+0.11, z0-0.11, z1+0.11)
        add_box3d(v2_list, n2_list, uv2_list, 16.00, 16.13, y0-0.05, y1+0.11, z0-0.11, z0)
        add_box3d(v2_list, n2_list, uv2_list, 16.00, 16.13, y0-0.05, y1+0.11, z1, z1+0.11)
        add_box3d(v2_list, n2_list, uv2_list, 16.00, 16.13, y1, y1+0.11, z0-0.11, z1+0.11)

pos2_new = np.array(v2_list, dtype=np.float32)
nrm2_new = np.array(n2_list, dtype=np.float32)
uv2_new  = np.array(uv2_list, dtype=np.float32)
idx2_new = np.arange(len(pos2_new), dtype=np.uint32).reshape(-1, 3)

final_apt = []
for i in range(24):
    base = dict(hi_apt[i])
    if i == 0:
        base['pos'], base['nrm'], base['uv'], base['idx'] = pos0_new, nrm0_new, uv0_new, idx0_new
    elif i == 2:
        base['pos'], base['nrm'], base['uv'], base['idx'] = pos2_new, nrm2_new, uv2_new, idx2_new
    elif i == 4:
        base['pos'], base['nrm'], base['uv'], base['idx'] = pos4_new, nrm4_new, uv4_new, idx4_new
    elif i in (1, 3, 8):
        base = dict(lo_apt[i])
    final_apt.append(base)

write_glb(final_apt, '/tmp/inspect/crashdrive/assets/env/apartment_hi.glb')
write_glb(final_apt, '/tmp/inspect/crashdrive/assets/env/apartment.glb')
