import sys,glob,json
sys.path.insert(0,'/home/claude/tools')
from jb import parse
root='/home/claude/mods/car/vehicles'
parts={}
for f in glob.glob(root+'/**/*.jbeam',recursive=True):
    d=parse(open(f,encoding='utf-8',errors='replace').read())
    for pn,p in d.items(): parts[pn]=p
cfg=json.load(open(sys.argv[1]))
chosen=cfg['parts']
active={}
def add(pname):
    if pname in active or pname not in parts: return
    active[pname]=parts[pname]
    p=parts[pname]
    for key in ('slots','slots2'):
        rows=p.get(key,[])
        for r in rows[1:]:
            if key=='slots': slot=r[0]; default=r[1] if len(r)>1 else ''
            else: slot=r[0]; default=r[3] if len(r)>3 else ''
            c=chosen.get(slot,None)
            if c is None: c=default
            if c: add(c)
add(cfg['mainPartName'])
def rows(p,k):
    t=p.get(k,[])
    return [r for r in t[1:] if isinstance(r,list)]
N=B=T=W=F=0
for pn,p in active.items():
    N+=len(rows(p,'nodes')); B+=len(rows(p,'beams')); T+=len(rows(p,'triangles')); F+=len(rows(p,'flexbodies'))
    W+=len(rows(p,'pressureWheels'))
print(sys.argv[1].split('/')[-1],'active parts',len(active),'nodes',N,'beams',B,'tris',T,'flexbodies',F,'wheels',W)
eng=[ (pn,p['mainEngine']) for pn,p in active.items() if 'mainEngine' in p]
for pn,e in eng: print('engine',pn,{k:(v if not isinstance(v,list) else '[..]') for k,v in e.items() if k in('idleRPM','maxRPM','inertia','torque','torqueCurve')})
for pn,p in active.items():
    if 'pressureWheels' in p:
        print('wheels',pn,[ (r[0],r[1]) for r in rows(p,'pressureWheels')][:2])
