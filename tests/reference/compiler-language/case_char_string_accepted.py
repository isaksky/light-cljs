"""Independent canonical case acceptance; complete raw source/executable replay."""
from pathlib import Path
from unittest.mock import patch
import argparse, copy, json, os
import case_char_string_gap as original
import sci_cljc_accepted_context as proof
import sci_execution as sci
import sci_symbol_keyword_context as packets

ROOT=sci.ROOT
PROFILE='canonical-case-char-string-accepted-v1'
DRIVER=ROOT/'tests/compiler_case_char_string_accepted.rs'
TEST='accepted_case_constants_execute_in_fresh_and_reused_macro_hosts'
CONTRACTS=Path(__file__).with_name('case-char-string-accepted-contracts.json')
CONTRACTS_SHA256='ff97fa105fb32dfffcf4d8035f38c5cb3189322876808e8e858e4053d67c9ea6'
CONTROLS=(
 ('integer-float',r'''(case 1 1 :integer 1.0 :float)'''),
 ('characters',r'''[(case \a \a 1 \b 2 3) (case \z \a 1 \b 2 3)]'''),
 ('strings',r'''[(case "a" "a" 1 "b" 2 3) (case "z" "a" 1 "b" 2 3)]'''),
 ('mixed-grouped-default',r'''[(case \a ("a" \a) 1 ("b" \b) 2 3) (case "b" ("a" \a) 1 ("b" \b) 2 3) (case \z ("a" \a) 1 ("b" \b) 2 3)]'''),
 ('selector-branch-effects',r'''(let [events (atom []) result (case (do (swap! events conj :selector) \a) "a" (do (swap! events conj :first) 11) \a (do (swap! events conj :second) 22) (do (swap! events conj :default) 33))] [result @events])'''),
 ('default-effects',r'''(let [events (atom []) result (case (do (swap! events conj :selector) \z) "a" (do (swap! events conj :first) 11) \a (do (swap! events conj :second) 22) (do (swap! events conj :default) 33))] [result @events])'''),
 ('nested-vector-list',r'''[(case [[\a]] [["a"]] 1 [[\a]] 2 3) (case '(\a) ["a"] 1 [\a] 2 3)]'''),
 ('map-set-constants',r'''[(case {:x \a} {:x "a"} 1 {:x \a} 2 3) (case #{\a} #{"a"} 1 #{\a} 2 3)]'''),
 ('map-set-literals',r'''[(count {\a 1 "a" 2}) (get {\a 1 "a" 2} \a) (count #{\a "a"}) (contains? #{\a "a"} "a")]'''),
 ('unicode-character',r'''[(case \u0061 "a" 1 \a 2 3) (case \a \u0061 7 9)]'''),
)
NEGATIVES=(
 ('duplicate-characters',r'''(case \a \a 1 \a 2 3)''','Duplicate case test constant','duplicate case test constant'),
 ('duplicate-character-spelling',r'''(case \a \a 1 \u0061 2 3)''','Duplicate case test constant','duplicate case test constant'),
 ('duplicate-strings',r'''(case "a" "a" 1 "a" 2 3)''','Duplicate case test constant','duplicate case test constant'),
 ('duplicate-grouped',r'''(case \a (\a \b) 1 \b 2 3)''','Duplicate case test constant','duplicate case test constant'),
 ('malformed-selector',r'''(case* 1 [[1]] [2] 3)''','case* must switch on symbol','case* must switch on a symbol'),
 ('malformed-groups',r'''(let [x 1] (case* x [1] [2] 3))''','case* tests must be grouped in vectors','case* requires constant groups'),
 ('malformed-constant',r'''(let [x 1] (case* x [[:a]] [2] 3))''','case* tests must be numbers, strings, or constants','case* tests must be numbers, strings, or constants'),
)
RECOVERY='(defmacro case-recovery [] (+ 20 22)) (case-recovery)'


def specs():
    row=original.load()
    row.update(obligation_id='local::tests/reference/sci/evaluator-dispositions.tsv::'+original.SCI_ID,
               original_source_sha256=row['source_sha256'],source_context_profile=PROFILE)
    rows=[row]
    for name,source in CONTROLS:
        identity='case-control-'+name
        empty={'id':identity,'outcome':{'status':'returned','value':None},'trace':[],'stdout':''}
        rows.append({'id':identity,'source':source,'source_sha256':sci.digest(source),'original_expected':empty,'canonical_expected':None})
    negatives=[]
    for name,source,pin,local in NEGATIVES:
        identity='case-rejection-'+name
        negatives.append({'id':identity,'source':source,'source_sha256':sci.digest(source),
                          'original_expected':{'id':identity,'outcome':{'status':'returned','value':None},'trace':[],'stdout':''},
                          'canonical_expected':None,'pin_diagnostic':pin,'local_diagnostic':local})
    return rows,negatives


