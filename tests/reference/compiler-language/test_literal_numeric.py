"""Source, numeric identity, and fail-closed raw proof guards."""
import copy
import unittest
from unittest.mock import patch
import literal_numeric as literal


class NumericLiteralSourceGuards(unittest.TestCase):
    def test_exact_original_and_complete_independent_catalog(self):
        rows,negative=literal.load()
        self.assertEqual(literal.ORIGINAL,rows[0]['source'])
        self.assertEqual([42,26,40],[len(rows),len(negative),sum(r['canonical_acceptance'] for r in rows)])
        self.assertEqual(2,sum(not r['two_pin_agreement'] for r in rows))
        for row in rows+negative:
            self.assertEqual(literal.sci.digest(row['source']),row['source_sha256'])
            for phase in ('application','macro'):
                self.assertIn('(do '+row['source']+')',literal.proof.historical.source(row['source'],phase))
        by_id={r['id']:r for r in rows}
        original=by_id['literal-numeric-original']['canonical_expected']['outcome']['value']['$vector']
        self.assertEqual([2,2,{'$vector':[1,1]}],[original[0],original[2],original[3]])
        self.assertEqual('int',original[1]['$keyword']['name'])
        self.assertEqual('2\n11\n',by_id['literal-numeric-map-effects']['canonical_expected']['outcome']['value']['$vector'][-1])
        self.assertEqual('2\n',by_id['literal-numeric-set-effects']['canonical_expected']['outcome']['value']['$vector'][-1])
        self.assertIn('##NaN',by_id['literal-numeric-collision-and-nan-count']['source'])
        self.assertTrue(by_id['literal-numeric-collision-and-nan-count']['canonical_acceptance'])
        self.assertEqual([3,False],by_id['literal-numeric-collision-and-nan-count']['canonical_expected']['outcome']['value']['$vector'][-2:])

    def test_pin_and_traversal_gaps_stay_nonpassing(self):
        rows,_=literal.load();gaps=[r for r in rows if not r['canonical_acceptance']]
        self.assertEqual({'dynamic-set','vector-set-members'},
                         {r['id'].removeprefix('literal-numeric-') for r in gaps})
        for row in gaps:
            self.assertTrue(row['nonpassing_reason'])
            self.assertNotEqual(row['canonical_expected'],row['local_expected'])
        for row in rows:
            if row['canonical_acceptance']:
                self.assertTrue(row['two_pin_agreement'])
                self.assertEqual(row['canonical_expected'],row['local_expected'])

    def test_numeric_categories_thresholds_and_diagnostics_are_retained(self):
        rows,negative=literal.load();by_id={r['id']:r for r in rows}
        val=lambda name:by_id['literal-numeric-'+name]['canonical_expected']['outcome']['value']['$vector']
        self.assertEqual(3,val('decimal-categories')[0])
        self.assertEqual(8,val('map-eight-pairs')[0])
        self.assertEqual(8,val('map-nine-pairs')[0])
        self.assertEqual('int',val('map-nine-pairs')[1]['$keyword']['name'])
        self.assertEqual('int',val('map-nine-reverse')[1]['$keyword']['name'])
        self.assertEqual('decimal',val('map-ten-categories')[1]['$keyword']['name'])
        self.assertEqual({'$number':'-0'},val('zero-integer-float')[-1]['$vector'][-1])
        self.assertEqual([2,2,False],val('repeated-nan'))
        for name in ('metadata-wide-map','metadata-wide-set'):
            row=by_id['literal-numeric-'+name]
            self.assertEqual(1,row['original_expected']['outcome']['value'])
            self.assertEqual(2,row['canonical_expected']['outcome']['value'])
            self.assertTrue(row['historical_expectation_basis'])
        self.assertEqual(26,len({r['id'] for r in negative}))
        self.assertEqual('number is not a valid ClojureScript constant',negative[-1]['local_diagnostic'])
        self.assertEqual(2,sum(not r['canonical_diagnostic_agreement'] for r in negative))
        self.assertTrue(literal.diagnostic_matches('number is not a valid ClojureScript constant','Lower(NumericOverflow)'))

    def test_contract_hash_and_source_drift_fail_closed(self):
        with patch.object(literal,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):literal.load()
        changed=list(literal.CONTROLS);changed[0]=(changed[0][0],changed[0][1]+' ')
        with patch.object(literal,'CONTROLS',tuple(changed)):
            with self.assertRaises(ValueError):literal.load()

    def test_reconciliation_requires_current_complete_raw_artifacts(self):
        with self.assertRaises(ValueError):literal.reconcile({'profile':literal.PROFILE,'artifacts_root':'relative'})
        with self.assertRaises(FileNotFoundError):literal.reconcile({'profile':literal.PROFILE,'artifacts_root':str(literal.Path('/tmp/literal-numeric-missing-raw').resolve())})
        with self.assertRaises(ValueError):literal.reconcile({'profile':'detached'})


if __name__=='__main__':unittest.main()
