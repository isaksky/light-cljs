#!/usr/bin/env python3
"""Compare namespace collisions and ordered ordinary redefinition on both pins."""
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

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('/tmp/light-cljs-154-shadow-contract-reference'))
    args = parser.parse_args()
    source_root = 'tests/fixtures/compiler-namespace-shadowing'
    source = (ROOT / source_root / 'shadow/main.cljs').read_text()
    identity = 'namespace-shadowing-ordered-redefinition'
    selected = [{'id': identity, 'source': source, 'source_root': source_root,
                 'entry': 'shadow.main', 'export': 'run',
                 'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': '[3 7 1 7 7 17 [9 11 13 70 7 19 70] 90 [:before :after]]'}, 'trace': [], 'stdout': ''}}]
    evidence = {'cases': selected, 'references': {}}
    for kind in ('release', 'checkout'):
        evidence['references'][kind] = report.reference_cases(selected, kind, ROOT / 'research/clojurescript', args.output / kind, 240)
        print(kind, [(name, value['status']) for name, value in evidence['references'][kind].items()], flush=True)
    paths = [str(path.relative_to(ROOT)) for path in sorted((ROOT / source_root).rglob('*.cljs'))]
    evidence['support_source_sha256'] = {name: digest((ROOT / name).read_bytes()) for name in paths + ['src/analyzer.rs', 'tests/compiler_namespace_shadowing.rs', 'tests/reference/compiler-namespace-shadowing.py']}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert all(value['status'] == 'passed' for reference in evidence['references'].values() for value in reference.values())
