#!/usr/bin/env python3
"""Run variadic IFn contracts and the exact original Partial body on both pins."""
from pathlib import Path
import argparse
import hashlib
import json
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report


def cases():
    rows = []
    fixture = ROOT / 'tests/fixtures/compiler_ifn_rest.tsv'
    for line in fixture.read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        name, source, expected = line.split('\t')
        identity = 'ifn-rest-' + name
        rows.append({'id': identity, 'source': '(pr-str ' + source + ')',
            'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': expected},
                         'trace': [], 'stdout': ''}})
    probe = json.loads((ROOT / 'tests/reference/compiler-residual-completion-omissions.json').read_text())[0]
    assert probe['body_id'] == 'core_test.cljs::test-cljs-2133'
    assert hashlib.sha256(probe['source'].encode()).hexdigest() == probe['source_sha256']
    identity = 'cljs-residual-completion-test-cljs-2133'
    rows.append({'id': identity, 'source': probe['source'],
        'complete_original_provenance': probe['complete_original_provenance'],
        'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': 3},
                     'trace': [], 'stdout': ''}})
    return rows


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('/tmp/light-cljs-149-reference-final'))
    args = parser.parse_args()
    selected = cases()
    evidence = {'cases': selected, 'references': {}}
    for kind in ('release', 'checkout'):
        evidence['references'][kind] = report.reference_cases(selected, kind,
            ROOT / 'research/clojurescript', args.output / kind, 240)
        print(kind, [(identity, result['status']) for identity, result in evidence['references'][kind].items()], flush=True)
    evidence['source_sha256'] = {row['id']: hashlib.sha256(row['source'].encode()).hexdigest() for row in selected}
    evidence['support_source_sha256'] = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in (
        'tests/compiler_ifn_rest.rs', 'tests/fixtures/compiler_ifn_rest.tsv',
        'tests/compiler_residual_completion.rs', 'tests/fixtures/compiler-residual-completion.tsv')}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert all(result['status'] == 'passed' for reference in evidence['references'].values() for result in reference.values())
