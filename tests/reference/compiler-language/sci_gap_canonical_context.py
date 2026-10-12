"""Exact canonical contracts for unchanged SCI sources; originals stay immutable.

Fresh raw source execution is mandatory. The three current public gaps remain
missing; an ordinary canonical observation never substitutes for demanded macro
computation. The 27 canonical differences retain zero original SCI pass credit.
"""
from pathlib import Path
import argparse
import copy
import json
import os
import shutil
import configured_source_context as runner
import sci_gap_return_candidates as candidates
import sci_execution as sci
import sci_source_context as context
import sci_symbol_keyword_context as packets

ROOT = sci.ROOT
CONTRACTS = Path(__file__).with_name('sci-gap-canonical-contracts.jsonl')
DRIVER = ROOT/'tests/compiler_sci_gap_phase_probe.rs'
TEST = 'original_gap_sources_execute_through_actual_macro_hosts'
VARIANTS = ('application-fresh','macro-fresh','application-reused','macro-reused')
PROFILE = 'exact-historical-gap-return-canonical-context-v1'


def same(actual, expected, reason):
    if not sci.report_module().same_json(actual, expected):
        raise ValueError(reason)


def load():
    originals = {row['sci_id']: row for row in candidates.load()}
    contracts = [json.loads(line) for line in CONTRACTS.read_text().splitlines()]
    if len(contracts) != 30 or len({r['sci_id'] for r in contracts}) != 30:
        raise ValueError('Missing/duplicate exact reviewed canonical contracts')
    result = []
    for contract in contracts:
        identity = contract['sci_id']; original = originals[identity]
        for key in ('source_sha256','original_row_sha256','body_sha256'):
            same(contract[key], original[key], 'Original source/row/disposition binding drift')
        canonical = contract['canonical_expected']
        if (canonical['id'] != original['id'] or set(canonical) != {'id','outcome','trace','stdout'}
                or not isinstance(canonical['trace'], list) or not isinstance(canonical['stdout'], str)
                or not contract.get('scope_basis') or not contract.get('reason')):
            raise ValueError('Incomplete explicit canonical result/effect/scope contract')
        result.append({**original,'original_expected': original['expected'],
            'canonical_expected': canonical,'scope_basis':contract['scope_basis'],
            'reason':contract['reason'],'focused_owner':contract.get('focused_owner'),
            'source_context_profile': PROFILE,'original_semantic_pass_credit':0,
            'original_expectation_status':'unresolved'})
    return result


def module(row, phase):
    source = row['source']
    if phase == 'application': return '(fn* [observe] '+source+')'
    if phase != 'macro': raise ValueError('Unknown actual phase')
    return '(defmacro upstream [] (let [events (atom []) observe (fn* [v] (swap! events conj v) v)] (list \'quote [(try [:returned (do '+source+')] (catch :default e [:threw (cond (instance? js/TypeError e) "type-error" (instance? js/RangeError e) "range-error" (instance? js/ReferenceError e) "reference-error" :else "error") (.-message e)])) @events]))) (upstream)'


def ordinary_packet(observed, phase):
    if phase == 'application': return observed
    if observed.get('trace') != [] or observed.get('stdout') != '':
        raise ValueError('Macro/application effects leaked across phases')
    value = observed.get('outcome',{}).get('value')
    pair = value.get('$vector') if isinstance(value,dict) else None
    if observed.get('outcome',{}).get('status') != 'returned' or not isinstance(pair,list) or len(pair)!=2:
        raise ValueError('Invalid demanded macro packet')
    tagged, trace = pair
    if not isinstance(tagged,dict) or set(tagged)!={'$vector'} or not isinstance(trace,dict) or set(trace)!={'$vector'}:
        raise ValueError('Invalid actual macro result/effects brands')
    tag = tagged['$vector']; kind = tag[0].get('$keyword',{}).get('spelling') if tag and isinstance(tag[0],dict) else None
    if kind == 'returned' and len(tag)==2:
        outcome={'status':'returned','value':tag[1]}
    elif kind == 'threw' and len(tag)==3 and tag[1] in {'type-error','range-error','reference-error','error'} and isinstance(tag[2],str):
        outcome={'status':'threw','error':{'category':tag[1]}}
    else: raise ValueError('Unreviewed actual macro result/error tag')
    return {'id': observed['id'],'outcome':outcome,'trace':trace['$vector'],'stdout':''}


