#!/usr/bin/env python3
"""Reproduce complete pinned utility bodies and assertion witness links.

This is a source adapter, not an execution/pass declaration. The original
Copyright (c) Rich Hickey and Eclipse Public License 1.0 notices remain in
research/clojurescript; see epl-v10.html there.
"""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tests/reference/compiler-language'
sys.path.insert(0, str(HERE))
import extract_inventory as inv

PREFIX = 'cljs-utilities-completion-'
PARSE = 'src/test/cljs/cljs/parse_test.cljs::'
OTHER = 'src/test/cljs/cljs/other_functions_test.cljs::'
ARRAY = 'src/test/cljs/cljs/array_access_test.cljs::'
GENERATIVE_BODIES = {PARSE + 'test-gen-parse-long', PARSE + 'test-gen-parse-double'}
EXTRA_BODIES = {
    'primitives_test.cljs::' + name for name in (
        'test-js-primitives', 'test-booleans', 'test-partial-and-comp',
        'test-arrays', 'test-make-array', 'test-misc', 'test-char?')
} | {
    'core_test.cljs::' + name for name in (
        'test-uuid', 'aget-test', 'aset-test', 'unchecked-get-test',
        'memfn-test', 'test-730', 'test-cljs-2865', 'test-str_')
} | {
    ARRAY + name for name in ('aget-test', 'aset-test', 'unchecked-aget-test', 'unchecked-set-test',
        'checked-aget-test', 'checked-aset-test', "checked-aget'-test", "checked-aset'-test")
}
SUPPRESS_ERRS = 'src/test/cljs/cljs/array_access_test.cljc::suppress-errs'
SETUP = {
    'primitives_test.cljs::test-make-array': ['primitives_test.cljs::primitive-arrays-equal'],
    'core_test.cljs::test-cljs-2865': ['core_test.cljs::str-fn-2865'],
    **{ARRAY + name + '-test': [ARRAY + name + '-alias', SUPPRESS_ERRS]
        for name in ('checked-aget', 'checked-aset')},
    **{ARRAY + name + '-test': [ARRAY + name + '-alias']
        for name in ("checked-aget'", "checked-aset'")},
}
RATIONALE = (
    'Retain the complete pinned upstream body, every are row, helper setup, '
    'collection input, loop, mutation, random effect and explicit exception path. '
    'testing becomes do; is/are use a non-throwing check accumulator and the '
    'complete body asserts that no check failed. This keeps an is false inside '
    'catch :default from being mistaken for an expected parse exception. '
    'Original expected predicates/messages and exception classes are retained. '
    'Checked array aliases retain their full original helper forms; suppress-errs '
    'uses its exact pinned with-redefs *print-err-fn* expansion, with the original '
    '.cljc macro source/hash retained as setup provenance. '
    'Existing assertion witnesses stay authoritative; test.check generative '
    'parse bodies are separate unresolved obligations, never sampled here.'
)
NOTICE = '''# Copyright (c) Rich Hickey. All rights reserved.
# The use and distribution terms for upstream adapted bodies are covered by
# the Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php).
# See research/clojurescript/epl-v10.html. This notice must be retained.
# id\tsource\texpected JSON\toriginal complete body/hash
'''


def read(name):
    return [json.loads(line) for line in (HERE / name).read_text().splitlines() if line]


def whitespace(text):
    # This is reader whitespace only; literal strings and regexes are handled
    # as complete leaf spans, so comments/whitespace inside literals survive.
    return re.sub(r';[^\n]*', '', text).replace('\n', ' ').replace('\t', ' ')


