#!/usr/bin/env python3
"""Exact upstream invocation-emission program scope; no semantic pass substitution."""
from __future__ import annotations
import argparse
import ast
import json
from pathlib import Path
import re
import subprocess
import os
import zipfile
import extract_inventory as inv
from arity_resume_candidates import digest
ROOT = Path(__file__).resolve().parents[3]
REVISION = 'a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e'
RELEASE_TAG = 'c4295f303100bbf5afac449242d30bca1126f1a1'
MODULE = 'src/test/cljs/cljs/invoke_test.cljs'
MODULE_SHA256 = '5238e8708c0a1a631647aeef9674f9218c57b769b2dd194f38b17a5f777bc17e'
CALLER = 'src/test/clojure/cljs/compiler_tests.clj'
CALLER_TEST = 'test-optimized-invoke-emit'
CALLER_SHA256 = '63ab500527def1c1556a40e021833419058de07591e2ebb65fb8b51df7832a7c'
GROUP_SHA256 = 'f3e25d6a72914bb406675ce959d086d5112288d036622c224d1fd38b1c2f151a'
REASON = ('The unchanged complete module is the exact source fixture consumed by '
          'the original JVM test-optimized-invoke-emit with :static-fns true. '
          'All eleven original predicates inspect generated JS spelling, optimized '
          'dispatch slots, temporary bindings or direct record field access. '
          'The module deliberately invokes an uninitialized declared-fn before '
          'its record tail; it has no exported callback or public runtime test '
          'harness. Its three higher-order functions are emission-shape subjects '
          'and are not executed by the original program. Generated JS spelling '
          'and optimizer shape are outside the adopted compiler API goals. '
          'Every independent public call/IFn/record/declare/keyword obligation '
          'remains intended; no semantic or phase pass is supplied here.')


def pinned(path, revision=REVISION):
    return subprocess.check_output(['git', '-C', str(inv.UPSTREAM), 'show', revision+':'+path])


def validate():
    if inv.REVISION != REVISION:
        raise ValueError('invocation source scope pin drift')
    raw = pinned(MODULE)
    if raw != (inv.UPSTREAM/MODULE).read_bytes() or digest(raw) != MODULE_SHA256:
        raise ValueError('complete original invocation module drift')
    caller_raw = pinned(CALLER)
    if caller_raw != (inv.UPSTREAM/CALLER).read_bytes():
        raise ValueError('original compiler caller working source drift')
    source = inv.Source(CALLER, caller_raw)
    bodies = [node for node in source.forms if len(node.children)>1 and inv.short_head(node)=='deftest' and node.children[1].token==CALLER_TEST]
    if len(bodies)!=1 or digest(source.text[bodies[0].start:bodies[0].end])!=CALLER_SHA256:
        raise ValueError('complete original invocation optimizer caller drift')
    if inv.INTERNAL_TESTS['compiler_tests.clj'].get(CALLER_TEST) != 'Exact optimized JavaScript code shape/dispatch names and temporary bindings.':
        raise ValueError('existing exact optimizer scope boundary drift')
    forms = inv.Reader(raw.decode()).all()
    if len(forms)!=16 or inv.short_head(forms[0])!='ns' or inv.short_head(forms[11])!='declare' or inv.short_head(forms[12])!='declared-fn':
        raise ValueError('complete original declared call/program order drift')
    if not raw.decode().endswith('(:foo-field-a foo-record)\n'):
        raise ValueError('original record tail omitted')
    body = bodies[0]
    return [{'id': 'program::'+MODULE, 'obligation_kind': 'source-program', 'source': MODULE,
             'body_id': 'program::'+MODULE, 'body_sha256': MODULE_SHA256,
             'source_span': inv.Source(MODULE, raw).span(0,len(raw.decode())),
             'disposition':'unrelated','category':'upstream-internals','reason':REASON,
             'scope_basis':'User-adopted compiler-language-completion group186 revision2: generated JS spelling and optimizer shape are not compiler API goals.',
             'scope_basis_record':{'group':'compiler-language-completion','id':186,'revision':2,'context_sha256':GROUP_SHA256},
             'original_caller':CALLER,'original_caller_test':CALLER_TEST,'original_caller_body_sha256':CALLER_SHA256,
             'original_emission_assertions':11,'semantic_pass_credit':False,'local_execution_credit':False,'macro_phase_credit':False}]


