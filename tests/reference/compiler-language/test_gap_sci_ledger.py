"""Failure accounting controls; synthetic metadata grants no execution credit."""
from copy import deepcopy
import unittest
import full_ledger as ledger


class CanonicalGapFailureAccounting(unittest.TestCase):
    def source(self):
        layers={kind:{'status':'mismatch','observed':{'value':True}}
                for kind in ('javascript','release','checkout')}
        row={'status':'version-difference','layers':layers,
            'columns':{'quickjs_macro':{'status':'passed'}},
            'gap_sci_context_reconciliation':{'id':'control',
                'reconciliation':'verified-canonical-sci-difference',
                'original_expectation_status':'unresolved','original_semantic_pass_credit':0,
                'canonical_contract_status':'passed','actual_macro_contract_status':'passed',
                'checked_pin_contract_status':{'release':'passed','checkout':'passed'}}}
        return {'assertions':[row]}, {'control':deepcopy(layers)}

    def failed(self,report,evidence):
        return ledger.sci_execution_failed(report,evidence,{'mismatch','rejected','failed'})

    def test_only_verified_difference_excuses_original_mismatch(self):
        report,evidence=self.source()
        self.assertFalse(self.failed(report,evidence))
        for status in ('missing','covered','unrelated'):
            changed=deepcopy(report);changed['assertions'][0]['status']=status
            self.assertTrue(self.failed(changed,evidence))

    def test_every_source_pin_and_actual_phase_condition_is_required(self):
        report,evidence=self.source()
        fields={'reconciliation':'original-contract-unresolved',
            'original_expectation_status':'passed','original_semantic_pass_credit':1,
            'canonical_contract_status':'mismatch','actual_macro_contract_status':'mismatch',
            'checked_pin_contract_status':{'release':'passed','checkout':'not-run'}}
        for key,value in fields.items():
            changed=deepcopy(report)
            changed['assertions'][0]['gap_sci_context_reconciliation'][key]=value
            self.assertTrue(self.failed(changed,evidence),key)
        changed=deepcopy(report);changed['assertions'][0]['columns']['quickjs_macro']['status']='not-run'
        self.assertTrue(self.failed(changed,evidence))

    def test_boolean_number_or_detached_layers_cannot_be_reconciled(self):
        report,evidence=self.source()
        changed=deepcopy(evidence);changed['control']['javascript']['observed']['value']=1
        self.assertTrue(self.failed(report,changed))
        changed=deepcopy(report);changed['assertions'][0]['layers']={}
        self.assertTrue(self.failed(changed,evidence))

    def test_unrelated_failure_and_rejection_still_fail(self):
        report,evidence=self.source()
        evidence['another']={'javascript':{'status':'mismatch'}}
        self.assertTrue(self.failed(report,evidence))
        report,evidence=self.source();evidence['control']['javascript']['status']='rejected'
        report['assertions'][0]['layers']=deepcopy(evidence['control'])
        self.assertTrue(self.failed(report,evidence))


if __name__=='__main__':
    unittest.main()
