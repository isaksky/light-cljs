import json
from pathlib import Path
import unittest
from unittest import mock
import arity_resume_candidates as cohort


class ArityResumeCandidatesTests(unittest.TestCase):
    def test_exact_whole_workflow_exports_have_complete_application_phase_pin_anchors(self):
        generated = cohort.exports()
        ordinary = generated['obligation-witnesses.jsonl']
        phases = generated['phase-witnesses.jsonl']
        cases = {row['id']: row for row in generated['full-cases.jsonl']}
        self.assertEqual(24, len(ordinary))
        self.assertEqual(9, len(cases))
        self.assertEqual([row['obligation_id'] for row in ordinary], [row['obligation_id'] for row in phases])
        for ordinary, phase in zip(ordinary, phases):
            case = cases[ordinary['reference_fixture_id']]
            self.assertEqual(cohort.digest(case['source']), ordinary['reference_source_sha256'])
            self.assertEqual(cohort.digest(json.dumps(case['expected'], sort_keys=True)), ordinary['reference_contract_sha256'])
            self.assertEqual(ordinary['local_execution']['test_file'], phase['test_file'])
            self.assertIn(cohort.HELPER, phase['support_source_sha256'])
            self.assertEqual([], ordinary['assertion_ids'])

    def test_quoted_discarded_and_thread_injected_calls_are_not_direct_arities(self):
        source = "['(compare 1 2) #_(compare 2 3) (-> 1 (compare 2)) (compare 4 5)]"
        self.assertEqual(['(compare 4 5)'], [call['call_source'] for call in cohort.direct_calls(source, 'compare')])

    def test_receiver_method_declarations_are_not_calls(self):
        source = '(reify IFoo (compare [_ x] (compare x 1)))'
        self.assertEqual(['(compare x 1)'], [call['call_source'] for call in cohort.direct_calls(source, 'compare')])

    def test_changed_whole_fixture_literal_is_rejected(self):
        original = Path.read_text
        def altered(path, *args, **kwargs):
            text = original(path, *args, **kwargs)
            if str(path).endswith('compiler-core-factories.tsv'):
                text = text.replace('(get-method x :a)', '(get-method x :missing)')
            return text
        with mock.patch.object(Path, 'read_text', altered), self.assertRaisesRegex(ValueError, 'whole reviewed fixture row drift'):
            cohort.exports()

    def test_changed_original_setup_body_is_rejected(self):
        original = Path.read_text
        def altered(path, *args, **kwargs):
            text = original(path, *args, **kwargs)
            if str(path).endswith('full-bodies.jsonl'):
                rows = list(map(json.loads, text.splitlines()))
                row = next(row for row in rows if row['id']=='core_test.cljs::test-transducers')
                row['source_span']['text'] += ' nil'
                text = '\n'.join(map(json.dumps, rows))
            return text
        with mock.patch.object(Path, 'read_text', altered), self.assertRaisesRegex(ValueError, 'original complete body text drift'):
            cohort.exports()

    def test_detached_actual_macro_driver_is_rejected(self):
        original = Path.read_text
        def altered(path, *args, **kwargs):
            text = original(path, *args, **kwargs)
            if str(path).endswith('compiler_core_factories.rs'):
                text = text.replace('defmacro', 'undemanded')
            return text
        with mock.patch.object(Path, 'read_text', altered), self.assertRaisesRegex(ValueError, 'actual MacroHost source contract detached'):
            cohort.exports()

    def test_changed_receiver_signature_is_rejected(self):
        original = Path.read_text
        def altered(path, *args, **kwargs):
            text = original(path, *args, **kwargs)
            if str(path).endswith('obligations.jsonl'):
                rows = list(map(json.loads, text.splitlines()))
                row = next(row for row in rows if row['id']=='api::src/main/cljs/cljs/core.cljs::-iterator::arity-1')
                row['signature']['minimum'] = 2
                text = '\n'.join(map(json.dumps, rows))
            return text
        with mock.patch.object(Path, 'read_text', altered), self.assertRaisesRegex(ValueError, 'exact public signature drift'):
            cohort.exports()


if __name__ == '__main__':
    unittest.main()
