import copy
import unittest
import tempfile
from pathlib import Path
import sci_reviewed_source_profiles as audit


class ReviewedProfileGuards(unittest.TestCase):
    def test_partition_exact_four_originals_only(self):
        rows = audit.load()
        ordinary, selected = audit.partition([{'sci_id': 'unreviewed'}, *rows])
        self.assertEqual(ordinary, [{'sci_id': 'unreviewed'}])
        self.assertEqual(selected, rows)

    def test_partition_rejects_reduced_source_wrong_literal_or_duplicate(self):
        rows = audit.load()
        for key, value in [('source', 'true'), ('source_sha256', '0'*64),
                           ('expected', {}), ('body_sha256', '0'*64)]:
            wrong = copy.deepcopy(rows); wrong[0][key] = value
            with self.assertRaises(ValueError): audit.partition(wrong)
        with self.assertRaises(ValueError): audit.partition([*rows, rows[0]])

    def test_registered_expression_catalog_binds_original_corpus_by_hash(self):
        originals = {r['sci_id'] for r in audit.load()}
        rows = [r for r in audit.sci.load_all() if r['sci_id'] in originals]
        ordinary, selected = audit.partition(rows)
        self.assertEqual(ordinary, [])
        self.assertEqual([r['sci_id'] for r in selected], [r['sci_id'] for r in rows])
        wrong = copy.deepcopy(rows); wrong[0]['original_source_sha256'] = '0'*64
        with self.assertRaises(ValueError): audit.partition(wrong)

    def document(self):
        return {'schema_version': 1, 'profile': audit.PROFILE, 'source_bindings': audit.load(),
            'compiler_build_producer_source_sha256': audit.snapshot(),
            'cohorts': {'symbols': {}, 'records': {}}, 'status': 'passed'}

    def test_no_status_only_cached_or_partial_sidecar_credit(self):
        for document in [{'status': 'passed'}, {'schema_version': 1, 'profile': audit.PROFILE}]:
            with self.assertRaises(ValueError): audit.reconcile(document)
        document = self.document(); document['cohorts'].pop('records')
        with self.assertRaises(ValueError): audit.reconcile(document)

    def test_schema_bool_or_profile_drift_rejected(self):
        for key, value in [('schema_version', True), ('profile', 'generic-default-header')]:
            document = self.document(); document[key] = value
            with self.assertRaises(ValueError): audit.reconcile(document)

    def test_current_producer_and_original_binding_drift_rejected(self):
        for key, value in [('compiler_build_producer_source_sha256', {}), ('source_bindings', [])]:
            document = self.document(); document[key] = value
            with self.assertRaises(ValueError): audit.reconcile(document)

    def test_missing_raw_artifacts_never_grant_receipt_pass(self):
        document = self.document()
        document['cohorts']['symbols'] = {'artifact_root': '/tmp/no-such-reviewed-source-proof', 'evidence': {'status': 'passed'}}
        with self.assertRaises((OSError, ValueError)): audit.reconcile(document)

    def test_emitted_reference_namespace_dependency_or_extra_file_drift_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); (root/'user.js').write_text('original compiled namespace')
            original = audit.artifact_manifest(root)
            (root/'user.js').write_text('changed compiled namespace')
            with self.assertRaises(ValueError): audit.same(audit.artifact_manifest(root), original, 'drift')
            (root/'user.js').write_text('original compiled namespace')
            (root/'unreviewed-dependency.js').write_text('extra dependency')
            with self.assertRaises(ValueError): audit.same(audit.artifact_manifest(root), original, 'drift')


if __name__ == '__main__': unittest.main()
