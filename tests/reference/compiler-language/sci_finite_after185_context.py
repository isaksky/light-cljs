"""Exact whole finite-source canonical contracts with current executable replay.

Seven positive contracts retain zero original SCI pass credit. Seven complete
ratio-bearing originals remain rejected by both pins and receive no credit.
"""
from pathlib import Path
import argparse,copy,json,os,subprocess,sys,tempfile
import sci_finite_after185_candidates as candidates
import sci_gap_canonical_context as proof
import sci_execution as sci

PROFILE='exact-finite-original-sci-after185-v1'
CONTRACTS=Path(__file__).with_name('sci-finite-after185-contracts.jsonl')
CONTRACTS_SHA256='ba8b61d5d545bd755f7b2b8d69ba7668d49daea92c9a2a11422ab362fd4e1d5f'
POSITIVE_IDS=('sci-meta-list-tail','sci-control-case-map','sci-control-case-set',
    'sci-iteration-doseq-zero-generators','sci-iteration-doseq-prefix-let',
    'sci-iteration-dotimes-fraction','sci-iteration-dotimes-char')


def load():
    proof.same(sci.digest(CONTRACTS.read_bytes()),CONTRACTS_SHA256,'Unreviewed finite literal contracts')
    frozen=[json.loads(line) for line in CONTRACTS.read_text().splitlines()]
    proof.same(tuple(r['sci_id'] for r in frozen),POSITIVE_IDS,'Finite positive cohort changed')
    originals={r['sci_id']:r for r in candidates.load()};result=[]
    for contract in frozen:
        row=originals[contract['sci_id']]
        for key in ('source_sha256','original_row_sha256','body_sha256'):
            proof.same(contract[key],row[key],'Finite original body binding drift')
        packet=contract['canonical_expected']
        if (set(packet)!={'id','outcome','trace','stdout'} or packet['id']!=row['id']
                or not isinstance(packet['trace'],list) or not isinstance(packet['stdout'],str)
                or not contract['reason'] or not contract['scope_basis']):
            raise ValueError('Incomplete finite literal result/effect contract')
        if sci.report_module().same_json(packet,row['expected']):raise ValueError('Finite difference no longer differs from original')
        result.append({**row,'original_expected':copy.deepcopy(row['expected']),
            'canonical_expected':copy.deepcopy(packet),'scope_basis':contract['scope_basis'],
            'reason':contract['reason'],'focused_owner':None,'source_context_profile':PROFILE,
            'original_semantic_pass_credit':0,'original_expectation_status':'unresolved'})
    return result


def snapshot():
    return {**proof.snapshot(),**candidates.snapshot()}


