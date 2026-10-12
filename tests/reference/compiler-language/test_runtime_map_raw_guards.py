"""Exercise the runtime map proof's raw replayer after refreshing artifact hashes."""
from copy import deepcopy
import json
import os
from pathlib import Path
import unittest
import runtime_map_traversal as literal


@unittest.skipUnless(os.environ.get('RUNTIME_MAP_RECEIPT'), 'requires fresh executable runtime map acceptance')
class RuntimeMapRawReplayGuards(unittest.TestCase):
    def setUp(self):
        self.document = json.loads(Path(os.environ['RUNTIME_MAP_RECEIPT']).read_text())
        self.root = Path(self.document['artifacts_root'])
        literal.reconcile(self.document)

    def test_raw_source_unit_node_and_command_mutations_fail_after_recensus(self):
        identity = self.document['source_bindings'][0]['id']
        prefix = 'phases/' + identity + '/macro-reused/'
        paths = [prefix + name for name in ('user.cljc', 'compiled.js', 'quickjs.raw', 'node.stdin', 'node.stdout')]
        paths += ['phases/audit/command.json']
        binding_path = self.root / 'binding.json'
        saved_binding = binding_path.read_bytes()
        for relative in paths:
            with self.subTest(artifact=relative):
                path = self.root / relative
                saved = path.read_bytes()
                try:
                    path.write_bytes(saved + b'\nTAMPER\n')
                    binding = json.loads(saved_binding)
                    binding['artifact_sha256'][relative] = literal.sci.digest(path.read_bytes())
                    binding_path.write_text(json.dumps(binding, indent=2) + '\n')
                    # Replay the mutated binding itself, so the failure must
                    # arise from raw semantics/source/command checks.
                    with self.assertRaises((ValueError, json.JSONDecodeError)):
                        literal.verified_document(self.root)
                finally:
                    path.write_bytes(saved)
                    binding_path.write_bytes(saved_binding)
        self.assertEqual(self.document, literal.verified_document(self.root))

    def test_environment_executable_summary_and_missing_artifacts_fail_closed(self):
        changed = deepcopy(self.document)
        changed['full_language_completion_credit'] = 1
        with self.assertRaises(ValueError):
            literal.reconcile(changed)
        binding_path = self.root / 'binding.json'
        saved = binding_path.read_bytes()
        for field in ('audit_environment', 'executables_sha256'):
            try:
                binding = json.loads(saved)
                if field == 'audit_environment':
                    binding[field]['NODE'] = '/tmp/unbound-node'
                else:
                    binding[field][binding['audit_executable']] = '0' * 64
                binding_path.write_text(json.dumps(binding, indent=2) + '\n')
                with self.assertRaises(ValueError):
                    literal.verified_document(self.root)
            finally:
                binding_path.write_bytes(saved)
        path = self.root / 'phases' / self.document['source_bindings'][0]['id'] / 'application-fresh' / 'node.stdout'
        parked = path.with_suffix('.parked')
        try:
            path.rename(parked)
            with self.assertRaises(ValueError):
                literal.verified_document(self.root)
        finally:
            parked.rename(path)
        self.assertEqual(self.document, literal.verified_document(self.root))


if __name__ == '__main__':
    unittest.main()
