#!/usr/bin/env python3
"""Original SCI symbols/keywords source in user, with fresh actual phase receipts.

Standalone unregistered candidate. No stored receipt or static registration
supplies coverage. The complete original expression and literal result stay fixed.
"""
from __future__ import annotations
import argparse, copy, json, os, shutil
from pathlib import Path
import configured_source_context as runner
import sci_execution as sci
import sci_source_context as context

ROOT = sci.ROOT
ID = 'sci-execution-sci-symbols-keywords'
SCI_ID = 'sci-symbols-keywords'
PROFILE = 'original-sci-user-symbol-keyword-v1'
SOURCE = "['x 'alpha/x :x :alpha/x ::x]"
SOURCE_HASH = '06aa443910912f086e15bef5090fded87e5421e0e38cf9e0d831cb30a4bf8d8f'
ROW_HASH = '82eeb15094a3317506767a0cbb65a885eb7594d2c790d7ba0edff64deeb5eeb1'
BODY_HASH = '41ad0419a57a146e24c20853a5012662331eb0ad821985628f37a2053afe5e13'
DRIVER = ROOT/'tests/compiler_sci_symbol_keyword_context.rs'
TEST = 'original_symbol_keyword_source_executes_in_both_phases_and_lifecycles'
VARIANTS = ('application-fresh','macro-fresh','application-reused','macro-reused')

def expected():
    def named(kind,namespace):
        return {kind:{'namespace':namespace,'name':'x','spelling':(namespace+'/' if namespace else '')+'x'}}
    return {'id':ID,'outcome':{'status':'returned','value':{'$vector':[named('$symbol',None),named('$symbol','alpha'),named('$keyword',None),named('$keyword','alpha'),named('$keyword','user')]}},'trace':[],'stdout':''}

def load():
    raw, fields = sci.rows(sci.CORPUS)[SCI_ID]
    body, dispositions = sci.rows(sci.DISPOSITIONS)[SCI_ID]
    if (fields != dispositions[:7] or sci.digest(raw)!=ROW_HASH or sci.digest(body)!=BODY_HASH
            or fields[2]!=SOURCE or sci.digest(fields[2])!=SOURCE_HASH
            or not sci.report_module().same_json(sci.expected(fields),expected())):
        raise ValueError('Changed original complete source/row/disposition/expectation')
    return {'id':ID,'sci_id':SCI_ID,'obligation_id':'local::tests/reference/sci/evaluator-dispositions.tsv::'+SCI_ID,
        'source':SOURCE,'source_sha256':SOURCE_HASH,'original_source_sha256':SOURCE_HASH,
        'original_row_sha256':ROW_HASH,'body_sha256':BODY_HASH,'expected':expected(),
        'namespace':'user','source_name':'sci-original.cljs','forms':[{'kind':'runtime','source':SOURCE,'utf8_byte_start':0,'utf8_byte_end':len(SOURCE.encode())}],
        'reference_setup':sci.REFERENCE_SETUP,'comparison':sci.PROJECTION,
        'source_context_profile':{'schema_version':1,'profile':PROFILE}}

def validate(row):
    if not sci.report_module().same_json(row,load()):
        raise ValueError('Missing/changed exact original source context profile')
    return row

def module(phase):
    if phase=='application':return '(ns user)\n(defn run [] '+SOURCE+')\n'
    if phase=='macro':return "(ns user)\n(defmacro upstream [] (list 'quote "+SOURCE+'))\n(defn run [] (upstream))\n'
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
    paths=[Path(__file__),DRIVER,Path(__file__).with_name('test_sci_symbol_keyword_context.py'),
           Path(__file__).with_name('sci-symbol-keyword-context.md'), sci.CORPUS,sci.DISPOSITIONS,
           ROOT/'tests/support/sci_execution_observation.js',ROOT/context.ORACLE,
           ROOT/'tests/reference/execution.clj',ROOT/'tests/support/execution.js',
           Path(sci.__file__),Path(context.__file__),Path(runner.__file__),ROOT/'tests/reference/compiler-language/report.py']
    return {**context.compiler_sources(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}

def run_local(directory,timeout=240):
    row=load();directory=Path(directory).resolve()
    if directory.exists():raise ValueError('Fresh source audit artifacts required')
    directory.mkdir(parents=True);before=snapshot()
    request=directory/'input.tsv';output=directory/'observations.tsv'
    request.write_text(ID+'\t'+SOURCE.encode().hex()+'\t'+json.dumps(expected(),separators=(',',':')).encode().hex()+'\n')
    build,_=runner.run(['cargo','test','--locked','--test','compiler_sci_symbol_keyword_context','--no-run','--message-format=json'],directory/'build',timeout)
    artifacts=[json.loads(line) for line in build.splitlines() if line.startswith('{')]
    binaries=[Path(r['executable']) for r in artifacts if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')=='compiler_sci_symbol_keyword_context' and r.get('executable')]
    if len(binaries)!=1:raise ValueError('Missing/duplicate exact audit build')
    binary=binaries[0];binary_hash=sci.digest(binary.read_bytes());node=Path(shutil.which(os.environ.get('NODE','node')) or '').resolve()
    if not node.is_file():raise ValueError('Node unavailable')
    node_hash=sci.digest(node.read_bytes())
    env={**os.environ,'LIGHT_SCI_SYMBOL_KEYWORD_INPUT':str(request),'LIGHT_SCI_SYMBOL_KEYWORD_OUTPUT':str(output),'LIGHT_SCI_SYMBOL_KEYWORD_ROOT':str(directory/'modules')}
    stdout,_=runner.run([str(binary),TEST,'--ignored','--exact','--nocapture'],directory/'audit',timeout,environment=env)
    if 'running 1 test' not in stdout or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in stdout:
        raise ValueError('Actual source audit did not execute')
    frames={}
    for variant,engine,source,javascript,raw,warmup,observed in parse_raw(output.read_text(),row):
        frames[variant+'/'+engine]={'status':'passed','observed':observed,'phase_source_sha256':sci.digest(source),
            'compiled_js_sha256':sci.digest(javascript),'raw_observation_sha256':sci.digest(raw),
            'original_source_sha256':SOURCE_HASH,'original_row_sha256':ROW_HASH,'body_sha256':BODY_HASH,
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
    for kind in ('release','checkout'):
        layer=context.reference_cases([row],kind,directory/kind,timeout)[ID]
        if layer.get('status')!='passed':raise ValueError('Full original checked-pin source failed: '+kind)
        layer['build_stdout_sha256']=sci.digest((directory/kind/'build/stdout').read_bytes())
        layer['raw_observation_sha256']=sci.digest((directory/kind/ID/'stdout').read_bytes())
        layer['reference_javascript_sha256']=sci.digest((directory/kind/'reference.js').read_bytes())
        result['references'][kind]=layer
        print(kind,'passed',flush=True)
    runner.unchanged(before,snapshot());result.update(status='passed',catalog_credit=0,compiler_build_producer_source_sha256=before)
    (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n');return result

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path);parser.add_argument('--timeout',type=int,default=240);args=parser.parse_args()
    if args.output:run(args.output,args.timeout);print('Original source: eight actual phase frames and both checked pins passed')
    else:load();print('Exact original source profile validated; no execution credit')
if __name__=='__main__':main()
