import sys, json, math, os
sys.path.insert(0, '/home/claude/tools')
import numpy as np, trimesh, pymeshlab
from scipy.spatial import cKDTree
from PIL import Image
from prep_lib import shrink, build_glb
OUT = '/home/claude/crashdrive/assets/env'
ENV = '/home/claude/env/conv'
os.makedirs(OUT, exist_ok=True)

def mat_info(g):
    m = getattr(g.visual, 'material', None)
    img = getattr(m, 'baseColorTexture', None) if m is not None else None
    col = np.array([1, 1, 1, 1.0])
    if m is not None and getattr(m, 'baseColorFactor', None) is not None:
        col = np.array(m.baseColorFactor, float)
        if col.max() > 1.5: col = col / 255.0
    elif getattr(g.visual, 'kind', '') == 'vertex':
        vc = np.array(g.visual.vertex_colors, float) / 255.0
        col = vc.mean(0)
    uv = None
    if getattr(g.visual, 'uv', None) is not None and img is not None: uv = np.array(g.visual.uv, float)
    return img, col, uv

def weld_decimate(V, F, UV, target):
    """geometry-only decimation (welded) then UV re-projection from the nearest original face."""
    V = np.asarray(V, float); F = np.asarray(F, np.int32)
    if len(F) <= target:
        Vc = V[F.reshape(-1)]; UVc = UV[F.reshape(-1)] if UV is not None else None
        return Vc, UVc, V, F
    ms = pymeshlab.MeshSet(); ms.add_mesh(pymeshlab.Mesh(vertex_matrix=V, face_matrix=F))
    ms.meshing_merge_close_vertices(threshold=pymeshlab.PureValue(1e-4 * max(1.0, np.ptp(V, axis=0).max())))
    ms.meshing_decimation_quadric_edge_collapse(targetfacenum=int(target), preservenormal=True, preserveboundary=True, planarquadric=True)
    m = ms.current_mesh(); V2 = m.vertex_matrix(); F2 = m.face_matrix()
    Vc = V2[F2.reshape(-1)]
    if UV is None: return Vc, None, V2, F2
    cent = V[F].mean(1); tree = cKDTree(cent)
    fc = V2[F2].mean(1); _, tid = tree.query(fc)
    tri = V[F[tid]]                                  # (n,3,3)
    P = V2[F2]                                       # (n,3,3)
    a, b, c = tri[:, 0], tri[:, 1], tri[:, 2]
    v0, v1 = b - a, c - a
    d00 = (v0 * v0).sum(1); d01 = (v0 * v1).sum(1); d11 = (v1 * v1).sum(1)
    den = d00 * d11 - d01 * d01; den[den == 0] = 1
    UVc = np.zeros((len(F2), 3, 2))
    for k in range(3):
        v2 = P[:, k] - a
        d20 = (v2 * v0).sum(1); d21 = (v2 * v1).sum(1)
        bv = (d11 * d20 - d01 * d21) / den; bw = (d00 * d21 - d01 * d20) / den; bu = 1 - bv - bw
        w = np.clip(np.stack([bu, bv, bw], 1), 0, 1); w = w / np.maximum(w.sum(1, keepdims=True), 1e-9)
        u = UV[F[tid]]                                 # (n,3,2)
        UVc[:, k] = (w[:, :, None] * u).sum(1)
    return Vc, UVc.reshape(-1, 2), V2, F2

def normals_corner(V2, F2):
    n = np.zeros_like(V2)
    fn = np.cross(V2[F2[:, 1]] - V2[F2[:, 0]], V2[F2[:, 2]] - V2[F2[:, 0]])
    for k in range(3): np.add.at(n, F2[:, k], fn)
    l = np.linalg.norm(n, axis=1, keepdims=True); l[l == 0] = 1
    return (n / l)[F2.reshape(-1)]

def gather(sc, accept):
    items = []
    for nm in sc.graph.nodes_geometry:
        T, gn = sc.graph[nm]
        if not accept(gn): continue
        g = sc.geometry[gn]
        V = trimesh.transform_points(np.array(g.vertices), T)
        img, col, uv = mat_info(g)
        items.append(dict(name=gn, V=V, F=np.array(g.faces), UV=uv, img=img, col=col))
    return items

def export_group(items, out, budget, xform, glass_prefix='Glass', min_faces=60, maxtex=1024):
    tot = sum(len(i['F']) for i in items)
    parts = []
    cache = {}
    for it in items:
        V = xform(it['V'])
        tgt = max(min_faces, int(len(it['F']) * budget / tot))
        Vc, UVc, V2, F2 = weld_decimate(V, it['F'], it['UV'], tgt)
        N = normals_corner(V2, F2)
        img = None
        if it['img'] is not None:
            k = id(it['img'])
            if k not in cache: cache[k] = shrink(it['img'], maxtex)
            img = cache[k]
        col = it['col'].copy()
        if it['name'].startswith(glass_prefix): col = np.array([0.30, 0.42, 0.52, 0.40]); img = None; UVc = None
        parts.append(dict(V=Vc, N=N, UV=UVc, F=np.arange(len(Vc)).reshape(-1, 3), img=img, color=col))
    build_glb(parts, out)
    print(out.split('/')[-1], 'tris', sum(len(p['F']) for p in parts), 'size KB', os.path.getsize(out) // 1024)

# ---------------- apartment (building + glass only, Y-up metres)
sc = trimesh.load(f'{ENV}/apartment.glb', force='scene')
items = gather(sc, lambda gn: gn.startswith('Building_') or gn.startswith('Glass_'))
allV = np.vstack([i['V'] for i in items]); mn, mx = allV.min(0), allV.max(0)
cx, cz = (mn[0] + mx[0]) / 2, (mn[2] + mx[2]) / 2
export_group(items, f'{OUT}/apartment.glb', 26000, lambda V: V - np.array([cx, mn[1] + 0.0, cz]))
print('apartment size m', np.round(mx - mn, 1))

# ---------------- school (z-up, cm -> Y-up metres)
sc = trimesh.load(f'{ENV}/school.glb', force='scene')
items = gather(sc, lambda gn: gn not in ('Plane', 'Plane.001', 'Mesh.004'))
def zup(V):
    W = V * 0.01
    return np.stack([W[:, 0], W[:, 2], -W[:, 1]], 1)
allV = zup(np.vstack([i['V'] for i in items])); mn, mx = allV.min(0), allV.max(0)
cx, cz = (mn[0] + mx[0]) / 2, (mn[2] + mx[2]) / 2
export_group(items, f'{OUT}/school.glb', 30000, lambda V: zup(V) - np.array([cx, mn[1], cz]), glass_prefix='@@')
print('school size m', np.round(mx - mn, 1))
