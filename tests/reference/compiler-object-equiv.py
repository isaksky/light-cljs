#!/usr/bin/env python3
"""Reproduce full non-lite Object equiv source and run controls on both pins.

Copyright (c) Rich Hickey. Eclipse Public License 1.0. Original upstream notices
remain in research/clojurescript/epl-v10.html; retain this notice.
"""
from pathlib import Path
import argparse
import importlib.util
import json
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report
spec = importlib.util.spec_from_file_location('completion', ROOT / 'tests/reference/compiler-residual-completion.py')
completion = importlib.util.module_from_spec(spec)
spec.loader.exec_module(completion)
residual = completion.residual
inv = completion.inv
PREFIX = 'cljs-object-equiv-'
IDENTITY = 'src/test/cljs/cljs/interop_test.cljs::test-obj-equiv'
FIXTURE = 'tests/fixtures/compiler-object-equiv-original.tsv'
TEST = 'tests/compiler_object_equiv.rs'
TEST_NAME = 'complete_non_lite_object_equiv_body_executes_in_both_phases'
RATIONALE = ('Complete original non-lite Object equiv body with all seven assertions and LITE_MODE false; '
             'only cljs.test testing/is become executing do/assert and the body returns true after all assertions. '
             'Every .equiv call, collection literal and lazy computation is preserved. Ordinary applications '
             'execute in Node/QuickJS; macros independently compute the full body in QuickJS before expansion.')


def generate(catalogs=False):
    body = next(row for row in residual.read('full-bodies.jsonl') if row['id'] == IDENTITY)
    source, origin = completion.original_form(body)
    text = body['source_span']['text']
    source = ('(do (def LITE_MODE false) ' + residual.adapt(text, inv.Reader(text).all()[0]) + ')').replace('\n', ' ').replace('\t', ' ')
    probe = json.loads((ROOT / 'tests/reference/compiler-residual-completion-omissions.json').read_text())[1]
    assert probe['body_id'] == IDENTITY and probe['source'] == source
    assert inv.digest(source.encode()) == probe['source_sha256'] == '18ab06bef880d002aaa5584b75724664bc0cd0aacd0bf61894e6bcc3aee08d40'
    assert len(body['assertions']) == 7
    case_id = PREFIX + 'original'
    fixture = '# Copyright (c) Rich Hickey. Eclipse Public License 1.0.\n# id\tsource\texpected\tcomplete original body/setup hashes\n' + '\t'.join([case_id, source, 'true', json.dumps([origin], separators=(',', ':'))]) + '\n'
    completion.persist(ROOT / FIXTURE, fixture)
    case = {'id': case_id, 'source': source, 'complete_original_provenance': [origin],
            'expected': {'id': case_id, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}, 'native_stdout': None}
    test_hash = inv.digest((ROOT / TEST).read_bytes())
    local = {'test_file': TEST, 'test_name': TEST_NAME, 'test_source_sha256': test_hash}
    body_mapping = {'body_id': IDENTITY, 'body_sha256': body['source_span']['sha256'],
                    'reference_fixture_id': case_id, 'reference_source_sha256': inv.digest(source.encode()),
                    'reference_contract_sha256': inv.digest(json.dumps(case['expected'], sort_keys=True).encode()),
                    'rationale': RATIONALE, 'local_execution': local,
                    'support_source_sha256': {FIXTURE: inv.digest(fixture.encode())}}
    # Assertion-bearing bodies have source witnesses, not invented body:: API
    # obligations. Preserve their exact body/application/macro test mapping on
    # the full source case and in the reproducible evidence proposal.
    case['complete_body_phase_mapping'] = body_mapping
    witnesses = [{'assertion_id': row['id'], 'assertion_sha256': row['assertion']['sha256'],
                  'fixture_id': case_id, 'rationale': RATIONALE}
                 for row in residual.read('full-inventory.jsonl') if row['body_id'] == IDENTITY]
    assert len(witnesses) == 7
    proposal = {'full-cases.jsonl': [case], 'full-witnesses.jsonl': witnesses,
                'complete-body-phase-mappings': [body_mapping]}
    completion.persist(ROOT / 'tests/reference/compiler-object-equiv-proposals.json', json.dumps(proposal, indent=2) + '\n')
    if catalogs:
        for name, field in [('full-cases.jsonl', 'id'), ('full-witnesses.jsonl', 'fixture_id')]:
            residual.update_owned(name, lambda row: row.get(field, '').startswith(PREFIX), proposal[name])
    return case


def cases(original):
    result = [original]
    for line in (ROOT / 'tests/fixtures/compiler_object_equiv.tsv').read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        name, source, expected = line.split('\t')
        identity = PREFIX + name
        result.append({'id': identity, 'source': '(pr-str ' + source + ')',
                       'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': expected}, 'trace': [], 'stdout': ''}})
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true')
    parser.add_argument('--catalogs', action='store_true')
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--output', type=Path, default=Path('/tmp/light-cljs-150-reference-current'))
    args = parser.parse_args()
    original = generate(args.catalogs)
    if args.run:
        selected = cases(original)
        evidence = {'cases': selected, 'references': {}}
        for kind in ('release', 'checkout'):
            evidence['references'][kind] = report.reference_cases(selected, kind,
                ROOT / 'research/clojurescript', args.output / kind, 240)
            print(kind, [(identity, result['status']) for identity, result in evidence['references'][kind].items()], flush=True)
        evidence['source_sha256'] = {row['id']: inv.digest(row['source'].encode()) for row in selected}
        evidence['support_source_sha256'] = {name: inv.digest((ROOT / name).read_bytes()) for name in (TEST, FIXTURE, 'tests/fixtures/compiler_object_equiv.tsv')}
        args.output.mkdir(parents=True, exist_ok=True)
        (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
        assert all(result['status'] == 'passed' for reference in evidence['references'].values() for result in reference.values())
    print('1 complete non-lite body, 7 original assertion mappings; supplemental execution required')
