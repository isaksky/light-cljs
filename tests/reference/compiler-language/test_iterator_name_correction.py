"""Keep invalid canonical-name evidence fail-closed, including raw recensus."""
from copy import deepcopy
from pathlib import Path
from unittest.mock import patch
import json
import os
import unittest
import iterator_name_correction as correction


class IteratorNameCorrection(unittest.TestCase):
    def test_frozen_whole_source_is_thrown_and_zero_credit(self):
        row = correction.rows()[0]
        self.assertEqual('9c0082c2dcd72d457b2994cceb77103567539d88d929beb3f4d524f153f23fd4',row['source_sha256'])
        self.assertEqual('threw',row['canonical_expected']['outcome']['status'])
        self.assertFalse(row['canonical_acceptance'])
        changed = deepcopy(row['canonical_expected'])
        changed['outcome'] = {'status':'returned','value':[]}
        self.assertEqual('mismatch',correction.runtime.exact_observation(row,changed)['status'])
        with patch.object(correction.runtime,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):correction.rows()

    def test_both_full_core_sources_omit_unqualified_name(self):
        for declaration in correction.declarations().values():
            self.assertFalse(declaration['iterator_seq_declared'])
            self.assertTrue(declaration['canonical_helper'].startswith('(defn es6-iterator-seq'))

    @unittest.skipUnless(os.environ.get('ITERATOR_NAME_RECEIPT'),'requires fresh exact raw proof')
    def test_raw_mutations_fail_even_after_artifact_recensus(self):
        document = json.loads(Path(os.environ['ITERATOR_NAME_RECEIPT']).read_text())
        correction.reconcile(document)
        root = Path(document['artifacts_root'])
        binding_path = root/'binding.json'
        binding_bytes = binding_path.read_bytes()
        prefix = 'phases/'+correction.IDENTITY+'/macro-reused/'
        for relative in ('release/node/'+correction.IDENTITY+'/stdout','release/build/stderr',prefix+'user.cljc',prefix+'rejection.txt','phases/audit/command.json'):
            path = root/relative
            original = path.read_bytes()
            try:
                path.write_bytes(original+b'\nTAMPER\n')
                binding = json.loads(binding_bytes)
                binding['artifact_sha256'][relative] = correction.sci.digest(path.read_bytes())
                binding_path.write_text(json.dumps(binding,indent=2)+'\n')
                with self.assertRaises((ValueError,json.JSONDecodeError)):
                    correction.verified_document(root)
            finally:
                path.write_bytes(original)
                binding_path.write_bytes(binding_bytes)
        changed = deepcopy(document)
        changed['original_semantic_pass_credit'] = 1
        with self.assertRaises(ValueError):correction.reconcile(changed)
        missing = root/'release/node'/correction.IDENTITY/'stdout'
        original = missing.read_bytes()
        try:
            missing.unlink()
            with self.assertRaises((ValueError,FileNotFoundError)):correction.verified_document(root)
        finally:
            missing.write_bytes(original)
        self.assertEqual(document,correction.verified_document(root))


if __name__ == '__main__':unittest.main()
