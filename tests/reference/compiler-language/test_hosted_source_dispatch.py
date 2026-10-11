"""Registration cannot silently fall through to generic CLI or partial receipts."""
import copy
import unittest
from unittest import mock
import hosted_source_dispatch as hosted
import report

class HostedSourceRegistration(unittest.TestCase):
    def test_missing_or_unknown_profile_rejected(self):
        for case in hosted.ordinary_profiles():
            self.assertTrue(hosted.validate_case(case))
            changed=copy.deepcopy(case);changed.pop('hosted_source_adapter')
            with self.assertRaises(ValueError):hosted.validate_case(changed)
            changed=copy.deepcopy(case);changed['hosted_source_adapter']['schema_version']=True
            with self.assertRaises(ValueError):hosted.validate_case(changed)
            changed=copy.deepcopy(case);changed['hosted_source_adapter']['profile']='generic-cli'
            with self.assertRaises(ValueError):hosted.validate_case(changed)
        with self.assertRaises(ValueError):hosted.validate_case({'id':'invented','hosted_source_adapter':{}})

    def test_full_source_dependency_expectation_and_order_drift_rejected(self):
        case=hosted.ordinary_profiles()[0]
        for key,value in [('source','true'),('closure_dependency_sha256',{}),('original_assertion_sha256',list(reversed(case['original_assertion_sha256']))),('expected',{'outcome':{'value':True}})]:
            changed=copy.deepcopy(case);changed[key]=value
            with self.assertRaises(ValueError):hosted.validate_case(changed)

    def test_configured_profile_cannot_use_generic_cli(self):
        case=hosted.register(hosted.configured.load())
        self.assertTrue(hosted.validate_case(case))
        with self.assertRaises(ValueError):report.local_layers(case,'unused',None,1)
        changed=copy.deepcopy(case);changed['compiler_data_readers']=[]
        with self.assertRaises(ValueError):hosted.validate_case(changed)

    def test_current_receipt_requires_all_frames_and_provenance(self):
        identity=hosted.imports.ID
        with self.assertRaises(ValueError):hosted.layers({'status':'passed','observations':{}},identity)
        observations={v+'/'+e:{'status':'passed'} for v in hosted.configured.VARIANTS for e in ('QuickJS','Node')}
        with self.assertRaises(ValueError):hosted.layers({'status':'passed','observations':observations},identity)

    def test_witnesses_cannot_omit_reorder_or_add_original_sites(self):
        inventory,witnesses,cases=report.load_contract('intended-language')
        case=cases[hosted.imports.ID]
        matching=[key for key,row in witnesses.items() if row['fixture_id']==case['id']]
        hosted.validate_witnesses(case,witnesses,inventory)
        missing=dict(witnesses);missing.pop(matching[0])
        with self.assertRaises(ValueError):hosted.validate_witnesses(case,missing,inventory)
        reordered={key:row for key,row in witnesses.items() if key not in matching}
        reordered.update((key,witnesses[key]) for key in reversed(matching))
        with self.assertRaises(ValueError):hosted.validate_witnesses(case,reordered,inventory)
        extra=copy.deepcopy(witnesses)
        key=next(key for key,row in extra.items() if row['fixture_id']!=case['id'])
        extra[key]['fixture_id']=case['id']
        with self.assertRaises(ValueError):hosted.validate_witnesses(case,extra,inventory)

    def test_dispatch_does_not_accept_generic_case_or_alternate_checkout(self):
        with self.assertRaises(ValueError):hosted.execute([{'id':'generic'}],'/tmp/unused-hosted-test',1)
        with self.assertRaises(ValueError):hosted.execute(hosted.ordinary_profiles(),'/tmp/unused-hosted-test',1,checkout='/tmp/other-checkout')

if __name__=='__main__':unittest.main()
