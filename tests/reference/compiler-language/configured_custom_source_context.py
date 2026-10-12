#!/usr/bin/env python3
"""Complete original custom-reader source/configuration with fresh actual phase receipts.

Standalone unregistered candidate. No stored receipt or static registration
supplies coverage. The complete original module, assertion and reader map stay fixed.
"""
from __future__ import annotations
import argparse, copy, json, os, shutil
from pathlib import Path
import configured_source_context as runner
import sci_execution as sci
import sci_source_context as context

ROOT = sci.ROOT
ID = 'cljs-original-configured-custom-reader-context'
PROFILE = 'original-custom-source-reader-form-v1'
UPSTREAM_SOURCE = 'src/test/cljs/data_readers_test/core.cljc'
UPSTREAM_CONFIG = 'src/test/cljs/data_readers.cljc'
SOURCE_HASH = '6c1fd2c66f39224f6ccaf9991c1195b244dfcd9723aa819759e6b40e9c6a55fe'
CONFIG_HASH = '344170f2e2b2b00dd8e21d4450f33b951eb48c9da62ef4857e175282a1f7f9d9'
BODY_HASH = '9af4a87d9b1f8d7b3bddb165279a6c533021f22c8b01b355603b502a0128dddd'
ASSERTION_ID = UPSTREAM_SOURCE+'::top-level-003::assertion-001'
ASSERTION = '(assert (= 1 #test/custom-identity 1))'
FORM_READER = '(fn [x] `(js/Array.of ~x))'
RESULT_CHECK = '(assert (and (array? result) (= ["foo"] (vec result))))'
DRIVER = ROOT/'tests/compiler_configured_custom_source_context.rs'
TEST = 'complete_custom_reader_source_executes_in_both_phases_and_lifecycles'
VARIANTS = ('application-fresh','macro-fresh','application-reused','macro-reused')

def expected():
    return {'id':ID,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''}

def load():
    original=(ROOT/'research/clojurescript'/UPSTREAM_SOURCE).read_text()
    config=(ROOT/'research/clojurescript'/UPSTREAM_CONFIG).read_text()
    if sci.digest(original)!=SOURCE_HASH or sci.digest(config)!=CONFIG_HASH:
        raise ValueError('Changed complete original module or configuration')
    inventory=[json.loads(line) for line in (ROOT/'tests/reference/compiler-language/full-inventory.jsonl').read_text().splitlines()]
    matches=[r for r in inventory if r['id']==ASSERTION_ID]
    if len(matches)!=1 or matches[0]['assertion']['text']!=ASSERTION or matches[0]['assertion']['sha256']!=BODY_HASH or matches[0]['body_sha256']!=BODY_HASH:
        raise ValueError('Changed sole complete original assertion')
    # The source compiler reads the :cljs mapping, then the JVM function's
    # documented :clj form-producing branch. The local configured reader uses
    # exactly that branch as ordinary source; the original CLJC stays untouched.
    if ('test/custom-identity data-readers-test.core/custom-identity' not in config
        or 'test/custom-form #?(:cljs data-readers-test.core/custom-form-cljs :clj clojure.core/identity)' not in config
        or '(def custom-identity identity)' not in original
        or '#?(:clj `(js/Array.of ~x)' not in original):
        raise ValueError('Changed original handler phase/source binding')
    return {'id':ID,'source':original,'source_sha256':SOURCE_HASH,'module_source':UPSTREAM_SOURCE,
        'configuration':{'source':UPSTREAM_CONFIG,'text':config,'sha256':CONFIG_HASH},
        'configured_readers':[{'tag':'test/custom-identity','source':'identity'},
            {'tag':'test/custom-form','source':FORM_READER,'original_phase':'custom-form-cljs :clj source-compilation branch'}],
        'expected':expected(),'source_context_profile':{'schema_version':1,'profile':PROFILE},
        'original_assertion':matches[0],'prospective_original_assertion_credit':1,
        'complete_original_module_sha256':SOURCE_HASH}

def validate(row):
    if not sci.report_module().same_json(row,load()):
        raise ValueError('Missing/changed original whole-source/configured-reader profile')
    return row


def case():
    """One exact whole-source fixture, linked to its sole original assertion."""
    row = load()
    return {**row, 'native_stdout': None,
        'hosted_source_adapter': {'schema_version': 1, 'profile': PROFILE},
        'complete_original_provenance': [{'body_id': row['original_assertion']['body_id'],
            'source_sha256': BODY_HASH}],
        'reference_adaptation': 'Complete unchanged original CLJC and data_readers.cljc configuration. '
            'Compiler embedding uses the genuine original custom-form-cljs :clj form-producing branch. '
            'The sole original assert and independently added Array.of result check execute in each phase.'}


def validate_case(value):
    if not sci.report_module().same_json(value, case()):
        raise ValueError('Changed/reduced complete original custom-reader fixture')
    return True


