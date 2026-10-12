"""Failclosed guards for whole outcome packets, demanded macros and disjoint mappings."""
import copy,json,unittest
from unittest.mock import patch
import arity_after194 as cohort
from arity_resume_candidates import direct_calls

class After194Guards(unittest.TestCase):
    def test_complete_original_contracts(self):
        for row in cohort.rows():
            self.assertEqual(row['source'],row['case']['source'])
            self.assertEqual(row['source_sha256'],cohort.digest(row['source']))
        self.assertEqual(len(cohort.rows()),27)

    def test_whole_binding_effects(self):
        row=next(r for r in cohort.rows()if r['original_id']=='control-binding-once')
        self.assertEqual(row['case']['expected']['trace'],[2,3])
        self.assertIn('(observe 9)',row['source'])
        self.assertIn('(when-some [x (observe 3)] x)',row['source'])

    def test_original_uninitialized_macro_boundary_has_zero_credit(self):
        c=json.loads(cohort.CATALOG.read_text());ids={r['id']for r in cohort.rows()}
        for id,record in c['excluded_original_macro_demand'].items():
            self.assertNotIn(id,ids)
            for field in ['ordinary_credit','phase_credit','source_site_credit']:self.assertEqual(record[field],0)
        self.assertNotIn('arity-after194-extension-direct-2',ids)

    def test_packet_computed_inside_real_demanded_macro(self):
        real=cohort.Path.read_text
        def read(path,*args,**kw):
            value=real(path,*args,**kw)
            return value.replace('js* {macro_observer:?} arity-original','quoted source')if path==cohort.ROOT/cohort.DRIVER else value
        with patch.object(cohort.Path,'read_text',read):
            with self.assertRaises(ValueError):cohort.rows()

    def test_exact_disjoint_new_pairs(self):
        exports=cohort.exports();ids=[r['obligation_id']for r in exports['obligation-witnesses.jsonl']]
        self.assertEqual(len(ids),16);self.assertEqual(len(set(ids)),len(ids))
        self.assertEqual(set(ids),{r['obligation_id']for r in exports['phase-witnesses.jsonl']})
        c=json.loads(cohort.CATALOG.read_text())
        self.assertTrue(set(ids)<=set(c['baseline_missing_ids']))
        self.assertTrue(set(ids).isdisjoint(c['preserved_prior_arity_rows']['obligation-witnesses.jsonl']))
        state=cohort.audit_installed_loaders(exports)
        if state['state']=='installed-exact-paired':self.assertEqual(state['ordinary_loader'],'passed-exact-own-cohort')

    def test_original_arrays_and_transducer_reuse_are_whole(self):
        row=next(r for r in cohort.rows()if r['original_id']=='partitionv-all')
        self.assertIn('(into [] xf [1 2 3])',row['source']);self.assertIn('(into [] xf [4 5])',row['source'])
        self.assertEqual(row['case']['expected']['outcome']['value'],[['returned','[[[1 2] [3]] [[[1 2] [3]] [[4 5]]]]'],'[]'])

    def test_quotes_discard_and_method_vectors_are_not_calls(self):
        for source in ["'(MapEntry. :a 1 nil)",'#_(MapEntry. :a 1 nil)','(reify P (MapEntry. [this a b c] 42))']:
            self.assertEqual(direct_calls(source,'MapEntry.'),[])

    def test_constructor_spelling_and_full_arity(self):
        selected=next(r for r in cohort.SELECTION if r['call_name']=='MapEntry.')
        target={'name':'MapEntry','declaration':'deftype','signature':{'kind':'fixed','minimum':3}}
        self.assertEqual(cohort.operation_evidence('(MapEntry. :a 1 nil)',selected,target)[0]['argument_count'],3)
        with self.assertRaises(ValueError):cohort.operation_evidence('(MapEntry. :a 1)',selected,target)

    def test_wrong_signature_and_spelling_rejected(self):
        selected={'call_name':'conj','direct_arities':[0]}
        with self.assertRaises(ValueError):cohort.operation_evidence('(conj)',selected,{'name':'conj','signature':{'kind':'fixed','minimum':1}})
        with self.assertRaises(ValueError):cohort.operation_evidence('(conj)',selected,{'name':'other'})

    def test_fixture_mutation_rejected(self):
        real=cohort.Path.read_text;path=cohort.ROOT/cohort.REVIEWED[0]['fixture']
        def read(p,*a,**kw):return real(p,*a,**kw)+' 'if p==path else real(p,*a,**kw)
        with patch.object(cohort.Path,'read_text',read):
            with self.assertRaises(ValueError):cohort.rows()

    def frames(self):
        return '\n'.join('arity-after194-observation\t'+'\t'.join([r['id'],host,phase,engine,json.dumps(r['case']['expected'],separators=(',',':'))])for r in cohort.rows()for host in ['fresh','reused']for phase in ['application','macro']for engine in ['QuickJS','Node'])

    def test_complete_ordered_frames(self):self.assertEqual(len(cohort.parse_frames(self.frames())),216)
    def test_missing_frame_rejected(self):
        with self.assertRaises(ValueError):cohort.parse_frames('\n'.join(self.frames().splitlines()[1:]))
    def test_duplicate_frame_rejected(self):
        raw=self.frames()
        with self.assertRaises(ValueError):cohort.parse_frames(raw+'\n'+raw.splitlines()[0])
    def test_changed_complete_result_rejected(self):
        with self.assertRaises(ValueError):cohort.parse_frames(self.frames().replace('"value":true','"value":false',1))
    def test_changed_trace_stdout_rejected(self):
        with self.assertRaises(ValueError):cohort.parse_frames(self.frames().replace('"trace":[2,3]','"trace":[2]',1))
        with self.assertRaises(ValueError):cohort.parse_frames(self.frames().replace('"stdout":""','"stdout":"extra"',1))

    def installed(self,ordinary,phase):
        exported=cohort.exports();real=cohort.Path.read_text;replacement={cohort.HERE/'obligation-witnesses.jsonl':ordinary,cohort.HERE/'phase-witnesses.jsonl':phase}
        def read(path,*a,**kw):
            if path not in replacement:return real(path,*a,**kw)
            prior=[r for r in map(json.loads,real(path).splitlines())if r['obligation_id']not in {r['obligation_id']for r in exported['obligation-witnesses.jsonl']}]
            return ''.join(json.dumps(r)+'\n'for r in [*prior,*replacement[path]])
        with patch.object(cohort.Path,'read_text',read):return cohort.audit_installation(exported)
    def test_exact_pair_installed(self):
        e=cohort.exports();self.assertEqual(self.installed(e['obligation-witnesses.jsonl'],e['phase-witnesses.jsonl'])['state'],'installed-exact-paired')
    def test_partial_or_unpaired_rejected(self):
        e=cohort.exports()
        for ordinary,phase in [(e['obligation-witnesses.jsonl'][:-1],e['phase-witnesses.jsonl']),(e['obligation-witnesses.jsonl'],[])]:
            with self.assertRaises(ValueError):self.installed(ordinary,phase)
    def test_other_cohort_overlap_rejected(self):
        e=cohort.exports();wrong=copy.deepcopy(e['obligation-witnesses.jsonl']);wrong[0]['local_execution']['test_file']='tests/compiler_arity_after191.rs'
        with self.assertRaises(ValueError):self.installed(wrong,e['phase-witnesses.jsonl'])
    def test_phase_support_drift_rejected(self):
        e=cohort.exports();wrong=copy.deepcopy(e['phase-witnesses.jsonl']);wrong[0]['support_source_sha256'][cohort.HELPER]='0'*64
        with self.assertRaises(ValueError):self.installed(e['obligation-witnesses.jsonl'],wrong)
    def test_previous_cohort_row_drift_rejected(self):
        e=cohort.exports();real=cohort.Path.read_text;c=json.loads(cohort.CATALOG.read_text());id=next(iter(c['preserved_prior_arity_rows']['obligation-witnesses.jsonl']))
        def read(path,*a,**kw):
            value=real(path,*a,**kw)
            if path==cohort.HERE/'obligation-witnesses.jsonl':
                rows=list(map(json.loads,value.splitlines()));next(r for r in rows if r['obligation_id']==id)['rationale']='drift';return ''.join(json.dumps(r)+'\n'for r in rows)
            return value
        with patch.object(cohort.Path,'read_text',read):
            with self.assertRaises(ValueError):cohort.audit_installation(e)

if __name__=='__main__':unittest.main()
