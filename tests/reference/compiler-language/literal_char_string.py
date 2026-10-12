"""Fresh independent two-pin and eight-frame character/string literal proof."""
from pathlib import Path
from unittest.mock import patch
import argparse, copy, json, os
import sci_cljc_accepted_context as proof
import sci_execution as sci

ROOT=sci.ROOT
PROFILE='canonical-literal-char-string-v1'
DRIVER=ROOT/'tests/compiler_literal_char_string.rs'
TEST='literal_char_string_sources_execute_in_fresh_and_reused_macro_hosts'
CONTRACTS=Path(__file__).with_name('literal-char-string-contracts.json')
CONTRACTS_SHA256='e77840fcef2218413abecf911c84d48f8e6e91402511f7dc3fe05a8a315e8432'
ORIGINAL=r'''[(count {\a 1 "a" 2}) (get {\a 1 "a" 2} \a) (count #{\a "a"}) (contains? #{\a "a"} "a")]'''
CONTROLS=(
 ('original',ORIGINAL),
 ('map-order-lookup',r'''(let [m {\a 1 "a" 2}] [(count m) (get m \a) (get m "a") (contains? m \a) (contains? m "a") (vec (seq m)) (vec (keys m)) (vec (vals m))])'''),
 ('map-reverse-order',r'''(let [m {"a" 2 \a 1}] [(count m) (get m \a) (get m "a") (vec (seq m))])'''),
 ('set-order-membership',r'''(let [s #{\a "a"}] [(count s) (contains? s \a) (contains? s "a") (get s \a) (get s "a") (vec (seq s))])'''),
 ('set-reverse-order',r'''(let [s #{"a" \a}] [(count s) (vec (seq s)) (contains? s "a")])'''),
 ('quoted',r'''(let [m '{\a 1 "a" 2} s '#{\a "a"}] [(count m) (get m \a) (vec (seq m)) (count s) (vec (seq s))])'''),
 ('nested-literals',r'''(let [x [{:m {\a 1 "a" 2} :s #{\a "a"}}] m (get (first x) :m) s (get (first x) :s)] [(count m) (get m \a) (vec (seq m)) (count s) (vec (seq s))])'''),
 ('quoted-nested',r'''(let [x '({:m {\a 1 "a" 2} :s #{\a "a"}}) m (get (first x) :m) s (get (first x) :s)] [(count m) (get m \a) (count s) (vec (seq s))])'''),
 ('unicode-spelling',r'''[(count {\u0061 1 "a" 2}) (get {\u0061 1 "a" 2} \a) (count #{\u0061 "a"}) (vec (seq #{\u0061 "a"}))]'''),
 ('named-spelling',r'''[(count {\newline 1 "\n" 2}) (get {\newline 1 "\n" 2} \u000a) (count #{\space " "}) (vec (seq #{\space " "}))]'''),
 ('map-effects',r'''(let [events (atom []) m {\a (do (swap! events conj :first) 11) "a" (do (swap! events conj :second) 22)} output (with-out-str (println (count m)) (println (get m \a)))] [(count m) (get m \a) (vec (seq m)) @events output])'''),
 ('runtime-constructors',r'''(let [m (hash-map \a 1 "a" 2) s (set [\a "a"])] [(count m) (get m \a) (count s) (vec (seq s))])'''),
 ('dynamic-map',r'''(let [a \a b "a" m {a 1 b 2}] [(count m) (get m a) (get m b) (vec (seq m))])'''),
 ('dynamic-set-distinct',r'''(let [a \a b "b"] [(count #{a b}) (contains? #{a b} "a") (contains? #{a b} "b")])'''),
 ('map-eight-pairs',r'''(let [m {\a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8}] [(count m) (get m \a) (get m "a") (vec (vals m))])'''),
 ('map-nine-pairs',r'''(let [m {\a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m \a) (get m "a")])'''),
 ('set-ten-scalars',r'''(let [s #{\a "a" :b :c :d :e :f :g :h :i}] [(count s) (contains? s \a) (contains? s :i)])'''),
 ('vector-map-keys',r'''(let [m {[\a] 1 ["a"] 2}] [(count m) (get m [\a]) (vec (seq m))])'''),
 ('quoted-vector-map-keys',r'''(let [m '{[\a] 1 ["a"] 2}] [(count m) (get m [\a]) (vec (seq m))])'''),
 ('vector-set-distinct',r'''(let [s #{[\a] ["b"]}] [(count s) (contains? s ["a"]) (contains? s [\b])])'''),
 ('quoted-vector-set-members',r'''(let [s '#{[\a] ["a"]}] [(count s) (vec (seq s)) (contains? s ["a"])])'''),
 ('quoted-composite-map-keys',r'''(let [m '{{\a 1} 11 {"a" 1} 22} n '{#{\a} 33 #{"a"} 44}] [(count m) (get m {"a" 1}) (count n) (get n #{"a"})])'''),
 ('quoted-composite-set-members',r'''(let [s '#{{\a 1} {"a" 1}} t '#{#{\a} #{"a"}}] [(count s) (count t) (contains? s {"a" 1}) (contains? t #{"a"})])'''),
 ('runtime-small-constructor-difference',r'''[(count (array-map \a 1 "a" 2)) (get (array-map \a 1 "a" 2) \a) (count (hash-set \a "a"))]'''),
 ('dynamic-set-duplicate-difference',r'''(let [a \a b "a"] (try [(count #{a b})] (catch :default e [:duplicate])))'''),
 ('vector-set-duplicate-difference',r'''(try [(count #{[\a] ["a"]})] (catch :default e [:duplicate]))'''),
 ('quoted-inner-quote-set',r'''(let [s '#{\a "a" '\b \b}] [(count s) (contains? s \b) (contains? s '(quote \b))])'''),
 ('quoted-inner-quote-set-order-difference',r'''(let [s '#{\a "a" '\b \b}] [(count s) (contains? s \b) (contains? s '(quote \b)) (vec (seq s))])'''),
 ('quoted-inner-quote-map',r'''(let [m '{\a 1 "a" 2 '\b 3 \b 4}] [(count m) (get m \b) (get m '(quote \b)) (vec (vals m))])'''),
 ('numeric-literals',r'''[(count {1 :int 1.0 :float}) (get {1 :int 1.0 :float} 1) (count #{1 1.0}) (vec (seq #{1 1.0}))]'''),
)
NEGATIVES=(
 ('map-character',r'''{\a 1 \a 2}''','Duplicate key','duplicate map key'),
 ('map-character-spelling',r'''{\a 1 \u0061 2}''','Duplicate key','duplicate map key'),
 ('map-string',r'''{"a" 1 "a" 2}''','Duplicate key','duplicate map key'),
 ('set-character',r'''#{\a \a}''','Duplicate key','duplicate set member'),
 ('set-character-spelling',r'''#{\a \u0061}''','Duplicate key','duplicate set member'),
 ('set-string',r'''#{"a" "a"}''','Duplicate key','duplicate set member'),
 ('map-nested-duplicate',r'''{:x {\a 1 \u0061 2}}''','Duplicate key','duplicate map key'),
 ('set-nested-duplicate',r'''[#{"a" "a"}]''','Duplicate key','duplicate set member'),
 ('map-odd',r'''{\a 1 "a"}''','even number','even number'),
 ('map-unclosed',r'''{\a 1 "a" 2''','EOF','expected'),
 ('set-unclosed',r'''#{\a "a"''','EOF','expected'),
)
RECOVERY='(defmacro literal-recovery [] (+ 20 22)) (literal-recovery)'


