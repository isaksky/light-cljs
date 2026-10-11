#!/usr/bin/env python3
"""Fresh library evidence participates in the full gate without excusing gaps."""
import unittest
from unittest import mock
import full_ledger
import sci_library_context as context
import sci_library_context_receipts as receipts

class LibraryLedgerTests(unittest.TestCase):
    def test_full_gate_requires_current_complete_four_way_library_contracts(self):
        rows = context.load()
        records = receipts.load()
        evidence = {record['id']: record['layers'] for record in records}
        obligations = [{'id': row['obligation_id'], 'body_sha256': row['body_sha256'],
                        'disposition': 'intended', 'obligation_kind': 'sci-corpus',
                        'family': 'sci-corpus', 'name': row['sci_id'], 'source_span': {},
                        'reason': 'Original SCI library obligation'} for row in rows]
        historical = {'counts': dict.fromkeys(('covered', 'missing', 'version-difference', 'unrelated'), 0),
                      'strict_complete': False, 'boundary_strict_complete': True}
        with mock.patch.object(full_ledger, 'load', return_value=([], obligations, {'counts': {}})), \
             mock.patch.object(full_ledger, 'load_obligation_witnesses', return_value={}), \
             mock.patch.object(full_ledger, 'load_phase_witnesses', return_value={}), \
             mock.patch.object(receipts, 'load', side_effect=AssertionError('Stored receipts grant no pass')):
            static = full_ledger.expand_report(historical, {}, {})
            fresh = full_ledger.expand_report(historical, {}, {}, sci_evidence=evidence)
        agreed = sum(record['reconciliation'] == 'original-source-agreement' for record in records)
        self.assertEqual(static['counts']['missing'], len(rows))
        self.assertEqual(fresh['counts']['covered'], agreed)
        self.assertEqual(fresh['counts']['missing'], len(rows) - agreed)
        self.assertEqual(fresh['counts']['version-difference'], 0)
        self.assertFalse(fresh['strict_complete'])
        self.assertTrue(all(row['columns']['quickjs_macro']['status'] == 'not-run' for row in fresh['assertions']))
        self.assertTrue(full_ledger.sci_execution_failed(fresh, evidence, {'mismatch', 'failed', 'invalid-output'}))
        self.assertTrue(all(row['library_context_reconciliation']['original_expectation_status'] == 'unresolved'
                            for row in fresh['assertions'] if row['status'] == 'missing'))

if __name__ == '__main__': unittest.main()
