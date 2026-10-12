import copy
from pathlib import Path
import unittest
from unittest import mock
import arity_after185 as cohort


class ArityAfter185Tests(unittest.TestCase):
    def test_current_exact_sources_and_intended_operation_rows(self):
        exports = cohort.exports()
        self.assertEqual(54, len(exports['obligation-witnesses.jsonl']))
        self.assertEqual(32, len(cohort.rows()))
        self.assertEqual(256, len(self.frames()))
        self.assertTrue(all(not row['assertion_ids'] for row in exports['obligation-witnesses.jsonl']))
        self.assertFalse(any('::checked-aget::' in row['obligation_id'] or '::checked-aset::' in row['obligation_id'] for row in exports['obligation-witnesses.jsonl']))

    def changed_source(self, name, before, after):
        original = Path.read_text
        def changed(path, *args, **kwargs):
            result = original(path, *args, **kwargs)
            return result.replace(before, after) if path.name == name else result
        return mock.patch.object(Path, 'read_text', changed)

    def test_original_hundred_element_loop_is_preserved(self):
        with self.changed_source('cljs-transients-collections-test-test-transient-edge-case-2.cljs', '(range 100)', '(range 1)'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_original_while_callback_must_be_invoked(self):
        with self.changed_source('cljs-residual-completion-cljs-2866-test.cljs', '(atom 1)', '(atom 0)'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_original_apply_alias_cannot_be_replaced(self):
        with self.changed_source('cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aget-test.cljs', '(apply checked-aget args)', '(checked-aget nil 1)'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_alias_forwarding_guard_rejects_direct_substitution(self):
        selected = next(row for row in cohort.SELECTION if row['evidence_mode']=='applied-alias')
        row = next(row for row in cohort.rows() if row['id']==selected['workflow'])
        target={'name': 'checked-aget'}
        with self.assertRaisesRegex(ValueError, 'apply argument forwarding drift'):
            cohort.operation_evidence(row['source'].replace('(apply checked-aget args)','(checked-aget nil 1)'), selected, target)

    def test_printing_controls_have_actual_binding_bodies(self):
        receipts=cohort.exports()['arity-after185-call-receipts.jsonl']
        controls=[row for row in receipts if row['evidence_mode']=='binding-control']
        self.assertEqual(10,len(controls))
        for row in controls:
            self.assertTrue(row['direct_calls'])
            self.assertTrue(all(site['binding_source'].startswith('(binding ') for site in row['direct_calls']))
            self.assertEqual([],row['direct_arities'])

    def test_absent_printing_control_fails_closed(self):
        selected=next(row for row in cohort.SELECTION if row['evidence_mode']=='binding-control')
        with self.assertRaisesRegex(ValueError,'absent demanded printing binding control'):
            cohort.operation_evidence('(binding [*unrelated* 1] true)',selected,{})

    def test_original_copy_arguments_callback_and_apply_are_preserved(self):
        with self.changed_source('cljs-residual-source-context-apply.cljs', '(copy-arguments a)', '(conj a 1)'), self.assertRaisesRegex(ValueError, 'complete original oracle drift'):
            cohort.exports()

    def test_missing_actual_macro_invocation_fails_closed(self):
        with self.changed_source('compiler_arity_after185.rs', '{invoke}', 'true'), self.assertRaisesRegex(ValueError, 'whole source invocation'):
            cohort.exports()

    def test_private_and_printing_callbacks_keep_original_arity(self):
        reviewed=copy.deepcopy(cohort.REVIEWED)
        next(row for row in reviewed if row['id']=='cljs-printing-completion-test-print-knobs')['invoke']='(run)'
        with mock.patch.object(cohort,'REVIEWED',reviewed), self.assertRaisesRegex(ValueError,'callback invocation arity drift'):
            cohort.exports()

    def test_operation_hash_signature_and_actual_arity_are_frozen(self):
        for field,value,message in [('body_sha256','0'*64,'operation/body drift'),('signature_sha256','0'*64,'signature drift'),('direct_arities',[0],'actual source calls')]:
            selected=copy.deepcopy(cohort.SELECTION)
            row=next(row for row in selected if row['evidence_mode']=='direct')
            row[field]=value
            with mock.patch.object(cohort,'SELECTION',selected),self.assertRaisesRegex(ValueError,message):
                cohort.exports()

    def test_original_equiv_receivers_and_full_vector_remain(self):
        rows=cohort.rows()
        row=next(row for row in rows if row['id']=='cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests')
        self.assertEqual({'$vector':[True]*165},row['case']['expected']['outcome']['value'])
        self.assertEqual('['+' '.join(['true']*165)+']',cohort.EXPECTED_LITERAL_RAW[row['id']])
        with self.changed_source(Path(row['fixture']).name,'(map-entry-interface-tests (first (sorted-map :key :val :other :value)))','true'),self.assertRaisesRegex(ValueError,'complete original oracle drift'):
            cohort.exports()

    def frames(self):
        return ['arity-after185-observation\t'+row['id']+'\t'+host+'\t'+phase+'\t'+engine+'\t'+cohort.EXPECTED_LITERAL_RAW[row['id']]
            for row in cohort.rows() for host in ['fresh','reused']
            for phase in ['application','macro'] for engine in ['QuickJS','Node']]

    def test_actual_frames_require_complete_ordered_success(self):
        lines=self.frames()
        self.assertEqual(256,len(cohort.parse_frames('\n'.join(lines))))
        for changed in [lines[:-1],list(reversed(lines)),[*lines,lines[-1]],[line.replace('\ttrue','\tfalse') for line in lines]]:
            with self.assertRaises(ValueError):cohort.parse_frames('\n'.join(changed))


if __name__=='__main__':unittest.main()
