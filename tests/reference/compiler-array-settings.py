#!/usr/bin/env python3
"""Reproduce retained pinned array-setting bodies and ordinary local macro setup.

Copyright (c) Rich Hickey. All rights reserved. Eclipse Public License 1.0.
See research/clojurescript/epl-v10.html. Retain this notice.
"""
from pathlib import Path
import hashlib
import json
import subprocess
import sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import extract_inventory as inv
SOURCE = 'src/test/cljs/cljs/array_access_test.cljs'
NAMES = ['cljs-2861-test', 'unchecked-arrays-file-scope-test']
PREFIX = 'cljs-array-settings-'
PROVENANCE = ROOT / 'tests/reference/compiler-array-settings-provenance.json'
LEDGER = ROOT / 'tests/reference/compiler-language'
TEST = 'tests/compiler_array_settings.rs'
TEST_NAME = 'array_setting_var_and_macro_state_execute_in_both_phases'


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def pinned(source):
    return subprocess.check_output(['git', '-C', str(ROOT / 'research/clojurescript'), 'show', inv.REVISION + ':' + source]).decode()


def read_rows(name):
    return [json.loads(line) for line in (LEDGER / name).read_text().splitlines() if line]


def adapt(text, node, assertions):
    head = inv.short_head(node)
    if head == 'deftest':
        return '(do ' + ' '.join(adapt(text, child, assertions) for child in node.children[2:]) + ')'
    if head == 'is':
        identity = assertions[sha(text[node.start:node.end])]
        return '(array-settings-check ' + json.dumps(identity) + ' ' + ' '.join(adapt(text, child, assertions) for child in node.children[1:]) + ')'
    if not node.children:
        return text[node.start:node.end]
    parts, offset = [], node.start
    for child in node.children:
        parts.extend([text[offset:child.start], adapt(text, child, assertions)])
        offset = child.end
    return ''.join(parts) + text[offset:node.end]


