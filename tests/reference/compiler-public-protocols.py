#!/usr/bin/env python3
"""Reconcile public protocol source/body/signature evidence; execute checked pins.

Adapted upstream source is Copyright (c) Rich Hickey, EPL-1.0. Original
notices are retained in research/clojurescript; see its epl-v10.html.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tests/reference/compiler-language'
sys.path.insert(0, str(HERE))
import extract_inventory as inv
import full_ledger
import report
spec = importlib.util.spec_from_file_location('sequence_adapter', ROOT / 'tests/reference/compiler-sequence-completion.py')
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--write', action='store_true')
parser.add_argument('--reference', choices=['release', 'checkout', 'both'])
parser.add_argument('--artifacts', type=Path)
args = parser.parse_args()
sha = lambda data: hashlib.sha256(data).hexdigest()
read = lambda name: [json.loads(line) for line in (HERE / name).read_text().splitlines() if line]
PREFIX = 'public-protocol-body-'
TEST = 'tests/compiler_public_protocols.rs'
SUPPORT = ['tests/fixtures/compiler-public-protocols.tsv', 'tests/fixtures/compiler-public-protocol-bodies.tsv']
NAME = 'public_protocol_api_arities_and_macro_phase_execute'
TARGETS = {'Inst', 'inst-ms*', 'inst-ms', 'inst?', 'IUUID', 'uuid?', 'ASeq', 'IDerefWithTimeout', '-deref-with-timeout'}
BODIES = {'core_test.cljs::test-uuid': [], 'src/test/cljs/cljs/predicates_test.cljs::test-preds': ['src/test/cljs/cljs/predicates_test.cljs::pred-val-table']}
bodies = {b['id']: b for b in read('full-bodies.jsonl')}
rows, body_map = [], {}
for body_id, setup in BODIES.items():
    forms, provenance = [], []
    for identity in [*setup, body_id]:
        original = bodies[identity]
        span = original['source_span']
        text = span['text']
        assert sha(text.encode()) == span['sha256']
        assert full_ledger.pinned(original['source'])[span['start_byte']:span['end_byte']].decode() == text
        forms.append(adapter.adapt(text, inv.Reader(text).all()[0]))
        provenance.append({'body_id':identity,'source':original['source'],'sha256':span['sha256']})
    # Preserve the source namespace of the sole auto-qualified table keyword
    # when embedding the entire table/body in the independent case namespace.
    source = '(do ' + ' '.join(forms) + ')'
    source = source.replace('::foo', ':cljs.predicates-test/foo').replace('\n', ' ').replace('\t', ' ')
    identity = PREFIX + body_id.replace('/', '-').replace('.cljs::', '-').replace('_', '-')
    rows.append([identity, source, 'true', json.dumps(provenance, separators=(',', ':'))])
    body_map[body_id] = identity
body_fixture = '# Copyright (c) Rich Hickey; EPL-1.0; retain this notice.\n# id\tsource\texpected\tcomplete source/body hashes\n' + ''.join('\t'.join(row)+'\n' for row in rows)
p = ROOT / SUPPORT[1]
if args.write: p.write_text(body_fixture)
else: assert p.read_text() == body_fixture, 'Upstream protocol-body fixture drift'
execution = {'test_file':TEST,'test_name':NAME,'test_source_sha256':sha((ROOT/TEST).read_bytes())}
supports = {path:sha((ROOT/path).read_bytes()) for path in SUPPORT}
rationale = ('Execute all four public protocol identities, fixed direct methods, predicates, native/default extensions, marker membership, receiver identity, saved/apply calls and negative dispatch in QuickJS/Node applications and canonical macros; complete UUID and predicate table bodies retain every original setup, iteration and assertion. Persistent results and isolated phase mutation are explicitly tested.')
phases, mappings = [], []
for target in read('obligations.jsonl'):
    if target['name'] not in TARGETS or target['disposition'] != 'intended' or target['obligation_kind'] not in {'public-api','local-api'}: continue
    common = {'obligation_id':target['id'],'body_sha256':target['body_sha256'],'rationale':rationale}
    if target.get('signature'): common['signature_sha256'] = sha(json.dumps(target['signature'],sort_keys=True).encode())
    phases.append({**common,**execution,'support_source_sha256':supports})
    mappings.append({**common,'local_execution':execution})
# Existing source assertion witnesses remain authoritative, while the full UUID
# body is still executed independently; the previously unwitnessed predicate
# table obtains all its original assertion-site links here.
existing = {r['assertion_id'] for f in ('witnesses.jsonl','full-witnesses.jsonl') for r in read(f) if not r['fixture_id'].startswith(PREFIX)}
witnesses = [{'assertion_id':r['id'],'assertion_sha256':r['assertion']['sha256'],'fixture_id':body_map[r['body_id']],
    'rationale':'Complete pinned body/setup; is becomes assert, testing becomes do. Every predicate table row and iteration, original expected value/message and UUID hashing/lookup path is retained. ::foo is explicitly qualified to its original source namespace in the table adapter.'}
    for r in read('full-inventory.jsonl') if r['body_id'] in body_map and r['disposition']=='in-scope' and r['id'] not in existing]
cases = [{'id':id,'source':source,'expected':{'id':id,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''},'native_stdout':None} for id,source,_,_ in rows]
# Preserve reviewed exact pinned-reference oracles when reproducing this family's bindings.
import runpy
oracle_links = runpy.run_path(str(ROOT / 'tests/reference/compiler-public-gate-links.py'))
attach_reference = oracle_links['attach_reference']
mappings = [attach_reference(row) for row in mappings]
for filename, generated, key, replace in [
    ('phase-witnesses.jsonl',phases,'obligation_id',lambda r:r.get('test_file')==TEST),
    ('obligation-witnesses.jsonl',mappings,'obligation_id',lambda r:r.get('local_execution',{}).get('test_file')==TEST),
    ('full-witnesses.jsonl',witnesses,'assertion_id',lambda r:r['fixture_id'].startswith(PREFIX)),
    ('full-cases.jsonl',[c for c in cases if c['id'] in {w['fixture_id'] for w in witnesses}],'id',lambda r:r['id'].startswith(PREFIX))]:
    prior = [r for r in read(filename) if not replace(r)]
    existing_keys = {r[key] for r in prior}
    # Replace the exact named public contracts; other pre-existing witnesses for
    # these predicates remain valid evidence and do not need replacement.
    generated = [r for r in generated if r[key] not in existing_keys]
    result = ''.join(json.dumps(r,separators=(',',':'))+'\n' for r in oracle_links['replace_owned'](read(filename), generated, replace))
    path = HERE / filename
    if args.write: path.write_text(result)
    else: assert [r for r in read(filename) if replace(r)] == generated, filename+' reconciliation drift'
print(json.dumps({'upstream_bodies':len(rows),'public_local_phase_rows':len(phases),'source_assertions':len(witnesses)}),flush=True)
if args.reference:
    assert args.artifacts, '--reference requires a fresh --artifacts directory'
    args.artifacts.mkdir(parents=True,exist_ok=False)
    contracts=[]
    for line in (ROOT/SUPPORT[0]).read_text().splitlines():
        if line.startswith('#') or not line: continue
        id,source,expected=line.split('\t')
        assert expected=='true'
        contracts.append({'id':id,'source':source,'isolated':True,'expected':{'id':id,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''}})
    for kind in ('release','checkout') if args.reference=='both' else [args.reference]:
        observed=report.reference_cases([*contracts,*cases],kind,ROOT/'research/clojurescript',args.artifacts/kind,180)
        (args.artifacts/(kind+'.json')).write_text(json.dumps(observed,indent=2)+'\n')
        print(kind,json.dumps({id:r['status'] for id,r in observed.items()}),flush=True)
        assert len(observed)==len(contracts)+len(cases) and all(r['status']=='passed' for r in observed.values()), observed
