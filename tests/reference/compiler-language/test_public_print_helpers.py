"""Exact printing contracts and raw/executable fail-closed guards."""
import copy,json,os,unittest
from pathlib import Path
from unittest.mock import patch
import public_print_helpers as helpers

class ContractGuards(unittest.TestCase):
    def test_original_callers_and_checked_nil_difference_remain_exact(self):
        rows=helpers.load()
        self.assertEqual(10,len(rows))
        self.assertEqual(9,sum(r['canonical_acceptance'] for r in rows))
        self.assertEqual('type-error',rows[-1]['canonical_expected']['outcome']['value']['$vector'][0])
        self.assertEqual('error',rows[-1]['local_expected']['outcome']['value']['$vector'][0])
    def test_unreviewed_contract_and_effect_packet_fail_closed(self):
        with patch.object(helpers,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):helpers.load()
        row=helpers.load()[0]
        for field in ('outcome','trace','stdout'):
            raw=copy.deepcopy(row['canonical_expected'])
            raw[field]=None
            self.assertEqual('mismatch',helpers.exact_observation(row,raw)['status'])
        with self.assertRaises(ValueError):helpers.reconcile({'profile':helpers.PROFILE,'artifacts_root':'relative'})

@unittest.skipUnless(os.environ.get('PUBLIC_PRINT_RECEIPT'),'requires fresh accepted executable proof')
class RawReplayGuards(unittest.TestCase):
    def test_changed_raw_units_sources_commands_and_summary_fail_after_rehash(self):
        document=json.loads(Path(os.environ['PUBLIC_PRINT_RECEIPT']).read_text()); root=Path(document['artifacts_root'])
        binding_path=root/'binding.json'; original=binding_path.read_bytes()
        prefix='phases/'+document['source_bindings'][0]['id']+'/macro-reused/'
        paths=[prefix+n for n in ('user.cljc','compiled.js','script.js','quickjs.raw','node.stdin','node.stdout')]+['phases/audit/command.json']
        for name in paths:
            with self.subTest(artifact=name):
                path=root/name; saved=path.read_bytes()
                try:
                    path.write_bytes(saved+b'\nTAMPER\n'); binding=json.loads(original)
                    binding['artifact_sha256'][name]=helpers.sci.digest(path.read_bytes())
                    binding_path.write_text(json.dumps(binding,indent=2)+'\n')
                    with self.assertRaises((ValueError,json.JSONDecodeError)):helpers.verified_document(root)
                finally:path.write_bytes(saved); binding_path.write_bytes(original)
        mutated=copy.deepcopy(document);mutated['full_language_completion_credit']=1
        with self.assertRaises(ValueError):helpers.reconcile(mutated)
        self.assertEqual(document,helpers.reconcile(document))

if __name__=='__main__':unittest.main()
