import copy
import unittest
from unittest.mock import patch
import reviewed_sci_context_dispatch as audit


class ExactNineSourceDispatchGuards(unittest.TestCase):
    def test_exact_registered_sources_partition_without_invented_scope(self):
        ids = {r['sci_id'] for r in audit.bindings()}
        rows = [r for r in audit.sci.load_all() if r['sci_id'] in ids]
        ordinary, selected = audit.partition([{'sci_id': 'unreviewed'}, *rows])
        self.assertEqual(ordinary, [{'sci_id': 'unreviewed'}])
        self.assertEqual({r['sci_id'] for r in selected}, {r['sci_id'] for r in rows})
        ordinary, selected = audit.partition(audit.bindings())
        self.assertEqual(ordinary, [])
        self.assertEqual({r['sci_id'] for r in selected}, ids)

    def test_source_literal_binding_duplicate_and_bool_drift_fail(self):
        rows = audit.bindings()
        for key, value in [('source', 'true'), ('body_sha256', '0'*64),
                           ('expected', {'outcome': {'value': True}})]:
            wrong = copy.deepcopy(rows); wrong[-1][key] = value
            with self.assertRaises(ValueError): audit.partition(wrong)
        with self.assertRaises(ValueError): audit.partition([*rows, rows[0]])

    def test_no_status_only_or_stored_receipt_credit(self):
        for document in [{'status': 'verified'}, {'profile': audit.PROFILE, 'schema_version': True}]:
            with self.assertRaises(ValueError): audit.reconcile(document)

    def test_current_dispatcher_and_originals_rechecked_before_raw_cohorts(self):
        document = {'schema_version': 1, 'profile': audit.PROFILE,
            'source_bindings': audit.bindings(), 'producer_source_sha256': audit.snapshot(),
            'status': 'verified', 'agreements': {}, 'differences': {}, 'declarations': {}}
        for key, value in [('source_bindings', []), ('producer_source_sha256', {}),
                           ('schema_version', True), ('profile', 'generic')]:
            wrong = copy.deepcopy(document); wrong[key] = value
            with patch.object(audit.agreements, 'reconcile') as raw:
                with self.assertRaises(ValueError): audit.reconcile(wrong)
                raw.assert_not_called()
        with self.assertRaises((ValueError, KeyError, OSError)): audit.reconcile(document)


if __name__ == '__main__':
    unittest.main()
