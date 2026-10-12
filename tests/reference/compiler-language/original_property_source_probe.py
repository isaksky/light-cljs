"""Whole unchanged property bodies under genuine checked test.check reference hosts.

These reference-only observations grant zero local or shared completion credit.
"""
from pathlib import Path
import argparse, hashlib, json, os, subprocess
import extract_inventory as inventory
import report
ROOT=Path(__file__).resolve().parents[3]
SOURCES={'cljs.parse-test':'src/test/cljs/cljs/parse_test.cljs',
         'clojure.string-test':'src/test/cljs/clojure/string_test.cljs'}
TARGETS={'cljs.parse-test':['test-gen-parse-long','test-gen-parse-double'],
         'clojure.string-test':['test-cljs-2300']}
def digest(data):return hashlib.sha256(data if isinstance(data,bytes) else data.encode()).hexdigest()
def command(args,folder):
 folder.mkdir(parents=True);result=subprocess.run(args,cwd=ROOT,text=True,capture_output=True,timeout=600)
 (folder/'invocation.json').write_text(json.dumps(args,indent=2)+'\n');(folder/'stdout').write_text(result.stdout);(folder/'stderr').write_text(result.stderr)
 if result.returncode:raise ValueError('Reference command failed: '+str(folder))
 return result.stdout

def run(kind,output):
 output=Path(output).resolve();output.mkdir(parents=True,exist_ok=False);source=output/'source';bindings=[]
 for namespace,path in SOURCES.items():
  original=inventory.UPSTREAM.joinpath(path).read_text();dest=source/(namespace.replace('.','/').replace('-','_')+'.cljs');dest.parent.mkdir(parents=True,exist_ok=True);dest.write_text(original)
  forms=inventory.Reader(original).all()
  for target in TARGETS[namespace]:
   matches=[n for n in forms if inventory.head(n) in ('deftest','defspec') and n.children[1].token==target]
   if len(matches)!=1:raise ValueError('Exact property body unavailable')
   body=matches[0];bindings.append({'namespace':namespace,'name':target,'source':path,'source_sha256':digest(original),'body':original[body.start:body.end],'body_sha256':digest(original[body.start:body.end]),'original_source_unchanged':True})
 host='''(ns original.property-host (:require [cljs.test :as test] [cljs.parse-test] [clojure.string-test]))
(defn -main [& args]
 (test/set-env! (test/empty-env))
 (test/test-vars [#'cljs.parse-test/test-gen-parse-long #'cljs.parse-test/test-gen-parse-double #'clojure.string-test/test-cljs-2300])
 (let [counters (:report-counters (test/get-current-env))]
  (println "ORIGINAL PROPERTY COUNTERS" (js/JSON.stringify (clj->js counters)))
  (when-not (= {:test 3 :pass 3 :fail 0 :error 0} counters) (set! (.-exitCode js/process) 1))))
(set! *main-cli-fn* -main)
'''
 h=source/'original/property_host.cljs';h.parent.mkdir(parents=True);h.write_text(host)
 prefix=(ROOT/'tests/reference/execution.clj').read_text().split('(try\n',1)[0]
 oracle=prefix+'''\n(let [[kind identity checkout output source] *command-line-args*]
 (checked-reference kind identity checkout)
 (checked-build (java.io.File. source) output 'original.property-host #{'original.property-host 'cljs.parse-test 'clojure.string-test})
 (println "ORIGINAL PROPERTY BUILD COMPLETED"))\n'''
 build=output/'build.clj';build.write_text(oracle)
 identity=report.RELEASE if kind=='release' else report.REVISION
 dep='{:mvn/version "'+report.RELEASE+'"}' if kind=='release' else '{:local/root '+json.dumps(str(inventory.UPSTREAM))+'}'
 deps='{:paths [] :deps {org.clojure/clojurescript '+dep+' org.clojure/test.check {:mvn/version "1.1.1"}}}'
 stdout=command([os.environ.get('CLOJURE','clojure'),'-Srepro','-Sdeps',deps,'-M',str(build),kind,identity,'-' if kind=='release' else str(inventory.UPSTREAM),str(output),str(source)],output/'build')
 if 'REFERENCE IDENTITY '+kind+' '+identity not in stdout or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in stdout:raise ValueError('Unverified required reference identity/settings')
 raw=command([os.environ.get('NODE','node'),'-e','process.chdir('+json.dumps(str(output))+');require('+json.dumps(str(output/'reference.js'))+');'],output/'node')
 packets=[json.loads(line.split('ORIGINAL PROPERTY COUNTERS ',1)[1]) for line in raw.splitlines() if line.startswith('ORIGINAL PROPERTY COUNTERS ')]
 if packets!=[{'test':3,'pass':3,'fail':0,'error':0}]:raise ValueError('Complete original property result changed')
 receipt={'schema_version':1,'profile':'unchanged-original-property-reference-host-v1','reference_identity':kind+' '+identity,'settings':{'optimizations':'none','static-fns':False,'def-emits-var':False},'test_check_version':'1.1.1','original_bodies':bindings,'observed':packets[0],'artifact_root':str(output),'artifact_sha256':{str(p.relative_to(output)):digest(p.read_bytes()) for p in sorted(output.rglob('*')) if p.is_file()},'producer_source_sha256':{str(p.relative_to(ROOT)):digest(p.read_bytes()) for p in (Path(__file__),ROOT/'tests/reference/execution.clj',ROOT/'research/clojurescript/deps.edn')},'semantic_pass_credit':0,'macro_phase_credit':0,'source_disposition_changed':False,'strict_complete':False}
 (output/'evidence.json').write_text(json.dumps(receipt,indent=2)+'\n');print(kind,packets[0]);return receipt

if __name__=='__main__':
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--reference',choices=('release','checkout'),required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();run(args.reference,args.output)
