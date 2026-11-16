# يحوّل طرق الخريطة (DecalRoad: أسفلت، خطوط، حواف، مجاري...) إلى شرائح مجسمة فوق التضاريس.
# الاستعمال: python bake_roads.py west_coast_usa_bw.zip out_dir
import sys, os, io, json, struct, zipfile, math
import numpy as np
from PIL import Image
zpath, out = sys.argv[1], sys.argv[2]
LVL = sys.argv[3] if len(sys.argv) > 3 else 'west_coast_usa_bw'
STEP = int(sys.argv[4]) if len(sys.argv) > 4 else 2
HFPATH = sys.argv[5] if len(sys.argv) > 5 else None
import re
def jload(t):
    t = t.decode('utf8', 'replace') if isinstance(t, bytes) else t
    return json.loads(re.sub(r',(\s*[}\]])', r'\1', t))
z = zipfile.ZipFile(zpath); names = z.namelist(); low = {n.lower(): n for n in names}
os.makedirs(out + '/tex', exist_ok=True)
# ---- التضاريس (نفس شبكة اللعبة)
tb = None
for n in names:
    if n.endswith('items.level.json'):
        for line in z.read(n).decode('utf8', 'replace').splitlines():
            if '"TerrainBlock"' in line:
                tb = json.loads(line); break
    if tb: break
if tb is None: tb = {'position': [-1023.5, -1023.5, 0], 'maxHeight': 384.640015, 'squareSize': 1.0}
terfile = [n for n in names if n.lower().endswith('.ter')][0]
d = z.read(terfile); N = struct.unpack('<I', d[1:5])[0]
MAXH = float(tb.get('maxHeight', 100)); SQ = float(tb.get('squareSize', 1.0)); ORIGIN = float(tb['position'][0]); ORIGIN_Y = float(tb['position'][1]); HBASE = float(tb['position'][2])
hm = np.frombuffer(d[5:5+N*N*2], dtype='<u2').reshape(N, N)[::STEP, ::STEP].astype(np.float32) * (MAXH / 65535.0) + HBASE
M = hm.shape[0]; CELLM = SQ * STEP
surf_arr = None; hf_off = 0; hf_n = 0
if HFPATH:
    hb = bytearray(open(HFPATH, 'rb').read())
    hf_n = struct.unpack('<I', hb[4:8])[0]; hf_off = (32 if hb[0:4] == b'HF02' else 24) + hf_n * hf_n * 2
    surf_arr = np.frombuffer(bytes(hb[hf_off:hf_off + hf_n * hf_n]), dtype=np.uint8).reshape(hf_n, hf_n).copy()
cur_surf = 0
def surf_of(mat):
    m = mat.lower()
    if 'asphalt' in m or 'line' in m or 'concrete' in m or 'sidewalk' in m: return 0
    if 'sand' in m or 'beach' in m: return 4
    if any(k in m for k in ('dirt', 'mud', 'gravel', 'track', 'erosion', 'rock')): return 2
    if 'grass' in m: return 3
    return 0
def raster_quad(Q, sid):
    if surf_arr is None: return
    pts = [((p[0] - ORIGIN) / CELLM, (-p[2] - ORIGIN_Y) / CELLM) for p in Q]
    xs = [p[0] for p in pts]; ys = [p[1] for p in pts]
    for iy in range(max(int(min(ys)) - 1, 0), min(int(max(ys)) + 2, hf_n)):
        for ix in range(max(int(min(xs)) - 1, 0), min(int(max(xs)) + 2, hf_n)):
            px, py = ix, iy
            for tri in ((0, 1, 2), (0, 2, 3)):
                a, b, c = pts[tri[0]], pts[tri[1]], pts[tri[2]]
                d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
                if abs(d) < 1e-9: continue
                l1 = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / d
                l2 = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / d
                if l1 >= -0.15 and l2 >= -0.15 and 1 - l1 - l2 >= -0.15:
                    surf_arr[iy, ix] = sid; break
def height(bx, by):
    fx = min(max((bx - ORIGIN) / CELLM, 0), M - 1.001); fy = min(max((by - ORIGIN_Y) / CELLM, 0), M - 1.001)
    ix, iy = int(fx), int(fy); tx, ty = fx - ix, fy - iy
    return ((hm[iy, ix] * (1-tx) + hm[iy, ix+1] * tx) * (1-ty) + (hm[iy+1, ix] * (1-tx) + hm[iy+1, ix+1] * tx) * ty)
