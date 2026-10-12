#!/usr/bin/env python3
"""Batch real pins without changing any existing complete namespace/expression oracle."""
from pathlib import Path
import argparse
import json
import os
import arity_after185 as cohort
import report
ROOT=cohort.ROOT


def execute(kind, directory):
    before={str(path.relative_to(ROOT)):cohort.digest(path.read_bytes()) for path in [Path(__file__).resolve(),Path(cohort.__file__),ROOT/'tests/reference/execution.clj',ROOT/'tests/support/execution.js',Path(report.__file__),ROOT/'tests/reference/compiler-language/extract_inventory.py',ROOT/'tests/reference/compiler-language/arity_resume_candidates.py',ROOT/'tests/fixtures/compiler-language.tsv',ROOT/'tests/reference/compiler-language/full-cases.jsonl']}
    rows=cohort.rows();cases=[row['case'] for row in rows]
    directory=Path(directory).resolve();directory.mkdir(parents=True,exist_ok=False)
    source=directory/'source';source.mkdir();namespaces=[];calls=[]
    for index,row in enumerate(rows):
        if row['invoke']:
            namespace=row['namespace'];original=row['source'];call='('+namespace+'/run'+(' observe' if row['invoke']=='(run nil)' else '')+')'
        else:
            namespace='arityreference.case'+str(index);original='(ns '+namespace+' (:require [cljs.core :as c]))\n(def run (fn* [observe] '+row['source']+'))\n';call='('+namespace+'/run observe)'
        path=source/(namespace.replace('.','/').replace('-','_')+'.cljs');path.parent.mkdir(parents=True,exist_ok=True);path.write_text(original)
        namespaces.append(namespace);calls.append('(fn* [observe] '+call+')')
    entry='arityreference.host'
    host='(ns '+entry+' (:require '+' '.join('['+namespace+']' for namespace in namespaces)+'))\n'
    host+='(set! (.-lightCases js/globalThis) (array '+ ' '.join(calls)+'))\n'
    host+='(set! (.-lightCaseMetadata js/globalThis) (array '+ ' '.join('(array '+json.dumps(case['id'])+' '+json.dumps(case['source'])+')' for case in cases)+'))\n'
    # Reuse the genuine oracle's inspector definition directly, preserving
    # source/identity/settings machinery while batching namespace files.
    harness=(ROOT/'tests/reference/execution.clj').read_text()
    prefix=harness.split('(try\n',1)[0]
    script=prefix+'''\n(try
  (let [[kind identity checkout output source] *command-line-args*]
    (checked-reference kind identity checkout)
    (let [file (io/file source "arityreference" "host.cljs")]
      (spit file (str (slurp file) inspect-value-source)))
    (checked-build (java.io.File. source) output 'arityreference.host
      #{'''+ ' '.join("'"+ns for ns in [*namespaces,entry])+'''})
    (checked-reference kind identity checkout))
  (finally (shutdown-agents)))
'''
    file=source/'arityreference/host.cljs';file.parent.mkdir(parents=True,exist_ok=True);file.write_text(host)
    oracle=directory/'build.clj';oracle.write_text(script)
    dependency='{:mvn/version "'+report.RELEASE+'"}' if kind=='release' else '{:local/root '+json.dumps(str(ROOT/'research/clojurescript'))+'}'
    identity=report.RELEASE if kind=='release' else report.REVISION
    command=['clojure','-Srepro','-Sdeps','{:paths ['+json.dumps(str(source))+'] :deps {org.clojure/clojurescript '+dependency+'}}','-M',str(oracle),kind,identity,'-' if kind=='release' else str(ROOT/'research/clojurescript'),str(directory),str(source)]
    compiled=report.invoke(command,directory/'build',timeout=600)
    if compiled['status']!='completed' or compiled['stdout'].count('REFERENCE IDENTITY '+kind+' '+identity)!=2 or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in compiled['stdout']:
        raise ValueError('checked pin source build/identity/settings failed: '+str(compiled))
    # Original namespace bytes and source expressions remain in their proper
    # per-case namespace. The generated inspector suffix only observes values.
    for index,row in enumerate(rows):
        if row['invoke']:
            path=source/(row['namespace'].replace('.','/').replace('-','_')+'.cljs')
            if path.read_text()!=row['source']:raise ValueError('original namespace changed by batching')
    javascript='process.chdir('+json.dumps(str(directory))+');require('+json.dumps(str(directory/'reference.js'))+');\n'+(ROOT/'tests/support/execution.js').read_text()
    raw=report.invoke([os.environ.get('NODE','node'),'-'],directory/'node',source=javascript,timeout=600)
    if raw['status']!='completed':raise ValueError('whole original source pin execution unavailable')
    observed=report.observations(raw['stdout'],cases)
    if len(observed)!=len(cases) or any(observed[case['id']]['status']!='passed' for case in cases):
        raise ValueError('whole original source pin outcomes failed: '+str(observed))
    for row in observed.values():row.update(reference_identity=kind+' '+identity,settings={'optimizations':'none','static-fns':False,'def-emits-var':False})
    after={str(path.relative_to(ROOT)):cohort.digest(path.read_bytes()) for path in [Path(__file__).resolve(),Path(cohort.__file__),ROOT/'tests/reference/execution.clj',ROOT/'tests/support/execution.js',Path(report.__file__),ROOT/'tests/reference/compiler-language/extract_inventory.py',ROOT/'tests/reference/compiler-language/arity_resume_candidates.py',ROOT/'tests/fixtures/compiler-language.tsv',ROOT/'tests/reference/compiler-language/full-cases.jsonl']}
    if before!=after:raise ValueError('actual reference producer source drift')
    receipt={'reference_identity':kind+' '+identity,'build_command':command,'node_command':[os.environ.get('NODE','node'),'-'],'build_invocation_sha256':cohort.digest((directory/'build/invocation.json').read_bytes()),'node_invocation_sha256':cohort.digest((directory/'node/invocation.json').read_bytes()),'node_stdin_sha256':cohort.digest((directory/'node/stdin').read_bytes()),'observations':observed,'original_case_sha256':{row['id']:row['case_sha256'] for row in rows},'producer_source_sha256':before,'build_script_sha256':cohort.digest(script),'build_stdout_sha256':cohort.digest(compiled['stdout']),'reference_js_sha256':cohort.digest((directory/'reference.js').read_bytes()),'raw_stdout_sha256':cohort.digest(raw['stdout']),'source_file_sha256':{str(path.relative_to(source)):cohort.digest(path.read_bytes()) for path in sorted(source.rglob('*')) if path.is_file()}}
    (directory/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(kind,str(len(cases))+' unchanged whole oracles passed',flush=True)
    return receipt


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--reference',choices=['release','checkout'],required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();execute(args.reference,args.output)
