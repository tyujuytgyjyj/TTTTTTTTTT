# يحوّل مجسمات الخريطة (DAE) ومواقعها (forest4 + TSStatic) إلى حزمة props تفهمها لعبة APEX.drive
# الاستعمال: python bake_props.py west_coast_usa_bw.zip out_dir
import sys, os, io, json, struct, zipfile
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from daeparse import parse_dae
zpath, out = sys.argv[1], sys.argv[2]
import re
def jload(t):
    t = t.decode('utf8', 'replace') if isinstance(t, bytes) else t
    return json.loads(re.sub(r',(\s*[}\]])', r'\1', t))
z = zipfile.ZipFile(zpath); names = z.namelist(); low = {n.lower(): n for n in names}
os.makedirs(out + '/tex', exist_ok=True)
# ---- المواد -> الخريطة اللونية
matmap = {}
for n in names:
    if n.endswith('materials.json'):
        try: d = jload(z.read(n))
        except Exception: continue
        for k, m in d.items():
            if not isinstance(m, dict): continue
            cm = None; at = bool(m.get('alphaTest'))
            for s0 in (m.get('Stages') or []):
                if isinstance(s0, dict):
                    cm = s0.get('baseColorMap') or s0.get('colorMap') or s0.get('diffuseMap')
                    if cm: at = at or bool(s0.get('alphaTest')); break
            for key in (k, m.get('name'), m.get('mapTo')):
                if key and cm: matmap[key] = (cm, at)
print('materials', len(matmap))
def find_file(p):
    p = p.lstrip('/')
    cands = [p, p.replace('west_coast_usa/', 'west_coast_usa_bw/'), os.path.splitext(p)[0] + '.dds', os.path.splitext(p.replace('west_coast_usa/', 'west_coast_usa_bw/'))[0] + '.dds', os.path.splitext(p.replace('west_coast_usa/', 'west_coast_usa_bw/'))[0] + '.png']
    for c in cands:
        if c.lower() in low: return low[c.lower()]
    b = os.path.splitext(os.path.basename(p))[0].lower()
    for k, v in low.items():
        if os.path.splitext(os.path.basename(k))[0] == b and k.endswith(('.dds', '.png', '.jpg')): return v
    return None
textures = {}  # path -> id
def tex_id(matname):
    if matname not in matmap and matname.endswith('-material'): matname = matname[:-9]
    if matname not in matmap: return 0xFFFFFFFF
    cm, _ = matmap[matname]
    f = find_file(cm)
    if not f: return 0xFFFFFFFF
    if f in textures: return textures[f]
    try:
        im = Image.open(io.BytesIO(z.read(f))); im.load(); im = im.convert('RGBA')
    except Exception as e:
        print('tex fail', f, e); textures[f] = 0xFFFFFFFF; return 0xFFFFFFFF
    if max(im.size) > 1024: im = im.resize((min(im.size[0], 1024), min(im.size[1], 1024)), Image.LANCZOS)
    i = len(textures); textures[f] = i
    im.save('%s/tex/%d.png' % (out, i), optimize=True)
    return i
