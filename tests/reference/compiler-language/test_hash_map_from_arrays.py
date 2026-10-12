"""Keep full factory expectations and actual raw execution evidence fail-closed."""
from copy import deepcopy
from pathlib import Path
from unittest.mock import patch
import json
import os
import unittest
import hash_map_from_arrays as factory


class HashMapFromArrays(unittest.TestCase):
    def test_original_source_and_complete_packets_remain_exact(self):
        rows=factory.rows()
        self.assertEqual('3058d5292009a7bf86efa2e24ef9c3f60045c86e3ee548077f3a894bf8907dc0',rows[0]['source_sha256'])
        self.assertEqual(12,len(rows))
        changed=deepcopy(rows[0]['canonical_expected'])
        changed['outcome']['value']['$vector'][1]['$vector'].reverse()
        self.assertEqual('mismatch',factory.runtime.exact_observation(rows[0],changed)['status'])
        for field,value in [('trace',[1]),('stdout','invented')]:
            changed=deepcopy(rows[0]['canonical_expected']);changed[field]=value
            self.assertEqual('mismatch',factory.runtime.exact_observation(rows[0],changed)['status'])
        with patch.object(factory,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):factory.rows()

    def test_both_pins_declare_factory_and_missing_value_check(self):
        for row in factory.declarations().values():
            self.assertIn('(fn [ks vs]',row['declaration'])
            self.assertIn('No value supplied for key:',row['declaration'])

    @unittest.skipUnless(os.environ.get('FROM_ARRAYS_RECEIPT'),'requires fresh executable/raw proof')
    def test_raw_mutations_fail_even_with_refreshed_artifact_hashes(self):
        document=json.loads(Path(os.environ['FROM_ARRAYS_RECEIPT']).read_text())
        factory.reconcile(document)
        root=Path(document['artifacts_root']);binding_path=root/'binding.json';saved=binding_path.read_bytes()
        prefix='phases/'+factory.IDENTITY+'/macro-reused/'
        paths=['release/node/'+factory.IDENTITY+'/stdout','checkout/original-sources/'+factory.IDENTITY+'/compiled.js']
        paths += [prefix+p for p in ('user.cljc','compiled.js','quickjs.raw','node.stdin','node.stdout')]
        paths += ['phases/audit/command.json']
        for relative in paths:
            with self.subTest(artifact=relative):
                path=root/relative;original=path.read_bytes()
                try:
                    path.write_bytes(original+b'\nTAMPER\n')
                    binding=json.loads(saved);binding['artifact_sha256'][relative]=factory.sci.digest(path.read_bytes())
                    binding_path.write_text(json.dumps(binding,indent=2)+'\n')
                    with self.assertRaises((ValueError,json.JSONDecodeError)):factory.verified_document(root)
                finally:path.write_bytes(original);binding_path.write_bytes(saved)
        for field in ('audit_environment','executables_sha256'):
            try:
                binding=json.loads(saved)
                if field=='audit_environment':binding[field]['NODE']='/tmp/unbound-node'
                else:binding[field][binding['audit_executable']]='0'*64
                binding_path.write_text(json.dumps(binding,indent=2)+'\n')
                with self.assertRaises(ValueError):factory.verified_document(root)
            finally:binding_path.write_bytes(saved)
        changed=deepcopy(document);changed['full_language_completion_credit']=1
        with self.assertRaises(ValueError):factory.reconcile(changed)
        path=root/prefix/'node.stdout';original=path.read_bytes()
        try:
            path.unlink()
            with self.assertRaises((ValueError,FileNotFoundError)):factory.verified_document(root)
        finally:path.write_bytes(original)
        self.assertEqual(document,factory.verified_document(root))


if __name__=='__main__':unittest.main()