def parse_raw(text):
    rows = load(); by_id={row['id']:row for row in rows}
    records=[line.split('\t') for line in text.splitlines()]
    order=[(row['id'],variant,engine) for row in rows for variant in VARIANTS for engine in ('QuickJS','Node')]
    if len(records)!=240 or any(len(r)!=7 for r in records) or [tuple(r[:3]) for r in records]!=order:
        raise ValueError('Missing/duplicate/reordered actual source/phase frames')
    frames={row['id']:{} for row in rows}
    for identity,variant,engine,source_hex,unit_hex,raw_hex,warmup_hex in records:
        source,unit,raw=[bytes.fromhex(v).decode() for v in (source_hex,unit_hex,raw_hex)]
        if source != module(by_id[identity],variant.split('-')[0]) or not unit.startswith('(__lightSessionRuntime0) => (() => {') or not unit.endswith('})()\n'):
            raise ValueError('Reduced or changed original demanded source/compiled unit')
        if (warmup_hex=='-')!=variant.endswith('fresh') or (variant.endswith('reused') and not bytes.fromhex(warmup_hex)):
            raise ValueError('Missing actual reused-host warmup')
        observed=packets.strict_json(raw)
        if observed.get('id')!=identity: raise ValueError('Wrong source observation identity')
        packet=ordinary_packet(observed,variant.split('-')[0])
        frames[identity][variant+'/'+engine]={'status':'passed' if sci.report_module().same_json(sci.projection(packet),sci.projection(by_id[identity]['canonical_expected'])) else 'mismatch',
            'observed':observed,'ordinary_projection':packet,'phase_source_sha256':sci.digest(source),
            'compiled_js_sha256':sci.digest(unit),'raw_observation_sha256':sci.digest(raw),
            'warmup_compiled_js_sha256':None if warmup_hex=='-' else sci.digest(bytes.fromhex(warmup_hex))}
    for left,right in zip(records[::2],records[1::2]):
        if left[3:5]!=right[3:5] or left[6]!=right[6]:raise ValueError('Engines detached from one source compilation')
    return frames


