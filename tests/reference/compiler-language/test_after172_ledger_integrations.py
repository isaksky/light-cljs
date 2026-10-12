from copy import deepcopy
import unittest
import arity_after172 as arity
import full_ledger as ledger


class After172LedgerIntegrationGuards(unittest.TestCase):
    def test_complete_new_arity_catalog_cannot_drop_or_change_owned_rows(self):
        _, obligations, _ = ledger.load()
        for filename in ('obligation-witnesses.jsonl', 'phase-witnesses.jsonl'):
            rows = arity.exports()[filename]
            complete = {r['obligation_id']: r for r in rows}
            ledger.validate_arity_after172(complete, filename, obligations)
            missing = deepcopy(complete); missing.pop(rows[-1]['obligation_id'])
            with self.assertRaisesRegex(ValueError, 'missing, changed or detached'):
                ledger.validate_arity_after172(missing, filename, obligations)
            changed = deepcopy(complete); changed[rows[0]['obligation_id']]['rationale'] = 'registration only'
            with self.assertRaisesRegex(ValueError, 'missing, changed or detached'):
                ledger.validate_arity_after172(changed, filename, obligations)

    def test_unrelated_target_subset_requires_no_new_cohort(self):
        ledger.validate_arity_after172({}, 'phase-witnesses.jsonl', [{'id': 'unrelated-unit-target'}])


if __name__ == '__main__':
    unittest.main()
