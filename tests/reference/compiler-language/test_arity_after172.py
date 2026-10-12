import copy
import json
from pathlib import Path
import unittest
from unittest import mock
import arity_after172 as cohort


class ArityAfter172Tests(unittest.TestCase):
    def test_current_whole_oracles_operation_rows_and_exact_arities(self):
        exported = cohort.exports()
        self.assertEqual(56, len(exported['obligation-witnesses.jsonl']))
        self.assertEqual(9, sum('signature_sha256' in row for row in exported['obligation-witnesses.jsonl']))
        self.assertEqual(30, len(cohort.rows()))
        for row in exported['obligation-witnesses.jsonl']:
            self.assertEqual([], row['assertion_ids'])
        self.assertFalse(any(row['obligation_id'].endswith('::dispatch-fn') for row in exported['obligation-witnesses.jsonl']))

    def test_removed_original_apply_fails_closed(self):
        original = Path.read_text
        def changed(path, *args, **kwargs):
            result = original(path, *args, **kwargs)
            if path.name == 'arity-resume-core-factory-body-core-test-test-multimethods-1.cljs':
                return result.replace('(apply (-get-method bar [:cljs.core-test/rect :cljs.core-test/shape]) [:cljs.core-test/rect :cljs.core-test/shape])', '(bar ::rect ::shape)')
            return result
        with mock.patch.object(Path, 'read_text', changed), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_changed_original_loop_fails_closed(self):
        original = Path.read_text
        def changed(path, *args, **kwargs):
            result = original(path, *args, **kwargs)
            if path.name == 'arity-resume-public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds.cljs':
                return result.replace('(dotimes [i (count row)]', '(dotimes [i 1]')
            return result
        with mock.patch.object(Path, 'read_text', changed), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_missing_actual_run_invocation_fails_closed(self):
        original = Path.read_text
        def changed(path, *args, **kwargs):
            result = original(path, *args, **kwargs)
            return result.replace('{invoke}', 'true') if path.name == 'compiler_arity_after172.rs' else result
        with mock.patch.object(Path, 'read_text', changed), self.assertRaisesRegex(ValueError, 'whole source invocation'):
            cohort.exports()

    def test_original_exported_callback_arity_is_not_weakened(self):
        reviewed = copy.deepcopy(cohort.REVIEWED)
        row = next(row for row in reviewed if row['id'] == 'cljs-multimethods-test-multimethods-2')
        row['invoke'] = '(run)'
        with mock.patch.object(cohort, 'REVIEWED', reviewed), self.assertRaisesRegex(ValueError, 'callback invocation arity drift'):
            cohort.exports()

    def test_defn_hidden_macro_arguments_and_identity_controls_are_uncredited(self):
        exported = cohort.exports()
        self.assertFalse(any(row['obligation_id'].endswith('::defn::arity-1') or row['obligation_id'].endswith('::dispatch-fn') for row in exported['obligation-witnesses.jsonl']))
        self.assertTrue(any(row['obligation_id'].endswith('::fn~2::arity-1') for row in exported['obligation-witnesses.jsonl']))

    def test_public_signature_drift_fails_closed(self):
        selected = copy.deepcopy(cohort.SELECTION)
        next(row for row in selected if row['signature_sha256'])['signature_sha256'] = '0'*64
        with mock.patch.object(cohort, 'SELECTION', selected), self.assertRaisesRegex(ValueError, 'exact public signature drift'):
            cohort.exports()

    def test_noninvoking_arity_cannot_supply_evidence(self):
        selected = copy.deepcopy(cohort.SELECTION)
        selected[0]['direct_arities'] = [0]
        with mock.patch.object(cohort, 'SELECTION', selected), self.assertRaisesRegex(ValueError, 'actual source calls'):
            cohort.exports()

    def test_actual_frames_require_complete_ordered_success(self):
        lines = ['arity-after172-observation\t'+row['id']+'\t'+host+'\t'+phase+'\t'+engine+'\ttrue'
            for row in cohort.rows() for host in ['fresh', 'reused']
            for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
        self.assertEqual(240, len(cohort.parse_frames('\n'.join(lines))))
        for changed in [lines[:-1], list(reversed(lines)), [*lines, lines[-1]], [line.replace('\ttrue','\tfalse') for line in lines]]:
            with self.assertRaises(ValueError):
                cohort.parse_frames('\n'.join(changed))


if __name__ == '__main__':
    unittest.main()
