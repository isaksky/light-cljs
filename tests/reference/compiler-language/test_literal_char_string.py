"""Source-bound literal proof guards; raw evidence is mandatory."""
import copy, json, unittest
from unittest.mock import patch
import literal_char_string as literal

class LiteralSourceGuards(unittest.TestCase):
    def test_exact_original_and_reviewed_controls(self):
        rows,negative=literal.load()
        self.assertEqual(literal.ORIGINAL,rows[0]['source'])
        self.assertEqual({'$vector':[2,1,2,True]},rows[0]['canonical_expected']['outcome']['value'])
        self.assertEqual(30,len(rows));self.assertEqual(11,len(negative))
        self.assertEqual(27,sum(row['canonical_acceptance'] for row in rows))
        for row in rows:
            self.assertEqual(literal.sci.digest(row['source']),row['source_sha256'])
            for phase in ('application','macro'):
                self.assertIn('(do '+row['source']+')',literal.proof.historical.source(row['source'],phase))
        by_id={row['id']:row for row in rows}
        self.assertIn('with-out-str',by_id['literal-char-string-map-effects']['source'])
        self.assertEqual('2\n11\n',by_id['literal-char-string-map-effects']['canonical_expected']['outcome']['value']['$vector'][-1])

    def test_pin_and_traversal_differences_are_nonpassing(self):
        rows,_=literal.load();different=[row for row in rows if not row['canonical_acceptance']]
        self.assertEqual(3,len(different))
        self.assertEqual(2,sum(not row['two_pin_agreement'] for row in rows))
        for row in different:
            self.assertNotEqual(row['canonical_expected'],row['local_expected'])
        numeric=next(row for row in rows if row['id'].endswith('numeric-literals'))
        self.assertTrue(numeric['two_pin_agreement']);self.assertTrue(numeric['canonical_acceptance'])
        self.assertEqual(numeric['canonical_expected'],numeric['local_expected'])

    def test_reviewed_contract_hash_fails_closed(self):
        with patch.object(literal,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):literal.load()

    def test_reconciliation_requires_normalized_complete_raw_artifacts(self):
        with self.assertRaises(ValueError):literal.reconcile({'profile':literal.PROFILE,'artifacts_root':'relative'})
        with self.assertRaises(FileNotFoundError):literal.reconcile({'profile':literal.PROFILE,'artifacts_root':str(literal.Path('/tmp/literal-char-string-missing-raw').resolve())})
        with self.assertRaises(ValueError):literal.reconcile({'profile':'detached'})

if __name__=='__main__':unittest.main()
