import sys, json, math, os
sys.path.insert(0, '/home/claude/tools')
import numpy as np, trimesh
from PIL import Image
from scipy import ndimage
Image.MAX_IMAGE_PIXELS = None
UP = '/home/claude/uploads_glb'
OUT = '/home/claude/crashdrive/assets/env'
exec(open('/home/claude/tools/prep_assets.py').read().split("# ---------------- apartment")[0])   # reuse helpers (gather, export_group ...)

def bake_topdown(path, out, res=1024):
    sc = trimesh.load(path, force='scene')
    P = []; C = []
    img = None
    for nm in sc.graph.nodes_geometry:
        T, gn = sc.graph[nm]; g = sc.geometry[gn]
        V = trimesh.transform_points(np.array(g.vertices), T); F = np.array(g.faces); UV = np.array(g.visual.uv)
        m = g.visual.material; img = getattr(m, 'baseColorTexture', None) or img
        P.append(V[F].mean(1)); C.append(UV[F].mean(1))
    P = np.vstack(P); C = np.vstack(C)
    tex = np.asarray(img.convert('RGB'))
    H, W = tex.shape[:2]
    px = np.clip((C[:, 0] * (W - 1)).astype(int), 0, W - 1); py = np.clip(((1 - C[:, 1]) * (H - 1)).astype(int), 0, H - 1)
    col = tex[py, px].astype(float)
    x0, x1 = np.percentile(P[:, 0], [0.2, 99.8]); z0, z1 = np.percentile(P[:, 2], [0.2, 99.8])
    sx, sz = x1 - x0, z1 - z0
    ix = np.clip(((P[:, 0] - x0) / sx * (res - 1)).astype(int), 0, res - 1)
    iz = np.clip(((z1 - P[:, 2]) / sz * (res - 1)).astype(int), 0, res - 1)
    acc = np.zeros((res, res, 3)); cnt = np.zeros((res, res))
    np.add.at(acc, (iz, ix), col); np.add.at(cnt, (iz, ix), 1)
    valid = cnt > 0
    im = np.where(valid[..., None], acc / np.maximum(cnt, 1)[..., None], 0)
    if (~valid).any():
        idx = ndimage.distance_transform_edt(~valid, return_distances=False, return_indices=True)
        im = im[idx[0], idx[1]]
    im = ndimage.gaussian_filter(im, sigma=(0.7, 0.7, 0))
    Image.fromarray(np.clip(im, 0, 255).astype(np.uint8)).save(out, quality=90)
    print(out.split('/')[-1], 'footprint m', round(sx, 2), 'x', round(sz, 2), 'holes filled', int((~valid).sum()))
    return sx, sz

fa = bake_topdown(f'{UP}/afloor.glb', f'{OUT}/floor_a.jpg')
fb = bake_topdown(f'{UP}/floor3.glb', f'{OUT}/floor_b.jpg')
mh = bake_topdown(f'{UP}/manhole.glb', f'{OUT}/manhole.jpg')
json.dump({'floor_a': fa, 'floor_b': fb, 'manhole': mh}, open(f'{OUT}/floor_sizes.json', 'w'))

# ---------------- school from the textured GLB
sc = trimesh.load(f'{UP}/school.glb', force='scene')
items = gather(sc, lambda gn: not gn.startswith('Plane') and gn != 'Mesh.004')
def zup(V):
    return np.stack([V[:, 0], V[:, 2], -V[:, 1]], 1)
allV = zup(np.vstack([i['V'] for i in items])); mn, mx = allV.min(0), allV.max(0)
print('school raw size', np.round(mx - mn, 1))
cx, cz = (mn[0] + mx[0]) / 2, (mn[2] + mx[2]) / 2
export_group(items, f'{OUT}/school.glb', 32000, lambda V: zup(V) - np.array([cx, mn[1], cz]), glass_prefix='@@')
