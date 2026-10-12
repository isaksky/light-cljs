"""Exact original .cljc rejections and host recovery; no original semantic credit."""
from pathlib import Path
import argparse, copy, json, os, shutil, subprocess, tempfile
import report
import sci_execution as sci
import sci_cljc_source_probe as historical
ROOT=report.ROOT
HERE=Path(__file__).parent
TABLE=HERE/'cljc-rejections-after191-contracts.json'
TABLE_SHA='432dc7dd6cf67fe1bd1554d04fd1cbe9dbd5a19b5a43623259bc6bffd8ffdcb9'
DRIVER=ROOT/'tests/compiler_cljc_rejections_after191.rs'
TEST='unchanged_original_reader_rejections_preserve_source_and_host_recovery'
PROFILE='exact-original-cljc-rejections-after191-v1'
WARM='(defmacro before-reader-rejection [] (+ 20 22)) (before-reader-rejection)'
RECOVERY='(defmacro after-reader-rejection [] (+ 20 22)) (after-reader-rejection)'

def same(a,b,reason):
    if not report.same_json(a,b):raise ValueError(reason)

def load():
    same(sci.digest(TABLE.read_bytes()),TABLE_SHA,'Unreviewed rejection catalog')
    rows=json.loads(TABLE.read_text());same(len(rows),4,'Missing exact rejection originals')
    originals={row['id']:row for row in historical.catalog()}
    same(len({r['id'] for r in rows}),4,'Duplicate rejection identity')
    for row in rows:
        original=originals[row['id']]
        same({key:row[key] for key in original},original,'Changed original source/body/row/value/effects')
    same((ROOT/'tests/fixtures/cljc-rejections-after191.tsv').read_text(),input_text(rows),'Changed complete default original fixture')
    return rows

