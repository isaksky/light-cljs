#!/usr/bin/env python3
"""Reproduce sorted fixtures from complete pinned bodies; never declare execution."""
from pathlib import Path
import json
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import extract_inventory as inv


def adapt(text, node):
    head = inv.short_head(node)
    if head == 'deftest':
        return '(do ' + ' '.join(adapt(text, c) for c in node.children[2:]) + ' true)'
    if head == 'testing':
        return '(do ' + ' '.join(adapt(text, c) for c in node.children[2:]) + ')'
    if head == 'is':
        return '(assert ' + adapt(text, node.children[1]) + ')'
    if head == 'are':
        bindings, expr, values = node.children[1].children, node.children[2], node.children[3:]
        n = len(bindings)
        assert n and len(values) % n == 0
        return '(do ' + ' '.join(
            '(let [' + ' '.join(adapt(text, b) + ' ' + adapt(text, v)
                for b, v in zip(bindings, values[i:i+n])) + '] (assert ' + adapt(text, expr) + '))'
            for i in range(0, len(values), n)) + ')'
    if not node.children:
        return text[node.start:node.end]
    parts, offset = [], node.start
    for child in node.children:
        parts.extend([text[offset:child.start], adapt(text, child)])
        offset = child.end
    return ''.join(parts) + text[offset:node.end]


def fixtures():
    here = ROOT / 'tests/reference/compiler-language'
    read = lambda name: [json.loads(l) for l in (here / name).read_text().splitlines() if l]
    wanted = {r['body_id'] for r in read('full-inventory.jsonl')
              if r['category'] == 'sorted-collections' and r['historical_scope']}
    wanted.remove('core_test.cljs::test-multimethods-2')
    wanted.update({'src/test/cljs/cljs/other_functions_test.cljs::test-update-keys',
                   'src/test/cljs/cljs/other_functions_test.cljs::test-update-vals',
                   'src/test/cljs/cljs/set_equiv_test.cljs::test-set-equality'})
    result = []
    for body in read('full-bodies.jsonl'):
        if body['id'] not in wanted:
            continue
        text = body['source_span']['text']
        source = adapt(text, inv.Reader(text).all()[0])
        if body['id'] == 'core_test.cljs::test-cljs-2079':
            source = '(do (defrecord CLJS2079 [a b]) ' + source + ')'
        if 'LITE_MODE' in source:
            source = '(let [LITE_MODE false] ' + source + ')'
        source = ' '.join(re.sub(r';[^\n]*', '', source).split())
        identity = 'cljs-sorted-' + body['id'].replace('.cljs::', '-').replace('_', '-').replace('/', '-')
        result.append([identity, source, 'true', body['id'] + ';sha256=' + body['source_span']['sha256']])
    assert len(result) == 18
    return result


if __name__ == '__main__':
    path = ROOT / 'tests/fixtures/compiler-sorted-collections.tsv'
    actual = [l.split('\t') for l in path.read_text().splitlines() if l and not l.startswith('#')]
    expected = fixtures()
    assert len({r[0] for r in actual}) == len(actual) == len(expected)
    assert {r[0]: r[1:] for r in actual} == {r[0]: r[1:] for r in expected}, 'Sorted fixtures differ from full original body/setup adaptation'
    print('18 sorted fixtures reproduce every original body/setup/assertion path')
