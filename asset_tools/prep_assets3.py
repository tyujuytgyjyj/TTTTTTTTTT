import sys, json, math, os
sys.path.insert(0, '/home/claude/tools')
import numpy as np, trimesh
from PIL import Image, ImageDraw, ImageFilter
Image.MAX_IMAGE_PIXELS = None
OUT = '/home/claude/crashdrive/assets/env'
UP = '/home/claude/uploads_glb'
exec(open('/home/claude/tools/prep_assets.py').read().split("# ---------------- apartment")[0])

# --- landscape: ground, mountains, trees
sc = trimesh.load(f'{UP}/landscape.glb', force='scene')
rows = []
for nm in sc.graph.nodes_geometry:
    T, gn = sc.graph[nm]; g = sc.geometry[gn]
    V = trimesh.transform_points(np.array(g.vertices), T)
    m = getattr(g.visual, 'material', None)
    img = getattr(m, 'baseColorTexture', None)
    uv = np.array(g.visual.uv) if getattr(g.visual, 'uv', None) is not None else None
    rows.append(dict(name=gn, V=V, F=np.array(g.faces), UV=uv, img=img, mname=getattr(m, 'name', ''), faces=len(g.faces)))
ground = [r for r in rows if r['name'].startswith('Plane_Material')][0]
mount = [r for r in rows if r['name'].startswith('Plane.00') and r['faces'] > 100]
trees = [r for r in rows if r['faces'] <= 20]
print('ground tris', ground['faces'], 'mountains', [r['name'][:12] for r in mount], 'trees', len(trees))
hs = [np.ptp(r['V'][:, 1]) for r in trees]
print('tree world height median', np.median(hs), 'ground bbox', np.round(ground['V'].min(0)), np.round(ground['V'].max(0)))
S = 22.0 / np.median(hs)
Vg = ground['V']
best = None; rad = 130.0 / S
for cx in np.arange(Vg[:, 0].min() + rad, Vg[:, 0].max() - rad, 80):
    for cz in np.arange(Vg[:, 2].min() + rad, Vg[:, 2].max() - rad, 80):
        m = ((Vg[:, 0] - cx) ** 2 + (Vg[:, 2] - cz) ** 2) < rad ** 2
        if m.sum() < 6: continue
        v = Vg[m, 1].std()
        if best is None or v < best[0]: best = (v, cx, cz, np.median(Vg[m, 1]))
print('scale', round(S, 4), 'flat window std(units)', round(best[0], 1), 'center', np.round(best[1:3]), 'base y', round(best[3], 1))
_, ACX, ACZ, ABY = best
def loc(P): return np.stack([(P[:, 0] - ACX) * S, (P[:, 1] - ABY) * S, (P[:, 2] - ACZ) * S], 1)

def subdiv(V, F, UV):
    V = list(map(tuple, V)); UVl = list(map(tuple, UV)); cache = {}; NF = []
    def mid(a, b):
        k = (min(a, b), max(a, b))
        if k not in cache:
            V.append(tuple((np.array(V[a]) + np.array(V[b])) / 2)); UVl.append(tuple((np.array(UVl[a]) + np.array(UVl[b])) / 2)); cache[k] = len(V) - 1
        return cache[k]
    for a, b, c in F:
        ab, bc, ca = mid(a, b), mid(b, c), mid(c, a)
        NF += [(a, ab, ca), (ab, b, bc), (ca, bc, c), (ab, bc, ca)]
    return np.array(V), np.array(NF), np.array(UVl)

V, F, UV = loc(Vg), ground['F'], ground['UV']
for _ in range(3): V, F, UV = subdiv(V, F, UV)
r = np.hypot(V[:, 0], V[:, 2])
t = np.clip((r - 150) / 50, 0, 1); t = t * t * (3 - 2 * t)
V[:, 1] = -0.35 * (1 - t) + V[:, 1] * t
N = np.zeros_like(V); fn = np.cross(V[F[:, 1]] - V[F[:, 0]], V[F[:, 2]] - V[F[:, 0]])
for k in range(3): np.add.at(N, F[:, k], fn)
N /= np.maximum(np.linalg.norm(N, axis=1, keepdims=True), 1e-9)
parts = [dict(V=V[F.reshape(-1)], N=N[F.reshape(-1)], UV=UV[F.reshape(-1)], F=np.arange(len(F) * 3).reshape(-1, 3), img=shrink(ground['img'], 2048), color=np.ones(4))]
build_glb(parts, f'{OUT}/forest_ground.glb'); print('ground glb tris', len(F), 'KB', os.path.getsize(f'{OUT}/forest_ground.glb') // 1024)

# mountains (backdrop cards)
parts = []
for r_ in mount:
    Vm = loc(r_['V']); Fm = r_['F']
    nn = np.cross(Vm[Fm[:, 1]] - Vm[Fm[:, 0]], Vm[Fm[:, 2]] - Vm[Fm[:, 0]]); nn /= np.maximum(np.linalg.norm(nn, axis=1, keepdims=True), 1e-9)
    parts.append(dict(V=Vm[Fm.reshape(-1)], N=np.repeat(nn, 3, axis=0), UV=r_['UV'][Fm.reshape(-1)] if r_['UV'] is not None else None, F=np.arange(len(Fm) * 3).reshape(-1, 3), img=shrink(r_['img'], 1024) if r_['img'] is not None else None, color=np.ones(4)))
build_glb(parts, f'{OUT}/mountains.glb'); print('mountains parts', len(parts), 'KB', os.path.getsize(f'{OUT}/mountains.glb') // 1024)

# trees grouped by texture/material, cleared around the arena
groups = {}
for r_ in trees:
    c = loc(r_['V']).mean(0)
    if np.hypot(c[0], c[2]) < 195: continue
    groups.setdefault(r_['mname'], []).append(r_)
parts = []
for mn_, lst in groups.items():
    Vs = []; Ns = []; Us = []
    for r_ in lst:
        Vm = loc(r_['V']); Fm = r_['F']
        Vs.append(Vm[Fm.reshape(-1)]); Us.append(r_['UV'][Fm.reshape(-1)])
        Ns.append(np.tile(np.array([[0, 1, 0]]), (len(Fm) * 3, 1)))
    Vs = np.vstack(Vs); parts.append(dict(V=Vs, N=np.vstack(Ns), UV=np.vstack(Us), F=np.arange(len(Vs)).reshape(-1, 3), img=shrink(lst[0]['img'], 1024), color=np.ones(4)))
    print('  tree type', mn_, 'count', len(lst))
build_glb(parts, f'{OUT}/trees.glb'); print('trees tris', sum(len(p['F']) for p in parts), 'KB', os.path.getsize(f'{OUT}/trees.glb') // 1024)
json.dump({'scale': float(S), 'ground_extent': [float(np.ptp(Vg[:, 0]) * S), float(np.ptp(Vg[:, 2]) * S)]}, open(f'{OUT}/landscape.json', 'w'))
