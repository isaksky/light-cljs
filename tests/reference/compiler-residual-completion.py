#!/usr/bin/env python3
"""Reproduce complete residual programs and propose, never register, witnesses.

Copyright (c) Rich Hickey. Eclipse Public License 1.0. Original source notices
remain in research/clojurescript/epl-v10.html; retain this notice.
"""
from pathlib import Path
import hashlib
import importlib.util
import json
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tests/reference/compiler-language'
sys.path.insert(0, str(HERE))
import extract_inventory as inv
spec = importlib.util.spec_from_file_location('residual', ROOT / 'tests/reference/compiler-residual-bodies.py')
residual = importlib.util.module_from_spec(spec)
spec.loader.exec_module(residual)
PREFIX = 'cljs-residual-completion-'
FIXTURE = 'tests/fixtures/compiler-residual-completion.tsv'
TEST = 'tests/compiler_residual_completion.rs'
TEST_NAME = 'remaining_complete_residual_bodies_execute_in_both_phases'
SOURCE_ROOT = 'tests/fixtures/compiler-residual-completion'
REFERENCE_ROOT = SOURCE_ROOT + '-reference'
SELECTED = {
    'core_test.cljs::test-cljs-2133': ['core_test.cljs::Partial'],
    'core_test.cljs::var-desugar-test': [],
    'core_test.cljs::test-cljs-3313': ['core_test.cljs::ExtMetaProtocol', 'core_test.cljs::top-level-272'],
    'core_test.cljs::test-new-method': [],
}
NOTICE = ';; Copyright (c) Rich Hickey. Eclipse Public License 1.0.\n;; See research/clojurescript/epl-v10.html; retain this notice.\n'


def persist(path, text):
    if '--write' in sys.argv:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    else:
        assert path.read_text() == text, str(path) + ' drift'


def original_form(body):
    span = body['source_span']
    text = span['text']
    pinned = subprocess.check_output(['git', '-C', str(ROOT / 'research/clojurescript'),
        'show', inv.REVISION + ':' + body['source']])
    assert hashlib.sha256(text.encode()).hexdigest() == span['sha256']
    assert pinned[span['start_byte']:span['end_byte']].decode() == text
    node = inv.Reader(text).all()[0]
    if inv.short_head(node) == 'deftest':
        # Assertion-free tests retain their actual final result. No invented
        # assertion or unconditional `true` may hide a failed computation.
        source = '(do ' + ' '.join(residual.adapt(text, c) for c in node.children[2:]) + ')'
    else:
        source = residual.adapt(text, node)
    return source, {'body_id': body['id'], 'source_sha256': span['sha256']}


def omission_probes(bodies):
    result = []
    selection = [
        ('core_test.cljs::test-cljs-2133', ['core_test.cljs::Partial'], 3,
         'Repaired by light-cljs-149: complete original Partial/test-cljs-2133 now returns 3 in Node/QuickJS application and QuickJS macro computations against both pinned references; original probe and provenance preserved.'),
        ('src/test/cljs/cljs/interop_test.cljs::test-obj-equiv', [], True,
         'Repaired by light-cljs-150: complete original non-lite Object .equiv body retains all seven assertions and returns true in Node/QuickJS applications and independent QuickJS macro computations against both pinned references; original probe and provenance preserved.'),
        ('src/test/cljs/cljs/predicates_test.cljs::test-int-preds', ['src/test/cljs/cljs/predicates_test.cljs::int-val-table'], True,
         'Pending explicit host-input boundary review: the complete original table includes goog.math.Long and goog.math.Integer representations via upstream LongImpl and Integer setup. Current compiler cannot resolve cljs.core/LongImpl; preserve all fourteen table rows, eight predicate columns and original loops. This does not establish a defect in JavaScript-number predicate behavior or assign a focused feature owner.'),
    ]
    for identity, setup, expected, scope in selection:
        forms, origins = [], []
        for body in [bodies[s] for s in setup] + [bodies[identity]]:
            form, origin = original_form(body)
            # Assertion-bearing upstream bodies end in assertions whose value
            # is nil. The ordinary residual adapter returns true only after
            # every retained assertion has actually executed successfully.
            if body['id'] == identity and body['assertions']:
                text = body['source_span']['text']
                form = residual.adapt(text, inv.Reader(text).all()[0])
            forms.append(form)
            origins.append(origin)
        if identity.endswith('::test-obj-equiv'):
            forms.insert(0, '(def LITE_MODE false)')
        source = '(do ' + ' '.join(forms) + ')'
        if identity.endswith('::test-int-preds'):
            source = '(let [Integer js/goog.math.Integer LongImpl cljs.core/LongImpl] ' + source + ')'
        source = source.replace('\n', ' ').replace('\t', ' ')
        result.append({'body_id': identity, 'source': source,
            'source_sha256': inv.digest(source.encode()), 'complete_original_provenance': origins,
            'assertion_ids': bodies[identity]['assertions'], 'expected_returned_value': expected,
            'status': 'witnessed-semantic-omission' if identity.endswith(('::test-cljs-2133', '::test-obj-equiv')) else 'unwitnessed-host-input-boundary' if identity.endswith('::test-int-preds') else 'unwitnessed-semantic-omission',
            'focused_owner_scope': scope})
    persist(ROOT / 'tests/reference/compiler-residual-completion-omissions.json', json.dumps(result, indent=2) + '\n')


