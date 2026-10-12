"""Numeric evidence must retain its literal/source/executable binding."""
from copy import deepcopy
import json, os, unittest
from pathlib import Path
from unittest.mock import patch
import case_numeric as numeric


class NumericCaseGuards(unittest.TestCase):
    def test_complete_independent_catalog_keeps_number_execution_distinct(self):
        accepted, rejected = numeric.load()
        self.assertEqual('(case 1 1 :integer 1.0 :float)', accepted[0]['source'])
        self.assertEqual('integer', accepted[0]['canonical_expected']['outcome']['value']['$keyword']['name'])
        self.assertEqual((16, 11), (len(accepted), len(rejected)))
        self.assertTrue(any('9223372036854775809N' in row['source'] for row in accepted))
        self.assertTrue(any('1.0000000000000001M' in row['source'] for row in accepted))
        self.assertTrue(any('0.0' in row['source'] for row in rejected))

    def test_changed_contract_or_summary_has_no_positive_fallback(self):
        with patch.object(numeric, 'CONTRACTS_SHA256', '0'*64):
            with self.assertRaises(ValueError): numeric.load()
        with self.assertRaises(ValueError): numeric.reconcile({'profile': numeric.PROFILE, 'artifacts_root': 'relative'})
        with self.assertRaises(FileNotFoundError): numeric.reconcile({'profile': numeric.PROFILE, 'artifacts_root': str(Path('/tmp/missing-numeric-case-proof').resolve())})

    @unittest.skipUnless(os.environ.get('CASE_NUMERIC_RECEIPT'), 'requires fresh raw numeric proof')
    def test_actual_raw_replay_rejects_rehashed_source_packet_and_recovery_tampering(self):
        document = json.loads(Path(os.environ['CASE_NUMERIC_RECEIPT']).read_text()); root = Path(document['artifacts_root'])
        self.assertEqual(document, numeric.reconcile(document))
        binding_path = root/'binding.json'; saved_binding = binding_path.read_bytes()
        identity = document['source_bindings'][0]['id']; negative = document['diagnostic_sources'][0]['id']
        for relative in [f'phases/{identity}/macro-reused/user.cljc', f'phases/{identity}/application-fresh/quickjs.raw',
                         f'release/original-sources/{identity}/compiled.js', f'diagnostics/{negative}/macro-reused/recovery.cljs',
                         'phases/audit/command.json']:
            path = root/relative; saved = path.read_bytes()
            try:
                path.write_bytes(saved+b'\nTAMPER\n')
                binding = json.loads(saved_binding); binding['artifact_sha256'][relative] = numeric.sci.digest(path.read_bytes())
                binding_path.write_text(json.dumps(binding, indent=2)+'\n')
                with self.assertRaises((ValueError, json.JSONDecodeError)): numeric.reconcile(document)
            finally:
                path.write_bytes(saved); binding_path.write_bytes(saved_binding)
        changed = deepcopy(document); changed['status'] = 'complete'
        with self.assertRaises(ValueError): numeric.reconcile(changed)
        self.assertEqual(document, numeric.reconcile(document))


if __name__ == '__main__': unittest.main()
