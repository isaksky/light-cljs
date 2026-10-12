"""Only the exact catalog path may support a phase witness; it never earns a pass."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import full_ledger as ledger
import report

class ExactCatalogPhaseSupport(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='phase-catalog-guard-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root/'tests').mkdir(); (self.root/'src').mkdir()
        self.rust = self.root/'tests/audit.rs'; self.rust.write_text('#[test]\nfn demands_operation() {}\n')
        self.catalog = self.root/'src/core_var_metadata.tsv'; self.catalog.write_text('not execution evidence\n')
        self.target = {'id': 'local-test', 'disposition': 'intended', 'body_sha256': 'a'*64}
        self.witness = {'obligation_id': self.target['id'], 'body_sha256': self.target['body_sha256'],
            'rationale': 'Exact named execution remains independently required.',
            'test_file': 'tests/audit.rs', 'test_name': 'demands_operation',
            'test_source_sha256': ledger.inv.digest(self.rust.read_bytes()),
            'support_source_sha256': {'src/core_var_metadata.tsv': ledger.inv.digest(self.catalog.read_bytes())}}
        self.patches = [mock.patch.object(ledger, 'ROOT', self.root), mock.patch.object(ledger, 'HERE', self.root)]
        for patch in self.patches: patch.start(); self.addCleanup(patch.stop)

    def write(self):
        (self.root/'phase-witnesses.jsonl').write_text(json.dumps(self.witness)+'\n')

    def test_exact_catalog_support_still_requires_current_hash(self):
        self.write()
        self.assertEqual(ledger.load_phase_witnesses([self.target]), {self.target['id']: self.witness})
        self.catalog.write_text('changed catalog\n')
        with self.assertRaises(ValueError): ledger.load_phase_witnesses([self.target])
        self.catalog.unlink()
        with self.assertRaises(ValueError): ledger.load_phase_witnesses([self.target])

    def test_other_src_absolute_traversal_and_alias_spellings_rejected(self):
        for name in ['src/core_var_metadata_other.tsv', 'src/compiler.rs', str(self.catalog),
                     'src/../src/core_var_metadata.tsv', 'src//core_var_metadata.tsv',
                     './src/core_var_metadata.tsv', 'src/./core_var_metadata.tsv']:
            with self.subTest(name=name):
                other = self.root/name
                if not other.exists() and '..' not in Path(name).parts:
                    other.parent.mkdir(parents=True, exist_ok=True); other.write_bytes(self.catalog.read_bytes())
                self.witness['support_source_sha256'] = {name: ledger.inv.digest(self.catalog.read_bytes())}
                self.write()
                with self.assertRaises(ValueError): ledger.load_phase_witnesses([self.target])

    def test_tests_support_paths_and_test_hash_guard_remain(self):
        support = self.root/'tests/literal.tsv'; support.write_text('independent literal\n')
        self.witness['support_source_sha256'] = {'tests/literal.tsv': ledger.inv.digest(support.read_bytes())}
        self.write(); self.assertIn(self.target['id'], ledger.load_phase_witnesses([self.target]))
        self.witness['test_source_sha256'] = '0'*64; self.write()
        with self.assertRaises(ValueError): ledger.load_phase_witnesses([self.target])

    def test_catalog_registration_and_zero_test_run_never_pass(self):
        self.write()
        for stdout in ['running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored;\n',
                       'running 1 test\ntest demands_operation ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored;\n']:
            with mock.patch.object(report, 'invoke', return_value={'status': 'completed', 'stdout': stdout}):
                result = ledger.run_phase_witnesses([self.target], self.root/'artifacts', 1)
                self.assertEqual(result[self.target['id']]['status'], 'invalid-output')

if __name__ == '__main__': unittest.main()
