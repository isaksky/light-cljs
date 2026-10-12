from copy import deepcopy
import unittest
from unittest.mock import patch
import sci_gap_return_candidates as cohort


class GapReturnCandidateTests(unittest.TestCase):
    def test_complete_originals_keep_expectations_and_zero_phase_credit(self):
        cases = cohort.load()
        self.assertEqual(35, len(cases))
        self.assertEqual(len(cases), len({row['obligation_id'] for row in cases}))
        originals = cohort.sci.rows(cohort.sci.CORPUS)
        for row in cases:
            self.assertEqual(originals[row['sci_id']][1][2], row['source'])
            self.assertEqual(cohort.sci.expected(originals[row['sci_id']][1]), row['expected'])
            self.assertFalse(row['semantic_pass_credit'])
            self.assertEqual('not-run', row['macro_phase_status'])

    def test_changed_source_and_disposition_fail_closed(self):
        actual = cohort.sci.rows
        def changed(path):
            rows = deepcopy(actual(path))
            if path == cohort.sci.DISPOSITIONS:
                identity = next(iter(cohort.REVIEWED))
                line, fields = rows[identity]
                fields[2] = 'true'
                rows[identity] = ('\t'.join(fields), fields)
            return rows
        with patch.object(cohort.sci, 'rows', changed), self.assertRaisesRegex(ValueError, 'candidate row/profile drift'):
            cohort.load()

    def test_changed_original_result_fails_closed(self):
        actual = cohort.sci.rows
        def changed(path):
            rows = deepcopy(actual(path))
            if path == cohort.sci.CORPUS:
                identity = next(iter(cohort.REVIEWED))
                line, fields = rows[identity]
                fields[3] = '(return nil)'
                rows[identity] = ('\t'.join(fields), fields)
            return rows
        with patch.object(cohort.sci, 'rows', changed), self.assertRaisesRegex(ValueError, 'candidate row/profile drift'):
            cohort.load()


if __name__ == '__main__':
    unittest.main()
