"""Narrow original cljs.eval-test diagnostic. Never modifies a shared ledger."""
from pathlib import Path
import argparse, hashlib, json, os, subprocess, sys, zipfile
sys.dont_write_bytecode=True
from extract_inventory import Reader, head

ROOT=Path(__file__).resolve().parents[3]
ORIGINAL=ROOT/'research/clojurescript/src/test/cljs/cljs/eval_test.cljs'
DRIVER=ROOT/'tests/compiler_original_eval_source_probe.rs'
TEST='whole_original_eval_body_uses_installed_application_service'
def digest(value): return hashlib.sha256(value if isinstance(value,bytes) else value.encode()).hexdigest()
def adapter(original):
    # Keep every original declaration and the entire deftest body byte-for-byte.
    # Only the cljs.test namespace dependency is supplied by small report macros.
    retained=original.split(';;; This test namespace',1)[1]
    prefix='''(ns cljs.eval-test)
(def original-probe-results (atom []))
(defn original-probe-check [thunk]
  (try (swap! original-probe-results conj [:returned (boolean (thunk))])
    (catch :default e (swap! original-probe-results conj [:thrown (.-message e)]))))
(defmacro is [expr] (list 'cljs.eval-test/original-probe-check (list 'fn [] expr)))
(defmacro deftest [name & body] (list 'defn name [] (cons 'do body)))
;;; This test namespace'''
    return prefix+retained+'\n(test-eval)\n@original-probe-results\n'
def run(command, directory, env=None):
    result=subprocess.run(command,cwd=ROOT,env=env,text=True,capture_output=True)
    directory.mkdir(parents=True,exist_ok=True)
    (directory/'stdout.txt').write_text(result.stdout);(directory/'stderr.txt').write_text(result.stderr)
    if result.returncode:raise RuntimeError(f'{command[0]} failed; see {directory}')
    return result.stdout
