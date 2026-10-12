import copy, unittest
from unittest.mock import patch
import case_char_string_accepted as case

class CaseAcceptanceGuards(unittest.TestCase):
    def test_original_sci_difference_and_reviewed_complete_controls(self):
        rows,negatives=case.load()
        self.assertEqual([2,2],rows[0]['original_expected']['outcome']['value']['$vector'])
        self.assertEqual([1,1],rows[0]['canonical_expected']['outcome']['value']['$vector'])
        self.assertEqual(11,len(rows));self.assertEqual(7,len(negatives))
        self.assertIn('swap!',rows[5]['source'])
        for row in rows:
            for phase in ('application','macro'):
                self.assertIn('(do '+row['source']+')',case.proof.historical.source(row['source'],phase))

    def test_reviewed_literals_and_original_identity_fail_closed(self):
        with patch.object(case,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):case.load()
        with patch.object(case.original,'ROW_HASH','0'*64):
            with self.assertRaises(ValueError):case.load()

    def test_reconciliation_replays_raw_evidence(self):
        document={'profile':case.PROFILE,'artifacts_root':str(case.Path('/tmp/case-raw-missing').resolve())}
        with self.assertRaises(FileNotFoundError):case.reconcile(document)
        with self.assertRaises(ValueError):case.reconcile({'profile':case.PROFILE,'artifacts_root':'relative'})

if __name__=='__main__':unittest.main()
