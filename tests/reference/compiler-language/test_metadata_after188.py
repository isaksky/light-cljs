#!/usr/bin/env python3
"""Exact original :test callback: both pins, current phase rejection and recovery."""
from pathlib import Path
from unittest.mock import patch
import argparse
import hashlib
import json
import subprocess
import arity_after188_reference as reference
import arity_after188 as cohort
import report

ROOT = cohort.ROOT
BASE = ROOT/'tests/reference/compiler-language'
DRIVER = 'tests/compiler_test_metadata_after188.rs'
TEST = 'metadata_test_original_source_preserves_phase_failures_and_recovery'
SOURCE = 'tests/fixtures/arity-after188/cljs-definitions-var-test.cljs'
ID = 'cljs-definitions-var-test'
SOURCE_SHA256 = 'a6aea8026522804962ec49a6f5e2531c482eb9830c1c16b5962cf21f22b847b0'
CASE_SHA256 = 'ba0f8526d6a7da4acec44ba0de3e781e0529e2e88664ba705e66e0c3ac5fcd86'
SIGNATURE = 'Assert failed: (= :ok (test cljs-3411-function))'


def load():
    case = report.fixtures('intended-language')[ID]
    source = (ROOT/SOURCE).read_text()
    if source != case['source'] or cohort.digest(source) != SOURCE_SHA256 or cohort.digest(json.dumps(case,sort_keys=True)) != CASE_SHA256:
        raise ValueError('original complete metadata callback contract drift')
    if case['expected'] != {'id':ID,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''}:
        raise ValueError('independent literal original metadata oracle drift')
    return {'id':ID,'namespace':'cljs.var-test','invoke':'(run nil)','source':source,'case':case,'case_sha256':CASE_SHA256}


def snapshot():
    paths = subprocess.check_output(['git','ls-files','src','build.rs','Cargo.toml','Cargo.lock'],cwd=ROOT,text=True).splitlines()
    paths += [DRIVER,SOURCE,'tests/reference/compiler-language/test_metadata_after188.py',
        cohort.HELPER,cohort.SCANNER,'tests/reference/compiler-language/arity_after188_reference.py',
        'tests/reference/compiler-language/report.py','tests/reference/compiler-language/extract_inventory.py',
        'tests/reference/execution.clj','tests/support/execution.js',
        'tests/fixtures/compiler-language.tsv','tests/reference/compiler-language/full-cases.jsonl']
    return {p:cohort.digest((ROOT/p).read_bytes()) for p in paths}


def pins(directory):
    before = snapshot()
    row = load()
    for kind in ['release','checkout']:
        with patch.object(reference.cohort,'rows',lambda:[row]):
            value = reference.execute(kind,Path(directory)/kind)
        value['gap_producer_source_sha256'] = before
        (Path(directory)/kind/'receipt.json').write_text(json.dumps(value,indent=2)+'\n')
    if snapshot() != before:
        raise ValueError('metadata pin producer drift')