# ---- المواد
matmap = {}
for n in names:
    if n.endswith('materials.json'):
        try: dd = jload(z.read(n))
        except Exception: continue
        for k, m in dd.items():
            if not isinstance(m, dict): continue
            cm = None
            for s0 in (m.get('Stages') or []):
                if isinstance(s0, dict):
                    cm = s0.get('baseColorMap') or s0.get('colorMap') or s0.get('diffuseMap')
                    if cm: break
            if not cm: cm = m.get('diffuseMap')
            if cm:
                for key in (k, m.get('name'), m.get('mapTo')):
                    if key: matmap[key] = cm
def find_file(p):
    p = p.lstrip('/'); q = p.replace('west_coast_usa/', 'west_coast_usa_bw/'); b0 = os.path.splitext(q)[0]
    for c in (p, q, b0 + '.dds', b0 + '.png', os.path.splitext(b0)[0] + '.dds', os.path.splitext(b0)[0] + '.png'):
        if c.lower() in low: return low[c.lower()]
    return None
textures = {}
def tex_id(mat):
    cm = matmap.get(mat)
    if not cm: return None
    f = find_file(cm)
    if not f: return None
    if f in textures: return textures[f]
    try:
        im = Image.open(io.BytesIO(z.read(f))); im.load(); im = im.convert('RGBA')
    except Exception as e:
        print('tex fail', f, e); textures[f] = None; return None
    if max(im.size) > 1024: im = im.resize((min(im.size[0], 1024), min(im.size[1], 1024)), Image.LANCZOS)
    i = len([v for v in textures.values() if v is not None]); textures[f] = i
    im.save('%s/tex/%d.png' % (out, i), optimize=True)
    return i
# ---- الطرق
roads = []
for n in names:
    if n.endswith('items.level.json') and 'main/' in n:
        for line in z.read(n).decode('utf8', 'replace').splitlines():
            if '"DecalRoad"' in line:
                try: roads.append(json.loads(line))
                except Exception: pass
for n in names:
    if n.endswith('items.level.json'):
        for line in z.read(n).decode('utf8', 'replace').splitlines():
            if '"MeshRoad"' in line:
                try: m = json.loads(line)
                except Exception: continue
                m['meshroad'] = True; m['material'] = m.get('topMaterial') or 'concrete'; m['renderPriority'] = 40
                m['nodes'] = [nd[:4] for nd in m['nodes']]; m['improvedSpline'] = True; m['textureLength'] = 6.0
                roads.append(m)
solid_cache = {}
def solid_tex(col):
    if col in solid_cache: return solid_cache[col]
    i = 1000 + len(solid_cache)
    Image.new('RGBA', (4, 4), col + (255,)).save('%s/tex/%d.png' % (out, i)); solid_cache[col] = i; return i
def nodes_of(r):
    nd = r['nodes']; nd = json.loads(nd) if isinstance(nd, str) else nd
    return np.array(nd, dtype=np.float64)
prios = sorted({float(r.get('renderPriority') or 0) for r in roads}); level_of = {p: i for i, p in enumerate(prios)}
def catmull(P, step):
    pts = []
    for i in range(len(P) - 1):
        p0 = P[max(i - 1, 0)]; p1 = P[i]; p2 = P[i + 1]; p3 = P[min(i + 2, len(P) - 1)]
        L = np.linalg.norm(p2[:2] - p1[:2]); n = max(1, int(math.ceil(L / step)))
        for k in range(n):
            t = k / n
            pts.append(0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t + (-p0 + 3 * p1 - 3 * p2 + p3) * t ** 3))
    pts.append(P[-1].copy())
    return np.array(pts)
CELL = 256.0
groups = {}  # (cx,cz,tex) -> [verts, idx]
stats = {'roads': 0, 'quads': 0, 'skipped_bridge': 0}
def emit(key, quad_pts, uvs):
    raster_quad(quad_pts, cur_surf)
    g = groups.setdefault(key, [[], []])
    if len(g[0]) + 4 > 60000:
        groups.setdefault(key + (len(groups),), [[], []]); 
        g = groups[key + (len(groups) - 1,)]
    b = len(g[0])
    A, B, C, D = quad_pts
    for p, uv in zip(quad_pts, uvs): g[0].append((p[0], p[1], p[2], uv[0], uv[1]))
    # اجعل الوجه للأعلى
    nrm_y = (B[2] - A[2]) * (C[0] - A[0]) - (B[0] - A[0]) * (C[2] - A[2])
    if nrm_y >= 0: g[1] += [b, b + 1, b + 2, b, b + 2, b + 3]
    else: g[1] += [b, b + 2, b + 1, b, b + 3, b + 2]
    stats['quads'] += 1
