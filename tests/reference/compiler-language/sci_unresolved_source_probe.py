"""Candidate current seven-source audit. Never grants shared-ledger credit."""
from pathlib import Path
import argparse, collections, json, os, shutil
import sci_source_context as context
import sci_execution as sci
import configured_source_context as runner
from extract_inventory import Reader
HERE=Path(__file__).resolve().parent
DRIVER=sci.ROOT/'tests/compiler_sci_unresolved_source_probe.rs'
TEST='unchanged_seven_sources_execute_actual_phases_and_lifecycles'
IDS=('sci-macro-var-meta','sci-macro-emit-defs','sci-macro-multi-attrs','sci-syntax-quote-dotted','sci-syntax-quote-qualified','sci-syntax-quote-metadata-vector','sci-binding-defn-arglist-conditions')
VARIANTS=('application-fresh','macro-fresh','application-reused','macro-reused')
def rows():
    catalog={r['sci_id']:r for r in context.load()}
    receipts=[json.loads(l) for l in (HERE/'sci-source-context-receipts.jsonl').read_text().splitlines()]
    if tuple(r['source_binding']['sci_id'] for r in receipts if r['reconciliation']=='original-contract-unresolved') != IDS: raise ValueError('Seven-source unresolved cohort changed')
    return [catalog[i] for i in IDS]
def snapshot():
    paths=[Path(__file__),DRIVER,sci.CORPUS,sci.DISPOSITIONS,Path(context.__file__),Path(sci.__file__),Path(runner.__file__),HERE/'report.py',HERE/'sci_source_context.clj',sci.ROOT/'tests/support/sci_execution_observation.js',sci.ROOT/'tests/support/execution.js',sci.ROOT/'tests/reference/execution.clj']
    return {**context.compiler_sources(),**{str(p.relative_to(sci.ROOT)):sci.digest(p.read_bytes()) for p in paths}}
def module(row,phase):
    original=row['source']; last=Reader(original).all()[-1]
    if phase=='application': return original
    return original[:last.start]+"(defmacro upstream [] (list 'quote "+original[last.start:last.end]+")) (upstream)"

def local(directory,timeout):
    directory.mkdir(parents=True); rs=rows(); request=directory/'input.tsv'; output=directory/'observations.tsv'
    request.write_text(''.join(r['id']+'\t'+r['source'].encode().hex()+'\n' for r in rs))
    build,_=runner.run(['cargo','test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],directory/'build',timeout)
    binaries=[Path(r['executable']) for line in build.splitlines() if line.startswith('{') for r in [json.loads(line)] if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    if len(binaries)!=1: raise ValueError('Exact audit executable missing')
    binary=binaries[0]; binary_hash=sci.digest(binary.read_bytes()); node=Path(shutil.which(os.environ.get('NODE','node'))).resolve(); node_hash=sci.digest(node.read_bytes())
    env={**os.environ,'LIGHT_SCI_UNRESOLVED_INPUT':str(request),'LIGHT_SCI_UNRESOLVED_ROOT':str(directory/'modules'),'LIGHT_SCI_UNRESOLVED_OUTPUT':str(output)}
    stdout,_=runner.run([str(binary),TEST,'--ignored','--exact','--nocapture'],directory/'audit',timeout,environment=env)
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in stdout or 'running 1 test' not in stdout: raise ValueError('Audit not executed')
    raw=[l.split('\t') for l in output.read_text().splitlines()]
    order=[(r['id'],v,e) for r in rs for v in VARIANTS for e in ('QuickJS','Node')]
    if len(raw)!=56 or any(len(f)!=8 for f in raw) or [tuple(f[:3]) for f in raw]!=order: raise ValueError('Missing/reordered source phases/engines')
    frames={r['id']:{} for r in rs}; byid={r['id']:r for r in rs}
    for identity,variant,engine,status,source_hex,js_hex,observation_hex,warmup_hex in raw:
        source=bytes.fromhex(source_hex).decode(); observation=bytes.fromhex(observation_hex).decode()
        if source!=module(byid[identity],variant.split('-')[0]): raise ValueError('Whole original source/module changed')
        if (warmup_hex=='-')!=variant.endswith('fresh'): raise ValueError('Host lifecycle provenance missing')
        target=directory/identity/variant/engine; target.mkdir(parents=True)
        (target/'source.cljs').write_text(source); (target/'raw.txt').write_text(observation)
        frame={'status':status,'phase_source_sha256':sci.digest(source),'raw_sha256':sci.digest(observation),'warmup_js_sha256':None if warmup_hex=='-' else sci.digest(bytes.fromhex(warmup_hex))}
        if status=='executed':
            js=bytes.fromhex(js_hex).decode(); (target/'compiled.js').write_text(js); frame['compiled_js_sha256']=sci.digest(js)
            try:
                observed=json.loads(observation); frame.update(sci.verify(byid[identity],{'status':'completed','observed':observed},sci.report_module().same_json))
            except ValueError: frame.update(status='invalid-output',detail=observation)
        else: frame['detail']=observation
        frames[identity][variant+'/'+engine]=frame
    if sci.digest(binary.read_bytes())!=binary_hash or sci.digest(node.read_bytes())!=node_hash: raise ValueError('Execution binary drift')
    return {'frames':frames,'audit_executable_sha256':binary_hash,'node_executable_sha256':node_hash,'build_stdout_sha256':sci.digest(build),'raw_observations_sha256':sci.digest(output.read_bytes())}
def run(directory,timeout):
    directory=directory.resolve(); directory.mkdir(parents=True,exist_ok=False); before=snapshot(); rs=rows()
    document={'schema_version':1,'production_head':runner.run(['git','rev-parse','HEAD'],directory/'head',timeout)[0].strip(),'source_bindings':rs,'original_semantic_pass_credit':0,'original_expectation_status':'unresolved','artifact_root':str(directory),'references':{}}
    document['local']=local(directory/'local',timeout)
    print('local',dict(collections.Counter(f['status'] for fs in document['local']['frames'].values() for f in fs.values())),flush=True)
    for kind in ('release','checkout'):
        document['references'][kind]=context.reference_cases(rs,kind,directory/kind,timeout)
        print(kind,dict(collections.Counter(l['status'] for l in document['references'][kind].values())),flush=True)
    runner.unchanged(before,snapshot()); document['compiler_build_producer_source_sha256']=before
    document['artifact_sha256']={str(p.relative_to(directory)):sci.digest(p.read_bytes()) for p in sorted(directory.rglob('*')) if p.is_file()}
    document['status']='candidate-executed-unresolved'
    (directory/'evidence.json').write_text(json.dumps(document,indent=2)+'\n')
    print('evidence_sha256',sci.digest((directory/'evidence.json').read_bytes()),flush=True)
    return document
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__); p.add_argument('--output',type=Path,required=True); p.add_argument('--timeout',type=int,default=300); a=p.parse_args();run(a.output,a.timeout)