def verify_references(directory):
    row = load()
    result = {}
    for kind in ['release','checkout']:
        root = (Path(directory)/kind).resolve()
        receipt = json.loads((root/'receipt.json').read_text())
        identity = kind+' '+(report.RELEASE if kind=='release' else report.REVISION)
        if receipt['reference_identity'] != identity or receipt['original_case_sha256'] != {ID:CASE_SHA256}:
            raise ValueError('wrong original case or required pin')
        if receipt['gap_producer_source_sha256'] != snapshot():
            raise ValueError('current diagnostic/compiler/pin producer drift')
        for name,digest in receipt['producer_source_sha256'].items():
            if cohort.digest((ROOT/name).read_bytes()) != digest:
                raise ValueError('raw canonical producer drift')
        sources = {str(p.relative_to(root/'source')):cohort.digest(p.read_bytes()) for p in sorted((root/'source').rglob('*')) if p.is_file()}
        if sources != receipt['source_file_sha256'] or (root/'source/cljs/var_test.cljs').read_text() != row['source']:
            raise ValueError('original whole canonical source drift')
        for name,field in [('build.clj','build_script_sha256'),('build/stdout','build_stdout_sha256'),('reference.js','reference_js_sha256'),('node/stdout','raw_stdout_sha256'),('build/invocation.json','build_invocation_sha256'),('node/invocation.json','node_invocation_sha256'),('node/stdin','node_stdin_sha256')]:
            if cohort.digest((root/name).read_bytes()) != receipt[field]:
                raise ValueError('actual canonical input/output/build drift')
        stdout = (root/'build/stdout').read_text()
        if stdout.count('REFERENCE IDENTITY '+identity) != 2 or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in stdout:
            raise ValueError('actual canonical identity/settings absent')
        expected_stdin = 'process.chdir('+json.dumps(str(root))+');require('+json.dumps(str(root/'reference.js'))+');\n'+(ROOT/'tests/support/execution.js').read_text()
        if (root/'node/stdin').read_text() != expected_stdin or json.loads((root/'node/invocation.json').read_text()) != receipt['node_command']:
            raise ValueError('actual canonical engine command/input drift')
        if json.loads((root/'build/invocation.json').read_text()) != receipt['build_command']:
            raise ValueError('actual canonical build command drift')
        observations = [json.loads(line) for line in (root/'node/stdout').read_text().splitlines()]
        if observations != [row['case']['expected']] or receipt['observations'][ID]['status'] != 'passed' or receipt['observations'][ID]['observed'] != row['case']['expected']:
            raise ValueError('both pins must pass unchanged literal metadata contract')
        result[kind] = {'receipt_path':str(root/'receipt.json'),'receipt_sha256':cohort.digest((root/'receipt.json').read_bytes()),**receipt}
    return result


