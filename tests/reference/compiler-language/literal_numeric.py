"""Fresh independent two-pin and eight-frame numeric literal proof."""
from pathlib import Path
from unittest.mock import patch
import argparse, copy, json, os
import sci_cljc_accepted_context as proof
import sci_execution as sci

ROOT=sci.ROOT
PROFILE='canonical-literal-numeric-v1'
DRIVER=ROOT/'tests/compiler_literal_numeric.rs'
TEST='literal_numeric_sources_execute_in_fresh_and_reused_macro_hosts'
CONTRACTS=Path(__file__).with_name('literal-numeric-contracts.json')
CONTRACTS_SHA256='e5a345ad217810674aa7f588cb6accaa0c428940a69a79d78a4e210daedea568'
ORIGINAL='[(count {1 :int 1.0 :float}) (get {1 :int 1.0 :float} 1) (count #{1 1.0}) (vec (seq #{1 1.0}))]'
CONTROLS = (
    ('original', '[(count {1 :int 1.0 :float}) (get {1 :int 1.0 :float} 1) (count #{1 1.0}) (vec (seq #{1 1.0}))]'),
    ('map-order-lookup', '(let [m {1 :int 1.0 :float}] [(count m) (get m 1) (get m 1.0) (contains? m 1) (contains? m 1.0) (vec (seq m)) (vec (keys m)) (vec (vals m))])'),
    ('map-reverse-order', '(let [m {1.0 :float 1 :int}] [(count m) (get m 1) (get m 1.0) (vec (seq m))])'),
    ('set-reverse-order', '[(count #{1.0 1}) (vec (seq #{1.0 1})) (get #{1.0 1} 1) (contains? #{1.0 1} 1)]'),
    ('quoted', "(let [m '{1 :int 1.0 :float} s '#{1 1.0}] [(count m) (get m 1) (vec (seq m)) (count s) (vec (seq s))])"),
    ('nested-literals', '(let [x [{:m {1 :int 1.0 :float} :s #{1 1.0}}] m (get (first x) :m) s (get (first x) :s)] [(count m) (get m 1) (count s) (vec (seq s))])'),
    ('quoted-nested', "(let [x '({:m {1 :int 1.0 :float} :s #{1 1.0}}) m (get (first x) :m) s (get (first x) :s)] [(count m) (get m 1) (count s) (vec (seq s))])"),
    ('hex-octal', '[(count {0x1 :hex 1.0 :float}) (get {01 :octal 1e0 :float} 1) (count #{0x1 1.0}) (vec (seq #{01 1e0}))]'),
    ('bigint-ratio', '[(count {1N :big 1.0 :float}) (get {2/2 :ratio 1.0 :float} 1) (count #{1N 1.0}) (vec (seq #{2/2 1.0}))]'),
    ('decimal-categories', '(let [m {1 :int 1M :decimal 1.0 :float} s #{1 1M 1.0}] [(count m) (get m 1) (vec (seq m)) (count s) (vec (seq s))])'),
    ('decimal-precision', '[(count {1.0000000000000001M :precise 1M :rounded}) (get {1.0000000000000001M :precise 1M :rounded} 1) (count #{1.0000000000000001M 1M}) (vec (seq #{1.0000000000000001M 1M}))]'),
    ('rounding-integers', '[(count {9007199254740992 :first 9007199254740993 :second}) (get {9007199254740992 :first 9007199254740993 :second} 9007199254740992) (count #{9007199254740992 9007199254740993}) (vec (seq #{9007199254740992 9007199254740993}))]'),
    ('wide-integers', '[(count {9223372036854775808N :first 9223372036854775809N :second}) (get {9223372036854775808N :first 9223372036854775809N :second} 9223372036854775808N) (count #{9223372036854775808N 9223372036854775809N})]'),
    ('zero-integer-float', '[(count {0 :int -0.0 :float}) (get {0 :int -0.0 :float} 0) (count #{0 -0.0}) (vec (seq #{0 -0.0}))]'),
    ('nonfinite', '(let [m {##Inf :pos ##-Inf :neg ##NaN :nan} s #{##Inf ##-Inf ##NaN}] [(count m) (get m ##Inf) (get m ##-Inf) (contains? m ##NaN) (count s) (contains? s ##Inf) (contains? s ##NaN) (vec (seq s))])'),
    ('repeated-nan', '[(count {##NaN :first ##NaN :second}) (count #{##NaN ##NaN}) (contains? #{##NaN ##NaN} ##NaN)]'),
    ('collision-and-nan', '(let [m {1 :int 1.0 :float ##NaN :nan} s #{1 1.0 ##NaN}] [(count m) (get m 1) (count s) (vec (seq s)) (contains? s ##NaN)])'),
    ('map-effects', '(let [events (atom []) m {1 (do (swap! events conj :first) 11) 1.0 (do (swap! events conj :second) 22)} output (with-out-str (println (count m)) (println (get m 1)))] [(count m) (get m 1) (vec (seq m)) @events output])'),
    ('set-effects', '(let [events (atom []) s #{(do (swap! events conj :first) 1) (do (swap! events conj :second) 2)} output (with-out-str (println (count s)))] [(count s) @events output])'),
    ('map-eight-pairs', '(let [m {1 :int 1.0 :float :b 3 :c 4 :d 5 :e 6 :f 7 :g 8}] [(count m) (get m 1) (get m 1.0) (vec (vals m))])'),
    ('map-nine-pairs', '(let [m {1 :int 1.0 :float :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 1) (get m 1.0)])'),
    ('set-eight-scalars', '[(count #{1 1.0 :b :c :d :e :f :g}) (contains? #{1 1.0 :b :c :d :e :f :g} 1)]'),
    ('set-nine-scalars', '[(count #{1 1.0 :b :c :d :e :f :g :h}) (contains? #{1 1.0 :b :c :d :e :f :g :h} 1)]'),
    ('vector-map-keys', '(let [m {[1] :int [1.0] :float}] [(count m) (get m [1]) (vec (seq m))])'),
    ('quoted-vector-map-keys', "(let [m '{[1] :int [1.0] :float}] [(count m) (get m [1]) (vec (seq m))])"),
    ('vector-set-members', '(try [(count #{[1] [1.0]})] (catch :default e [:duplicate]))'),
    ('quoted-vector-set-members', "(let [s '#{[1] [1.0]}] [(count s) (vec (seq s)) (contains? s [1])])"),
    ('quoted-composite-map-keys', "(let [m '{{:x 1} :int {:x 1.0} :float} n '{#{1} :int #{1.0} :float}] [(count m) (get m {:x 1}) (count n) (get n #{1})])"),
    ('quoted-composite-set-members', "(let [s '#{{:x 1} {:x 1.0}} t '#{#{1} #{1.0}}] [(count s) (count t) (contains? s {:x 1}) (contains? t #{1})])"),
    ('runtime-constructors', '(let [m (hash-map 1 :int 1.0 :float) s (set [1 1.0])] [(count m) (get m 1) (count s) (vec (seq s))])'),
    ('runtime-small-constructors', '[(count (array-map 1 :int 1.0 :float)) (get (array-map 1 :int 1.0 :float) 1) (count (hash-set 1 1.0))]'),
    ('dynamic-map', '(let [a 1 b 1.0 m {a :int b :float}] [(count m) (get m a) (get m b) (vec (seq m))])'),
    ('dynamic-set', '(let [a 1 b 1.0] (try [(count #{a b})] (catch :default e [:duplicate])))'),
    ('set-traversal-gap', "(let [s '#{1 1.0 '2 2}] [(count s) (contains? s 2) (contains? s '(quote 2)) (vec (seq s))])"),
    ('collision-and-nan-count', '(let [m {1 :int 1.0 :float ##NaN :nan} s #{1 1.0 ##NaN}] [(count m) (get m 1) (count s) (contains? s ##NaN)])'),
    ('map-nine-reverse', '(let [m {1.0 :float 1 :int :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 1) (get m 1.0)])'),
    ('map-ten-categories', '(let [m {1 :int 1M :decimal 1.0 :float :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 1)])'),
    ('map-nine-wide-integers', '(let [m {9007199254740992 :first 9007199254740993 :second :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 9007199254740992) (get m 9007199254740993)])'),
    ('map-nine-decimal-precision', '(let [m {1.0000000000000001M :precise 1M :rounded :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 1)])'),
    ('map-nine-zero-categories', '(let [m {0 :int -0.0 :float :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m 0) (get m -0.0)])'),
    ('metadata-wide-map', '(count ^:x {1000000000000000000000 1 1000000000000000000001 2})'),
    ('metadata-wide-set', '(count ^:x #{1000000000000000000000 1000000000000000000001})'),
)
NEGATIVES = (
    ('map-integer-hex', '{1 :first 0x1 :second}', 'Duplicate key', 'duplicate map key'),
    ('set-integer-hex', '#{1 0x1}', 'Duplicate key', 'duplicate set member'),
    ('map-integer-octal', '{1 :first 01 :second}', 'Duplicate key', 'duplicate map key'),
    ('set-integer-octal', '#{1 01}', 'Duplicate key', 'duplicate set member'),
    ('map-integer-bigint', '{1 :first 1N :second}', 'Duplicate key', 'duplicate map key'),
    ('set-integer-bigint', '#{1 1N}', 'Duplicate key', 'duplicate set member'),
    ('map-integer-ratio', '{1 :first 2/2 :second}', 'Duplicate key', 'duplicate map key'),
    ('set-integer-ratio', '#{1 2/2}', 'Duplicate key', 'duplicate set member'),
    ('map-float-exponent', '{1.0 :first 1e0 :second}', 'Duplicate key', 'duplicate map key'),
    ('set-float-exponent', '#{1.0 1e0}', 'Duplicate key', 'duplicate set member'),
    ('map-decimal-scale', '{1M :first 1.0M :second}', 'Duplicate key', 'duplicate map key'),
    ('set-decimal-scale', '#{1M 1.0M}', 'Duplicate key', 'duplicate set member'),
    ('map-signed-zero', '{-0.0 :first 0.0 :second}', 'Duplicate key', 'duplicate map key'),
    ('set-signed-zero', '#{-0.0 0.0}', 'Duplicate key', 'duplicate set member'),
    ('map-infinity', '{##Inf :first ##Inf :second}', 'Duplicate key', 'duplicate map key'),
    ('set-infinity', '#{##Inf ##Inf}', 'Duplicate key', 'duplicate set member'),
    ('map-rounded-floats', '{9007199254740992.0 :first 9007199254740993.0 :second}', 'Duplicate key', 'duplicate map key'),
    ('set-rounded-floats', '#{9007199254740992.0 9007199254740993.0}', 'Duplicate key', 'duplicate set member'),
    ('map-ratio-reduced', '{1/2 :first 2/4 :second}', 'Duplicate key', 'number is not a valid ClojureScript constant'),
    ('set-ratio-reduced', '#{1/2 2/4}', 'Duplicate key', 'number is not a valid ClojureScript constant'),
    ('map-nested', '{:x {[1] :first [0x1] :second}}', 'Duplicate key', 'duplicate map key'),
    ('set-quoted-nested', "'[#{1 1N}]", 'Duplicate key', 'duplicate set member'),
    ('map-odd', '{1 :int 1.0}', 'even number', 'even number'),
    ('map-unclosed', '{1 :int 1.0 :float', 'EOF', 'expected'),
    ('set-unclosed', '#{1 1.0', 'EOF', 'expected'),
    ('nonintegral-ratio', '[(count {1/2 :ratio 0.5 :float}) (get {1/2 :ratio 0.5 :float} 0.5) (count #{1/2 0.5}) (vec (seq #{1/2 0.5}))]', 'not a valid ClojureScript constant', 'number is not a valid ClojureScript constant'),
)
RECOVERY='(defmacro literal-recovery [] (+ 20 22)) (literal-recovery)'