for r in roads:
    mat = r.get('material', '')
    if 'invisible' in mat: continue
    tid = tex_id(mat)
    isMesh = bool(r.get('meshroad'))
    if tid is None and isMesh: tid = solid_tex((150, 150, 148))
    if tid is None: continue
    P = nodes_of(r)
    if len(P) < 2: continue
    improved = bool(r.get('improvedSpline', False))
    S = catmull(P, 3.0) if improved and len(P) > 2 else (catmull(P, 4.0) if len(P) == 2 else P)
    if len(S) < 2: continue
    T = float(r.get('textureLength') or 5.0)
    lvl = level_of[float(r.get('renderPriority') or 0)]
    cur_surf = surf_of(mat)
    xy = S[:, :2]; tang = np.zeros_like(xy)
    tang[1:-1] = xy[2:] - xy[:-2]; tang[0] = xy[1] - xy[0]; tang[-1] = xy[-1] - xy[-2]
    tang /= np.maximum(np.linalg.norm(tang, axis=1, keepdims=True), 1e-9)
    nl = np.stack([-tang[:, 1], tang[:, 0]], 1)
    hw = np.maximum(S[:, 3], 0.05) / 2.0
    left = xy + nl * hw[:, None]; right = xy - nl * hw[:, None]
    s = np.concatenate([[0], np.cumsum(np.linalg.norm(np.diff(xy, axis=0), axis=1))])
    def pt(bx, by):
        h = float(height(bx, by)); return h
    ok = []
    for i in range(len(S)):
        hc = pt(xy[i][0], xy[i][1]); ok.append(True if isMesh else abs(S[i][2] - hc) < 2.0)
    stats['roads'] += 1
    for i in range(len(S) - 1):
        if not (ok[i] and ok[i + 1]): stats['skipped_bridge'] += 1; continue
        # قسّم الشريحة عند حدود تكرار النسيج حتى يبقى v داخل [0,1]
        s0, s1 = s[i], s[i + 1]
        if s1 - s0 < 1e-6: continue
        cuts = [s0]; k = math.floor(s0 / T) + 1
        while k * T < s1 - 1e-6:
            cuts.append(k * T); k += 1
        cuts.append(s1)
        for a, b in zip(cuts[:-1], cuts[1:]):
            ta, tb = (a - s0) / (s1 - s0), (b - s0) / (s1 - s0)
            def at(t, arr): return arr[i] * (1 - t) + arr[i + 1] * t
            quad = []
            for t, edge in ((ta, left), (ta, right), (tb, right), (tb, left)):
                p = at(t, edge)
                y = (float(at(t, S[:, 2])) + 0.03) if isMesh else (float(height(p[0], p[1])) + 0.03 + 0.008 * lvl)
                quad.append((p[0], y, -p[1]))
            va = ((a / T) - math.floor((a + b) / 2 / T)); vb = ((b / T) - math.floor((a + b) / 2 / T))
            uvs = [(0.0, va), (1.0, va), (1.0, vb), (0.0, vb)]
            cx = int(math.floor(quad[0][0] / CELL)); cz = int(math.floor(quad[0][2] / CELL))
            emit((cx, cz, tid), quad, uvs)
if surf_arr is not None:
    hb[hf_off:hf_off + hf_n * hf_n] = surf_arr.tobytes(); open(HFPATH, 'wb').write(bytes(hb)); print('surface grid updated')
print(stats, 'groups', len(groups), 'textures', len([v for v in textures.values() if v is not None]))
parts = []
for key, (V, I) in groups.items():
    if V: parts.append((key[2], V, I))
with open(out + '/roads.bin', 'wb') as f:
    f.write(b'RDS1'); f.write(struct.pack('<I', len(parts)))
    for tid, V, I in parts:
        f.write(struct.pack('<III', tid, len(V), len(I)))
        f.write(np.array(V, dtype='<f4').tobytes()); f.write(np.array(I, dtype='<u2').tobytes())
print('parts', len(parts), 'verts', sum(len(p[1]) for p in parts))