def generate():
    bodies = {b['id']: b for b in residual.read('full-bodies.jsonl')}
    omission_probes(bodies)
    rows, cases, mappings, phases = [], [], [], []
    for identity, setup in SELECTED.items():
        forms, provenance = [], []
        for original in [bodies[s] for s in setup] + [bodies[identity]]:
            form, origin = original_form(original)
            forms.append(form)
            provenance.append(origin)
        if identity.endswith('::test-new-method'):
            # Original :refer-global :only [Object String] supplies Object.
            # Bind that exact ECMAScript constructor in the portable adapter.
            forms.insert(0, '(def Object js/Object)')
        source = ('(do ' + ' '.join(forms) + ')').replace('\n', ' ').replace('\t', ' ')
        case_id = PREFIX + identity.split('::')[-1]
        expected = 3 if identity.endswith('::test-cljs-2133') else True
        rows.append([case_id, source, '3' if expected == 3 else 'true', json.dumps(provenance, separators=(',', ':'))])
        cases.append({'id': case_id, 'source': source,
            'expected': {'id': case_id, 'outcome': {'status': 'returned', 'value': expected}, 'trace': [], 'stdout': ''},
            'native_stdout': None})
        mappings.append({'obligation_id': 'body::' + identity,
            'body_sha256': bodies[identity]['source_span']['sha256'], 'reference_fixture_id': case_id,
            'reference_source_sha256': inv.digest(source.encode()),
            'reference_contract_sha256': inv.digest(json.dumps(cases[-1]['expected'], sort_keys=True).encode()),
            'rationale': 'Complete assertion-free upstream body and all relevant original setup execute with its actual final returned oracle in both application engines and as a QuickJS macro computation. No source assertion or pass count is invented. Original global Object alias is bound to the identical ECMAScript constructor where required.'})
    # This entire upstream module defines one macro, with no assertions or
    # runtime invocations. Preserve the exact module and execute the macro in
    # an explicit supplemental scope/restoration contract rather than count
    # its declaration as an executed assertion.
    identity = 'program::src/test/cljs/cljs/array_access_test.cljc'
    original = next(row for row in residual.read('obligations.jsonl') if row['id'] == identity)
    span = original['source_span']
    module = span['text']
    pinned = subprocess.check_output(['git', '-C', str(ROOT / 'research/clojurescript'),
        'show', inv.REVISION + ':' + original['source']])
    assert inv.digest(module.encode()) == span['sha256']
    assert pinned.decode() == module
    module_path = SOURCE_ROOT + '/cljs/array_access_test.cljc'
    persist(ROOT / module_path, module)
    persist(ROOT / REFERENCE_ROOT / 'cljs/array_access_test.cljc', module)
    source = '(let [previous *print-err-fn* result (suppress-errs (nil? *print-err-fn*))] [result (identical? previous *print-err-fn*)])'
    case_id = PREFIX + 'array-access-program'
    program = NOTICE + '(ns residualcomplete.arrayprobe (:require [cljs.array-access-test :refer [suppress-errs]]))\n(defn run [] ' + source + ')\n'
    persist(ROOT / SOURCE_ROOT / 'residualcomplete/arrayprobe.cljs', program)
    reference_program = program.replace('(:require [cljs.array-access-test', '(:require-macros [cljs.array-access-test')
    persist(ROOT / REFERENCE_ROOT / 'residualcomplete/arrayprobe.cljs', reference_program)
    rows.append([case_id, source, '[true true]', json.dumps({'program_id': identity, 'source_sha256': span['sha256']}, separators=(',', ':'))])
    cases.append({'id': case_id, 'source': program, 'source_root': SOURCE_ROOT,
        'reference_source_root': REFERENCE_ROOT,
        'reference_adaptation': 'Local ordinary :require loads macros from the unchanged complete .cljc module. Canonical CLJS requires :require-macros for its JVM macro compiler; only this driver import clause differs. Original module and supplemental expansion/restoration contract are identical.',
        'reference_files_sha256': {'cljs/array_access_test.cljc': inv.digest(module.encode()), 'residualcomplete/arrayprobe.cljs': inv.digest(reference_program.encode())},
        'entry': 'residualcomplete.arrayprobe', 'export': 'run',
        'expected': {'id': case_id, 'outcome': {'status': 'returned', 'value': {'$vector': [True, True]}}, 'trace': [], 'stdout': ''},
        'native_stdout': None})
    mappings.append({'obligation_id': identity, 'body_sha256': original['body_sha256'],
        'reference_fixture_id': case_id, 'reference_source_sha256': inv.digest(program.encode()),
        'reference_contract_sha256': inv.digest(json.dumps(cases[-1]['expected'], sort_keys=True).encode()),
        'rationale': 'Complete original assertion-free macro module preserved byte-for-byte, with an explicit supplemental invocation proving nil error-printer binding and restoration. Execute its full helper expansion in ordinary applications and inside a QuickJS macro, then independently in both pinned references. No source assertion or invocation is invented as an upstream pass.'})
    text = '# Copyright (c) Rich Hickey. Eclipse Public License 1.0.\n# id\tsource\texpected\tcomplete original body/setup hashes\n' + ''.join('\t'.join(row) + '\n' for row in rows)
    persist(ROOT / FIXTURE, text)
    local = {'test_file': TEST, 'test_name': TEST_NAME,
        'test_source_sha256': inv.digest((ROOT / TEST).read_bytes()),
        'support_source_sha256': {FIXTURE: inv.digest(text.encode()), module_path: inv.digest(module.encode())}}
    for row in mappings:
        row['local_execution'] = {key: local[key] for key in ('test_file', 'test_name', 'test_source_sha256')}
        phases.append({**row, **local})
    proposal = {'full-cases.jsonl': cases, 'full-witnesses.jsonl': [],
        'obligation-witnesses.jsonl': mappings, 'phase-witnesses.jsonl': phases}
    persist(ROOT / 'tests/reference/compiler-residual-completion-proposals.json', json.dumps(proposal, indent=2) + '\n')
    if '--catalogs' in sys.argv:
        # Opt in only after the main integration owner coordinates the shared
        # catalog write. Without --write this checks registered owned rows.
        ownership = {
            'full-cases.jsonl': lambda row: row['id'].startswith(PREFIX),
            'full-witnesses.jsonl': lambda row: row['fixture_id'].startswith(PREFIX),
            'obligation-witnesses.jsonl': lambda row: row.get('reference_fixture_id', '').startswith(PREFIX),
            'phase-witnesses.jsonl': lambda row: row.get('test_file') == TEST,
        }
        for name, owned in ownership.items():
            residual.update_owned(name, owned, proposal[name])
    print(f'{len(SELECTED)} complete assertion-free bodies and 1 complete macro module; proposed witnesses only, execution required')
    return cases


if __name__ == '__main__':
    generate()
