"""Execute the complete original mixed Number/Closure-integer predicate table.

Genuine reference-host observations only; no local/source/phase credit.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import extract_inventory as inventory
import report
from original_property_source_probe import command
ROOT = Path(__file__).resolve().parents[3]
SOURCE = 'src/test/cljs/cljs/predicates_test.cljs'

def digest(value):
    return hashlib.sha256(value if isinstance(value, bytes) else value.encode()).hexdigest()

def run(kind, output):
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    original = (inventory.UPSTREAM / SOURCE).read_text()
    forms = inventory.Reader(original).all()
    retained = {}
    for target in ('int-val-table', 'test-int-preds'):
        matches = [n for n in forms if len(n.children) > 1 and n.children[1].token == target]
        if len(matches) != 1:
            raise ValueError('Original mixed integer source unavailable')
        form = matches[0]
        text = original[form.start:form.end]
        retained[target] = {'source': text, 'sha256': digest(text)}
    source = output / 'source'
    fixture = source / 'cljs/predicates_test.cljs'
    fixture.parent.mkdir(parents=True)
    fixture.write_text(original)
    host = '''(ns original.integer-predicate-host
 (:require [cljs.test :as test] [cljs.predicates-test]))
(defn -main [& args]
 (test/set-env! (test/empty-env))
 (test/test-vars [#'cljs.predicates-test/test-int-preds])
 (let [counters (:report-counters (test/get-current-env))]
  (println "ORIGINAL INTEGER COUNTERS" (js/JSON.stringify (clj->js counters)))
  (when-not (= {:test 1 :pass 112 :fail 0 :error 0} counters)
    (set! (.-exitCode js/process) 1))))
(set! *main-cli-fn* -main)
'''
    host_path = source / 'original/integer_predicate_host.cljs'
    host_path.parent.mkdir(parents=True)
    host_path.write_text(host)
    helper = ROOT / 'tests/reference/execution.clj'
    oracle = helper.read_text().split('(try\n', 1)[0] + '''
(let [[kind identity checkout output source] *command-line-args*]
 (checked-reference kind identity checkout)
 (checked-build (java.io.File. source) output 'original.integer-predicate-host
                #{'original.integer-predicate-host 'cljs.predicates-test})
 (println "ORIGINAL INTEGER BUILD COMPLETED"))
'''
    build = output / 'build.clj'
    build.write_text(oracle)
    identity = report.RELEASE if kind == 'release' else report.REVISION
    dependency = ('{:mvn/version "' + report.RELEASE + '"}' if kind == 'release'
                  else '{:local/root ' + json.dumps(str(inventory.UPSTREAM)) + '}')
    deps = '{:paths [] :deps {org.clojure/clojurescript ' + dependency + '}}'
    built = command([os.environ.get('CLOJURE', 'clojure'), '-Srepro', '-Sdeps', deps,
                     '-M', str(build), kind, identity, '-' if kind == 'release' else str(inventory.UPSTREAM),
                     str(output), str(source)], output / 'build')
    if ('REFERENCE IDENTITY ' + kind + ' ' + identity not in built
        or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in built):
        raise ValueError('Unverified reference identity/settings')
    raw = command([os.environ.get('NODE', 'node'), '-e',
                   'process.chdir(' + json.dumps(str(output)) + ');require('
                   + json.dumps(str(output / 'reference.js')) + ');'], output / 'node')
    packets = [json.loads(line.split('ORIGINAL INTEGER COUNTERS ', 1)[1])
               for line in raw.splitlines() if line.startswith('ORIGINAL INTEGER COUNTERS ')]
    if packets != [{'test': 1, 'pass': 112, 'fail': 0, 'error': 0}]:
        raise ValueError('Unchanged mixed integer table differs')
    receipt = {'schema_version': 1, 'profile': 'whole-original-mixed-integer-reference-host-v1',
               'reference_identity': kind + ' ' + identity,
               'settings': {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False},
               'source': SOURCE, 'source_sha256': digest(original), 'original_forms': retained,
               'complete_source_unchanged': True, 'observed': packets[0],
               'host_dependency': 'Original cljs.core/LongImpl and goog.math.Integer; no Number-only table substitution',
               'artifact_root': str(output),
               'artifact_sha256': {str(p.relative_to(output)): digest(p.read_bytes())
                                   for p in sorted(output.rglob('*')) if p.is_file()},
               'producer_source_sha256': {str(p.relative_to(ROOT)): digest(p.read_bytes())
                                         for p in (Path(__file__), helper,
                                                   ROOT / 'tests/reference/compiler-language/original_property_source_probe.py')},
               'semantic_pass_credit': 0, 'macro_phase_credit': 0,
               'source_disposition_changed': False, 'strict_complete': False}
    (output / 'evidence.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(kind, packets[0])
    return receipt

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', choices=('release', 'checkout'), required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    run(args.reference, args.output)
