"""Complete-result dispatch and fail-closed engine/driver accounting."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import namespace_results as ns
import report
import syntax_context


class NamespaceResultTests(unittest.TestCase):
    def setUp(self):
        self.case = syntax_context.cases()[-1]
        self.driver = syntax_context.TESTS['namespace-nsstar-alias']

    def test_only_exact_complete_sources_results_and_drivers_dispatch(self):
        for case in syntax_context.cases()[-2:]:
            self.assertTrue(ns.validate(case))
        self.assertFalse(ns.validate({'id': 'ordinary-namespace'}))
        for field, value in [('source', self.case['source'].replace('1-2', '2-3')),
                             ('entry', 'syntax.other'), ('source_root', 'tests/fixtures/other'),
                             ('expected', {'outcome': {'status': 'returned', 'value': 'true'}}),
                             ('export', 'other')]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                ns.validate({**self.case, field: value})
        with patch.object(ns, 'DRIVER_HASH', '0' * 64), self.assertRaises(ValueError):
            ns.validate(self.case)

    def run_local(self, *, scalar='true', quick_status='completed', driver_status='completed',
                  exact=True, drift=False):
        expected = {**self.case['expected'], 'outcome': {'status': 'returned', 'value': scalar}}
        runtime = (report.ROOT / 'src/js_runtime.js').read_text().strip()
        def invoke(command, directory, **kwargs):
            if '--compile' in command:
                return report.result('completed', stdout='(() => { const rt = '+runtime+'; return "true"; })()')
            if '--test' in command:
                name = command[command.index('--exact') + 1]
                if name == 'complete_namespace_observation_executes_in_quickjs':
                    for arg in command:
                        if str(arg).startswith('LIGHT_CLJS_NAMESPACE_OUTPUT='):
                            Path(str(arg).split('=', 1)[1]).write_text(json.dumps(expected)+'\n')
                    status = quick_status
                else:
                    status = driver_status
                text = f'running 1 test\ntest {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;'
                return report.result(status, stdout=text if exact else 'running 0 tests\n')
            if command[0] == 'node':
                return report.result('completed', stdout=json.dumps(expected)+'\n')
            self.assertNotIn('--eval', command)
            return report.result('completed', stdout='')
        with tempfile.TemporaryDirectory() as root, patch.object(report, 'invoke', side_effect=invoke):
            if drift:
                with patch.object(ns, 'snapshot', side_effect=[{'source': 'original'}, {'source': 'changed'}]):
                    return ns.local(self.case, 'compiler', Path(root), 1)
            return ns.local(self.case, 'compiler', Path(root), 1)

    def test_complete_scalar_is_separate_from_original_callback_and_native(self):
        layers = self.run_local()
        self.assertEqual(layers['javascript']['status'], 'passed')
        self.assertEqual(layers['evaluation']['observed']['outcome']['value'], 'true')
        self.assertEqual(layers['javascript']['original_expected']['outcome']['value'], True)
        self.assertEqual(layers['native']['status'], 'historical-not-executed')
        self.assertEqual(layers['native']['observed'], 'true\n')

    def test_wrong_result_missing_engine_driver_or_zero_test_cannot_pass(self):
        for options in [{'scalar': True}, {'scalar': 'false'}, {'quick_status': 'failed'},
                        {'quick_status': 'unavailable'}, {'driver_status': 'failed'}, {'exact': False}]:
            with self.subTest(options=options):
                self.assertEqual(self.run_local(**options)['javascript']['status'], 'failed')
        self.assertEqual(self.run_local(drift=True)['javascript']['status'], 'stale-build-input')

    def test_unavailable_reference_and_changed_reference_source_fail_closed(self):
        with tempfile.TemporaryDirectory() as root:
            with patch.object(report, 'invoke', return_value=report.result('unavailable', 'missing reference')):
                value = report.reference_program(self.case, 'release', report.ROOT/'research/clojurescript', Path(root), 1)
                self.assertEqual(value[self.case['id']]['status'], 'reference-unavailable')
            with self.assertRaises(ValueError):
                report.reference_program({**self.case, 'source': 'true'}, 'release', report.ROOT, Path(root), 1)

    def test_full_ledger_requires_current_complete_scalar_and_both_engines(self):
        layer = self.run_local()['javascript']
        self.assertTrue(ns.verified_local(self.case, layer))
        for mutate in [lambda row: row.pop('source_sha256'),
                       lambda row: row['engines'].pop('QuickJS'),
                       lambda row: row['engines']['Node'].update(status='failed'),
                       lambda row: row['engines']['QuickJS']['observed']['outcome'].update(value=True),
                       lambda row: row['actual_macro_driver'].update(test_source_sha256='wrong')]:
            changed = copy.deepcopy(layer)
            mutate(changed)
            self.assertFalse(ns.verified_local(self.case, changed))

    def test_explicit_fixture_phase_selection_retains_other_obligations_missing(self):
        import full_ledger
        _, obligations, _ = full_ledger.load()
        def invoke(command, directory, **kwargs):
            name = command[command.index('--exact') + 1]
            return report.result('completed', stdout=f'running 1 test\ntest {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;')
        with tempfile.TemporaryDirectory() as root, patch.object(report, 'invoke', side_effect=invoke) as called:
            evidence = full_ledger.run_phase_witnesses(obligations, Path(root), 1, reference_fixtures=ns.IDS)
        self.assertEqual(set(evidence), {'syntax::ns', 'syntax::ns*'})
        self.assertEqual(called.call_count, 2)
        h, w, _ = report.load_contract()
        _, w, _ = report.load_contract('intended-language')
        assembled = full_ledger.expand_report(report.assemble(h, report.load_contract()[1], {}), w, {}, phase_evidence=evidence)
        self.assertFalse(assembled['strict_complete'])
        for identity in ('syntax::ns', 'syntax::ns*'):
            self.assertEqual(next(row for row in assembled['assertions'] if row['id'] == identity)['status'], 'missing')


if __name__ == '__main__':
    unittest.main()
