#!/usr/bin/env python3
"""Ensure upstream array-setting oracles cannot silently compile local &env macros."""
from pathlib import Path
import copy
import importlib.util
import sys
import tempfile
import unittest
from unittest.mock import patch
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import report
spec = importlib.util.spec_from_file_location('array_settings', ROOT / 'tests/reference/compiler-array-settings.py')
fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixtures)


class ReferenceSourceTests(unittest.TestCase):
    def setUp(self):
        self.case = fixtures.generated()[1][0]

    def test_retained_reference_graph_has_exact_file_hashes(self):
        report.validate_reference_source(self.case)
        for changes in [
            {'reference_source_root': '/tmp/oracle'},
            {'reference_source_root': '../oracle'},
            {'reference_adaptation': ''},
            {'reference_files_sha256': {}},
            {'reference_files_sha256': {**self.case['reference_files_sha256'], 'cljs/array_access/helper.clj': '0' * 64}},
            {'reference_source_root': self.case['source_root']},
        ]:
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                report.validate_reference_source({**self.case, **changes})
        case = copy.deepcopy(self.case)
        del case['source_root']
        with self.assertRaises(ValueError):
            report.validate_reference_source(case)

    def test_oracle_classpath_and_build_input_use_original_helper_clj_tree(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(report, 'invoke', return_value=report.result('unavailable')) as invoke:
            report.reference_program(self.case, 'release', ROOT / 'research/clojurescript', Path(temporary), 1)
        command = invoke.call_args.args[0]
        original = str(ROOT / self.case['reference_source_root'])
        local = str(ROOT / self.case['source_root'])
        self.assertEqual(command[command.index('program') + 1], original)
        self.assertIn(original, command[command.index('-Sdeps') + 1])
        self.assertNotIn(local, command[command.index('-Sdeps') + 1])


if __name__ == '__main__':
    unittest.main()