def snapshot():
    paths=[Path(__file__),TABLE,DRIVER,HERE/'test_cljc_rejections_after191.py',HERE/'cljc_rejection_after191_accounting.py',ROOT/'tests/fixtures/cljc-rejections-after191.tsv']
    return {**historical.snapshot(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}

def tree(root):
    return {str(p.relative_to(root)):sci.digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file()}

def executable_binding():
    result={}
    for name in ('cargo','node','clojure'):
        p=Path(shutil.which(name) or '').resolve()
        if not p.is_file():raise ValueError('Missing actual executable '+name)
        result[name]={'path':str(p),'sha256':sci.digest(p.read_bytes())}
    return result

def source(original,variant):
    if variant.startswith('macro-'):
        return "(ns user)\n(defmacro upstream [] (list 'quote (do "+original+")))\n(defn run [observe] (upstream))\nrun\n"
    return '(ns user)\n(defn run [observe] (do '+original+'))\nrun\n'

def input_text(rows):
    return ''.join(r['id']+'\t'+r['source'].encode().hex()+'\t'+r['required_diagnostic'].encode().hex()+'\n' for r in rows)

def local_run(root,rows,original_root=None):
    root.mkdir();(root/'input.tsv').write_text(input_text(rows))
    cmd=['env','LIGHT_CLJC_REJECTIONS_AFTER191_ROOT='+str(root)]
    if original_root:cmd+=['LIGHT_CLJC_REJECTIONS_AFTER191_SOURCE_ROOT='+str(original_root)]
    cmd+=['cargo','test','--locked','--message-format=json','--test',DRIVER.stem,TEST,'--','--exact','--nocapture']
    ran=report.invoke(cmd,root/'command',timeout=300)
    if ran['status']!='completed' or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in ran.get('stdout',''):raise ValueError('Actual reader/recovery test failed')
    artifacts=[]
    for line in ran['stdout'].splitlines():
        try:packet=json.loads(line)
        except json.JSONDecodeError:continue
        if packet.get('reason')=='compiler-artifact' and packet.get('target',{}).get('name')==DRIVER.stem and packet.get('executable'):artifacts.append(packet['executable'])
    same(len(artifacts),1,'Missing actual driver executable')
    executable=Path(artifacts[0]);executable_hash=sci.digest(executable.read_bytes())
    frames=parse_local(root,rows,original_root or root)
    return {'command':cmd,'command_result':ran,'actual_executable':str(executable),'actual_executable_sha256':executable_hash,'frames':frames,'artifact_sha256':tree(root)}

def parse_local(root,rows,original_root):
    same((root/'input.tsv').read_text(),input_text(rows),'Changed exact original input')
    same((root/'warmup.cljs').read_text(),WARM,'Changed real macro warmup')
    byid={r['id']:r for r in rows};frames=[];rejections=set();recoveries=set();warmups=set()
    for line in (root/'observations.tsv').read_text().splitlines():
        f=line.split('\t');kind=f[0]
        if kind=='warmup':
            same(len(f),3,'Malformed warmup frame');same(bytes.fromhex(f[2]).decode(),'42','Nonpassing actual warmup')
            if f[1] not in ('QuickJS','Node') or f[1] in warmups:raise ValueError('Bad/duplicate warmup frame')
            warmups.add(f[1]);continue
        identity,variant=f[1:3]
        if identity not in byid or variant not in historical.VARIANTS:raise ValueError('Unknown source/phase identity')
        row=byid[identity];directory=root/identity/variant;original=original_root/identity/variant/'user.cljc'
        expected=source(row['source'],variant)
        same(original.read_text(),expected,'Changed complete physical original source')
        same((directory/'input-source.txt').read_text(),expected,'Changed full input source packet')
        if kind=='rejected':
            same(len(f),11,'Malformed located rejection frame');same(bytes.fromhex(f[3]).decode(),expected,'Reduced/changed original rejection source')
            message=bytes.fromhex(f[4]).decode();start,end=int(f[5]),int(f[6]);path=bytes.fromhex(f[7]).decode();line_no,col=int(f[8]),int(f[9]);detail=bytes.fromhex(f[10]).decode()
            same(path,str(original),'Changed actual diagnostic source path')
            if not 0<=start<end<=len(expected.encode()):raise ValueError('Invalid original diagnostic span')
            before=expected.encode()[:start].decode();same(line_no,before.count('\n')+1,'Wrong diagnostic line');same(col,len(before.rsplit('\n',1)[-1])+1,'Wrong diagnostic column')
            same(detail,f'{path}:{line_no}:{col}: {message} (bytes {start}..{end})','Detached located diagnostic')
            same((directory/'diagnostic.txt').read_text(),detail,'Changed saved raw diagnostic')
            if row['required_diagnostic'] not in message:raise ValueError('Wrong original rejection category')
            if (identity,variant) in rejections:raise ValueError('Duplicate rejection frame')
            rejections.add((identity,variant));frames.append({'id':identity,'variant':variant,'status':'rejected-before-original-computation','source_sha256':sci.digest(expected),'source_path':path,'message':message,'span':[start,end],'line_column':[line_no,col],'detail':detail})
        elif kind=='recovery':
            same(len(f),6,'Malformed recovery frame');engine=f[3]
            if engine not in ('QuickJS','Node') or (identity,variant,engine) in recoveries:raise ValueError('Bad/duplicate recovery engine')
            same((directory/'recovery.cljs').read_text(),RECOVERY,'Changed actual recovery source')
            js=bytes.fromhex(f[4]).decode();raw=bytes.fromhex(f[5]).decode();same((directory/'recovery.js').read_text(),js,'Changed executed recovery JS');same(raw,'42','Nonpassing same-host recovery')
            recoveries.add((identity,variant,engine));frames.append({'id':identity,'variant':variant,'engine':engine,'status':'recovery-passed','recovery_source_sha256':sci.digest(RECOVERY),'emitted_sha256':sci.digest(js),'raw':raw})
        else:raise ValueError('Unknown original/recovery frame')
    same(warmups,{'QuickJS','Node'},'Missing warmup engine pair')
    same(rejections,{(r['id'],v) for r in rows for v in historical.VARIANTS},'Incomplete original source rejection census')
    same(recoveries,{(r['id'],v,e) for r in rows for v in historical.VARIANTS for e in ('QuickJS','Node')},'Incomplete same-host recovery census')
    return frames

def reference_check(rows,refs):
    result=[]
    for row in rows:
        kind=row['canonical_reference_rejection'];layers={pin:refs[pin][row['id']] for pin in ('release','checkout')}
        for pin,layer in layers.items():
            same(layer['original_source_sha256'],row['source_sha256'],'Reference original changed')
            same(layer['reference_identity'],pin+' '+(report.RELEASE if pin=='release' else report.REVISION),'Wrong default pin identity')
            same(layer['oracle_sha256'],sci.digest(historical.ORACLE.read_bytes()),'Changed actual oracle producer')
            same(layer['settings'],{'optimizations':'none','static-fns':False,'def-emits-var':False},'Changed default reference settings')
            if kind:
                same(layer['status'],'rejected','Malformed original unexpectedly executed in a pin')
                if kind not in str(layer['detail']):raise ValueError('Wrong canonical original reader rejection')
            else:
                same(layer['status'],'passed','Canonical metadata original changed')
                same(layer['observed'],row['expected'],'Canonical metadata packet changed')
        result.append({'id':row['id'],'classification':'verified-original-source-rejection' if kind else 'unresolved-local-unbound-name-policy','original_semantic_pass_credit':0,'original_macro_execution_pass_credit':0,'reference_layers':layers})
    return result

def run(root):
    root=Path(root).resolve();root.mkdir(parents=True,exist_ok=False);rows=load();before=snapshot();executables=executable_binding()
    local=local_run(root/'local',rows);refs={};packages={}
    for pin in ('release','checkout'):
        refs[pin]=historical.reference(rows,pin,root/pin);packages[pin]=tree(root/pin)
    same(snapshot(),before,'Producer/input changed during exact original audit');same(executable_binding(),executables,'Executable drift')
    doc={'schema_version':1,'profile':PROFILE,'root':str(root),'source_binding':rows,'source_snapshot':before,'executable_binding':executables,'local':local,'references':refs,'reference_artifact_sha256':packages,'classifications':reference_check(rows,refs),'original_semantic_pass_credit':0,'original_macro_execution_pass_credit':0,'strict_complete':False,'prior_failed_history':'development-v1 canonical /tmp filesystem alias mismatch is nonpassing; sources were not changed'}
    (root/'evidence.json').write_text(json.dumps(doc,indent=2)+'\n');return doc

def replay(doc):
    same(doc['profile'],PROFILE,'Wrong diagnostic profile');rows=load();same(doc['source_binding'],rows,'Changed original binding');same(doc['source_snapshot'],snapshot(),'Stale original rejection producer');same(doc['executable_binding'],executable_binding(),'Changed actual executables');root=Path(doc['root'])
    same(tree(root/'local'),doc['local']['artifact_sha256'],'Changed local raw/source/emitted packet')
    same(parse_local(root/'local',rows,root/'local'),doc['local']['frames'],'Changed located diagnostic/recovery frames')
    executable=Path(doc['local']['actual_executable']);same(sci.digest(executable.read_bytes()),doc['local']['actual_executable_sha256'],'Changed actual driver executable')
    for pin in ('release','checkout'):same(tree(root/pin),doc['reference_artifact_sha256'][pin],'Changed complete pin artifacts')
    for pin in ('release','checkout'):
        same((root/pin/'input.tsv').read_text(),''.join(row['id']+'\t'+row['source'].encode().hex()+'\n' for row in rows),'Changed complete original pin input')
    same(reference_check(rows,doc['references']),doc['classifications'],'Changed canonical source classification')
    before=snapshot()
    with tempfile.TemporaryDirectory(prefix='light-cljs-cljc-rejections-after191-replay-') as temporary:
        tmp=Path(temporary).resolve();fresh=local_run(tmp/'local',rows,root/'local')
        same(fresh['frames'],doc['local']['frames'],'Fresh original input recompile changed diagnostics/recovery')
        for pin in ('release','checkout'):
            rebuilt=historical.reference(rows,pin,tmp/pin);same(rebuilt,doc['references'][pin],'Fresh default pin changed complete original result/rejection')
            originals={name:digest for name,digest in doc['reference_artifact_sha256'][pin].items() if name.endswith(('.cljs','.cljc','.clj','.js'))}
            units={name:digest for name,digest in tree(tmp/pin).items() if name.endswith(('.cljs','.cljc','.clj','.js'))}
            same(units,originals,'Fresh pin source/emitted tree changed')
    same(snapshot(),before,'Producer changed during pristine replay');return True

def reconcile(document):
    """Only canonical source-invalid originals gain explicit rejection evidence.

    The original computation never executed; no engine/macro pass is invented.
    Metadata/no-match remains unresolved under the local unbound-name policy.
    """
    replay(document)
    import sci_cljc_accepted_context as original
    bindings={r["id"]:r for r in original.load()}
    result={}
    for row in load():
        if row["canonical_reference_rejection"] is None:continue
        frames=[f for f in document["local"]["frames"] if f.get("id")==row["id"]]
        rejected=[f for f in frames if f["status"]=="rejected-before-original-computation"]
        recovery=[f for f in frames if f["status"]=="recovery-passed"]
        same(len(rejected),4,"Incomplete original rejection phase/host census")
        same(len(recovery),8,"Incomplete same-host recovery engines")
        binding=copy.deepcopy(bindings[row["id"]])
        if binding.get("canonical_expected") is not None:raise ValueError("Original rejection cannot replace canonical result literal")
        result[binding["obligation_id"]]={"id":row["id"],"status":"verified-rejection",
          "reconciliation":"verified-canonical-original-source-rejection",
          "source_binding":binding,"profile":PROFILE,
          "layers":{"diagnostic":{"status":"verified-original-source-rejection","frames":rejected},
                    "javascript":{"status":"not-executed-original","original_semantic_pass_credit":0},
                    **{pin:document["references"][pin][row["id"]] for pin in ("release","checkout")}},
          "actual_phase_frames":{f["variant"]+"/rejected":f for f in rejected},
          "actual_macro_contract_status":"rejected-before-original-computation",
          "same_host_recovery_frames":recovery,
          "original_semantic_pass_credit":0,"original_macro_execution_pass_credit":0,
          "reason":"Both pinned readers reject this complete unchanged original source; fresh/reused local application and macro readers reject it with located diagnostics and both engines recover on the same hosts. The original body was not executed."}
    same(len(result),3,"Original source rejection cohort changed")
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path);p.add_argument('--replay',type=Path);a=p.parse_args()
    if a.replay:replay(json.loads(a.replay.read_text()))
    elif a.output:run(a.output)
    else:p.error('one of --output/--replay is required')
