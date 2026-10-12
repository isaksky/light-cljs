#!/usr/bin/env python3
"""Execute immutable original, actual compiler phases, and checked pinned oracles.

This focused receipt is evidence for one original source, never a whole-gate pass.
All raw observations and source/build/binary provenance remain in the output root.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import configured_source_context as runner
import report
import sci_execution as sci
import sci_source_context as context

ROOT = sci.ROOT
DRIVER = ROOT / 'tests/compiler_sci_record_duplicate_method_probe.rs'
TEST = 'original_duplicate_method_executes_with_last_dispatch_in_both_phases'
ORIGINAL = '(defprotocol P (p [x])) (defrecord R [x] P (p [_] x) (p [_] 9)) (p (->R 3))'
ORIGINAL_HASH = 'a665b0da5f38af34aa5198dbe3e476edb3703dc2fdb1bc98bc8e6feb005965a6'
ROW_HASH = 'afbfd88e0edf6735ff6783f2a4550ba9b90eeeed2d1b87993ed36cc3f005aaf9'
CONTROLS = {
    'fields-arities-effects': ('(def events (atom [])) (defprotocol P (p [x] [x n]) (q [x])) (defrecord R [v] P (p [_] (swap! events conj :discarded) 0) (p [_ n] (+ v n)) (q [_] (* v 2)) (p [_] (swap! events conj v) v)) (def old (->R 3)) (def current (->R 7)) [(p old) (p current) (p old 4) (q current) @events]', {'$vector': [3, 7, 7, 14, {'$vector': [3, 7]}]}),
    'deftype': ('(defprotocol P (p [x])) (deftype R [x] P (p [_] x) (p [_] 9)) (p (->R 3))', 9),
    'reify': ('(defprotocol P (p [x])) (p (reify P (p [_] 3) (p [_] 9)))', 9),
    'generations': ('(defprotocol P (p [x])) (defrecord R [x] P (p [_] 0) (p [_] x)) (def old (->R 3)) (def factory ->R) (defrecord R [x] P (p [_] 0) (p [_] (+ 10 x))) (def current (->R 7)) [(p old) (p current) (p (factory 5)) (p (assoc old :x 4))]', {'$vector': [3, 17, 15, 14]}),
}
CASES = {'original': (ORIGINAL, 9), **CONTROLS}


def digest(value):
    return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()


def expected(identity, value):
    return {'id': identity, 'outcome': {'status': 'returned', 'value': value}, 'trace': [], 'stdout': ''}


def validate_original():
    raw, fields = sci.rows(sci.CORPUS)['sci-record-method-duplicate-method']
    if digest(raw) != ROW_HASH or fields[2] != ORIGINAL or digest(ORIGINAL) != ORIGINAL_HASH:
        raise ValueError('Immutable original source/row drift')
    if not report.same_json(sci.expected(fields), expected('sci-execution-sci-record-method-duplicate-method', 9)):
        raise ValueError('Immutable original SCI expectation drift')


def module(source, phase):
    from extract_inventory import Reader
    last = Reader(source).all()[-1]
    setup, body = source[:last.start], source[last.start:last.end]
    if setup + body != source: raise ValueError('Incomplete source')
    if phase == 'application': return f'(ns user)\n{setup}(defn run [] {body})\n'
    if phase == 'top-level-macro': return f"(ns user)\n{setup}(defmacro upstream [] (list 'quote {body}))\n(defn run [] (upstream))\n"
    if phase == 'complete-macro': return f"(ns user)\n(defmacro upstream [] (list 'quote (do {source})))\n(defn run [] (upstream))\n"
    raise ValueError('Unknown phase')


def snapshot():
    paths = [Path(__file__).resolve(), DRIVER, sci.CORPUS, sci.DISPOSITIONS,
             ROOT / 'tests/support/sci_execution_observation.js', ROOT / 'tests/support/execution.js',
             ROOT / 'tests/reference/execution.clj', Path(report.__file__), Path(runner.__file__),
             Path(sci.__file__), Path(context.__file__)]
    return {**context.compiler_sources(), **{str(p.relative_to(ROOT)): digest(p.read_bytes()) for p in paths}}


def local_frames(root):
    frames = {}
    if (root / 'original.cljs').read_text() != ORIGINAL: raise ValueError('Original source drift')
    if not (root / 'warmup.js').read_bytes(): raise ValueError('Missing reused-host warmup')
    directories = [p for p in root.iterdir() if p.is_dir()]
    # The split redefinition setup is independently nonpassing phase evidence.
    # Its complete sequential source/application still verifies generation roots.
    if len(directories) != 28: raise ValueError('Incomplete actual-phase observations')
    for name, (source, value) in CASES.items():
        for lifecycle in ('fresh', 'reused'):
            for phase in ('application', 'top-level-macro', 'complete-macro'):
                if name == 'generations' and phase == 'top-level-macro': continue
                identity = f'{name}-{phase}-{lifecycle}'
                directory = root / identity
                if (directory / 'user.cljs').read_text() != module(source, phase): raise ValueError('Changed original setup/body/phase')
                javascript = (directory / 'compiled.js').read_text()
                if javascript.count(' = // Original bounded runtime') != 1: raise ValueError('Missing compiled shared runtime')
                quickjs = (directory / 'quickjs.json').read_text()
                node = (directory / 'node.stdout').read_text()
                if json.loads(quickjs) != expected('duplicate-method', value) or node != quickjs + '\n' or (directory / 'node.stderr').read_bytes():
                    raise ValueError('Missing/mismatching/extra actual engine observation')
                if 'protocol method arity mismatch' not in (directory / 'error.txt').read_text(): raise ValueError('Missing malformed-method recovery control')
                frames[identity] = {'observed': json.loads(quickjs), 'phase_source_sha256': digest(module(source, phase)), 'compiled_js_sha256': digest(javascript), 'quickjs_raw_sha256': digest(quickjs), 'node_raw_sha256': digest(node)}
    return frames


def run(directory, timeout):
    validate_original()
    directory = Path(directory).resolve(); directory.mkdir(parents=True, exist_ok=False)
    before = snapshot()
    build, _ = runner.run(['cargo', 'test', '--locked', '--test', DRIVER.stem, '--no-run', '--message-format=json'], directory / 'build', timeout)
    binaries = [Path(row['executable']) for row in (json.loads(line) for line in build.splitlines() if line.startswith('{')) if row.get('reason') == 'compiler-artifact' and row.get('target', {}).get('name') == DRIVER.stem and row.get('executable')]
    if len(binaries) != 1: raise ValueError('Missing actual audit binary')
    binary = binaries[0]; binary_hash = digest(binary.read_bytes())
    node = Path(shutil.which(os.environ.get('NODE', 'node'))).resolve(); node_hash = digest(node.read_bytes())
    stdout, _ = runner.run([str(binary), TEST, '--exact', '--nocapture'], directory / 'audit', timeout, environment={**os.environ, 'LIGHT_SCI_RECORD_DUPLICATE_METHOD_ROOT': str(directory / 'local')})
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in stdout: raise ValueError('Actual phase audit not executed')
    document = {'schema_version': 1, 'profile': 'original-sci-record-last-method-v1', 'original_source': ORIGINAL, 'original_source_sha256': ORIGINAL_HASH, 'original_row_sha256': ROW_HASH, 'original_expected': expected('sci-execution-sci-record-method-duplicate-method', 9), 'artifact_root': str(directory), 'compiler_build_producer_source_sha256': before, 'audit_executable_sha256': binary_hash, 'node_executable_sha256': node_hash, 'frames': local_frames(directory / 'local'), 'references': {}}
    source_root = directory / 'source'; source_root.mkdir(); (source_root / 'user.cljs').write_text(module(ORIGINAL, 'application'))
    cases = [{'id': 'original', 'source': module(ORIGINAL, 'application'), 'source_root': str(source_root), 'entry': 'user', 'export': 'run', 'expected': expected('original', 9)}]
    cases += [{'id': name, 'source': source, 'expected': expected(name, value)} for name, (source, value) in CONTROLS.items()]
    for pin in ('release', 'checkout'):
        layers = report.reference_cases(cases, pin, ROOT / 'research/clojurescript', directory / pin, timeout)
        if set(layers) != set(CASES): raise ValueError('Missing/replaced pin observations')
        for name, layer in layers.items():
            if layer.get('status') != 'passed' or layer.get('reference_identity') != pin + ' ' + (report.RELEASE if pin == 'release' else report.REVISION) or layer.get('settings') != {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False}:
                raise ValueError(f'Unverified pin {pin}/{name}: {layer}')
        document['references'][pin] = layers
        print(pin, f'{len(CASES)}/{len(CASES)} complete original/control observations passed', flush=True)
    runner.unchanged(before, snapshot())
    if digest(binary.read_bytes()) != binary_hash or digest(node.read_bytes()) != node_hash: raise ValueError('Actual executable drift')
    document['artifact_sha256'] = {str(p.relative_to(directory)): digest(p.read_bytes()) for p in sorted(directory.rglob('*')) if p.is_file()}
    document['status'] = 'passed'
    (directory / 'evidence.json').write_text(json.dumps(document, indent=2) + '\n')
    return document


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    parser.add_argument('--timeout', type=int, default=240)
    args = parser.parse_args()
    run(args.output, args.timeout)
