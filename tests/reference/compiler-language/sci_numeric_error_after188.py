"""Faithful complete numeric/error originals with independently pinned results.

Original SCI execution errors stay immutable. Only these seventeen source IDs
can gain explicit canonical-difference evidence; original semantic credit is0.
"""
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
import argparse,copy,json,os,subprocess,sys,tempfile
import sci_numeric_error_after188_candidates as candidates
import sci_gap_canonical_context as proof
import sci_finite_after185_context as executable
import sci_execution as sci
import sci_cljc_accepted_context as pin_inputs

PROFILE='exact-original-sci-numeric-errors-after188-v1'
CONTRACTS=Path(__file__).with_name('sci-numeric-error-after188-contracts.json')
CONTRACTS_SHA256='48df74675b3f9098f9f41fb03d2073fbdb0ab169a17512c11618ba67f747db91'


def load():
    proof.same(sci.digest(CONTRACTS.read_bytes()),CONTRACTS_SHA256,'Unreviewed numeric complete literal contracts')
    contracts=json.loads(CONTRACTS.read_text());originals=candidates.load()
    proof.same(tuple(r['sci_id'] for r in contracts),candidates.IDS,'Numeric canonical source cohort changed')
    result=[]
    for contract,row in zip(contracts,originals):
        for key in ('sci_id','body_sha256','original_row_sha256','source_sha256'):
            proof.same(contract[key],row[key],'Numeric original complete source binding drift')
        packet=contract['canonical_expected']
        proof.same(set(packet),{'id','outcome','trace','stdout'},'Incomplete numeric canonical packet')
        proof.same(packet['id'],row['id'],'Detached numeric canonical source identity')
        proof.same(packet['outcome']['status'],'returned','Numeric canonical return contract changed')
        proof.same(packet['trace'],row['expected']['trace'],'Numeric original ordered effects differ')
        proof.same(packet['stdout'],row['expected']['stdout'],'Numeric original stdout differs')
        if not contract['reason'] or not contract['scope_basis']:raise ValueError('Missing numeric canonical scope basis')
        result.append({**row,'original_expected':copy.deepcopy(row['expected']),
          'canonical_expected':copy.deepcopy(packet),'reason':contract['reason'],
          'scope_basis':contract['scope_basis'],'focused_owner':None,
          'source_context_profile':PROFILE,'original_semantic_pass_credit':0,
          'original_expectation_status':'unresolved'})
    return result


