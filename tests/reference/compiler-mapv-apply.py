#!/usr/bin/env python3
"""Check unchanged applied mapv source and focused controls on both canonical pins."""
from pathlib import Path
import argparse
import hashlib
import json
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report
import sci_execution as sci

FIXTURE = ROOT / 'tests/fixtures/compiler-mapv-apply.tsv'
SOURCE_HASH = 'e0a907824be6de52f6110387000b5a50bdc2a73524df8dfe1edb88d55f121303'
ROW_HASH = '73a201bf60f8ee89a8037528864be6e3f22cd6083c97d1b3f8387cc90e79a4d6'
DISPOSITION_HASH = '4d74c7bf9c80509472c346ea2f383caed1f5981d3a70e418811692a4f7ce2e7b'
def digest(value): return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()

def cases():
    original_row, original = sci.rows(sci.CORPUS)['sci-seq-apply-mapv-empty']
    disposition_row, _ = sci.rows(sci.DISPOSITIONS)['sci-seq-apply-mapv-empty']
    assert digest(original[2]) == SOURCE_HASH and digest(original_row) == ROW_HASH
    assert digest(disposition_row) == DISPOSITION_HASH
    rows = [line.split('\t') for line in FIXTURE.read_text().splitlines() if line and not line.startswith('#')]
    assert rows[0][1] == original[2]
    result = [{'id': 'sci-execution-sci-seq-apply-mapv-empty', 'source': original[2],
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
    evidence = {'schema_version': 1, 'scope': 'variadic mapv shortest-input laziness; no full-language completion credit',
                'original_source_sha256': SOURCE_HASH, 'original_row_sha256': ROW_HASH,
                'historical_nonpassing_disposition_sha256': DISPOSITION_HASH, 'cases': selected, 'references': {}}
    for kind in ('release', 'checkout'):
        results = report.reference_cases(selected, kind, ROOT / 'research/clojurescript', args.output / kind, 240)
        evidence['references'][kind] = results
        print(kind, [(identity, result['status']) for identity, result in results.items()], flush=True)
    evidence['support_source_sha256'] = {name: digest((ROOT / name).read_bytes()) for name in (
        'src/js_runtime.js', 'tests/compiler_mapv_apply.rs', 'tests/fixtures/compiler-mapv-apply.tsv',
        'tests/reference/compiler-mapv-apply.py')}
    evidence['artifact_sha256'] = {str(path.relative_to(args.output)): digest(path.read_bytes())
                                  for path in sorted(args.output.rglob('*')) if path.is_file()}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert all(result['status'] == 'passed' for layer in evidence['references'].values() for result in layer.values())
