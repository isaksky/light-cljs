#!/usr/bin/env python3
"""Check unchanged Atom watch key source and focused controls on both canonical pins."""
from pathlib import Path
import argparse
import hashlib
import json
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report
import sci_execution as sci

FIXTURE = ROOT / 'tests/fixtures/compiler-watch-keys.tsv'
SOURCE_HASH = '12c9352edf5e727173dbae03d2b88ac500f94b8e6b673dc121c96bfb46bbf3e6'
ROW_HASH = 'b16a81daad3dd51d213dbe579117cfe2fc58d382efa2b0d8576d5f8abf5dcadd'
DISPOSITION_HASH = '25cdc9e5af4d6e985dddff4f2448d0429f4aaea41855b6f4e5010cf57d942d86'
def digest(value): return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()

def cases():
    original_row, original = sci.rows(sci.CORPUS)['sci-state-watch-key-remove-nil']
    disposition_row, _ = sci.rows(sci.DISPOSITIONS)['sci-state-watch-key-remove-nil']
    assert digest(original[2]) == SOURCE_HASH and digest(original_row) == ROW_HASH
    assert digest(disposition_row) == DISPOSITION_HASH
    rows = [line.split('\t') for line in FIXTURE.read_text().splitlines() if line and not line.startswith('#')]
    assert original[2] in rows[0][1]
    result = [{'id': 'sci-execution-sci-state-watch-key-remove-nil', 'source': original[2],
               'expected': sci.expected(original), 'isolated': True}]
    result += [{'id': name, 'source': '(pr-str (do ' + source + '))', 'isolated': True,
                'expected': {'id': name, 'outcome': {'status': 'returned', 'value': expected}, 'trace': [], 'stdout': ''}}
               for name, source, expected in rows]
    return result

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    selected = cases()
    evidence = {'schema_version': 1, 'scope': 'public Atom watch key lookup effects; no full-language completion credit',
                'original_source_sha256': SOURCE_HASH, 'original_row_sha256': ROW_HASH,
                'historical_nonpassing_disposition_sha256': DISPOSITION_HASH, 'cases': selected, 'references': {}}
    for kind in ('release', 'checkout'):
        results = report.reference_cases(selected, kind, ROOT / 'research/clojurescript', args.output / kind, 240)
        evidence['references'][kind] = results
        print(kind, [(identity, result['status']) for identity, result in results.items()], flush=True)
    evidence['support_source_sha256'] = {name: digest((ROOT / name).read_bytes()) for name in (
        'src/js_runtime.js', 'tests/compiler_watch_keys.rs', 'tests/fixtures/compiler-watch-keys.tsv',
        'tests/reference/compiler-watch-keys.py')}
    evidence['artifact_sha256'] = {str(path.relative_to(args.output)): digest(path.read_bytes())
                                  for path in sorted(args.output.rglob('*')) if path.is_file()}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert all(result['status'] == 'passed' for layer in evidence['references'].values() for result in layer.values())
