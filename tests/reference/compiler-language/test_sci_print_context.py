from copy import deepcopy
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import sci_print_context as cohort
import sci_gap_canonical_context as proof


class PrintingCanonicalContextTests(unittest.TestCase):
    def document(self):
        rows = cohort.load()
        root = Path('/tmp/print-context-guard').resolve()
        frames = {r['id']: {v+'/'+e: {'status': 'passed'}
            for v in proof.VARIANTS for e in ('QuickJS', 'Node')} for r in rows}
        layers = {r['id']: {k: {'status': 'mismatch', 'observed': deepcopy(r['canonical_expected'])}
            for k in ('javascript', 'release', 'checkout')} for r in rows}
        return {'schema_version': 1, 'profile': cohort.PROFILE, 'source_bindings': rows,
            'compiler_build_producer_source_sha256': cohort.snapshot(),
            'reference_artifacts_root': str(root/'originals'), 'phase_artifacts_root': str(root/'phases'),
            'frames': frames, 'reference_evidence': {'evidence': layers},
            'raw_phase_observations': 'guard', 'summaries': [], 'original_semantic_pass_credit': 0}

    def test_exact_originals_and_literal_strings_remain_separate(self):
        rows = cohort.load()
        originals = {r['sci_id']: r for r in cohort.candidates.load()}
        self.assertEqual(14, len(rows))
        for row in rows:
            self.assertEqual(originals[row['sci_id']]['expected'], row['original_expected'])
            self.assertIn(row['source'], proof.module(row, 'application'))
            self.assertIn('(do '+row['source']+')', proof.module(row, 'macro'))
            self.assertFalse(proof.sci.report_module().same_json(
                row['original_expected'], row['canonical_expected']))
            self.assertEqual(0, row['original_semantic_pass_credit'])

    def test_original_row_drift_rejects(self):
        rows = [json.loads(line) for line in cohort.CONTRACTS.read_text().splitlines()]
        rows[0]['body_sha256'] = '0'*64
        with patch.object(cohort.candidates, 'load', return_value=cohort.candidates.load()), patch.object(type(cohort.CONTRACTS), 'read_text', return_value='\n'.join(map(json.dumps, rows))):
            with self.assertRaisesRegex(ValueError, 'binding drift'):
                cohort.load()

    def test_missing_duplicate_and_reordered_contracts_reject(self):
        rows = [json.loads(line) for line in cohort.CONTRACTS.read_text().splitlines()]
        for changed in (rows[:-1], rows+[rows[0]], list(reversed(rows))):
            with patch.object(cohort.candidates, 'load', return_value=cohort.candidates.load()), patch.object(type(cohort.CONTRACTS), 'read_text', return_value='\n'.join(map(json.dumps, changed))):
                with self.assertRaisesRegex(ValueError, 'printing contracts'):
                    cohort.load()

    def test_actual_proof_required_and_all_fields_replayed(self):
        document = self.document()
        with patch.object(proof, 'verified_document', return_value=deepcopy(document)) as replay:
            rows = cohort.reconcile(document)
        self.assertEqual(1, replay.call_count)
        self.assertEqual(14, len(rows))
        self.assertTrue(all(r['status'] == 'version-difference' for r in rows.values()))
        self.assertTrue(all(r['original_semantic_pass_credit'] == 0 for r in rows.values()))
        changed = deepcopy(document); changed['summaries'] = [{'status': 'covered'}]
        with patch.object(proof, 'verified_document', return_value=deepcopy(document)):
            with self.assertRaisesRegex(ValueError, 'current artifact replay'):
                cohort.reconcile(changed)

    def test_one_missing_macro_frame_prevents_difference_credit(self):
        document = self.document()
        identity = document['source_bindings'][0]['id']
        document['frames'][identity]['macro-reused/Node']['status'] = 'mismatch'
        with patch.object(proof, 'verified_document', return_value=deepcopy(document)):
            row = cohort.reconcile(document)[document['source_bindings'][0]['obligation_id']]
        self.assertEqual('missing', row['status'])
        self.assertEqual('mismatch', row['actual_macro_contract_status'])
        self.assertEqual(0, row['original_semantic_pass_credit'])

    def test_empty_actual_frames_reject(self):
        with self.assertRaisesRegex(ValueError, 'actual source/phase frames'):
            proof.parse_raw('', cohort=cohort)

    def test_wrong_profile_and_stale_producer_reject(self):
        for field, value in (('profile', proof.PROFILE), ('compiler_build_producer_source_sha256', {})):
            document = self.document(); document[field] = value
            with self.assertRaisesRegex(ValueError, 'drift'):
                cohort.reconcile(document)

    def test_current_transitive_producer_binding(self):
        hashes = cohort.snapshot()
        for file in (cohort.__file__, cohort.candidates.__file__, proof.__file__,
                     cohort.CONTRACTS, proof.DRIVER):
            path = Path(file).resolve()
            self.assertEqual(proof.sci.digest(path.read_bytes()), hashes[str(path.relative_to(proof.ROOT))])

    def test_missing_executable_never_uses_serialized_status(self):
        document = self.document(); document['status'] = 'verified'
        with patch.object(proof, 'verified_document', side_effect=ValueError('missing executable')):
            with self.assertRaisesRegex(ValueError, 'missing executable'):
                cohort.reconcile(document)


if __name__ == '__main__':
    unittest.main()
