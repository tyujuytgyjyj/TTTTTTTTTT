import re, numpy as np, xml.etree.ElementTree as ET
def strip_ns(root):
    for e in root.iter():
        if '}' in e.tag: e.tag = e.tag.split('}',1)[1]
def parse_dae(text):
    root = ET.fromstring(text); strip_ns(root)
    geoms = {}
    for g in root.iter('geometry'):
        mesh = g.find('mesh')
        if mesh is None: continue
        srcs = {}
        for s in mesh.findall('source'):
            fa = s.find('float_array')
            if fa is None or not fa.text: continue
            arr = np.array(fa.text.split(), dtype=np.float32)
            acc = s.find('technique_common/accessor'); stride = int(acc.get('stride')) if acc is not None else 3
            srcs[s.get('id')] = arr.reshape(-1, stride)
        vtx = mesh.find('vertices'); vmap = {}
        if vtx is not None:
            for inp in vtx.findall('input'): vmap[inp.get('semantic')] = inp.get('source').lstrip('#')
        prims = []
        for p in mesh.findall('triangles') + mesh.findall('polylist'):
            ins = p.findall('input'); maxoff = max(int(i.get('offset')) for i in ins) + 1
            sem = {}
            for i in ins:
                s = i.get('semantic'); src = i.get('source').lstrip('#'); off = int(i.get('offset'))
                if s == 'VERTEX':
                    sem['POSITION'] = (vmap.get('POSITION'), off)
                    if 'NORMAL' in vmap and 'NORMAL' not in sem: sem['NORMAL'] = (vmap['NORMAL'], off)
                elif s == 'NORMAL': sem['NORMAL'] = (src, off)
                elif s == 'TEXCOORD' and 'TEXCOORD' not in sem: sem['TEXCOORD'] = (src, off)
            pe = p.find('p')
            if pe is None or not pe.text: continue
            idx = np.array(pe.text.split(), dtype=np.int64).reshape(-1, maxoff)
            if p.tag == 'polylist':
                vc = np.array(p.find('vcount').text.split(), dtype=np.int64)
                out = []; pos = 0
                for c in vc:
                    f = idx[pos:pos+c]; pos += c
                    for k in range(1, c-1): out += [f[0], f[k], f[k+1]]
                idx = np.array(out)
            P = srcs[sem['POSITION'][0]][idx[:, sem['POSITION'][1]]]
            N = srcs[sem['NORMAL'][0]][idx[:, sem['NORMAL'][1]]] if 'NORMAL' in sem else np.tile([0,0,1],(len(idx),1)).astype(np.float32)
            T = srcs[sem['TEXCOORD'][0]][idx[:, sem['TEXCOORD'][1]]][:, :2] if 'TEXCOORD' in sem else np.zeros((len(idx),2),np.float32)
            prims.append((p.get('material'), P, N, T))
        geoms[g.get('id')] = prims
    # visual scene: collect instances with world transform
    def mat_of(node):
        m = node.find('matrix')
        return np.array(m.text.split(), dtype=np.float64).reshape(4,4) if m is not None else np.eye(4)
    inst = []
    def walk(node, M):
        M = M @ mat_of(node)
        for ig in node.findall('instance_geometry'):
            binds = {}
            for im in ig.iter('instance_material'): binds[im.get('symbol')] = (im.get('target') or '').lstrip('#')
            inst.append((ig.get('url').lstrip('#'), M, binds, ig.get('name') or node.get('name') or ''))
        for c in node.findall('node'): walk(c, M)
    for vs in root.iter('visual_scene'):
        for n in vs.findall('node'): walk(n, np.eye(4))
    up = root.find('asset/up_axis'); upaxis = up.text if up is not None else 'Y_UP'
    return geoms, inst, upaxis
