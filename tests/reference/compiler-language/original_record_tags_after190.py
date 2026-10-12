"""Diagnose the unchanged original CLJC record tags with their genuine JVM companion.

Reference-host evidence only; no local or actual macro completion credit.
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
SOURCE = 'src/test/cljs/data_readers_test/records.cljc'


def digest(value):
    return hashlib.sha256(value if isinstance(value, bytes) else value.encode()).hexdigest()


def run(kind, output):
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    original = (inventory.UPSTREAM / SOURCE).read_text()
    source = output / 'source'
    fixture = source / 'data_readers_test/records.cljc'
    fixture.parent.mkdir(parents=True)
    fixture.write_text(original)
    forms = inventory.Reader(original).all()
    assertions = [original[n.start:n.end] for n in forms if inventory.head(n) == 'assert']
    if len(assertions) != 5:
        raise ValueError('Missing original record-tag assertions')
    host = '''(ns original.record-tags-host (:require [data-readers-test.records]))
(defn -main [& args] (println "ORIGINAL RECORD TAGS PASSED"))
(set! *main-cli-fn* -main)
'''
    host_path = source / 'original/record_tags_host.cljs'
    host_path.parent.mkdir(parents=True)
    host_path.write_text(host)
    helper = ROOT / 'tests/reference/execution.clj'
    oracle = helper.read_text().split('(try\n', 1)[0] + '''
(try
 (let [[kind identity checkout output source] *command-line-args*]
  (checked-reference kind identity checkout)
  (assert *assert* "Original JVM companion assertions must be enabled")
  ;; Load the complete original :clj source, including its five assertions,
  ;; before compiling its unchanged :cljs source. This is the original JVM
  ;; companion class, not an invented tag reader or constructor substitute.
  (require 'data-readers-test.records)
  (assert (= "data_readers_test.records.Foo" (.getName (class ((resolve 'data-readers-test.records/->Foo) 1 2)))))
  (println "ORIGINAL JVM COMPANION PASSED")
  (checked-build (java.io.File. source) output 'original.record-tags-host
                 #{'original.record-tags-host 'data-readers-test.records})
  (checked-reference kind identity checkout)
  (println "ORIGINAL RECORD TAG BUILD COMPLETED"))
 (finally (shutdown-agents)))
'''
    build = output / 'build.clj'
    build.write_text(oracle)
    identity = report.RELEASE if kind == 'release' else report.REVISION
    dependency = ('{:mvn/version "' + report.RELEASE + '"}' if kind == 'release'
                  else '{:local/root ' + json.dumps(str(inventory.UPSTREAM)) + '}')
    deps = '{:paths [' + json.dumps(str(source)) + '] :deps {org.clojure/clojurescript ' + dependency + '}}'
    built = command([os.environ.get('CLOJURE', 'clojure'), '-Srepro', '-Sdeps', deps,
                     '-M', str(build), kind, identity, '-' if kind == 'release' else str(inventory.UPSTREAM),
                     str(output), str(source)], output / 'build')
    for marker in ('REFERENCE IDENTITY ' + kind + ' ' + identity,
                   'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}',
                   'ORIGINAL JVM COMPANION PASSED', 'ORIGINAL RECORD TAG BUILD COMPLETED'):
        if marker not in built:
            raise ValueError('Missing checked original record context: ' + marker)
    emitted_assertions = (output / 'out/data_readers_test/records.js').read_text().count('Assert failed:')
    if emitted_assertions != 5:
        raise ValueError('Original five runtime assertions were elided or changed')
    raw = command([os.environ.get('NODE', 'node'), '-e',
                   'process.chdir(' + json.dumps(str(output)) + ');require('
                   + json.dumps(str(output / 'reference.js')) + ');'], output / 'node')
    if raw.splitlines().count('ORIGINAL RECORD TAGS PASSED') != 1:
        raise ValueError('Original complete record namespace did not execute')
    if fixture.read_text() != original:
        raise ValueError('Original record source changed during compilation')
    receipt = {'schema_version': 1, 'profile': 'whole-original-record-tags-reference-host-v1',
               'reference_identity': kind + ' ' + identity,
               'source': SOURCE, 'source_sha256': digest(original),
               'complete_source_unchanged': True,
               'original_runtime_assertions_emitted': emitted_assertions,
               'assertions': [{'source': text, 'sha256': digest(text)} for text in assertions],
               'jvm_companion': 'Complete unchanged original :clj source and actual data_readers_test.records.Foo class',
               'settings': {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False},
               'observed': 'All five unchanged top-level record-tag assertions executed without error',
               'artifact_root': str(output),
               'artifact_sha256': {str(p.relative_to(output)): digest(p.read_bytes())
                                   for p in sorted(output.rglob('*')) if p.is_file()},
               'producer_source_sha256': {str(p.relative_to(ROOT)): digest(p.read_bytes())
                                         for p in (Path(__file__), helper,
                                                   ROOT / 'tests/reference/compiler-language/original_property_source_probe.py')},
               'semantic_pass_credit': 0, 'macro_phase_credit': 0,
               'source_disposition_changed': False, 'strict_complete': False}
    (output / 'evidence.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(kind, receipt['observed'])
    return receipt


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', choices=('release', 'checkout'), required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    run(args.reference, args.output)