def adapt(text, node):
    head = inv.short_head(node)
    if head == 'deftest':
        return '(do ' + ' '.join(adapt(text, c) for c in node.children[2:]) + ')'
    if head == 'testing':
        return '(do ' + ' '.join(adapt(text, c) for c in node.children[2:]) + ')'
    if head == 'is':
        return '(utility-check ' + ' '.join(adapt(text, c) for c in node.children[1:]) + ')'
    if head == 'are':
        bindings, expr, values = node.children[1].children, node.children[2], node.children[3:]
        width = len(bindings)
        assert width and len(values) % width == 0
        return '(do ' + ' '.join(
            '(let [' + ' '.join(adapt(text, b) + ' ' + adapt(text, v)
                for b, v in zip(bindings, values[i:i+width])) + '] (utility-check ' + adapt(text, expr) + '))'
            for i in range(0, len(values), width)) + ')'
    if head == 'thrown?':
        return '(try ' + ' '.join(adapt(text, c) for c in node.children[2:]) + ' false (catch ' + adapt(text, node.children[1]) + ' utility-error true))'
    if head == 'suppress-errs':
        return '(with-redefs [*print-err-fn* nil] ' + ' '.join(adapt(text, c) for c in node.children[1:]) + ')'
    if not node.children:
        literal = text[node.start:node.end]
        if node.kind in {'string', 'regex'}:
            return literal.replace('\n', r'\n').replace('\t', r'\t')
        return literal
    parts, offset = [], node.start
    for child in node.children:
        parts.extend([whitespace(text[offset:child.start]), adapt(text, child)])
        offset = child.end
    return ''.join(parts) + whitespace(text[offset:node.end])


def checked_body(forms):
    return (
        '(let [utility-failures (atom []) utility-check '
        '(fn [value & message] (when-not value (swap! utility-failures conj message)) value)] '
        + ' '.join(forms) + ' (assert (empty? @utility-failures) (pr-str @utility-failures)) true)')


def caught_false_contract():
    source = '(deftest caught-false (try (is false) (catch :default _ (is true))))'
    adapted = checked_body([adapt(source, inv.Reader(source).all()[0])])
    # The false check must return normally through the try; its failure is
    # asserted only after that catch has ended. Engine tests execute this probe.
    nodes = inv.Reader(adapted).all()
    assert len(nodes) == 1
    assert '(try (utility-check false) (catch :default _ (utility-check true)))' in adapted
    assert adapted.endswith('(assert (empty? @utility-failures) (pr-str @utility-failures)) true)')
    return adapted


def fixtures():
    bodies = {body['id']: body for body in read('full-bodies.jsonl')}
    wanted = EXTRA_BODIES | {b['id'] for b in bodies.values()
        if b['form'] == 'deftest' and b['id'].startswith((PARSE, OTHER))
        and b['id'] not in GENERATIVE_BODIES}
    assert len(wanted) == 41, len(wanted)
    assert wanted <= bodies.keys(), wanted - bodies.keys()
    pinned, result = {}, []
    for body in bodies.values():
        if body['id'] not in wanted:
            continue
        originals = [bodies[identity] for identity in SETUP.get(body['id'], [])] + [body]
        forms = []
        for original in originals:
            source_path = original['source']
            if source_path not in pinned:
                pinned[source_path] = subprocess.check_output([
                    'git', '-C', str(ROOT / 'research/clojurescript'), 'show',
                    inv.REVISION + ':' + source_path])
            span = original['source_span']
            text = span['text']
            assert hashlib.sha256(text.encode()).hexdigest() == span['sha256']
            assert pinned[source_path][span['start_byte']:span['end_byte']].decode() == text
            if original['id'] == SUPPRESS_ERRS:
                assert text == ('(defmacro suppress-errs [& forms]\n'
                    '  `(cljs.core/with-redefs [cljs.core/*print-err-fn* nil]\n'
                    '     ~@forms))')
                continue
            forms.append(adapt(text, inv.Reader(text).all()[0]))
        source = checked_body(forms)
        assert '\n' not in source and '\t' not in source
        assert len(inv.Reader(source).all()) == 1
        identity = PREFIX + body['id'].replace('.cljs::', '-').replace('_', '-').replace('/', '-').replace('?', '-p')
        provenance = body['id'] + ';sha256=' + body['source_span']['sha256']
        for setup in originals[:-1]:
            provenance += ';setup=' + setup['id'] + ';sha256=' + setup['source_span']['sha256']
        result.append([identity, source, 'true', provenance])
    return result


