#!/usr/bin/env python3
"""Proposal only: complete original integer source, with independent raw replay.

The sole original site executes112times. No installed witness/table is modified.
Existing arity witnesses and Closure construction/loading boundaries are intact.
"""
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
import argparse, json, os, shutil, subprocess, tempfile
import integer_predicate_after192 as actual
import integer_predicate_after192_proof as accepted
import original_integer_predicate_after190 as upstream
import report

ROOT=actual.ROOT
ID='cljs-whole-original-integer-source-after194'
PROFILE='complete-original-integer-initialized-genuine-closure-after194-v1'
SOURCE='src/test/cljs/cljs/predicates_test.cljs'
BODY=SOURCE+'::test-int-preds'
SITE=BODY+'::assertion-001'
BODY_HASH='4effa47d1bb9402702f5b48342dba0c244a6846e7a5590d09b276accdc0f97ee'
TABLE_HASH='ef0594f249eb5094f5641283824eb6a6bfb3fe66c9dec755f4240acd06071479'
SITE_HASH='bbb0a27c9a70e334ec9664f906bb3d2e0f93af79438e88735747b4b8ef0e6f44'
COUNTERS={'test':1,'pass':112,'fail':0,'error':0}

def same(left,right,message):
    if left!=right:raise ValueError(message)


def binding():
    original=actual.original()
    same(original['forms']['int-val-table']['sha256'],TABLE_HASH,'Complete original14-row table drift')
    same(original['forms']['test-int-preds']['sha256'],BODY_HASH,'Complete original loop/body drift')
    inventories=[json.loads(line) for line in (ROOT/'tests/reference/compiler-language/full-inventory.jsonl').read_text().splitlines()]
    sites=[row for row in inventories if row['body_id']==BODY]
    same(len(sites),1,'Original source-site census changed')
    site=sites[0]
    same((site['id'],site['body_sha256'],site['assertion']['sha256'],site['disposition']),
         (SITE,BODY_HASH,SITE_HASH,'in-scope'),'Original assertion identity/hash/scope drift')
    return {'module_source':SOURCE,'module_sha256':original['module_sha256'],
      'complete_original_provenance':[{'body_id':BODY,'source_sha256':BODY_HASH}],
      'complete_original_setup':original['forms']['int-val-table'],
      'complete_original_body':original['forms']['test-int-preds'],
      'original_assertion':site,'original_table_rows':14,'predicates_per_row':8,
      'executed_original_comparisons':112,'source_profile':PROFILE,
      'adaptation':['Explicit initialized byte-audited genuine Closure dependency graph in each application/MacroHost context.',
        'External LongImpl binding resolves the genuine goog.math.Long module; this supplies no public constructor/ambient loader credit.',
        'External deftest/is collector leaves both complete original forms byte-identical, evaluates the original assertion message, and retains every112Boolean assertion record.',
        'Canonical pins execute the entire unchanged original namespace under real cljs.test; exact counters and all source/build/raw artifacts remain retained.'],
      'proposed_original_assertion_witnesses':[{'assertion_id':SITE,'assertion_sha256':SITE_HASH,'fixture_id':ID,
        'rationale':'Complete unchanged int-val-table setup and test-int-preds loop execute all14rows/eight predicates under an explicit initialized genuine Closure embedding; both default pins execute the unchanged whole namespace. Every original comparison and assertion message executes; source corpus/program/expectations remain unchanged.'}],
      'prospective_original_assertion_credit':1,'installed_source_credit':0,'new_public_or_local_arity_links':0,
      'strict_complete':False}


