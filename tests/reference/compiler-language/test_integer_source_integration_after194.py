"""Mechanical consumer guards; real whole-source acceptance is a separate audit."""
import unittest
from unittest.mock import patch
import integer_source_integration_after194 as integration
import integer_source_context_after194 as source
class CurrentIntegerReportIntegration(unittest.TestCase):
    def test_real_absent_full_report_preserves_whole_universe_and_missing_source(self):
        witnesses=integration.inputs();rows,obligations,_=integration.full.load()
        with patch.object(source,'reconcile',side_effect=AssertionError('Absent proof must not reconcile')):
            report=integration.full.expand_report(integration.historical(),witnesses,{})
        self.assertEqual({r['id'] for r in report['assertions']},{r['id'] for r in rows+obligations})
        self.assertEqual(next(r for r in report['assertions'] if r['id']==source.SITE)['status'],'missing')
        self.assertFalse(report['strict_complete'])
    def test_consumer_rejects_detached_overlap_excluded_and_arity_credit(self):
        proof={source.SITE:{'mapping':source.mapping(),'status':'covered','original_assertion_semantic_pass_credit':1,
          'new_public_or_local_arity_links':0,'actual_phase_frames':{},'references':{},'fixture_id':'mechanical-only'}}
        controls=integration.mechanical_guards(proof)
        self.assertEqual(len(controls['controls']),9)
        self.assertTrue(all(c['status']=='rejected' for c in controls['controls']))
        self.assertEqual(controls['original_semantic_pass_credit'],0)
if __name__=='__main__':unittest.main()
