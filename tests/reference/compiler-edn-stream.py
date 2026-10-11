#!/usr/bin/env python3
"""Independently check canonical public stream EOF and reader-option contracts."""
from pathlib import Path
import argparse, hashlib, json, sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
source = (ROOT / 'tests/fixtures/compiler-edn-stream-parity.cljs').read_text()
reference = source.replace('[cljs.reader :as reader-types]', '[cljs.tools.reader.reader-types :as reader-types]')
source_root = args.output / 'source'
(source_root / 'edn_stream').mkdir(parents=True, exist_ok=True)
reference_program = reference + '\n(defn run [] (stream-contract))\n'
(source_root / 'edn_stream/main.cljs').write_text(reference_program)
identity = 'cljs-edn-stream-eof-options'
case = {'id': identity, 'source': source, 'source_root': str(source_root),
        'entry': 'edn-stream.main', 'export': 'run', 'native_stdout': None,
        'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}}
evidence = {'case': case, 'references': {}, 'constructor_adaptation':
            'Local cljs.reader/string-push-back-reader; references retain cljs.tools.reader.reader-types. No mocked stream or read-string substitution.',
            'source_sha256': hashlib.sha256(source.encode()).hexdigest(),
            'reference_source_sha256': hashlib.sha256(reference_program.encode()).hexdigest(),
            'driver_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
for kind in ('release', 'checkout'):
    evidence['references'][kind] = report.reference_cases([case], kind, ROOT / 'research/clojurescript', args.output / kind, 240)
    print(kind, evidence['references'][kind][identity]['status'], flush=True)
(args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
assert all(layer[identity]['status'] == 'passed' for layer in evidence['references'].values())
