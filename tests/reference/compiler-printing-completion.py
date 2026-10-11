#!/usr/bin/env python3
"""Reproduce complete pinned printing_test bodies without asserting execution.

Copyright (c) Rich Hickey. All rights reserved.
The upstream bodies are distributed under Eclipse Public License 1.0.
See research/clojurescript/epl-v10.html; retain this notice.
"""
from pathlib import Path
import hashlib
import importlib.util
import json
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import extract_inventory as inv
spec = importlib.util.spec_from_file_location('utilities_adapter', ROOT / 'tests/reference/compiler-utilities-completion.py')
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)
SOURCE = 'src/test/cljs/cljs/printing_test.cljs'
NAMES = ['test-print-knobs', 'test-print-with-opts', 'test-printing']
NEGATIVE_ZERO = SOURCE + '::test-printing::assertion-005'
DEST = ROOT / 'tests/fixtures/compiler-printing-completion.tsv'
PROVENANCE = ROOT / 'tests/reference/compiler-printing-provenance.json'


def adapt_printing(text, node, assertions):
    head = inv.short_head(node)
    if head in {'deftest', 'testing'}:
        return '(do ' + ' '.join(adapt_printing(text, c, assertions) for c in node.children[2:]) + ')'
    if head == 'is':
        original = text[node.start:node.end]
        identity = assertions[hashlib.sha256(original.encode()).hexdigest()]
        return '(utility-check ' + json.dumps(identity) + ' ' + ' '.join(adapter.adapt(text, c) for c in node.children[1:]) + ')'
    if not node.children:
        return adapter.adapt(text, node)
    parts, offset = [], node.start
    for child in node.children:
        parts.extend([adapter.whitespace(text[offset:child.start]), adapt_printing(text, child, assertions)])
        offset = child.end
    return ''.join(parts) + adapter.whitespace(text[offset:node.end])


def complete_body(text, node):
    assertions = {row['assertion']['sha256']: row['id'] for row in read_rows('full-inventory.jsonl')
                  if row['source'] == SOURCE}
    return ('(let [utility-failures (atom []) utility-check '
            '(fn [identity value & message] (when-not value (swap! utility-failures conj identity)) value)] '
            + adapt_printing(text, node, assertions)
            + ' (if (empty? @utility-failures) true @utility-failures))')


def generated():
    text = subprocess.check_output(['git', '-C', str(ROOT / 'research/clojurescript'), 'show', inv.REVISION + ':' + SOURCE]).decode()
    forms = {node.children[1].token: node for node in inv.Reader(text).all()
             if inv.short_head(node) in {'deftest', 'defrecord'}}
    rows, origins, programs = [], [], {}
    for name in NAMES:
        node = forms[name]
        original = text[node.start:node.end]
        setup = [forms['PrintMe']] if name == 'test-printing' else []
        setup_text = [text[n.start:n.end] for n in setup]
        body = complete_body(text, node)
        source = '(ns cljs.printing-test) ' + ' '.join(setup_text) + ' ' + body
        assert '\n' not in source and '\t' not in source
        rows.append('\t'.join([name, source, 'true', hashlib.sha256(original.encode()).hexdigest()]))
        programs[name] = notice_source() + '(ns cljs.printing-test)\n' + ' '.join(setup_text) + '\n(defn run [observe]\n' + body + '\n)\nrun\n'
        origins.append({'id': name, 'revision': inv.REVISION, 'source': SOURCE,
                        'body': original, 'body_sha256': hashlib.sha256(original.encode()).hexdigest(),
                        'setup': [{'body': s, 'sha256': hashlib.sha256(s.encode()).hexdigest()} for s in setup_text],
                        'adapter': 'Complete deftest/testing bodies retained; is uses a nonthrowing accumulator of exact assertion IDs; the body returns true if empty or the complete failed-ID vector. Namespace and exact PrintMe declaration are retained; all date-loop iterations and collection inputs are executed.'})
    notice = '# Copyright (c) Rich Hickey. All rights reserved. Eclipse Public License 1.0.\n# See research/clojurescript/epl-v10.html. Retain this notice.\n# name\tcomplete adapted source\texpected\toriginal body sha256\n'
    return notice + '\n'.join(rows) + '\n', json.dumps(origins, indent=2, ensure_ascii=False) + '\n', programs



def notice_source():
    text = subprocess.check_output(['git', '-C', str(ROOT / 'research/clojurescript'), 'show', inv.REVISION + ':' + SOURCE]).decode()
    return text[:text.index('(ns ')] + '; Complete pinned printing_test body adapter.\n'


def read_rows(name):
    return [json.loads(line) for line in (ROOT / 'tests/reference/compiler-language' / name).read_text().splitlines() if line]