# ---- الأنواع
dae_of = {os.path.splitext(os.path.basename(n))[0].lower(): n for n in names if n.endswith('.dae')}
species = []; sp_index = {}; sp_cols = {}
def build_species(key):
    g, inst, up = parse_dae(z.read(dae_of[key]).decode('utf8', 'replace'))
    subs = {}
    import re as _re
    keep = []
    colpts = []
    for it in inst:
        nm = it[3].lower()
        if nm.startswith('col') or 'colmesh' in nm:
            for (mat, P, N, T) in g.get(it[0], []):
                P4 = np.concatenate([P, np.ones((len(P), 1), np.float32)], 1) @ it[1].T.astype(np.float32)
                Pn = P4[:, :3]
                if up == 'Z_UP': Pn = np.stack([Pn[:, 0], Pn[:, 2], -Pn[:, 1]], 1)
                colpts.append(Pn)
            continue
        keep.append(it)
    build_species.last_col = None
    if colpts:
        A = np.concatenate(colpts); mn, mx = A.min(0), A.max(0)
        build_species.last_col = ((mn + mx) / 2, (mx - mn) / 2)
    sizes = [int(m.group(1)) for it in keep for m in [_re.search(r'_a(\d+)$', it[3].lower())] if m]
    if sizes:
        top = max(sizes)
        keep = [it for it in keep if (not _re.search(r'_a(\d+)$', it[3].lower())) or int(_re.search(r'_a(\d+)$', it[3].lower()).group(1)) == top]
    for url, M, binds, _nm in keep:
        for (mat, P, N, T) in g.get(url, []):
            t = tex_id(binds.get(mat, mat) if binds else mat)
            P4 = np.concatenate([P, np.ones((len(P), 1), np.float32)], 1) @ M.T.astype(np.float32)
            Pn = P4[:, :3]; R = M[:3, :3].astype(np.float32); Nn = N @ R.T
            Nn /= np.maximum(np.linalg.norm(Nn, axis=1, keepdims=True), 1e-6)
            if up == 'Z_UP':  # (x,y,z)->(x,z,-y)
                Pn = np.stack([Pn[:, 0], Pn[:, 2], -Pn[:, 1]], 1); Nn = np.stack([Nn[:, 0], Nn[:, 2], -Nn[:, 1]], 1)
            uv = T.copy(); uv[:, 1] = 1.0 - uv[:, 1]
            subs.setdefault(t, []).append((Pn, Nn, uv))
    outsubs = []
    for t, lst in subs.items():
        P = np.concatenate([a[0] for a in lst]); N = np.concatenate([a[1] for a in lst]); U = np.concatenate([a[2] for a in lst])
        V = np.concatenate([P, N, U], 1).astype(np.float32)
        uniq, inv = np.unique(np.round(V, 5), axis=0, return_inverse=True)
        if len(uniq) > 65000: print('too many verts', key, len(uniq)); continue
        outsubs.append((t, uniq.astype(np.float32), inv.reshape(-1).astype(np.uint16)))
    return outsubs
def get_species(key):
    if key in sp_index: return sp_index[key]
    if key not in dae_of: return None
    try: subs = build_species(key)
    except Exception as e:
        print('species fail', key, e); sp_index[key] = None; return None
    sp_index[key] = len(species); species.append((key, subs)); sp_cols[sp_index[key]] = build_species.last_col; return sp_index[key]
# ---- المواقع
CELL = 128.0
placements = []
def add(sp, bx, by, bz, theta, sc, sy=None, sz=None):
    placements.append((sp, bx, bz, -by, theta, sc, sc if sy is None else sy, sc if sz is None else sz))
for n in names:
    if not n.endswith('.forest4.json'): continue
    key = os.path.splitext(os.path.splitext(os.path.basename(n))[0])[0].lower()
    if key not in dae_of: continue
    sp = get_species(key)
    if sp is None: continue
    for line in z.read(n).decode().splitlines():
        try: o = json.loads(line)
        except Exception: continue
        p = o['pos']; r = o.get('rotationMatrix', [1, 0, 0, 0, 1, 0, 0, 0, 1])
        sc = o.get('scale', 1.0); sc = float(sc if not isinstance(sc, list) else sc[0])
        add(sp, p[0], p[1], p[2], float(np.arctan2(r[3], r[0])), sc)
nforest = len(placements)
for n in names:
    if n.endswith('items.level.json') and 'main/' in n:
        for line in z.read(n).decode('utf8', 'replace').splitlines():
            try: o = json.loads(line)
            except Exception: continue
            if o.get('class') != 'TSStatic' or not o.get('shapeName'): continue
            key = os.path.splitext(os.path.basename(o['shapeName']))[0].lower()
            if key not in dae_of: continue
            sp = get_species(key)
            if sp is None: continue
            p = o.get('position')
            if not p: continue
            r = o.get('rotationMatrix', [1, 0, 0, 0, 1, 0, 0, 0, 1]); s = o.get('scale', [1, 1, 1])
            if isinstance(s, list) and len(s) >= 3: sx, sy, sz = float(s[0]), float(s[2]), float(s[1])
            else: sx = sy = sz = float(s if not isinstance(s, list) else s[0])
            add(sp, p[0], p[1], p[2], float(np.arctan2(r[3], r[0])), sx, sy, sz)
