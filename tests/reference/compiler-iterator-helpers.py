#!/usr/bin/env python3
"""Retain pinned public iterator declarations, signatures, setups and observations."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import zipfile
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / 'tests/reference/compiler-language'
sys.path.insert(0, str(LEDGER))
import extract_inventory as inv
import report
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--write', action='store_true')
parser.add_argument('--reference', action='store_true')
parser.add_argument('--artifacts', type=Path)
args = parser.parse_args()
sha = lambda data: hashlib.sha256(data).hexdigest()
TEST = 'tests/compiler_iterator_helpers.rs'
SUPPORT = ['tests/fixtures/compiler-iterator-helpers.cljs', 'tests/fixtures/compiler-core-factory-bodies.tsv']
NAME = 'iterator_helpers_execute_in_applications_and_macros'
read = lambda name: [json.loads(line) for line in (LEDGER/name).read_text().splitlines() if line]
execution = {'test_file': TEST, 'test_name': NAME, 'test_source_sha256': sha((ROOT/TEST).read_bytes())}
supports = {p: sha((ROOT/p).read_bytes()) for p in SUPPORT}
rationale = ('Direct, saved and apply seq-iter/ranged-iterator helpers execute in QuickJS/Node applications and canonical macros. Literal metadata signatures, nominal/slot identity, INIT/START initialization, ordered lazy effects, repeated hasNext, host array/iterator chunk mutation, explicit empty/reversed/end bounds, exhausted throw/unchecked-next distinctions and multiple vector chunk transitions are retained. The complete pinned iterator equivalence helper, record and test body run independently without removing setup or iteration. Phase roots, unsupported expansion result diagnostics/recovery and 1000 repetitions have separate local witnesses.')
obligations, phases = [], []
for target in read('obligations.jsonl'):
    if target['name'] not in {'seq-iter', 'ranged-iterator'} or target['disposition'] != 'intended': continue
    common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'], 'rationale': rationale}
    if target.get('signature'):
        common['signature_sha256'] = sha(json.dumps(target['signature'],sort_keys=True).encode())
    obligations.append({**common, 'local_execution': execution, 'assertion_ids': []})
    phases.append({**common, **execution, 'support_source_sha256': supports})
assert len(obligations) == 4, len(obligations)

# Preserve reviewed exact pinned-reference oracles when reproducing this family's bindings.
import runpy
attach_reference = runpy.run_path(str(ROOT / 'tests/reference/compiler-public-gate-links.py'))['attach_reference']
obligations = [attach_reference(row) for row in obligations]
for filename, expected in [('obligation-witnesses.jsonl', obligations), ('phase-witnesses.jsonl', phases)]:
    identities = {r['obligation_id'] for r in expected}
    path = LEDGER/filename
    if args.write:
        retained = [line for line in path.read_text().splitlines() if json.loads(line)['obligation_id'] not in identities]
        path.write_text('\n'.join(retained+[json.dumps(r,separators=(',', ':')) for r in expected])+'\n')
    else:
        current = {r['obligation_id']:r for r in read(filename)}
        for row in expected: assert current[row['obligation_id']] == row, (filename,row['obligation_id'])
print('Four public/local iterator helper execution mappings verified',flush=True)
if args.reference:
    assert args.artifacts, '--reference requires fresh --artifacts'
    args.artifacts.mkdir(parents=True, exist_ok=False)
    contract = ' '.join(line for line in (ROOT/SUPPORT[0]).read_text().splitlines() if not line.lstrip().startswith(';'))
    cases = [{'id':'iterator-helper-contract', 'source':'(do '+contract+' (iterator-helper-contract))', 'isolated':True}]
    for line in (ROOT/SUPPORT[1]).read_text().splitlines():
        if line.startswith('core-factory-body-src-test-cljs-cljs-iterator-test-coll-iter-seq-match\t'):
            identity,source,expected,provenance = line.split('\t')
            assert expected == 'true'
            cases.append({'id':identity, 'source':source, 'isolated':True})
            upstream_setup = json.loads(provenance)
    assert len(cases)==2
    for case in cases:
        case['expected'] = {'id':case['id'],'outcome':{'status':'returned','value':True},'trace':[],'stdout':''}
    receipt = {'owner':'light-cljs-144','cases':cases,'unchanged_source_sha256':{c['id']:sha(c['source'].encode()) for c in cases},
               'references':{},'complete_upstream_declarations':{},'complete_upstream_setup':upstream_setup,
               'local_witnesses':{TEST:execution['test_source_sha256'],**supports}}
    for kind in ['release','checkout']:
        if kind == 'release':
            jar = Path.home()/'.m2/repository/org/clojure/clojurescript/1.12.134/clojurescript-1.12.134.jar'
            with zipfile.ZipFile(jar) as z: data=z.read('cljs/core.cljs')
            origin='Maven org.clojure/clojurescript 1.12.134 cljs/core.cljs'
        else:
            data=(ROOT/'research/clojurescript/src/main/cljs/cljs/core.cljs').read_bytes()
            origin=inv.REVISION+' src/main/cljs/cljs/core.cljs'
        text=data.decode(); declarations=[]
        for form in inv.Reader(text).all():
            if form.kind == 'list' and len(form.children)>1 and form.children[1].token in {'INIT','START','SeqIter','RangedIterator','seq-iter','ranged-iterator'}:
                body=text[form.start:form.end]
                declarations.append({'name':form.children[1].token,'source':origin,'text':body,'sha256':sha(body.encode()),'start_line':text.count('\n',0,form.start)+1})
        assert len(declarations)==6
        receipt['complete_upstream_declarations'][kind]=declarations
        result=report.reference_cases(cases,kind,ROOT/'research/clojurescript',args.artifacts/kind,180)
        receipt['references'][kind]=result
        print(kind, json.dumps({id:r['status'] for id,r in result.items()}),flush=True)
        assert len(result)==2 and all(r['status']=='passed' for r in result.values()),result
    (args.artifacts/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