def snapshot():
    paths=set(subprocess.check_output(['git','ls-files','src','build.rs','Cargo.toml','Cargo.lock'],cwd=ROOT,text=True).splitlines())
    paths.update([str(Path(__file__).relative_to(ROOT)),'tests/reference/compiler-language/test_integer_original_source_after194.py',
      'tests/reference/compiler-language/integer-original-source-after194.md',actual.DRIVER,actual.MODULE,
      'tests/reference/compiler-language/integer_predicate_after192.py',
      'tests/reference/compiler-language/integer_predicate_after192_proof.py',
      'tests/reference/compiler-language/original_integer_predicate_after190.py',
      'tests/reference/compiler-language/original_property_source_probe.py',
      'tests/reference/compiler-language/hosted_import_source_context.py',
      'tests/reference/compiler-language/extract_inventory.py','tests/reference/compiler-language/report.py',
      'tests/reference/compiler-language/full-inventory.jsonl','tests/reference/compiler-language/full-bodies.jsonl',
      'tests/reference/execution.clj','tests/support/sci_execution_observation.js'])
    return {name:actual.digest((ROOT/name).read_bytes()) for name in sorted(paths)}


def programs():
    names={name:str(Path(shutil.which(os.environ.get(name.upper(),name)) or '').resolve()) for name in ('node','clojure')}
    if any(not Path(path).is_file() for path in names.values()):raise ValueError('Required real Node/Clojure executable unavailable')
    return {'programs':names,'executable_sha256':{path:actual.digest(Path(path).read_bytes()) for path in names.values()}}


def run(command,*,env=None):
    result=subprocess.run(command,cwd=ROOT,env=env,capture_output=True,text=True,timeout=600)
    if result.returncode:raise ValueError('Independent original-source command failed: '+repr(command)+'\n'+result.stderr[-4000:])
    return result


def expected_local_packet():
    # Independently enumerated literal complete result/message contract. Function
    # printer spellings are a local observation contract, not canonical parity.
    printers=['identity','isNegative','isPositive','isInteger','isInt','isNegativeInt','isPositiveInt','isNaturalInt']
    receivers=['0','-0.0','1','-1','1','-1','100000000000','-100000000000',
               '#object[Long 0]','#object[Long 100000000000]','#object[Long -100000000000]',
               '#object[Object 0]','#object[Object 100000000000]','#object[Object -100000000000]']
    checks=[accepted.vector([i,True,'(#object['+name+'] '+receiver+')']) for receiver in receivers for i,name in enumerate(printers)]
    return {'id':'integer-predicate-after192','outcome':{'status':'returned','value':accepted.vector([
      accepted.vector(accepted.EXPECTED_CONTROLS),accepted.vector(checks),accepted.EXPECTED_FOCUSED])},'trace':[],'stdout':''}


def replay_actual(directory,receipt):
    binary=receipt['actual_executable']
    built=run(['cargo','test','--locked','--test','integer_predicate_after192','--no-run','--message-format=json'])
    artifacts=[json.loads(line) for line in built.stdout.splitlines() if line.startswith('{')]
    current=[row['executable'] for row in artifacts if row.get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='integer_predicate_after192' and row.get('executable')]
    same(current,[binary],'Retained original-source executable is not current Cargo build')
    same(actual.digest(Path(binary).read_bytes()),receipt['actual_executable_sha256'],'Original-source executable bytes changed')
    packets={}
    with tempfile.TemporaryDirectory(prefix='integer-original-after194-actual-replay-') as temporary:
        fresh=Path(temporary)/'actual';actual.prepare(fresh)
        output=run([binary,actual.TEST,'--ignored','--exact','--nocapture'],env={**os.environ,'PYTHONDONTWRITEBYTECODE':'1','LIGHT_INTEGER_PREDICATE_AFTER192':str(fresh)})
        if output.stdout.count('test result: ok. 1 passed; 0 failed; 0 ignored;')!=1:raise ValueError('Complete original actual replay did not execute exactly once')
        retained={name:digest for name,digest in receipt['actual_artifact_sha256'].items() if name!='snapshot-before.json'}
        rebuilt={str(p.relative_to(fresh)):actual.digest(p.read_bytes()) for p in sorted(fresh.rglob('*')) if p.is_file() and p.name!='snapshot-before.json'}
        same(rebuilt,retained,'Independent actual original source/emitted/raw/input artifact replay differs')
        for host in ('fresh','reused'):
            for phase in ('application','macro'):
                folder=directory/host/phase
                for engine,name in [('QuickJS','quickjs.stdout'),('Node','node.stdout')]:
                    packet=json.loads((folder/name).read_text())
                    same(packet,expected_local_packet(),'Complete literal original112outcomes/messages/effects/stdout differs')
                    if engine=='Node':
                        result=subprocess.run([programs()['programs']['node'],'-'],cwd=ROOT,input=(folder/'node.stdin').read_text(),capture_output=True,text=True,timeout=60)
                        same(result.returncode,0,'Actual Node input executable replay failed')
                        same(result.stdout,(folder/name).read_text(),'Retained Node raw differs from input execution')
                        same(result.stderr,'','Unexpected actual Node stderr')
                    packets[host+'/'+phase+'/'+engine]=packet
    return packets


