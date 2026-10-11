#!/usr/bin/env python3
"""Retain existing Object equiv source and compare sequence/name contracts on both pins."""
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
    fixture = ROOT / 'tests/fixtures/compiler_sequence_object_equiv.tsv'
    for line in fixture.read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        name, source, expected = line.split('\t')
        identity = 'sequence-object-equiv-' + name
        rows.append({'id': identity, 'source': '(pr-str ' + source + ')', 'isolated': True,
                     'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': expected},
                                  'trace': [], 'stdout': ''}})
    # Preserve the complete upstream non-lite test body and its recorded origins.
    fields = next(line for line in (ROOT / 'tests/fixtures/compiler-object-equiv-original.tsv').read_text().splitlines()
                  if line and not line.startswith('#')).split('\t')
    name, source, expected, origins = fields
    assert expected == 'true'
    assert digest(source.encode()) == '18ab06bef880d002aaa5584b75724664bc0cd0aacd0bf61894e6bcc3aee08d40'
    origin_rows = json.loads(origins)
    original = next(json.loads(line) for line in (ROOT / 'tests/reference/compiler-language/full-bodies.jsonl').read_text().splitlines()
                    if json.loads(line)['id'] == origin_rows[0]['body_id'])
    assert digest(original['source_span']['text'].encode()) == origin_rows[0]['source_sha256']
    rows.append({'id': name, 'source': source, 'complete_original_provenance': origin_rows,
                 'complete_original_body': original['source_span'], 'isolated': True,
                 'expected': {'id': name, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}})
    return rows

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('/tmp/light-cljs-151-reference'))
    args = parser.parse_args()
    selected = cases()
    evidence = {'cases': selected, 'references': {}}
    for kind in ('release', 'checkout'):
        evidence['references'][kind] = report.reference_cases(selected, kind,
            ROOT / 'research/clojurescript', args.output / kind, 240)
        print(kind, [(identity, result['status']) for identity, result in evidence['references'][kind].items()], flush=True)
    evidence['source_sha256'] = {row['id']: digest(row['source'].encode()) for row in selected}
    evidence['support_source_sha256'] = {name: digest((ROOT / name).read_bytes()) for name in (
        'src/js_runtime.js', 'tests/compiler_sequence_object_equiv.rs', 'tests/fixtures/compiler_sequence_object_equiv.tsv',
        'tests/compiler_object_equiv.rs', 'tests/fixtures/compiler-object-equiv-original.tsv')}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert all(result['status'] == 'passed' for reference in evidence['references'].values() for result in reference.values())
