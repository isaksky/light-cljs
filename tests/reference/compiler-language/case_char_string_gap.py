"""Exact original case character/string contrast; negative evidence only."""
from pathlib import Path
import argparse, json, os
import sci_cljc_accepted_context as proof
import sci_execution as sci

ROOT=sci.ROOT
SCI_ID='sci-control-case-char-string'
ID='sci-execution-'+SCI_ID
SOURCE=r'''[(case \a "a" 1 \a 2 3) (case "a" \a 1 "a" 2 3)]'''
SOURCE_HASH='2ffde7c522ee281f970f51bd142f5c19ec3dea01dba215b1b08d596f180a15c9'
ROW_HASH='e5e221d30be5ec180eed342b952a721250e8e153f967a8aab2358f03f6c71d38'
BODY_HASH='2283cdb6e2ac981d417c506bdfc15c93e1b091bddc347f33cb09f6c19d082b12'
DRIVER=ROOT/'tests/compiler_case_char_string_gap.rs'
TEST='original_case_char_string_rejection_and_actual_macro_recovery'
RECOVERY='(defmacro case-recovery [] (+ 20 22)) (case-recovery)'


def load():
    line,fields=sci.rows(sci.CORPUS)[SCI_ID];body,disposition=sci.rows(sci.DISPOSITIONS)[SCI_ID]
    proof.same([sci.digest(line),sci.digest(body),fields[2]], [ROW_HASH,BODY_HASH,SOURCE], 'Changed exact original case source/row/body')
    proof.same(disposition[:7],fields,'Original case expectation drift')
    original={'id':ID,'outcome':{'status':'returned','value':{'$vector':[2,2]}},'trace':[],'stdout':''}
    proof.same(sci.expected(fields),original,'Original SCI case result drift')
    return {'id':ID,'sci_id':SCI_ID,'source':SOURCE,'source_sha256':SOURCE_HASH,'original_row_sha256':ROW_HASH,
            'body_sha256':BODY_HASH,'original_expected':original,'canonical_expected':{'id':ID,'outcome':{'status':'returned','value':{'$vector':[1,1]}},'trace':[],'stdout':''}}


def module(phase):
    if phase=='application':return '(ns user)\n(defn run [] (do '+SOURCE+'))\nrun\n'
    if phase=='macro':return "(ns user)\n(defmacro upstream [] (list 'quote (do "+SOURCE+")))\n(defn run [] (upstream))\nrun\n"
    raise ValueError('Unknown case phase')


def snapshot():
    paths=[Path(__file__),DRIVER,Path(__file__).with_name('test_case_char_string_gap.py')]
    return {**proof.snapshot(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def run(directory,timeout=600):
    root=Path(directory).resolve();root.mkdir(parents=True,exist_ok=False)
    row=load();before=snapshot();programs=proof.executable_paths()
    binaries={p:sci.digest(Path(p).read_bytes()) for p in programs.values()}
    local=root/'local';local.mkdir();(local/'original.cljs').write_text(SOURCE)
    build_command=[programs['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json']
    build=proof.command(build_command,local/'build',timeout)
    emitted=[r['executable'] for r in map(json.loads,build.stdout.splitlines()) if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    if len(emitted)!=1:raise ValueError('Missing actual case diagnostic executable')
    binary=emitted[0];binaries[binary]=sci.digest(Path(binary).read_bytes())
    env={'LIGHT_CASE_CHAR_STRING_ROOT':str(local),'NODE':programs['node']}
    audit_command=[binary,TEST,'--ignored','--exact','--nocapture']
    ran=proof.command(audit_command,local/'audit',min(timeout,300),environment={**os.environ,**env},record_environment=env)
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in ran.stdout:raise ValueError('Narrow actual original case audit failed')
    proof.same(ran.stderr,'','Unexpected diagnostic audit stderr')
    records=[line.split('\t') for line in (local/'observations.tsv').read_text().splitlines()]
    proof.same([r[0] for r in records],list(proof.VARIANTS),'Missing/reordered exact actual case contexts')
    frames=[]
    for fields in records:
        if len(fields)!=6:raise ValueError('Malformed actual original case rejection/recovery frame')
        variant,source,error,unit,qjs,node=fields
        source,error,unit=[bytes.fromhex(v).decode() for v in (source,error,unit)]
        folder=local/variant
        proof.same(source,module(variant.split('-')[0]),'Changed original actual case context')
        proof.same((folder/'user.cljs').read_text(),source,'Reduced original case namespace')
        proof.same((folder/'rejection.txt').read_text(),error,'Changed actual duplicate rejection')
        if 'duplicate case test constant' not in error:raise ValueError('Unexpected original case failure')
        proof.same((folder/'recovery.cljs').read_text(),RECOVERY,'Invented recovery control')
        proof.same((folder/'recovery.js').read_text(),unit,'Wrong actual recovery compiled code')
        proof.same((folder/'node.stdin').read_text(),'console.log('+unit+');','Wrong actual recovery Node input')
        proof.same((folder/'node.stdout').read_text().strip(),'42','Actual Node recovery failed')
        proof.same((folder/'node.stderr').read_text(),'','Actual Node recovery stderr')
        proof.same([qjs,node],['42','42'],'Actual same-host QuickJS/Node recovery failed')
        if '  return 42;\n' not in unit:raise ValueError('Missing actual macro recovery computation')
        frames.append({'variant':variant,'actual_complete_original_source':source,'source_sha256':sci.digest(source),
                       'status':'rejected','detail':error,'same_host_actual_macro_recovery':{'source':RECOVERY,'compiled_js_sha256':sci.digest(unit),'QuickJS':42,'Node':42}})
    references={}
    for kind in ('release','checkout'):
        proof.produce_reference(kind,root/kind,[row],programs,timeout)
        references[kind]=proof.verify_reference(kind,root/kind,[row],programs)[ID]
        proof.same(references[kind]['status'],'passed','Exact original canonical case contrast unconfirmed')
        print(kind+' exact original case returns [1 1]',flush=True)
    proof.same(snapshot(),before,'Current source/producer changed during narrow diagnosis')
    for p,h in binaries.items():proof.same(sci.digest(Path(p).read_bytes()),h,'Actual diagnostic executable drift')
    result={'schema_version':1,'profile':'exact-original-case-char-string-negative-diagnostic-v1','artifacts_root':str(root),
            'original_source_binding':row,'references':references,'actual_local_rejections':frames,
            'compiler_build_producer_source_sha256':before,'executables_sha256':binaries,
            'artifact_sha256':proof.tree(root),'semantic_pass_credit':0,'local_execution_credit':0,'macro_phase_credit':0,
            'public_gap':'Canonical character/string case constants return [1 1]; current complete original application and demanded macro sources reject duplicate case test constant. Historical SCI [2 2] remains immutable.'}
    (root/'evidence.json').write_text(json.dumps(result,indent=2)+'\n')
    print('Confirmed exact public case contrast; four source rejections with eight same-host engine recovery controls; zero credit',flush=True)
    return result


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',required=True,type=Path);a=p.parse_args();run(a.output)