def load():
    raw=CONTRACTS.read_bytes()
    proof.same(sci.digest(raw),CONTRACTS_SHA256,'Unreviewed independently derived case literals')
    frozen=json.loads(raw)
    rows,negative=specs()
    proof.same(frozen['sources'],[{k:r[k] for k in ('id','source','source_sha256')} for r in rows+negative],'Case control source drift')
    for row,expected in zip(rows,frozen['canonical_expected']):
        row['canonical_expected']=expected
        proof.same(expected['id'],row['id'],'Wrong reviewed case literal identity')
    proof.same(len(frozen['canonical_expected']),len(rows),'Missing reviewed case literal')
    proof.same(rows[0]['canonical_expected'],original.load()['canonical_expected'],'Original canonical case literal drift')
    for row in negative:
        row['canonical_expected']=frozen['diagnostic_canonical_expected'].get(row['id'])
    return rows,negative


def snapshot():
    paths=[Path(__file__),DRIVER,CONTRACTS,Path(__file__).with_name('test_case_char_string_accepted.py'),Path(original.__file__),Path(__file__).with_name('full_ledger.py'),Path(__file__).with_name('test_case_char_string_integration.py')]
    return {**proof.snapshot(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def references(root,rows,negative,programs):
    result={kind:proof.verify_reference(kind,root/kind,rows+negative,programs) for kind in ('release','checkout')}
    for row in rows:
        for kind in result:proof.same(result[kind][row['id']]['status'],'passed','Canonical case literal disagrees with independently checked '+kind)
    for row in negative:
        for kind in result:
            frame=result[kind][row['id']]
            proof.same(frame['status'],'rejected','True duplicate/malformed case source accepted by pin')
            if row['pin_diagnostic'] not in frame['detail']:raise ValueError('Wrong canonical case diagnostic')
    return result


def verify_diagnostics(root,negative):
    proof.same((root/'input.tsv').read_text(),proof.input_text(negative),'Changed diagnostic sources')
    records=[line.split('\t') for line in (root/'observations.tsv').read_text().splitlines()]
    required={'input.tsv','observations.tsv'};frames={};cursor=0
    for row in negative:
        frames[row['id']]={}
        for variant in proof.VARIANTS:
            folder=root/row['id']/variant;prefix=row['id']+'/'+variant+'/'
            source=proof.historical.source(row['source'],variant.split('-')[0])
            proof.same((folder/'user.cljc').read_text(),source,'Diagnostic full source drift')
            error=(folder/'rejection.txt').read_text()
            if row['local_diagnostic'] not in error:raise ValueError('Wrong actual local case diagnostic')
            proof.same((folder/'recovery.cljs').read_text(),RECOVERY,'Recovery source drift')
            js=(folder/'recovery.js').read_text()
            if '  return 42;\n' not in js:raise ValueError('Missing demanded macro recovery computation')
            script='JSON.stringify('+js+')'
            proof.same((folder/'script.js').read_text(),script,'Recovery script drift')
            proof.same((folder/'node.stdin').read_text(),'console.log('+script+');','Recovery Node input drift')
            proof.same([(folder/'node.exit').read_text(),(folder/'node.stderr').read_text(),(folder/'node.stdout').read_text()],['0','','42\n'],'Actual Node recovery failed')
            proof.same((folder/'quickjs.raw').read_text(),'42','Actual QuickJS recovery failed')
            for engine in ('QuickJS','Node'):
                fields=records[cursor];cursor+=1
                proof.same(fields,[row['id'],variant,engine,source.encode().hex(),error.encode().hex(),js.encode().hex(),'42'.encode().hex()],'Detached diagnostic/recovery engine frame')
            frames[row['id']][variant]={'status':'rejected','detail':error,'phase_source_sha256':sci.digest(source),
                                      'same_host_actual_macro_recovery':{'source':RECOVERY,'compiled_js_sha256':sci.digest(js),'QuickJS':42,'Node':42}}
            required|={prefix+name for name in ('user.cljc','rejection.txt','recovery.cljs','recovery.js','script.js','quickjs.raw','node.stdin','node.stdout','node.stderr','node.exit')}
    proof.same(cursor,len(records),'Extra diagnostic/recovery frames')
    proof.same(set(proof.tree(root)),required,'Missing/extra diagnostic artifacts')
    return frames


def verified_document(directory):
    root=Path(directory).resolve();before=snapshot();rows,negative=load()
    binding=json.loads((root/'binding.json').read_text())
    proof.same(binding['producer_source_sha256'],before,'Current case/compiler producer drift')
    proof.same(binding['programs'],proof.executable_paths(),'Actual executable path drift')
    proof.same(set(binding['executables_sha256']),set(binding['programs'].values())|{binding['audit_executable']},'Missing executable hashes')
    for path,expected in binding['executables_sha256'].items():proof.same(sci.digest(Path(path).read_bytes()),expected,'Executable drift')
    proof.same(binding['audit_environment'],{'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(root/'phases'),'NODE':binding['programs']['node']},'Phase environment drift')
    proof.same(binding['artifact_sha256'],proof.tree(root,('binding.json','evidence.json')),'Raw artifact census/hash drift')
    refs=references(root,rows,negative,binding['programs'])
    with patch.object(proof,'DRIVER',DRIVER),patch.object(proof,'TEST',TEST):frames=proof.verify_local(root/'phases',rows,binding)
    for row in rows:
        proof.same(list(frames[row['id']]),[v+'/'+e for v in proof.VARIANTS for e in ('QuickJS','Node')],'Missing eight actual case contexts')
        for frame in frames[row['id']].values():
            if row['id']=='case-control-map-set-literals':
                if frame['status'] not in ('passed','mismatch') or frame.get('observed',{}).get('outcome',{}).get('status')!='returned':raise ValueError('Neighboring literal observation failed')
            else:proof.same(frame['status'],'passed','Actual source phase mismatch')
    diagnostics=verify_diagnostics(root/'diagnostics',negative)
    proof.same(before,snapshot(),'Producer changed during replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'source_bindings':rows,'diagnostic_sources':negative,
            'execution_binding':binding,'compiler_build_producer_source_sha256':before,'references':refs,'frames':frames,'diagnostics':diagnostics,
            'canonical_contract_status':'passed','actual_macro_contract_status':'passed','original_expectation_status':'unresolved',
            'status':'version-difference','original_semantic_pass_credit':0,
            'numeric_case_status':'passed',
            'neighboring_literal_status':'passed' if all(f['status']=='passed' for f in frames['case-control-map-set-literals'].values()) else 'mismatch'}


def reconcile(document):
    proof.same(document.get('profile'),PROFILE,'Wrong case evidence profile')
    root=document.get('artifacts_root')
    if not isinstance(root,str) or str(Path(root).resolve())!=root:raise ValueError('Missing normalized raw case artifact root')
    replay=verified_document(root)
    proof.same(document,replay,'Case evidence differs from entire raw executable replay')
    row=replay['source_bindings'][0];identity=row['id']
    local={engine:copy.deepcopy(replay['frames'][identity]['application-fresh/'+engine]) for engine in ('QuickJS','Node')}
    layers={'javascript':{'status':'mismatch','engines':local}}
    for kind in ('release','checkout'):
        layers[kind]=copy.deepcopy(replay['references'][kind][identity]);layers[kind]['status']='mismatch'
    return {row['obligation_id']:{'id':identity,'obligation_id':row['obligation_id'],'status':'version-difference',
            'canonical_contract_status':'passed','actual_macro_contract_status':'passed','original_expectation_status':'unresolved',
            'original_semantic_pass_credit':0,'checked_pin_contract_status':{'release':'passed','checkout':'passed'},
            'source_binding':copy.deepcopy(row),'layers':layers,'actual_phase_frames':copy.deepcopy(replay['frames'][identity]),
            'reconciliation':'verified-canonical-sci-difference'}}


def run(directory,timeout=600):
    root=Path(directory).resolve();root.mkdir(parents=True,exist_ok=False)
    before=snapshot();rows,negative=load();programs=proof.executable_paths()
    hashes={path:sci.digest(Path(path).read_bytes()) for path in programs.values()}
    for kind in ('release','checkout'):
        proof.produce_reference(kind,root/kind,rows+negative,programs,timeout)
        print('Fresh '+kind+' case source observations completed',flush=True)
    references(root,rows,negative,programs)
    phases=root/'phases';phases.mkdir();(phases/'input.tsv').write_text(proof.input_text(rows))
    diagnostics=root/'diagnostics';diagnostics.mkdir();(diagnostics/'input.tsv').write_text(proof.input_text(negative))
    build=proof.command([programs['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],phases/'build',timeout)
    emitted=[r['executable'] for r in map(json.loads,build.stdout.splitlines()) if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    proof.same(len(emitted),1,'Missing exact actual Cargo audit binary');binary=emitted[0];hashes[binary]=sci.digest(Path(binary).read_bytes())
    env={'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(phases),'NODE':programs['node']}
    proof.command([binary,TEST,'--ignored','--exact','--nocapture'],phases/'audit',min(timeout,300),environment={**os.environ,**env},record_environment=env)
    proof.same(before,snapshot(),'Source drift during fresh case acceptance')
    binding={'producer_source_sha256':before,'programs':programs,'executables_sha256':hashes,'audit_executable':binary,'audit_environment':env,'artifact_sha256':proof.tree(root)}
    (root/'binding.json').write_text(json.dumps(binding,indent=2)+'\n')
    document=verified_document(root);(root/'evidence.json').write_text(json.dumps(document,indent=2)+'\n');reconcile(document)
    print('Verified original + 9 case controls, 1 neighboring literal diagnostic in 88 actual source-phase engine frames; 7 rejections in 28 same-host recovery contexts',flush=True)
    return document

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',required=True,type=Path);args=parser.parse_args();run(args.output)
