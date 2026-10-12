"""One exact upstream optimizer-shape body; no public/phase/execution credit."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile
import extract_inventory as inv

ROOT = Path(__file__).resolve().parents[3]
REVISION = 'a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e'
RELEASE_TAG = 'c4295f303100bbf5afac449242d30bca1126f1a1'
TEST_PATH = 'src/test/cljs/cljs/inference_test.cljs'
MACRO_PATH = 'src/test/cljs/cljs/inference_util.clj'
SOURCE_SHA256 = {
    TEST_PATH: '59f26a8786c9c6144f442dd2846dd72b9199971b43bebf32b4b42c6126d51ea3',
    MACRO_PATH: '288ada0c0d6e9452a53d6ce8ba96131684b9d1b766f798af48191af824c5c4ba',
}
BODY_ID = 'src/test/cljs/cljs/inference_test.cljs::test-cljs-2825'
BODY_SHA256 = '1f5d427a5971afda26215c00fa46166ecccbcec700f3e725ad610799eb9fa8bb'
NAMED_SHA256 = {
    'test-cljs-2825': BODY_SHA256,
    'cljs-2866-test': 'd19b1c06e47d08746e78a4e61aaf7e2ffc84d554f970cd9fbaba9c0a20773305',
    'cljs-3068-test': '68730faa866b68996857a987b436a7d885e27ae5fd561032b6740b74267d3f1c',
    'truth_-called?': 'ecf101ca8db46d3c52c02592db444beb8d04acaa1632cb91b1e867adf2cc4edd',
    'truth_-not-called?': 'b6eeee4ee7855f13624915a430fca8b31268a5d1bd9a7652729acb475a627d56',
}
TRUTH_DECLARATION = '(defn truth_\n  "Internal - do not use!"\n  [x]\n  (cljs.core/truth_ x))'
TRUTH_SHA256 = 'c0d998211ec06329661fdd0cf395211227a10202c142fbd86bec5a36d0cc2e75'
GROUP_HASH = 'f3e25d6a72914bb406675ce959d086d5112288d036622c224d1fd38b1c2f151a'
REASON = ('The complete body uses the genuine JVM inference helper to check whether '
          'emitted conditional JavaScript calls upstream cljs.core/truth_, explicitly '
          'documented Internal - do not use. This is exact optimizer shape, outside '
          'the adopted compiler API scope. All 65 public predicate forms, neighboring '
          'public assertions and independent public/API/phase obligations stay retained '
          'and intended; no semantic or execution pass is supplied by this disposition.')
SCOPE_BASIS = ('User-adopted compiler-language-completion group186 revision2: '
               'Exact upstream analyzer AST/cache shape, private hash-trie node/storage '
               'classes, generated JS spelling and optimizer shape are not compiler API goals.')

def digest(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()

def pinned(path, revision=REVISION):
    return subprocess.check_output(['git','-C',str(inv.UPSTREAM),'show',revision+':'+path])

def validate():
    if inv.REVISION != REVISION:
        raise ValueError('inference scope revision drift')
    named = {}
    for path, expected in SOURCE_SHA256.items():
        raw = pinned(path)
        if (inv.UPSTREAM/path).read_bytes() != raw:
            raise ValueError('inference scope working source differs from pin')
        if digest(raw) != expected:
            raise ValueError('inference scope complete source drift')
        source = inv.Source(path, raw)
        for node in source.forms:
            if node.kind == 'list' and inv.short_head(node) in ('deftest','defmacro'):
                name = inv.effective(node.children[1]).token
                if name in named:
                    raise ValueError('inference scope duplicate active body')
                named[name] = (source, node)
    if set(named) != set(NAMED_SHA256):
        raise ValueError('inference scope missing/extra active bodies')
    for name, expected in NAMED_SHA256.items():
        source, node = named[name]
        if source.record(node)['sha256'] != expected:
            raise ValueError('inference scope original body/helper drift: '+name)
    source, body = named['test-cljs-2825']
    call = body.children[2]
    if inv.short_head(call) != 'truth_-not-called?' or len(call.children) != 66:
        raise ValueError('inference scope complete 65-form optimizer context drift')
    return [{'id':'body::'+BODY_ID,'obligation_kind':'assertion-free-test-body',
             'source':TEST_PATH,'body_id':BODY_ID,'body_sha256':BODY_SHA256,
             'source_span':source.record(body),'disposition':'unrelated',
             'category':'upstream-internals','reason':REASON,'scope_basis':SCOPE_BASIS,
             'scope_basis_record':{'group':'compiler-language-completion','id':186,
                                  'revision':2,'context_sha256':GROUP_HASH},
             'semantic_pass_credit':False,'local_execution_credit':False,
             'macro_phase_credit':False,'original_predicate_forms':65}]

def run(command, directory, timeout=240):
    directory.mkdir(parents=True)
    result=subprocess.run(command,cwd=ROOT,text=True,capture_output=True,timeout=timeout)
    (directory/'stdout').write_text(result.stdout)
    (directory/'stderr').write_text(result.stderr)
    if result.returncode:
        raise RuntimeError('reference command failed: '+str(directory))
    return result.stdout

def execute(output):
    """Both real pin compilers execute every unchanged original module test."""
    rows=validate(); producer_hash=digest(Path(__file__).read_bytes())
    output=Path(output).resolve();output.mkdir(parents=True,exist_ok=False)
    for path,expected in SOURCE_SHA256.items():
        if digest(pinned(path,RELEASE_TAG)) != expected:
            raise ValueError('release tag original module/helper differs')
    core=pinned('src/main/cljs/cljs/core.cljs').decode()
    jar=Path.home()/'.m2/repository/org/clojure/clojurescript/1.12.134/clojurescript-1.12.134.jar'
    with zipfile.ZipFile(jar) as archive:
        release_core=archive.read('cljs/core.cljs')
    for raw in (core, release_core.decode()):
        if raw.count(TRUTH_DECLARATION)!=1 or digest(TRUTH_DECLARATION)!=TRUTH_SHA256:
            raise ValueError('real pin internal truth_ declaration drift')
    host='''(ns inference-scope.host
  (:require [cljs.inference-test] [cljs.test :as test :include-macros true]))
(defn -main [& args] (test/run-tests 'cljs.inference-test))
(set! *main-cli-fn* -main)
'''
    helper=(ROOT/'tests/reference/execution.clj').read_text()
    oracle=helper.split('(try\n',1)[0]+'''\n(try
  (let [[kind identity checkout output source] *command-line-args*]
    (checked-reference kind identity checkout)
    (checked-build (java.io.File. source) output 'inference-scope.host
      #{'inference-scope.host 'cljs.inference-test})
    (checked-reference kind identity checkout))
  (finally (shutdown-agents)))
'''
    references={}
    for kind in ('release','checkout'):
        directory=output/kind;directory.mkdir()
        source=directory/'sources'
        for path in SOURCE_SHA256:
            target=source/'cljs'/Path(path).name;target.parent.mkdir(parents=True,exist_ok=True)
            target.write_bytes(pinned(path))
        target=source/'inference_scope/host.cljs';target.parent.mkdir(parents=True);target.write_text(host)
        oracle_path=directory/'build.clj';oracle_path.write_text(oracle)
        dependency='{:mvn/version "1.12.134"}' if kind=='release' else '{:local/root '+json.dumps(str(inv.UPSTREAM))+'}'
        identity='1.12.134' if kind=='release' else REVISION
        build=run(['clojure','-Srepro','-Sdeps','{:paths ['+json.dumps(str(source))+'] :deps {org.clojure/clojurescript '+dependency+'}}','-M',str(oracle_path),kind,identity,'-' if kind=='release' else str(inv.UPSTREAM),str(directory),str(source)],directory/'build')
        if build.count('REFERENCE IDENTITY '+kind+' '+identity)!=2:
            raise ValueError('missing checked pin identity')
        raw=run(['node','-e','process.chdir('+json.dumps(str(directory))+');require('+json.dumps(str(directory/'reference.js'))+');'],directory/'node')
        if 'Ran 3 tests containing 67 assertions.' not in raw or '0 failures, 0 errors.' not in raw:
            raise ValueError('whole original inference module did not pass')
        references[kind]={'reference_identity':kind+' '+identity,'raw_stdout':raw,
                          'whole_original_tests':3,'whole_original_assertions':67,
                          'target_macro_generated_assertions':65,
                          'settings':{'optimizations':'none','static-fns':False,'def-emits-var':False},
                          'original_source_sha256':SOURCE_SHA256,
                          'build_stdout_sha256':digest(build),
                          'emitted_target_module_sha256':digest((directory/'out/cljs/inference_test.js').read_bytes()),
                          'reference_javascript_sha256':digest((directory/'reference.js').read_bytes()),
                          'host_sha256':digest(host),'oracle_sha256':digest(oracle)}
        print(kind,'3 original tests / 67 genuine assertions passed',flush=True)
    validate()
    if digest(Path(__file__).read_bytes()) != producer_hash:
        raise ValueError('inference scope producer changed during execution')
    result={'schema_version':1,'profile':'exact-upstream-truth-optimizer-body-v1',
            'scope_proposal':rows,'references':references,
            'release_jar_sha256':digest(jar.read_bytes()),
            'release_core_source_sha256':digest(release_core),
            'checkout_core_source_sha256':digest(core),
            'internal_truth_declaration':TRUTH_DECLARATION,'internal_truth_sha256':TRUTH_SHA256,
            'original_release_tag':RELEASE_TAG,'helper_sha256':producer_hash,
            'original_semantic_pass_credit':0,'local_execution_credit':0,'macro_phase_credit':0}
    (output/'receipt.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();execute(args.output)
