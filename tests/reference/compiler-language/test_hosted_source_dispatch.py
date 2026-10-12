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



class AddedHostedSourceRoutes(unittest.TestCase):
    """Synthetic receipt controls test routing; they confer no execution credit."""
    @staticmethod
    def api_layer(case):
        rows = {}
        for variant in ('application-fresh', 'application-reused', 'macro-fresh', 'macro-reused'):
            phase, host = variant.split('-')
            for engine in ('QuickJS', 'Node'):
                rows[variant+'/'+engine] = {
                    'status': 'passed', 'observed': copy.deepcopy(case['expected']),
                    'actual_frame': {'id': case['id'], 'phase': phase, 'host': host, 'engine': engine, 'raw': 'true'},
                    'raw_observation_sha256': hosted.local_apis.digest('true'),
                    'compiler_build_producer_source_sha256': hosted.local_apis.snapshot(),
                    'audit_executable_sha256': 'a'*64, 'compiled_js_sha256': 'b'*64,
                    'module_source_sha256': 'c'*64}
        return {'status': 'passed', 'observed': copy.deepcopy(case['expected']),
                'observations': rows, 'hosted_source_adapter': hosted.local_apis.PROFILE,
                'generic_cli': 'not claimed'}

    def test_known_domain_does_not_auto_export_121_cases(self):
        self.assertEqual(len(hosted.LOCAL_API_IDS), 121)
        self.assertFalse(any(c['id'] in hosted.LOCAL_API_IDS for c in hosted.ordinary_profiles()))
        self.assertEqual([c['id'] for c in hosted.ordinary_profiles()],
                         [hosted.imports.ID, *[s[1] for s in hosted.namespaces.SPECS], hosted.custom.ID])
        for case in hosted.local_apis.cases():
            self.assertTrue(hosted.validate_case(case))

    def test_api_source_literal_types_marker_and_fake_origins_rejected(self):
        case = hosted.local_apis.cases()[0]
        for key, value in [('source', 'true'), ('expected', {'id': case['id'], 'outcome': {'status': 'returned', 'value': 1}, 'trace': [], 'stdout': ''}),
                           ('complete_original_provenance', [{'body_id': 'invented', 'source_sha256': 'a'*64}])]:
            changed = copy.deepcopy(case); changed[key] = value
            with self.assertRaises(ValueError): hosted.validate_case(changed)
        changed = copy.deepcopy(case); changed['hosted_source_adapter']['schema_version'] = True
        with self.assertRaises(ValueError): hosted.validate_case(changed)
        changed = copy.deepcopy(case); changed.pop('hosted_source_adapter')
        with self.assertRaises(ValueError): hosted.validate_case(changed)
        with self.assertRaises(ValueError): hosted.validate_case({'id': 'local-api-source-invented'})
        with self.assertRaises(ValueError): report.local_layers(case, 'unused', None, 1)

    def test_api_does_not_invent_original_assertion_witnesses(self):
        case = hosted.local_apis.cases()[0]
        hosted.validate_witnesses(case, {}, {})
        with self.assertRaises(ValueError):
            hosted.validate_witnesses(case, {'fake': {'fixture_id': case['id'], 'assertion_id': 'fake'}}, {})

    def test_api_frames_require_complete_distinct_phases_engines_hosts(self):
        case = hosted.local_apis.cases()[0]; good = self.api_layer(case)
        self.assertIs(hosted.local_api_layers(case, good), good)
        for mutate in [lambda r: r['observations'].pop('macro-reused/Node'),
                       lambda r: r.update(observations=dict(reversed(list(r['observations'].items())))),
                       lambda r: r['observations']['macro-fresh/Node']['actual_frame'].update(phase='application'),
                       lambda r: r['observations']['application-fresh/QuickJS']['actual_frame'].update(raw='1'),
                       lambda r: r['observations']['macro-fresh/QuickJS']['observed']['outcome'].update(value=1),
                       lambda r: r['observations']['macro-reused/QuickJS'].update(compiler_build_producer_source_sha256={}),
                       lambda r: r['observations']['macro-reused/QuickJS'].update(module_source_sha256='x'*64),
                       lambda r: r['observations']['macro-reused/QuickJS'].pop('audit_executable_sha256'),
                       lambda r: r.update(generic_cli='passed')]:
            changed = copy.deepcopy(good); mutate(changed)
            with self.assertRaises(ValueError): hosted.local_api_layers(case, changed)

    def test_api_executor_once_exact_subset_and_pin_outcomes_retained(self):
        from pathlib import Path
        cases = hosted.local_apis.cases()[:2]
        results = {c['id']: {'javascript': self.api_layer(c), 'release': {'status': 'passed'},
                            'checkout': {'status': 'failed'}} for c in cases}
        with mock.patch.object(hosted.local_apis, 'execute', return_value=results) as execute:
            observed = hosted._execute(cases, Path('/tmp/mock-only-api-route'), 7, 'both', None)
            execute.assert_called_once_with(cases, Path('/tmp/mock-only-api-route/local-api'), 7, 'both', None)
            self.assertIs(observed[cases[0]['id']]['checkout'], results[cases[0]['id']]['checkout'])
            self.assertEqual(observed[cases[0]['id']]['checkout']['status'], 'failed')
        for malformed in [{}, {**results, 'invented': {}}]:
            with mock.patch.object(hosted.local_apis, 'execute', return_value=malformed):
                with self.assertRaises(ValueError): hosted._execute(cases, Path('/tmp/mock-only-api-route'), 7)
        with mock.patch.object(hosted.local_apis, 'execute') as execute:
            with self.assertRaises(ValueError): hosted._execute([cases[0], cases[0]], Path('/tmp/mock-only-api-route'), 7)
            execute.assert_not_called()

    def test_custom_exact_original_body_witness_and_executor_route(self):
        from pathlib import Path
        case = hosted.custom.case()
        self.assertTrue(hosted.validate_case(case))
        origin = case['complete_original_provenance'][0]
        inventory = {'original': {'id': 'original', 'body_id': origin['body_id'],
                                  'body_sha256': origin['source_sha256'], 'disposition': 'in-scope'}}
        witnesses = {'original': {'fixture_id': case['id'], 'assertion_id': 'original'}}
        hosted.validate_witnesses(case, witnesses, inventory)
        with self.assertRaises(ValueError): hosted.validate_witnesses(case, {}, inventory)
        changed = copy.deepcopy(case); changed['source'] = 'true'
        with self.assertRaises(ValueError): hosted.validate_case(changed)
        observations = {v+'/'+e: {'status': 'passed', 'observed': copy.deepcopy(case['expected']),
                        'raw_observation_sha256': 'a'*64, 'compiler_build_producer_source_sha256': {'mock': 'a'*64},
                        'audit_executable_sha256': 'b'*64, 'compiled_js_sha256': 'c'*64}
                        for v in hosted.custom.VARIANTS for e in ('QuickJS', 'Node')}
        result = {case['id']: {'javascript': {'status': 'passed', 'observations': observations},
                              'release': {'status': 'passed'}, 'checkout': {'status': 'failed'}}}
        with mock.patch.object(hosted.custom, 'execute', return_value=result) as execute:
            observed = hosted._execute([case], Path('/tmp/mock-only-custom-route'), 9, 'release', None)
            execute.assert_called_once_with([case], Path('/tmp/mock-only-custom-route/custom'), 9, 'release', None)
            self.assertEqual(observed[case['id']]['checkout']['status'], 'failed')
        del observations['macro-reused/Node']
        with mock.patch.object(hosted.custom, 'execute', return_value=result):
            with self.assertRaises(ValueError): hosted._execute([case], Path('/tmp/mock-only-custom-route'), 9)

if __name__=='__main__':unittest.main()
