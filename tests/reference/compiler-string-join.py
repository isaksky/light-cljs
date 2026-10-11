#!/usr/bin/env python3
"""Reexecute canonical join literals while preserving the original SCI conflict."""
from pathlib import Path
import argparse, hashlib, json, sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report
import sci_library_context as context
import sci_library_context_receipts as receipts

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
source = (ROOT / 'tests/fixtures/compiler-string-join.cljs').read_text()
rows = context.load()
original = next(row for row in rows if row['sci_id'] == 'sci-string-join-values')
assert '(defn original-five [] ' + original['source'] + ')' in source
prior = [json.loads(line) for line in receipts.RECEIPTS.read_text().splitlines()]
evidence = {'catalog_sha256': context.sci.digest(context.CATALOG.read_bytes()),
            'evidence': {row['id']: dict(record['layers']) for row, record in zip(rows, prior)}}
local = context.local_cases(rows, args.output / 'libraries', 240)
for row in rows:
    evidence['evidence'][row['id']]['javascript'] = local[row['id']]
print('Fresh local library cases:', len(local), flush=True)
root = args.output / 'source'
(root / 'string_join').mkdir(parents=True, exist_ok=True)
(root / 'string_join/main.cljs').write_text(source + '\n(defn run [] (join-contract))\n')
identity = 'cljs-string-join-conversion'
case = {'id': identity, 'source': source, 'source_root': str(root.resolve()),
        'entry': 'string-join.main', 'export': 'run', 'native_stdout': None,
        'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}}
contract = {'source_sha256': context.sci.digest(source), 'references': {},
            'original_source': original['source'], 'original_source_sha256': original['source_sha256'],
            'original_expectation': original['expected'], 'adopted_profile': 'canonical ClojureScript separator conversion',
            'canonical_observation': {**original['expected'], 'outcome': {'status': 'returned', 'value':
                {'$vector': ['falsexa', ',1,:x', '1null2', '1-2', '1:x2']}}},
            'original_expectation_status': 'unresolved',
            'conflict': 'Original SCI expects 12 for nil separator; both required pins and local application/macro phases return 1null2. Canonical library/public-arity evidence does not grant original SCI agreement.'}
# Keep case19's original reference namespace; unrelated reference receipts stay exact.
selected = rows[:rows.index(original) + 1]
for kind in ('release', 'checkout'):
    observations = context.reference_cases(selected, kind, args.output / (kind + '-original'), 240)
    observed = observations[original['id']]
    assert observed['observed'] == contract['canonical_observation'], observed
    evidence['evidence'][original['id']][kind] = observed
    contract['references'][kind] = report.reference_cases([case], kind, ROOT / 'research/clojurescript', args.output / (kind + '-contract'), 240)
    assert contract['references'][kind][identity]['status'] == 'passed', contract['references'][kind]
    print(kind, 'complete original and conversion/effect/arity contract verified', flush=True)
for engine in local[original['id']]['engines'].values():
    assert engine['observed'] == contract['canonical_observation'], engine
(args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
(args.output / 'contract.json').write_text(json.dumps(contract, indent=2) + '\n')