def execute(cases, directory, timeout=240, reference='both', checkout=None):
    """Return freshly executed layers; a registration or stored receipt is insufficient."""
    if len(cases) != 1 or not validate_case(cases[0]):
        raise ValueError('Missing/duplicate custom-reader hosted fixture')
    if reference not in ('none', 'release', 'checkout', 'both'):
        raise ValueError('Unknown checked reference selection')
    if checkout is not None and Path(checkout).resolve() != (ROOT/'research/clojurescript').resolve():
        raise ValueError('Custom reader requires the reviewed checked pin')
    proof = run(directory, timeout)
    frames = proof['local']['frames']
    if list(frames) != [v+'/'+e for v in VARIANTS for e in ('QuickJS', 'Node')]:
        raise ValueError('Missing complete current custom-reader phase frames')
    for frame in frames.values():
        if frame.get('status') != 'passed' or not sci.report_module().same_json(frame.get('observed'), expected()):
            raise ValueError('Nonpassing current custom-reader frame')
    layers = {'javascript': {'status': 'passed', 'observed': frames['application-fresh/QuickJS']['observed'],
        'hosted_source_adapter': PROFILE, 'observations': frames, 'generic_cli': 'not claimed'}}
    for kind in ('release', 'checkout'):
        if reference in (kind, 'both'):
            layers[kind] = proof['references'][kind]
    return {ID: layers}


def module(phase):
    original=load()['source']
    if phase=='application':return original+'\n(defn run [] (do '+RESULT_CHECK+' true))\n'
    if phase=='macro':
        prefix,body=original.split('(ns data-readers-test.core)',1)
        return prefix+'(ns data-readers-test.core)\n(defmacro upstream [] (do '+body+' '+RESULT_CHECK+' true))\n(defn run [] (upstream))\n'
    raise ValueError('Unknown phase')

def strict_json(text):
    def pairs(rows):
        result={}
        for key,value in rows:
            if key in result:raise ValueError('Duplicate raw observation field')
            result[key]=value
        return result
    def constant(value):raise ValueError('Nonfinite raw observation')
    return json.loads(text,object_pairs_hook=pairs,parse_constant=constant)

def parse_raw(text,row):
    validate(row)
    records=[line.split('\t') for line in text.splitlines()]
    order=[(v,e) for v in VARIANTS for e in ('QuickJS','Node')]
    if len(records)!=8 or any(len(r)!=6 for r in records) or [(r[0],r[1]) for r in records]!=order:
        raise ValueError('Missing/partial/duplicate/reordered actual phase frames')
    result=[]
    for variant,engine,src_hex,js_hex,raw_hex,warmup_hex in records:
        source,javascript,raw=[bytes.fromhex(x).decode() for x in (src_hex,js_hex,raw_hex)]
        if source!=module(variant.split('-')[0]) or javascript.count(' = // Original bounded runtime')!=1:
            raise ValueError('Changed original namespace/complete phase source/runtime anchor')
        if (warmup_hex=='-')!=variant.endswith('fresh') or (variant.endswith('reused') and not bytes.fromhex(warmup_hex)):
            raise ValueError('Missing actual reused-host warmup')
        observed=strict_json(raw)
        if not sci.report_module().same_json(observed,expected()):
            raise ValueError('Original literal result/effect/output mismatch')
        if engine=='Node' and raw!=json.dumps(observed,separators=(',',':'))+'\n':
            # Rust writes one console.log frame. Object order is retained in raw.
            raise ValueError('Extra/malformed Node raw output')
        result.append((variant,engine,source,javascript,raw,warmup_hex,observed))
    for left,right in zip(result[::2],result[1::2]):
        if left[2:4]!=right[2:4] or left[5]!=right[5]:raise ValueError('Engines detached from same compilation')
    return result