def executable_replay(phase_root,document):
    """Run the current hash-bound compiler audit, rather than trusting raw tags."""
    binding=document['phase_execution_binding'];binary=json.loads((phase_root/'audit/command.json').read_text())[0]
    built=subprocess.run(['cargo','test','--locked','--test',proof.DRIVER.stem,'--no-run','--message-format=json'],cwd=sci.ROOT,capture_output=True,text=True,timeout=300)
    if built.returncode:raise ValueError('Current Cargo finite audit build replay failed')
    artifacts=[json.loads(line) for line in built.stdout.splitlines() if line.startswith('{')]
    actual=[a['executable'] for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')==proof.DRIVER.stem and a.get('executable')]
    proof.same(actual,[binary],'Bound finite/lazy audit is not the current Cargo executable')
    proof.same(sci.digest(Path(binary).read_bytes()),binding['executables_sha256'][binary],'Current Cargo finite executable differs')
    node=[p for p in binding['executables_sha256'] if p!=binary]
    proof.same(len(node),1,'Missing exact finite Node executable')
    with tempfile.TemporaryDirectory(prefix='light-finite-after185-replay-') as temporary:
        replay=Path(temporary);(replay/'input.tsv').write_text((phase_root/'input.tsv').read_text())
        process=subprocess.run([binary,proof.TEST,'--ignored','--exact','--nocapture'],cwd=sci.ROOT,
            env={**os.environ,'LIGHT_SCI_GAP_PHASE_ROOT':str(replay),'NODE':node[0]},
            capture_output=True,text=True,timeout=300)
        if process.returncode or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in process.stdout:
            raise ValueError('Current finite compiler/QuickJS/Node executable replay failed')
        for name in ('observations.tsv','warmup.js'):
            proof.same((replay/name).read_text(),(phase_root/name).read_text(),'Finite emitted unit/raw observation differs from executable replay')


def reference_replay(root,document):
    """Execute each retained pin-generated unit against the frozen literal."""
    positive={r['id']:r for r in load()}
    node=next(p for p in document['phase_execution_binding']['executables_sha256'] if p!=json.loads((Path(document['phase_artifacts_root'])/'audit/command.json').read_text())[0])
    for kind in ('release','checkout'):
        pin=root/kind;accepted=[line.split('\t')[0] for line in (pin/'preflight/analysis.tsv').read_text().splitlines() if line.split('\t')[1] in ('accepted','warning')]
        proof.same(set(accepted),set(positive),'Changed finite pin accepted-source set')
        for index,identity in enumerate(accepted):
            execution=pin/'execution';folder=execution/('node-'+str(index));script=(folder/'stdin').read_text()
            expected_script=('process.chdir('+json.dumps(str(execution))+');\n'
                +'require('+json.dumps(str(execution/'reference.js'))+');\n'
                +'const selected = '+json.dumps([index])+';\n'
                +'globalThis.lightCases = selected.map(i => lightCases[i]);\n'
                +'globalThis.lightCaseMetadata = selected.map(i => lightCaseMetadata[i]);\n'
                +(sci.ROOT/'tests/support/execution.js').read_text())
            proof.same(script,expected_script,'Changed finite reference execution script')
            proof.same(json.loads((folder/'invocation.json').read_text()),[os.environ.get('NODE','node'),'-'],'Wrong finite reference Node invocation')
            process=subprocess.run([node,'-'],input=script,cwd=sci.ROOT,capture_output=True,text=True,timeout=60)
            if process.returncode:raise ValueError('Finite pin-generated executable replay failed')
            proof.same(process.stdout,(folder/'stdout').read_text(),'Finite pin raw output differs from executable replay')
            proof.same(process.stderr,(folder/'stderr').read_text(),'Finite pin raw stderr differs from executable replay')
            raw=sci.report_module().observations(process.stdout,[positive[identity]])[identity]['observed']
            proof.same(raw,positive[identity]['canonical_expected'],'Finite literal differs from independently executable pin unit')


def verified_document(reference_path,phase_root):
    before=snapshot();phase_root=Path(phase_root).resolve()
    document=proof.verified_document(reference_path,phase_root,cohort=sys.modules[__name__])
    for row in load():
        for frame in document['frames'][row['id']].values():
            proof.same(frame['ordinary_projection'],row['canonical_expected'],'Exact finite typed packet/effect contract mismatch')
            proof.same(frame['status'],'passed','Finite complete source phase failed')
    executable_replay(phase_root,document)
    reference_replay(Path(reference_path).parent,document)
    proof.same(before,snapshot(),'Finite current producer changed during executable replay')
    return document


def reconcile(document):
    proof.same(document.get('profile'),PROFILE,'Wrong finite original proof profile')
    proof.same(document.get('compiler_build_producer_source_sha256'),snapshot(),'Current finite producer drift')
    reference=Path(document['reference_artifacts_root'])/'evidence.json';phase=Path(document['phase_artifacts_root'])
    replay=verified_document(reference,phase)
    proof.same(document,replay,'Finite supplied proof differs from entire executable replay')
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


def guard_existing_lazy(document):
    """Supplement the current lazy proof; return no new ledger rows or credit."""
    import sci_lazy_context as lazy
    proof.reconcile(document,cohort=lazy)
    actual={r['sci_id']:r for r in document['source_bindings']}
    for row in load():
        existing=actual[row['sci_id']]
        for key in ('source','source_sha256','original_row_sha256','body_sha256','original_expected','canonical_expected'):
            proof.same(existing[key],row[key],'Existing lazy finite source/literal drift')
        for frame in document['frames'][row['id']].values():
            proof.same(frame['ordinary_projection'],row['canonical_expected'],'Existing lazy finite typed packet/effects differ')
    executable_replay(Path(document['phase_artifacts_root']),document)
    return {'scope':'Supplementary exact finite typed packets and whole existing lazy compiler/engine replay',
            'checked_original_ids':list(POSITIVE_IDS),'new_ledger_rows':0,'original_semantic_pass_credit':0}


def run(directory,timeout=600):
    root=Path(directory).resolve();root.mkdir(parents=True,exist_ok=False);before=snapshot()
    candidates.run(root/'originals',timeout)
    proof.execute(root/'phases',min(timeout,300),cohort=sys.modules[__name__])
    document=verified_document(root/'originals/evidence.json',root/'phases')
    proof.same(before,snapshot(),'Finite producer changed during fresh audit')
    (root/'evidence.json').write_text(json.dumps(document,indent=2)+'\n')
    reconcile(document)
    print('Verified7 complete finite SCI differences,56 actual source/phase/engine frames;7 ratio-bearing originals retain both-pin rejection and zero credit',flush=True)
    return document

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--timeout',type=int,default=600);args=parser.parse_args();run(args.output,args.timeout)
