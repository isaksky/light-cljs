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
NESTED = '''[(binding [*print-level* 1] (pr-str [#js {:a 1} #js [1] {:a 1} [1] #{1}]))
    (binding [*print-level* 2] (pr-str [#js {:a #js {:b 1} :b #js [1]}]))
    (binding [*print-level* 1 *print-length* 1 *print-readably* false] (pr-str #js {:a #js {:b "x"} :b "y"}))
    (binding [*print-level* 0] (pr-str-with-opts [#js {:a 1} #js [1]] {:readably true :print-length 0}))
    (binding [*print-meta* true *print-level* 1] (pr-str (with-meta [#js {:a 1}] {:tag "v"})))]'''
RESTORATION = '''(let [settings [*print-level* *print-length* *print-readably* *print-meta*]]
    [(try (binding [*print-level* 1 *print-length* 0 *print-readably* false *print-meta* true]
        (pr-str-with-opts [#js {:a #js {:b 1}}] {:alt-impl (fn [x writer opts]
            (if (keyword? x) (throw (js/Error. "printer")) ((:fallback-impl opts) x writer opts)))}))
      (catch js/Error e (.-message e)))
     (= settings [*print-level* *print-length* *print-readably* *print-meta*])
     (pr-str #js {:a 1} #js [1])])'''

def printed_vector(values):
    return '[' + ' '.join(json.dumps(v, ensure_ascii=False) if isinstance(v, str) else str(v).lower() for v in values) + ']\n'

def cases(kind):
    negative = '0' if kind == 'release' else '-0.0'
    values = [negative, negative, negative, negative + ' ##NaN ##Inf ##-Inf', False, False]
    nested = ['[#js # # # # #]', '[#js {:a #js #, :b #}]', '#js {:a #js #, ...}',
              '#js # #', '^{:tag "v"} [#js #]']
    restored = ['printer', True, '#js {:a 1} #js [1]']
    result = []
    for name, source, value, native in [('negative-zero', NEGATIVE, {'$vector': values},
        printed_vector(values)),
        ('print-level-zero', LEVEL, '# # # # #js #', '"# # # # #js #"\n'),
        ('nested-level-boundaries', NESTED, {'$vector': nested}, printed_vector(nested)),
        ('printer-restoration', RESTORATION, {'$vector': restored}, printed_vector(restored))]:
        identity = 'cljs-surface-printing-probe-' + name
        result.append({'id': identity, 'source': ' '.join(source.splitlines()), 'expected': {'id': identity,
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
        assert row['javascript']['status'] == 'passed' and row['evaluation']['status'] == 'passed', row
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
