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
        self.assertEqual(len(rows), 191)
        corpus = sci.rows(sci.CORPUS)
        for row in rows:
            original = corpus[row['sci_id']][1]
            self.assertEqual(row['source'], original[2])
            self.assertEqual(row['expected'], sci.expected(original))
            self.assertEqual(row['source_sha256'], sci.digest(original[2]))
            self.assertEqual(row['declaration_source'] + row['runtime_source'], row['adapted_source'])
            self.assertEqual((row['adapted_source'], row['source_edits']), sci.catch_adapter(original[2]))
        for library in ('string', 'set', 'walk', 'edn'):
            rows_for_library = [row for row in rows if ('sci-string-' if library == 'string' else 'sci-data-' + library + '-') in row['sci_id']]
            self.assertGreaterEqual(len(rows_for_library), 4)
            self.assertTrue(any(row['declaration_source'].startswith('(require ') for row in rows_for_library))

    def test_planning_cannot_claim_unexecuted_library_sources_pass(self):
        rows = context.planning()
        self.assertEqual(len(rows), 197)
        self.assertEqual(sum(row['status'] == 'not-run' for row in rows), 6)
        self.assertEqual(sum(row['status'] == 'selected-for-execution' for row in rows), 191)
        self.assertFalse(any(row['status'] == 'passed' for row in rows))

    def test_literal_receipts_preserve_edn_discard_failure_against_original(self):
        rows = receipts.load()
        self.assertGreater(sum(row['reconciliation'] == 'original-source-agreement' for row in rows), 15)
        row = next(row for row in rows if row['source_binding']['sci_id'] == 'sci-data-edn-tag-discard')
        self.assertEqual(row['source_binding']['expected']['outcome'], {'status': 'returned', 'value': 2})
        self.assertEqual(row['layers']['javascript']['status'], 'passed')
        for kind in ('release', 'checkout'):
            self.assertEqual(row['layers'][kind]['status'], 'mismatch')
            self.assertEqual(row['layers'][kind]['observed']['outcome']['status'], 'threw')
        self.assertEqual(row['original_expectation_status'], 'unresolved')
        self.assertEqual(sum('prior_evidence' in row for row in rows), 16)
        self.assertEqual(row['source_binding']['expected']['trace'], [1])
        self.assertEqual(row['layers']['javascript']['engines']['Node']['observed']['trace'], [1])
        self.assertIn('prior_evidence', row['prior_evidence'])

    def test_current_library_driver_inspector_oracle_and_pin_hashes_required(self):
        row = context.load()[0]
        original = receipts.load()[0]['layers']
        for kind, key in (('javascript', 'test_source_sha256'), ('javascript', 'inspection_source_sha256'),
                          ('javascript', 'library_source_sha256'), ('release', 'oracle_source_sha256'),
                          ('checkout', 'oracle_helper_source_sha256'), ('release', 'settings'),
                          ('checkout', 'namespace'), ('checkout', 'reference_identity'),
                          ('javascript', 'library_context_adapter_sha256'), ('checkout', 'library_context_adapter_sha256'),
                          ('javascript', 'compiler_source_sha256'),
                          ('release', 'raw_stdout'), ('checkout', 'raw_stderr'),
                          ('release', 'reference_build_stdout'), ('checkout', 'reference_build_stderr')):
            changed = copy.deepcopy(original)
            changed[kind][key] = None if key == 'reference_build_stderr' else 'changed'
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

    def test_unicode_json_data_and_strict_newline_frames(self):
        identity = 'unicode-library-fixture'
        value = {'id': identity, 'outcome': {'status': 'returned', 'value': '\u0085\u2028\u2029'},
                 'trace': [], 'stdout': ''}
        row = {'id': identity, 'expected': value}
        raw = json.dumps(value, ensure_ascii=False) + '\n'
        parsed = context.observations(raw, [row])[identity]
        self.assertEqual(parsed['status'], 'passed')
        self.assertEqual(parsed['observed'], value)
        invalid = [raw + raw, raw + '\n', raw.replace(identity, 'other'),
                   raw.replace('"id":', '"id":"duplicate", "id":'),
                   json.dumps({key: val for key, val in value.items() if key != 'trace'}) + '\n']
        for changed in invalid:
            self.assertEqual(context.observations(changed, [row])[identity]['status'], 'invalid-output')

    def test_local_batch_timeouts_cannot_be_promoted_or_drop_cases(self):
        rows = context.load()[:35]
        calls = []
        def fake_batch(selected, directory, timeout):
            calls.append((len(selected), directory.name, timeout))
            return {row['id']: {'status': 'timeout'} for row in selected}
        with mock.patch.object(context, 'local_batch', side_effect=fake_batch):
            result = context.local_cases(rows, Path('/tmp/library-batch-fixture'), 7)
        self.assertEqual(calls, [(16, 'batch-0', 7), (16, 'batch-1', 7), (3, 'batch-2', 7)])
        self.assertEqual(list(result), [row['id'] for row in rows])
        self.assertTrue(all(layer['status'] == 'timeout' for layer in result.values()))

    def test_changed_compiler_inputs_reject_entire_local_batch(self):
        runner = context.sci.report_module()
        with tempfile.TemporaryDirectory() as directory:
            with mock.patch.object(context, 'compiler_sources', side_effect=[{'old': 'hash'}, {'new': 'hash'}]), \
                 mock.patch.object(context.sci, 'report_module', return_value=runner), \
                 mock.patch.object(runner, 'invoke', return_value={'status': 'completed'}):
                result = context.local_batch(context.load()[:2], Path(directory), 7)
        self.assertEqual(len(result), 2)
        self.assertEqual({layer['status'] for layer in result.values()}, {'stale-build-input'})

    def test_specialization_requires_exact_original_bindings(self):
        contexts = context.load()
        generic = [copy.deepcopy(contexts[0]), {'id': 'unrelated-source'}]
        self.assertEqual(context.expression_rows(generic, contexts), [generic[1]])
        for key in ('obligation_id', 'original_source_sha256', 'original_row_sha256', 'body_sha256', 'expected'):
            changed = copy.deepcopy(generic)
            changed[0][key] = 'changed'
            with self.assertRaisesRegex(ValueError, 'detached from original'):
                context.expression_rows(changed, contexts)
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            context.expression_rows(generic, [contexts[0], contexts[0]])

    def test_full_report_credit_requires_fresh_complete_current_evidence(self):
        rows = context.load()
        stored = receipts.load()
        agreed = next((row, record) for row, record in zip(rows, stored)
                      if record['reconciliation'] == 'original-source-agreement')
        self.assertEqual(receipts.full_report_evidence(agreed[0], agreed[1]['layers'])['status'], 'covered')
        self.assertEqual(receipts.full_report_evidence(agreed[0], {})['status'], 'missing')
        self.assertEqual(receipts.full_report_evidence(agreed[0], None)['status'], 'missing')
        self.assertEqual(receipts.full_report_evidence(agreed[0], {'stored_receipt': agreed[1]})['status'], 'missing')
        for mutation in ('engine', 'pin', 'hash', 'observation', 'raw-local', 'raw-stderr'):
            layers = copy.deepcopy(agreed[1]['layers'])
            if mutation == 'engine': del layers['javascript']['engines']['QuickJS']
            elif mutation == 'pin': layers['checkout']['reference_identity'] = 'checkout unverified'
            elif mutation == 'hash': layers['javascript']['library_source_sha256'] = {}
            elif mutation == 'observation': layers['release']['observed']['stdout'] += 'changed'
            elif mutation == 'raw-local': layers['javascript']['engines']['Node']['raw_output'] = 'null'
            else: layers['release']['raw_stderr'] = None
            self.assertEqual(receipts.full_report_evidence(agreed[0], layers)['status'], 'missing')
        for row, record in zip(rows, stored):
            if record['reconciliation'] != 'original-source-agreement':
                result = receipts.full_report_evidence(row, record['layers'])
                self.assertEqual(result['status'], 'missing')
                self.assertEqual(result['original_expectation_status'], 'unresolved')

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

    def test_fresh_writer_retains_entire_prior_receipt_history(self):
        stored = receipts.load()
        document = {'catalog_sha256': sci.digest(context.CATALOG.read_bytes()),
                    'evidence': {record['id']: record['layers'] for record in stored}}
        with tempfile.TemporaryDirectory() as directory:
            evidence_path = Path(directory) / 'evidence.json'
            evidence_path.write_text(json.dumps(document))
            recorded = receipts.record_evidence(document, evidence_path, stored[:1])
            self.assertEqual(recorded[0]['prior_evidence']['layers'], stored[0]['layers'])
            self.assertEqual(recorded[0]['prior_evidence']['prior_evidence'], stored[0]['prior_evidence'])
            self.assertFalse(any('prior_evidence' in record for record in recorded[1:]))
            with self.assertRaises(ValueError):
                receipts.record_evidence(document, evidence_path, [stored[0], stored[0]])
            incomplete = copy.deepcopy(document)
            del incomplete['evidence'][stored[-1]['id']]['checkout']
            with self.assertRaises(ValueError):
                receipts.record_evidence(incomplete, evidence_path)

if __name__ == '__main__': unittest.main()
