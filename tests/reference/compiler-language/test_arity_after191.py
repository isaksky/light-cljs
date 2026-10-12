"""Independent structural and complete raw-frame rejection controls."""
import copy,json,unittest
from unittest.mock import patch
import arity_after191 as cohort
from arity_resume_candidates import direct_calls

class After191Guards(unittest.TestCase):
    def test_original_complete_row_contracts(self):
        for row in cohort.rows():
            self.assertEqual(row['source'],row['case']['source'])
            self.assertEqual(row['source_sha256'],cohort.digest(row['source']))
            self.assertEqual(row['case']['expected']['trace'],[])
            self.assertEqual(row['case']['expected']['stdout'],'')

    def test_unchanged_protocol_receiver_and_effect_sources(self):
        sources={row['original_id']:row['source'] for row in cohort.rows()}
        for name in ['state-watch-protocol-returns','extension-instance-object-ifn','core-protocol-writer-flush']:
            self.assertEqual(sources[name],next(row['case']['source'] for row in cohort.REVIEWED if row['original_id']==name))
        selected=next(row for row in cohort.SELECTION if row['call_name']=='specify!')
        self.assertEqual(selected['workflow'],'arity-after191-extension-instance-object-ifn')
        self.assertEqual(selected['direct_arities'],[7])

    def test_native_array_macro_transport_has_zero_credit(self):
        catalog=json.loads(cohort.CATALOG.read_text())
        self.assertNotIn('extension-direct-2',{row['original_id'] for row in cohort.rows()})
        excluded=catalog['excluded_js_array_macro_transport']
        for key in ['ordinary_credit','phase_credit','source_site_credit']:self.assertEqual(excluded[key],0)

    def test_public_indirect_eval_input_is_demanded(self):
        real=cohort.Path.read_text
        def read(path,*args,**kwargs):
            value=real(path,*args,**kwargs)
            return value.replace('(0, eval)({script:?})','direct eval') if path==cohort.ROOT/cohort.DRIVER else value
        with patch.object(cohort.Path,'read_text',read):
            with self.assertRaises(ValueError):cohort.rows()

    def test_new_and_missing_cohort(self):
        exports=cohort.exports();ids=[r['obligation_id'] for r in exports['obligation-witnesses.jsonl']]
        self.assertEqual(len(ids),len(set(ids)))
        self.assertEqual(set(ids),{r['obligation_id'] for r in cohort.SELECTION})
        self.assertEqual(set(ids),{r['obligation_id'] for r in exports['phase-witnesses.jsonl']})
        baseline=set(json.loads(cohort.CATALOG.read_text())['baseline_missing_ids'])
        self.assertTrue(set(ids)<=baseline)
        audit=cohort.audit_installed_loaders(exports)
        if audit['state']=='installed-exact-paired':
            self.assertEqual(audit['ordinary_loader'],'passed-exact-own-cohort')
            self.assertEqual(audit['phase_loader'],'passed-exact-own-cohort')

    def test_full_lazy_effect_contract(self):
        row=next(r for r in cohort.rows() if r['original_id']=='sequences-bounded-count')
        for original in ['(bounded-count 2 s)','(bounded-count 2 [0 1 2 3 4])','(= @a [0 1 2])',"'(0 1 2 3 4)"]:
            self.assertIn(original,row['source'])
        selected=next(s for s in cohort.SELECTION if s['call_name']=='bounded-count')
        target={'name':'bounded-count','signature':{'kind':'fixed','minimum':2}}
        self.assertEqual(len(cohort.operation_evidence(row['source'],selected,target)),2)

    def test_quoted_calls_are_not_execution(self):
        self.assertEqual(direct_calls("'(bit-count 3)",'bit-count'),[])
        self.assertEqual(direct_calls('#_(bit-count 3)','bit-count'),[])

    def test_missing_original_operation_rejected(self):
        selected={'call_name':'bit-count','direct_arities':[1]}
        with self.assertRaises(ValueError):cohort.operation_evidence('[3]',selected,{'name':'bit-count','signature':{'kind':'fixed','minimum':1}})

    def test_wrong_exact_signature_rejected(self):
        with self.assertRaises(ValueError):cohort.operation_evidence('(bit-count 3)',{'call_name':'bit-count','direct_arities':[1]},{'name':'bit-count','signature':{'kind':'fixed','minimum':2}})

    def test_fixture_mutation_rejected(self):
        real=cohort.Path.read_text
        path=cohort.ROOT/cohort.REVIEWED[0]['fixture']
        def read(p,*args,**kw):return real(p,*args,**kw)+' ' if p==path else real(p,*args,**kw)
        with patch.object(cohort.Path,'read_text',read):
            with self.assertRaises(ValueError):cohort.rows()

    def frames(self):
        return '\n'.join('arity-after191-observation\t'+ '\t'.join([row['id'],host,phase,engine,json.dumps(row['case']['expected'],separators=(',',':'))]) for row in cohort.rows() for host in ['fresh','reused'] for phase in ['application','macro'] for engine in ['QuickJS','Node'])

    def test_complete_ordered_frames(self):
        frames=cohort.parse_frames(self.frames())
        self.assertEqual(len(frames),8*len(cohort.REVIEWED))
        self.assertEqual({r['phase'] for r in frames},{'application','macro'})

    def test_missing_actual_frame_rejected(self):
        with self.assertRaises(ValueError):cohort.parse_frames('\n'.join(self.frames().splitlines()[1:]))

    def test_duplicate_actual_frame_rejected(self):
        frames=self.frames()
        with self.assertRaises(ValueError):cohort.parse_frames(frames+'\n'+frames.splitlines()[0])

    def test_changed_complete_typed_result_rejected(self):
        frames=self.frames().replace('4294967297','4294967298',1)
        with self.assertRaises(ValueError):cohort.parse_frames(frames)

    def test_changed_original_effect_packet_rejected(self):
        frames=self.frames().replace('"stdout":""','"stdout":"unexpected"',1)
        with self.assertRaises(ValueError):cohort.parse_frames(frames)

    def installed(self,ordinary,phases):
        exported=cohort.exports()
        real=cohort.Path.read_text
        replacement={cohort.HERE/'obligation-witnesses.jsonl':ordinary,cohort.HERE/'phase-witnesses.jsonl':phases}
        def read(path,*args,**kwargs):
            return ''.join(json.dumps(row)+'\n' for row in replacement[path]) if path in replacement else real(path,*args,**kwargs)
        with patch.object(cohort.Path,'read_text',read):return cohort.audit_installation(exported)

    def test_exact_own_installed_pair_accepted(self):
        e=cohort.exports()
        result=self.installed(e['obligation-witnesses.jsonl'],e['phase-witnesses.jsonl'])
        self.assertEqual(result['state'],'installed-exact-paired')
        self.assertEqual(result['ordinary_rows'],len(cohort.SELECTION))
        self.assertEqual(result['phase_rows'],len(cohort.SELECTION))

    def test_partial_own_installation_rejected(self):
        e=cohort.exports()
        with self.assertRaises(ValueError):self.installed(e['obligation-witnesses.jsonl'][:-1],e['phase-witnesses.jsonl'])

    def test_unpaired_own_installation_rejected(self):
        e=cohort.exports()
        with self.assertRaises(ValueError):self.installed(e['obligation-witnesses.jsonl'],[])

    def test_other_cohort_overlap_rejected(self):
        e=cohort.exports();wrong=copy.deepcopy(e['obligation-witnesses.jsonl']);wrong[0]['local_execution']['test_file']='tests/compiler_arity_after190.rs'
        with self.assertRaises(ValueError):self.installed(wrong,e['phase-witnesses.jsonl'])

    def test_installed_phase_support_drift_rejected(self):
        e=cohort.exports();wrong=copy.deepcopy(e['phase-witnesses.jsonl']);wrong[0]['support_source_sha256'][cohort.HELPER]='0'*64
        with self.assertRaises(ValueError):self.installed(e['obligation-witnesses.jsonl'],wrong)

if __name__=='__main__':unittest.main()
