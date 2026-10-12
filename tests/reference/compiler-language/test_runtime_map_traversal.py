"""Immutable complete sources, exact typed order and fail-closed map contracts."""
import copy
import json
import unittest
from unittest.mock import patch
import runtime_map_traversal as runtime

class RuntimeMapTraversalGuards(unittest.TestCase):
    def test_original_sources_and_full_sequences(self):
        rows=runtime.load(); self.assertEqual(64,len(rows))
        historical=json.loads(runtime.CONTRACTS.with_name('literal-map-traversal-contracts.json').read_text())
        self.assertEqual(historical['sources'],[{k:r[k] for k in ('id','source','source_sha256')} for r in rows[:39]])
        witness=next(r for r in rows if r['id']=='literal-map-traversal-runtime-hash-map-sequence-nonpassing')
        self.assertEqual(4,sum(not r['canonical_acceptance'] for r in rows))
        self.assertEqual(0,sum(not r['two_pin_agreement'] for r in rows))
        self.assertTrue(witness['canonical_acceptance'])
        self.assertEqual(witness['canonical_expected'],witness['local_expected'])
        self.assertEqual(list('egchbdfia'),[p['$vector'][0]['$keyword']['name'] for p in witness['canonical_expected']['outcome']['value']['$vector']])

    def test_exact_comparison_rejects_sequence_effect_and_stdout_changes(self):
        row=next(r for r in runtime.load() if r['id']=='literal-map-traversal-runtime-hash-map-sequence-nonpassing')
        for field in ('sequence','stdout','trace'):
            changed=copy.deepcopy(row['canonical_expected'])
            if field=='sequence':changed['outcome']['value']['$vector'].reverse()
            elif field=='stdout':changed['stdout']='extra\n'
            else:changed['trace']=[1]
            self.assertEqual('mismatch',runtime.exact_observation(row,changed)['status'])

    def test_contract_drift_and_missing_raw_proof_fail_closed(self):
        with patch.object(runtime,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):runtime.load()
        with self.assertRaises(ValueError):runtime.reconcile({'profile':runtime.PROFILE,'artifacts_root':'relative'})
        with self.assertRaises(FileNotFoundError):runtime.reconcile({'profile':runtime.PROFILE,'artifacts_root':'/private/tmp/missing-runtime-map-proof-181'})

if __name__=='__main__':unittest.main()
