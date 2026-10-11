#!/usr/bin/env python3
"""Reproduce full upstream equality/metadata workflows and explicit adaptations.

Upstream Copyright (c) Rich Hickey; Eclipse Public License 1.0. Original notices
remain in the pinned research/clojurescript sources; see epl-v10.html there.
"""
from pathlib import Path
import hashlib
import json
import re
import sys
import subprocess
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import extract_inventory as inv
FILES = ['hashing_test.cljs', 'hash_map_test.cljs', 'hash_set_test.cljs',
         'set_equiv_test.cljs', 'metadata_test.cljc', 'map_entry_test.cljs', 'var_test.cljs']
REPLACEMENTS = {
 '(gobject/get (js-obj) "foo")': '(aget (js-obj) "foo")',
 '(.-EMPTY List)': '()', '(.-EMPTY PersistentVector)': '[]',
 '(.-EMPTY PersistentHashSet)': '(hash-set)', '(.-EMPTY PersistentTreeSet)': '(sorted-set)',
 '(.-EMPTY PersistentArrayMap)': '(array-map)', '(.-EMPTY PersistentHashMap)': '(hash-map)',
 '(.-EMPTY PersistentTreeMap)': '(sorted-map)',
 '(BlackNode. :key :val nil nil nil)': '(first (sorted-map :key :val))',
 '(RedNode. :key :val nil nil nil)': '(first (sorted-map :key :val :other :value))',
}
PRIVATE = {'(instance? NodeSeq node-seq)', '(instance? ArrayNodeSeq array-node-seq)'}

def adapt(text, n):
    raw = text[n.start:n.end]
    if raw in REPLACEMENTS:
        return REPLACEMENTS[raw]
    if n.kind == 'conditional':
        pairs = n.children[0].children
        return next(adapt(text, pairs[i+1]) for i in range(0, len(pairs), 2) if pairs[i].token == ':cljs')
    h = inv.short_head(n)
    if h == 'deftest':
        return '(do ' + ' '.join(adapt(text, c) for c in n.children[2:]) + ')'
    if h == 'testing':
        return '(do ' + ' '.join(adapt(text, c) for c in n.children[2:]) + ')'
    if h == 'is':
        expr = n.children[1]
        if text[expr.start:expr.end] in PRIVATE:
            return 'nil'
        return '(swap! observations conj (boolean ' + adapt(text, expr) + '))'
    if h == 'are':
        bindings, expr, values = n.children[1].children, n.children[2], n.children[3:]
        width = len(bindings)
        assert width and len(values) % width == 0
        return '(do ' + ' '.join('(let [' + ' '.join(adapt(text,b)+' '+adapt(text,v) for b,v in zip(bindings,values[i:i+width])) + '] (swap! observations conj (boolean '+adapt(text,expr)+')))' for i in range(0,len(values),width)) + ')'
    if h == 'thrown?':
        return '(try ' + ' '.join(adapt(text,c) for c in n.children[2:]) + ' false (catch ' + adapt(text,n.children[1]) + ' e true))'
    if not n.children:
        return raw
    parts, offset = [], n.start
    for c in n.children:
        # Drop comments only in reader whitespace, never inside strings/regexes.
        parts.extend([re.sub(r';[^\n]*', '', text[offset:c.start]), adapt(text,c)])
        offset = c.end
    return ''.join(parts) + re.sub(r';[^\n]*', '', text[offset:n.end])

def generate():
    result=[]
    manifest=[]
    for filename in FILES:
        path=ROOT/'research/clojurescript/src/test/cljs/cljs'/filename
        text=path.read_text()
        pinned=subprocess.check_output(['git', '-C', str(ROOT/'research/clojurescript'),
            'show', inv.REVISION+':src/test/cljs/cljs/'+filename]).decode()
        assert text == pinned, 'Upstream source differs from pinned revision: '+filename
        forms=inv.Reader(text).all()
        helpers=[f for f in forms if inv.short_head(f) in {'defn','defrecord','deftype','def'}]
        for f in forms:
            if inv.short_head(f) != 'deftest': continue
            name=f.children[1].token
            prefix=' '.join(adapt(text,h) for h in helpers)
            source='(do (def observations (atom [])) '+prefix+' '+adapt(text,f)+' @observations)'
            # Compact whitespace structurally, retaining spaces in literal strings.
            source=source.replace('\n',' ').replace('\t',' ')
            identity='cljs-equality-'+filename.replace('.cljc','').replace('.cljs','').replace('_','-')+'-'+name
            sha=hashlib.sha256(text[f.start:f.end].encode()).hexdigest()
            result.append([identity,source,'PENDING',filename+'::'+name+';sha256='+sha])
            manifest.append({'id':identity,'source':str(path.relative_to(ROOT)), 'body':name,'sha256':sha,'setup_sha256':[hashlib.sha256(text[h.start:h.end].encode()).hexdigest() for h in helpers]})
    return result,manifest

if __name__=='__main__':
    rows,manifest=generate()
    target=ROOT/'tests/fixtures/compiler-equality.tsv'
    if '--write' in sys.argv:
        target.write_text('# Full pinned upstream equality/metadata bodies; see tests/reference/compiler-equality.md\n'+'\n'.join('\t'.join(r) for r in rows)+'\n')
        (ROOT/'tests/reference/compiler-equality-provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
    else:
        actual=[l.split('\t') for l in target.read_text().splitlines() if l and not l.startswith('#')]
        assert len(actual)==len(rows)
        for a,b in zip(actual,rows): assert a[:2]+a[3:]==b[:2]+b[3:],a[0]
        assert json.loads((ROOT/'tests/reference/compiler-equality-provenance.json').read_text())==manifest
        print(f'{len(rows)} complete upstream bodies and all helper setup reproduce exactly')