def run(command, directory, *, cwd=ROOT, check=True, timeout=600, environment=None):
    directory.mkdir(parents=True,exist_ok=False)
    result=subprocess.run(command,cwd=cwd,text=True,capture_output=True,timeout=timeout,env=environment)
    (directory/'stdout').write_text(result.stdout);(directory/'stderr').write_text(result.stderr)
    (directory/'command.json').write_text(json.dumps(command))
    if check and result.returncode:
        raise RuntimeError('invoke audit command failed: '+str(directory))
    return result


def compiler_script():
    source=inv.Source(CALLER,pinned(CALLER)); body=next(node for node in source.forms if len(node.children)>1 and inv.short_head(node)=='deftest' and node.children[1].token==CALLER_TEST)
    original=source.text[body.start:body.end]
    prefix=(ROOT/'tests/reference/execution.clj').read_text().split('(def expected-settings',1)[0]
    return prefix+'''\n(require '[cljs.compiler.api :as comp-api] '[clojure.test :as test :refer [deftest is]])
'''+original+'''\n(try
  (let [[kind identity checkout output] *command-line-args*]
    (checked-reference kind identity checkout)
    (let [results (test/run-tests 'user)]
      (assert (= {:test 1 :pass 11 :fail 0 :error 0} (select-keys results [:test :pass :fail :error]))))
    (build/build "src/test/cljs" {:main 'cljs.invoke-test :target :nodejs
      :optimizations :none :static-fns true :nodejs-rt false :cache-analysis false
      :output-to (str (io/file output "reference.js"))
      :output-dir (str (io/file output "out")) :parallel-build false})
    (println "INVOKE ORIGINAL BUILD SETTINGS" (pr-str {:optimizations :none :static-fns true :nodejs-rt false}))
    (checked-reference kind identity checkout))
  (finally (shutdown-agents)))
'''


def reference(kind, directory):
    validate();directory=Path(directory).resolve();directory.mkdir(parents=True,exist_ok=False)
    if digest(pinned(MODULE,RELEASE_TAG))!=MODULE_SHA256:
        raise ValueError('release whole source fixture differs')
    source=inv.Source(CALLER,pinned(CALLER,RELEASE_TAG));body=next(node for node in source.forms if len(node.children)>1 and inv.short_head(node)=='deftest' and node.children[1].token==CALLER_TEST)
    if digest(source.text[body.start:body.end])!=CALLER_SHA256:
        raise ValueError('release original optimized caller differs')
    original=directory/MODULE;original.parent.mkdir(parents=True);original.write_bytes(pinned(MODULE))
    (directory/'target').mkdir(); script=compiler_script();script_path=directory/'audit.clj';script_path.write_text(script)
    dependency='{:mvn/version "1.12.134"}' if kind=='release' else '{:local/root '+json.dumps(str(inv.UPSTREAM))+'}'
    identity='1.12.134' if kind=='release' else REVISION
    build=run(['clojure','-Srepro','-Sdeps','{:paths [] :deps {org.clojure/clojurescript '+dependency+'}}','-M',str(script_path),kind,identity,'-' if kind=='release' else str(inv.UPSTREAM),str(directory)],directory/'build',cwd=directory)
    if build.stdout.count('REFERENCE IDENTITY '+kind+' '+identity)!=2 or '1 tests containing 11 assertions' not in build.stdout:
        raise ValueError('exact eleven original emission assertions did not execute')
    script='process.chdir('+json.dumps(str(directory))+');try {require('+json.dumps(str(directory/'out/goog/bootstrap/nodejs.js'))+');require('+json.dumps(str(directory/'out/cljs_deps.js'))+');goog.require("cljs.invoke_test");console.log("UNEXPECTED PROGRAM COMPLETION");} catch(e) {console.log(JSON.stringify({category:e.name,message:e.message}));}'
    runtime=run(['node','-e',script],directory/'runtime',cwd=directory)
    outcome=json.loads(runtime.stdout.strip())
    if outcome['category']!='TypeError' or 'declared_fn' not in outcome['message']:
        raise ValueError('unchanged original program did not stop at its undefined declared call')
    result={'reference_identity':kind+' '+identity,'settings':{'optimizations':'none','static-fns':True,'nodejs-rt':False},'whole_original_source_sha256':MODULE_SHA256,'original_caller_sha256':CALLER_SHA256,'original_emission_assertions':11,'original_emission_tests':1,'runtime_expected_failure':outcome,'build_stdout_sha256':digest(build.stdout),'emitted_fixture_sha256':digest((directory/'target/invoke_test.js').read_bytes()),'reference_js_sha256':digest((directory/'reference.js').read_bytes()),'script_sha256':digest(compiler_script()),'runtime_raw_sha256':digest(runtime.stdout)}
    (directory/'receipt.json').write_text(json.dumps(result,indent=2)+'\n');print(kind,'11 exact emission assertions passed; original program TypeError at declared_fn',flush=True);return result



