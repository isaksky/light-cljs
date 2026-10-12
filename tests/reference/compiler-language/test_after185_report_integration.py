"""Mutation controls for supplementing unresolved original observers, no semantic credit."""
import copy
import unittest
from unittest.mock import patch
import full_ledger as ledger
import sci_cljc_transport_after185 as transport


class TransportIntegrationGuards(unittest.TestCase):
    def setUp(self):
        self.prior = {}
        self.current = {}
        for row in transport.load():
            identity = row['obligation_id']
            original = copy.deepcopy(row)
            original['canonical_expected'] = None
            self.prior[identity] = {'status': 'missing', 'source_binding': original,
                                    'layers': {'observer': 'unresolved original retained'}}
            self.current[identity] = {'status': 'version-difference', 'source_binding': copy.deepcopy(row),
                                      'original_semantic_pass_credit': 0}

    def merge(self):
        with patch.object(transport, 'reconcile', return_value=copy.deepcopy(self.current)):
            return ledger.verified_cljc_transport({'explicit_guard_fixture': True}, self.prior)

    def test_pristine_keeps_the_original_nonpassing_observer(self):
        previous = copy.deepcopy(self.prior)
        merged = self.merge()
        self.assertEqual(set(merged), set(self.prior))
        self.assertEqual(previous, self.prior)
        for identity in merged:
            self.assertEqual(merged[identity]['prior_observer_reconciliation'], previous[identity])
            self.assertEqual(merged[identity]['original_semantic_pass_credit'], 0)

    def test_missing_base_context_rejects(self):
        self.prior.pop(next(iter(self.prior)))
        with self.assertRaises(ValueError): self.merge()

    def test_already_passing_base_context_rejects(self):
        next(iter(self.prior.values()))['status'] = 'covered'
        with self.assertRaises(ValueError): self.merge()

    def test_existing_canonical_literal_cannot_be_replaced(self):
        next(iter(self.prior.values()))['source_binding']['canonical_expected'] = {'changed': True}
        with self.assertRaises(ValueError): self.merge()

    def test_source_and_expectation_mutations_reject(self):
        for key in ('source', 'source_sha256', 'original_row_sha256', 'body_sha256', 'original_expected'):
            saved = copy.deepcopy(self.current)
            next(iter(self.current.values()))['source_binding'][key] = 'coherently detached'
            with self.assertRaises(ValueError): self.merge()
            self.current = saved

    def test_extra_or_incomplete_transport_census_rejects(self):
        saved = copy.deepcopy(self.current)
        self.current['unselected-original'] = copy.deepcopy(next(iter(self.current.values())))
        with self.assertRaises(ValueError): self.merge()
        self.current = saved
        self.current.pop(next(iter(self.current)))
        with self.assertRaises(ValueError): self.merge()

    def test_original_sci_semantic_credit_or_false_coverage_rejects(self):
        for key, value in [('original_semantic_pass_credit', 1), ('status', 'covered')]:
            saved = copy.deepcopy(self.current)
            next(iter(self.current.values()))[key] = value
            with self.assertRaises(ValueError): self.merge()
            self.current = saved


class TransportFailureAccountingGuards(unittest.TestCase):
    def setUp(self):
        original = copy.deepcopy(transport.load()[0])
        prior = copy.deepcopy(original)
        prior['canonical_expected'] = None
        identity = original['id']
        self.evidence = {identity: {'javascript': {'status': 'mismatch', 'original': True}}}
        proof = {'id': identity, 'source_binding': original,
                 'intermediate_value_transport_profile': transport.PROFILE,
                 'reconciliation': 'verified-canonical-sci-difference',
                 'original_expectation_status': 'unresolved', 'original_semantic_pass_credit': 0,
                 'canonical_contract_status': 'passed', 'actual_macro_contract_status': 'passed',
                 'checked_pin_contract_status': {'release': 'passed', 'checkout': 'passed'},
                 'layers': {'javascript': {'status': 'passed', 'typed': True}},
                 'prior_observer_reconciliation': {'status': 'missing', 'source_binding': prior,
                                                   'layers': copy.deepcopy(self.evidence[identity])}}
        self.row = {'status': 'version-difference', 'gap_sci_context_reconciliation': proof,
                    'columns': {'quickjs_macro': {'status': 'passed'}}, 'layers': copy.deepcopy(proof['layers'])}

    def failed(self):
        return ledger.sci_execution_failed({'assertions': [self.row]}, self.evidence, {'mismatch', 'rejected', 'timed-out'})

    def test_exact_retained_observer_mismatch_is_explained(self):
        self.assertFalse(self.failed())

    def test_other_failure_statuses_are_never_suppressed(self):
        for status in ('rejected', 'timed-out'):
            next(iter(self.evidence.values()))['javascript']['status'] = status
            self.row['gap_sci_context_reconciliation']['prior_observer_reconciliation']['layers'] = copy.deepcopy(next(iter(self.evidence.values())))
            self.assertTrue(self.failed())

    def test_detached_or_unverified_linkage_does_not_explain_failure(self):
        saved = copy.deepcopy(self.row)
        proof_fields = [('id', 'wrong-source'), ('intermediate_value_transport_profile', 'wrong-profile'),
                        ('original_semantic_pass_credit', 1), ('actual_macro_contract_status', 'missing'),
                        ('checked_pin_contract_status', {'release': 'passed'}),
                        ('prior_observer_reconciliation', {})]
        for key, value in proof_fields:
            self.row = copy.deepcopy(saved)
            self.row['gap_sci_context_reconciliation'][key] = value
            self.assertTrue(self.failed(), key)
        for key in ('source', 'source_sha256', 'original_row_sha256', 'body_sha256', 'original_expected'):
            self.row = copy.deepcopy(saved)
            self.row['gap_sci_context_reconciliation']['source_binding'][key] = 'detached'
            self.assertTrue(self.failed(), key)
        self.row = copy.deepcopy(saved)
        self.row['columns']['quickjs_macro']['status'] = 'missing'
        self.assertTrue(self.failed())
        self.row = copy.deepcopy(saved)
        self.row['gap_sci_context_reconciliation']['prior_observer_reconciliation']['layers'] = {'javascript': {'status': 'mismatch', 'detached': True}}
        self.assertTrue(self.failed())
        self.row = copy.deepcopy(saved)
        self.row['layers'] = {'javascript': {'status': 'passed', 'detached': True}}
        self.assertTrue(self.failed())


if __name__ == '__main__': unittest.main()
