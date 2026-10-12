#!/usr/bin/env python3
"""Check public find effects on independently verified canonical pins."""
from pathlib import Path
import argparse
import hashlib
import json
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report

FIXTURE = ROOT / 'tests/fixtures/compiler-find-effects.tsv'
def digest(value): return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()
def cases():
    gap = ROOT / 'tests/fixtures/compiler-find-key-effects-gap.tsv'
    assert digest(gap.read_bytes()) == '30734357a5779b0b39364e1842413ba9ced86e328c3daffe2db02e86c07f61b8'
    rows = [line.split('\t') for line in FIXTURE.read_text().splitlines() if line and not line.startswith('#')]
    assert rows[0] == next(line.split('\t') for line in gap.read_text().splitlines() if line and not line.startswith('#'))
    assert all(len(row) == 3 and row[2] != 'PENDING' for row in rows)
    return [{'id': name, 'source': '(pr-str (do ' + source + '))', 'isolated': True,
             'expected': {'id': name, 'outcome': {'status': 'returned', 'value': expected}, 'trace': [], 'stdout': ''}}
            for name, source, expected in rows]

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    selected = cases()
    evidence = {'schema_version': 1, 'scope': 'public find single-search effects; no full-language completion credit',
                'historical_gap_sha256': digest((ROOT / 'tests/fixtures/compiler-find-key-effects-gap.tsv').read_bytes()), 'cases': selected, 'references': {}}
    for kind in ('release', 'checkout'):
        results = report.reference_cases(selected, kind, ROOT / 'research/clojurescript', args.output / kind, 240)
        evidence['references'][kind] = results
        print(kind, [(identity, result['status']) for identity, result in results.items()], flush=True)
    evidence['support_source_sha256'] = {name: digest((ROOT / name).read_bytes()) for name in (
        'src/js_runtime.js', 'tests/compiler_find_effects.rs', 'tests/fixtures/compiler-find-effects.tsv',
        'tests/reference/compiler-find-effects.py')}
    evidence['artifact_sha256'] = {str(path.relative_to(args.output)): digest(path.read_bytes())
                                  for path in sorted(args.output.rglob('*')) if path.is_file()}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert all(result['status'] == 'passed' for layer in evidence['references'].values() for result in layer.values())
