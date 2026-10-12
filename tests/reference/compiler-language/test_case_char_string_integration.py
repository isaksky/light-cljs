"""The exact original remains a canonical difference under current phase proof."""
from copy import deepcopy
import json
import os
from pathlib import Path
import unittest
from unittest.mock import patch
import case_char_string_accepted as case
import full_ledger as ledger


class CaseLedgerIntegrationTests(unittest.TestCase):
    def fixture(self):
        binding = case.load()[0][0]
        target = next(row for row in ledger.load()[1] if row['id'] == binding['obligation_id'])
        proof = {'id': binding['id'], 'obligation_id': binding['obligation_id'],
                 'status': 'version-difference', 'source_binding': binding,
                 'original_semantic_pass_credit': 0, 'original_expectation_status': 'unresolved',
                 'canonical_contract_status': 'passed', 'actual_macro_contract_status': 'passed',
                 'checked_pin_contract_status': {'release': 'passed', 'checkout': 'passed'},
                 'reconciliation': 'verified-canonical-sci-difference',
                 'layers': {kind: {'status': 'mismatch', 'observed': binding['canonical_expected']}
                            for kind in ('javascript', 'release', 'checkout')},
                 'actual_phase_frames': {variant + '/' + engine: {'status': 'passed'}
                                         for variant in case.proof.VARIANTS
                                         for engine in ('QuickJS', 'Node')}}
        return target, proof

    def expand(self, target, proof, evidence):
        historical = {'counts': {}, 'strict_complete': False, 'boundary_strict_complete': True}
        with patch.object(ledger, 'load', return_value=([], [target], {'counts': {}})), \
             patch.object(ledger, 'load_obligation_witnesses', return_value={}), \
             patch.object(ledger, 'load_phase_witnesses', return_value={}):
            return ledger.expand_report(historical, {}, {}, identities=(target['id'],),
                                        sci_evidence=evidence, case_sci_context_evidence={'actual': 'required'})

    def test_canonical_difference_keeps_original_credit_zero_and_actual_macro_frames(self):
        target, proof = self.fixture()
        with patch.object(case, 'reconcile', return_value={target['id']: proof}) as replay:
            result = self.expand(target, proof, {proof['id']: proof['layers']})
        replay.assert_called_once_with({'actual': 'required'})
        row = result['assertions'][0]
        self.assertEqual('version-difference', row['status'])
        self.assertEqual(0, row['gap_sci_context_reconciliation']['original_semantic_pass_credit'])
        self.assertEqual([2, 2], proof['source_binding']['original_expected']['outcome']['value']['$vector'])
        self.assertEqual('passed', row['columns']['quickjs_macro']['status'])
        self.assertEqual(4, len(row['columns']['quickjs_macro']['frames']))
        self.assertFalse(result['strict_complete'])
        self.assertFalse(ledger.sci_execution_failed(result, {proof['id']: proof['layers']}, {'mismatch'}))

    def test_detached_fresh_layers_and_changed_original_body_reject(self):
        target, proof = self.fixture()
        with patch.object(case, 'reconcile', return_value={target['id']: proof}):
            with self.assertRaisesRegex(ValueError, 'detached from fresh report execution'):
                self.expand(target, proof, {proof['id']: {}})
            changed = deepcopy(target); changed['body_sha256'] = '0' * 64
            with self.assertRaisesRegex(ValueError, 'detached from original intended SCI obligation'):
                self.expand(changed, proof, {proof['id']: proof['layers']})

    @unittest.skipUnless(os.environ.get('CASE_CHAR_STRING_RECEIPT'), 'requires fresh raw/executable acceptance receipt')
    def test_actual_raw_proof_integrates_the_complete_original(self):
        document = json.loads(Path(os.environ['CASE_CHAR_STRING_RECEIPT']).read_text())
        proofs = case.reconcile(document)
        proof = next(iter(proofs.values()))
        target = next(row for row in ledger.load()[1] if row['id'] == proof['obligation_id'])
        historical = {'counts': {'covered': 0, 'missing': 0, 'version-difference': 0}, 'strict_complete': False, 'boundary_strict_complete': True}
        _, witnesses, _ = case.proof.report.load_contract('intended-language')
        result = ledger.expand_report(historical, witnesses, {}, identities=(target['id'],),
                                      sci_evidence={proof['id']: proof['layers']}, case_sci_context_evidence=document)
        row = next(row for row in result['assertions'] if row['id'] == target['id'])
        self.assertEqual('version-difference', row['status'])
        self.assertEqual('passed', row['columns']['quickjs_macro']['status'])
        self.assertFalse(result['strict_complete'])
        self.assertEqual(0, result['counts']['covered'])


if __name__ == '__main__':
    unittest.main()
