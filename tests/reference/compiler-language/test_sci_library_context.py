#!/usr/bin/env python3
"""Library adapters retain complete sources, declarations, effects and failures."""
import copy, json, tempfile, unittest
from pathlib import Path
from unittest import mock
import sci_library_context as context
import sci_library_context_receipts as receipts
import sci_execution as sci

class LibraryContextTests(unittest.TestCase):
    def test_bounded_catalog_retains_complete_originals_and_declarations(self):
        rows = context.load()
        self.assertEqual(len(rows), 16)
        corpus = sci.rows(sci.CORPUS)
        for row in rows:
            original = corpus[row['sci_id']][1]
            self.assertEqual(row['source'], original[2])
            self.assertEqual(row['expected'], sci.expected(original))
            self.assertEqual(row['source_sha256'], sci.digest(original[2]))
            self.assertEqual(row['declaration_source'] + row['runtime_source'], row['adapted_source'])
            self.assertEqual(row['source_edits'], [])
        for library in ('string', 'set', 'walk', 'edn'):
            rows_for_library = [row for row in rows if ('sci-string-' if library == 'string' else 'sci-data-' + library + '-') in row['sci_id']]
            self.assertEqual(len(rows_for_library), 4)
            self.assertTrue(any(row['declaration_source'].startswith('(require ') for row in rows_for_library))

    def test_planning_cannot_claim_unexecuted_library_sources_pass(self):
        rows = context.planning()
        self.assertEqual(len(rows), 197)
        self.assertEqual(sum(row['status'] == 'not-run' for row in rows), 181)
        self.assertEqual(sum(row['status'] == 'selected-for-execution' for row in rows), 16)
        self.assertFalse(any(row['status'] == 'passed' for row in rows))

    def test_literal_receipts_preserve_edn_discard_failure_against_original(self):
        rows = receipts.load()
        self.assertEqual(sum(row['reconciliation'] == 'original-source-agreement' for row in rows), 15)
        row = next(row for row in rows if row['source_binding']['sci_id'] == 'sci-data-edn-tag-discard')
        self.assertEqual(row['source_binding']['expected']['outcome'], {'status': 'returned', 'value': 2})
        self.assertEqual(row['layers']['javascript']['status'], 'passed')
        for kind in ('release', 'checkout'):
            self.assertEqual(row['layers'][kind]['status'], 'mismatch')
            self.assertEqual(row['layers'][kind]['observed']['outcome']['status'], 'threw')
        self.assertEqual(row['original_expectation_status'], 'unresolved')
        self.assertEqual(len(rows), sum(len(row['prior_evidence']['layers']) == 3 for row in rows))

    def test_current_library_driver_inspector_oracle_and_pin_hashes_required(self):
        row = context.load()[0]
        original = receipts.load()[0]['layers']
        for kind, key in (('javascript', 'test_source_sha256'), ('javascript', 'inspection_source_sha256'),
                          ('javascript', 'library_source_sha256'), ('release', 'oracle_source_sha256'),
                          ('checkout', 'oracle_helper_source_sha256'), ('release', 'settings'),
                          ('checkout', 'namespace'), ('checkout', 'reference_identity')):
            changed = copy.deepcopy(original)
            changed[kind][key] = 'changed'
            with self.assertRaises(ValueError): receipts.classification(row, changed)
        changed = copy.deepcopy(original)
        del changed['javascript']['engines']['QuickJS']
        with self.assertRaises(ValueError): receipts.classification(row, changed)

    def test_changed_catalog_expectation_and_declaration_cannot_pass(self):
        original = context.load()
        for key in ('source', 'expected', 'declaration_source', 'runtime_source', 'body_sha256'):
            changed = copy.deepcopy(original)
            changed[0][key] = 'changed'
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'catalog.jsonl'
                path.write_text(''.join(json.dumps(row) + '\n' for row in changed))
                with mock.patch.object(context, 'CATALOG', path):
                    with self.assertRaisesRegex(ValueError, 'stale or incomplete'): context.load()

    def test_unresolved_reference_cannot_be_promoted_or_lost_in_fresh_receipts(self):
        rows = receipts.load()
        evidence = {row['id']: copy.deepcopy(row['layers']) for row in rows}
        identity = 'sci-execution-sci-data-edn-tag-discard'
        evidence[identity]['release']['status'] = 'passed'
        with self.assertRaises(ValueError): receipts.verify(evidence)
        evidence = {row['id']: copy.deepcopy(row['layers']) for row in rows}
        evidence[identity]['checkout']['status'] = 'not-run'
        with self.assertRaises(ValueError): receipts.verify(evidence)
        changed = copy.deepcopy(rows)
        row = next(row for row in changed if row['id'] == identity)
        row['original_expectation_status'] = 'passed'
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'receipts.jsonl'
            path.write_text(''.join(json.dumps(row) + '\n' for row in changed))
            with mock.patch.object(receipts, 'RECEIPTS', path):
                with self.assertRaises(ValueError): receipts.load()

if __name__ == '__main__': unittest.main()
