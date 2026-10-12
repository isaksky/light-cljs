"""Exact context proofs participate in the ledger without hiding missing work."""
import copy
import unittest
from unittest.mock import patch
import full_ledger
import reviewed_sci_context_dispatch as contexts


class ReviewedSciLedgerGuards(unittest.TestCase):
    def obligations(self):
        return [{'id': row['obligation_id'], 'body_sha256': row['body_sha256'],
            'disposition': 'intended', 'obligation_kind': 'sci-case', 'family': 'sci-case',
            'name': row['sci_id'], 'source_span': {}, 'reason': 'Immutable original SCI context'}
            for row in contexts.bindings()]

    def historical(self):
        return {'counts': dict.fromkeys(('covered', 'missing', 'version-difference', 'unrelated'), 0),
            'strict_complete': False, 'boundary_strict_complete': True}

    def test_source_registration_and_status_only_sidecar_never_grant_credit(self):
        with patch.object(full_ledger, 'load', return_value=([], self.obligations(), {'counts': {}})), \
             patch.object(full_ledger, 'load_obligation_witnesses', return_value={}), \
             patch.object(full_ledger, 'load_phase_witnesses', return_value={}):
            result = full_ledger.expand_report(self.historical(), {}, {})
            self.assertEqual(result['counts']['missing'], 9)
            self.assertFalse(result['strict_complete'])
            self.assertTrue(all(r['columns']['quickjs_macro']['status'] == 'not-run' for r in result['assertions']))
            with self.assertRaises(ValueError):
                full_ledger.expand_report(self.historical(), {}, {},
                    reviewed_sci_context_evidence={'status': 'verified'})

    def difference(self):
        identity = 'sci-execution-sci-record-method-recur'
        layers = {'javascript': {'status': 'mismatch'}, 'release': {'status': 'mismatch'},
                  'checkout': {'status': 'mismatch'}}
        proof = {'id': identity, 'reconciliation': 'verified-canonical-sci-difference',
            'original_expectation_status': 'unresolved', 'original_semantic_pass_credit': 0,
            'canonical_contract_status': 'passed'}
        row = {'status': 'version-difference', 'layers': layers,
            'columns': {'quickjs_macro': {'status': 'passed'}},
            'reviewed_sci_context_reconciliation': proof}
        return row, {identity: layers}

    def test_replayed_whole_source_uses_its_own_adapter_shape(self):
        # Synthetic seam control: only the real dispatcher can validate raw
        # execution. This reproduces the engine-list shape of its agreements.
        binding = next(r for r in contexts.bindings() if r['sci_id'] == 'sci-record-constructors')
        layers = {'javascript': {'status': 'passed', 'engines': ['QuickJS', 'Node']},
                  'release': {'status': 'passed'}, 'checkout': {'status': 'passed'}}
        proof = {'id': binding['id'], 'source_binding': binding, 'layers': layers,
                 'status': 'covered', 'actual_phase_frames': {}}
        obligations = [r for r in self.obligations() if r['id'] == binding['obligation_id']]
        with patch.object(full_ledger, 'load', return_value=([], obligations, {'counts': {}})), \
             patch.object(full_ledger, 'load_obligation_witnesses', return_value={}), \
             patch.object(full_ledger, 'load_phase_witnesses', return_value={}), \
             patch.object(contexts, 'reconcile', return_value={binding['obligation_id']: proof}):
            result = full_ledger.expand_report(self.historical(), {}, {},
                sci_evidence={binding['id']: layers}, reviewed_sci_context_evidence={})
            self.assertEqual(result['assertions'][0]['status'], 'covered')
            self.assertEqual(result['assertions'][0]['layers'], layers)
            self.assertIn('reviewed_sci_context_reconciliation', result['assertions'][0])
            with self.assertRaises(ValueError):
                full_ledger.expand_report(self.historical(), {}, {},
                    sci_evidence={}, reviewed_sci_context_evidence={})

    def test_only_verified_explicit_difference_can_explain_mismatch(self):
        # Synthetic classification controls do not supply raw execution credit.
        row, evidence = self.difference()
        failures = {'mismatch', 'failed', 'invalid-output'}
        self.assertFalse(full_ledger.sci_execution_failed({'assertions': [row]}, evidence, failures))
        for key, value in [('original_expectation_status', 'passed'),
                           ('original_semantic_pass_credit', 1), ('canonical_contract_status', 'missing')]:
            wrong = copy.deepcopy(row); wrong['reviewed_sci_context_reconciliation'][key] = value
            self.assertTrue(full_ledger.sci_execution_failed({'assertions': [wrong]}, evidence, failures))
        wrong = copy.deepcopy(row); wrong['columns']['quickjs_macro']['status'] = 'not-run'
        self.assertTrue(full_ledger.sci_execution_failed({'assertions': [wrong]}, evidence, failures))

    def test_failed_or_detached_layers_are_never_hidden_as_difference(self):
        row, evidence = self.difference()
        failures = {'mismatch', 'failed', 'invalid-output'}
        identity = row['reviewed_sci_context_reconciliation']['id']
        for status in ('failed', 'invalid-output'):
            wrong = copy.deepcopy(evidence); wrong[identity]['release']['status'] = status
            self.assertTrue(full_ledger.sci_execution_failed({'assertions': [row]}, wrong, failures))
        wrong = copy.deepcopy(row); wrong['layers'] = {}
        self.assertTrue(full_ledger.sci_execution_failed({'assertions': [wrong]}, evidence, failures))


if __name__ == '__main__':
    unittest.main()
