from copy import deepcopy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import sci_gap_canonical_context as cohort


def keyword(name):
    return {'$keyword': {'namespace':None,'name':name,'spelling':name}}


def macro_packet(value, trace):
    return {'id':'control','outcome':{'status':'returned','value':{'$vector':[
        {'$vector':[keyword('returned'),value]}, {'$vector':trace}]}},'trace':[],'stdout':''}


class GapCanonicalContextTests(unittest.TestCase):
    def document(self):
        rows=cohort.load()
        root=Path('/tmp/canonical-reconcile-guard').resolve()
        frames={row['id']:{variant+'/'+engine:{'status':'mismatch' if row['focused_owner'] else 'passed',
                    'observed':deepcopy(row['canonical_expected'])}
                for variant in cohort.VARIANTS for engine in ('QuickJS','Node')} for row in rows}
        layers={row['id']:{'javascript':{'status':'mismatch','observed':deepcopy(row['canonical_expected'])},
                'release':{'status':'passed' if row['focused_owner'] else 'mismatch','observed':deepcopy(row['canonical_expected'])},
                'checkout':{'status':'passed' if row['focused_owner'] else 'mismatch','observed':deepcopy(row['canonical_expected'])}} for row in rows}
        return {'schema_version':1,'profile':cohort.PROFILE,'source_bindings':rows,
            'compiler_build_producer_source_sha256':cohort.snapshot(),
            'reference_artifacts_root':str(root/'originals'),'phase_artifacts_root':str(root/'phases'),
            'frames':frames,'reference_evidence':{'evidence':layers},
            'raw_phase_observations':'guard raw','summaries':[],'original_semantic_pass_credit':0}

    def test_exact_source_contracts_preserve_original_results_and_known_gaps(self):
        rows=cohort.load()
        self.assertEqual(30,len(rows))
        self.assertEqual({'light-cljs-167','light-cljs-168','light-cljs-169'},
            {r['focused_owner'] for r in rows if r['focused_owner']})
        originals={r['sci_id']:r for r in cohort.candidates.load()}
        for row in rows:
            self.assertEqual(originals[row['sci_id']]['expected'],row['original_expected'])
            self.assertEqual(0,row['original_semantic_pass_credit'])
            self.assertIn(row['source'],cohort.module(row,'application'))
            self.assertIn('(do '+row['source']+')',cohort.module(row,'macro'))

    def test_actual_macro_packet_keeps_value_and_ordered_effects(self):
        actual=macro_packet({'$vector':[False,1]},[keyword('before'),keyword('after')])
        converted=cohort.ordinary_packet(actual,'macro')
        self.assertEqual({'$vector':[False,1]},converted['outcome']['value'])
        self.assertEqual([keyword('before'),keyword('after')],converted['trace'])
        changed=deepcopy(converted);changed['outcome']['value']['$vector'][0]=0
        self.assertFalse(cohort.sci.report_module().same_json(converted,changed))

    def test_actual_macro_failure_keeps_category_and_prior_effects(self):
        packet=macro_packet(None,[keyword('before')])
        packet['outcome']['value']['$vector'][0]={'$vector':[keyword('threw'),'type-error','message']}
        converted=cohort.ordinary_packet(packet,'macro')
        self.assertEqual({'status':'threw','error':{'category':'type-error'}},converted['outcome'])
        self.assertEqual([keyword('before')],converted['trace'])

    def test_macro_effect_leak_rejects(self):
        packet=macro_packet(1,[]);packet['stdout']='unclaimed'
        with self.assertRaisesRegex(ValueError,'effects leaked'):
            cohort.ordinary_packet(packet,'macro')

    def test_bare_value_and_unknown_macro_error_tags_reject(self):
        packet={'id':'control','outcome':{'status':'returned','value':1},'trace':[],'stdout':''}
        with self.assertRaisesRegex(ValueError,'macro packet'):
            cohort.ordinary_packet(packet,'macro')
        packet=macro_packet(None,[])
        packet['outcome']['value']['$vector'][0]={'$vector':[keyword('threw'),'timeout','message']}
        with self.assertRaisesRegex(ValueError,'result/error tag'):
            cohort.ordinary_packet(packet,'macro')

    def test_missing_frame_cannot_be_semantic_evidence(self):
        with self.assertRaisesRegex(ValueError,'actual source/phase frames'):
            cohort.parse_raw('')

    def test_warmup_requires_actual_ordinary_literal_compilation(self):
        cohort.verify_warmup('(() => {\n  return 42;\n})()\n')
        for invalid in ('', '42', '(() => {\n  return 41;\n})()\n',
                '(__lightSessionRuntime0) => (() => {\n  return 42;\n})()\n'):
            with self.assertRaisesRegex(ValueError, 'actual compiled warmup'):
                cohort.verify_warmup(invalid)

    def test_changed_original_body_binding_rejects(self):
        original=cohort.CONTRACTS.read_text()
        rows=[json.loads(line) for line in original.splitlines()]
        rows[0]['source_sha256']='0'*64
        with patch.object(type(cohort.CONTRACTS),'read_text',return_value='\n'.join(map(json.dumps,rows))):
            with self.assertRaises((ValueError,KeyError)):
                cohort.load()

    def test_reconcile_replays_all_fields_and_preserves_original_layers(self):
        document=self.document()
        with patch.object(cohort,'verified_document',return_value=deepcopy(document)) as replay:
            result=cohort.reconcile(document)
        self.assertEqual(1,replay.call_count)
        self.assertEqual(30,len(result))
        self.assertEqual(27,sum(r['status']=='version-difference' for r in result.values()))
        self.assertEqual(3,sum(r['status']=='missing' for r in result.values()))
        for row in result.values():
            self.assertEqual(document['reference_evidence']['evidence'][row['id']],row['layers'])
            self.assertEqual(0,row['original_semantic_pass_credit'])
            if row['focused_owner']:
                self.assertEqual('mismatch',row['actual_macro_contract_status'])
                self.assertEqual('original-contract-unresolved',row['reconciliation'])
        result[next(iter(result))]['layers']['javascript']['status']='changed'
        self.assertEqual('mismatch',document['reference_evidence']['evidence'][document['source_bindings'][0]['id']]['javascript']['status'])

    def test_reconcile_rejects_cached_status_raw_or_frame_changes(self):
        verified=self.document()
        mutations=[]
        changed=deepcopy(verified);changed['summaries']=[{'status':'covered'}];mutations.append(changed)
        changed=deepcopy(verified);changed['raw_phase_observations']='invented';mutations.append(changed)
        changed=deepcopy(verified);identity=verified['source_bindings'][-1]['id']
        changed['frames'][identity]['macro-fresh/QuickJS']['status']='passed';mutations.append(changed)
        changed=deepcopy(verified);changed['original_semantic_pass_credit']=30;mutations.append(changed)
        for changed in mutations:
            with patch.object(cohort,'verified_document',return_value=deepcopy(verified)):
                with self.assertRaisesRegex(ValueError,'current artifact replay'):
                    cohort.reconcile(changed)

    def test_future_original_agreement_requires_every_actual_frame(self):
        document=self.document()
        row=next(row for row in document['source_bindings'] if row['focused_owner'])
        for frame in document['frames'][row['id']].values():
            frame['status']='passed'
        with patch.object(cohort,'verified_document',return_value=deepcopy(document)):
            result=cohort.reconcile(document)[row['obligation_id']]
        self.assertEqual('covered',result['status'])
        self.assertEqual('passed',result['original_expectation_status'])
        self.assertEqual(1,result['original_semantic_pass_credit'])
        document['frames'][row['id']]['macro-reused/Node']['status']='mismatch'
        with patch.object(cohort,'verified_document',return_value=deepcopy(document)):
            result=cohort.reconcile(document)[row['obligation_id']]
        self.assertEqual('missing',result['status'])
        self.assertEqual(0,result['original_semantic_pass_credit'])

    def test_reconcile_missing_actual_proof_never_uses_passing_status(self):
        document=self.document();document['status']='verified'
        with patch.object(cohort,'verified_document',side_effect=ValueError('missing actual executable')):
            with self.assertRaisesRegex(ValueError,'missing actual executable'):
                cohort.reconcile(document)

    def test_reconcile_rejects_current_producer_drift_and_detached_roots(self):
        document=self.document();document['compiler_build_producer_source_sha256']={}
        with self.assertRaisesRegex(ValueError,'Current canonical producer drift'):
            cohort.reconcile(document)
        document=self.document();document['phase_artifacts_root']=str(Path('/tmp/detached/phases').resolve())
        with self.assertRaisesRegex(ValueError,'fresh producer layout'):
            cohort.reconcile(document)

    def test_executing_transitive_producers_are_bound(self):
        hashes=cohort.snapshot()
        for path in ('tests/reference/compiler-language/configured_source_context.py',
                'tests/reference/compiler-language/sci-analysis.clj','tests/reference/analysis_boundaries.clj'):
            self.assertEqual(cohort.sci.digest((cohort.ROOT/path).read_bytes()),hashes[path])

    def test_phase_artifacts_cannot_omit_actual_executables(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            payloads={'input.tsv':'','observations.tsv':'','warmup.js':'',
                'build/stdout':json.dumps({'reason':'compiler-artifact','target':{'name':cohort.DRIVER.stem},'executable':str(root/'invented-binary')})+'\n',
                'build/stderr':'','build/command.json':'[]','audit/stdout':'','audit/stderr':'','audit/command.json':'[]'}
            for name,text in payloads.items():
                path=root/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(text)
            binding={'producer_source_sha256':cohort.snapshot(),'executables_sha256':{},
                'artifact_sha256':{name:cohort.sci.digest(text) for name,text in payloads.items()}}
            (root/'execution-binding.json').write_text(json.dumps(binding))
            with patch.object(cohort,'reference_proof',return_value={}),patch.object(cohort,'parse_raw',return_value={}):
                with self.assertRaisesRegex(ValueError,'actual audit/Node executables'):
                    cohort.verified_document(root/'references.json',root)


if __name__=='__main__':
    unittest.main()
