import copy
import json
from pathlib import Path
import unittest
from unittest import mock
import arity_resume_extended as cohort


class ArityResumeExtendedTests(unittest.TestCase):
    def test_complete_sources_exact_operation_rows_and_receiver_demands(self):
        exported = cohort.exports()
        self.assertEqual(35, len(exported['obligation-witnesses.jsonl']))
        self.assertEqual(10, len(exported['full-cases.jsonl']))
        receipts = exported['arity-extended-call-receipts.jsonl']
        self.assertEqual(3, sum(row['receiver_demand'] is not None for row in receipts))
        self.assertFalse(any(row['obligation_id'].endswith('::-key->js') for row in receipts))
        for mapping in exported['obligation-witnesses.jsonl']:
            case = next(case for case in exported['full-cases.jsonl'] if case['id'] == mapping['reference_fixture_id'])
            self.assertEqual(cohort.digest(case['source']), mapping['reference_source_sha256'])
            self.assertEqual(cohort.digest(json.dumps(case['expected'], sort_keys=True)), mapping['reference_contract_sha256'])
            self.assertEqual([], mapping['assertion_ids'])

    def test_method_definition_without_converter_demand_has_zero_credit(self):
        row = next(row for row in cohort.rows() if row['id'] == 'language-helper-js-conversions')
        row = {**row, 'source': row['source'].replace('(js->clj custom :keywordize-keys true)', 'true')}
        proof = next(selected['receiver_dispatch'] for selected in cohort.SELECTION if selected['obligation_id'].endswith('::-js->clj::arity-1'))
        with self.assertRaisesRegex(ValueError, 'not actually demanded'):
            cohort.receiver_demand(row, proof)

    def test_unused_key_encoder_method_cannot_supply_receiver_credit(self):
        row = next(row for row in cohort.rows() if row['id'] == 'language-helper-js-conversions')
        proof = {'method': '-key->js', 'caller': 'key->js', 'receiver': 'out', 'caller_argument_count': 1}
        with self.assertRaisesRegex(ValueError, 'not actually demanded'):
            cohort.receiver_demand(row, proof)

    def test_changed_receiver_options_are_rejected(self):
        row = next(row for row in cohort.rows() if row['id'] == 'language-helper-js-conversions')
        row = {**row, 'source': row['source'].replace(':keywordize-keys true', ':keywordize-keys false')}
        proof = next(selected['receiver_dispatch'] for selected in cohort.SELECTION if selected['obligation_id'].endswith('::-js->clj::arity-1'))
        with self.assertRaisesRegex(ValueError, 'not actually demanded'):
            cohort.receiver_demand(row, proof)

    def test_changed_original_loops_or_literals_fail_closed(self):
        original = Path.read_text
        def altered(path, *args, **kwargs):
            text = original(path, *args, **kwargs)
            if str(path).endswith('compiler-core-helpers.tsv'):
                text = text.replace('(trampoline step 50000 0)', '(trampoline step 50 0)')
            return text
        with mock.patch.object(Path, 'read_text', altered), self.assertRaisesRegex(ValueError, 'whole original helper row drift'):
            cohort.exports()

    def test_actual_phase_parser_requires_every_ordered_frame(self):
        lines = ['arity-extended-observation\t'+row['id']+'\t'+host+'\t'+phase+'\t'+engine+'\ttrue'
            for row in cohort.rows() for host in ['fresh', 'reused']
            for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
        self.assertEqual(80, len(cohort.parse_frames('\n'.join(lines))))
        for changed in [lines[:-1], list(reversed(lines)), [*lines, lines[-1]], [line.replace('\ttrue', '\tfalse') for line in lines]]:
            with self.assertRaises(ValueError):
                cohort.parse_frames('\n'.join(changed))

    def test_drifted_public_arity_cannot_be_exported(self):
        changed = copy.deepcopy(cohort.SELECTION)
        changed[0]['signature_sha256'] = '0'*64
        with mock.patch.object(cohort, 'SELECTION', changed), self.assertRaisesRegex(ValueError, 'exact public signature drift'):
            cohort.exports()


if __name__ == '__main__':
    unittest.main()
