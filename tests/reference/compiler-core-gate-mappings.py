#!/usr/bin/env python3
"""Reproduce reviewed core operation mappings and their literal pinned oracles.

Catalog entries establish identity, never execution. The full report executes
both checked pins for unchanged literal semantics and exact phase tests.
"""
from pathlib import Path
import argparse
import importlib.util
import json
import sys
ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tests/reference/compiler-language'
spec = importlib.util.spec_from_file_location('core_gate', ROOT / 'tests/reference/compiler-core-gate.py')
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)
PREFIX = 'cljs-core-gate-literal-'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--write', action='store_true', help='coordinate replacement of owned rows in shared tables')
parser.add_argument('--write-owned', action='store_true', help='write only core-gate-call-receipts.jsonl')
parser.add_argument('--export-owned', type=Path, help='export owned mapping/case rows for coordinated integration')
args = parser.parse_args()
if args.write and args.write_owned: parser.error('choose --write or --write-owned')
if args.export_owned: args.export_owned.mkdir(parents=True, exist_ok=True)
ordinary, phases, receipts = gate.execution_mappings(include_existing=True)
by_receipt = {r['obligation_id']: r for r in receipts}
cases = []
for group, identity, source, expected, provenance in gate.fixture_rows():
    if not identity.startswith('literal::'): continue
    fixture_id = PREFIX + identity.removeprefix('literal::')
    # Preserve the whole original workflow and independently adjacent literal
    # expectation. Runtime equality permits map printing order, never new data.
    expression = "(= '" + expected + ' (do ' + source + '))'
    case = {'id': fixture_id, 'source': expression, 'expected': {'id': fixture_id,
            'outcome': {'status':'returned','value':True}, 'trace': [], 'stdout': ''}, 'native_stdout': 'true\n'}
    cases.append(case)
    for mapping in ordinary:
        if mapping.get('local_execution') and by_receipt[mapping['obligation_id']]['source_body'] == identity:
            mapping.update(reference_fixture_id=fixture_id, reference_source_sha256=gate.digest(expression),
                           reference_contract_sha256=gate.digest(json.dumps(case['expected'],sort_keys=True)))
# A literal workflow can exercise only arities already bound to an earlier
# complete oracle. Keep it in the executable core fixture, but do not add an
# unreferenced report fixture that has no independent obligation mapping.
linked_oracles = {row.get('reference_fixture_id') for row in ordinary}
cases = [case for case in cases if case['id'] in linked_oracles]
for filename, generated, key in [('obligation-witnesses.jsonl',ordinary,'obligation_id'),('phase-witnesses.jsonl',phases,'obligation_id'),
                                 ('full-cases.jsonl',cases,'id')]:
    old=gate.read(filename)
    def owned(row):
        if filename=='full-cases.jsonl':return row['id'].startswith(PREFIX)
        if filename=='phase-witnesses.jsonl':return row.get('test_file')==gate.TEST_FILE
        return row.get('local_execution',{}).get('test_file')==gate.TEST_FILE or row['obligation_id'] in {r['obligation_id'] for r in gate.read('phase-witnesses.jsonl') if r.get('test_file')==gate.TEST_FILE}
    seen={r[key] for r in old if not owned(r)}
    replacements={r[key]:r for r in generated if r[key] not in seen}
    result=[]
    for row in old:
        if not owned(row):result.append(row)
        elif row[key] in replacements:result.append(replacements.pop(row[key]))
    result.extend(replacements.values())
    text=''.join(json.dumps(r,separators=(',',':'))+'\n'for r in result)
    path=HERE/filename
    if args.write:path.write_text(text)
    elif args.export_owned:
        (args.export_owned/filename).write_text(''.join(json.dumps(r,separators=(',',':'))+'\n' for r in result if owned(r)))
    elif not args.write_owned:assert path.read_text()==text, filename+' reviewed core mappings drift'
text=''.join(json.dumps(r,separators=(',',':'))+'\n'for r in receipts)
path=HERE/'core-gate-call-receipts.jsonl'
if args.write or args.write_owned:path.write_text(text)
elif not args.export_owned:assert path.read_text()==text
if args.export_owned:(args.export_owned/'core-gate-call-receipts.jsonl').write_text(text)
print(f'{len(receipts)} reviewed operation receipts and {len(cases)} literal pinned oracles reproduce')
