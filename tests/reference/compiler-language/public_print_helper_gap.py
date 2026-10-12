"""Exact public printing calls: checked pin agreement and local rejection, no credit."""
from pathlib import Path
import argparse
import json
import os
import tempfile
import sci_cljc_accepted_context as proof
import sci_execution as sci
import report

ROOT = sci.ROOT
CONTRACTS = Path(__file__).with_name('public-print-helper-gap-contracts.json')
DRIVER = ROOT/'tests/compiler_public_print_helper_gap.rs'
TEST = 'public_print_helper_rejection_and_actual_macro_recovery'
RECOVERY = '(defmacro print-recovery [] (+ 20 22)) (print-recovery)'


def load():
    cases = json.loads(CONTRACTS.read_text())
    proof.same([c['id'] for c in cases], ['public-pr-str-star', 'public-string-buffer-writer'], 'Changed complete public caller census')
    for case in cases:
        source = ROOT/'tests/fixtures/public-print-helper-gap'/(case['id']+'.cljs')
        proof.same(source.read_text(), case['source'], 'Public caller/source drift')
        proof.same(case['expected']['id'], case['id'], 'Public literal identity drift')
    return cases


def module(case, phase):
    body = case['source']
    return ('(ns user)\n(defmacro upstream [] (list \'quote (do '+body+')))\n(defn run [] (upstream))\nrun\n'
            if phase == 'macro' else '(ns user)\n(defn run [] (do '+body+'))\nrun\n')


def snapshot():
    paths = [Path(__file__), CONTRACTS, DRIVER, Path(report.__file__),
             ROOT/'tests/reference/execution.clj', ROOT/'tests/support/execution.js']
    paths += sorted((ROOT/'tests/fixtures/public-print-helper-gap').glob('*.cljs'))
    return {**proof.snapshot(), **{str(p.relative_to(ROOT)): sci.digest(p.read_bytes()) for p in paths}}


def local_frames(root, cases):
    records = [line.split('\t') for line in (root/'observations.tsv').read_text().splitlines()]
    proof.same([(r[0],r[1]) for r in records], [(c['id'],v) for c in cases for v in proof.VARIANTS], 'Missing/changed exact source contexts')
    result = []
    for case in cases:
        missing = 'pr-str*' if case['id']=='public-pr-str-star' else 'StringBufferWriter'
        for fields in [r for r in records if r[0]==case['id']]:
            proof.same(len(fields),7,'Malformed actual public rejection/recovery')
            identity, variant, source, error, unit, quickjs, node = fields
            source,error,unit = [bytes.fromhex(v).decode() for v in (source,error,unit)]
            folder = root/identity/variant
            proof.same(source,module(case,variant.split('-')[0]),'Reduced complete public caller')
            proof.same((folder/'user.cljs').read_text(),source,'Changed actual caller file')
            proof.same((folder/'rejection.txt').read_text(),error,'Changed actual rejection')
            if 'unresolved symbol: '+missing not in error:
                raise ValueError('Wrong public source rejection category')
            proof.same((folder/'recovery.cljs').read_text(),RECOVERY,'Changed same-host recovery')
            proof.same((folder/'recovery.js').read_text(),unit,'Changed actual recovery emitted unit')
            proof.same((folder/'node.stdin').read_text(),'console.log('+unit+');','Changed recovery Node execution input')
            proof.same((folder/'node.stdout').read_text().strip(),'42','Recovery Node failed')
            proof.same((folder/'node.stderr').read_text(),'','Unexpected recovery stderr')
            proof.same([quickjs,node],['42','42'],'Actual same-host engine recovery failed')
            result.append({'id':identity,'variant':variant,'status':'rejected','actual_source':source,
                           'source_sha256':sci.digest(source),'detail':error,
                           'same_host_macro_recovery':{'source':RECOVERY,'compiled_js_sha256':sci.digest(unit),'QuickJS':42,'Node':42}})
    return result


