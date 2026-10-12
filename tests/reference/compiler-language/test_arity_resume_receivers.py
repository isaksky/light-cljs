import copy
from pathlib import Path
import unittest
from unittest import mock
import arity_resume_receivers as cohort


class ArityResumeReceiversTests(unittest.TestCase):
    def test_whole_contracts_keep_exact_receiver_signatures_and_all_phase_links(self):
        exported=cohort.exports()
        ordinary=exported['obligation-witnesses.jsonl']
        self.assertEqual(31,len(ordinary))
        self.assertEqual(2,len(exported['full-cases.jsonl']))
        self.assertEqual([row['obligation_id'] for row in ordinary],
            [row['obligation_id'] for row in exported['phase-witnesses.jsonl']])
        self.assertTrue(all(row['assertion_ids']==[] for row in ordinary))

    def test_receiver_method_definitions_are_not_direct_calls(self):
        source='(reify ITransientCollection (-conj! [this x] this) (-persistent! [_] :unused))'
        self.assertEqual([],cohort.direct_calls(source,'-conj!'))
        self.assertEqual([],cohort.direct_calls(source,'-persistent!'))

    def test_saved_valid_subseq_calls_do_not_use_invalid_direct_controls(self):
        generated=cohort.exports()['arity-receiver-call-receipts.jsonl']
        saved=[row for row in generated if row.get('saved_alias')]
        self.assertEqual(2,len(saved))
        self.assertTrue(all(row['direct_arities']==[3,5] for row in saved))
        self.assertTrue(all(call['argument_count'] in [3,5] for row in saved for call in row['actual_calls']))

    def test_detached_saved_operation_is_rejected(self):
        row=next(row for row in cohort.rows() if row['id']=='sorted-full')
        row={**row,'source':row['source'].replace('sub subseq','sub reverse')}
        selected=next(row for row in cohort.SELECTION if row.get('saved_alias')=='sub')
        with self.assertRaisesRegex(ValueError,'alias detached'):
            cohort.saved_calls(row,{'name':'subseq'},selected)

    def test_whole_original_contract_effect_changes_fail_closed(self):
        original=Path.read_text
        def altered(path,*args,**kwargs):
            text=original(path,*args,**kwargs)
            if str(path).endswith('compiler_transients.rs'):
                text=text.replace('(swap! events conj :transient)','(swap! events conj :changed)')
            return text
        with mock.patch.object(Path,'read_text',altered),self.assertRaisesRegex(ValueError,'whole original receiver contract drift'):
            cohort.exports()

    def test_changed_independent_literal_is_rejected(self):
        original=Path.read_text
        def altered(path,*args,**kwargs):
            text=original(path,*args,**kwargs)
            if str(path).endswith('compiler_transients.rs'):
                text=text.replace('let expected = "[[1 20]', 'let expected = "[[1 21]')
            return text
        with mock.patch.object(Path,'read_text',altered),self.assertRaisesRegex(ValueError,'independent receiver literal changed'):
            cohort.exports()

    def test_receiver_inclusive_signature_drift_is_rejected(self):
        changed=copy.deepcopy(cohort.SELECTION)
        changed[0]['signature_sha256']='0'*64
        with mock.patch.object(cohort,'SELECTION',changed),self.assertRaisesRegex(ValueError,'receiver-inclusive signature drift'):
            cohort.exports()


if __name__=='__main__':
    unittest.main()
