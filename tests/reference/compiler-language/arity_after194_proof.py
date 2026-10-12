#!/usr/bin/env python3
"""Failclosed complete raw input/output proof for independent after194 arities."""
from pathlib import Path
import argparse,json,shutil,subprocess
import arity_after194 as cohort
import report
ROOT=cohort.ROOT
PRODUCERS=[cohort.HELPER,cohort.SCANNER,cohort.DRIVER,str(cohort.CATALOG.relative_to(ROOT)),
 'tests/reference/compiler-language/arity_after194_reference.py',
 'tests/reference/compiler-language/arity_after194_proof.py',
 'tests/reference/compiler-language/arity_after194_raw_guard.py',
 'tests/reference/compiler-language/test_arity_after194.py',
 *sorted({row['original_fixture'] for row in cohort.REVIEWED}),
 'tests/reference/execution.clj','tests/support/execution.js','tests/support/sci_execution_observation.js',
 'tests/reference/compiler-language/report.py','tests/reference/compiler-language/extract_inventory.py',
 'tests/reference/compiler-language/obligations.jsonl','tests/reference/compiler-language/obligation-witnesses.jsonl']


def frozen_snapshot():
    paths=set(subprocess.check_output(['git','ls-files','src','build.rs','Cargo.toml','Cargo.lock'],cwd=ROOT,text=True).splitlines())
    paths.update(PRODUCERS)
    catalog=json.loads(cohort.CATALOG.read_text())
    paths.update(catalog.get('preserved_discovery_history',{}))
    paths.update(catalog.get('preserved_prior_arity_source_sha256',{}))
    paths.update(row['fixture'] for row in cohort.REVIEWED)
    paths.update('tests/reference/compiler-language/'+filename for filename in ['full_ledger.py','full-manifest.json','full-inventory.jsonl','full-bodies.jsonl','full-cases.jsonl','phase-witnesses.jsonl'])
    return {'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'source_sha256':{path:cohort.digest((ROOT/path).read_bytes()) for path in sorted(paths)}}


def actual_program(source,phase,identity):
    observer=(ROOT/'tests/support/sci_execution_observation.js').read_text()
    macro_observer='(('+observer+')(rt))('+json.dumps(identity)+', ~{})'
    return '(ns user)\n(defn arity-original [observe] (do '+source+'))\n(defmacro arity-witness [] (js* '+json.dumps(macro_observer,ensure_ascii=False)+' arity-original))\n(defn run [] (arity-witness))\nrun\n' if phase=='macro' else '(ns user)\n(defn run [observe] (do '+source+'))\nrun\n'


def capture(source):
    anchor=' = // Original bounded runtime'
    if anchor in source:
        if source.count(anchor)!=1:raise ValueError('ambiguous executed runtime binding')
        return source.replace(anchor,' = globalThis.lightRuntime = // Original bounded runtime',1)
    return source


def verify_reference(kind,path,rows):
    path=Path(path).resolve();pin=json.loads((path/'receipt.json').read_text());identity=kind+' '+(report.RELEASE if kind=='release' else report.REVISION)
    if pin['reference_identity']!=identity:raise ValueError('required canonical reference identity changed')
    expected={row['id']:row['case']['expected'] for row in rows}
    if set(pin['observations'])!=set(expected):raise ValueError('incomplete original reference observation census')
    for id,contract in expected.items():
        observed=pin['observations'][id]
        if observed['status']!='passed' or observed['observed']!=contract or observed['reference_identity']!=identity or observed['settings']!={'optimizations':'none','static-fns':False,'def-emits-var':False}:
            raise ValueError('canonical full typed result/effect contract changed')
    if pin['original_case_sha256']!={row['id']:row['case_sha256'] for row in rows}:raise ValueError('whole original reference case drift')
    files={'build/invocation.json':'build_invocation_sha256','node/invocation.json':'node_invocation_sha256','node/stdin':'node_stdin_sha256','build.clj':'build_script_sha256','build/stdout':'build_stdout_sha256','reference.js':'reference_js_sha256','node/stdout':'raw_stdout_sha256'}
    for relative,field in files.items():
        if cohort.digest((path/relative).read_bytes())!=pin[field]:raise ValueError('saved raw reference artifact drift: '+relative)
    build=json.loads((path/'build/invocation.json').read_text());node=json.loads((path/'node/invocation.json').read_text())
    if build!=pin['build_command'] or build[:3]!=['clojure','-Srepro','-Sdeps'] or build[-6:]!=[str(path/'build.clj'),kind,identity.split(' ',1)[1],'-' if kind=='release' else str(ROOT/'research/clojurescript'),str(path),str(path/'source')]:raise ValueError('saved canonical reference command drift')
    dependency='{:mvn/version "'+report.RELEASE+'"}' if kind=='release' else '{:local/root '+json.dumps(str(ROOT/'research/clojurescript'))+'}'
    if build[3]!='{:paths ['+json.dumps(str(path/'source'))+'] :deps {org.clojure/clojurescript '+dependency+'}}' or build[4]!='-M':raise ValueError('canonical reference dependency drift')
    if node!=pin['node_command'] or node!=['node','-']:raise ValueError('saved required Node command drift')
    stdin='process.chdir('+json.dumps(str(path))+');require('+json.dumps(str(path/'reference.js'))+');\n'+(ROOT/'tests/support/execution.js').read_text()
    if (path/'node/stdin').read_text()!=stdin:raise ValueError('saved whole canonical Node input changed')
    stdout=(path/'build/stdout').read_text()
    if stdout.count('REFERENCE IDENTITY '+identity)!=2 or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in stdout:raise ValueError('actual default settings/identity absent')
    if [json.loads(line) for line in (path/'node/stdout').read_text().splitlines()]!=list(expected.values()):raise ValueError('raw canonical outcome/effects mismatch')
    source_hashes={str(p.relative_to(path/'source')):cohort.digest(p.read_bytes()) for p in sorted((path/'source').rglob('*')) if p.is_file()}
    if source_hashes!=pin['source_file_sha256']:raise ValueError('saved canonical source census changed')
    expected_sources={}
    for index,row in enumerate(rows):
        namespace='arityreference.case'+str(index)
        expected_sources[namespace.replace('.','/')+'.cljs']='(ns '+namespace+' (:require [cljs.core :as c]))\n(def run (fn* [observe] '+row['source']+'))\n'
    if set(source_hashes)!={*expected_sources,'arityreference/host.cljs'}:raise ValueError('canonical original source file census omitted/extra')
    for relative,source in expected_sources.items():
        if (path/'source'/relative).read_text()!=source:raise ValueError('canonical complete original source wrapper changed')
    namespaces=['arityreference.case'+str(index) for index in range(len(rows))]
    host='(ns arityreference.host (:require '+' '.join('['+namespace+']' for namespace in namespaces)+'))\n'
    calls=['(fn* [observe] ('+namespace+'/run observe))' for namespace in namespaces]
    host+='(set! (.-lightCases js/globalThis) (array '+' '.join(calls)+'))\n'
    host+='(set! (.-lightCaseMetadata js/globalThis) (array '+' '.join('(array '+json.dumps(row['case']['id'])+' '+json.dumps(row['case']['source'])+')' for row in rows)+'))\n'
    harness=(ROOT/'tests/reference/execution.clj').read_text()
    from extract_inventory import Reader
    definitions=[node for node in Reader(harness).all() if node.kind=='list' and len(node.children)>2 and node.children[0].token=='def' and node.children[1].token=='inspect-value-source']
    if len(definitions)!=1 or definitions[0].children[2].children[0].token!='str':raise ValueError('independent pinned observer declaration changed')
    strings=definitions[0].children[2].children[1:]
    if any(node.kind!='string' for node in strings):raise ValueError('independent pinned observer source is not literal strings')
    inspector=''.join(json.loads(harness[node.start:node.end]) for node in strings)
    if (path/'source/arityreference/host.cljs').read_text()!=host+inspector:raise ValueError('canonical demanded original callbacks/observer host changed')
    for relative,digest in pin['producer_source_sha256'].items():
        if cohort.digest((ROOT/relative).read_bytes())!=digest:raise ValueError('canonical producer source drift: '+relative)
    return {'receipt_path':str(path/'receipt.json'),'receipt_sha256':cohort.digest((path/'receipt.json').read_bytes()),**pin}


def validate(actual,references,modules,guards=None,before=None):
    rows=cohort.rows();exports=cohort.exports();raw=Path(actual).read_text();frames=cohort.parse_frames(raw)
    if raw.count('test result: ok. 1 passed; 0 failed; 0 ignored;')!=1 or 'test '+cohort.TEST+' ... ok' not in raw:raise ValueError('exact named Rust test did not pass exactly once')
    input_frames=[line.split('\t') for line in raw.splitlines() if line.startswith('arity-after194-artifacts\t')]
    expected_keys=[(row['id'],host,phase) for row in rows for host in ['fresh','reused'] for phase in ['application','macro']]
    if any(len(f)!=7 for f in input_frames) or [tuple(f[1:4]) for f in input_frames]!=expected_keys:raise ValueError('missing/extra actual Node source/emitted/observer input hash frames')
    metadata=[json.loads(line) for line in raw.splitlines() if line.startswith('{')]
    binaries=[row['executable'] for row in metadata if row.get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='compiler_arity_after194' and row.get('executable')]
    if len(binaries)!=1 or not Path(binaries[0]).is_file():raise ValueError('Cargo-built exact actual executable missing')
    if guards:
        guard_raw=Path(guards).read_text()
        if 'Ran 20 tests' not in guard_raw or not guard_raw.rstrip().endswith('OK'):raise ValueError('all twenty failclosed source/frame guards must pass')
    snapshot=frozen_snapshot()
    if before and json.loads(Path(before).read_text())!=snapshot:raise ValueError('frozen pre/post production/catalog/producer snapshot drift')
    pins={kind:verify_reference(kind,Path(references)/kind,rows) for kind in ['release','checkout']}
    modules=Path(modules).resolve();expected_files={'observer-runtime.js','warmup.cljs','warmup.js'}
    if (modules/'warmup.cljs').read_text()!='(defmacro arity-ready [] (+ 20 22)) (arity-ready)':raise ValueError('actual demanded reused host warmup source changed')
    runtime=(modules/'observer-runtime.js').read_text()
    if not runtime or ' = // Original bounded runtime' not in runtime or not (modules/'warmup.js').read_text():raise ValueError('actual warmup/observer runtime omitted')
    artifact_packets={}
    for row in rows:
        for host in ['fresh','reused']:
            for phase in ['application','macro']:
                base='/'.join([row['id'],host,phase])+'/'
                expected_files.update(base+name for name in ['user.cljs','compiled.js','script.js','quickjs.stdin','quickjs.raw','node.stdin','node.stdout'])
                folder=modules/base;program=actual_program(row['source'],phase,row['id'])
                frame=next(f for f in input_frames if tuple(f[1:4])==(row['id'],host,phase))
                if frame[4:]!=[cohort.digest((folder/name).read_bytes()) for name in ['compiled.js','user.cljs','script.js']]:raise ValueError('saved artifacts differ from actual executed Node input hashes')
                if (folder/'user.cljs').read_text()!=program:raise ValueError('actual whole source or phase invocation changed')
                js=(folder/'compiled.js').read_text()
                script="(() => {const run="+capture(js)+";return run();})()" if phase=='macro' else "(() => {void "+capture(runtime)+";const run="+capture(js)+";return ("+(ROOT/'tests/support/sci_execution_observation.js').read_text()+")(globalThis.lightRuntime)('"+row['id']+"',observe=>run(observe));})()"
                js_string=lambda value:json.dumps(value,ensure_ascii=False)
                node_input="const sha=x=>require('node:crypto').createHash('sha256').update(x).digest('hex');console.log(sha("+js_string(js)+")+'\\t'+sha("+js_string(program)+")+'\\t'+sha("+js_string(script)+"));console.log("+script+");"
                if (folder/'script.js').read_text()!=script or (folder/'node.stdin').read_text()!=node_input or (folder/'quickjs.stdin').read_text()!='(0, eval)('+js_string(script)+')':raise ValueError('actual engine original input/observer or public indirect eval boundary changed')
                for engine,relative in [('QuickJS','quickjs.raw'),('Node','node.stdout')]:
                    saved=(folder/relative).read_text()
                    matched=[f for f in frames if (f['id'],f['host'],f['phase'],f['engine'])==(row['id'],host,phase,engine)]
                    expected_raw=("\t".join(frame[4:])+"\n" if engine=='Node' else '')+matched[0]['raw']+('\n' if engine=='Node' else '') if len(matched)==1 else None
                    if saved!=expected_raw:raise ValueError('saved full raw actual engine output/frame changed')
                    artifact_packets[(row['id'],host,phase,engine).__repr__()]=cohort.digest(saved)
    files={str(p.relative_to(modules)):cohort.digest(p.read_bytes()) for p in sorted(modules.rglob('*')) if p.is_file()}
    if set(files)!=expected_files:raise ValueError('missing/extra actual source/module/raw artifact')
    return {'phase_transport':json.loads(cohort.CATALOG.read_text())['phase_transport'],'head':snapshot['head'],'frozen_source_snapshot':snapshot,'exports':exports,'installed_loader_state':cohort.audit_installed_loaders(exports),'actual_frames':frames,'actual_input_hash_frames':input_frames,'actual_log_sha256':cohort.digest(raw),'actual_executable':binaries[0],'actual_executable_sha256':cohort.digest(Path(binaries[0]).read_bytes()),'module_source_emitted_js_and_raw_sha256':files,'actual_engine_packet_sha256':artifact_packets,'references':pins,'strict_complete':False,'source_site_credit_added':0}


def produce(output,actual,guards,references,modules,before=None):
    receipt=validate(actual,references,modules,guards,before);output=Path(output).resolve();output.mkdir(parents=True,exist_ok=True)
    for name,records in receipt['exports'].items():(output/name).write_text(''.join(json.dumps(row,separators=(',',':'))+'\n' for row in records))
    shutil.copyfile(actual,output/'actual.log');shutil.copyfile(guards,output/'guards.log')
    receipt.update(actual_log_path=str(Path(actual).resolve()),guards_log_path=str(Path(guards).resolve()),modules_path=str(Path(modules).resolve()),references_path=str(Path(references).resolve()),guard_tests_passed=20,guard_log_sha256=cohort.digest(Path(guards).read_bytes()))
    if before:
        (output/'snapshot-after.json').write_text(json.dumps(receipt['frozen_source_snapshot'],indent=2)+'\n')
        receipt.update(frozen_before_snapshot_path=str(Path(before).resolve()),frozen_before_snapshot_sha256=cohort.digest(Path(before).read_bytes()),frozen_before_after_equal=True)
    (output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(output/'receipt.json');return receipt


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['output','actual','guards','references','modules']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--before',type=Path);a=p.parse_args();produce(a.output,a.actual,a.guards,a.references,a.modules,a.before)
