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
PREFIX = 'core-factory-body-'
TEST = 'tests/compiler_core_factories.rs'
SUPPORT = ['tests/fixtures/compiler-core-factories.tsv', 'tests/fixtures/compiler-core-factory-bodies.tsv']
NAME = 'core_factories_execute_in_applications_and_macros'
CONSTRUCTORS = {'IndexedSeq','UUID','Range','IntegerRange','PersistentTreeMap','PersistentTreeSet','PersistentTreeMapSeq','Eduction','MultiFn','SeqIter','RangedIterator','Namespace','TaggedLiteral'}
TARGETS = CONSTRUCTORS | {'->' + name for name in CONSTRUCTORS}
BODIES = {
    'collections_test.cljs::test-range': [],
    'collections_test.cljs::test-sorted-map': [],
    'collections_test.cljs::test-sorted-sets': [],
    'core_test.cljs::test-transducers': [],
    'core_test.cljs::test-multimethods-1': ['core_test.cljs::bar', 'core_test.cljs::bar~2', 'core_test.cljs::bar~3'],
    'core_test.cljs::test-uuid': [],
    'core_test.cljs::tagged-literals': [],
    'src/test/cljs/cljs/iterator_test.cljs::coll-iter-seq-match': ['src/test/cljs/cljs/iterator_test.cljs::seq-iter-match', 'src/test/cljs/cljs/iterator_test.cljs::TestIterRec'],
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
    if body_id == 'core_test.cljs::test-multimethods-1':
        # The preceding test supplies one hierarchy relation used by this body.
        # Retain that exact setup expression/hash, without claiming the other
        # hierarchy assertions (which exercise the separate ObjMap type).
        original = bodies['core_test.cljs::test-hierarchy']
        selected = '(derive ::rect ::shape)'
        assert selected in original['source_span']['text']
        forms.insert(0, selected)
        provenance.insert(0, {'body_id': original['id'], 'source': original['source'],
            'sha256': original['source_span']['sha256'], 'selected_setup': selected,
            'selected_setup_sha256': sha(selected.encode())})
    source = '(do ' + ' '.join(forms) + ')'
    if 'LITE_MODE' in source:
        source = '(let [LITE_MODE false] ' + source + ')'
    source = source.replace('::rect', ':cljs.core-test/rect').replace('::shape', ':cljs.core-test/shape').replace('::square', ':cljs.core-test/square')
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
        'Retain complete pinned body/setup, every original assertion/effect and both LITE_MODE branches; is/are become assert and testing becomes do. LITE_MODE false executes the normal configuration. Reader-discarded source remains discarded. Exact original source/setup hashes are embedded in the fixture. Multimethods retain the complete tested body/method definitions and exact required prior hierarchy derive, with its selected setup hash; the unrelated hierarchy assertions are not claimed. Source-namespace keywords are explicitly qualified.',
        'Collection, symbol/keyword, sequence, reduction and lazy realization assertions execute on both checked reference pins and in canonical applications/macros.']) + '\n')
origin_fixture = ''.join([*origin_lines[:origin_start], *origins,
    *(line for line in origin_lines[origin_start:] if not is_body_origin(line))])
if args.write: origin_path.write_text(origin_fixture)
else: assert origin_path.read_text() == origin_fixture, 'Constructor body origin reconciliation drift'
execution = {'test_file':TEST,'test_name':NAME,'test_source_sha256':sha((ROOT/TEST).read_bytes())}
supports = {path:sha((ROOT/path).read_bytes()) for path in SUPPORT}
rationale = ('Execute thirteen exact public constructor/factory signatures through direct construction, saved/new constructors and direct/saved/apply factories in QuickJS/Node applications and canonical macro computations. Preserve supplied indexed array, iterator state, range chunk/count/hash, sorted tree/stack/count/hash, eduction transformation, multifn atom tables, Namespace and TaggedLiteral slots, protocol/nominal identity and effects. Complete range, sorted, UUID, tagged-literal, transducer, iterator and multimethod bodies retain setup, loops and effects; source-namespace keywords are explicitly qualified and LITE_MODE false retains both original branches. Phase isolation, unsupported expansion diagnostics and bounded repetition have separate local witnesses.')
phases, mappings = [], []
retained_phases = {r["obligation_id"]: r for r in read("phase-witnesses.jsonl") if r.get("test_file") != TEST}
for target in read('obligations.jsonl'):
    if target['name'] not in TARGETS or target['disposition'] != 'intended' or target['obligation_kind'] not in {'public-api','local-api'}: continue
    if target['obligation_kind'] == 'public-api':
        span = target['source_span']
        assert sha(span['text'].encode()) == span['sha256']
        assert full_ledger.pinned(target['source'])[span['start_byte']:span['end_byte']].decode() == span['text']
    common = {'obligation_id':target['id'],'body_sha256':target['body_sha256'],'rationale':rationale}
    if target.get('signature'): common['signature_sha256'] = sha(json.dumps(target['signature'],sort_keys=True).encode())
    phases.append({**common,**execution,'support_source_sha256':supports})
    retained = retained_phases.get(target['id'])
    exact_execution = {k: retained[k] for k in ('test_file','test_name','test_source_sha256')} if retained else execution
    mappings.append({**common,'local_execution':exact_execution})
# Existing source assertion witnesses remain authoritative. The complete
# relevant upstream bodies also execute independently in the named local test.
existing = {r['assertion_id'] for f in ('witnesses.jsonl','full-witnesses.jsonl') for r in read(f) if not r['fixture_id'].startswith(PREFIX)}
witnesses = [{'assertion_id':r['id'],'assertion_sha256':r['assertion']['sha256'],'fixture_id':body_map[r['body_id']],
    'rationale':'Complete pinned body/setup; is/are become assert, testing becomes do. Every original symbol/keyword, collection, lazy realization, metadata, name/hash, reduction/early completion and sequence assertion/effect path is retained. LITE_MODE false executes the pinned normal configuration with both branches retained. Constructor-specific execution is independently bound to the exact named application/macro test.'}
    for r in read('full-inventory.jsonl') if r['body_id'] in body_map and r['disposition']=='in-scope' and r['id'] not in existing]
cases = [{'id':id,'source':source,'expected':{'id':id,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''},'native_stdout':None} for id,source,_,_ in rows]
for filename, generated, key, replace in [
    ('phase-witnesses.jsonl',phases,'obligation_id',lambda r:r.get('test_file')==TEST),
    ('obligation-witnesses.jsonl',mappings,'obligation_id',lambda r:r.get('local_execution',{}).get('test_file')==TEST or r.get('rationale')==rationale),
    ('full-witnesses.jsonl',witnesses,'assertion_id',lambda r:r['fixture_id'].startswith(PREFIX)),
    ('full-cases.jsonl',[c for c in cases if c['id'] in {w['fixture_id'] for w in witnesses}],'id',lambda r:r['id'].startswith(PREFIX))]:
    prior = [r for r in read(filename) if not replace(r)]
    existing_keys = {r[key] for r in prior}
    # Retain unrelated witnesses and replace only this named test's evidence.
    generated = [r for r in generated if r[key] not in existing_keys]
    result = ''.join(json.dumps(r,separators=(',',':'))+'\n' for r in [*prior,*generated])
    path = HERE / filename
    if args.write: path.write_text(result)
    else: assert path.read_text()==result, filename+' reconciliation drift'
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
