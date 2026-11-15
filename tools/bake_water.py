# يحوّل WaterPlane و River في مرحلة BeamNG إلى water.bin (شرائح مياه) للعبة APEX.drive
# الاستعمال: python bake_water.py level.zip out_dir
import sys, os, json, struct, zipfile, math, re
import numpy as np
zpath, out = sys.argv[1], sys.argv[2]
z = zipfile.ZipFile(zpath)
objs = []
for n in z.namelist():
    if n.endswith('.json') and ('items.level.json' in n):
        for line in z.read(n).decode('utf8', 'replace').splitlines():
            if '"WaterPlane"' in line or '"River"' in line:
                try: objs.append(json.loads(line))
                except Exception: pass
def color_of(o, default):
    c = o.get('baseColor')
    if isinstance(c, list) and len(c) >= 3:
        s = 255.0 if max(c[:3]) > 1.5 else 1.0
        return tuple(int(min(255, max(0, v / s * 255))) for v in c[:3])
    return default
meshes = []
EXT = 3200.0
for o in objs:
    if o['class'] == 'WaterPlane':
        lvl = float(o['position'][2]); col = color_of(o, (14, 52, 66))
        if sum(col) < 30: col = (14, 52, 66)
        V = [(-EXT, lvl, -EXT), (EXT, lvl, -EXT), (EXT, lvl, EXT), (-EXT, lvl, EXT)]
        meshes.append((V, [0, 2, 1, 0, 3, 2], col + (255,), 'plane', lvl))
    elif o['class'] == 'River':
        nd = np.array(o['nodes'], dtype=np.float64)
        if len(nd) < 2: continue
        col = color_of(o, (20, 70, 85))
        if sum(col) < 30: col = (20, 70, 85)
        pts = []
        for i in range(len(nd) - 1):
            p0 = nd[max(i - 1, 0)]; p1 = nd[i]; p2 = nd[i + 1]; p3 = nd[min(i + 2, len(nd) - 1)]
            L = np.linalg.norm(p2[:2] - p1[:2]); k = max(1, int(math.ceil(L / 4.0)))
            for j in range(k):
                t = j / k
                pts.append(0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t + (-p0 + 3 * p1 - 3 * p2 + p3) * t ** 3))
        pts.append(nd[-1])
        S = np.array(pts)
        xy = S[:, :2]; tg = np.zeros_like(xy)
        tg[1:-1] = xy[2:] - xy[:-2]; tg[0] = xy[1] - xy[0]; tg[-1] = xy[-1] - xy[-2]
        tg /= np.maximum(np.linalg.norm(tg, axis=1, keepdims=True), 1e-9)
        nl = np.stack([-tg[:, 1], tg[:, 0]], 1); hw = np.maximum(S[:, 3], 0.5) / 2
        V = []; I = []
        for i in range(len(S)):
            l = xy[i] + nl[i] * hw[i]; r = xy[i] - nl[i] * hw[i]
            V.append((l[0], S[i, 2], -l[1])); V.append((r[0], S[i, 2], -r[1]))
        for i in range(len(S) - 1):
            a, b, c, d = 2 * i, 2 * i + 1, 2 * i + 3, 2 * i + 2
            # الوجه للأعلى: نتحقق من اتجاه المثلث
            def up(i0, i1, i2):
                ax, az = V[i1][0] - V[i0][0], V[i1][2] - V[i0][2]; bx, bz = V[i2][0] - V[i0][0], V[i2][2] - V[i0][2]
                return az * bx - ax * bz
            I += [a, b, c] if up(a, b, c) >= 0 else [a, c, b]
            I += [a, c, d] if up(a, c, d) >= 0 else [a, d, c]
        meshes.append((V, I, col + (255,), 'river', None))
with open(out + '/water.bin', 'wb') as f:
    f.write(b'WAT1'); f.write(struct.pack('<I', len(meshes)))
    for V, I, col, kind, lvl in meshes:
        f.write(struct.pack('<IIf', len(V), len(I), lvl if lvl is not None else -1e9))
        for v in V: f.write(struct.pack('<fff', *v) + bytes(col))
        f.write(np.array(I, dtype='<u2').tobytes())
print('water meshes', [(m[3], len(m[0])) for m in meshes])