def local(directory):
    original=ORIGINAL.read_text(); forms=Reader(original).all()
    body=[n for n in forms if head(n)=='deftest'][0]
    assertions=[original[n.start:n.end] for n in body.children[2:]]
    assert len(assertions)==14 and all(head(n)=='is' for n in body.children[2:])
    directory.mkdir(parents=True,exist_ok=False)
    source=adapter(original); (directory/'source.cljs').write_text(source)
    before={str(p.relative_to(ROOT)):digest(p.read_bytes()) for p in (ROOT/'src').rglob('*') if p.is_file()}
    build=run(['cargo','test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],directory/'build')
    binaries=[Path(r['executable']) for l in build.splitlines() if l.startswith('{') for r in [json.loads(l)] if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    assert len(binaries)==1
    output=directory/'observations.tsv'
    stdout=run([str(binaries[0]),TEST,'--ignored','--exact','--nocapture'],directory/'audit',{**os.environ,'LIGHT_ORIGINAL_EVAL_SOURCE':str(directory/'source.cljs'),'LIGHT_ORIGINAL_EVAL_OUTPUT':str(output)})
    assert 'test result: ok. 1 passed; 0 failed; 0 ignored;' in stdout
    observations={variant:bytes.fromhex(raw).decode() for variant,raw in (l.split('\t') for l in output.read_text().splitlines())}
    after={str(p.relative_to(ROOT)):digest(p.read_bytes()) for p in (ROOT/'src').rglob('*') if p.is_file()};assert before==after
    evidence={'schema_version':1,'original_semantic_pass_credit':0,'profile':'whole-original-eval-body-installed-application-service-diagnostic-v1','original_file_sha256':digest(original),'original_body_sha256':digest(original[body.start:body.end]),'original_assertions':[{'index':i+1,'source':s,'sha256':digest(s)} for i,s in enumerate(assertions)],'adapter_source_sha256':digest(source),'audit_driver_sha256':digest(DRIVER.read_bytes()),'audit_executable_sha256':digest(binaries[0].read_bytes()),'compiler_source_sha256':before,'observations':observations}
    (directory/'evidence.json').write_text(json.dumps(evidence,indent=2)+'\n')
    summary={variant:[('passed' if n.children[0].token==':returned' and n.children[1].token=='true' else 'false' if n.children[0].token==':returned' else 'thrown') for n in Reader(raw).all()[0].children] for variant,raw in observations.items()}
    print(json.dumps(summary,indent=2));return evidence

def reference(directory,kind):
    """Use the pin's genuine cljs.js implementation and upstream loader plumbing."""
    directory.mkdir(parents=True,exist_ok=False)
    upstream=ROOT/'research/clojurescript'
    identity='1.12.134' if kind=='release' else 'a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e'
    source_root=directory/'sources';source_root.mkdir()
    if kind=='release':
        jar=Path.home()/'.m2/repository/org/clojure/clojurescript/1.12.134/clojurescript-1.12.134.jar'
        with zipfile.ZipFile(jar) as archive:
            for entry in archive.namelist():
                if entry.endswith(('.cljs','.cljc','.clj')) and '..' not in Path(entry).parts:
                    destination=source_root/entry;destination.parent.mkdir(parents=True,exist_ok=True);destination.write_bytes(archive.read(entry))
        paths=[directory/'out',source_root]
        dependency='{:mvn/version "1.12.134"}'
    else:
        paths=[directory/'out',upstream/'src/main/cljs',upstream/'src/main/clojure',upstream/'src/test/cljs']
        dependency='{:local/root '+json.dumps(str(upstream))+'}'
    loader_file=upstream/'src/test/self/self_parity/test.cljs'
    loader=loader_file.read_text().split(';; Error handler',1)[0]
    # Use the upstream loader's source definitions, with an isolated output path,
    # synchronous callback filesystem loading and only this single original test.
    loader=loader.replace('self-parity.test','original-eval.host').replace('\n            [self-parity.auxiliary]','\n            [cljs.env :as env]\n            [cljs.eval-test]\n            [cljs.test :refer-macros [run-tests]]')
    loader=loader.replace('(def out-dir "builds/out-self-parity")','(def out-dir '+json.dumps(str(directory/'out'))+')')
    start=loader.index('(def src-paths');end=loader.index('(defn require*',start)
    loader=loader[:start]+'(def src-paths ['+' '.join(json.dumps(str(p)) for p in paths)+'])\n\n'+loader[end:]
    loader=loader.replace('(def load-fn (make-load-fn src-paths node-read-file))','(def load-fn (make-load-fn src-paths (fn [filename cb] (cb (try (node-read-file-sync filename) (catch :default e nil))))))')
    original=ORIGINAL.read_text();runtime_source=original
    loader+='''\n(defn -main [& args]
  (init-runtime)
  (set! cljs/*eval-fn* node-eval)
  (set! cljs/*load-fn* load-fn)
  (cljs/eval st '(ns cljs.eval-test)
    {:ns 'cljs.user :eval node-eval :load load-fn}
    (fn [{:keys [error]}]
      (if error (do (println "ORIGINAL EVAL HOST ERROR" error) (set! (.-exitCode js/process) 1))
        (do (binding [env/*compiler* st *ns* (create-ns 'cljs.eval-test)]
              (run-tests 'cljs.eval-test))
            (println "ORIGINAL EVAL HOST COMPLETED"))))))
(set! *main-cli-fn* -main)
'''
    host_root=directory/'host';host_path=host_root/'original_eval/host.cljs';host_path.parent.mkdir(parents=True);host_path.write_text(loader)
    retained_path=host_root/'cljs/eval_test.cljs';retained_path.parent.mkdir(parents=True);retained_path.write_text(original)
    helper=ROOT/'tests/reference/execution.clj'
    oracle=helper.read_text().split('(try\n',1)[0]+'''\n(let [[kind identity checkout output src] *command-line-args*]
  (checked-reference kind identity checkout)
  (checked-build (java.io.File. src) output 'original-eval.host #{'original-eval.host})
  (println "ORIGINAL EVAL HOST BUILD COMPLETED"))\n'''
    oracle_path=directory/'build-host.clj';oracle_path.write_text(oracle)
    command=[os.environ.get('CLOJURE','clojure'),'-Srepro','-Sdeps','{:paths [] :deps {org.clojure/clojurescript '+dependency+'}}','-M',str(oracle_path),kind,identity,'-' if kind=='release' else str(upstream),str(directory),str(host_root)]
    build=run(command,directory/'build')
    assert 'REFERENCE IDENTITY '+kind+' '+identity in build and 'ORIGINAL EVAL HOST BUILD COMPLETED' in build
    raw=run([os.environ.get('NODE','node'),'-e','process.chdir('+json.dumps(str(directory))+');require('+json.dumps(str(directory/'reference.js'))+');'],directory/'audit')
    passed='Ran 1 tests containing 14 assertions.' in raw and '0 failures, 0 errors.' in raw and 'ORIGINAL EVAL HOST COMPLETED' in raw
    evidence={'reference_identity':kind+' '+identity,'status':'passed' if passed else 'unresolved','original_file_sha256':digest(original),'runtime_source_sha256':digest(runtime_source),'original_source_unchanged':runtime_source.startswith(original),'self_host_eval_service':'pin cljs.js/eval-impl; default Function/Var emit-constant* methods; :def-emits-var true','bootstrap_settings':{'optimizations':'none','static-fns':False,'def-emits-var':False},'loader_upstream_source_sha256':digest(loader_file.read_bytes()),'loader_adapter_sha256':digest(loader),'build_oracle_sha256':digest(oracle),'raw_stdout':raw,'original_semantic_pass_credit':0}
    (directory/'evidence.json').write_text(json.dumps(evidence,indent=2)+'\n');print(kind,passed);return evidence
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);p.add_argument('--reference',choices=('release','checkout'));a=p.parse_args()
    if a.reference:reference(a.output.resolve(),a.reference)
    else:local(a.output.resolve())