def specs():
    rows=[]
    for name,source in CONTROLS:
        identity='literal-char-string-'+name
        empty={'id':identity,'outcome':{'status':'returned','value':None},'trace':[],'stdout':''}
        rows.append({'id':identity,'source':source,'source_sha256':sci.digest(source),'original_expected':empty,'canonical_expected':None})
    negatives=[]
    for name,source,pin,local in NEGATIVES:
        identity='literal-rejection-'+name
        negatives.append({'id':identity,'source':source,'source_sha256':sci.digest(source),'original_expected':{'id':identity,'outcome':{'status':'returned','value':None},'trace':[],'stdout':''},'canonical_expected':None,'pin_diagnostic':pin,'local_diagnostic':local})
    return rows,negatives


def load():
    raw=CONTRACTS.read_bytes();proof.same(sci.digest(raw),CONTRACTS_SHA256,'Unreviewed independently derived literal contract')
    frozen=json.loads(raw);rows,negative=specs()
    proof.same(frozen['sources'],[{k:r[k] for k in ('id','source','source_sha256')} for r in rows+negative],'Literal source drift')
    proof.same(len(frozen['canonical_expected']),len(rows),'Missing reviewed literal')
    for row,expected in zip(rows,frozen['canonical_expected']):
        proof.same(expected['id'],row['id'],'Wrong literal identity');row['canonical_expected']=expected
        row['original_expected']=copy.deepcopy(expected)
        row['checkout_expected']=frozen['checkout_expected'].get(row['id'],copy.deepcopy(expected))
        row['two_pin_agreement']=expected==row['checkout_expected']
        row['canonical_acceptance']=not row['id'].endswith('set-duplicate-difference')
        row['local_expected']=copy.deepcopy(expected)
        if row['id'].endswith('set-duplicate-difference'):
            row['local_expected']=copy.deepcopy(row['checkout_expected'])
    proof.same(rows[0]['canonical_expected']['outcome']['value'],{'$vector':[2,1,2,True]},'Original literal result drift')
    return rows,negative