def records(rows):
    body_fixtures = {row[3].split(';')[0]: row[0] for row in rows}
    existing = {w['assertion_id'] for w in read('full-witnesses.jsonl')
        if not w['fixture_id'].startswith(PREFIX)}
    existing.update(w['assertion_id'] for w in read('witnesses.jsonl'))
    witnesses = [{'assertion_id': row['id'], 'assertion_sha256': row['assertion']['sha256'],
        'fixture_id': body_fixtures[row['body_id']], 'rationale': RATIONALE}
        for row in read('full-inventory.jsonl')
        if row['body_id'] in body_fixtures and row['disposition'] == 'in-scope'
        and row['id'] not in existing]
    linked = {w['fixture_id'] for w in witnesses}
    cases = [{'id': identity, 'source': source,
        'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True},
            'trace': [], 'stdout': ''}, 'native_stdout': None}
        for identity, source, _, _ in rows if identity in linked]
    return cases, witnesses


def literal_contracts():
    # Expectations come from independent literal Rust contracts. Same-file
    # compiler-executed macros are a local dialect: preserve the full helper
    # bodies/assertions but invoke only the ordinary application in the oracle.
    path = ROOT / 'tests/compiler_utilities_completion.rs'
    text = path.read_text()
    result = []
    for index, match in enumerate(re.finditer(r'r#"(.*?)"#', text, re.S)):
        source = match.group(1)
        expected = re.match(r',\s*("(?:\\.|[^"\\])*")', text[match.end():])
        if not expected:
            # Negative adapter injection probes deliberately have no returned
            # value contract and therefore cannot become passing oracle rows.
            assert 'utility-check' in source, 'Missing independent literal expectation'
            continue
        value = json.loads(expected.group(1))
        if any('(defn ' + name + ' ' in source for name in ('witness', 'array-witness', 'property-witness')):
            originals = inv.Reader(source).all()
            assert len(originals) == 3
            assert inv.short_head(originals[0]) == 'defn'
            assert inv.short_head(originals[1]) == 'defmacro'
            name = originals[0].children[1].token
            source = source[originals[0].start:originals[0].end] + ' (' + name + ')'
            if name in {'witness', 'property-witness'}:
                assert value == '[true true]'
                value = 'true'
            else:
                assert name == 'array-witness'
                expected_vector = inv.Reader(value).all()[0]
                first, second = expected_vector.children
                assert value[first.start:first.end] == value[second.start:second.end]
                value = value[first.start:first.end]
        # Encode only reader whitespace, never change literal contents.
        source = ' '.join(adapt(source, n) for n in inv.Reader(source).all())
        result.append(['cljs-utilities-contract-' + str(index), source, value,
            'tests/compiler_utilities_completion.rs;sha256=' + hashlib.sha256(text.encode()).hexdigest()])
    assert len(result) == 5, len(result)
    return result


