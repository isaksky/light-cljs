#!/usr/bin/env python3
"""Reproduce pinned public name/kind and newly exposed Var metadata catalogs.

Source ClojureScript a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e, EPL-1.0.
This is source provenance, never an execution or compatibility claim.
"""
from pathlib import Path
import json
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import extract_inventory as inv
rows = [json.loads(line) for line in (ROOT / 'tests/reference/compiler-language/obligations.jsonl').read_text().splitlines()]
core_sources = {'src/main/cljs/cljs/core.cljs', 'src/main/clojure/cljs/core.cljc'}
names = {}
for row in rows:
    if row.get('obligation_kind') == 'public-api' and row.get('disposition') == 'intended' and row.get('source') in core_sources:
        kind = {'deftype':'Type', 'defrecord':'Record', 'defprotocol':'Protocol', 'defmacro':'Macro', 'imported-macro':'Macro'}.get(row.get('declaration'), 'Value')
        if names.get(row['name']) != 'Macro': names[row['name']] = kind
catalog = ';; Public declarations from pinned ClojureScript a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e; EPL-1.0. Runtime availability is filtered by CoreFnId.\n' + ''.join(f'{name}\t{kind}\n' for name,kind in sorted(names.items()))
metadata_names = set('Namespace create-ns find-ns ns-name system-time add-tap remove-tap tap> flush set-print-fn! set-print-err-fn! string-print *print-meta* *print-dup* *flush-on-newline* *print-fn-bodies* *print-level* *print-length* *print-err-fn*'.split())
by_name = {}
for row in rows:
    if row.get('obligation_kind') == 'public-api' and row['name'] in metadata_names and row.get('source') == 'src/main/cljs/cljs/core.cljs': by_name.setdefault(row['name'], []).append(row)
metadata = {}
for name, declarations in sorted(by_name.items()):
    row = declarations[0]; span = row['source_span']; text = span['text']; form = inv.Reader(text).all()[0]
    doc = 'nil'
    for node in form.children[2:]:
        if node.kind == 'string':
            doc = text[node.start:node.end].replace('\n', '\\n').replace('\r', '\\r').replace('\t', '\\t'); break
        if node.kind in {'vector', 'list'}: break
    if doc == 'nil':
        def find_doc(node):
            if node.kind == 'map':
                for key,value in zip(node.children[::2],node.children[1::2]):
                    if key.token == ':doc' and value.kind == 'string': return text[value.start:value.end].replace('\n','\\n').replace('\r','\\r').replace('\t','\\t')
            for child in node.children:
                result = find_doc(child)
                if result: return result
        doc = find_doc(form.children[1]) or 'nil'
    params = list(dict.fromkeys('[' + ' '.join(d['signature']['parameters']) + ']' for d in declarations))
    arglists = '()' if name.startswith('*') or row['declaration'] == 'deftype' else '(' + ' '.join(params) + ')'
    column = 10 if row['declaration'] == 'deftype' else 1
    metadata[name] = f'{{:file "cljs/core.cljs" :line {span["start_line"]} :column {column} :doc {doc} :arglists {arglists}' + (' :dynamic true' if name.startswith('*') else '') + '}'
path = ROOT / 'src/core_public_bindings.tsv'
meta_path = ROOT / 'src/core_var_metadata.tsv'
if '--write' in sys.argv:
    path.write_text(catalog)
# The pinned analyzer generator owns the complete metadata table. Compare the
# independent public-source projection structurally; extra analyzer fields and
# map key order are immaterial. Never replace canonical rows with this subset.
assert path.read_text() == catalog, 'Public core declaration catalog differs from pinned inventory'
current = dict(line.split('\t', 1) for line in meta_path.read_text().splitlines() if '\t' in line)
def shape(text, node):
    if node.children: return (node.kind, tuple(shape(text, child) for child in node.children))
    return (node.kind, text[node.start:node.end])
def fields(text):
    form = inv.Reader(text).all()[0]
    return {key.token: shape(text, value) for key, value in zip(form.children[::2], form.children[1::2])}
for name, data in metadata.items():
    expected, actual = fields(data), fields(current[name])
    for key, value in expected.items():
        assert actual.get(key, ('atom', 'nil')) == value, f'Var metadata differs: {name} {key}'
print(f'{len(names)} pinned public names/kinds and {len(metadata)} reflected Var metadata projections match')
# Executable declaration/arity links are independently hashed; source catalogs
# above do not supply a semantic pass.
import hashlib
namespace_names = {'Namespace','create-ns','find-ns','ns-name','ns-interns','ns-publics','ns-imports'}
host_names = {'system-time','add-tap','remove-tap','tap>'}
direct,phase = [],[]
for row in rows:
    name = row['name'].rsplit('/',1)[-1]
    if row['owner'] != 'light-cljs-124' or row['disposition'] != 'intended' or row['obligation_kind'] not in {'public-api','local-api','local-metadata'} or name not in namespace_names | host_names: continue
    path,test = ('tests/compiler_reflection_completion.rs','namespace_api_arities_execute_in_applications_and_macros') if name in namespace_names else ('tests/compiler_host_hooks.rs','host_hooks_have_macro_application_phase_witnesses')
    execution = {'test_file':path,'test_name':test,'test_source_sha256':hashlib.sha256((ROOT/path).read_bytes()).hexdigest()}
    rationale = 'The entire hashed contract executes this exact public declaration and retained arity through Node/QuickJS applications and a QuickJS macro. Namespace tests retain constructor identity, source public/private Var metadata, real saved invocation and imports; host tests retain ordered tap registration/removal/reentrancy and the ECMAScript clock contract. Host scheduling/callback differences remain explicit in the embedding contract.'
    common={'obligation_id':row['id'],'body_sha256':row['body_sha256'],'rationale':rationale}
    if row.get('signature'): common['signature_sha256']=hashlib.sha256(json.dumps(row['signature'],sort_keys=True).encode()).hexdigest()
    direct.append({**common,'local_execution':execution,'assertion_ids':[]})
    phase.append({**common,**execution})
for filename,expected in [('obligation-witnesses.jsonl',direct),('phase-witnesses.jsonl',phase)]:
    destination=ROOT/'tests/reference/compiler-language'/filename
    identities={row['obligation_id'] for row in expected}
    if '--write' in sys.argv:
        old=[line for line in destination.read_text().splitlines() if line and json.loads(line)['obligation_id'] not in identities]
        destination.write_text('\n'.join(old)+'\n'+''.join(json.dumps(row,separators=(',',':'))+'\n' for row in expected))
    else:
        actual={row['obligation_id']:row for row in [json.loads(line) for line in destination.read_text().splitlines()]}
        for row in expected: assert actual[row['obligation_id']] == row, f'{filename} differs: {row["obligation_id"]}'
print(f'{len(direct)} namespace/host public and local execution/phase mappings match')
