import unittest
from unittest.mock import patch
import case_char_string_gap as gap


class CaseCharStringGapGuards(unittest.TestCase):
    def test_exact_original_and_canonical_contrast(self):
        row=gap.load()
        self.assertEqual([2,2],row['original_expected']['outcome']['value']['$vector'])
        self.assertEqual([1,1],row['canonical_expected']['outcome']['value']['$vector'])
        for phase in ('application','macro'):
            self.assertIn('(do '+gap.SOURCE+')',gap.module(phase))
        self.assertIn('(upstream)',gap.module('macro'))

    def test_original_row_binding_cannot_drift(self):
        with patch.object(gap,'ROW_HASH','0'*64):
            with self.assertRaises(ValueError):gap.load()
        with patch.object(gap,'BODY_HASH','0'*64):
            with self.assertRaises(ValueError):gap.load()

    def test_unknown_phase_rejects(self):
        with self.assertRaises(ValueError):gap.module('unknown')


if __name__=='__main__':unittest.main()