def snapshot():
    paths=[Path(__file__),DRIVER,Path(__file__).with_name('test_configured_custom_source_context.py'),
           Path(__file__).with_name('configured-custom-source-context.md'), ROOT/'research/clojurescript'/UPSTREAM_SOURCE, ROOT/'research/clojurescript'/UPSTREAM_CONFIG, ROOT/'tests/reference/compiler-language/full-inventory.jsonl',
           ROOT/'tests/support/sci_execution_observation.js',ROOT/context.ORACLE,
           ROOT/'tests/reference/execution.clj',ROOT/'tests/support/execution.js',
           Path(sci.__file__),Path(context.__file__),Path(runner.__file__),ROOT/'tests/reference/compiler-language/report.py']
    return {**context.compiler_sources(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}

def run_local(directory,timeout=240):
    row=load();directory=Path(directory).resolve()
    if directory.exists():raise ValueError('Fresh source audit artifacts required')
    directory.mkdir(parents=True);before=snapshot()
    request=directory/'input.tsv';output=directory/'observations.tsv'
    request.write_text(ID+'\t'+row['source'].encode().hex()+'\t'+row['configuration']['text'].encode().hex()+'\t'+json.dumps(expected(),separators=(',',':')).encode().hex()+'\n')
    build,_=runner.run(['cargo','test','--locked','--test','compiler_configured_custom_source_context','--no-run','--message-format=json'],directory/'build',timeout)
    artifacts=[json.loads(line) for line in build.splitlines() if line.startswith('{')]
    binaries=[Path(r['executable']) for r in artifacts if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')=='compiler_configured_custom_source_context' and r.get('executable')]
    if len(binaries)!=1:raise ValueError('Missing/duplicate exact audit build')
    binary=binaries[0];binary_hash=sci.digest(binary.read_bytes());node=Path(shutil.which(os.environ.get('NODE','node')) or '').resolve()
    if not node.is_file():raise ValueError('Node unavailable')
    node_hash=sci.digest(node.read_bytes())
    env={**os.environ,'LIGHT_CONFIGURED_CUSTOM_SOURCE_INPUT':str(request),'LIGHT_CONFIGURED_CUSTOM_SOURCE_OUTPUT':str(output),'LIGHT_CONFIGURED_CUSTOM_SOURCE_ROOT':str(directory/'modules')}
    stdout,_=runner.run([str(binary),TEST,'--ignored','--exact','--nocapture'],directory/'audit',timeout,environment=env)
    if 'running 1 test' not in stdout or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in stdout:
        raise ValueError('Actual source audit did not execute')
    frames={}
    for variant,engine,source,javascript,raw,warmup,observed in parse_raw(output.read_text(),row):
        frames[variant+'/'+engine]={'status':'passed','observed':observed,'phase_source_sha256':sci.digest(source),
            'compiled_js_sha256':sci.digest(javascript),'raw_observation_sha256':sci.digest(raw),
            'original_source_sha256':SOURCE_HASH,'reader_configuration_sha256':CONFIG_HASH,'body_sha256':BODY_HASH,
            'warmup_compiled_js_sha256':None if warmup=='-' else sci.digest(bytes.fromhex(warmup)),
            'compiler_build_producer_source_sha256':before,'audit_executable_sha256':binary_hash,
            'node_executable_sha256':node_hash,'build_stdout_sha256':sci.digest(build)}
    runner.unchanged(before,snapshot())
    if sci.digest(binary.read_bytes())!=binary_hash or sci.digest(node.read_bytes())!=node_hash:raise ValueError('Execution binary drift')
    result={'status':'passed','source_binding':row,'frames':frames,'compiler_build_producer_source_sha256':before,
        'raw_observations_sha256':sci.digest(output.read_bytes()),'source_context_profile':copy.deepcopy(row['source_context_profile']),
        'test_file':str(DRIVER.relative_to(ROOT)),'test_name':TEST,'catalog_credit':0}
    (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n');return result

def run(directory,timeout=240):
    directory=Path(directory).resolve()
    if directory.exists():raise ValueError('Fresh complete evidence artifacts required')
    directory.mkdir(parents=True);before=snapshot();row=load()
    result={'source_binding':row,'local':run_local(directory/'local',timeout),'references':{}}
    refroot=directory/'reference-source';(refroot/'data_readers_test').mkdir(parents=True)
    (refroot/'data_readers_test/core.cljc').write_text(module('application'))
    (refroot/'data_readers.cljc').write_text(row['configuration']['text'])
    case={'id':ID,'source':module('application'),'source_root':str(refroot),'entry':'data-readers-test.core','export':'run','expected':expected()}
    result['reference_files_sha256']={str(p.relative_to(refroot)):sci.digest(p.read_bytes()) for p in sorted(refroot.rglob('*')) if p.is_file()}
    for kind in ('release','checkout'):
        layer=sci.report_module().reference_cases([case],kind,ROOT/'research/clojurescript',directory/kind,timeout)[ID]
        if layer.get('status')!='passed':raise ValueError('Full original checked-pin source failed: '+kind)
        artifact=directory/kind/ID
        layer['build_stdout_sha256']=sci.digest((artifact/'build/stdout').read_bytes())
        build_stderr=(artifact/'build/stderr').read_text()
        layer['build_stderr_sha256']=sci.digest(build_stderr)
        layer['reference_warnings']=[line for line in build_stderr.splitlines() if line.startswith('WARNING:')]
        layer['source_sha256']=SOURCE_HASH
        layer['reader_configuration_sha256']=CONFIG_HASH
        layer['raw_observation_sha256']=sci.digest((artifact/'node/stdout').read_bytes())
        layer['reference_javascript_sha256']=sci.digest((artifact/'reference.js').read_bytes())
        result['references'][kind]=layer
        print(kind,'passed',flush=True)
    after_reference_files={str(p.relative_to(refroot)):sci.digest(p.read_bytes()) for p in sorted(refroot.rglob('*')) if p.is_file()}
    runner.unchanged(result['reference_files_sha256'],after_reference_files)
    runner.unchanged(before,snapshot());result.update(status='passed',catalog_credit=0,compiler_build_producer_source_sha256=before)
    (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n');return result

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path);parser.add_argument('--timeout',type=int,default=240);args=parser.parse_args()
    if args.output:run(args.output,args.timeout);print('Complete original source/config: eight actual phase frames and both checked pins passed')
    else:load();print('Exact original source profile validated; no execution credit')
if __name__=='__main__':main()
