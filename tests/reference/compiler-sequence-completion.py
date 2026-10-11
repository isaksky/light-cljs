#!/usr/bin/env python3
"""Reproduce complete pinned sequence bodies and their assertion witness links.

This adapter preserves complete executable upstream bodies and helper setup.
It does not declare execution or a pass; execution belongs to the report runner.
Upstream Copyright (c) Rich Hickey; Eclipse Public License 1.0. The original
notices remain in research/clojurescript; see epl-v10.html there.
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

PREFIX = 'cljs-sequence-completion-'
EXTRA_BODIES = {
    'src/test/cljs/cljs/iterator_test.cljs::coll-iter-seq-match',
    'core_test.cljs::test-delay',
    'src/test/cljs/cljs/interop_test.cljs::test-es6-interfaces',
    'core_test.cljs::test-transducers',
    'core_test.cljs::test-cljs-2028',
    'collections_test.cljs::test-lazy-seq-realized?',
    'collections_test.cljs::test-cljs-2474',
    'collections_test.cljs::test-cljs-1809',
}
SETUP = {
    'seqs_test.cljs::test-cljs-2482': ['seqs_test.cljs::Foo'],
    'src/test/cljs/cljs/iterator_test.cljs::coll-iter-seq-match': [
        'src/test/cljs/cljs/iterator_test.cljs::seq-iter-match',
        'src/test/cljs/cljs/iterator_test.cljs::TestIterRec'],
}
RATIONALE = (
    'Retain the complete pinned upstream body, all setup, collection inputs, '
    'loops, realization checks, random effects and assertion paths. testing '
    'becomes do; is/are become assert (including original assertion messages); '
    'thrown? becomes a try/catch boolean with the original exception class. '
    'LITE_MODE is false, preserving both source branches and executing the '
    'normal pinned configuration. Original Foo or seq-iter-match/TestIterRec '
    'support forms are retained in the fixtures that use them. Existing '
    'witness links remain authoritative for previously linked assertion sites.'
)
NOTICE = '''# Copyright (c) Rich Hickey. All rights reserved.
# The use and distribution terms for upstream adapted bodies are covered by
# the Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php).
# See research/clojurescript/epl-v10.html. This notice must be retained.
# id\tsource\texpected JSON\toriginal complete body/hash
'''


def read(name):
    return [json.loads(line) for line in (HERE / name).read_text().splitlines() if line]


def adapt(text, node):
    head = inv.short_head(node)
    if head == 'deftest':
        return '(do ' + ' '.join(adapt(text, c) for c in node.children[2:]) + ' true)'
    if head == 'testing':
        return '(do ' + ' '.join(adapt(text, c) for c in node.children[2:]) + ')'
    if head == 'is':
        return '(assert ' + ' '.join(adapt(text, c) for c in node.children[1:]) + ')'
    if head == 'are':
        bindings, expr, values = node.children[1].children, node.children[2], node.children[3:]
        width = len(bindings)
        assert width and len(values) % width == 0
        return '(do ' + ' '.join(
            '(let [' + ' '.join(adapt(text, b) + ' ' + adapt(text, v)
                for b, v in zip(bindings, values[i:i+width])) + '] (assert ' + adapt(text, expr) + '))'
            for i in range(0, len(values), width)) + ')'
    if head == 'thrown?':
        return '(try ' + ' '.join(adapt(text, c) for c in node.children[2:]) + ' false (catch ' + adapt(text, node.children[1]) + ' sequence-error true))'
    if not node.children:
        return text[node.start:node.end]
    parts, offset = [], node.start
    for child in node.children:
        # Remove comments only in reader whitespace, never in strings/regexes.
        parts.extend([re.sub(r';[^\n]*', '', text[offset:child.start]), adapt(text, child)])
        offset = child.end
    return ''.join(parts) + re.sub(r';[^\n]*', '', text[offset:node.end])


def fixtures():
    bodies = {body['id']: body for body in read('full-bodies.jsonl')}
    wanted = EXTRA_BODIES | {b['id'] for b in bodies.values()
        if b['source'].endswith('/seqs_test.cljs') and b['form'] == 'deftest'}
    assert len(wanted) == 35, len(wanted)
    assert wanted <= bodies.keys(), wanted - bodies.keys()
    pinned = {}
    result = []
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
            forms.append(adapt(text, inv.Reader(text).all()[0]))
        source = forms[-1] if len(forms) == 1 else '(do ' + ' '.join(forms) + ')'
        if 'LITE_MODE' in source:
            source = '(let [LITE_MODE false] ' + source + ')'
        # Whitespace in literal strings is retained; no global split/join.
        source = source.replace('\n', ' ').replace('\t', ' ')
        identity = PREFIX + body['id'].replace('.cljs::', '-').replace('_', '-').replace('/', '-').replace('?', '-p')
        provenance = body['id'] + ';sha256=' + body['source_span']['sha256']
        for setup in originals[:-1]:
            provenance += ';setup=' + setup['id'] + ';sha256=' + setup['source_span']['sha256']
        result.append([identity, source, 'true', provenance])
    return result


def literal_contracts():
    # Raw strings and their adjacent literal expectations are the independent
    # engine probes. Same-file defmacro calls are a local phase dialect; retain
    # the ordinary helpers/assertions in the pinned reference invocation.
    text = (ROOT / 'tests/compiler_sequence_completion.rs').read_text()
    result = []
    for index, match in enumerate(re.finditer(r'r#"(.*?)"#', text, re.S)):
        source = match.group(1)
        if '(macro-consume)' in source:
            continue
        expected = re.match(r',\s*"([^"\n]*)"', text[match.end():])
        if '(defn witness ' in source:
            value = 'true'
        elif '(defn chunk-witness ' in source:
            value = 'true'
        else:
            assert expected, 'Missing independent literal expectation'
            value = expected.group(1)
        source = re.sub(r'\(defmacro (phase-witness|chunk-phase) \[\] \((witness|chunk-witness)\)\)', '', source)
        source = source.replace('[(witness) (phase-witness)]', '(witness)').replace('[(chunk-witness) (chunk-phase)]', '(chunk-witness)')
        result.append(['cljs-sequence-contract-' + str(index), source.replace('\n', ' ').replace('\t', ' '), value,
            'tests/compiler_sequence_completion.rs;sha256=' + hashlib.sha256(text.encode()).hexdigest()])
    assert len(result) == 13
    return result


def records(rows):
    cases = [{'id': identity, 'source': source,
        'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True},
            'trace': [], 'stdout': ''}, 'native_stdout': None}
        for identity, source, _, provenance in rows]
    body_fixtures = {row[3].split(';')[0]: row[0] for row in rows}
    existing = {w['assertion_id'] for w in read('full-witnesses.jsonl')
        if not w['fixture_id'].startswith(PREFIX)}
    existing.update(w['assertion_id'] for w in read('witnesses.jsonl'))
    witnesses = [{'assertion_id': row['id'], 'assertion_sha256': row['assertion']['sha256'],
        'fixture_id': body_fixtures[row['body_id']], 'rationale': RATIONALE}
        for row in read('full-inventory.jsonl')
        if row['body_id'] in body_fixtures and row['disposition'] == 'in-scope'
        and row['id'] not in existing]
    # Previously mapped sites keep their authoritative historical fixtures. Only
    # new witnesses need additional report cases; the direct engine/oracle suite
    # still executes every complete fixture, including bodies already mapped.
    linked = {w['fixture_id'] for w in witnesses}
    cases = [case for case in cases if case['id'] in linked]
    return cases, witnesses


def main():
    rows = fixtures()
    cases, witnesses = records(rows)
    fixture = ROOT / 'tests/fixtures/compiler-sequence-completion.tsv'
    literals = literal_contracts()
    literal_fixture = ROOT / 'tests/fixtures/compiler-sequence-contracts.tsv'
    if '--write' in sys.argv:
        fixture.write_text(NOTICE + '\n'.join('\t'.join(row) for row in rows) + '\n')
        literal_fixture.write_text('# Independent literal engine contracts; see compiler_sequence_completion.rs\n' + '\n'.join('\t'.join(row) for row in literals) + '\n')
        for name, expected, key in [('full-cases.jsonl', cases, 'id'),
                                     ('full-witnesses.jsonl', witnesses, 'fixture_id')]:
            old = [line for line in (HERE / name).read_text().splitlines()
                if line and not json.loads(line)[key].startswith(PREFIX)]
            (HERE / name).write_text('\n'.join(old) + '\n' + ''.join(
                json.dumps(row, separators=(',', ':')) + '\n' for row in expected))
    else:
        actual = [line.split('\t') for line in fixture.read_text().splitlines()
            if line and not line.startswith('#')]
        assert actual == rows, 'Sequence fixtures differ from complete upstream body/setup adaptation'
        actual_literals = [line.split('\t') for line in literal_fixture.read_text().splitlines() if line and not line.startswith('#')]
        assert actual_literals == literals, 'Independent contract sources/expectations changed'
        for name, expected, key in [('full-cases.jsonl', cases, 'id'),
                                     ('full-witnesses.jsonl', witnesses, 'fixture_id')]:
            actual = [row for row in read(name) if row[key].startswith(PREFIX)]
            assert actual == expected, name + ' differs from fixture/witness reproduction'
    print(f'{len(rows)} complete sequence/iterator/realization bodies and {len(witnesses)} new assertion witnesses reproduce exactly')


if __name__ == '__main__':
    main()
