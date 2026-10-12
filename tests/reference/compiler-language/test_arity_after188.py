import copy
from pathlib import Path
import unittest
from unittest import mock
import arity_after188 as cohort


class ArityAfter188Tests(unittest.TestCase):
    def changed_source(self, name, before, after):
        original = Path.read_text
        def changed(path, *args, **kwargs):
            result = original(path, *args, **kwargs)
            return result.replace(before, after) if path.name == name else result
        return mock.patch.object(Path, 'read_text', changed)

    def test_exact_current_census_and_no_original_site_credit(self):
        rows = cohort.exports()['obligation-witnesses.jsonl']
        self.assertEqual(20, len(rows))
        self.assertEqual(13, len(cohort.rows()))
        self.assertTrue(all(row['assertion_ids'] == [] for row in rows))

    def test_original_shuffle_hundred_values_are_not_shortened(self):
        with self.changed_source('cljs-extra-seqs-test-rearrange-sequential.cljs', '(take 100', '(take 1'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_complete_transducer_and_completion_remain(self):
        with self.changed_source('cljs-sequence-completion-core-test-test-transducers.cljs', '(mapcat flatten)', '(mapcat identity)'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_original_equiv_helper_is_actually_invoked(self):
        with self.changed_source('cljs-deftype-core.cljs', '(test-cljs-2083)', 'true'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_write_all_custom_printer_receivers_are_preserved(self):
        with self.changed_source('cljs-extension-test-cljs-2812.cljs', '(pr-str #js [1 2 3])', '"#array[3]"'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_writer_factory_apply_and_callback_this_are_preserved(self):
        with self.changed_source('public-print-writer-factories.cljs', '(apply factory [sb])', '(factory sb)'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_callback_registration_alone_gives_no_credit(self):
        selection = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'higher-order')
        with self.assertRaisesRegex(ValueError, 'absent original demanded'):
            cohort.operation_evidence('(let [callback shuffle] true)', selection, {'name': 'shuffle'})

    def test_protocol_identity_without_actual_methods_gives_no_credit(self):
        selection = next(row for row in cohort.SELECTION if row['evidence_mode'] == 'protocol-receiver')
        with self.assertRaisesRegex(ValueError, 'method/callback demand omitted'):
            cohort.operation_evidence('(satisfies? IWriter a)', selection, {'name': 'IWriter'})

    def test_missing_demanded_macro_invocation_rejects(self):
        with self.changed_source('compiler_arity_after188.rs', '{invoke}', 'true'), self.assertRaisesRegex(ValueError, 'whole source invocation'):
            cohort.exports()

    def test_original_run_callback_arity_is_frozen(self):
        reviewed = copy.deepcopy(cohort.REVIEWED)
        next(row for row in reviewed if row['id'] == 'cljs-deftype-core')['invoke'] = '(run)'
        with mock.patch.object(cohort, 'REVIEWED', reviewed), self.assertRaisesRegex(ValueError, 'callback invocation arity drift'):
            cohort.exports()

    def test_operation_hash_signature_and_actual_arity_are_frozen(self):
        for field, value, message in [('body_sha256', '0'*64, 'operation/body drift'), ('signature_sha256', '0'*64, 'signature drift'), ('direct_arities', [0], 'actual source calls')]:
            selected = copy.deepcopy(cohort.SELECTION)
            next(row for row in selected if row['evidence_mode'] == 'direct')[field] = value
            with mock.patch.object(cohort, 'SELECTION', selected), self.assertRaisesRegex(ValueError, message):
                cohort.exports()

    def test_actual_frames_require_complete_original_literal_order(self):
        lines = ['arity-after188-observation\t'+row['id']+'\t'+host+'\t'+phase+'\t'+engine+'\t'+cohort.EXPECTED_LITERAL_RAW[row['id']]
            for row in cohort.rows() for host in ['fresh', 'reused'] for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
        self.assertEqual(104, len(cohort.parse_frames('\n'.join(lines))))
        for altered in [lines[:-1], list(reversed(lines)), [*lines, lines[-1]], [line.replace('\ttrue', '\tfalse') for line in lines]]:
            with self.assertRaises(ValueError):
                cohort.parse_frames('\n'.join(altered))


if __name__ == '__main__':
    unittest.main()
