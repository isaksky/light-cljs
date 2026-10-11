#!/usr/bin/env python3
"""Fail-closed transport/provenance controls; the Rust audit supplies semantics."""
import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import sci_library_reconciliation as audit


class ExactContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.contracts = audit.load_contracts()

    def table(self, rows):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        path = Path(temporary.name) / 'contracts.jsonl'
        path.write_text(''.join(json.dumps(row) + '\n' for row in rows))
        return path

    def test_complete_original_bindings_and_separate_dialects(self):
        self.assertEqual(len(self.contracts), 22)
        counts = {kind: sum(c['disposition'] == kind for c in self.contracts)
                  for kind in ('canonical-sci-difference', 'adopted-strict-edn-dialect-difference')}
        self.assertEqual(set(counts.values()), {11})
        for contract in self.contracts:
            self.assertEqual(contract['historical_expected'], contract['source_binding']['expected'])
            self.assertEqual(contract['original_expectation_status_expected'], 'unresolved')
            self.assertEqual(contract['checked_pin_original_expectation_status_expected'],
                             {'release': 'mismatch', 'checkout': 'mismatch'})

    def test_table_rejects_missing_duplicate_extra_or_reordered_originals(self):
        for rows in (self.contracts[:-1], self.contracts + [self.contracts[-1]],
                     list(reversed(self.contracts)), [self.contracts[1]] + self.contracts[1:]):
            with self.subTest(ids=[row['id'] for row in rows]), self.assertRaises(ValueError):
                audit.load_contracts(self.table(rows))

    def test_source_body_expectation_hash_adapter_and_context_are_immutable(self):
        for key in ('source', 'runtime_source', 'declaration_source', 'original_row_sha256',
                    'body_sha256', 'adapted_source_sha256', 'expected', 'source_edits',
                    'namespace', 'reference_namespace', 'libraries'):
            changed = copy.deepcopy(self.contracts)
            changed[0]['source_binding'][key] = 'changed'
            with self.subTest(key=key), self.assertRaises(ValueError):
                audit.load_contracts(self.table(changed))

    def test_strict_edn_cannot_be_replaced_with_pin_or_promoted_to_full_sci_pass(self):
        for key, value in (('local_expected', self.contracts[0]['release_expected']),
                           ('original_expectation_status_expected', 'passed'),
                           ('disposition', 'canonical-sci-difference')):
            changed = copy.deepcopy(self.contracts)
            changed[0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                audit.load_contracts(self.table(changed))

    def test_canonical_cannot_hide_sci_mismatch_or_disagree_with_one_pin(self):
        for key, value in (('local_expected', self.contracts[1]['historical_expected']),
                           ('checkout_expected', self.contracts[1]['historical_expected']),
                           ('local_original_expectation_status_expected', 'passed')):
            changed = copy.deepcopy(self.contracts)
            changed[1][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                audit.load_contracts(self.table(changed))

    def frames(self):
        # These finite literal transport controls are not runtime evidence.
        by_id = {c['id']: c for c in self.contracts}
        return [{'id': identity, 'host': host, 'phase': phase, 'engine': engine,
                 'source_sha256': audit.sci.digest(audit.module_source(
                     by_id[identity]['source_binding'], phase).encode()),
                 'exit': 0, 'raw_stderr': '',
                 'raw_stdout': json.dumps(by_id[identity]['local_expected']) + '\n',
                 'observed': copy.deepcopy(by_id[identity]['local_expected']),
                 'original_expectation_status': by_id[identity]['local_original_expectation_status_expected']}
                for identity, host, phase, engine in audit.frame_order()]

    def test_phase_guard_requires_all_176_ordered_engine_and_host_frames(self):
        frames = self.frames()
        self.assertTrue(audit.validate_frames(self.contracts, frames))
        for broken in (frames[:-1], frames + [frames[-1]], frames[1:] + [frames[0]],
                       [f for f in frames if f['phase'] != 'macro'],
                       [f for f in frames if f['host'] != 'reused'],
                       [f for f in frames if f['engine'] != 'QuickJS']):
            with self.subTest(count=len(broken)), self.assertRaises(ValueError):
                audit.validate_frames(self.contracts, broken)

    def test_raw_macro_effects_errors_and_stdout_cannot_be_inferred_from_application(self):
        for field, value in (('trace', [1, 2]), ('stdout', 'unobserved print'),
                             ('outcome', {'status': 'returned', 'value': 99})):
            frames = self.frames()
            index = next(i for i, f in enumerate(frames) if f['phase'] == 'macro')
            frames[index]['observed'][field] = value
            frames[index]['raw_stdout'] = json.dumps(frames[index]['observed']) + '\n'
            with self.subTest(field=field), self.assertRaises(ValueError):
                audit.validate_frames(self.contracts, frames)

    def test_raw_duplicates_extra_frames_stderr_stale_source_and_failure_are_rejected(self):
        for field, value in (('raw_stdout', '{"id":1,"id":2}\n'),
                             ('raw_stdout', json.dumps(self.contracts[0]['local_expected']) + '\n{}\n'),
                             ('raw_stderr', 'unexpected'), ('source_sha256', '0' * 64),
                             ('exit', 1), ('original_expectation_status', 'mismatch')):
            frames = self.frames()
            frames[0][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                audit.validate_frames(self.contracts, frames)
        with self.assertRaises(ValueError):
            audit.strict_json('{"value":NaN}')

    def test_unexecuted_transport_controls_never_validate_as_runtime_evidence(self):
        with self.assertRaises(ValueError):
            audit.validate_local({'schema_version': 1, 'provenance': audit.provenance(),
                                  'contracts': self.contracts, 'frames': self.frames()})

    def test_subset_renumbering_and_partial_pin_documents_are_rejected(self):
        catalog = audit.context.load()
        full = {'catalog_sha256': audit.sci.digest(audit.context.CATALOG.read_bytes()),
                'evidence': {row['id']: {} for row in catalog}}
        with patch.object(audit, 'validate_local', return_value=self.contracts):
            for broken in ({'evidence': full['evidence']},
                           {**full, 'evidence': {c['id']: {} for c in self.contracts}},
                           {**full, 'evidence': dict(reversed(list(full['evidence'].items())))}):
                with self.assertRaises(ValueError):
                    audit.reconcile({}, broken)
            # Full identity order alone cannot replace checked pin build/source proof.
            with self.assertRaises(ValueError):
                audit.reconcile({}, full)


if __name__ == '__main__':
    unittest.main()
