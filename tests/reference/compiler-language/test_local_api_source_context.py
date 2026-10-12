#!/usr/bin/env python3
"""Independent guards for the original SCI local API contract adapter.

Optional actual controls use the current CLI and checked source-context pins.
They create only unregistered /tmp artifacts and never change cohort inputs.
"""
import copy
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

import local_api_source_context as cohort
import sci_execution as sci


# Each actual expression is independent of the matcher implementation. Every
# false case keeps equal element values while changing a retained wire type.
TYPE_CONTROLS = [
    ('vector', '[1 2]', "'[1 2]", True),
    ('vector-list', '[1 2]', "'(1 2)", False),
    ('list-vector', '(1 2)', "'[1 2]", False),
    ('list-seq-projection', '(1 2)', '(seq [1 2])', True),
    ('empty-list-seq', '()', '(rest [1])', True),
    ('empty-vector-list', '[]', "'()", False),
    ('empty-list-vector', '()', "'[]", False),
    ('vector-nested-list', '[(1 2)]', "'[[1 2]]", False),
    ('vector-nested-vector', '[[1 2]]', "'[(1 2)]", False),
    ('list-nested-vector', '([1 2])', "'((1 2))", False),
    ('list-nested-list', '((1 2))', "'([1 2])", False),
    ('deep-sequence-swap', '[(:a [1 2])]', "'[(:a (1 2))]", False),
    ('map-vector-key', '{[1 2] :value}', "'{(1 2) :value}", False),
    ('map-list-key', '{(1 2) :value}', "'{[1 2] :value}", False),
    ('map-deep-key', '{[:k [1 2]] :value}', "'{[:k (1 2)] :value}", False),
    ('map-vector-value', '{:k [1 2]}', "'{:k (1 2)}", False),
    ('map-list-value', '{:k (1 2)}', "'{:k [1 2]}", False),
    ('map-deep-value', '{:k {:nested [[1 2]]}}', "'{:k {:nested [(1 2)]}}", False),
    ('map-key-and-value', '{[1] [(2)]}', "'{[1] [(2)]}", True),
    ('map-order', '{:a [1] :b (2)}', "'{:b (2) :a [1]}", True),
    ('map-extra-entry', '{:a [1]}', "'{:a [1] :b 2}", False),
    ('set-vector-member', '#{[1 2]}', "'#{(1 2)}", False),
    ('set-list-member', '#{(1 2)}', "'#{[1 2]}", False),
    ('set-deep-member', '#{[:k [1 2]]}', "'#{[:k (1 2)]}", False),
    ('set-map-member', '#{{:k [1 2]}}', "'#{{:k (1 2)}}", False),
    ('set-order', '#{[1] (2) :a}', "'#{:a (2) [1]}", True),
    ('set-extra-member', '#{[1]}', "'#{[1] [2]}", False),
    ('effects-nested-payload', '[[:observed [1 2]]]', "'[[:observed (1 2)]]", False),
    ('nil', 'nil', 'nil', True),
    ('nil-false', 'nil', 'false', False),
    ('boolean-number', 'false', '0', False),
    ('string-symbol', '"a"', "'a", False),
    ('symbol-string', 'a', '"a"', False),
    ('keyword-symbol', ':a', "'a", False),
    ('symbol-keyword', 'a', ':a', False),
    ('number-string', '1', '"1"', False),
    ('zero', '0', '0', True),
    ('negative-zero', '0', '(js* "-0")', False),
    ('nan', '0', 'js/NaN', False),
    ('infinity', '0', 'js/Infinity', False),
]