def generated():
    text = pinned(SOURCE)
    notice = text[:text.index('(ns ')]
    core_text = pinned('src/main/cljs/cljs/core.cljs')
    declaration_node = next(node for node in inv.Reader(core_text).all()
                            if inv.short_head(node) == 'def' and node.children[1].token == '*unchecked-arrays*')
    declaration = core_text[declaration_node.start:declaration_node.end]
    forms = {node.children[1].token: node for node in inv.Reader(text).all() if inv.short_head(node) == 'deftest'}
    assertions = {row['assertion']['sha256']: row['id'] for row in read_rows('full-inventory.jsonl') if row['source'] == SOURCE}
    original_setup = {name: pinned('src/test/cljs/cljs/array_access/' + name) for name in ['alpha.cljs', 'beta.cljs', 'helper.clj']}
    local_setup = {
        'alpha.cljs': original_setup['alpha.cljs'].replace('(:require-macros [cljs.array-access.helper :as helper])\n  (:require [cljs.array-access.beta])', '(:require [cljs.array-access.helper :as helper]\n            [cljs.array-access.beta])'),
        'beta.cljs': original_setup['beta.cljs'].replace(':require-macros', ':require'),
        'helper.cljc': notice + '(ns cljs.array-access.helper)\n\n(defmacro unchecked-arrays? []\n  (:light-cljs/unchecked-arrays &env))\n',
    }
    files, cases, witnesses, provenance = {}, [], [], []
    for name in NAMES:
        node = forms[name]
        original = text[node.start:node.end]
        body = ('(let [array-settings-failures (atom [])\n'
                '      array-settings-check (fn [identity value & message]\n'
                '        (when-not value (swap! array-settings-failures conj identity)) value)]\n'
                + adapt(text, node, assertions) + '\n'
                '(if (empty? @array-settings-failures) true @array-settings-failures))')
        source = notice + '; Complete pinned array-setting body; assertion adapter only.\n' + '(ns cljs.array-access-test\n  (:require [cljs.array-access.alpha :as alpha]))\n\n(defn run [observe]\n' + body + '\n)\nrun\n'
        case_root = 'tests/fixtures/compiler-array-settings/' + name
        oracle_root = 'tests/reference/compiler-array-settings/original/' + name
        for base, setup in [(case_root, local_setup), (oracle_root, original_setup)]:
            files[base + '/cljs/array_access_test.cljs'] = source
            for filename, content in setup.items():
                files[base + '/cljs/array_access/' + filename] = content
        identity = PREFIX + name
        cases.append({'id': identity, 'source_root': case_root, 'entry': 'cljs.array-access-test', 'export': 'run', 'source': source,
                      'reference_source_root': oracle_root,
                      'reference_files_sha256': {'cljs/array_access_test.cljs': sha(source), **{'cljs/array_access/' + filename: sha(content) for filename, content in original_setup.items()}},
                      'reference_adaptation': 'Original pinned alpha/beta/helper modules supply the analyzer macro oracle. Only the entry test wrappers adapt deftest/is; local ordinary helper.cljc is never compiled by an upstream reference.',
                      'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}, 'native_stdout': None})
        for row in read_rows('full-inventory.jsonl'):
            if row['body_id'] == SOURCE + '::' + name and row['disposition'] == 'in-scope':
                witnesses.append({'assertion_id': row['id'], 'assertion_sha256': row['assertion']['sha256'], 'fixture_id': identity,
                                  'rationale': 'Retain the complete pinned body, runtime variable read, alpha function call, alpha/beta/helper namespace graph, and beta file-scope set!. deftest becomes do; is accumulates the exact failed assertion ID. Local alpha uses ordinary requires and helper.cljc obtains its ordinary macro expansion from the explicit &env array-setting key; both references use the retained original module paths and helper.clj analyzer oracle.'})
        provenance.append({'id': identity, 'revision': inv.REVISION, 'source': SOURCE, 'body': original, 'body_sha256': sha(original),
                           'source_root': case_root, 'reference_source_root': oracle_root, 'entry': 'cljs.array-access-test', 'export': 'run',
                           'setup': [{'source': 'src/test/cljs/cljs/array_access/' + filename, 'body': content, 'sha256': sha(content)} for filename, content in original_setup.items()],
                           'adapted_setup': [{'file': filename, 'body': content, 'sha256': sha(content)} for filename, content in local_setup.items()],
                           'adapter': 'The body retains every source expression. Only deftest/is wrappers change to a nonthrowing failed-ID accumulator. The runtime declaration remains the core declaration, never redefined by a fixture. Original alpha and beta require-macros become ordinary local require edges; original helper.clj is reference-only. Local helper.cljc is an ordinary defmacro reading (:light-cljs/unchecked-arrays &env). Beta retains (set! *unchecked-arrays* true).',
                           'public_declaration': {'source': 'src/main/cljs/cljs/core.cljs', 'body': declaration, 'sha256': sha(declaration)}})
    return files, cases, witnesses, provenance


def public_mappings():
    targets = [row for row in read_rows('obligations.jsonl') if row['id'] in {
        'api::src/main/cljs/cljs/core.cljs::*unchecked-arrays*::arity-1',
        'local::src/core_var_metadata.tsv::*unchecked-arrays*'}]
    assert len(targets) == 2
    digest = hashlib.sha256((ROOT / TEST).read_bytes()).hexdigest()
    execution = {'test_file': TEST, 'test_name': TEST_NAME, 'test_source_sha256': digest}
    obligations, phases = [], []
    for target in targets:
        common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'],
                  'rationale': 'The complete named local test reads the public false runtime value and dereferenced Var, asserts source file/line/name/namespace metadata, and executes ordinary macros reading the explicit &env array-setting key before/during/after unqualified compiler-setting changes. It checks that an unqualified setter has no runtime mutation or RHS effects, while a qualified setter changes the runtime value independently of macro state; fresh analyses reset state even when sharing a MacroHost. Node/QuickJS application execution and a compiler-executed macro both read the runtime root. Original upstream helper.clj is used only by the two pinned references.'}
        if target.get('signature'):
            common['signature_sha256'] = sha(json.dumps(target['signature'], sort_keys=True))
        obligations.append({**common, 'local_execution': execution, 'assertion_ids': []})
        phases.append({**common, **execution})
    return obligations, phases


def update(name, rows, owns):
    path = LEDGER / name
    retained = [line for line in path.read_text().splitlines() if line and not owns(json.loads(line))]
    path.write_text('\n'.join(retained + [json.dumps(row, separators=(',', ':'), ensure_ascii=False) for row in rows]) + '\n')


if __name__ == '__main__':
    files, cases, witnesses, provenance = generated()
    writing = '--write' in sys.argv
    for path, content in files.items():
        target = ROOT / path
        if writing:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content)
        else:
            assert target.read_text() == content, path
    rendered = json.dumps(provenance, indent=2, ensure_ascii=False) + '\n'
    if writing:
        PROVENANCE.write_text(rendered)
        update('full-cases.jsonl', cases, lambda row: row['id'].startswith(PREFIX))
        update('full-witnesses.jsonl', witnesses, lambda row: row['fixture_id'].startswith(PREFIX))
    else:
        assert PROVENANCE.read_text() == rendered
        assert [row for row in read_rows('full-cases.jsonl') if row['id'].startswith(PREFIX)] == cases
        assert [row for row in read_rows('full-witnesses.jsonl') if row['fixture_id'].startswith(PREFIX)] == witnesses
    if '--fixtures-only' not in sys.argv:
        obligations, phases = public_mappings()
        keys = {row['obligation_id'] for row in obligations}
        owns = lambda row: row['obligation_id'] in keys
        for name, rows in [('obligation-witnesses.jsonl', obligations), ('phase-witnesses.jsonl', phases)]:
            if writing:
                update(name, rows, owns)
            else:
                assert [row for row in read_rows(name) if owns(row)] == rows
    if '--references' in sys.argv:
        import report
        import tempfile
        directory = Path(tempfile.mkdtemp(prefix='light-cljs-array-settings-oracles-'))
        evidence = {}
        for kind in ['release', 'checkout']:
            evidence[kind] = {}
            for case in cases:
                evidence[kind].update(report.reference_program(case, kind, ROOT / 'research/clojurescript', directory / kind / case['id'], 180))
        for kind, outcomes in evidence.items():
            for case in cases:
                observed = outcomes[case['id']]
                assert observed['status'] == 'passed' and observed['observed'] == case['expected'], (kind, observed)
        (directory / 'results.json').write_text(json.dumps(evidence, indent=2) + '\n')
        print('Both exact pinned reference source trees returned true; logs: ' + str(directory))
    print('Two retained array-setting bodies and complete alpha/beta/helper setup reproduce exactly')
