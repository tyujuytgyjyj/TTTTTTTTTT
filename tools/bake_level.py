# يحوّل مرحلة BeamNG (تضاريس v8) إلى hf.bin + terrain_albedo.jpg + spawns.txt للعبة APEX.drive
# الاستعمال: python bake_level.py level.zip level_name out_dir [step=4]
import sys, os, io, json, struct, zipfile, re
import numpy as np
from PIL import Image
zpath, lvl, out = sys.argv[1], sys.argv[2], sys.argv[3]
step = int(sys.argv[4]) if len(sys.argv) > 4 else 4
os.makedirs(out, exist_ok=True)
z = zipfile.ZipFile(zpath); names = z.namelist(); low = {n.lower(): n for n in names}
def jload(b):
    t = b.decode('utf8', 'replace') if isinstance(b, bytes) else b
    t = re.sub(r',(\s*[}\]])', r'\1', t)
    return json.loads(t)
P = 'levels/%s/' % lvl
tj = None
for n in names:
    if n.lower().endswith('.terrain.json') and n.startswith(P): tj = jload(z.read(n)); break
N = tj['size']; mats = tj['materials']
# معاملات كتلة التضاريس
tb = None
for n in names:
    if n.endswith('items.level.json') and 'TerrainBlock' in z.read(n).decode('utf8', 'replace'):
        for line in z.read(n).decode('utf8', 'replace').splitlines():
            if '"TerrainBlock"' in line: tb = json.loads(line); break
    if tb: break
pos = tb['position']; maxh = float(tb.get('maxHeight', 100)); sq = float(tb.get('squareSize', 1.0))
print('N', N, 'pos', pos, 'maxH', maxh, 'sq', sq, 'mats', mats)
terfile = [n for n in names if n.lower() == (P + 'theterrain.ter').lower() or n.lower().endswith('.ter')][0]
d = z.read(terfile)
hm = np.frombuffer(d[5:5 + N * N * 2], dtype='<u2').reshape(N, N)
lm = np.frombuffer(d[5 + N * N * 2:5 + N * N * 3], dtype='u1').reshape(N, N)
hbase = float(pos[2]); hscale = maxh / 65535.0
ox, oy = float(pos[0]), float(pos[1])
# ---- أسطح
surf_name = {'grass': 3, 'grass2': 3, 'mud': 2, 'dirt': 2, 'asphalt': 0, 'concrete': 0, 'rock': 2, 'rockydirt': 2, 'dirt_grass': 3}
lut = np.full(256, 3, dtype=np.uint8)
for i, m in enumerate(mats): lut[i] = surf_name.get(m.lower(), 3)
hm2 = np.ascontiguousarray(hm[::step, ::step]); sf2 = lut[lm[::step, ::step]]; m2 = hm2.shape[0]
cell = sq * step
with open(out + '/hf.bin', 'wb') as f:
    f.write(b'HF02'); f.write(struct.pack('<Iffffff', m2, cell, ox, oy, hscale, hbase, 0.0))
    f.write(hm2.astype('<u2').tobytes()); f.write(sf2.tobytes())
print('hf.bin', m2, 'cell', cell)
# ---- ألوان المواد من نسيج كل مادة
mj = {}
for n in names:
    if n.endswith('materials.json'):
        try: mj.update(jload(z.read(n)))
        except Exception as e: print('mat parse fail', n, e)
def find_file(p):
    p = p.lstrip('/'); b0 = os.path.splitext(p)[0]
    for c in (p, b0 + '.dds', b0 + '.png', b0 + '.jpg'):
        if c.lower() in low: return low[c.lower()]
    return None
tint = {}; tile = {}
for i, m in enumerate(mats):
    cand = [k for k, v in mj.items() if isinstance(v, dict) and v.get('class') == 'TerrainMaterial' and (v.get('internalName') == m or k == m)]
    col = (110, 110, 100); t128 = None; dsize = 10.0
    if cand:
        v = mj[cand[0]]; dsize = float(v.get('diffuseSize') or 10.0)
        cm = v.get('baseColorBaseTex') or v.get('diffuseMap') or v.get('colorMap')
        f = find_file(cm) if cm else None
        if f:
            try:
                im = Image.open(io.BytesIO(z.read(f))); im.load(); im = im.convert('RGB').resize((128, 128), Image.LANCZOS)
                a = np.asarray(im, dtype=np.float32); col = tuple(a.reshape(-1, 3).mean(0)); t128 = a
            except Exception as e: print('tex fail', f, e)
    tint[i] = col; tile[i] = (t128, dsize); print(i, m, [int(c) for c in col], dsize)
OUT = 4096
lm_s = np.asarray(Image.fromarray(lm[::max(1, N // 2048), ::max(1, N // 2048)]).resize((OUT, OUT), Image.NEAREST))
ys, xs = np.mgrid[0:OUT, 0:OUT]
ppm = OUT / (N * sq)
def blur(a, r=3):
    k = np.ones(2*r+1, dtype=np.float32) / (2*r+1)
    a = np.apply_along_axis(lambda v: np.convolve(v, k, mode='same'), 0, a)
    return np.apply_along_axis(lambda v: np.convolve(v, k, mode='same'), 1, a)
img = np.zeros((OUT, OUT, 3), dtype=np.float32); wsum = np.zeros((OUT, OUT), dtype=np.float32)
for i in range(len(mats)):
    mask = (lm_s == i)
    if not mask.any(): continue
    w = blur(mask.astype(np.float32))
    col = np.array(tint[i], dtype=np.float32)
    t128, dsize = tile[i]
    if t128 is not None:
        tx = (((xs / ppm) % dsize) / dsize * 127).astype(np.int32); ty = (((ys / ppm) % dsize) / dsize * 127).astype(np.int32)
        tex = t128[ty, tx]; mean = np.maximum(tex.mean(axis=(0, 1)), 1.0)
        c = col[None, None, :] * np.clip(tex / mean, 0.6, 1.5)
    else: c = col[None, None, :]
    img += c * w[..., None]; wsum += w
img /= np.maximum(wsum, 1e-4)[..., None]
Image.fromarray(np.clip(img, 0, 255).astype(np.uint8)).save(out + '/terrain_albedo.jpg', quality=88)
print('albedo done')
# ---- نقاط البداية
sp = []
for n in names:
    if n.endswith('items.level.json'):
        for line in z.read(n).decode('utf8', 'replace').splitlines():
            if '"SpawnSphere"' in line:
                try: o = json.loads(line)
                except Exception: continue
                r = o.get('rotationMatrix', [1, 0, 0, 0, 1, 0, 0, 0, 1]); p = o['position']
                # اتجاه السير: محور y للجسم (BeamNG) = (r[1], r[4]) ؛ yaw = atan2(dx, -dy)
                fx, fy = r[1], r[4]
                sp.append((o.get('name', ''), p[0], p[2], -p[1], float(np.arctan2(fx, -fy))))
default = tj and None
info = jload(z.read(P + 'info.json')) if (P + 'info.json') in names else {}
dname = info.get('defaultSpawnPointName')
sp.sort(key=lambda s: 0 if s[0] == dname else 1)
with open(out + '/spawns.txt', 'w') as f:
    for s in sp: f.write('%.2f %.2f %.2f %.4f\n' % s[1:])
print('spawns', sp)