def snapshot():
    paths=[Path(__file__),DRIVER,CONTRACTS,Path(__file__).with_name('test_literal_char_string.py')]
    return {**proof.snapshot(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def references(root,rows,negative,programs):
    result={}
    for kind in ('release','checkout'):
        pin_rows=copy.deepcopy(rows)
        if kind=='checkout':
            for row in pin_rows:row['canonical_expected']=row['checkout_expected']
        result[kind]=proof.verify_reference(kind,root/kind,pin_rows+negative,programs)
    for row in rows:
        for kind in result:proof.same(result[kind][row['id']]['status'],'passed','Literal disagrees with independent '+kind)
    for row in negative:
        for kind in result:
            frame=result[kind][row['id']];proof.same(frame['status'],'rejected','Duplicate/malformed literal accepted by pin')
            proof.same(frame['detail'],json.loads(CONTRACTS.read_text())['diagnostics'][kind][row['id']],'Canonical literal diagnostic drift')
            if row['pin_diagnostic'] not in frame['detail']:raise ValueError('Wrong canonical literal diagnostic: '+row['id']+': '+frame['detail'])
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
            proof.same((folder/'user.cljc').read_text(),source,'Diagnostic source drift')
            error=(folder/'rejection.txt').read_text()
            if row['local_diagnostic'] not in error and not (row['local_diagnostic']=='duplicate map key' and 'DuplicateMapKey' in error):raise ValueError('Wrong actual local literal diagnostic: '+row['id']+': '+error)
            proof.same((folder/'recovery.cljs').read_text(),RECOVERY,'Recovery source drift')
            js=(folder/'recovery.js').read_text()
            if '  return 42;\n' not in js:raise ValueError('Missing demanded macro recovery')
            script='JSON.stringify('+js+')'
            proof.same((folder/'script.js').read_text(),script,'Recovery script drift')
            proof.same((folder/'node.stdin').read_text(),'console.log('+script+');','Recovery Node source drift')
            proof.same([(folder/'node.exit').read_text(),(folder/'node.stderr').read_text(),(folder/'node.stdout').read_text()],['0','','42\n'],'Actual Node recovery failed')
            proof.same((folder/'quickjs.raw').read_text(),'42','Actual QuickJS recovery failed')
            for engine in ('QuickJS','Node'):
                fields=records[cursor];cursor+=1
                proof.same(fields,[row['id'],variant,engine,source.encode().hex(),error.encode().hex(),js.encode().hex(),'42'.encode().hex()],'Detached recovery frame')
            frames[row['id']][variant]={'status':'rejected','detail':error,'phase_source_sha256':sci.digest(source),'same_host_actual_macro_recovery':{'source':RECOVERY,'compiled_js_sha256':sci.digest(js),'QuickJS':42,'Node':42}}
            required|={prefix+name for name in ('user.cljc','rejection.txt','recovery.cljs','recovery.js','script.js','quickjs.raw','node.stdin','node.stdout','node.stderr','node.exit')}
    proof.same(cursor,len(records),'Extra recovery frames');proof.same(set(proof.tree(root)),required,'Missing/extra diagnostic artifacts')
    return frames


def verified_document(directory):
    root=Path(directory).resolve();before=snapshot();rows,negative=load();binding=json.loads((root/'binding.json').read_text())
    proof.same(binding['producer_source_sha256'],before,'Literal/compiler producer drift')
    proof.same(binding['programs'],proof.executable_paths(),'Executable path drift')
    proof.same(set(binding['executables_sha256']),set(binding['programs'].values())|{binding['audit_executable']},'Missing executable hashes')
    for path,expected in binding['executables_sha256'].items():proof.same(sci.digest(Path(path).read_bytes()),expected,'Executable drift')
    proof.same(binding['audit_environment'],{'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(root/'phases'),'NODE':binding['programs']['node']},'Phase environment drift')
    proof.same(binding['artifact_sha256'],proof.tree(root,('binding.json','evidence.json')),'Artifact census/hash drift')
    refs=references(root,rows,negative,binding['programs'])
    local_rows=copy.deepcopy(rows)
    for row in local_rows:row['canonical_expected']=row['local_expected']
    with patch.object(proof,'DRIVER',DRIVER),patch.object(proof,'TEST',TEST):frames=proof.verify_local(root/'phases',local_rows,binding)
    for row in rows:
        proof.same(list(frames[row['id']]),[v+'/'+e for v in proof.VARIANTS for e in ('QuickJS','Node')],'Missing eight actual literal contexts')
        for frame in frames[row['id']].values():
            proof.same(frame['status'],'passed','Actual literal phase mismatch: '+row['id'])
            frame['local_regression_status']='passed'
            if not row['canonical_acceptance']:frame['status']='canonical-mismatch'
            frame['two_pin_agreement_credit']=1 if row['canonical_acceptance'] else 0
    diagnostics=verify_diagnostics(root/'diagnostics',negative);proof.same(before,snapshot(),'Producer changed during replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'source_bindings':rows,'diagnostic_sources':negative,'execution_binding':binding,'compiler_build_producer_source_sha256':before,'references':refs,'frames':frames,'diagnostics':diagnostics,'canonical_contract_status':'passed','actual_macro_contract_status':'passed','status':'passed','accepted_source_count':sum(r['canonical_acceptance'] for r in rows),'accepted_engine_frame_count':sum(r['canonical_acceptance'] for r in rows)*8,'nonpassing_neighbor_count':sum(not r['canonical_acceptance'] for r in rows),'original_semantic_pass_credit':0}


def reconcile(document):
    proof.same(document.get('profile'),PROFILE,'Wrong literal evidence profile')
    root=document.get('artifacts_root')
    if not isinstance(root,str) or str(Path(root).resolve())!=root:raise ValueError('Missing normalized raw literal artifact root')
    replay=verified_document(root);proof.same(document,replay,'Literal evidence differs from raw executable replay')
    return replay


def run(directory,timeout=600):
    root=Path(directory).resolve();root.mkdir(parents=True,exist_ok=False);before=snapshot();rows,negative=load();programs=proof.executable_paths()
    hashes={path:sci.digest(Path(path).read_bytes()) for path in programs.values()}
    for kind in ('release','checkout'):
        proof.produce_reference(kind,root/kind,rows+negative,programs,timeout);print('Fresh '+kind+' literal observations completed',flush=True)
    references(root,rows,negative,programs)
    phases=root/'phases';phases.mkdir();(phases/'input.tsv').write_text(proof.input_text(rows))
    diagnostics=root/'diagnostics';diagnostics.mkdir();(diagnostics/'input.tsv').write_text(proof.input_text(negative))
    build=proof.command([programs['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],phases/'build',timeout)
    emitted=[r['executable'] for r in map(json.loads,build.stdout.splitlines()) if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    proof.same(len(emitted),1,'Missing exact actual Cargo audit binary');binary=emitted[0];hashes[binary]=sci.digest(Path(binary).read_bytes())
    env={'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(phases),'NODE':programs['node']}
    proof.command([binary,TEST,'--ignored','--exact','--nocapture'],phases/'audit',min(timeout,300),environment={**os.environ,**env},record_environment=env)
    proof.same(before,snapshot(),'Source drift during fresh literal proof')
    binding={'producer_source_sha256':before,'programs':programs,'executables_sha256':hashes,'audit_executable':binary,'audit_environment':env,'artifact_sha256':proof.tree(root)}
    (root/'binding.json').write_text(json.dumps(binding,indent=2)+'\n');document=verified_document(root)
    (root/'evidence.json').write_text(json.dumps(document,indent=2)+'\n');reconcile(document)
    print('Verified '+str(len(rows)*8)+' actual literal source-phase engine frames and '+str(len(negative)*8)+' same-host diagnostic recovery engine outcomes',flush=True)
    return document

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',required=True,type=Path);parser.add_argument('--timeout',default=600,type=int);args=parser.parse_args();run(args.output,args.timeout)
