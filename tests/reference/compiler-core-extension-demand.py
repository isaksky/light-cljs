#!/usr/bin/env python3
"""Compare the original core-macro IEquiv extension regression on both pins."""
from pathlib import Path
import argparse
import hashlib
import json
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report

def digest(data):
    return hashlib.sha256(data).hexdigest()

def cases():
    rows = []
    fixture = ROOT / 'tests/fixtures/compiler_core_extension_demand.tsv'
    for line in fixture.read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        name, source, expected = line.split('\t')
        identity = 'core-extension-demand-' + name
        rows.append({'id': identity, 'source': '(pr-str ' + source + ')', 'isolated': True,
                     'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': expected},
                                  'trace': [], 'stdout': ''}})
    return rows

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('/tmp/light-cljs-152-reference'))
    args = parser.parse_args()
    selected = cases()
    evidence = {'cases': selected, 'references': {}}
    for kind in ('release', 'checkout'):
        evidence['references'][kind] = report.reference_cases(selected, kind,
            ROOT / 'research/clojurescript', args.output / kind, 240)
        print(kind, [(identity, result['status']) for identity, result in evidence['references'][kind].items()], flush=True)
    evidence['source_sha256'] = {row['id']: digest(row['source'].encode()) for row in selected}
    evidence['support_source_sha256'] = {name: digest((ROOT / name).read_bytes()) for name in (
        'src/compiler_bootstrap.rs', 'src/compilation_session.rs', 'tests/compiler_core_extension_demand.rs', 'tests/fixtures/compiler_core_extension_demand.tsv')}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert all(result['status'] == 'passed' for reference in evidence['references'].values() for result in reference.values())