def pin_replay(references):
    proof={}
    # cljs.test embeds :output-dir in js_filename. Keep the validated original
    # source AND output locations, temporarily preserving every original emitted
    # byte outside the build. A missing output tree forces fresh compilation.
    # No emitted/source/output normalization or cache acceptance is permitted.
    for kind in ('release','checkout'):
        original=references/kind
        def emitted(root):return {str(p.relative_to(root)):actual.digest(p.read_bytes()) for p in sorted(root.rglob('*.js')) if p.is_file()}
        before=emitted(original);reference_js=(original/'reference.js').read_bytes()
        command=json.loads((original/'build/invocation.json').read_text());command[0]=programs()['programs']['clojure']
        node=json.loads((original/'node/invocation.json').read_text());node[0]=programs()['programs']['node']
        retained=run(node)
        same(retained.stdout,(original/'node/stdout').read_text(),'Retained original pin raw differs from executable replay')
        same(retained.stderr,(original/'node/stderr').read_text(),'Retained original pin stderr differs from executable replay')
        with tempfile.TemporaryDirectory(prefix='integer-original-after194-preserved-emitted-') as temporary:
            saved=Path(temporary)/'out';shutil.move(original/'out',saved)
            try:
                built=run(command)
                identity=kind+' '+(report.RELEASE if kind=='release' else report.REVISION)
                if 'REFERENCE IDENTITY '+identity not in built.stdout or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in built.stdout:raise ValueError('Fresh independent pin identity/default settings absent')
                same(emitted(original),before,'Original pin complete independently emitted JS units differ')
                fresh=run(node)
                same(fresh.stdout,retained.stdout,'Independent pin full raw assertion results differ')
                same(fresh.stderr,retained.stderr,'Independent pin stderr differs')
                same(fresh.stdout,'ORIGINAL INTEGER COUNTERS '+json.dumps(COUNTERS,separators=(',',':'))+'\n','Complete original pin exact literal counters/stdout differ')
                proof[kind]={'identity':identity,'settings':{'optimizations':'none','static-fns':False,'def-emits-var':False},
                  'compiler_command':command,'emitted_units_sha256':before,'stdout':fresh.stdout,'stderr':fresh.stderr,'assertion_counters':COUNTERS,
                  'independent_compile_profile':'Preserved original source/output locations; original emitted tree moved aside, cache-analysis false; all fresh JS units exact, pristine original tree restored.'}
            finally:
                if (original/'out').exists():shutil.rmtree(original/'out')
                shutil.move(saved,original/'out');(original/'reference.js').write_bytes(reference_js)
        same(emitted(original),before,'Pristine full original emitted units not restored')
    return proof


def validate(root):
    root=Path(root).resolve();before=snapshot();execution=programs();source=binding()
    stored=json.loads((root/'binding-before.json').read_text());same(stored,{'source_snapshot':before,'execution_binding':execution,'source_binding':source},'Original-source frozen producer/source/executable binding drift')
    receipt=accepted.validate(root/'actual',root/'actual.log',root/'pins')
    packets=replay_actual(root/'actual',receipt);pins=pin_replay(root/'pins')
    same(snapshot(),before,'Original-source current source changed during replay')
    same(programs(),execution,'Original-source executable changed during replay')
    return {'schema_version':1,'profile':PROFILE,'status':'passed-proposal-only','artifact_root':str(root),
      'source_binding':source,'source_snapshot':before,'execution_binding':execution,
      'complete192_acceptance':receipt,'actual_complete_literal_packets':packets,'independent_pin_replay':pins,
      'prospective_original_assertion_credit':1,'installed_original_assertion_credit':0,
      'existing_public_local_arity_witnesses':'corroborated only; no new/replacement links','strict_complete':False}


