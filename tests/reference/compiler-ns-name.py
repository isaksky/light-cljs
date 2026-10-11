#!/usr/bin/env python3
"""Run unchanged namespace reflection probes and retain complete pinned bodies."""
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
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
sha = lambda data: hashlib.sha256(data).hexdigest()
contract = (ROOT / 'tests/fixtures/compiler-ns-name.cljs').read_text()
source = '(do ' + ' '.join(line for line in contract.splitlines() if not line.lstrip().startswith(';')) + ' (ns-name-contract))'
case = {'id': 'ns-name-result-types', 'source': source, 'isolated': True,
        'expected': {'id': 'ns-name-result-types', 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}}
receipt = {'owner': 'light-cljs-143', 'cases': [case], 'unchanged_source_sha256': sha(source.encode()), 'references': {}, 'complete_upstream_bodies': {},
           'local_witnesses': {p: sha((ROOT/p).read_bytes()) for p in ['tests/compiler_ns_name.rs', 'tests/fixtures/compiler-ns-name.cljs']}}
for kind in ['release', 'checkout']:
    if kind == 'release':
        jar = Path.home() / '.m2/repository/org/clojure/clojurescript/1.12.134/clojurescript-1.12.134.jar'
        with zipfile.ZipFile(jar) as z:
            data = z.read('cljs/core.cljs')
        origin = 'Maven org.clojure/clojurescript 1.12.134 cljs/core.cljs'
    else:
        data = (ROOT/'research/clojurescript/src/main/cljs/cljs/core.cljs').read_bytes()
        origin = inv.REVISION + ' src/main/cljs/cljs/core.cljs'
    text = data.decode()
    bodies = []
    for form in inv.Reader(text).all():
        if form.kind == 'list' and len(form.children) > 1 and form.children[1].token in {'Namespace','create-ns','find-ns','find-ns-obj','find-ns-obj*','ns-lookup','ns-name'}:
            body = text[form.start:form.end]
            bodies.append({'name': form.children[1].token, 'source': origin, 'text': body, 'sha256': sha(body.encode()), 'start_line': text.count('\n',0,form.start)+1})
    assert {b['name'] for b in bodies} == {'Namespace','create-ns','find-ns','find-ns-obj','find-ns-obj*','ns-lookup','ns-name'}
    receipt['complete_upstream_bodies'][kind] = bodies
    result = report.reference_cases([case], kind, ROOT/'research/clojurescript', args.output/kind, 180)
    receipt['references'][kind] = result
    fixture = args.output/kind/'src/semantic/case0.cljs'
    receipt.setdefault('fixture_sha256', {})[kind] = sha(fixture.read_bytes())
    assert result[case['id']]['status'] == 'passed', result
args.output.mkdir(parents=True,exist_ok=True)
(args.output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('Unchanged constructor/create/find/metadata ns-name contract passed both pins')
