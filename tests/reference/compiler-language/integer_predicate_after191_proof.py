#!/usr/bin/env python3
"""Failclosed, zero-credit original Closure integer predicate discrepancy proof."""
from pathlib import Path
import argparse,json,tempfile,shutil
import integer_predicate_after191 as c
import hosted_import_source_context as hosted
import report
ROOT=c.ROOT
EXPECTED_CONTROLS=[True]*6+[0,100000000000,-100000000000,0,100000000000,-100000000000]
EXPECTED_FAILURES={(row,4) for row in range(8,14)}|{(row,5) for row in [10,13]}|{(row,6) for row in [9,12]}|{(row,7) for row in [8,9,11,12]}
PREDICATES=['identity','neg?','pos?','integer?','int?','neg-int?','pos-int?','nat-int?']
RECEIVERS=['0','negative-zero','1','-1','1.0','-1.0','large-positive-Number','large-negative-Number','Long-zero','Long-positive','Long-negative','Integer-zero','Integer-positive','Integer-negative']


def pin(kind,path,catalog):
    path=Path(path).resolve();receipt=json.loads((path/'evidence.json').read_text());identity=kind+' '+(report.RELEASE if kind=='release' else report.REVISION)
    if receipt['reference_identity']!=identity or receipt['settings']!={'optimizations':'none','static-fns':False,'def-emits-var':False} or receipt['observed']!={'test':1,'pass':112,'fail':0,'error':0}:raise ValueError('whole original pin identity/default settings/112 assertions drift')
    if not receipt['complete_source_unchanged'] or receipt['source_sha256']!=catalog['module_sha256'] or receipt['original_forms']!=catalog['forms'] or (path/'source/cljs/predicates_test.cljs').read_text()!=(ROOT/c.MODULE).read_text():raise ValueError('whole original pin source/table/body drift')
    artifacts={str(p.relative_to(path)):c.digest(p.read_bytes()) for p in sorted(path.rglob('*')) if p.is_file() and p.name!='evidence.json'}
    if artifacts!=receipt['artifact_sha256']:raise ValueError('raw canonical pin artifact census/hash drift')
    dependency='{:mvn/version "'+report.RELEASE+'"}' if kind=='release' else '{:local/root '+json.dumps(str(ROOT/'research/clojurescript'))+'}'
    build=['clojure','-Srepro','-Sdeps','{:paths [] :deps {org.clojure/clojurescript '+dependency+'}}','-M',str(path/'build.clj'),kind,identity.split(' ',1)[1],'-' if kind=='release' else str(ROOT/'research/clojurescript'),str(path),str(path/'source')]
    node=['node','-e','process.chdir('+json.dumps(str(path))+');require('+json.dumps(str(path/'reference.js'))+');']
    if json.loads((path/'build/invocation.json').read_text())!=build or json.loads((path/'node/invocation.json').read_text())!=node:raise ValueError('exact canonical pin command drift')
    raw_build=(path/'build/stdout').read_text();raw_node=(path/'node/stdout').read_text()
    if 'REFERENCE IDENTITY '+identity not in raw_build or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in raw_build:raise ValueError('actual canonical identity/settings absent')
    packets=[json.loads(line.split('ORIGINAL INTEGER COUNTERS ',1)[1]) for line in raw_node.splitlines() if line.startswith('ORIGINAL INTEGER COUNTERS ')]
    if packets!=[{'test':1,'pass':112,'fail':0,'error':0}]:raise ValueError('raw complete original112 reference assertion counters differ')
    shared={name:c.digest((path/'out/goog'/name).read_bytes()) for name in hosted.DEPENDENCIES if (path/'out/goog'/name).is_file()}
    if any(shared[name]!=expected for name,expected in hosted.DEPENDENCIES.items() if name in shared) or not {'base.js','deps.js','math/long.js','math/integer.js'}<=set(shared):raise ValueError('fresh canonical Closure origin/class bytes differ')
    for relative,digest in receipt['producer_source_sha256'].items():
        if c.digest((ROOT/relative).read_bytes())!=digest:raise ValueError('original reference producer source drift')
    return {'identity':identity,'receipt_sha256':c.digest((path/'evidence.json').read_bytes()),'unchanged_complete_assertion_counters':packets[0],'genuine_dependency_shared_with_audited_embedding_sha256':shared,'artifact_sha256':artifacts}


