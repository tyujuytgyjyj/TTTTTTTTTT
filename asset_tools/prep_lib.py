import numpy as np, pymeshlab, trimesh, io
from PIL import Image

def decimate(V, F, UV, target, pb=True):
    """Quadric decimation keeping UVs. Returns corner-expanded arrays (V3,UV3,F3) i.e. 3 verts per face."""
    V = np.asarray(V, float); F = np.asarray(F, np.int32)
    if UV is not None:
        UV = np.asarray(UV, float)
    if len(F) > target and len(F) > 8:
        ms = pymeshlab.MeshSet()
        ms.add_mesh(pymeshlab.Mesh(vertex_matrix=V, face_matrix=F, v_tex_coords_matrix=UV) if UV is not None else pymeshlab.Mesh(vertex_matrix=V, face_matrix=F))
        if UV is not None:
            ms.compute_texcoord_transfer_vertex_to_wedge()
            ms.meshing_decimation_quadric_edge_collapse_with_texture(targetfacenum=int(target), preserveboundary=pb)
        else:
            ms.meshing_decimation_quadric_edge_collapse(targetfacenum=int(target), preservenormal=True, preserveboundary=True)
        m = ms.current_mesh()
        V2 = m.vertex_matrix(); F2 = m.face_matrix()
        if UV is not None:
            W = m.wedge_tex_coord_matrix()  # (3F,2)
            UVc = W.reshape(-1, 2)
        else:
            UVc = None
    else:
        V2, F2 = V, F
        UVc = UV[F2.reshape(-1)] if UV is not None else None
    Vc = V2[F2.reshape(-1)]
    Fc = np.arange(len(F2) * 3).reshape(-1, 3)
    return Vc, UVc, Fc, V2, F2

def smooth_normals(V2, F2, Fc_count):
    """Vertex normals welded by position, expanded per corner."""
    n = np.zeros_like(V2)
    fn = np.cross(V2[F2[:, 1]] - V2[F2[:, 0]], V2[F2[:, 2]] - V2[F2[:, 0]])
    for k in range(3): np.add.at(n, F2[:, k], fn)
    l = np.linalg.norm(n, axis=1, keepdims=True); l[l == 0] = 1
    n = n / l
    return n[F2.reshape(-1)]

def shrink(img, maxs=1024):
    img = img.convert('RGBA') if img.mode in ('RGBA', 'LA', 'P') else img.convert('RGB')
    w, h = img.size
    s = min(1.0, maxs / max(w, h))
    if s < 1.0: img = img.resize((max(1, int(w * s)), max(1, int(h * s))), Image.LANCZOS)
    return img

def build_glb(parts, out):
    """parts: list of dict(V,N,UV,F,img(PIL or None),color(rgba 0-1)). Writes a GLB with one primitive per part."""
    scene = trimesh.Scene()
    for i, p in enumerate(parts):
        m = trimesh.Trimesh(vertices=p['V'], faces=p['F'], vertex_normals=p['N'], process=False)
        if p.get('img') is not None and p.get('UV') is not None:
            mat = trimesh.visual.material.PBRMaterial(baseColorTexture=p['img'], baseColorFactor=[255, 255, 255, 255], metallicFactor=0.0, roughnessFactor=0.9, doubleSided=True)
            m.visual = trimesh.visual.TextureVisuals(uv=p['UV'], material=mat)
        else:
            c = (np.array(p['color']) * 255).astype(np.uint8)
            mat = trimesh.visual.material.PBRMaterial(baseColorFactor=c.tolist(), metallicFactor=0.0, roughnessFactor=0.6, doubleSided=True,
                                                      alphaMode='BLEND' if c[3] < 250 else 'OPAQUE')
            m.visual = trimesh.visual.TextureVisuals(material=mat)
        scene.add_geometry(m, node_name=f'part{i}', geom_name=f'part{i}')
    scene.export(out)
