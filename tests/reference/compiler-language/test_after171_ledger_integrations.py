from copy import deepcopy
import unittest
import arity_after171 as arity
import full_ledger as ledger
import inference_optimizer_scope as optimizer
import invoke_program_scope as invoke


class After171LedgerIntegrationGuards(unittest.TestCase):
    def test_complete_arity_catalog_cannot_drop_or_change_owned_rows(self):
        _, obligations, _ = ledger.load()
        for filename in ('obligation-witnesses.jsonl', 'phase-witnesses.jsonl'):
            rows = arity.exports()[filename]
            complete = {r['obligation_id']: r for r in rows}
            ledger.validate_arity_after171(complete, filename, obligations)
            missing = deepcopy(complete); missing.pop(rows[-1]['obligation_id'])
            with self.assertRaisesRegex(ValueError, 'missing, changed or detached'):
                ledger.validate_arity_after171(missing, filename, obligations)
            changed = deepcopy(complete); changed[rows[0]['obligation_id']]['rationale'] = 'registration only'
            with self.assertRaisesRegex(ValueError, 'missing, changed or detached'):
                ledger.validate_arity_after171(changed, filename, obligations)

    def test_unrelated_unit_target_subset_does_not_require_foreign_cohort(self):
        ledger.validate_arity_after171({}, 'phase-witnesses.jsonl', [{'id': 'unrelated-unit-target'}])

    def test_optimizer_disposition_is_exact_and_supplies_no_pass(self):
        _, obligations, _ = ledger.load()
        reviewed = optimizer.validate()[0]
        actual = next(r for r in obligations if r['id'] == reviewed['id'])
        self.assertEqual(actual['body_sha256'], reviewed['body_sha256'])
        self.assertEqual('host', actual['disposition'])
        self.assertEqual(65, actual['original_predicate_forms'])
        for field in ('semantic_pass_credit', 'local_execution_credit', 'macro_phase_credit'):
            self.assertIs(False, actual[field])
        self.assertEqual(len(optimizer.validate()), 1)
        self.assertTrue(any(r['obligation_kind'] == 'public-api' and r['name'] == 'truth_'
            and r['disposition'] == 'intended' for r in obligations))

    def test_invocation_program_disposition_is_exact_and_supplies_no_pass(self):
        _, obligations, _ = ledger.load()
        reviewed = invoke.validate()[0]
        actual = next(r for r in obligations if r['id'] == reviewed['id'])
        self.assertEqual(actual['body_sha256'], reviewed['body_sha256'])
        self.assertEqual('host', actual['disposition'])
        self.assertEqual(11, actual['original_emission_assertions'])
        self.assertEqual('test-optimized-invoke-emit', actual['original_caller_test'])
        for field in ('semantic_pass_credit', 'local_execution_credit', 'macro_phase_credit'):
            self.assertIs(False, actual[field])
        self.assertEqual(len(invoke.validate()), 1)
        self.assertTrue(any(r['obligation_kind'] == 'public-api' and r['name'] == 'IFn'
            and r['disposition'] == 'intended' for r in obligations))


if __name__ == '__main__':
    unittest.main()