def snapshot():
    paths=[Path(__file__),Path(__file__).with_name('test_sci_gap_canonical_context.py'),CONTRACTS,DRIVER,Path(candidates.__file__),Path(sci.__file__),
        Path(context.__file__),Path(packets.__file__),ROOT/'tests/support/sci_execution_observation.js',
        ROOT/'tests/support/execution.js',ROOT/'tests/reference/execution.clj',ROOT/'tests/reference/compiler-language/report.py',
        Path(runner.__file__),ROOT/'tests/reference/compiler-language/sci-analysis.clj',
        ROOT/'tests/reference/analysis_boundaries.clj']
    return {**context.compiler_sources(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def verify_warmup(warmup):
    # compile_with_macros emits an ordinary IIFE for the literal warmup;
    # the demanded source modules use compile_session's runtime factory.
    if (not warmup.startswith('(() => {\n') or not warmup.endswith('})()\n')
            or '  return 42;\n' not in warmup):
        raise ValueError('Invalid actual compiled warmup')


def reference_proof(path):
    document=json.loads(Path(path).read_text())
    same(document['cases'],candidates.load(),'Original reference source/expectation cohort drift')
    same(document['producer_sha256'],candidates.snapshot(),'Original reference producer/compiler drift')
    root=Path(path).parent
    manifest={str(p.relative_to(root)):sci.digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file() and p!=Path(path)}
    same(document['artifact_sha256'],manifest,'Missing/extra/changed actual reference artifacts')
    originals=candidates.load();by_id={row['id']:row for row in originals}
    for kind in ('release','checkout'):
        pin=root/kind
        preflight=(pin/'preflight/analysis.tsv').read_text().splitlines()
        analysis=[line.split('\t') for line in preflight]
        same([line[0] for line in analysis],[row['id'] for row in originals],'Missing/reordered actual pin analysis')
        same((pin/'preflight/cases.tsv').read_text(),''.join(row['id']+'\texpr\t\t(fn* [observe] '+sci.REFERENCE_SETUP+row['source']+')\t\n' for row in originals),'Changed reference original analysis sources')
        accepted=[by_id[line[0]] for line in analysis if line[1] in {'accepted','warning'}]
        same((pin/'execution/cases.tsv').read_text(),''.join(row['id']+'\t'+sci.REFERENCE_SETUP+row['source']+'\n' for row in accepted),'Changed original accepted reference sources')
        build=(pin/'execution/build/stdout').read_text()
        identity=kind+' '+(sci.report_module().RELEASE if kind=='release' else sci.report_module().REVISION)
        if 'REFERENCE IDENTITY '+identity not in build or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in build:
            raise ValueError('Unchecked actual reference identity/settings build')
        for index,row in enumerate(accepted):
            node=pin/'execution'/('node-'+str(index))
            observed=sci.report_module().observations((node/'stdout').read_text(),[row])[row['id']]
            if not isinstance(observed.get('observed'),dict):raise ValueError('Invalid actual raw canonical output')
            same(observed['observed'],document['evidence'][row['id']][kind].get('observed'),'Serialized canonical observation differs from actual raw output')
            stderr=(node/'stderr').read_text().splitlines()
            if len(stderr)!=2 or not stderr[0].startswith('CASE START ') or not stderr[1].startswith('CASE END '):
                raise ValueError('Unexpected reference source-bearing stderr')
            same(json.loads(stderr[0].removeprefix('CASE START ')),{'id':row['id'],'source':sci.REFERENCE_SETUP+row['source']},'Reference output detached from original source')
            same(json.loads(stderr[1].removeprefix('CASE END ')),{'id':row['id']},'Missing exact reference source end')
    for row in load():
        for kind in ('release','checkout'):
            layer=document['evidence'][row['id']][kind]
            same(layer.get('observed'),row['canonical_expected'],'Literal canonical pin contract drift')
            same(layer.get('reference_identity'),kind+' '+(sci.report_module().RELEASE if kind=='release' else sci.report_module().REVISION),'Unchecked reference identity')
            same(layer.get('settings'),{'optimizations':'none','static-fns':False,'def-emits-var':False},'Unchecked reference settings')
            same(layer.get('stderr'),'','Unexpected original source stderr')
    return document


def verified_document(reference_path,phase_root):
    """Reconstruct a proof from current executable, source and raw artifacts."""
    before=snapshot(); rows=load(); references=reference_proof(reference_path)
    phase_root=Path(phase_root).resolve()
    raw=phase_root/'observations.tsv'
    frames=parse_raw(raw.read_text())
    execution=json.loads((phase_root/'execution-binding.json').read_text())
    same(execution['producer_source_sha256'],snapshot(),'Actual phase producer drift')
    required={'input.tsv','observations.tsv','warmup.js','build/stdout','build/stderr','build/command.json','audit/stdout','audit/stderr','audit/command.json'}
    manifest={str(p.relative_to(phase_root)):sci.digest(p.read_bytes()) for p in sorted(phase_root.rglob('*')) if p.is_file() and p.name!='execution-binding.json'}
    same(set(manifest),required,'Missing/extra actual phase build/audit artifacts')
    same(execution['artifact_sha256'],manifest,'Actual raw phase artifact drift')
    build=[json.loads(line) for line in (phase_root/'build/stdout').read_text().splitlines() if line.startswith('{')]
    binaries=[str(Path(r['executable'])) for r in build if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    if len(binaries)!=1:raise ValueError('Missing exact actual built audit executable')
    node=str(Path(shutil.which(os.environ.get('NODE','node')) or '').resolve())
    same(set(execution['executables_sha256']),{binaries[0],node},'Missing/extra actual audit/Node executables')
    for path,digest in execution['executables_sha256'].items():
        same(sci.digest(Path(path).read_bytes()),digest,'Actual phase executable drift')
    same(json.loads((phase_root/'build/command.json').read_text()),['cargo','test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],'Wrong actual phase build command')
    same(json.loads((phase_root/'audit/command.json').read_text()),[binaries[0],TEST,'--ignored','--exact','--nocapture'],'Wrong actual phase audit command')
    same((phase_root/'input.tsv').read_text(),''.join(row['id']+'\t'+row['source'].encode().hex()+'\n' for row in rows),'Changed original phase input')
    warmup=(phase_root/'warmup.js').read_text()
    verify_warmup(warmup)
    for observations in frames.values():
        for key,frame in observations.items():
            if key.startswith('macro-reused/') or key.startswith('application-reused/'):
                same(frame['warmup_compiled_js_sha256'],sci.digest(warmup),'Wrong actual reused-host warmup')
    log=(phase_root/'audit/stdout').read_text()
    if 'running 1 test' not in log or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in log:
        raise ValueError('Actual exact source audit did not execute')
    rows_by_id={row['id']:row for row in rows}; summaries=[]
    for identity, observations in frames.items():
        row=rows_by_id[identity]
        agreeing=all(frame['status']=='passed' for frame in observations.values())
        original_agreement=agreeing and sci.report_module().same_json(sci.projection(row['original_expected']),sci.projection(row['canonical_expected']))
        summaries.append({'id':identity,'obligation_id':row['obligation_id'],
            'status':('covered' if sci.report_module().same_json(sci.projection(row['original_expected']),sci.projection(row['canonical_expected'])) else 'version-difference') if agreeing else 'missing',
            'original_expectation_status':'passed' if original_agreement else 'unresolved',
            'original_semantic_pass_credit':1 if original_agreement else 0,
            'canonical_contract_status':'passed' if agreeing else 'mismatch',
            'checked_pin_contract_status':{'release':'passed','checkout':'passed'},
            'actual_macro_contract_status':'passed' if agreeing else 'mismatch',
            'focused_owner':row.get('focused_owner')})
    same(before,snapshot(),'Source/producer drift during receipt binding')
    result={'schema_version':1,'profile':PROFILE,'source_bindings':rows,
        'compiler_build_producer_source_sha256':before,'phase_driver_sha256':sci.digest(DRIVER.read_bytes()),
        'phase_execution_binding':execution,
        'reference_evidence_sha256':sci.digest(Path(reference_path).read_bytes()),
        'reference_artifacts_root':str(Path(reference_path).resolve().parent),
        'phase_artifacts_root':str(phase_root),
        'reference_evidence':references,'raw_phase_observations':raw.read_text(),
        'raw_phase_observations_sha256':sci.digest(raw.read_bytes()),'frames':frames,'summaries':summaries,
        'original_semantic_pass_credit':sum(row['original_semantic_pass_credit'] for row in summaries)}
    return result


def _bind(reference_path,phase_root,output):
    result=verified_document(reference_path,phase_root)
    output=Path(output)
    output.write_text(json.dumps(result,indent=2)+'\n')
    summaries=result['summaries']
    print('Bound',len(result['source_bindings']),'whole sources;',sum(r['status']=='version-difference' for r in summaries),'canonical agreements;',sum(r['status']=='missing' for r in summaries),'focused public gaps')
    return result


def reconcile(document):
    """Replay raw current proofs; serialized passing statuses never grant credit."""
    same(document.get('schema_version'),1,'Canonical reconciliation schema drift')
    same(document.get('profile'),PROFILE,'Canonical reconciliation profile drift')
    rows=load()
    same(document.get('source_bindings'),rows,'Original canonical cohort source drift')
    same(document.get('compiler_build_producer_source_sha256'),snapshot(),'Current canonical producer drift')
    reference_value=document.get('reference_artifacts_root')
    phase_value=document.get('phase_artifacts_root')
    if not isinstance(reference_value,str) or not isinstance(phase_value,str):
        raise ValueError('Missing actual canonical artifact roots')
    reference_root=Path(reference_value).resolve(); phase_root=Path(phase_value).resolve()
    if (str(reference_root)!=reference_value or str(phase_root)!=phase_value
            or reference_root.name!='originals' or phase_root.name!='phases'
            or reference_root.parent!=phase_root.parent):
        raise ValueError('Canonical artifacts detached from fresh producer layout')
    replay=verified_document(reference_root/'evidence.json',phase_root)
    same(document,replay,'Canonical supplied proof differs from current artifact replay')
    result={}
    for row in rows:
        identity=row['id']; frames=replay['frames'][identity]
        agreeing=all(frame['status']=='passed' for frame in frames.values())
        difference=not sci.report_module().same_json(sci.projection(row['original_expected']),sci.projection(row['canonical_expected']))
        status=('version-difference' if difference else 'covered') if agreeing else 'missing'
        result[row['obligation_id']]={'id':identity,'source_binding':copy.deepcopy(row),
            'layers':copy.deepcopy(replay['reference_evidence']['evidence'][identity]),
            'actual_phase_frames':copy.deepcopy(frames),'status':status,
            'reconciliation':('verified-canonical-sci-difference' if difference else 'verified-original-source-agreement') if agreeing else 'original-contract-unresolved',
            'original_expectation_status':'passed' if agreeing and not difference else 'unresolved',
            'original_semantic_pass_credit':1 if agreeing and not difference else 0,
            'canonical_contract_status':'passed' if agreeing else 'mismatch',
            'actual_macro_contract_status':'passed' if agreeing else 'mismatch',
            'checked_pin_contract_status':{'release':'passed','checkout':'passed'},
            'focused_owner':row.get('focused_owner')}
    same(list(result),[row['obligation_id'] for row in rows],'Missing/extra canonical source proof')
    return result


def execute(directory, timeout=300):
    directory=Path(directory).resolve(); directory.mkdir(parents=True,exist_ok=False)
    before=snapshot();rows=load()
    (directory/'input.tsv').write_text(''.join(row['id']+'\t'+row['source'].encode().hex()+'\n' for row in rows))
    build,_=runner.run(['cargo','test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],directory/'build',timeout)
    artifacts=[json.loads(line) for line in build.splitlines() if line.startswith('{')]
    binaries=[Path(r['executable']) for r in artifacts if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    if len(binaries)!=1:raise ValueError('Missing exact built source/phase audit executable')
    binary=binaries[0];node=Path(shutil.which(os.environ.get('NODE','node')) or '').resolve()
    if not node.is_file():raise ValueError('Missing Node executable')
    executables={str(path):sci.digest(path.read_bytes()) for path in (binary,node)}
    env={**os.environ,'LIGHT_SCI_GAP_PHASE_ROOT':str(directory),'NODE':str(node)}
    runner.run([str(binary),TEST,'--ignored','--exact','--nocapture'],directory/'audit',timeout,environment=env)
    parse_raw((directory/'observations.tsv').read_text())
    same(before,snapshot(),'Compiler/source/producer changed during actual phase audit')
    for path,digest in executables.items():same(sci.digest(Path(path).read_bytes()),digest,'Built audit binary drift')
    binding={'producer_source_sha256':before,'executables_sha256':executables,
        'artifact_sha256':{str(p.relative_to(directory)):sci.digest(p.read_bytes()) for p in sorted(directory.rglob('*')) if p.is_file()}}
    (directory/'execution-binding.json').write_text(json.dumps(binding,indent=2)+'\n')
    return binding


def run(directory,timeout=600):
    """Only positive producer entry point: fresh local, both pins and phases."""
    directory=Path(directory).resolve();directory.mkdir(parents=True,exist_ok=False)
    before=snapshot()
    candidates.run(directory/'originals',timeout)
    execute(directory/'phases',min(timeout,300))
    result=_bind(directory/'originals/evidence.json',directory/'phases',directory/'evidence.json')
    same(before,snapshot(),'Source/producer changed during fresh complete audit')
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--timeout',type=int,default=600)
    args=parser.parse_args()
    run(args.output,args.timeout)
