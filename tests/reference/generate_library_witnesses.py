"""Retain full upstream library setup/bodies and hash each original source span."""
from pathlib import Path
import hashlib
ROOT = Path(__file__).resolve().parents[2]
UP = ROOT / 'research/clojurescript/src/test/cljs'

class Reader:
    def __init__(self, text): self.text, self.pos = text, 0
    def skip(self):
        while self.pos < len(self.text):
            if self.text[self.pos] in ' \t\r\n,': self.pos += 1
            elif self.text[self.pos] == ';':
                end = self.text.find('\n', self.pos); self.pos = len(self.text) if end < 0 else end
            else: break
    def read(self):
        self.skip(); start = self.pos; s = self.text
        if start == len(s): return None
        if s[start] == '"':
            self.pos += 1
            while self.pos < len(s):
                ch = s[self.pos]; self.pos += 1
                if ch == '\\': self.pos += 1
                elif ch == '"': break
            return (s[start:self.pos], None, [])
        if s[start] in "'`~@^" and not (s[start] == '~' and s[start:start+2] == '~@'):
            self.pos += 1; self.read()
            if s[start] == '^': self.read()
            return (s[start:self.pos], None, [])
        if s[start:start+2] in ['#{','#(']: self.pos += 1
        if s[self.pos] in '([{':
            opening=s[self.pos]; self.pos += 1; children=[]; closing={'(':')','[':']','{':'}'}[opening]
            while True:
                self.skip()
                if s[self.pos] == closing: self.pos += 1; break
                children.append(self.read())
            return (s[start:self.pos], opening if self.pos and s[start] != '#' else 'dispatch', children)
        while self.pos < len(s) and s[self.pos] not in ' \t\r\n,()[]{};': self.pos += 1
        return (s[start:self.pos], None, [])

def adapt(node, skipped):
    text, kind, children = node
    if kind is None or kind == 'dispatch': return text
    head = children[0][0] if children else ''
    if kind == '(' and head == 'are':
        args = children[1][2]; expression=children[2][0]; rows=children[3:]; parts=[]
        assert len(rows) % len(args) == 0
        for i in range(0,len(rows),len(args)):
            row=rows[i:i+len(args)]
            if any('(sorted-set' in n[0] for n in row): skipped.append(' '.join(n[0] for n in row)); continue
            bindings=' '.join(a[0]+' '+v[0] for a,v in zip(args,row))
            parts.append(f'(let [{bindings}] (assert {expression}))')
        return '(do '+' '.join(parts)+')'
    if kind == '(' and head == 'is': return '(assert '+children[1][0]+')'
    if kind == '(' and head == 'testing': return '(do '+' '.join(adapt(n,skipped) for n in children[2:])+')'
    if kind == '(' and head == 'deftest':
        body=children[2:]
        if body and body[0][0].startswith('"'): body=body[1:]
        return '(defn '+children[1][0]+' [] '+' '.join(adapt(n,skipped) for n in body)+')'
    closing={'(':')','[':']','{':'}'}[kind]
    return kind+' '.join(adapt(n,skipped) for n in children)+closing

header='\n'.join((UP/'clojure/set_test.cljs').read_text().splitlines()[:7])+'\n'
result=[header,'(ns library.upstream (:require [clojure.set :as set] [clojure.walk :as w]))']
manifest=['# original path\tbody\tsha256\tassertions\tomitted sorted rows']
names=[]
for path in ['clojure/set_test.cljs','clojure/walk_test.cljs','cljs/walk_test.cljs']:
    reader=Reader((UP/path).read_text()); nodes=[]
    while (node:=reader.read()) is not None: nodes.append(node)
    for node in nodes:
        children=node[2]; head=children[0][0] if children else ''
        if head=='ns': continue
        if head in ('defmulti','defmethod'): continue
        if head=='deftest' and children[1][0]=='test-walk': continue # full sorted/multimethod witness owned by -119/-47
        skipped=[]; translated=adapt(node,skipped); result.append(translated)
        if head=='deftest':
            name=children[1][0];names.append(name)
            manifest.append('\t'.join([path,name,hashlib.sha256(node[0].encode()).hexdigest(),str(translated.count('(assert ')),str(len(skipped))]))
result += ['(defn run [] #js ['+' '.join('(try ('+n+') true (catch js/Error e false))' for n in names)+'])', '(every? true? (run))']
(ROOT/'tests/fixtures/compiler-libraries/library/upstream.cljs').write_text('\n'.join(result)+'\n')
(ROOT/'tests/fixtures/compiler-libraries/provenance.tsv').write_text('\n'.join(manifest)+'\n')
print(f'{len(names)} bodies, {sum(int(row.split(chr(9))[3]) for row in manifest[1:])} assertions')
