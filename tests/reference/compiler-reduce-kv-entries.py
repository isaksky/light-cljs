#!/usr/bin/env python3
"""Check unchanged reduce-kv entry source and focused controls on both canonical pins."""
from pathlib import Path
import argparse
import hashlib
import json
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report
import sci_execution as sci

FIXTURE = ROOT / 'tests/fixtures/compiler-reduce-kv-entries.tsv'
SOURCE_HASH = 'dc98b9aef1ab00352258d9247014772b60c45f9cb240485313a0bffc18c84f37'
ROW_HASH = '1b978471ffb1985b01ba88000ddb83bfb9b70e7db25e1e4d3300a94e1d9710d0'
DISPOSITION_HASH = 'ce8f84a70f402dba4c537fdd7d88dbbb25c6518a51a4745e53144cc00ed28532'
def digest(value): return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()

def cases():
    original_row, original = sci.rows(sci.CORPUS)['sci-seq-reduce-kv-nested-entry']
    disposition_row, _ = sci.rows(sci.DISPOSITIONS)['sci-seq-reduce-kv-nested-entry']
    assert digest(original[2]) == SOURCE_HASH and digest(original_row) == ROW_HASH
    assert digest(disposition_row) == DISPOSITION_HASH
    rows = [line.split('\t') for line in FIXTURE.read_text().splitlines() if line and not line.startswith('#')]
    assert rows[0][1] == original[2]
    result = [{'id': 'sci-execution-sci-seq-reduce-kv-nested-entry', 'source': original[2],
               'expected': sci.expected(original), 'isolated': True}]
    result += [{'id': name, 'source': '(pr-str (do ' + source + '))', 'isolated': True,
                'expected': {'id': name, 'outcome': {'status': 'returned', 'value': expected}, 'trace': [], 'stdout': ''}}
               for name, source, expected in rows]
    # Canonical CLJS reports a missing-protocol Error here, even though the old
    # SCI row categorizes its historical failure as execution/type. Keep both.
    _, plain = sci.rows(sci.CORPUS)['sci-seq-reduce-kv-entry-itself']
    result.append({'id': 'nonnested-original', 'source': plain[2], 'isolated': True,
                   'expected': {'id': 'nonnested-original', 'outcome': {'status': 'threw', 'error': {'category': 'error'}}, 'trace': [], 'stdout': ''}})
    return result

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    selected = cases()
    evidence = {'schema_version': 1, 'scope': 'reduce-kv MapEntry fallback; no full-language completion credit',
                'original_source_sha256': SOURCE_HASH, 'original_row_sha256': ROW_HASH,
                'historical_nonpassing_disposition_sha256': DISPOSITION_HASH, 'cases': selected, 'references': {}}
    for kind in ('release', 'checkout'):
        results = report.reference_cases(selected, kind, ROOT / 'research/clojurescript', args.output / kind, 240)
        evidence['references'][kind] = results
        print(kind, [(identity, result['status']) for identity, result in results.items()], flush=True)
    evidence['support_source_sha256'] = {name: digest((ROOT / name).read_bytes()) for name in (
        'src/js_runtime.js', 'tests/compiler_reduce_kv_entries.rs', 'tests/fixtures/compiler-reduce-kv-entries.tsv',
        'tests/reference/compiler-reduce-kv-entries.py')}
    evidence['artifact_sha256'] = {str(path.relative_to(args.output)): digest(path.read_bytes())
                                  for path in sorted(args.output.rglob('*')) if path.is_file()}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert all(result['status'] == 'passed' for layer in evidence['references'].values() for result in layer.values())