def validate(actual,log,references):
    actual=Path(actual).resolve();raw=Path(log).read_text();catalog=json.loads((actual/'source-catalog.json').read_text());original=c.original()
    if any(catalog.get(key)!=value for key,value in original.items()):raise ValueError('complete source-unit/original forms drift')
    before=json.loads((actual/'snapshot-before.json').read_text());after={relative:c.digest((ROOT/relative).read_bytes()) for relative in before}
    if before!=after:raise ValueError('compiler/original/producer snapshot drift')
    loader,dependencies=hosted.dependency_bundle(c.Genuine);origin=hosted.library_origin(c.Genuine)
    if catalog['compiled_dependency_sha256']!=dependencies or catalog['library_origin']!=origin or catalog['closure_loader_sha256']!=c.digest(loader) or (actual/'genuine-loader.js').read_text()!=loader:raise ValueError('genuine compiled loader/archive provenance drift')
    if any(catalog[key]!=0 for key in ['original_site_credit','ordinary_credit','phase_credit']) or catalog['strict_complete'] is not False:raise ValueError('diagnostic granted semantic/site credit')
    if raw.count('test result: ok. 1 passed; 0 failed; 0 ignored;')!=1 or 'test '+c.TEST+' ... ok' not in raw or 'integer-after191-rejection\t' in raw:raise ValueError('complete exact named diagnostic test not observed')
    metadata=[json.loads(line) for line in raw.splitlines() if line.startswith('{')]
    executables=[row['executable'] for row in metadata if row.get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='integer_predicate_after191' and row.get('executable')]
    if len(executables)!=1 or not Path(executables[0]).is_file():raise ValueError('actual Cargo diagnostic executable absent')
    lines=[line.split('\t',4) for line in raw.splitlines() if line.startswith('integer-after191-observation\t')]
    expected_keys=[(host,phase,engine) for host in ['fresh','reused'] for phase in ['application','macro'] for engine in ['QuickJS','Node']]
    if any(len(line)!=5 for line in lines) or [tuple(line[1:4]) for line in lines]!=expected_keys:raise ValueError('missing/reordered/extra actual integer engine frame')
    observations=[];files={'source-catalog.json','snapshot-before.json','genuine-loader.js','application.cljs','macro.cljs','warmup.cljs','warmup.js'}
    if (actual/'warmup.cljs').read_text()!='(defmacro integer-ready [] (+ 20 22)) (integer-ready)' or not (actual/'warmup.js').read_text():raise ValueError('actual reused host warmup missing')
    packets={}
    for host,phase in [(h,p) for h in ['fresh','reused'] for p in ['application','macro']]:
        source=c.program(original,loader,phase);folder=actual/host/phase;relative=host+'/'+phase+'/'
        if (actual/(phase+'.cljs')).read_text()!=source or (folder/'integer_predicate_after191.cljs').read_text()!=source:raise ValueError('unchanged original full table/body or adapter source replaced')
        for unit in original['forms'].values():
            if source.count(unit['source'])!=1:raise ValueError('complete original form shortened/duplicated')
        js=(folder/'compiled.js').read_text();anchor=' = // Original bounded runtime'
        if js.count(anchor)!=1:raise ValueError('compiled observer runtime anchor drift')
        captured=js.replace(anchor,' = globalThis.lightRuntime = // Original bounded runtime',1)
        script='(() => {const run='+captured+';return ('+(ROOT/'tests/support/sci_execution_observation.js').read_text()+")(globalThis.lightRuntime)('integer-predicate-after191',observe=>run(observe));})()"
        if (folder/'script.js').read_text()!=script or (folder/'quickjs.stdin').read_text()!='(0, eval)('+json.dumps(script,ensure_ascii=False)+')' or (folder/'node.stdin').read_text()!='console.log('+script+');':raise ValueError('full original actual engine input/observer changed')
        for engine,relative_raw in [('QuickJS','quickjs.stdout'),('Node','node.stdout')]:
            line=next(line for line in lines if tuple(line[1:4])==(host,phase,engine));raw_packet=(folder/relative_raw).read_text()
            if raw_packet!=line[4]+('\n' if engine=='Node' else ''):raise ValueError('actual complete raw engine frame changed')
            packet=json.loads(line[4]);packets[(host,phase,engine)]=packet
            if packet['id']!='integer-predicate-after191' or packet['outcome']['status']!='returned' or packet['trace']!=[] or packet['stdout']!='':raise ValueError('exception, output, or effects substituted for full integer body')
            vector=packet['outcome']['value']['$vector']
            if len(vector)!=2 or vector[0]!={'$vector':EXPECTED_CONTROLS}:raise ValueError('genuine initialized six receiver classes/values not observed')
            checks=vector[1]['$vector']
            if len(checks)!=112:raise ValueError('incomplete original14x8 assertion execution')
            failures=set()
            for index,item in enumerate(checks):
                record=item['$vector']
                if len(record)!=3 or record[0]!=index%8 or type(record[1]) is not bool or not isinstance(record[2],str) or not record[2]:raise ValueError('original assertion ordering/result/message lost')
                if not record[1]:failures.add((index//8,index%8))
            if failures!=EXPECTED_FAILURES:raise ValueError('independent exact initialized-Closure predicate discrepancy matrix changed')
            observations.append({'host':host,'phase':phase,'engine':engine,'original_comparisons':112,'pass':98,'fail':14,'genuine_receiver_controls':EXPECTED_CONTROLS,'complete_packet_sha256':c.digest(line[4]),'failures':[{'row':row,'receiver':RECEIVERS[row],'predicate':PREDICATES[index]} for row,index in sorted(failures)]})
        if packets[(host,phase,'QuickJS')]!=packets[(host,phase,'Node')]:raise ValueError('complete paired-engine original assertion records differ')
        files.update(relative+name for name in ['integer_predicate_after191.cljs','compiled.js','script.js','quickjs.stdin','quickjs.stdout','node.stdin','node.stdout'])
    census={str(p.relative_to(actual)):c.digest(p.read_bytes()) for p in sorted(actual.rglob('*')) if p.is_file()}
    if set(census)!=files:raise ValueError('actual complete diagnostic artifact census differs')
    pins={kind:pin(kind,Path(references)/kind,catalog) for kind in ['release','checkout']}
    return {'status':'initialized-genuine-Closure-predicate-discrepancy-confirmed','source_catalog':catalog,'actual_observations':observations,'actual_log_sha256':c.digest(raw),'actual_executable':executables[0],'actual_executable_sha256':c.digest(Path(executables[0]).read_bytes()),'actual_artifact_sha256':census,'frozen_before_after_source_equal':True,'references':pins,'existing_documented_boundary':'docs/compiler.md separates external Closure construction from JS Number predicates; full original test and public API ledger dispositions remain intended','ordinary_credit':0,'phase_credit':0,'original_site_credit':0,'strict_complete':False}


def controls(actual,log,references):
    pristine=validate(actual,log,references);results=[]
    with tempfile.TemporaryDirectory(prefix='integer-predicate-after191-controls-') as temp:
        root=Path(temp);copied=root/'actual';shutil.copytree(actual,copied);copied_log=root/'actual.log';shutil.copyfile(log,copied_log)
        def change(name,path,mutate):
            old=path.read_bytes()
            try:
                path.write_bytes(mutate(old))
                try:validate(copied,copied_log,references)
                except (ValueError,KeyError,IndexError,FileNotFoundError,json.JSONDecodeError) as e:results.append({'id':name,'status':'rejected','reason':str(e)})
                else:raise AssertionError('diagnostic mutation accepted: '+name)
            finally:path.write_bytes(old)
            validate(copied,copied_log,references)
        change('missing-actual-engine-frame',copied_log,lambda data:b'\n'.join(line for line in data.split(b'\n') if not line.startswith(b'integer-after191-observation\tfresh\tapplication\tQuickJS')))
        change('false-original-assertion-pass-substitution',copied_log,lambda data:data.replace(b'[4,false,',b'[4,true,',1))
        change('genuine-receiver-control-lost',copied_log,lambda data:data.replace(b'[true,true,true,true,true,true,0',b'[false,true,true,true,true,true,0',1))
        for filename in ['integer_predicate_after191.cljs','compiled.js','script.js','quickjs.stdin','quickjs.stdout','node.stdin','node.stdout']:
            change('changed-'+filename,copied/'fresh/application'/filename,lambda data:data+b' ')
        change('changed-genuine-library-loader',copied/'genuine-loader.js',lambda data:data+b' ')
        for kind in ['release','checkout']:
            folder=Path(references)/kind
            change('changed-'+kind+'-112-counters',folder/'node/stdout',lambda data:data.replace(b'"pass":112',b'"pass":111',1))
            change('changed-'+kind+'-original-table',folder/'source/cljs/predicates_test.cljs',lambda data:data.replace(b'[natl',b'[nil ',1))
    if validate(actual,log,references)!=pristine:raise ValueError('pristine raw restoration replay differs')
    return {'controls':results,'restored_pristine_replay':'passed','original_site_credit':0,'ordinary_credit':0,'phase_credit':0,'strict_complete':False}

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['actual','log','references','output']:p.add_argument('--'+name,required=True,type=Path)
    p.add_argument('--controls',action='store_true');a=p.parse_args();receipt=validate(a.actual,a.log,a.references)
    receipt.update(proof_source_sha256=c.digest(Path(__file__).read_bytes()),actual_root=str(a.actual.resolve()),actual_log_path=str(a.log.resolve()),references_root=str(a.references.resolve()))
    a.output.write_text(json.dumps(receipt,indent=2)+'\n')
    if a.controls:a.output.with_name(a.output.stem+'-raw-controls.json').write_text(json.dumps(controls(a.actual,a.log,a.references),indent=2)+'\n')
    print(receipt['status'],flush=True)