# ---- مباني تقريبية (نماذجها غير موجودة في نسخة bw): صناديق بأبعاد تقريبية
PROXY = [('container_01', 12.2, 2.6, 2.44, (110, 80, 60)), ('containers_40_white', 12.2, 2.6, 2.44, (225, 225, 225)), ('containers_40_brown', 12.2, 2.6, 2.44, (110, 70, 50)), ('container', 12.2, 2.6, 2.44, (90, 110, 140)), ('wood_stack', 3, 1.6, 2, (150, 110, 70)), ('wall_brick', 8, 3, 0.4, (150, 95, 80)), ('wall_wood', 8, 3, 0.3, (140, 105, 75)), ('buildingblock_concrete', 14, 9, 14, (170, 168, 160)),
 ('office_mobile', 9, 3, 3, (215, 215, 210)), ('warehouse_large', 50, 12, 30, (185, 185, 190)), ('warehouse', 30, 9, 18, (185, 185, 190)),
 ('shelter_industrial', 16, 6, 8, (170, 175, 180)), ('guardhouse', 4, 3, 4, (205, 200, 190)), ('shack', 5, 3, 4, (190, 185, 175)),
 ('house', 10, 6, 8, (215, 205, 185)), ('cabin', 7, 4, 6, (150, 110, 80)), ('workshop', 14, 6, 10, (175, 175, 180)),
 ('garage', 20, 7, 14, (180, 180, 185)), ('dealership', 25, 6, 18, (200, 200, 205)), ('gasstation', 18, 6, 10, (210, 210, 205)),
 ('station_downtown', 30, 10, 20, (190, 185, 175)), ('chinatown_corner', 20, 14, 20, (165, 125, 100)), ('pumpjack', 4, 6, 12, (200, 170, 60)),
 ('transformer_ground', 3, 2.5, 3, (120, 125, 120)), ('leanto', 5, 3, 4, (160, 130, 100))]
SKIP = ('fenc', 'sidewalk', 'sign', 'flag', 'light', 'arrow', 'timer', 'display', 'christmas', 'excavator', 'boom_lift', 'cylinder')
def proxy_for(key):
    if any(k in key for k in SKIP): return None
    for k, w, h, d, col in PROXY:
        if k in key: return (w, h, d, col)
    return None
cube_sp = {}
def cube_species(col):
    if col in cube_sp: return cube_sp[col]
    tid = len(textures); textures[('solid', col)] = tid
    Image.new('RGBA', (4, 4), col + (255,)).save('%s/tex/%d.png' % (out, tid))
    V = []; I = []
    faces = [((0, 0, 1), [(-.5, 0, .5), (.5, 0, .5), (.5, 1, .5), (-.5, 1, .5)]), ((0, 0, -1), [(.5, 0, -.5), (-.5, 0, -.5), (-.5, 1, -.5), (.5, 1, -.5)]),
             ((1, 0, 0), [(.5, 0, .5), (.5, 0, -.5), (.5, 1, -.5), (.5, 1, .5)]), ((-1, 0, 0), [(-.5, 0, -.5), (-.5, 0, .5), (-.5, 1, .5), (-.5, 1, -.5)]),
             ((0, 1, 0), [(-.5, 1, .5), (.5, 1, .5), (.5, 1, -.5), (-.5, 1, -.5)])]
    for nrm, quad in faces:
        b = len(V)
        for p in quad: V.append(list(p) + list(nrm) + [0.5, 0.5])
        I += [b, b + 1, b + 2, b, b + 2, b + 3]
    species.append(('proxy_%d_%d_%d' % col, [(tid, np.array(V, np.float32), np.array(I, np.uint16))]))
    sp_cols[len(species) - 1] = None
    cube_sp[col] = len(species) - 1
    return cube_sp[col]
proxy_box_colliders = []
nprox = 0
def add_proxy(key, bx, by, bz, theta):
    global nprox
    pf = proxy_for(key)
    if not pf: return
    w, h, d, col = pf
    add(cube_species(col), bx, by, bz, theta, w, h, d); nprox += 1
    ex, ey, ez = bx, bz, -by
    proxy_box_colliders.append((ex, ey + h / 2, ez, w / 2, h / 2, d / 2, theta))
for n in names:
    if n.endswith('.forest4.json'):
        key = os.path.splitext(os.path.splitext(os.path.basename(n))[0])[0].lower()
        if key in dae_of or proxy_for(key) is None: continue
        for line in z.read(n).decode().splitlines():
            try: o = json.loads(line)
            except Exception: continue
            r = o.get('rotationMatrix', [1, 0, 0, 0, 1, 0, 0, 0, 1]); p = o['pos']
            add_proxy(key, p[0], p[1], p[2], float(np.arctan2(r[3], r[0])))
    elif n.endswith('items.level.json') and 'main/' in n:
        for line in z.read(n).decode('utf8', 'replace').splitlines():
            try: o = json.loads(line)
            except Exception: continue
            if o.get('class') != 'TSStatic' or not o.get('shapeName'): continue
            key = os.path.splitext(os.path.basename(o['shapeName']))[0].lower()
            if key in dae_of: continue
            p = o.get('position')
            if not p: continue
            r = o.get('rotationMatrix', [1, 0, 0, 0, 1, 0, 0, 0, 1])
            add_proxy(key, p[0], p[1], p[2], float(np.arctan2(r[3], r[0])))
