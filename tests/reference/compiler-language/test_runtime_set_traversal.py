"""Original source retention, exact traversal, and scoped neighboring controls."""
import copy
import unittest
from unittest.mock import patch
import runtime_set_traversal as runtime

class RuntimeSetTraversalGuards(unittest.TestCase):
    def test_original_sources_and_full_sequences(self):
        rows=runtime.load(); self.assertEqual(36,len(rows))
        by_id={r['id']:r for r in rows}
        for name in ('sixteen-sequence-nonpassing','runtime-only-sequence-nonpassing'):
            row=by_id['literal-set-traversal-'+name]
            self.assertTrue(row['canonical_acceptance'])
            self.assertEqual(row['canonical_expected'],row['local_expected'])
        full=by_id['literal-set-traversal-sixteen-sequence-nonpassing']['canonical_expected']['outcome']['value']['$vector']
        self.assertEqual([16,True,True],full[:3])
        self.assertEqual(list(range(16)),full[3]['$vector'])
        self.assertEqual([14,3,12,9,2,15,5,10,7,0,13,8,1,6,4,11],full[4]['$vector'])
        self.assertEqual('16\n',full[5])
        for name in ('dynamic-duplicate','dynamic-vector-duplicate'):
            row=by_id['literal-set-traversal-'+name]
            self.assertFalse(row['canonical_acceptance'])
            self.assertFalse(row['two_pin_agreement'])
            self.assertNotEqual(row['canonical_expected'],row['checkout_expected'])

    def test_exact_comparison_rejects_complete_sequence_changes(self):
        row=next(r for r in runtime.load() if r['id']=='literal-set-traversal-runtime-only-sequence-nonpassing')
        changed=copy.deepcopy(row['canonical_expected'])
        changed['outcome']['value']['$vector'].reverse()
        self.assertEqual('mismatch',runtime.exact_observation(row,changed)['status'])
        changed=copy.deepcopy(row['canonical_expected']); changed['stdout']='extra\n'
        self.assertEqual('mismatch',runtime.exact_observation(row,changed)['status'])
        changed=copy.deepcopy(row['canonical_expected']); changed['trace']=[1]
        self.assertEqual('mismatch',runtime.exact_observation(row,changed)['status'])

    def test_contract_and_original_drift_fail_closed(self):
        with patch.object(runtime,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):runtime.load()
        with patch.object(runtime,'ORIGINAL',runtime.ORIGINAL+' '):
            with self.assertRaises(ValueError):runtime.load()

    def test_missing_raw_proof_fails_closed(self):
        with self.assertRaises(ValueError):runtime.reconcile({'profile':runtime.PROFILE,'artifacts_root':'relative'})
        with self.assertRaises(FileNotFoundError):runtime.reconcile({'profile':runtime.PROFILE,'artifacts_root':'/private/tmp/missing-runtime-set-proof-180'})

if __name__=='__main__':unittest.main()
