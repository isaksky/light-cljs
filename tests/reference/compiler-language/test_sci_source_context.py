#!/usr/bin/env python3
"""Original source and failures remain independent of declaration context setup."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import sci_source_context as context
import sci_source_context_receipts as receipts
import sci_execution as sci

class SourceContextTests(unittest.TestCase):
    def test_catalog_retains_every_source_expectation_form_span_and_order(self):
        catalog = context.load()
        self.assertEqual(len(catalog), 50)
        self.assertEqual({key: sum(row['classification'] == key for row in catalog) for key in context.CLASSES},
                         {'macro-declaration-context':30,'namespace-and-source-metadata':12,'pinned-version-difference':8})
        corpus = sci.rows(sci.CORPUS)
        for row in catalog:
            original = corpus[row['sci_id']][1]
            self.assertEqual(row['source'], original[2])
            self.assertEqual(row['expected'], sci.expected(original))
            self.assertEqual(row['source_sha256'], sci.digest(original[2]))
            self.assertEqual(row['namespace'], 'user')
            self.assertIsNone(row['canonical_contract'])
            last = 0
            for form in row['forms']:
                start,end = form['utf8_byte_start'],form['utf8_byte_end']
                self.assertGreaterEqual(start,last)
                self.assertEqual(row['source'].encode()[start:end].decode(),form['source'])
                last = end
        sequential = next(row for row in catalog if row['sci_id']=='sci-macro-redefinition')
        self.assertEqual([form['kind'] for form in sequential['forms']], ['macro','runtime','macro','runtime'])
        phase = next(row for row in catalog if row['sci_id']=='sci-vars-macro-analysis-scope')
        self.assertEqual([form['kind'] for form in phase['forms']], ['host-root','macro','runtime'])

    def test_changed_source_original_expectation_or_form_order_fail_closed(self):
        original=context.load()
        for key in ('source','source_sha256','original_row_sha256','expected','forms'):
            changed=copy.deepcopy(original);changed[0][key]='changed'
            with tempfile.TemporaryDirectory() as directory:
                path=Path(directory)/'catalog.jsonl'
                path.write_text(''.join(json.dumps(row)+'\n' for row in changed))
                with mock.patch.object(context,'CATALOG',path):
                    with self.assertRaisesRegex(ValueError,'stale or incomplete'):context.load()

    def test_runtime_versions_remain_original_expectations_without_automatic_retirement(self):
        versions=[row for row in context.load() if row['classification']=='pinned-version-difference']
        self.assertEqual(len(versions),8)
        runner=sci.report_module()
        for row in versions:
            wrong=copy.deepcopy(row['expected']);wrong['trace'].append('extra')
            self.assertEqual(sci.verify(row,{'status':'completed','observed':wrong},runner.same_json)['status'],'mismatch')
            for status in ('failed','timeout','not-run','rejected'):
                self.assertNotEqual(sci.verify(row,{'status':status,'observed':row['expected']},runner.same_json)['status'],'passed')

    def test_unverified_reference_identity_cannot_grant_a_source_pass(self):
        rows=context.load()[:1];runner=sci.report_module()
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(sci,'report_module',return_value=runner), \
             mock.patch.object(runner,'invoke',return_value={'status':'completed','stdout':'REFERENCE SETTINGS none','stderr':''}):
            result=context.reference_cases(rows,'release',Path(directory),1)
        self.assertEqual(result[rows[0]['id']]['status'],'invalid-reference')

    def test_receipts_keep_version_differences_and_rejections_nonpassing(self):
        recorded = receipts.load()
        self.assertEqual({kind: sum(row['reconciliation'] == kind for row in recorded)
                          for kind in ('original-source-agreement', 'verified-pinned-version-difference',
                                       'original-contract-unresolved')},
                         {'original-source-agreement': 35, 'verified-pinned-version-difference': 8,
                          'original-contract-unresolved': 7})
        for row in recorded:
            self.assertEqual(row['original_expectation_status'],
                             'passed' if row['reconciliation'] == 'original-source-agreement' else 'unresolved')

    def test_reference_provenance_or_local_test_hash_changes_fail_closed(self):
        row = context.load()[0]
        original = receipts.load()[0]['layers']
        for kind, key in (('release', 'reference_identity'), ('checkout', 'settings'),
                          ('release', 'source_sha256'), ('checkout', 'oracle_source_sha256'),
                          ('release', 'oracle_helper_source_sha256'), ('checkout', 'observation_source_sha256'),
                          ('javascript', 'test_source_sha256'), ('javascript', 'adapter_source_sha256')):
            changed = copy.deepcopy(original)
            changed[kind][key] = 'changed'
            with self.assertRaises(ValueError):
                receipts.classification(row, changed)

    def test_missing_engines_and_incomplete_execution_fail_closed(self):
        row = context.load()[0]
        original = receipts.load()[0]['layers']
        for status in ('failed', 'timeout', 'not-run', 'invalid-output'):
            changed = copy.deepcopy(original)
            changed['release']['status'] = status
            with self.assertRaisesRegex(ValueError, 'Incomplete reference execution'):
                receipts.classification(row, changed)
        for kind in ('Node', 'QuickJS'):
            changed = copy.deepcopy(original)
            del changed['javascript']['engines'][kind]
            with self.assertRaisesRegex(ValueError, 'local engines'):
                receipts.classification(row, changed)

    def test_version_difference_requires_two_distinct_executed_contracts(self):
        row = next(row for row in context.load() if row['sci_id'] == 'sci-binding-default-effects')
        original = next(record['layers'] for record in receipts.load() if record['id'] == row['id'])
        changed = copy.deepcopy(original)
        for key in ('observed', 'status'):
            changed['release'][key] = copy.deepcopy(changed['checkout'][key])
        with self.assertRaisesRegex(ValueError, 'distinct executed raw contracts'):
            receipts.classification(row, changed)

    def test_projection_equivalent_fresh_raw_contract_drift_is_rejected(self):
        recorded = receipts.load()
        evidence = {row['id']: copy.deepcopy(row['layers']) for row in recorded}
        identity = 'sci-execution-sci-macro-emit-defs'
        value = evidence[identity]['release']['observed']['outcome']['value']
        value['$seq'] = value.pop('$list')
        with self.assertRaisesRegex(ValueError, 'Pinned raw contract changed'):
            receipts.verify(evidence)

    def test_changed_receipt_expectation_catalog_or_pass_claim_fails_closed(self):
        original = receipts.load()
        for key, replacement in (('source_binding', {}), ('catalog_sha256', 'changed'),
                                 ('reconciliation', 'original-source-agreement'),
                                 ('original_expectation_status', 'passed')):
            changed = copy.deepcopy(original)
            index = next(i for i, row in enumerate(changed)
                         if row['reconciliation'] == 'verified-pinned-version-difference')
            changed[index][key] = replacement
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'receipts.jsonl'
                path.write_text(''.join(json.dumps(row) + '\n' for row in changed))
                with mock.patch.object(receipts, 'RECEIPTS', path):
                    with self.assertRaises(ValueError):
                        receipts.load()

    def test_boolean_expectation_and_settings_cannot_be_replaced_by_numbers(self):
        changed = copy.deepcopy(receipts.load())
        record = next(row for row in changed if row['source_binding']['sci_id'] == 'sci-macro-var-meta')
        record['source_binding']['expected']['outcome']['value']['$vector'][0] = 1
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'receipts.jsonl'
            path.write_text(''.join(json.dumps(row) + '\n' for row in changed))
            with mock.patch.object(receipts, 'RECEIPTS', path):
                with self.assertRaisesRegex(ValueError, 'Original source/expectation receipt changed'):
                    receipts.load()
        row = context.load()[0]
        layers = copy.deepcopy(receipts.load()[0]['layers'])
        layers['release']['settings']['static-fns'] = 0
        with self.assertRaisesRegex(ValueError, 'Unverified reference identity, source or settings'):
            receipts.classification(row, layers)

    def test_full_run_needs_fresh_complete_layers_without_loading_saved_receipts(self):
        rows = context.load()
        recorded = receipts.load()
        with mock.patch.object(receipts, 'load', side_effect=AssertionError('saved receipts cannot grant coverage')):
            self.assertTrue(all(receipts.full_report_evidence(row, {})['status'] == 'missing' for row in rows))
            results = [receipts.full_report_evidence(row, receipt['layers']) for row, receipt in zip(rows, recorded)]
        self.assertEqual([sum(result['status'] == status for result in results)
                          for status in ('covered', 'version-difference', 'missing')], [35, 8, 7])
        for row, receipt in zip(rows, recorded):
            changed = copy.deepcopy(receipt['layers'])
            changed['javascript']['engines']['QuickJS']['observed']['stdout'] += 'wrong'
            self.assertEqual(receipts.full_report_evidence(row, changed)['status'], 'missing')

    def test_distinct_pins_cannot_excuse_local_contract_matching_neither(self):
        row = next(row for row in context.load() if row['sci_id'] == 'sci-binding-default-effects')
        layers = copy.deepcopy(next(record['layers'] for record in receipts.load() if record['id'] == row['id']))
        for kind, value in (('release', 100), ('checkout', 200)):
            layers[kind]['observed']['outcome']['value'] = value
            layers[kind]['status'] = 'mismatch'
        self.assertEqual(receipts.full_report_evidence(row, layers)['status'], 'missing')
        with self.assertRaisesRegex(ValueError, 'matches neither reference pin'):
            receipts.classification(row, layers)

    def test_full_ledger_uses_checked_context_statuses_and_preserves_missing(self):
        import full_ledger
        rows = context.load()
        layers = {record['id']: record['layers'] for record in receipts.load()}
        obligations = [{'id': row['obligation_id'], 'body_sha256': row['body_sha256'],
                        'disposition': 'intended', 'obligation_kind': 'sci-corpus',
                        'family': 'sci-corpus', 'name': row['sci_id'], 'source_span': {},
                        'reason': 'Original SCI obligation'} for row in rows]
        historical = {'counts': dict.fromkeys(('covered', 'missing', 'version-difference', 'unrelated'), 0),
                      'strict_complete': False, 'boundary_strict_complete': True}
        with mock.patch.object(full_ledger, 'load', return_value=([], obligations, {'counts': {}})), \
             mock.patch.object(full_ledger, 'load_obligation_witnesses', return_value={}), \
             mock.patch.object(full_ledger, 'load_phase_witnesses', return_value={}):
            static = full_ledger.expand_report(historical, {}, {})
            fresh = full_ledger.expand_report(historical, {}, {}, sci_evidence=layers)
        self.assertEqual(static['counts']['missing'], 50)
        self.assertEqual([fresh['counts'][key] for key in ('covered', 'version-difference', 'missing')], [35, 8, 7])
        self.assertFalse(fresh['strict_complete'])
        self.assertTrue(all(row['columns']['quickjs_macro']['status'] == 'not-run' for row in fresh['assertions']))
        failures = {'mismatch', 'failed', 'invalid-output'}
        self.assertTrue(full_ledger.sci_execution_failed(fresh, layers, failures))
        differences = [row for row in fresh['assertions'] if row['status'] == 'version-difference']
        difference_layers = {row['source_context_reconciliation']['id']:
                             layers[row['source_context_reconciliation']['id']] for row in differences}
        self.assertFalse(full_ledger.sci_execution_failed({'assertions': differences}, difference_layers, failures))

    def test_specialization_preserves_all_other_rows_and_rejects_changed_originals(self):
        contexts = context.load()
        generic = sci.load_all()
        specialized = context.expression_rows(generic, contexts)
        selected_ids = {row['id'] for row in contexts}
        self.assertEqual(specialized, [row for row in generic if row['id'] not in selected_ids])
        self.assertEqual(len(generic) - len(specialized), 50)
        for key in ('body_sha256', 'original_source_sha256', 'original_row_sha256', 'expected', 'obligation_id'):
            changed = copy.deepcopy(generic)
            row = next(row for row in changed if row['id'] in selected_ids)
            row[key] = 'changed'
            with self.assertRaisesRegex(ValueError, 'specialization detached'):
                context.expression_rows(changed, contexts)

if __name__=='__main__':unittest.main()
