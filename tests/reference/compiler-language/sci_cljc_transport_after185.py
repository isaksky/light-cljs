"""Faithful original .cljc intermediate values, including undefined and regex.

Only three existing missing original IDs may be reconciled by this supplemental
proof. Original sources/SCI expectations remain unchanged and get zero credit.
"""
from pathlib import Path
from unittest.mock import patch
import argparse,copy,json,os,subprocess,tempfile
from concurrent.futures import ThreadPoolExecutor
import sci_cljc_accepted_context as proof
import sci_execution as sci
import sci_symbol_keyword_context as packets

ROOT=sci.ROOT
PROFILE='exact-original-cljc-intermediate-transport-after185-v1'
IDS=('sci-context-reader-skipped-arguments','sci-context-reader-skipped-regex-bigint','sci-context-reader-skipped-regex-identity')
ENCODER=Path(__file__).with_name('sci-cljc-transport-after185-encoder.cljs')
ORACLE=Path(__file__).with_suffix('.clj')
DRIVER=ROOT/'tests/compiler_sci_cljc_transport_after185.rs'
TEST='original_cljc_transport_values_execute_in_both_phases_and_host_lifecycles'
CONTRACTS=Path(__file__).with_name('sci-cljc-transport-after185-contracts.json')
CONTRACTS_SHA256='49454cfdbfac2e25fa6f33a714acd0ad5e58423c854a9b1721021d297da1b927'


def sources():
    rows={r['sci_id']:r for r in proof.load()};result=[]
    for identity in IDS:
        row=rows[identity]
        proof.same(row['canonical_expected'],None,'Transport source already has a main-cohort literal')
        result.append(row)
    return result


def load():
    if CONTRACTS_SHA256 is None:raise ValueError('Unreviewed transport canonical literals')
    proof.same(sci.digest(CONTRACTS.read_bytes()),CONTRACTS_SHA256,'Unreviewed complete transport literals')
    frozen=json.loads(CONTRACTS.read_text());rows=sources()
    proof.same(frozen['source_bindings'],rows,'Original transport source/expectation binding drift')
    proof.same(len(frozen['canonical_expected']),3,'Missing complete literal transport packet')
    result=[]
    for row,packet in zip(rows,frozen['canonical_expected']):
        proof.same(set(packet),{'id','outcome','trace','stdout'},'Incomplete canonical transport packet')
        proof.same(packet['id'],row['id'],'Wrong canonical transport identity')
        proof.same(packet['trace'],[],'Original source has no observe effects')
        proof.same(packet['stdout'],'','Original source has no output effects')
        result.append({**row,'canonical_expected':copy.deepcopy(packet),'source_context_profile':PROFILE,
            'transport_profile':'Complete typed intermediate observation; undefined brand, regex source/flags, Number, vector and set count/members. Original evaluates before encoding.',
            'reason':'The immutable SCI :clj observation differs from independently executed complete original :cljs reader source.',
            'scope_basis':'Adopted :cljs reader feature and finite macro intermediate computation; serialization occurs only after the entire original source executes.',
            'original_semantic_pass_credit':0})
    return result


def body(row):
    return '(let [original-value (do '+row['source']+')] '+ENCODER.read_text().replace('ORIGINAL_ID',json.dumps(row['id']))+')'


def module(row,phase):
    value=body(row)
    if phase=='application':return '(ns user)\n(defn run [] '+value+')\nrun\n'
    if phase=='macro':return '(ns user)\n(defmacro upstream [] '+value+')\n(defn run [] (upstream))\nrun\n'
    raise ValueError('Unknown actual transport phase')


def input_text(rows):return ''.join(r['id']+'\t'+r['source'].encode().hex()+'\t'+body(r).encode().hex()+'\n' for r in rows)