def execution_mappings():
    path = 'tests/compiler_utilities_completion.rs'
    execution = {'test_file': path,
        'test_name': 'public_utilities_execute_in_applications_and_macros',
        'test_source_sha256': hashlib.sha256((ROOT / path).read_bytes()).hexdigest()}
    direct, phase = [], []
    for row in read('obligations.jsonl'):
        if row['owner'] != 'light-cljs-122' or row['disposition'] != 'intended':
            continue
        if row['obligation_kind'] not in {'public-api', 'local-api', 'local-metadata'}:
            # Assertion-free macro/config modules need their own module evidence.
            continue
        rationale = (
            'The complete hashed witness invokes ' + row['name']
            + ' at each retained source arity in ordinary QuickJS/Node applications '
            'and a QuickJS compiler-executed macro. It preserves controlled random '
            'calls/restoration, original parse boundaries and invalid-type effects, '
            'array initialization/identity, protocol clone dispatch and scalar/string '
            'expectations. All fourteen added metadata Vars are checked for their '
            'source names, docs, lines and arglists. This direct declaration mapping '
            'does not claim completion of original generative or compiler-config modules.')
        common = {'obligation_id': row['id'], 'body_sha256': row['body_sha256'],
            'rationale': rationale}
        if row.get('signature'):
            common['signature_sha256'] = hashlib.sha256(
                json.dumps(row['signature'], sort_keys=True).encode()).hexdigest()
        direct.append({**common, 'local_execution': execution, 'assertion_ids': []})
        phase.append({**common, **execution})
    assert len(direct) == 37, len(direct)
    assert sum(row['obligation_id'].startswith('api::') for row in direct) == 22
    return direct, phase


def main():
    if '--caught-false' in sys.argv:
        print(caught_false_contract())
        return
    caught_false_contract()
    rows = fixtures()
    cases, witnesses = records(rows)
    fixture = ROOT / 'tests/fixtures/compiler-utilities-completion.tsv'
    literals = literal_contracts()
    direct, phase = execution_mappings()
    literal_fixture = ROOT / 'tests/fixtures/compiler-utilities-contracts.tsv'
    if '--write' in sys.argv:
        fixture.write_text(NOTICE + '\n'.join('\t'.join(row) for row in rows) + '\n')
        literal_fixture.write_text('# Independent literal engine contracts; see compiler_utilities_completion.rs\n'
            + '\n'.join('\t'.join(row) for row in literals) + '\n')
        for name, expected, key in [('full-cases.jsonl', cases, 'id'),
                                     ('full-witnesses.jsonl', witnesses, 'fixture_id')]:
            old = [line for line in (HERE / name).read_text().splitlines()
                if line and not json.loads(line)[key].startswith(PREFIX)]
            (HERE / name).write_text('\n'.join(old) + '\n' + ''.join(
                json.dumps(row, separators=(',', ':')) + '\n' for row in expected))
        for name, expected in [('obligation-witnesses.jsonl', direct), ('phase-witnesses.jsonl', phase)]:
            identities = {row['obligation_id'] for row in expected}
            old = [line for line in (HERE / name).read_text().splitlines()
                if line and json.loads(line)['obligation_id'] not in identities]
            (HERE / name).write_text('\n'.join(old) + '\n' + ''.join(
                json.dumps(row, separators=(',', ':')) + '\n' for row in expected))
    else:
        actual = [line.split('\t') for line in fixture.read_text().splitlines()
            if line and not line.startswith('#')]
        assert actual == rows, 'Utility fixtures differ from complete upstream body/setup adaptation'
        actual_literals = [line.split('\t') for line in literal_fixture.read_text().splitlines()
            if line and not line.startswith('#')]
        assert actual_literals == literals, 'Independent contract sources/expectations changed'
        for name, expected, key in [('full-cases.jsonl', cases, 'id'),
                                     ('full-witnesses.jsonl', witnesses, 'fixture_id')]:
            actual = [row for row in read(name) if row[key].startswith(PREFIX)]
            assert actual == expected, name + ' differs from fixture/witness reproduction'
        for name, expected in [('obligation-witnesses.jsonl', direct), ('phase-witnesses.jsonl', phase)]:
            identities = {row['obligation_id'] for row in expected}
            actual = [row for row in read(name) if row['obligation_id'] in identities]
            assert actual == expected, name + ' differs from utility declaration execution mappings'
    print(f'{len(rows)} complete utility bodies and {len(witnesses)} new assertion witnesses reproduce exactly; 2 generative parse bodies remain separate')


if __name__ == '__main__':
    main()
