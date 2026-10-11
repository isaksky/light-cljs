#!/usr/bin/env python3
"""Reexecute canonical projection while preserving both original SCI conflicts."""
from pathlib import Path
import argparse, json, sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report
import sci_library_context as context
import sci_library_context_receipts as receipts

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--local-evidence', type=Path, help='Reuse exact completed original observations after a reference-only failure; all current producer hashes are validated.')
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
source = (ROOT / 'tests/fixtures/compiler-set-project.cljs').read_text()
rows = context.load()
ids = ('sci-data-set-project-meta', 'sci-data-set-project-key-representative')
originals = [next(row for row in rows if row['sci_id'] == identity) for identity in ids]
for row, name in zip(originals, ('original-relation-meta', 'original-key-representative')):
    assert '(defn ' + name + ' [] ' + row['source'] + ')' in source
prior = [json.loads(line) for line in receipts.RECEIPTS.read_text().splitlines()]
evidence = {'catalog_sha256': context.sci.digest(context.CATALOG.read_bytes()),
            'evidence': {row['id']: dict(record['layers']) for row, record in zip(rows, prior)}}
local = (json.loads(args.local_evidence.read_text()) if args.local_evidence else
         context.local_cases(rows, args.output / 'libraries', 240))
assert list(local) == [row['id'] for row in rows]
for row in rows:
    # The receipt validator checks raw output, complete source, current compiler,
    # driver, inspector and library hashes even when recovering saved results.
    receipts.classification(row, {**evidence['evidence'][row['id']], 'javascript': local[row['id']]})
(args.output / 'local.json').write_text(json.dumps(local, indent=2) + '\n')
for row in rows:
    evidence['evidence'][row['id']]['javascript'] = local[row['id']]
print('Fresh local library cases:', len(local), flush=True)
root = args.output / 'source'
(root / 'set_project').mkdir(parents=True, exist_ok=True)
(root / 'set_project/main.cljs').write_text(source + '\n(defn run [] (project-contract))\n')
identity = 'cljs-set-project-metadata'
case = {'id': identity, 'source': source, 'source_root': str(root.resolve()),
        'entry': 'set-project.main', 'export': 'run', 'native_stdout': None,
        'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}}
contract = {'source_sha256': context.sci.digest(source), 'references': {}, 'originals': {},
            'adopted_profile': 'canonical ClojureScript relation metadata and requested key representatives'}
for row, canonical_value in zip(originals, (None, {'$map': [[{'$keyword': {'namespace': None, 'name': 'new', 'spelling': 'new'}}, True]], 'count': 1})):
    contract['originals'][row['id']] = {
        'source': row['source'], 'source_sha256': row['source_sha256'], 'original_expectation': row['expected'],
        'canonical_observation': {**row['expected'], 'outcome': {'status': 'returned', 'value': canonical_value}},
        'original_expectation_status': 'unresolved'}
# Preserve each original reference namespace and its complete source context.
selected = rows[:max(rows.index(row) for row in originals) + 1]
for kind in ('release', 'checkout'):
    observations = context.reference_cases(selected, kind, args.output / (kind + '-original'), 240)
    for row in originals:
        observed = observations[row['id']]
        assert observed['observed'] == contract['originals'][row['id']]['canonical_observation'], observed
        evidence['evidence'][row['id']][kind] = observed
    contract['references'][kind] = report.reference_cases([case], kind, ROOT / 'research/clojurescript', args.output / (kind + '-contract'), 240)
    assert contract['references'][kind][identity]['status'] == 'passed', contract['references'][kind]
    invalid_root = args.output / (kind + '-invalid-source')
    (invalid_root / 'set_project').mkdir(parents=True, exist_ok=True)
    (invalid_root / 'set_project/main.cljs').write_text(source + '\n(defn run [] (project-invalid-profile))\n')
    invalid_id = 'cljs-set-project-invalid-profile'
    invalid_case = {**case, 'id': invalid_id, 'source_root': str(invalid_root.resolve()),
                    'expected': {'id': invalid_id, 'outcome': {'status': 'returned', 'value':
                                  {'$vector': ['#{}', '#{{}}', '#{{:a 1}}', '#{{}}']}}, 'trace': [], 'stdout': ''}}
    invalid = report.reference_cases([invalid_case], kind, ROOT / 'research/clojurescript', args.output / (kind + '-invalid-contract'), 240)
    assert invalid[invalid_id]['status'] == 'passed', invalid
    contract.setdefault('retained_checked_native_boundary', {})[kind] = invalid
    print(kind, 'complete originals and metadata/representative/effect/arity contract verified', flush=True)
for row in originals:
    for engine in local[row['id']]['engines'].values():
        assert engine['observed'] == contract['originals'][row['id']]['canonical_observation'], engine
(args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
(args.output / 'contract.json').write_text(json.dumps(contract, indent=2) + '\n')
# Check the saved public source/arity/phase overlay without granting gate credit.
from hashlib import sha256
fixture_source = (ROOT / 'tests/fixtures/compiler-set-project/set_project/main.cljs').read_text()
assert fixture_source == source + '\n(defn run [] (project-contract))\n'
public_case = next(row for row in report.jsonl(report.HERE / 'full-cases.jsonl') if row['id'] == identity)
assert public_case['source'] == fixture_source
mapping = next(row for row in report.jsonl(report.HERE / 'obligation-witnesses.jsonl') if row['obligation_id'] == 'api::src/main/cljs/clojure/set.cljs::project::arity-1')
phase = next(row for row in report.jsonl(report.HERE / 'phase-witnesses.jsonl') if row['obligation_id'] == mapping['obligation_id'])
assert mapping['local_execution'] == {key: phase[key] for key in ('test_file', 'test_name', 'test_source_sha256')}
assert phase['test_source_sha256'] == sha256((ROOT / phase['test_file']).read_bytes()).hexdigest()
assert all(sha256((ROOT / path).read_bytes()).hexdigest() == digest for path, digest in phase['support_source_sha256'].items())
assert mapping['reference_source_sha256'] == sha256(fixture_source.encode()).hexdigest()
assert mapping['reference_contract_sha256'] == sha256(json.dumps(public_case['expected'], sort_keys=True).encode()).hexdigest()
print('Current project source/arity/phase overlay verified', flush=True)