def registrations(programs):
    cases, witnesses = [], []
    existing = {row['assertion_id'] for row in read_rows('full-witnesses.jsonl') + read_rows('witnesses.jsonl')
                if not row['fixture_id'].startswith('cljs-printing-completion-')}
    for name, source in programs.items():
        identity = 'cljs-printing-completion-' + name
        case_root = 'tests/fixtures/compiler-printing-completion/' + name
        cases.append({'id': identity, 'source_root': case_root, 'entry': 'cljs.printing-test', 'export': 'run',
                      'source': source, 'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}, 'native_stdout': None})
        for row in read_rows('full-inventory.jsonl'):
            if row['body_id'] == SOURCE + '::' + name and row['disposition'] == 'in-scope' and row['id'] not in existing:
                witnesses.append({'assertion_id': row['id'], 'assertion_sha256': row['assertion']['sha256'],
                                  'fixture_id': identity, 'rationale': 'Retain the entire pinned printing_test body, all input collections, alternate/fallback writer callbacks, PrintMe declaration and nominal namespace, UTF-16 examples and every date-loop iteration. testing becomes do; is records exact failed source assertion IDs without throwing; after all paths finish, the adapter returns true or the complete failed-ID vector.'})
    case_by_id = {c['id']: c for c in cases}
    for witness in witnesses:
        if witness['fixture_id'] == 'cljs-printing-completion-test-printing':
            expected = case_by_id[witness['fixture_id']]['expected']
            witness['version_difference'] = {
                'reason': 'The entire retained test-printing body executes in both pins. Release 1.12.134 prints negative zero as 0 and fails precisely upstream assertion-005; checkout a5e94c9 and local printing retain -0.0 and pass every check. The nonthrowing source adapter returns the exact failed-ID vector, so all other 34 assertion sites and date iterations must succeed. All sites in this shared body inherit its versioned outcome.',
                'release': {**expected, 'outcome': {'status': 'returned', 'value': {'$vector': [NEGATIVE_ZERO]}}},
                'checkout': expected}
    return cases, witnesses


def update_rows(filename, new, key, predicate):
    path = ROOT / 'tests/reference/compiler-language' / filename
    lines = [line for line in path.read_text().splitlines() if line and not predicate(json.loads(line))]
    lines.extend(json.dumps(r, ensure_ascii=False, separators=(',', ':')) for r in new)
    path.write_text('\n'.join(lines) + '\n')


def printing_obligations():
    helpers = {'pr-sequential-writer', 'pr-seq-writer', 'print-meta?', 'enable-console-print!'}
    sinks = {'pr', 'prn', 'print', 'println', 'set-print-fn!', 'set-print-err-fn!', '*print-fn*', '*print-err-fn*', '*print-newline*', '*flush-on-newline*', 'newline', 'string-print'}
    general = {'flush', 'pr-str', 'prn-str', 'print-str', 'println-str', 'pr-str-with-opts', 'prn-str-with-opts', '*print-meta*', '*print-dup*', '*print-length*', '*print-level*', '*print-readably*', '*print-namespace-maps*'}
    digest = hashlib.sha256((ROOT / 'tests/compiler_printing_completion.rs').read_bytes()).hexdigest()
    obligations, phases = [], []
    for row in read_rows('obligations.jsonl'):
        name = row['name'].split('/')[-1]
        if row['owner'] != 'light-cljs-124' or name not in helpers | sinks | general:
            continue
        test = ('public_writer_helpers_and_console_hooks_execute_in_both_phases' if name in helpers else
                'output_sinks_capture_nesting_flush_and_restoration' if name in sinks else
                'length_level_readability_metadata_and_option_overrides')
        if name in {'prn-str-with-opts', '*print-dup*'}:
            # These added settings/functions have dedicated explicit source use
            # in the options callback test below, alongside the same macro path.
            test = 'alternative_printers_receive_fallback_and_recurse_with_options'
        common = {'obligation_id': row['id'], 'body_sha256': row['body_sha256'],
                  'rationale': 'The complete hashed printing test directly invokes this public name at the retained source arity in Node/QuickJS application execution and a compiler-executed QuickJS macro. It asserts writer options, output ordering, saved sink identity, nesting/restoration, UTF-16 text, or collection print controls as appropriate; host hooks and stub flush remain explicit.'}
        if row.get('signature'):
            common['signature_sha256'] = inv.digest(json.dumps(row['signature'], sort_keys=True).encode())
        execution = {'test_file': 'tests/compiler_printing_completion.rs', 'test_name': test, 'test_source_sha256': digest}
        phases.append({**common, **execution})
        obligations.append({**common, 'local_execution': execution, 'assertion_ids': []})
    return obligations, phases


fixtures, provenance, programs = generated()
cases, witnesses = registrations(programs)
if '--write' in sys.argv:
    DEST.write_text(fixtures)
    PROVENANCE.write_text(provenance)
    printing, phases = printing_obligations()
    keys = {row['obligation_id'] for row in printing}
    update_rows('obligation-witnesses.jsonl', printing, 'obligation_id', lambda r: r['obligation_id'] in keys)
    update_rows('phase-witnesses.jsonl', phases, 'obligation_id', lambda r: r['obligation_id'] in keys)
    for name, source in programs.items():
        path = ROOT / 'tests/fixtures/compiler-printing-completion' / name / 'cljs/printing_test.cljs'
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source)
    update_rows('full-cases.jsonl', cases, 'id', lambda r: r['id'].startswith('cljs-printing-completion-'))
    update_rows('full-witnesses.jsonl', witnesses, 'assertion_id', lambda r: r['fixture_id'].startswith('cljs-printing-completion-'))
else:
    printing, phases = printing_obligations()
    keys = {row['obligation_id'] for row in printing}
    assert [r for r in read_rows('obligation-witnesses.jsonl') if r['obligation_id'] in keys] == printing
    assert [r for r in read_rows('phase-witnesses.jsonl') if r['obligation_id'] in keys] == phases
    assert DEST.read_text() == fixtures, 'Printing fixtures differ from pinned full bodies'
    assert PROVENANCE.read_text() == provenance, 'Printing source provenance differs'
    for case in cases:
        assert (ROOT / case['source_root'] / 'cljs/printing_test.cljs').read_text() == case['source']
    assert [r for r in read_rows('full-cases.jsonl') if r['id'].startswith('cljs-printing-completion-')] == cases
    assert [r for r in read_rows('full-witnesses.jsonl') if r['fixture_id'].startswith('cljs-printing-completion-')] == witnesses
    print('All three complete pinned printing workflows and PrintMe setup are reproducible')