def control_source():
    typed = [
        '(' + ('true?' if expected else 'false?') + " (api-review-same? '"
        + literal + ' ' + actual + '))'
        for _, literal, actual, expected in TYPE_CONTROLS
    ]
    # Invoke the actual cohort adapter, rather than a copy of its stdout logic.
    clean = cohort.complete_source('(do (observe [:one [1 2]]) 7)', '7', '[[:one [1 2]]]', [])
    dirty = cohort.complete_source('(do (observe [:one [1 2]]) (print "leak") 7)', '7', '[[:one [1 2]]]', [])
    nested = cohort.complete_source('(do (observe [:one [1 2]]) (with-out-str (print "inside")))', '"inside"', '[[:one [1 2]]]', [])
    typed += [
        "(= '[[true true []] \"\"] " + clean + ')',
        "(= '[[true true []] \"leak\"] " + dirty + ')',
        "(not (= '[[true true []] \"\"] " + dirty + '))',
        "(= '[[true true []] \"\"] " + nested + ')',
    ]
    # Evaluate every probe, even if an earlier contract unexpectedly fails.
    return '(let [api-review-same? ' + cohort.MATCHER + '] (every? true? [' + ' '.join(typed) + ']))'


class LocalApiOriginalContractTests(unittest.TestCase):
    def test_every_original_source_row_and_typed_expectation_is_preserved(self):
        original = sci.rows(sci.CORPUS)
        records = cohort.rows()
        self.assertEqual(len(records), 121)
        self.assertEqual(sum(len(row['names']) for row in records), 143)
        for row in records:
            raw, fields = original[row['sci_id']]
            self.assertEqual(row['original_source'], fields[2])
            self.assertEqual(row['original_row_sha256'], sci.digest(raw))
            self.assertEqual(row['source'], cohort.complete_source(
                fields[2], cohort.literal(sci.wire(fields[3])[1]),
                cohort.literal(sci.wire(fields[4])), row['names']))
            self.assertEqual(row['sci_binding']['original_source_sha256'], row['sci_binding']['source_sha256'])

    def test_wrapper_names_cannot_capture_any_original_free_name(self):
        for row in cohort.rows():
            for name in ['api-same?', 'api-effects', 'api-result', 'api-observed', 'api-stdout']:
                self.assertNotIn(name, row['original_source'])

    def test_detached_source_literal_or_history_fails_closed(self):
        original = (cohort.ROOT / cohort.FIXTURE).read_text()
        lines = original.splitlines()
        index = next(i for i, line in enumerate(lines) if line and not line.startswith('#'))
        fields = lines[index].split('\t')
        real_read = Path.read_text
        for field in ['source', 'expected_edn', 'original_source', 'original_row_sha256']:
            changed = fields[:]
            row = json.loads(changed[3])
            row[field] = 'changed'
            if field == 'source': changed[1] = row[field].encode().hex()
            if field == 'expected_edn': changed[2] = row[field].encode().hex()
            changed[3] = json.dumps(row)
            text = '\n'.join(lines[:index] + ['\t'.join(changed)] + lines[index + 1:]) + '\n'
            def read(path, *args, **kwargs):
                return text if path == cohort.ROOT / cohort.FIXTURE else real_read(path, *args, **kwargs)
            with patch.object(Path, 'read_text', read), self.assertRaises(ValueError):
                cohort.rows()

    def test_profile_does_not_accept_body_replacement_or_macro_inference(self):
        case = cohort.cases()[0]
        for field in ['source', 'expected', 'hosted_source_adapter']:
            changed = copy.deepcopy(case)
            changed[field] = 'changed'
            with self.assertRaises(ValueError): cohort.validate(changed)
        self.assertFalse(cohort.validate({'id': 'unreviewed-macro', 'source': '(defmacro m [] true)'}))

    def test_raw_frames_reject_missing_duplicate_false_and_reordered_receipts(self):
        lines = ['local-api-source-observation\t' + row['id'] + '\t' + host + '\t' + phase + '\t' + engine + '\ttrue'
                 for row in cohort.rows() for host in ['fresh', 'reused']
                 for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
        self.assertEqual(len(cohort.parse_frames('\n'.join(lines))), 968)
        for changed in [lines[:-1], lines + [lines[0]], [lines[1], lines[0], *lines[2:]],
                        [lines[0][:-4] + 'false', *lines[1:]]]:
            with self.assertRaises(ValueError): cohort.parse_frames('\n'.join(changed))

    def test_wire_projection_preserves_nested_types_and_negative_zero(self):
        self.assertNotEqual(sci.projection({'$vector': [{'$list': [1]}]}), sci.projection({'$vector': [{'$vector': [1]}]}))
        self.assertEqual(sci.projection({'$seq': [1]}), sci.projection({'$list': [1]}))
        self.assertNotEqual(sci.projection({'$number': '-0'}), sci.projection(0))
        self.assertEqual(cohort.literal(['char', '0061']), '"a"')
        with self.assertRaises(ValueError): cohort.literal(['float', '8000000000000000'])

    @unittest.skipUnless(os.environ.get('LIGHT_LOCAL_API_REVIEW_CONTROLS'), 'explicit actual-control artifact directory required')
    def test_recursive_contract_controls_execute_in_application_and_actual_macro(self):
        directory = Path(os.environ['LIGHT_LOCAL_API_REVIEW_CONTROLS'])
        directory.mkdir(parents=True, exist_ok=False)
        binary = cohort.ROOT / 'target/debug/light-cljs'
        producer = cohort.snapshot()
        binary_hash = cohort.digest(binary.read_bytes())
        source = control_source()
        observations = []
        for phase in ['application', 'macro']:
            root = directory / phase
            root.mkdir()
            module = '(ns user)\n' + (source if phase == 'application' else
                '(defmacro api-review-check [] (if ' + source + ' true false))\n(api-review-check)') + '\n'
            path = root / 'user.cljs'
            path.write_text(module)
            quick = subprocess.run([str(binary), '--eval', str(path)], cwd=cohort.ROOT, capture_output=True, text=True, timeout=60)
            (root / 'QuickJS.stdout').write_text(quick.stdout)
            (root / 'QuickJS.stderr').write_text(quick.stderr)
            self.assertEqual(quick.returncode, 0, quick.stderr)
            self.assertEqual(quick.stderr, '')
            self.assertEqual(quick.stdout.strip(), 'true', phase + ' QuickJS')
            compiled = subprocess.run([str(binary), '--compile', '--entry', 'user', '--source-root', str(root)], cwd=cohort.ROOT, capture_output=True, text=True, timeout=60)
            (root / 'compiled.js').write_text(compiled.stdout)
            (root / 'compiler.stderr').write_text(compiled.stderr)
            self.assertEqual(compiled.returncode, 0, compiled.stderr)
            self.assertEqual(compiled.stderr, '')
            node = subprocess.run(['node', '-'], input='console.log(' + compiled.stdout + ');', cwd=cohort.ROOT, capture_output=True, text=True, timeout=60)
            (root / 'Node.stdout').write_text(node.stdout)
            (root / 'Node.stderr').write_text(node.stderr)
            self.assertEqual(node.returncode, 0, node.stderr)
            self.assertEqual(node.stderr, '')
            self.assertEqual(node.stdout.strip(), 'true', phase + ' Node')
            observations.extend({'phase': phase, 'engine': engine, 'raw': output, 'module_source_sha256': cohort.digest(module)}
                                for engine, output in [('QuickJS', quick.stdout), ('Node', node.stdout)])
        references = {}
        if os.environ.get('LIGHT_LOCAL_API_REVIEW_PINS') == 'both':
            row = {'id': 'local-api-recursive-contract-controls', 'source': source, 'source_sha256': cohort.digest(source),
                   'forms': [{'kind': 'runtime', 'source': source}],
                   'expected': {'id': 'local-api-recursive-contract-controls', 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}}
            for kind in ['release', 'checkout']:
                references[kind] = cohort.context.reference_cases([row], kind, directory / kind, 300)
                (directory / (kind + '-receipt.json')).write_text(json.dumps(references[kind], indent=2) + '\n')
                self.assertEqual(references[kind][row['id']]['status'], 'passed')
        self.assertEqual(producer, cohort.snapshot(), 'producer changed during controls')
        self.assertEqual(binary_hash, cohort.digest(binary.read_bytes()))
        (directory / 'evidence.json').write_text(json.dumps({'controls': TYPE_CONTROLS, 'source': source,
            'source_sha256': cohort.digest(source), 'producer_source_sha256': producer,
            'audit_binary_sha256': binary_hash, 'observations': observations, 'references': references}, indent=2) + '\n')


if __name__ == '__main__':
    unittest.main()
