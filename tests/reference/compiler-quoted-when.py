#!/usr/bin/env python3
"""Generate complete canonical quoted-when source/body/arity evidence bindings.

Upstream Copyright (c) Rich Hickey; Eclipse Public License 1.0.
See research/clojurescript/epl-v10.html. No execution is inferred by generation.
"""
from pathlib import Path
import hashlib
import importlib.util
import json
import subprocess
import sys
ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tests/reference/compiler-language'
sys.path.insert(0, str(HERE))
import extract_inventory as inv
spec = importlib.util.spec_from_file_location('adapter', ROOT / 'tests/reference/compiler-sequence-completion.py')
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)
read = lambda name: [json.loads(line) for line in (HERE/name).read_text().splitlines() if line]
digest = lambda data: hashlib.sha256(data).hexdigest()
IDENTITY = 'src/test/cljs/cljs/macro_test.cljs::macroexpansion'
CASE = 'canonical-quoted-when-complete-body'
TEST = 'tests/compiler_quoted_when.rs'
NAME = 'quoted_when_body_shape_and_effects_execute_in_both_phases'
FIXTURE = 'tests/fixtures/compiler-quoted-when.tsv'
body = next(b for b in read('full-bodies.jsonl') if b['id'] == IDENTITY)
span = body['source_span']
pinned = subprocess.check_output(['git', '-C', str(inv.UPSTREAM), 'show', inv.REVISION + ':' + body['source']])
assert pinned[span['start_byte']:span['end_byte']].decode() == span['text']
assert digest(span['text'].encode()) == span['sha256']
original = adapter.adapt(span['text'], inv.Reader(span['text']).all()[0])
# These independent literal forms check source data rather than execute the
# expansion or derive an expected value from our own implementation.
pairs = [('(when true)', '(if true (do))'),
         ('(when false)', '(if false (do))'),
         ('(when nil :first :second)', '(if nil (do :first :second))'),
         ('(when 0 (f :one) (g :two))', '(if 0 (do (f :one) (g :two)))'),
         ('(when (condition x) (body-a x) (body-b y))', '(if (condition x) (do (body-a x) (body-b y)))'),
         ('(when-not false)', '(if false nil (do))'),
         ('(when-not true :first :second)', '(if true nil (do :first :second))'),
         ('(when-not nil (f 1) (g 2))', '(if nil nil (do (f 1) (g 2)))')]
checks = [original]
for form, expected in pairs:
    for operation in ('macroexpand-1', 'macroexpand'):
        checks.append(f"(assert (= '{expected} ({operation} '{form})))")
checks.append('''(let [effects (atom [])]
 (assert (= [nil nil nil 7 :last :other nil nil nil]
 [(when true) (when false (swap! effects conj :bad))
  (when nil (swap! effects conj :bad))
  (when (do (swap! effects conj :condition) true) (swap! effects conj :first) 7)
  (when 0 (swap! effects conj :second) (swap! effects conj :third) :last)
  (when-not false (swap! effects conj :fourth) :other)
  (when-not true (swap! effects conj :bad)) (when-not nil) (when-not false)]))
 (assert (= [:condition :first :second :third :fourth] @effects)))''')
source = '(do ' + ' '.join(checks).replace('\n',' ').replace('\t',' ') + ' true)'
expected = {'id':CASE, 'outcome':{'status':'returned', 'value':True}, 'trace':[], 'stdout':''}
case = {'id':CASE, 'source':source, 'expected':expected, 'native_stdout':None}
fixture = ('# Copyright (c) Rich Hickey. Eclipse Public License 1.0.\n'
           '# See research/clojurescript/epl-v10.html; retain this notice.\n'
           '# Complete macroexpansion body: only deftest/is harness becomes do/assert.\n'
           + CASE+'\t'+source+'\ttrue\t'+json.dumps({'body_id':IDENTITY, 'body_sha256':span['sha256'], 'setup':[]},separators=(',',':'))+'\n')
if '--write' in sys.argv: (ROOT/FIXTURE).write_text(fixture)
else: assert (ROOT/FIXTURE).read_text() == fixture
rationale = ('Complete original macroexpansion body and all four unchanged literal assertions execute with no required fixture setup; only deftest/is becomes do/assert. Independent when/when-not literal expansion forms cover empty/multiple bodies, true/false/nil/zero and uninterpreted condition/body order in both macroexpand operations. Ordinary values, ordered effects and nil results are asserted. The complete normal Node/QuickJS test also computes every assertion once in a demanded macro and verifies phase-state isolation.')
assertions = [r for r in read('full-inventory.jsonl') if r['body_id'] == IDENTITY]
witnesses = [{'assertion_id':r['id'], 'assertion_sha256':r['assertion']['sha256'], 'fixture_id':CASE, 'rationale':rationale} for r in assertions]
obligations = read('obligations.jsonl')
public_ids = ['api::src/main/clojure/cljs/core.cljc::'+n+'::arity-1' for n in ('macroexpand','when~2','when-not~2')]
local = {'test_file':TEST, 'test_name':NAME, 'test_source_sha256':digest((ROOT/TEST).read_bytes())}
support = {FIXTURE:digest((ROOT/FIXTURE).read_bytes())}
phases, mappings = [], []
for target in [r for r in obligations if r['id'] in public_ids]:
    common = {'obligation_id':target['id'], 'body_sha256':target['body_sha256'], 'rationale':rationale}
    if target.get('signature'): common['signature_sha256'] = digest(json.dumps(target['signature'],sort_keys=True).encode())
    phases.append({**common, **local, 'support_source_sha256':support})
    if target['id'] in public_ids:
        mappings.append({**common, 'local_execution':local, 'reference_fixture_id':CASE,
                         'reference_source_sha256':digest(source.encode()),
                         'reference_contract_sha256':digest(json.dumps(expected,sort_keys=True).encode())})
def update(name, owned, new):
    rows = read(name)
    old = [r for r in rows if owned(r)]
    if '--write' not in sys.argv: assert old == new, 'Owned catalog drift: '+name
    elif old != new:
        start = next((i for i,r in enumerate(rows) if owned(r)),len(rows))
        rows = rows[:start]+new+[r for r in rows[start:] if not owned(r)]
        (HERE/name).write_text(''.join(json.dumps(r,separators=(',',':'))+'\n' for r in rows))
update('full-cases.jsonl', lambda r:r['id']==CASE, [case])
update('full-witnesses.jsonl', lambda r:r.get('fixture_id')==CASE, witnesses)
update('phase-witnesses.jsonl', lambda r:r.get('test_file')==TEST, phases)
update('obligation-witnesses.jsonl', lambda r:r.get('reference_fixture_id')==CASE, mappings)
# Existing quoted-source probes retain their contracts. Correcting the old
# explicit-nil local expectation changes the complete Rust source hash only.
prior = 'tests/compiler_sci_service_probes.rs'
for name in ('phase-witnesses.jsonl','obligation-witnesses.jsonl'):
    rows = read(name)
    for row in rows:
        item = row.get('local_execution',row)
        if item.get('test_file') == prior:
            item['test_source_sha256'] = digest((ROOT/prior).read_bytes())
    if '--write' in sys.argv: (HERE/name).write_text(''.join(json.dumps(r,separators=(',',':'))+'\n' for r in rows))
    else: assert rows == read(name), 'Prior quoted-source hash drift: '+name
print('Validated quoted when complete-body, public arity and phase bindings')
