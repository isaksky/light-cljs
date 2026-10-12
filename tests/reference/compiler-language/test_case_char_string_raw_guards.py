"""Exercise the actual raw/executable replayer with self-consistent artifact hashes."""
from copy import deepcopy
import json
import os
from pathlib import Path
import unittest
import case_char_string_accepted as case


@unittest.skipUnless(os.environ.get('CASE_CHAR_STRING_RECEIPT'), 'requires fresh executable case acceptance')
class CaseRawReplayGuards(unittest.TestCase):
    def setUp(self):
        self.document = json.loads(Path(os.environ['CASE_CHAR_STRING_RECEIPT']).read_text())
        self.root = Path(self.document['artifacts_root'])
        case.reconcile(self.document)

    def test_altered_sources_units_packets_commands_and_recovery_fail_even_with_rehashed_artifacts(self):
        identity = self.document['source_bindings'][0]['id']
        local = 'phases/' + identity + '/macro-reused/'
        pin = 'release/original-sources/' + identity + '/'
        negative = 'diagnostics/case-rejection-duplicate-characters/macro-reused/'
        paths = [local + name for name in ('user.cljc', 'compiled.js', 'script.js', 'quickjs.raw', 'node.stdin', 'node.stdout')]
        paths += [pin + 'sci-original.cljc', pin + 'compiled.js', 'phases/audit/command.json',
                  negative + 'user.cljc', negative + 'rejection.txt', negative + 'recovery.cljs', negative + 'recovery.js']
        binding_path = self.root / 'binding.json'
        saved_binding = binding_path.read_bytes()
        for relative in paths:
            with self.subTest(artifact=relative):
                path = self.root / relative
                saved = path.read_bytes()
                try:
                    path.write_bytes(saved + b'\nTAMPER\n')
                    binding = json.loads(saved_binding)
                    binding['artifact_sha256'][relative] = case.sci.digest(path.read_bytes())
                    binding_path.write_text(json.dumps(binding, indent=2) + '\n')
                    with self.assertRaises((ValueError, json.JSONDecodeError)):
                        case.reconcile(self.document)
                finally:
                    path.write_bytes(saved)
                    binding_path.write_bytes(saved_binding)
        self.assertEqual(self.document, case.verified_document(self.root))

    def test_summary_executable_and_missing_artifact_have_no_positive_fallback(self):
        changed = deepcopy(self.document); changed['original_semantic_pass_credit'] = 1
        with self.assertRaises(ValueError): case.reconcile(changed)
        binding_path = self.root / 'binding.json'; saved = binding_path.read_bytes()
        try:
            binding = json.loads(saved)
            binding['executables_sha256'][binding['audit_executable']] = '0' * 64
            binding_path.write_text(json.dumps(binding, indent=2) + '\n')
            with self.assertRaises(ValueError): case.reconcile(self.document)
        finally:
            binding_path.write_bytes(saved)
        path = self.root / 'phases' / self.document['source_bindings'][0]['id'] / 'application-fresh' / 'node.stdout'
        parked = path.with_suffix('.parked')
        try:
            path.rename(parked)
            with self.assertRaises(ValueError): case.reconcile(self.document)
        finally:
            parked.rename(path)
        self.assertEqual(self.document, case.verified_document(self.root))


if __name__ == '__main__': unittest.main()