def specs():
    rows=[]
    for name,source in CONTROLS:
        identity='literal-numeric-'+name
        empty={'id':identity,'outcome':{'status':'returned','value':None},'trace':[],'stdout':''}
        rows.append({'id':identity,'source':source,'source_sha256':sci.digest(source),'original_expected':empty,'canonical_expected':None})
    negatives=[]
    for name,source,pin,local in NEGATIVES:
        identity='literal-rejection-'+name
        negatives.append({'id':identity,'source':source,'source_sha256':sci.digest(source),'original_expected':{'id':identity,'outcome':{'status':'returned','value':None},'trace':[],'stdout':''},'canonical_expected':None,'pin_diagnostic':pin,'local_diagnostic':local})
    return rows,negatives


def load():
    raw=CONTRACTS.read_bytes()
    proof.same(sci.digest(raw),CONTRACTS_SHA256,'Unreviewed independently derived numeric literal contract')
    frozen=json.loads(raw);rows,negative=specs()
    proof.same(frozen['sources'],[{k:r[k] for k in ('id','source','source_sha256')} for r in rows+negative],'Numeric literal source drift')
    proof.same([len(rows),len(negative)],[42,26],'Missing complete numeric source catalog')
    proof.same(len({r['id'] for r in rows+negative}),len(rows+negative),'Duplicate numeric source identity')
    proof.same(len(frozen['canonical_expected']),len(rows),'Missing reviewed numeric literal')
    for row,expected in zip(rows,frozen['canonical_expected']):
        proof.same(expected['id'],row['id'],'Wrong numeric literal identity')
        proof.same(set(expected),{'id','outcome','trace','stdout'},'Incomplete numeric typed packet')
        proof.same([expected['trace'],expected['stdout']],[[],''],'Wrong exact numeric effects/stdout')
        row['canonical_expected']=expected
        row['original_expected']=frozen.get('original_expected',{}).get(row['id'],copy.deepcopy(expected))
        row['historical_expectation_basis']=frozen.get('historical_expectation_basis',{}).get(row['id'])
        row['checkout_expected']=frozen['checkout_expected'].get(row['id'],copy.deepcopy(expected))
        row['two_pin_agreement']=expected==row['checkout_expected']
        row['canonical_acceptance']=row['id'] not in frozen['nonpassing_neighbors']
        row['nonpassing_reason']=frozen['nonpassing_neighbors'].get(row['id'])
        row['local_expected']=frozen['local_expected'].get(row['id'],copy.deepcopy(expected))
    proof.same(rows[0]['source'],ORIGINAL,'Original numeric literal source drift')
    keyword={'$keyword':{'namespace':None,'name':'int','spelling':'int'}}
    proof.same(rows[0]['canonical_expected']['outcome']['value'],{'$vector':[2,keyword,2,{'$vector':[1,1]}]},'Original numeric literal result drift')
    proof.same([sum(r['canonical_acceptance'] for r in rows),sum(not r['two_pin_agreement'] for r in rows)],[37,2],'Missing numeric accepted/gap coverage')
    for row in negative:
        row['canonical_diagnostic_agreement']=row['id'] not in frozen['diagnostic_gaps']
        row['nonpassing_reason']=frozen['diagnostic_gaps'].get(row['id'])
    return rows,negative


