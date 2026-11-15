import sys, os, json
sys.path.insert(0, '/home/claude/tools')
import numpy as np, trimesh
exec(open('/home/claude/tools/prep_assets.py').read().split("# ---------------- apartment")[0])
from prep_lib import decimate, smooth_normals
UP = '/home/claude/uploads_glb'
# ---- apartment hi-res (texture-aware decimation, keeps UVs intact)
sc = trimesh.load('/home/claude/env/conv/apartment.glb', force='scene')
items = gather(sc, lambda gn: gn.startswith('Building_') or gn.startswith('Glass_'))
allV = np.vstack([i['V'] for i in items]); mn, mx = allV.min(0), allV.max(0)
cx, cz = (mn[0] + mx[0]) / 2, (mn[2] + mx[2]) / 2
parts = []; cache = {}
for it in items:
    V = it['V'] - np.array([cx, mn[1], cz])
    tgt = max(80, int(len(it['F']) * 0.5))
    if it['UV'] is not None and len(it['F']) > 200:
        Vc, UVc, Fc, V2, F2 = decimate(V, it['F'], it['UV'], tgt, pb=False)
        N = smooth_normals(V2, F2, len(Fc))
    else:
        Vc, UVc, V2, F2 = weld_decimate(V, it['F'], it['UV'], tgt); N = normals_corner(V2, F2)
    img = None
    if it['img'] is not None:
        k = id(it['img']); cache.setdefault(k, shrink(it['img'], 1024)); img = cache[k]
    col = it['col'].copy(); 
    if it['name'].startswith('Glass'): col = np.array([0.30, 0.42, 0.52, 0.40]); img = None; UVc = None
    parts.append(dict(V=Vc, N=N, UV=UVc, F=np.arange(len(Vc)).reshape(-1, 3), img=img, color=col))
build_glb(parts, f'{OUT}/apartment_hi.glb')
print('apartment_hi tris', sum(len(p['F']) for p in parts), 'KB', os.path.getsize(f'{OUT}/apartment_hi.glb') // 1024)
# ---- tree types (one representative per texture, local at base-centre, unit scale 22 m tall)
sc = trimesh.load(f'{UP}/landscape.glb', force='scene')
types = {}
for nm in sc.graph.nodes_geometry:
    T, gn = sc.graph[nm]; g = sc.geometry[gn]
    if len(g.faces) > 20: continue
    m = g.visual.material
    if m.name in types: continue
    V = trimesh.transform_points(np.array(g.vertices), T)
    types[m.name] = (V, np.array(g.faces), np.array(g.visual.uv), m.baseColorTexture)
S = 28.854
parts = []
for name, (V, F, UV, img) in sorted(types.items()):
    c = np.array([V[:, 0].mean(), V[:, 1].min(), V[:, 2].mean()])
    W = (V - c) * S
    nn = np.tile(np.array([[0, 1, 0]]), (len(F) * 3, 1))
    parts.append(dict(V=W[F.reshape(-1)], N=nn, UV=UV[F.reshape(-1)], F=np.arange(len(F) * 3).reshape(-1, 3), img=shrink(img, 1024), color=np.ones(4)))
    print('tree', name, 'height', round(np.ptp(W[:, 1]), 1), 'width', round(np.ptp(W[:, 0]), 1))
build_glb(parts, f'{OUT}/tree_types.glb')
