"""Exact source, independently frozen order, and raw replay guards."""
import unittest
from unittest.mock import patch
import literal_set_traversal as literal


class LiteralSetTraversalGuards(unittest.TestCase):
    def test_complete_source_catalog_and_original_order(self):
        rows=literal.load()
        self.assertEqual(20,len(rows))
        self.assertEqual(literal.ORIGINAL,rows[0]['source'])
        self.assertEqual(literal.ORIGINAL_FULL,rows[1]['source'])
        for row in rows:
            self.assertEqual(literal.sci.digest(row['source']),row['source_sha256'])
            self.assertEqual([],row['canonical_expected']['trace'])
            self.assertEqual('',row['canonical_expected']['stdout'])
            for phase in ('application','macro'):
                self.assertIn('(do '+row['source']+')',literal.proof.historical.source(row['source'],phase))
        self.assertEqual([4,True,True],rows[0]['canonical_expected']['outcome']['value']['$vector'])
        self.assertEqual(['a','b','a'],
                         [v for v in rows[1]['canonical_expected']['outcome']['value']['$vector'][-1]['$vector'] if isinstance(v,str)])

    def test_nested_types_effects_and_version_differences(self):
        rows={r['id'].removeprefix('literal-set-traversal-'):r for r in literal.load()}
        for name in ('scalar-mix','quote-list','vectors','maps','nested-sets','all-composites','nested-quoted-data','large-nil-first','same-hash-strings','negative-float-scalars','parse-int-source-spans','sixteen-effect-members'):
            self.assertTrue(rows[name]['canonical_acceptance'])
            self.assertTrue(rows[name]['two_pin_agreement'])
        effects=rows['effects']['canonical_expected']['outcome']['value']['$vector']
        self.assertEqual([3,{'$vector':['b',1,'a']}],effects[:2])
        self.assertEqual('3\n',effects[-1])
        self.assertEqual(['third','first','second'],[v['$keyword']['name'] for v in effects[-2]['$vector']])
        self.assertEqual({'$vector':[2,1]},rows['parse-int-source-spans']['canonical_expected']['outcome']['value'])
        large=rows['large-nil-first']['canonical_expected']['outcome']['value']['$vector']
        self.assertEqual([12,True,True,True],large[:4])
        self.assertIsNone(large[-1]['$vector'][0])
        sixteen=rows['sixteen-effect-members']['canonical_expected']['outcome']['value']['$vector']
        self.assertEqual([16,True,True],sixteen[:3])
        self.assertEqual([14,3,12,9,2,15,5,10,7,0,13,8,1,6,4,11],sixteen[3]['$vector'])
        self.assertEqual('16\n',sixteen[4])
        full=rows['sixteen-sequence-nonpassing']
        self.assertFalse(full['canonical_acceptance'])
        self.assertTrue(full['two_pin_agreement'])
        self.assertEqual(list(range(16)),full['canonical_expected']['outcome']['value']['$vector'][3]['$vector'])
        self.assertEqual([14,3,12,9,2,15,5,10,7,0,13,8,1,6,4,11],full['local_expected']['outcome']['value']['$vector'][3]['$vector'])
        runtime=rows['runtime-only-sequence-nonpassing']
        self.assertFalse(runtime['canonical_acceptance'])
        self.assertTrue(runtime['two_pin_agreement'])
        self.assertEqual(list(range(16)),runtime['canonical_expected']['outcome']['value']['$vector'])
        for name in ('dynamic-duplicate','dynamic-vector-duplicate'):
            row=rows[name]
            self.assertFalse(row['canonical_acceptance'])
            self.assertFalse(row['two_pin_agreement'])
            self.assertTrue(row['nonpassing_reason'])
            self.assertNotEqual(row['canonical_expected'],row['checkout_expected'])

    def test_contract_hash_and_source_drift_fail_closed(self):
        with patch.object(literal,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):literal.load()
        with patch.object(literal,'ORIGINAL',literal.ORIGINAL+' '):
            with self.assertRaises(ValueError):literal.load()

    def test_reconciliation_requires_raw_artifacts(self):
        with self.assertRaises(ValueError):literal.reconcile({'profile':literal.PROFILE,'artifacts_root':'relative'})
        missing=str(literal.Path('/tmp/literal-set-traversal-missing-raw').resolve())
        with self.assertRaises(FileNotFoundError):literal.reconcile({'profile':literal.PROFILE,'artifacts_root':missing})


if __name__=='__main__':unittest.main()