print('proxy buildings', nprox)
# ---- Prefabs (مثل أرصفة المحطة): ملفات .prefab نصية بصيغة Torque
def parse_prefab(text):
    items = []
    for m in re.finditer(r'new TSStatic\(\)\s*\{(.*?)\n\s*\};', text, re.S):
        items.append(dict(re.findall(r'(\w+)\s*=\s*"([^"]*)";', m.group(1))))
    return items
prefab_jobs = []
for n in names:
    if n.endswith('items.level.json'):
        for line in z.read(n).decode('utf8', 'replace').splitlines():
            if '"Prefab"' not in line: continue
            try: o = json.loads(line)
            except Exception: continue
            fn = o.get('fileName') or o.get('filename') or ''
            R = np.array(o.get('rotationMatrix', [1, 0, 0, 0, 1, 0, 0, 0, 1]), dtype=np.float64).reshape(3, 3)
            P = np.array(o.get('position', [0, 0, 0]), dtype=np.float64)
            prefab_jobs.append((fn, R, P))
nprefab = 0
for fn, R, P in prefab_jobs:
    key_fn = fn.lstrip('/').lower()
    if key_fn.endswith('.prefab') and key_fn in low:
        for kv in parse_prefab(z.read(low[key_fn]).decode('utf8', 'replace')):
            sh = os.path.splitext(os.path.basename(kv.get('shapeName', '')))[0].lower()
            lp = np.array([float(x) for x in kv.get('position', '0 0 0').split()])
            Rl = np.array([float(x) for x in kv.get('rotationMatrix', '1 0 0 0 1 0 0 0 1').split()]).reshape(3, 3)
            sc = [float(x) for x in kv.get('scale', '1 1 1').split()]
            wp = R @ lp + P; Rw = R @ Rl; yaw = float(np.arctan2(Rw[1, 0], Rw[0, 0]))
            if sh in dae_of:
                sp = get_species(sh)
                if sp is not None: add(sp, wp[0], wp[1], wp[2], yaw, sc[0], sc[2], sc[1]); nprefab += 1
            else:
                add_proxy(sh, wp[0], wp[1], wp[2], yaw)
    else:
        k = os.path.splitext(os.path.splitext(os.path.basename(fn))[0])[0].lower()
        if k and proxy_for(k): add_proxy(k, P[0], P[1], P[2], float(np.arctan2(R[1, 0], R[0, 0])))
print('prefab objects', nprefab)
# ---- صناديق التصادم من Colmesh
colliders = list(proxy_box_colliders)
for sp, x, y, z_, th, sx, sy, sz in placements:
    cb = sp_cols.get(sp)
    if cb is None: continue
    c, hh = cb
    if hh[0] < 0.03 and hh[2] < 0.03: continue
    cth, sth = np.cos(th), np.sin(th)
    lx, lz = c[0] * sx, c[2] * sz
    wx = x + lx * cth + lz * sth; wz = z_ - lx * sth + lz * cth
    colliders.append((float(wx), float(y + c[1] * sy), float(wz), float(hh[0] * sx), float(hh[1] * sy), float(hh[2] * sz), float(th)))
with open(out + '/colliders.bin', 'wb') as f:
    f.write(b'COL1'); f.write(struct.pack('<I', len(colliders)))
    for c in colliders: f.write(struct.pack('<fffffff', *c))
print('colliders', len(colliders))
print('species', len(species), 'placements', len(placements), '(forest', nforest, ') textures', len(textures))
# ---- الكتابة
with open(out + '/props.bin', 'wb') as f:
    f.write(b'PRP1'); f.write(struct.pack('<I', len(species)))
    for key, subs in species:
        nb = key.encode(); f.write(struct.pack('<I', len(nb))); f.write(nb); f.write(struct.pack('<I', len(subs)))
        for t, V, I in subs:
            f.write(struct.pack('<III', t, len(V), len(I))); f.write(V.tobytes()); f.write(I.astype('<u2').tobytes())
with open(out + '/placements.bin', 'wb') as f:
    f.write(b'PLC2'); f.write(struct.pack('<I', len(placements)))
    for pl in placements: f.write(struct.pack('<Hfffffff', *pl))
with open(out + '/species.txt', 'w') as f:
    for key, subs in species: f.write('%s %d\n' % (key, sum(len(I) // 3 for _, _, I in subs)))
print('done')