def execute(root):
    root=Path(root).resolve();root.mkdir(parents=True,exist_ok=False)
    before={'source_snapshot':snapshot(),'execution_binding':programs(),'source_binding':binding()}
    (root/'binding-before.json').write_text(json.dumps(before,indent=2)+'\n')
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures=[pool.submit(upstream.run,kind,root/'pins'/kind) for kind in ('release','checkout')]
        for future in futures:future.result()
    actual.prepare(root/'actual')
    result=run(['cargo','test','--locked','--test','integer_predicate_after192',actual.TEST,'--message-format=json','--','--ignored','--exact','--nocapture'],env={**os.environ,'PYTHONDONTWRITEBYTECODE':'1','LIGHT_INTEGER_PREDICATE_AFTER192':str(root/'actual')})
    (root/'actual.log').write_text(result.stdout+result.stderr)
    receipt=validate(root);(root/'evidence.json').write_text(json.dumps(receipt,indent=2)+'\n');return receipt


def controls(root):
    root=Path(root);pristine=validate(root);results=[]
    paths=[('missing-engine-frame',root/'actual.log',lambda b:b'\n'.join(line for line in b.split(b'\n') if not line.startswith(b'integer-after192-observation\tfresh\tapplication\tQuickJS'))),
      ('changed-original-message',root/'actual/fresh/application/quickjs.stdout',lambda b:b.replace(b'#object[identity]',b'#object[changed!]',1)),
      ('changed-literal-outcome',root/'actual/fresh/application/quickjs.stdout',lambda b:b.replace(b'[0,true,',b'[0,false,',1)),
      ('changed-complete-emitted-js',root/'actual/fresh/application/compiled.js',lambda b:b+b' '),
      ('changed-complete-source',root/'actual/fresh/application/integer_predicate_after192.cljs',lambda b:b.replace(b'[natl',b'[nil ',1)),
      ('changed-indirect-QuickJS-input',root/'actual/fresh/application/quickjs.stdin',lambda b:b+b' '),
      ('changed-real-library-loader',root/'actual/genuine-loader.js',lambda b:b+b' ')]
    for kind in ('release','checkout'):
        paths.extend([(kind+'-changed-whole-original-namespace',root/'pins'/kind/'source/cljs/predicates_test.cljs',lambda b:b+b' '),
          (kind+'-changed-pin-emitted-unit',root/'pins'/kind/'out/cljs/predicates_test.js',lambda b:b+b' '),
          (kind+'-changed-full-counters',root/'pins'/kind/'node/stdout',lambda b:b.replace(b'"pass":112',b'"pass":111',1))])
    for identity,path,mutate in paths:
        original=path.read_bytes()
        try:
            changed=mutate(original)
            if changed==original:raise ValueError('Ineffective guard mutation '+identity)
            path.write_bytes(changed)
            try:validate(root)
            except (ValueError,KeyError,FileNotFoundError,json.JSONDecodeError) as error:results.append({'id':identity,'status':'rejected','reason':str(error)})
            else:raise AssertionError('Original-source raw mutation accepted '+identity)
        finally:path.write_bytes(original)
    same(validate(root),pristine,'Pristine original-source restored replay differs')
    return {'schema_version':1,'controls':results,'restored_pristine_replay':'passed','installed_original_credit':0,'strict_complete':False}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--execute',type=Path);parser.add_argument('--validate',type=Path);parser.add_argument('--controls',action='store_true');args=parser.parse_args()
    if bool(args.execute)==bool(args.validate):parser.error('Exactly one execution/validation root required')
    root=args.execute or args.validate;result=execute(root) if args.execute else validate(root)
    if args.controls:(root/'raw-controls.json').write_text(json.dumps(controls(root),indent=2)+'\n')
    print(result['status'],flush=True)
