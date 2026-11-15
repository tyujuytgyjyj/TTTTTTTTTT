import re,json,sys,glob,os
def parse(text):
    # lenient JSON: comments, missing/trailing commas
    i=0;n=len(text)
    def ws():
        nonlocal i
        while i<n:
            c=text[i]
            if c in ' \t\r\n,\ufeff': i+=1
            elif text.startswith('//',i):
                while i<n and text[i]!='\n': i+=1
            elif text.startswith('/*',i):
                j=text.find('*/',i+2); i=n if j<0 else j+2
            else: break
    def val():
        nonlocal i
        ws()
        c=text[i]
        if c=='{':
            i+=1; d={}
            while True:
                ws()
                if text[i]=='}': i+=1; return d
                k=val(); ws()
                if text[i]==':': i+=1
                d[k]=val()
        if c=='[':
            i+=1; a=[]
            while True:
                ws()
                if text[i]==']': i+=1; return a
                a.append(val())
        if c=='"':
            j=i+1
            while text[j]!='"':
                if text[j]=='\\': j+=1
                j+=1
            s=text[i+1:j]; i=j+1
            try: return json.loads('"'+s+'"')
            except Exception: return s
        m=re.compile(r'-?\d+\.?\d*(?:[eE][-+]?\d+)?').match(text,i)
        if m and m.end()>i:
            i=m.end(); s=m.group(0)
            return float(s) if ('.' in s or 'e' in s.lower()) else int(s)
        m=re.compile(r'[A-Za-z_][A-Za-z_0-9]*').match(text,i)
        if m:
            i=m.end(); w=m.group(0)
            return {'true':True,'false':False,'null':None}.get(w,w)
        raise ValueError('bad char %r at %d'%(c,i))
    ws(); r=val(); return r
if __name__=='__main__':
    root=sys.argv[1]
    tot={'nodes':0,'beams':0,'tris':0}
    parts={}
    for f in sorted(glob.glob(root+'/**/*.jbeam',recursive=True)):
        try: d=parse(open(f,encoding='utf-8',errors='replace').read())
        except Exception as e: print('FAIL',f,e); continue
        for pn,p in d.items(): parts[pn]=(f,p)
    print('parts',len(parts))
    import collections
    keys=collections.Counter()
    for pn,(f,p) in parts.items():
        for k in p: keys[k]+=1
    print(keys.most_common(60))