def closure_bundle(goog):
    graph={}
    for line in (goog/'deps.js').read_text().splitlines():
        match=re.fullmatch(r"goog.addDependency\('([^']+)', (\[[^\]]*\]), (\[[^\]]*\]), (.*)\);",line)
        if match:
            path,provides,requires,flags=match.groups()
            for name in ast.literal_eval(provides):
                graph[name]=(path,ast.literal_eval(requires))
    ordered=[];seen=set();active=set()
    def visit(name):
        path,requires=graph[name]
        if path in active:raise ValueError('real string dependency cycle')
        if path in seen:return
        active.add(path)
        for dependency in requires:visit(dependency)
        active.remove(path);seen.add(path);ordered.append(path)
    visit('goog.string')
    if len(ordered)!=34 or ordered[-1]!='string/string.js':
        raise ValueError('complete genuine string dependency closure drift')
    paths=['base.js','deps.js',*ordered]
    hashes={path:digest((goog/path).read_bytes()) for path in paths}
    matches=[]
    for archive in sorted((Path.home()/'.m2/repository/org/clojure/google-closure-library').rglob('*.jar')):
        with zipfile.ZipFile(archive) as jar:
            if digest(jar.read('goog/base.js'))==hashes['base.js'] and digest(jar.read('goog/deps.js'))==hashes['deps.js']:
                matches.append({'archive':str(archive),'archive_sha256':digest(archive.read_bytes()),'original_dependency_source_sha256':{path:digest(jar.read('goog/'+path)) for path in paths}})
    if len(matches)!=1:raise ValueError('ambiguous or absent genuine string library origin')
    bundle='globalThis.CLOSURE_NO_DEPS=true;\n(0,eval)('+json.dumps((goog/'base.js').read_text())+');\n'
    for path in ordered:bundle+='(0,eval)('+json.dumps((goog/path).read_text())+');\n'
    return '(function(){'+bundle+'return true;})()',hashes,matches[0]