def snapshot():
    paths=[Path(__file__),ORACLE,DRIVER,ENCODER,CONTRACTS,Path(__file__).with_name('test_sci_cljc_transport_after185.py')]
    return {**proof.snapshot(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def decode(row,raw):
    packet=packets.strict_json(raw)
    proof.same(set(packet),{'id','outcome','trace','stdout'},'Invalid complete original transport packet')
    proof.same(packet['id'],row['id'],'Detached original transport identity')
    proof.same(packet['trace'],[],'Changed original transport effects')
    proof.same(packet['stdout'],'','Changed original transport output')
    return packet


def references(root,rows,programs):
    result={}
    with patch.object(proof,'ORACLE',ORACLE):
        for kind in ('release','checkout'):
            layers=proof.verify_reference(kind,root/kind,sources(),programs)
            result[kind]={}
            for row in rows:
                layer=layers[row['id']];observed=layer.get('observed',{})
                proof.same(observed.get('outcome',{}).get('status'),'returned','Pin transport wrapper failed')
                raw=observed['outcome'].get('value')
                if not isinstance(raw,str):raise ValueError('Missing complete pin intermediate transport')
                packet=decode(row,raw);proof.same(packet,row['canonical_expected'],'Independently pinned literal transport contract differs')
                result[kind][row['id']]={**layer,'status':'passed','carrier_observed':observed,'observed':packet,'original_status':'mismatch'}
    return result


def verify_local(root,rows,binding):
    proof.same((root/'input.tsv').read_text(),input_text(rows),'Changed full original transport phase input')
    proof.check_command(root/'build',[binding['programs']['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'])
    proof.check_command(root/'audit',[binding['audit_executable'],TEST,'--ignored','--exact','--nocapture'],environment=binding['audit_environment'])
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in (root/'audit/stdout').read_text():raise ValueError('Actual transport audit did not run')
    proof.same((root/'audit/stderr').read_text(),'','Unexpected transport audit stderr')
    artifacts=[json.loads(l) for l in (root/'build/stdout').read_text().splitlines() if l.startswith('{')]
    proof.same([a['executable'] for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')==DRIVER.stem and a.get('executable')],[binding['audit_executable']],'Detached actual transport audit executable')
    warmup=(root/'warmup.js').read_text()
    if not warmup.startswith('(() => {\n') or not warmup.endswith('})()\n') or '  return 42;\n' not in warmup:raise ValueError('Missing actual transport macro warmup')
    records=[l.split('\t') for l in (root/'observations.tsv').read_text().splitlines()]
    required={'input.tsv','warmup.js','observations.tsv'}|{p+'/'+n for p in ('build','audit') for n in ('stdout','stderr','command.json')}
    order=[(r['id'],v,e) for r in rows for v in proof.VARIANTS for e in ('QuickJS','Node')]
    proof.same([tuple(f[:3]) for f in records],order,'Missing/reordered complete transport frames')
    result={r['id']:{} for r in rows};cursor=0
    for row in rows:
        for variant in proof.VARIANTS:
            folder=root/row['id']/variant;prefix=row['id']+'/'+variant+'/'
            source=module(row,variant.split('-')[0]);proof.same((folder/'user.cljc').read_text(),source,'Changed original intermediate source phase')
            unit=(folder/'compiled.js').read_text();script='(() => {const run='+unit+';return run();})()'
            proof.same((folder/'script.js').read_text(),script,'Changed complete original transport script')
            proof.same((folder/'node.stdin').read_text(),'console.log('+script+');','Changed actual transport Node input')
            proof.same([(folder/'node.exit').read_text(),(folder/'node.stderr').read_text()],['0',''],'Actual Node transport failure')
            for engine in ('QuickJS','Node'):
                fields=records[cursor];cursor+=1;proof.same(len(fields),7,'Malformed original transport frame')
                raw=(folder/'quickjs.raw').read_text() if engine=='QuickJS' else (folder/'node.stdout').read_text().strip()
                proof.same(fields,[row['id'],variant,engine,source.encode().hex(),unit.encode().hex(),raw.encode().hex(),'-' if variant.endswith('fresh') else warmup.encode().hex()],'Engine detached from full original transport computation')
                observed=decode(row,raw);proof.same(observed,row['canonical_expected'],'Complete original typed transport literal differs')
                result[row['id']][variant+'/'+engine]={'status':'passed','observed':observed,'phase_source_sha256':sci.digest(source),'compiled_js_sha256':sci.digest(unit),'raw_observation_sha256':sci.digest(raw),'warmup_compiled_js_sha256':None if variant.endswith('fresh') else sci.digest(warmup)}
            required|={prefix+n for n in ('user.cljc','compiled.js','script.js','quickjs.raw','node.stdin','node.stdout','node.stderr','node.exit')}
    proof.same(set(proof.tree(root)),required,'Missing/extra complete original transport raw artifacts')
    return result


def executable_replay(root,rows,binding):
    built=subprocess.run([binding['programs']['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],cwd=ROOT,capture_output=True,text=True,timeout=300)
    if built.returncode:raise ValueError('Current Cargo transport audit build replay failed')
    artifacts=[json.loads(line) for line in built.stdout.splitlines() if line.startswith('{')]
    actual=[a['executable'] for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')==DRIVER.stem and a.get('executable')]
    proof.same(actual,[binding['audit_executable']],'Bound transport audit is not the current Cargo executable')
    proof.same(sci.digest(Path(actual[0]).read_bytes()),binding['executables_sha256'][actual[0]],'Current Cargo transport executable differs')
    with tempfile.TemporaryDirectory(prefix='light-cljc-transport-replay-') as temporary:
        phase=Path(temporary)/'phases';phase.mkdir();(phase/'input.tsv').write_text(input_text(rows))
        process=subprocess.run([binding['audit_executable'],TEST,'--ignored','--exact','--nocapture'],cwd=ROOT,env={**os.environ,'LIGHT_SCI_CLJC_TRANSPORT_ROOT':str(phase),'NODE':binding['programs']['node']},capture_output=True,text=True,timeout=300)
        if process.returncode or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in process.stdout:raise ValueError('Current complete original transport compiler/engine replay failed')
        proof.same((phase/'observations.tsv').read_text(),(root/'phases/observations.tsv').read_text(),'Emitted original transport/raw packets differ from executable replay')
        proof.same((phase/'warmup.js').read_text(),(root/'phases/warmup.js').read_text(),'Transport reused host replay drift')
    with tempfile.TemporaryDirectory(prefix='light-cljc-transport-pin-replay-') as temporary:
        pin_root=Path(temporary)
        with patch.object(proof,'ORACLE',ORACLE),ThreadPoolExecutor(max_workers=2) as executor:
            futures=[executor.submit(proof.produce_reference,kind,pin_root/kind,sources(),binding['programs'],300) for kind in ('release','checkout')]
            for future in futures:future.result()
        for kind in ('release','checkout'):
            proof.same((pin_root/kind/'contexts.tsv').read_text(),(root/kind/'contexts.tsv').read_text(),'Pin complete source compile/rejection replay differs')
            for row in rows:
                for name in ('sci-original.cljc','compiled.js'):
                    proof.same((pin_root/kind/'original-sources'/row['id']/name).read_text(),(root/kind/'original-sources'/row['id']/name).read_text(),'Pin original source/emitted unit differs from compiler replay')
    for kind in ('release','checkout'):
        for index,row in enumerate(rows):
            folder=root/kind/'node'/row['id'];script=proof.node_script(root/kind,index)
            process=subprocess.run([binding['programs']['node'],'-'],cwd=ROOT,input=script,capture_output=True,text=True,timeout=60)
            if process.returncode:raise ValueError('Current independent pin-generated transport replay failed')
            proof.same(process.stdout,(folder/'stdout').read_text(),'Pin original transport packet differs from executable replay')
            proof.same(process.stderr,(folder/'stderr').read_text(),'Pin original transport source markers differ from executable replay')


def verified_document(root):
    root=Path(root).resolve();before=snapshot();rows=load();binding=json.loads((root/'binding.json').read_text())
    proof.same(binding['producer_source_sha256'],before,'Current transport source/compiler/producer drift')
    proof.same(binding['programs'],proof.executable_paths(),'Transport executable path drift')
    proof.same(set(binding['executables_sha256']),set(binding['programs'].values())|{binding['audit_executable']},'Missing actual transport executables')
    for path,expected in binding['executables_sha256'].items():proof.same(sci.digest(Path(path).read_bytes()),expected,'Current transport executable hash drift')
    proof.same(binding['audit_environment'],{'LIGHT_SCI_CLJC_TRANSPORT_ROOT':str(root/'phases'),'NODE':binding['programs']['node']},'Transport source environment drift')
    proof.same(binding['artifact_sha256'],proof.tree(root,('binding.json','evidence.json')),'Raw original transport artifact census/hash drift')
    refs=references(root,rows,binding['programs']);frames=verify_local(root/'phases',rows,binding)
    executable_replay(root,rows,binding);proof.same(before,snapshot(),'Transport producer changed during replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'source_bindings':rows,'compiler_build_producer_source_sha256':before,'execution_binding':binding,'references':refs,'frames':frames,'status':'verified','original_semantic_pass_credit':0}


def reconcile(document):
    proof.same(document.get('profile'),PROFILE,'Wrong original transport evidence profile')
    proof.same(document.get('compiler_build_producer_source_sha256'),snapshot(),'Current original transport producer drift')
    root=document.get('artifacts_root')
    if not isinstance(root,str) or str(Path(root).resolve())!=root:raise ValueError('Missing normalized complete transport artifact root')
    replay=verified_document(root);proof.same(document,replay,'Supplied original transport proof differs from entire executable replay')
    result={}
    for row in load():
        identity=row['id'];layers={'javascript':{'status':'mismatch','engines':{e:copy.deepcopy(replay['frames'][identity]['application-fresh/'+e]) for e in ('QuickJS','Node')}}}
        for kind in ('release','checkout'):
            layers[kind]=copy.deepcopy(replay['references'][kind][identity]);layers[kind]['status']='mismatch'
        result[row['obligation_id']]={'id':identity,'obligation_id':row['obligation_id'],'status':'version-difference','canonical_contract_status':'passed','actual_macro_contract_status':'passed','original_expectation_status':'unresolved','original_semantic_pass_credit':0,'checked_pin_contract_status':{'release':'passed','checkout':'passed'},'source_binding':copy.deepcopy(row),'layers':layers,'actual_phase_frames':copy.deepcopy(replay['frames'][identity]),'reconciliation':'verified-canonical-sci-difference'}
    return result


def discover_references(root,timeout=600):
    root=Path(root).resolve();root.mkdir(parents=True,exist_ok=False);programs=proof.executable_paths()
    with patch.object(proof,'ORACLE',ORACLE):
        for kind in ('release','checkout'):
            proof.produce_reference(kind,root/kind,sources(),programs,timeout)
            print('Fresh independent '+kind+' original CLJC intermediate transport completed',flush=True)
    return root


def run(directory,timeout=600):
    root=Path(directory).resolve();root.mkdir(parents=True,exist_ok=False);before=snapshot();rows=load();programs=proof.executable_paths();hashes={p:sci.digest(Path(p).read_bytes()) for p in programs.values()}
    with patch.object(proof,'ORACLE',ORACLE):
        for kind in ('release','checkout'):
            proof.produce_reference(kind,root/kind,sources(),programs,timeout);print('Fresh '+kind+' original intermediate transport completed',flush=True)
    phases=root/'phases';phases.mkdir();(phases/'input.tsv').write_text(input_text(rows))
    build=proof.command([programs['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],phases/'build',timeout)
    artifacts=[json.loads(l) for l in build.stdout.splitlines() if l.startswith('{')];binaries=[a['executable'] for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')==DRIVER.stem and a.get('executable')]
    proof.same(len(binaries),1,'Missing actual transport audit executable');binary=binaries[0];hashes[binary]=sci.digest(Path(binary).read_bytes());env={'LIGHT_SCI_CLJC_TRANSPORT_ROOT':str(phases),'NODE':programs['node']}
    proof.command([binary,TEST,'--ignored','--exact','--nocapture'],phases/'audit',min(timeout,300),environment={**os.environ,**env},record_environment=env)
    proof.same(before,snapshot(),'Original transport producer changed during fresh run')
    binding={'producer_source_sha256':before,'programs':programs,'executables_sha256':hashes,'audit_executable':binary,'audit_environment':env,'artifact_sha256':proof.tree(root)}
    (root/'binding.json').write_text(json.dumps(binding,indent=2)+'\n');document=verified_document(root);(root/'evidence.json').write_text(json.dumps(document,indent=2)+'\n');reconcile(document)
    print('Verified3 unchanged original CLJC intermediate sources/24 actualphase/engine frames;zero SCIoriginalpasscredit',flush=True);return document

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--timeout',type=int,default=600);args=parser.parse_args();run(args.output,args.timeout)