def run(directory, timeout=600):
    root = Path(directory).resolve(); root.mkdir(parents=True,exist_ok=False)
    before = snapshot(); cases=load(); programs=proof.executable_paths()
    hashes={p:sci.digest(Path(p).read_bytes()) for p in programs.values()}
    local=root/'local';local.mkdir()
    built=proof.command([programs['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],local/'build',timeout)
    binaries=[r['executable'] for r in map(json.loads,built.stdout.splitlines()) if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    proof.same(len(binaries),1,'Missing exact diagnostic executable')
    binary=binaries[0];hashes[binary]=sci.digest(Path(binary).read_bytes())
    env={'LIGHT_PUBLIC_PRINT_HELPER_ROOT':str(local),'NODE':programs['node']}
    audit=proof.command([binary,TEST,'--ignored','--exact','--nocapture'],local/'audit',timeout,environment={**os.environ,**env},record_environment=env)
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in audit.stdout:
        raise ValueError('Exact public rejection/recovery audit did not pass once')
    proof.same(audit.stderr,'','Unexpected diagnostic stderr')
    frames=local_frames(local,cases)
    # Independently rerun the actual hash-bound Rust audit; rehashed edited raw
    # packets or emitted recovery units cannot impersonate source computation.
    replay=Path(tempfile.mkdtemp(prefix='light-public-print-gap-replay-'))
    replay_env={'LIGHT_PUBLIC_PRINT_HELPER_ROOT':str(replay),'NODE':programs['node']}
    replay_audit=proof.command([binary,TEST,'--ignored','--exact','--nocapture'],root/'local-replay-command',timeout,environment={**os.environ,**replay_env},record_environment=replay_env)
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in replay_audit.stdout:
        raise ValueError('Actual public diagnostic replay failed')
    replay_frames=local_frames(replay,cases)
    for frame in replay_frames:
        prefix=str(replay.resolve())+'/'
        if not frame['detail'].startswith(prefix):
            raise ValueError('Replay diagnostic is not located in the exact source root')
        frame['detail']=str(local.resolve())+'/'+frame['detail'][len(prefix):]
    proof.same(replay_frames,frames,'Actual compiler/recovery replay differs from recorded frames')
    references={}
    for kind in ('release','checkout'):
        observed=report.reference_cases(cases,kind,ROOT/'research/clojurescript',root/kind,timeout)
        for case in cases:
            layer=observed[case['id']]
            proof.same(layer['status'],'passed','Public call lacks literal checked pin agreement')
            proof.same(layer['observed'],case['expected'],'Checked pin public packet drift')
            proof.same(layer['reference_identity'],kind+' '+(report.RELEASE if kind=='release' else report.REVISION),'Checked pin identity drift')
            proof.same(layer['settings'],{'optimizations':'none','static-fns':False,'def-emits-var':False},'Checked pin settings drift')
        references[kind]=observed
        print(kind+' exact complete public calls passed',flush=True)
    proof.same(before,snapshot(),'Compiler/source/producer changed during fresh diagnosis')
    for p,h in hashes.items():proof.same(sci.digest(Path(p).read_bytes()),h,'Diagnostic executable changed')
    result={'schema_version':1,'profile':'exact-public-print-helper-frontier-negative-v1','artifacts_root':str(root),
            'complete_cases':cases,'references':references,'actual_local_rejections':frames,
            'compiler_build_producer_source_sha256':before,'executables_sha256':hashes,
            'artifact_sha256':proof.tree(root),'replay_diagnostic_path_normalization':'Only the exact temporary source root prefix is replaced; file, line, column, byte ranges, phase and rejection text remain exact.',
            'semantic_pass_credit':0,'local_execution_credit':0,'macro_phase_credit':0,
            'scope':'Unmarked public canonical pr-str*, StringBufferWriter and ->StringBufferWriter remain intended independent arities. No private representation or optional ecosystem premise is used.',
            'public_gap':'Both checked pins execute the complete direct/qualified/saved/apply pr-str* callers and public writer IWriter dispatch; current application and demanded macro sources reject unresolved public names.'}
    (root/'evidence.json').write_text(json.dumps(result,indent=2)+'\n')
    print('Eight complete source rejections,16 actual same-host engine recovery observations; zero semantic credit',flush=True)
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args();run(args.output)
