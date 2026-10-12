"""Fail closed on the exact optimizer context; preserve public obligations."""
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import extract_inventory as inv
import inference_optimizer_scope as scope

class InferenceOptimizerScopeGuards(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.sources={path:scope.pinned(path) for path in scope.SOURCE_SHA256}

    def test_one_exact_body_supplies_no_public_or_phase_credit(self):
        rows=scope.validate()
        self.assertEqual(len(rows),1)
        row=rows[0]
        self.assertEqual(row['id'],'body::'+scope.BODY_ID)
        self.assertEqual(row['original_predicate_forms'],65)
        self.assertEqual(row['source_span']['sha256'],scope.BODY_SHA256)
        for key in ('semantic_pass_credit','local_execution_credit','macro_phase_credit'):
            self.assertIs(row[key],False)
        self.assertEqual(row['category'],'upstream-internals')

    def test_revision_and_whole_source_cannot_drift(self):
        with patch.object(inv,'REVISION','unreviewed'):
            with self.assertRaisesRegex(ValueError,'revision drift'):scope.validate()
        altered=dict(self.sources);altered[scope.TEST_PATH]+=b'\n'
        with patch.object(scope,'pinned',side_effect=altered.__getitem__), \
             patch.object(Path,'read_bytes',side_effect=altered.values()):
            with self.assertRaisesRegex(ValueError,'complete source drift'):scope.validate()

    def test_working_checkout_must_match_pin(self):
        with patch.object(scope,'pinned',side_effect=self.sources.__getitem__), \
             patch.object(Path,'read_bytes',return_value=b'changed'):
            with self.assertRaisesRegex(ValueError,'working source differs'):scope.validate()

    def reject_reduced(self,path,old,new):
        altered=dict(self.sources);self.assertIn(old.encode(),altered[path])
        altered[path]=altered[path].replace(old.encode(),new.encode(),1)
        hashes={p:scope.digest(v) for p,v in altered.items()}
        def read(path_object):
            return altered[str(path_object.relative_to(inv.UPSTREAM))]
        with patch.object(scope,'pinned',side_effect=altered.__getitem__), \
             patch.object(scope,'SOURCE_SHA256',hashes), \
             patch.object(Path,'read_bytes',read):
            with self.assertRaises(ValueError):scope.validate()

    def test_original_all_forms_and_neighbors_stay_frozen(self):
        self.reject_reduced(scope.TEST_PATH,'    (cljs.pprint/float? nil)','    (float? nil)')
        self.reject_reduced(scope.TEST_PATH,'    (object? nil)\n','')
        self.reject_reduced(scope.TEST_PATH,'(deftest test-cljs-2825','#_(deftest test-cljs-2825')
        self.reject_reduced(scope.TEST_PATH,'(is true)','(is false)')

    def test_genuine_private_truth_macro_cannot_be_substituted(self):
        self.reject_reduced(scope.MACRO_PATH,'(with-redefs [cljs.core/truth_', '(with-redefs [cljs.core/identity')
        self.reject_reduced(scope.MACRO_PATH,'(cljs.test/is (not (truth_-called? ~form)))','(cljs.test/is true)')

    def test_public_api_rows_and_neighbor_assertions_receive_no_exclusion(self):
        rows=[json.loads(line) for line in (inv.HERE/'obligations.jsonl').read_text().splitlines()]
        for name in ('object?','char?','integer?','map?','tagged-literal?'):
            public=[r for r in rows if r['name']==name]
            self.assertTrue(public,name)
            self.assertTrue(all(r['disposition']=='intended' for r in public),name)
        assertions=[json.loads(line) for line in (inv.HERE/'full-inventory.jsonl').read_text().splitlines()]
        neighbors=[r for r in assertions if r.get('source')==scope.TEST_PATH and r.get('test') in ('cljs-2866-test','cljs-3068-test')]
        self.assertEqual(len(neighbors),2)
        self.assertTrue(all(r['disposition']=='in-scope' for r in neighbors))
        returned={r['id'] for r in scope.validate()}
        self.assertTrue(all(r['id'] not in returned for r in [x for x in rows if x.get('obligation_kind')=='public-api']+neighbors))

if __name__=='__main__':unittest.main()