def finish(references, output):
    rows=validate();references=Path(references).resolve();output=Path(output).resolve();output.mkdir(parents=True,exist_ok=False)
    profiles={kind:closure_bundle(references/kind/'out/goog') for kind in ['release','checkout']}
    if profiles['release']!=profiles['checkout']:
        raise ValueError('real string dependency profiles differ between pins')
    pins={kind:json.loads((references/kind/'receipt.json').read_text()) for kind in ['release','checkout']}
    for kind in pins:
        if (references/kind/'audit.clj').read_text()!=compiler_script() or pins[kind]['script_sha256']!=digest(compiler_script()):
            raise ValueError('exact original reference compiler driver drift')
        if digest((references/kind/MODULE).read_bytes())!=MODULE_SHA256 or pins[kind]['original_emission_assertions']!=11 or pins[kind]['runtime_expected_failure']['category']!='TypeError':
            raise ValueError('whole original reference evidence incomplete')
    loader=output/'genuine-string-loader.js';loader.write_text(profiles['release'][0])
    driver=ROOT/'tests/compiler_invoke_program_scope.rs'
    paths=[p for p in (ROOT/'src').rglob('*') if p.is_file()]
    paths += [ROOT/path for path in ['Cargo.toml','Cargo.lock','build.rs','tests/reference/execution.clj','tests/reference/compiler-language/extract_inventory.py','tests/reference/compiler-language/arity_resume_candidates.py']]
    paths=[path for path in paths if path.is_file()]
    paths += [Path(__file__).resolve(),driver,ROOT/'tests/reference/compiler-language/test_invoke_program_scope.py',inv.UPSTREAM/MODULE,inv.UPSTREAM/CALLER]
    before={str(path.relative_to(ROOT)):digest(path.read_bytes()) for path in sorted(set(paths))}
    build=run(['cargo','test','--locked','--test','compiler_invoke_program_scope','--no-run','--message-format=json'],output/'cargo')
    binaries=[row['executable'] for row in map(json.loads,build.stdout.splitlines()) if row.get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='compiler_invoke_program_scope' and row.get('executable')]
    if len(binaries)!=1:raise ValueError('missing actual audit executable')
    binary=Path(binaries[0]);binary_hash=digest(binary.read_bytes())
    env={**os.environ,'LIGHT_INVOKE_SCOPE_LOADER':str(loader),'LIGHT_INVOKE_SCOPE_ARTIFACTS':str(output/'local')}
    audit=run([str(binary),'unchanged_invoke_fixture_preserves_its_original_declared_call_failure','--ignored','--exact','--nocapture'],output/'audit',environment=env)
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in audit.stdout:
        raise ValueError('exact whole original local audit not executed')
    frames=[]
    for line in audit.stdout.splitlines():
        if line.startswith('invoke-program-observation\t'):
            prefix,lifecycle,engine,raw=line.split('\t');observation=json.loads(bytes.fromhex(raw).decode())
            if observation['category']!='TypeError':raise ValueError('whole original local runtime result changed')
            frames.append({'lifecycle':lifecycle,'engine':engine,'expected_original_failure':observation})
    if [(f['lifecycle'],f['engine']) for f in frames]!=[(h,e) for h in ['fresh','reused'] for e in ['QuickJS','Node']]:
        raise ValueError('missing/extra/reordered local whole program observations')
    for lifecycle in ['fresh','reused']:
        if digest((output/'local'/lifecycle/'cljs/invoke_test.cljs').read_bytes())!=MODULE_SHA256:
            raise ValueError('local original whole source shortened')
        compiled=(output/'local'/lifecycle/'compiled.js').read_text()
        if '(g6).call(null, 1, 2);' not in compiled or 'rt.makeType(["foo-field-a", "foo-field-b"]' not in compiled or compiled.index('(g6).call(null, 1, 2);')>compiled.index('rt.makeType(["foo-field-a", "foo-field-b"]'):
            raise ValueError('original undefined declared call or record tail disappeared')
    after={str(path.relative_to(ROOT)):digest(path.read_bytes()) for path in sorted(set(paths))}
    if before!=after or digest(binary.read_bytes())!=binary_hash:raise ValueError('actual producer source/binary drift')
    validate()
    result={'schema_version':1,'profile':'exact-upstream-invocation-emission-program-v1','head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT).decode().strip(),'scope_proposal':rows,'references':pins,'reference_artifacts':str(references),'local_expected_failure_observations':frames,'compiler_build_producer_source_sha256':before,'actual_audit_executable_sha256':binary_hash,'genuine_library_origin':profiles['release'][2],'genuine_compiled_dependency_sha256':profiles['release'][1],'loader_sha256':digest(loader.read_bytes()),'actual_module_sha256':{str(path.relative_to(output)):digest(path.read_bytes()) for path in sorted((output/'local').rglob('*')) if path.is_file()},'raw_audit_stdout_sha256':digest(audit.stdout),'semantic_pass_credit':0,'local_execution_credit':0,'macro_phase_credit':0,'production_defect_demonstrated':False}
    (output/'receipt.json').write_text(json.dumps(result,indent=2)+'\n')
    print('exact emission program scoped with 22 real compiler assertions and four original local expected failures; zero semantic/phase credit',flush=True)
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--reference',choices=['release','checkout']);parser.add_argument('--references',type=Path);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();reference(args.reference,args.output) if args.reference else finish(args.references,args.output)
