import copy
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import sci_cljc_source_probe as audit

class CljcSourceProbeGuards(unittest.TestCase):
    def setUp(self):
        self.rows=audit.catalog()
        self.expected=audit.frozen()

    def synthetic(self):
        """Explicit fabricated frames test accounting only, never semantic proof."""
        local={};refs={k:{} for k in ('release','checkout')}
        for contract in self.expected['contracts']:
            identity=contract['id'];frame={'status':'mismatch','observed':contract['canonical_expected']}
            if contract['classification']=='original-agreement':frame['status']='passed'
            local[identity]={variant:{engine:copy.deepcopy(frame) for engine in ('QuickJS','Node')} for variant in audit.VARIANTS}
            for pin in refs:refs[pin][identity]=copy.deepcopy(frame)
        rows=[r for r in self.rows if r['id'] in local]
        return rows,local,refs

    def test_exact_originals_retained_in_cljc_and_both_wrappers(self):
        self.assertEqual(len(self.rows),30)
        for row in self.rows:
            self.assertEqual(row['source_name'],'sci-original.cljc')
            self.assertEqual(row['reader_feature'],'cljs')
            for phase in ('application','macro'):
                text=audit.source(row['source'],phase)
                self.assertIn('(do '+row['source']+')',text)
                self.assertEqual(text.count(row['source']),1)

    def test_changed_original_expectation_source_or_row_rejects(self):
        for field,new in [('source','42'),('expected',{}),('original_row_sha256','changed'),('body_sha256','changed')]:
            rows=copy.deepcopy(self.rows);rows[0][field]=new
            with patch.object(audit,'generated_catalog',return_value=rows):
                with self.assertRaisesRegex(ValueError,'original source/body/row/expectation changed'):audit.catalog()

    def test_changed_canonical_literal_catalog_rejects(self):
        raw=audit.FROZEN.read_bytes().replace(b'"reader_feature": "cljs"',b'"reader_feature": "clj"',1)
        with patch.object(Path,'read_bytes',return_value=raw):
            with self.assertRaisesRegex(ValueError,'unreviewed source/canonical literal catalog'):audit.frozen()

    def test_only_twenty_reviewed_literals_can_complete(self):
        rows,local,refs=self.synthetic()
        result=audit.classify(rows,local,refs)
        self.assertEqual(len(result),20)
        self.assertEqual(sum(r['original_agreement'] for r in result),2)
        self.assertEqual(sum(r['classification']=='canonical-source-feature-difference' for r in result),18)
        self.assertTrue(all(r['original_semantic_pass_credit']==0 for r in result))
        self.assertEqual(len(self.expected['focused_gap_contracts']),3)
        self.assertEqual(len(self.expected['catalog'])-len(self.expected['contracts'])-len(self.expected['focused_gap_contracts']),7)

    def test_missing_failed_or_mismatching_actual_phase_cannot_complete(self):
        for mutation in ('missing','rejection','mismatch','pin-failure'):
            rows,local,refs=self.synthetic();identity=rows[0]['id']
            if mutation=='missing':del local[identity]['macro-reused']
            if mutation=='rejection':local[identity]['macro-reused']={'status':'rejected','detail':'real located rejection'}
            if mutation=='mismatch':local[identity]['macro-reused']['QuickJS']['observed']={'id':identity,'outcome':{'status':'returned','value':42},'trace':[],'stdout':''}
            if mutation=='pin-failure':refs['checkout'][identity]={'status':'rejected','detail':'reader error'}
            row=audit.classify(rows,local,refs)[0]
            self.assertFalse(row['complete_eight_local_phase_frames_and_both_pins'])
            self.assertEqual(row['classification'],'unresolved')

    def test_public_gap_pin_failures_are_not_confirmed(self):
        contract=self.expected['focused_gap_contracts'][0];identity=contract['id']
        row=next(r for r in self.rows if r['id']==identity)
        local={identity:{v:{'status':'rejected'} for v in audit.VARIANTS}}
        pins={k:{identity:{'status':'completed','observed':contract['canonical_expected']}} for k in ('release','checkout')}
        result=audit.classify([row],local,pins)[0]
        self.assertEqual(result['classification'],'confirmed-public-reader-gap')
        self.assertFalse(result['complete_eight_local_phase_frames_and_both_pins'])
        pins['release'][identity]={'status':'observation-error'}
        self.assertEqual(audit.classify([row],local,pins)[0]['classification'],'unresolved')

if __name__=='__main__':unittest.main()