def snapshot():
    paths=[Path(__file__),DRIVER,CONTRACTS,Path(__file__).with_name('test_literal_numeric.py')]
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


def diagnostic_matches(wanted,error):
    return wanted in error or (wanted=='duplicate map key' and 'DuplicateMapKey' in error) or (wanted=='number is not a valid ClojureScript constant' and 'NumericOverflow' in error)


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
            if not diagnostic_matches(row['local_diagnostic'],error):raise ValueError('Wrong actual local literal diagnostic: '+row['id']+': '+error)
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
            frames[row['id']][variant]={'status':'rejected','detail':error,'phase_source_sha256':sci.digest(source),'canonical_diagnostic_agreement_credit':1 if row['canonical_diagnostic_agreement'] else 0,'nonpassing_reason':row['nonpassing_reason'],'same_host_actual_macro_recovery':{'source':RECOVERY,'compiled_js_sha256':sci.digest(js),'QuickJS':42,'Node':42}}
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
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'source_bindings':rows,'diagnostic_sources':negative,'execution_binding':binding,'compiler_build_producer_source_sha256':before,'references':refs,'frames':frames,'diagnostics':diagnostics,'canonical_contract_status':'passed','actual_macro_contract_status':'passed','status':'passed','accepted_source_count':sum(r['canonical_acceptance'] for r in rows),'accepted_engine_frame_count':sum(r['canonical_acceptance'] for r in rows)*8,'nonpassing_neighbor_count':sum(not r['canonical_acceptance'] for r in rows),'nonpassing_diagnostic_count':sum(not r['canonical_diagnostic_agreement'] for r in negative),'original_semantic_pass_credit':0,'full_language_completion_credit':0}


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
