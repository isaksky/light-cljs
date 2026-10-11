#!/usr/bin/env python3
"""Reconcile public constructor source/body/signature evidence; execute checked pins.

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
base_adapt = adapter.adapt
def adapt(text, node):
    # The ledger reader deliberately retains #_ forms. Preserve their complete
    # spelling as reader-discarded source, rather than turning the enclosing
    # discarded is into an executable empty assert through short_head.
    if node.kind == 'discard':
        return text[node.start:node.end]
    return base_adapt(text, node)
adapter.adapt = adapt
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--write', action='store_true')
parser.add_argument('--reference', choices=['release', 'checkout', 'both'])
parser.add_argument('--artifacts', type=Path)
args = parser.parse_args()
sha = lambda data: hashlib.sha256(data).hexdigest()
read = lambda name: [json.loads(line) for line in (HERE / name).read_text().splitlines() if line]
PREFIX = 'public-constructor-body-'
TEST = 'tests/compiler_public_constructors.rs'
SUPPORT = ['tests/fixtures/compiler-public-constructors.tsv', 'tests/fixtures/compiler-public-constructor-bodies.tsv']
NAME = 'public_constructor_api_arities_and_macro_phase_execute'
CONSTRUCTORS = {'Cons', 'Reduced', 'Symbol', 'List', 'EmptyList', 'Keyword',
    'Cycle', 'Repeat', 'Iterate', 'PersistentVector', 'Subvec', 'ChunkedSeq',
    'PersistentQueue', 'PersistentArrayMap', 'PersistentHashMap', 'PersistentHashSet'}
TARGETS = CONSTRUCTORS | {'->' + name for name in CONSTRUCTORS}
BODIES = {
    'core_test.cljs::test-symbol-meta': [],
    'core_test.cljs::test-reductions-obeys-reduced': [],
    'primitives_test.cljs::test-symbols-and-keywords': [],
    'seqs_test.cljs::test-seq-operations': [],
    'collections_test.cljs::test-map-operations': [],
    'collections_test.cljs::test-map': [],
    'collections_test.cljs::test-vectors': [],
    'collections_test.cljs::test-sets': [],
    'collections_test.cljs::test-cycle': [],
    'collections_test.cljs::test-repeat': [],
    'collections_test.cljs::test-iterate': [],
    'collections_test.cljs::test-lazy-seq-realized?': [],
    'collections_test.cljs::test-phm': [],
    'collections_test.cljs::test-pam': ['collections_test.cljs::array-map-conversion-threshold'],
    'collections_test.cljs::test-large-array-map': [],
    'collections_test.cljs::test-count-hash-set': [],
    'seqs_test.cljs::test-chunked': [],
    'seqs_test.cljs::test-empy-and-seq': [],
}
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
    source = '(do ' + ' '.join(forms) + ')'
    if 'LITE_MODE' in source:
        source = '(let [LITE_MODE false] ' + source + ')'
    source = source.replace('\n', ' ').replace('\t', ' ')
    identity = PREFIX + body_id.replace('/', '-').replace('.cljs::', '-').replace('_', '-').replace('?', '-p')
    rows.append([identity, source, 'true', json.dumps(provenance, separators=(',', ':'))])
    body_map[body_id] = identity
body_fixture = '# Copyright (c) Rich Hickey; EPL-1.0; retain this notice.\n# id\tsource\texpected\tcomplete source/body hashes\n' + ''.join('\t'.join(row)+'\n' for row in rows)
p = ROOT / SUPPORT[1]
if args.write: p.write_text(body_fixture)
else: assert p.read_text() == body_fixture, 'Upstream constructor-body fixture drift'
origin_path = ROOT / 'tests/reference/fixture-origins.tsv'
origin_lines = origin_path.read_text().splitlines(keepends=True)
is_body_origin = lambda line: line.startswith(SUPPORT[1] + '\t')
origin_start = next((i for i, line in enumerate(origin_lines) if is_body_origin(line)), len(origin_lines))
origins = []
for identity, source, expected, provenance in rows:
    original = bodies[json.loads(provenance)[-1]['body_id']]
    origins.append('\t'.join([SUPPORT[1], identity, 'adapted',
        'https://github.com/clojure/clojurescript', inv.REVISION,
        original['source'], original['name'],
        'Retain complete pinned body/setup, every original assertion/effect and both LITE_MODE branches; is/are become assert and testing becomes do. LITE_MODE false executes the normal configuration. Reader-discarded source remains discarded. Exact original source/setup hashes are embedded in the fixture.',
        'Collection, symbol/keyword, sequence, reduction and lazy realization assertions execute on both checked reference pins and in canonical applications/macros.']) + '\n')
origin_fixture = ''.join([*origin_lines[:origin_start], *origins,
    *(line for line in origin_lines[origin_start:] if not is_body_origin(line))])
if args.write: origin_path.write_text(origin_fixture)
else: assert origin_path.read_text() == origin_fixture, 'Constructor body origin reconciliation drift'
execution = {'test_file':TEST,'test_name':NAME,'test_source_sha256':sha((ROOT/TEST).read_bytes())}
supports = {path:sha((ROOT/path).read_bytes()) for path in SUPPORT}
rationale = ('Execute each exact positional constructor signature and generated factory using direct construction, saved/new calls and saved/apply factories in QuickJS/Node applications and canonical macros. Assert nominal/protocol identity, raw sequence/collection slots and printed symbol/keyword spelling, metadata, supplied and computed hash slots, callable lookup, ordered realization/reduction effects, phase isolation and finite expansion diagnostics. Complete upstream symbol/keyword, collection, lazy sequence, reduction and sequence operation bodies preserve all setup, loops, effects and assertions; LITE_MODE is false with both original branches retained.')
phases, mappings = [], []
for target in read('obligations.jsonl'):
    if target['name'] not in TARGETS or target['disposition'] != 'intended' or target['obligation_kind'] not in {'public-api','local-api'}: continue
    if target['obligation_kind'] == 'public-api':
        span = target['source_span']
        assert sha(span['text'].encode()) == span['sha256']
        assert full_ledger.pinned(target['source'])[span['start_byte']:span['end_byte']].decode() == span['text']
    common = {'obligation_id':target['id'],'body_sha256':target['body_sha256'],'rationale':rationale}
    if target.get('signature'): common['signature_sha256'] = sha(json.dumps(target['signature'],sort_keys=True).encode())
    phases.append({**common,**execution,'support_source_sha256':supports})
    mappings.append({**common,'local_execution':execution})
# Existing source assertion witnesses remain authoritative. The complete
# relevant upstream bodies also execute independently in the named local test.
existing = {r['assertion_id'] for f in ('witnesses.jsonl','full-witnesses.jsonl') for r in read(f) if not r['fixture_id'].startswith(PREFIX)}
witnesses = [{'assertion_id':r['id'],'assertion_sha256':r['assertion']['sha256'],'fixture_id':body_map[r['body_id']],
    'rationale':'Complete pinned body/setup; is/are become assert, testing becomes do. Every original symbol/keyword, collection, lazy realization, metadata, name/hash, reduction/early completion and sequence assertion/effect path is retained. LITE_MODE false executes the pinned normal configuration with both branches retained. Constructor-specific execution is independently bound to the exact named application/macro test.'}
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
    # Retain unrelated witnesses and replace only this named test's evidence.
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
    failures = {}
    for kind in ('release','checkout') if args.reference=='both' else [args.reference]:
        observed=report.reference_cases([*contracts,*cases],kind,ROOT/'research/clojurescript',args.artifacts/kind,180)
        (args.artifacts/(kind+'.json')).write_text(json.dumps(observed,indent=2)+'\n')
        print(kind,json.dumps({id:r['status'] for id,r in observed.items()}),flush=True)
        if len(observed) != len(contracts) + len(cases):
            failures[kind] = {'case_count': len(observed)}
        else:
            failures.update({kind + ':' + identity: result for identity, result in observed.items()
                if result['status'] != 'passed'})
    assert not failures, failures