def snapshot():
    paths=[Path(__file__),Path(executable.__file__),Path(pin_inputs.__file__)]
    return {**proof.snapshot(),**candidates.snapshot(),
            **{str(p.relative_to(sci.ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def node_script(folder,index):
    return ('process.chdir('+json.dumps(str(folder))+');\n'
      +'require('+json.dumps(str(folder/'reference.js'))+');\n'
      +'const selected = '+json.dumps([index])+';\n'
      +'globalThis.lightCases = selected.map(i => lightCases[i]);\n'
      +'globalThis.lightCaseMetadata = selected.map(i => lightCaseMetadata[i]);\n'
      +(sci.ROOT/'tests/support/execution.js').read_text())


def compiled_pin_units(root):
    return {str(p.relative_to(root)):sci.digest(p.read_bytes()) for p in sorted(root.rglob('*.js')) if p.is_file()}


def reference_replay(root,document):
    rows=load();before=snapshot();programs=pin_inputs.executable_paths()
    with tempfile.TemporaryDirectory(prefix='light-numeric-error-pin-replay-') as temporary:
        replay=Path(temporary)
        with ThreadPoolExecutor(max_workers=2) as pool:
            futures=[pool.submit(sci.reference_cases,candidates.load(),kind,sci.ROOT/'research/clojurescript',replay/kind,600) for kind in ('release','checkout')]
            for future in futures:future.result()
        for kind in ('release','checkout'):
            pin=root/kind
            proof.same((replay/kind/'preflight/analysis.tsv').read_text(),(pin/'preflight/analysis.tsv').read_text(),'Numeric pin analysis differs from independent compiler replay')
            proof.same(compiled_pin_units(replay/kind/'execution'),compiled_pin_units(pin/'execution'),'Numeric pin complete emitted units differ from compiler replay')
            for index,row in enumerate(rows):
                for name in ('src/semantic/case'+str(index)+'.cljs','cases.tsv'):
                    proof.same((replay/kind/'execution'/name).read_text(),(pin/'execution'/name).read_text(),'Numeric pin complete source differs from compiler replay')
    for kind in ('release','checkout'):
        pin=root/kind/'execution'
        for index,row in enumerate(rows):
            folder=pin/('node-'+str(index));script=node_script(pin,index)
            proof.same((folder/'stdin').read_text(),script,'Changed exact numeric reference Node input')
            proof.same(json.loads((folder/'invocation.json').read_text()),[os.environ.get('NODE','node'),'-'],'Changed numeric reference Node command')
            process=subprocess.run([programs['node'],'-'],cwd=sci.ROOT,input=script,capture_output=True,text=True,timeout=60)
            if process.returncode:raise ValueError('Numeric pin-generated Node replay failed')
            proof.same(process.stdout,(folder/'stdout').read_text(),'Numeric pin raw packet differs from executable replay')
            proof.same(process.stderr,(folder/'stderr').read_text(),'Numeric pin original source markers differ from executable replay')
            packet=sci.report_module().observations(process.stdout,[row])[row['id']]['observed']
            proof.same(packet,row['canonical_expected'],'Numeric pin exact typed literal differs')
    proof.same(before,snapshot(),'Numeric producer/pin inputs changed during executable replay')


def verified_document(reference_path,phase_root):
    before=snapshot();phase_root=Path(phase_root).resolve()
    document=proof.verified_document(reference_path,phase_root,cohort=sys.modules[__name__])
    proof.same(document['reference_evidence'].get('reference_execution_binding'),candidates.reference_execution_binding(),'Actual numeric reference executable path/hash drift')
    for row in load():
        frames=document['frames'][row['id']]
        for frame in frames.values():
            proof.same(frame['ordinary_projection'],row['canonical_expected'],'Complete exact numeric result/effect packet differs')
            proof.same(frame['status'],'passed','Numeric whole source phase failed')
    executable.executable_replay(phase_root,document)
    reference_replay(Path(reference_path).parent,document)
    proof.same(before,snapshot(),'Numeric producer changed during complete source replay')
    return document


def reconcile(document):
    proof.same(document.get('profile'),PROFILE,'Wrong numeric whole-source profile')
    proof.same(document.get('compiler_build_producer_source_sha256'),snapshot(),'Current numeric source/compiler/producer drift')
    reference=Path(document['reference_artifacts_root'])/'evidence.json';phase=Path(document['phase_artifacts_root'])
    replay=verified_document(reference,phase)
    proof.same(document,replay,'Numeric supplied receipt differs from actual source/executable replay')
    result={}
    for row in load():
        identity=row['id']
        result[row['obligation_id']]={'id':identity,'source_binding':copy.deepcopy(row),
          'layers':copy.deepcopy(replay['reference_evidence']['evidence'][identity]),
          'actual_phase_frames':copy.deepcopy(replay['frames'][identity]),
          'status':'version-difference','reconciliation':'verified-canonical-sci-difference',
          'original_expectation_status':'unresolved','original_semantic_pass_credit':0,
          'canonical_contract_status':'passed','actual_macro_contract_status':'passed',
          'checked_pin_contract_status':{'release':'passed','checkout':'passed'},'focused_owner':None}
    return result


def selected_integration(document):
    """Exercise the real ledger seam; this selected audit cannot grant full credit."""
    import full_ledger as ledger
    rows=load();identities=[row['obligation_id'] for row in rows]
    layers=copy.deepcopy(document['reference_evidence']['evidence'])
    historical={'counts':dict.fromkeys(('covered','missing','version-difference'),0),
                'strict_complete':False,'boundary_strict_complete':True}
    _,witnesses,_=sci.report_module().load_contract('intended-language')
    def selected_rows(result):
        selected=[row for row in result['assertions'] if row['id'] in identities]
        proof.same(len(selected),17,'Numeric selected ledger cohort missing or duplicated')
        proof.same({row['id'] for row in selected},set(identities),'Numeric selected ledger identities changed')
        return selected
    absent=ledger.expand_report(historical,witnesses,{},identities=identities,sci_evidence=layers)
    absent_rows=selected_rows(absent)
    proof.same({row['id']:row['status'] for row in absent_rows},
               dict.fromkeys(identities,'missing'),'Numeric originals credited without whole-source sidecar')
    present=ledger.expand_report(historical,witnesses,{},identities=identities,
                                sci_evidence=layers,numeric_sci_context_evidence=document)
    present_rows=selected_rows(present)
    proof.same({row['id']:row['status'] for row in present_rows},
               dict.fromkeys(identities,'version-difference'),'Selected numeric ledger cohort incomplete or credited incorrectly')
    summaries=[]
    for row in present_rows:
        context=row['gap_sci_context_reconciliation'];macro=row['columns']['quickjs_macro']
        proof.same(context['original_semantic_pass_credit'],0,'Numeric ledger granted original SCI credit')
        proof.same(context['original_expectation_status'],'unresolved','Numeric ledger changed original SCI expectation')
        proof.same(macro['status'],'passed','Numeric ledger lacks actual macro proof')
        proof.same({frame['variant'] for frame in macro['frames']},
                   {'macro-fresh/QuickJS','macro-fresh/Node','macro-reused/QuickJS','macro-reused/Node'},
                   'Numeric selected ledger macro lifecycle frames missing')
        proof.same(len(context['actual_phase_frames']),8,'Numeric selected ledger lost original application/macro frames')
        proof.same(context['layers'],layers[context['id']],'Numeric selected ledger layers changed')
        summaries.append({'id':row['id'],'status':row['status'],'original_semantic_pass_credit':0,
                          'original_expectation_status':'unresolved','macro':copy.deepcopy(macro),
                          'actual_phase_frames':copy.deepcopy(context['actual_phase_frames'])})
    for result in (absent,present):
        proof.same(result['selection']['full_scope_selected'],False,'Selected numeric integration claimed full scope')
        proof.same(result['strict_complete'],False,'Selected numeric integration claimed full completion')
        proof.same(result['source_strict_complete'],False,'Selected numeric integration claimed full source completion')
    detached=copy.deepcopy(layers);detached[rows[0]['id']]['release']={}
    try:
        ledger.expand_report(historical,witnesses,{},identities=identities,
                             sci_evidence=detached,numeric_sci_context_evidence=document)
    except ValueError as error:
        if 'context detached from fresh report execution' not in str(error):raise
        detached_reason=str(error)
    else:raise ValueError('Numeric selected ledger accepted detached proof layers')
    return {'schema_version':1,'profile':PROFILE,'scope':'selected integration only; no full gate credit',
            'compiler_build_producer_source_sha256':snapshot(),
            'reference_artifacts_root':document['reference_artifacts_root'],
            'phase_artifacts_root':document['phase_artifacts_root'],
            'selection':present['selection'],'counts':{'covered':0,'missing':0,'version-difference':17},'strict_complete':False,
            'full_report_rows_retained':len(present['assertions']),
            'original_semantic_pass_credit':0,'absent_sidecar_statuses':{row['id']:row['status'] for row in absent_rows},
            'detached_proof_layers_rejected':detached_reason,'assertions':summaries}


def run(directory,timeout=600):
    root=Path(directory).resolve();root.mkdir(parents=True,exist_ok=False);before=snapshot()
    candidates.run(root/'originals',timeout)
    proof.execute(root/'phases',min(timeout,300),cohort=sys.modules[__name__])
    document=verified_document(root/'originals/evidence.json',root/'phases')
    proof.same(before,snapshot(),'Numeric source producer changed during fresh complete audit')
    (root/'evidence.json').write_text(json.dumps(document,indent=2)+'\n');reconcile(document)
    print('Verified17 unchanged numeric error originals/136 actualframes/bothpins;zero originalSCIcredit',flush=True);return document

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--timeout',type=int,default=600);args=parser.parse_args();run(args.output,args.timeout)
