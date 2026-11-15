# يحوّل خريطة West Coast USA (BeamNG) إلى ملفات تفهمها لعبة APEX.drive:
#  hf.bin  : ارتفاعات + أسطح (شبكة 1024x1024 بخلية 2م)
#  terrain_albedo.jpg : نسيج الألوان (4096x4096) مدموج من طبقات المواد
#  spawns.txt : نقاط بداية على الأسفلت
# الاستعمال: python bake_west_coast.py west_coast_usa_bw.zip out_dir
import sys, zipfile, struct, json, io
import numpy as np
from PIL import Image
zpath, out = sys.argv[1], sys.argv[2]
z = zipfile.ZipFile(zpath)
P = 'levels/west_coast_usa_bw/'
d = z.read(P + 'west_coast_usa.ter')
n = struct.unpack('<I', d[1:5])[0]
MAXH = 384.640015
hm = np.frombuffer(d[5:5+n*n*2], dtype='<u2').reshape(n, n)
lm = np.frombuffer(d[5+n*n*2:5+n*n*3], dtype='u1').reshape(n, n)
ORIGIN = -1023.5
# ---- أسطح: 0 أسفلت، 2 تراب/حصى، 3 عشب
surf_of = {0:3, 1:3, 2:2, 3:3, 4:2, 5:2, 6:3, 7:2, 8:2, 9:0, 10:0}
lut = np.full(256, 3, dtype=np.uint8)
for k, v in surf_of.items(): lut[k] = v
step = 2  # 2048 -> 1024 (خلية 2م)
hm2 = np.ascontiguousarray(hm[::step, ::step])
sf2 = lut[lm[::step, ::step]]
m = hm2.shape[0]
with open(out + '/hf.bin', 'wb') as f:
    f.write(b'HF01')
    f.write(struct.pack('<Iffff', m, 2.0, ORIGIN, ORIGIN, MAXH / 65535.0))
    f.write(hm2.astype('<u2').tobytes())
    f.write(sf2.tobytes())
print('hf.bin', m)
# ---- نسيج الألوان: نسخة bw رمادية، فنلوّنها حسب نوع السطح ونحتفظ بتفاصيل الإضاءة منها
OUT = 4096
base = Image.open(io.BytesIO(z.read(P + 'art/terrains/t_terrain_base_b.png'))).convert('L')
base = base.transpose(Image.FLIP_TOP_BOTTOM).resize((OUT, OUT), Image.BILINEAR)  # اتجاه الصف 0 = جنوب مثل hf
L = np.asarray(base, dtype=np.float32)
tint = {0:(84,104,52), 1:(108,100,60), 2:(130,104,76), 3:(50,70,36), 4:(196,180,140), 5:(120,100,70),
        6:(92,110,56), 7:(110,104,98), 8:(128,120,110), 9:(64,64,68), 10:(74,74,78)}
lm_s = np.asarray(Image.fromarray(lm).resize((OUT, OUT), Image.NEAREST))
def blur(a, r=4):
    k = np.ones(2*r+1, dtype=np.float32) / (2*r+1)
    a = np.apply_along_axis(lambda v: np.convolve(v, k, mode='same'), 0, a)
    return np.apply_along_axis(lambda v: np.convolve(v, k, mode='same'), 1, a)
img = np.zeros((OUT, OUT, 3), dtype=np.float32)
wsum = np.zeros((OUT, OUT), dtype=np.float32)
for mid in [int(i) for i in np.unique(lm)]:
    mask = (lm_s == mid)
    if not mask.any(): continue
    col = np.array(tint.get(mid, tint[0]), dtype=np.float32)
    meanL = float(L[mask].mean()) + 1e-3
    ratio = np.clip(L / meanL, 0.5, 1.8)
    w = blur(mask.astype(np.float32))
    img += (col[None, None, :] * ratio[..., None]) * w[..., None]
    wsum += w
img /= np.maximum(wsum, 1e-4)[..., None]
Image.fromarray(np.clip(img, 0, 255).astype(np.uint8)).save(out + '/terrain_albedo.jpg', quality=88)
print('albedo done')
# ---- نقاط البداية: عقد طرق ظاهرة فوق الأسفلت وقريبة من spawn الرسمي
roads = []
for nme in z.namelist():
    if nme.endswith('items.level.json') and 'main/' in nme:
        for line in z.read(nme).decode('utf8', 'replace').splitlines():
            if '"DecalRoad"' in line:
                try: roads.append(json.loads(line))
                except Exception: pass
def surf_at(bx, by):
    ix = int(round((bx - ORIGIN) / 2.0)); iy = int(round((by - ORIGIN) / 2.0))
    ix = min(max(ix, 0), m-1); iy = min(max(iy, 0), m-1)
    return sf2[iy, ix]
cands = []
for r in roads:
    if 'invisible' in r.get('material', ''): continue
    nd = r['nodes'] if isinstance(r['nodes'], list) else json.loads(r['nodes'])
    for i in range(len(nd) - 1):
        a, b = nd[i], nd[i+1]
        dx, dy = b[0]-a[0], b[1]-a[1]
        L = (dx*dx + dy*dy) ** 0.5
        if L < 8 or surf_at(a[0], a[1]) != 0: continue
        # اتجاه السير في إحداثيات اللعبة: x=bx, z=-by
        yaw = float(np.arctan2(dx, -dy))
        cands.append((a[0], a[2], -a[1], yaw))
sp = (-712.76, 118.78, 105.04)  # spawns_industrial الأصلية (bx, h, by)
cands.sort(key=lambda c: (c[0]-sp[0])**2 + (c[2]+sp[2])**2)
chosen = []
for c in cands:
    if all((c[0]-o[0])**2 + (c[2]-o[2])**2 > 14.0**2 for o in chosen):
        chosen.append(c)
    if len(chosen) >= 8: break
with open(out + '/spawns.txt', 'w') as f:
    for c in chosen:
        f.write('%.2f %.2f %.2f %.4f\n' % c)
print('spawns', len(chosen), chosen[:3])
