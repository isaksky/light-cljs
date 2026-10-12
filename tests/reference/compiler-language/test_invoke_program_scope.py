import unittest
from unittest import mock
import invoke_program_scope as scope


class InvokeProgramScopeTests(unittest.TestCase):
    def test_only_complete_exact_emission_fixture_gets_scope_no_passes(self):
        rows=scope.validate()
        self.assertEqual(1,len(rows))
        row=rows[0]
        self.assertEqual('program::src/test/cljs/cljs/invoke_test.cljs',row['id'])
        self.assertEqual('upstream-internals',row['category'])
        self.assertEqual(11,row['original_emission_assertions'])
        for key in ['semantic_pass_credit','local_execution_credit','macro_phase_credit']:
            self.assertIs(False,row[key])

    def test_deleted_original_declared_call_fails_closed(self):
        original=scope.pinned
        def changed(path,*args,**kwargs):
            raw=original(path,*args,**kwargs)
            return raw.replace(b'(declared-fn 1 2)',b'nil') if path==scope.MODULE else raw
        with mock.patch.object(scope,'pinned',changed),self.assertRaisesRegex(ValueError,'original invocation module drift'):
            scope.validate()

    def test_removed_original_record_tail_fails_closed(self):
        original=scope.pinned
        def changed(path,*args,**kwargs):
            raw=original(path,*args,**kwargs)
            return raw.replace(b'(:foo-field-a foo-record)',b'nil') if path==scope.MODULE else raw
        with mock.patch.object(scope,'pinned',changed),self.assertRaisesRegex(ValueError,'original invocation module drift'):
            scope.validate()

    def test_changed_optimizer_caller_fails_closed(self):
        with mock.patch.object(scope,'CALLER_SHA256','0'*64),self.assertRaisesRegex(ValueError,'original invocation optimizer caller drift'):
            scope.validate()

    def test_existing_optimizer_boundary_cannot_be_invented(self):
        with mock.patch.dict(scope.inv.INTERNAL_TESTS['compiler_tests.clj'],{scope.CALLER_TEST:'public language behavior'}),self.assertRaisesRegex(ValueError,'existing exact optimizer scope boundary drift'):
            scope.validate()

    def test_original_compiler_settings_and_complete_caller_are_retained(self):
        script=scope.compiler_script()
        self.assertIn('comp-api/with-core-cljs',script)
        self.assertIn('{:static-fns true}',script)
        self.assertIn('(declared-fn',scope.pinned(scope.MODULE).decode())
        self.assertIn(':pass 11 :fail 0 :error 0',script)
        self.assertNotIn('(defn declared-fn',scope.pinned(scope.MODULE).decode())


if __name__=='__main__':
    unittest.main()
