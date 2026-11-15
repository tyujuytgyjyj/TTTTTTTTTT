import json,struct,sys
def info(path):
    b=open(path,'rb').read()
    l=struct.unpack('<I',b[12:16])[0]; j=json.loads(b[20:20+l])
    acc=j['accessors']; tv=0; tt=0; mn=[1e9]*3; mx=[-1e9]*3
    for m in j.get('meshes',[]):
        for p in m['primitives']:
            a=acc[p['attributes']['POSITION']]; tv+=a['count']
            tt+=(acc[p['indices']]['count'] if 'indices' in p else a['count'])//3
            for i in range(3): mn[i]=min(mn[i],a['min'][i]); mx[i]=max(mx[i],a['max'][i])
    imgs=j.get('images',[]); bv=j.get('bufferViews',[])
    isz=[bv[i['bufferView']]['byteLength']//1024 for i in imgs if 'bufferView' in i]
    mats=j.get('materials',[])
    print(path.split('/')[-1],'meshes',len(j.get('meshes',[])),'nodes',len(j.get('nodes',[])),'verts',tv,'tris',tt,'mats',len(mats),'images',len(imgs),'img KB',sorted(isz)[-4:],'bbox',[round(x,1) for x in mn],[round(x,1) for x in mx], 'ext',j.get('extensionsUsed'))
for p in sys.argv[1:]: info(p)