def produce(actual,modules,references,output):
    row = load()
    raw = Path(actual).read_text()
    root = Path(modules).resolve()
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in raw:
        raise ValueError('exact current metadata diagnostic/recovery test did not pass')
    records = [json.loads(line) for line in raw.splitlines() if line.startswith('{')]
    binaries = [r['executable'] for r in records if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')=='compiler_test_metadata_after188' and r.get('executable')]
    if len(binaries) != 1:
        raise ValueError('missing actual diagnostic executable metadata')
    tree = {str(p.relative_to(root)):cohort.digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file()}
    frames = []
    diagnostics = []
    recoveries = []
    packets = []
    recovery_packets = []
    for line in raw.splitlines():
        fields = line.split('\t')
        if fields[0]=='test-metadata-after188-observation': frames.append(fields[1:])
        if fields[0]=='test-metadata-after188-diagnostic': diagnostics.append(fields[1:])
        if fields[0]=='test-metadata-after188-recovery': recoveries.append(fields[1:])
        if fields[0]=='test-metadata-after188-artifacts': packets.append(fields[1:])
        if fields[0]=='test-metadata-after188-recovery-artifacts': recovery_packets.append(fields[1:])
    if frames != [[host,'application',engine,'true'] for host in ['fresh','reused'] for engine in ['QuickJS','Node']]:
        raise ValueError('missing current original application engine observations')
    if [(d[0],d[1]) for d in diagnostics] != [('fresh','macro'),('reused','macro')]:
        raise ValueError('missing fresh/reused actual demanded macro rejection')
    if recoveries != [[host,phase,engine,'42'] for host in ['fresh','reused'] for phase in ['application','macro'] for engine in ['QuickJS','Node']]:
        raise ValueError('same-host recovery observation drift')
    if [(p[0],p[1]) for p in packets] != [('fresh','application'),('reused','application')] or [(p[0],p[1]) for p in recovery_packets] != [(h,p) for h in ['fresh','reused'] for p in ['application','macro']]:
        raise ValueError('missing actual executed source/emitted hash packets')
    for host,phase,emitted,source in packets:
        if tree[host+'/'+phase+'/compiled.js']!=emitted or tree[host+'/'+phase+'/cljs/var_test.cljs']!=source:
            raise ValueError('original executed source/emitted input drift')
    for host,phase,emitted,source in recovery_packets:
        if tree[host+'/'+phase+'/recovery/compiled.js']!=emitted or tree[host+'/'+phase+'/recovery/user.cljs']!=source:
            raise ValueError('same-host recovery executed source/emitted input drift')
    expected_files = {'warmup/warm.cljs','warmup.js'}
    dispositions = []
    for host in ['fresh','reused']:
        for phase in ['application','macro']:
            prefix = host+'/'+phase+'/'
            program = row['source']+ ('\n(defmacro metadata-witness [] (run nil))\n(pr-str (metadata-witness))\n' if phase=='macro' else '\n(pr-str (run nil))\n')
            if (root/prefix/'cljs/var_test.cljs').read_text() != program:
                raise ValueError('original complete source/setup/callback/phase input drift')
            expected_files |= {prefix+'cljs/var_test.cljs',prefix+'recovery/user.cljs',prefix+'recovery/compiled.js'}
            if (root/prefix/'recovery/user.cljs').read_text() != '(ns user) (defmacro recovered-after188 [] (+ 40 2)) (pr-str (recovered-after188))':
                raise ValueError('same-host actual demanded macro recovery source drift')
            if phase=='macro':
                expected_files.add(prefix+'diagnostic.txt')
                detail = (root/prefix/'diagnostic.txt').read_text()
                if SIGNATURE not in detail or str(root/prefix/'cljs/var_test.cljs') not in detail or 'macro cljs.var-test/metadata-witness' not in detail:
                    raise ValueError('precise located original macro callback diagnostic drift')
                if Path(next(d[2] for d in diagnostics if d[:2]==[host,phase])).resolve() != root/prefix/'diagnostic.txt':
                    raise ValueError('actual diagnostic source artifact detached from execution')
                for engine in ['QuickJS','Node']:
                    dispositions.append({'host':host,'phase':phase,'engine':engine,'status':'macro-host-rejected' if engine=='QuickJS' else 'not-run-after-macro-host-rejection','detail':detail,'canonical_credit':0})
            else:
                expected_files.add(prefix+'compiled.js')
                for engine in ['QuickJS','Node']:
                    dispositions.append({'host':host,'phase':phase,'engine':engine,'status':'passed','literal':'true','complete_phase_credit':0})
    if set(tree) != expected_files:
        raise ValueError('complete current source/emitted/diagnostic/recovery artifact census drift')
    value = {'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        'source_contract':row,'producer_source_sha256':snapshot(),'references':verify_references(references),
        'actual_command':'LIGHT_TEST_METADATA_AFTER188_ARTIFACTS='+str(root)+' cargo test --locked --message-format=json --test compiler_test_metadata_after188 '+TEST+' -- --ignored --exact --nocapture',
        'actual_log_path':str(Path(actual).resolve()),'actual_log_sha256':cohort.digest(raw),
        'actual_executable':binaries[0],'actual_executable_sha256':cohort.digest(Path(binaries[0]).read_bytes()),
        'module_artifact_sha256':tree,'phase_engine_dispositions':dispositions,
        'same_host_recovery_observations':recoveries,'actual_executed_input_hash_packets':packets+recovery_packets,
        'confirmed_gap':'The original metadata :test callback succeeds in actual applications and both canonical pins but fails during actual demanded MacroHost computation.',
        'operation_credit_added':0,'source_site_credit_added':0,'strict_complete':False}
    Path(output).write_text(json.dumps(value,indent=2)+'\n')
    print('Exact original both pins passed; 4 application observations, 2 macro-host rejections, 8 same-host recovery observations; receipt',cohort.digest(Path(output).read_bytes()))
    return value


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--run-pins',type=Path)
    p.add_argument('--actual',type=Path)
    p.add_argument('--modules',type=Path)
    p.add_argument('--references',type=Path)
    p.add_argument('--output',type=Path)
    args=p.parse_args()
    if args.run_pins:pins(args.run_pins)
    else:produce(args.actual,args.modules,args.references,args.output)
