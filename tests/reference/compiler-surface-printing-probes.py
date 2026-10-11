#!/usr/bin/env python3
"""Preserve exact printing comparisons without turning differences into passes."""
import argparse
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report
NEGATIVE = '(let [n -0.0 from-js (js/Number "-0")] [(pr-str -0.0) (pr-str n) (pr-str from-js) (pr-str -0.0 js/NaN js/Infinity (- js/Infinity)) (.is js/Object n 0) (.is js/Object from-js 0)])'
LEVEL = '(binding [*print-level* 0] (pr-str [1] {:a 1} #{1} #js [1] #js {:a 1}))'

def cases(kind):
    negative = '0' if kind == 'release' else '-0.0'
    values = [negative, negative, negative, negative + ' ##NaN ##Inf ##-Inf', False, False]
    result = []
    for name, source, value, native in [('negative-zero', NEGATIVE, {'$vector': values},
        '[' + ' '.join(json.dumps(v) if isinstance(v, str) else 'false' for v in values) + ']\n'),
        ('print-level-zero', LEVEL, '# # # # #js #', '"# # # # #js #"\n')]:
        identity = 'cljs-surface-printing-probe-' + name
        result.append({'id': identity, 'source': source, 'expected': {'id': identity,
            'outcome': {'status': 'returned', 'value': value}, 'trace': [], 'stdout': ''}, 'native_stdout': native})
    return result

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--artifacts', type=Path, required=True)
    p.add_argument('--binary', type=Path, default=ROOT / 'target/debug/light-cljs')
    a = p.parse_args(); a.artifacts.mkdir(parents=True, exist_ok=False)
    local = {c['id']: report.local_layers(c, str(a.binary.resolve()), a.artifacts / 'local' / c['id'], 120) for c in cases('checkout')}
    (a.artifacts / 'local.json').write_text(json.dumps(local, indent=2) + '\n')
    for key, row in local.items():
        wanted = 'passed' if key.endswith('negative-zero') else 'mismatch'
        assert row['javascript']['status'] == wanted and row['evaluation']['status'] == wanted, row
    print('local', json.dumps({key: {layer: row[layer]['status'] for layer in ('javascript', 'evaluation')} for key, row in local.items()}), flush=True)
    for kind in ('release', 'checkout'):
        selected = cases(kind)
        (a.artifacts / (kind + '-cases.json')).write_text(json.dumps(selected, indent=2) + '\n')
        observed = report.reference_cases(selected, kind, ROOT / 'research/clojurescript', a.artifacts / kind, 300)
        (a.artifacts / (kind + '.json')).write_text(json.dumps(observed, indent=2) + '\n')
        print(kind, json.dumps(observed), flush=True)
        assert all(row['status'] == 'passed' for row in observed.values()), observed

if __name__ == '__main__':
    main()
