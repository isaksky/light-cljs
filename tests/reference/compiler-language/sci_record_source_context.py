#!/usr/bin/env python3
"""Three unchanged SCI record sources with explicit canonical core-name shadows.

No default-header pass is inferred. Both pins must retain the original collision
control as a TypeError and pass the narrowly scoped namespace profile.
"""
from __future__ import annotations
import argparse,copy,json,os,shutil
from pathlib import Path
import configured_source_context as runner
import sci_execution as sci
import sci_source_context as context
import sci_symbol_keyword_context as packets
from extract_inventory import Reader
ROOT=sci.ROOT
PROFILE='original-sci-record-core-name-shadow-v1'
DRIVER=ROOT/'tests/compiler_sci_record_source_context.rs'
TEST='original_record_contexts_execute_in_both_phases_and_lifecycles'
VARIANTS=('application-fresh','macro-fresh','application-reused','macro-reused')
SPECS=(
 ('sci-record-constructors','Box','8ff380b17957e6515682829266d131111cd9bb7ea5bd19e2b94eb3ac34f55549','c4d89867551ff0ecbd1a190d03a1237e2c5dcebb0dbea439706eb071ba4d30dd','0c508ab0ed787bd9683d6753ed0d8aca95b2123141f83e772f53158fb7ea990e'),
 ('sci-record-empty','Empty','f7655968e1a612cf943fc7230754f51e248b6b2f2c1d6f5ae144d03eccf19aa1','ee951336b6f410ce51c0e1cecee2493f31bf360d3b3e9cdd45dd13d308cfd2cf','89b053b16bb8829d433aabfec897c5d98b15ff8177b10d2c722134b8bceb35c0'),
 ('sci-record-method-acceptance','Box','80cfabeac230b96f9e1e18cd209b9f00615e4ef0589795515bb90a4e57bd7760','8450eb70a371b5c9ae72247cefa6d11851bf1f5a5c4e37d2811864ffd1757b1e','e16a87c33abae7248c0a45d2e9fe212060c62b217224ecbc6121975ac75b9634'))

def load():
 corpus=sci.rows(sci.CORPUS);dispositions=sci.rows(sci.DISPOSITIONS);rows=[]
 for identity,excluded,source_hash,row_hash,body_hash in SPECS:
  raw,fields=corpus[identity];body,disposition=dispositions[identity]
  if fields!=disposition[:7] or sci.digest(raw)!=row_hash or sci.digest(body)!=body_hash or sci.digest(fields[2])!=source_hash:
   raise ValueError('Changed original whole source/SCI expectation/disposition')
  original=fields[2];nodes=Reader(original).all();last=nodes[-1]
  rows.append({'id':'sci-execution-'+identity,'sci_id':identity,'obligation_id':'local::tests/reference/sci/evaluator-dispositions.tsv::'+identity,
    'source':original,'source_sha256':source_hash,'original_source_sha256':source_hash,'original_row_sha256':row_hash,'body_sha256':body_hash,
    'expected':sci.expected(fields),'namespace':'user','setup_source':original[:last.start],'runtime_source':original[last.start:last.end],
    'source_context_profile':{'schema_version':1,'profile':PROFILE,'namespace':'user','excluded_core_identifiers':[excluded]},
    'default_header':'(ns user)','profile_header':'(ns user (:refer-clojure :exclude ['+excluded+']))',
    'comparison':sci.PROJECTION})
 return rows

def validate(rows):
 if not sci.report_module().same_json(rows,load()):raise ValueError('Missing/reordered/drifted original record source/profile/expectation')
 return rows

def module(row,phase,default=False):
 if row not in load():raise ValueError('Unknown original record source')
 header=row['default_header'] if default else row['profile_header']
 setup=row['setup_source'];body=row['runtime_source']
 if setup+body!=row['source']:raise ValueError('Incomplete original source setup/runtime split')
 if phase=='application':return header+'\n'+setup+'(defn run [] '+body+')\n'
 if phase=='macro':return header+'\n'+setup+"(defmacro upstream [] (list 'quote "+body+'))\n(defn run [] (upstream))\n'
 raise ValueError('Unknown phase')

def parse_raw(text,rows):
 validate(rows);records=[line.split('\t') for line in text.splitlines()]
 order=[(r['id'],v,e) for r in rows for v in VARIANTS for e in ('QuickJS','Node')]
 if len(records)!=24 or any(len(r)!=7 for r in records) or [tuple(r[:3]) for r in records]!=order:
  raise ValueError('Missing/duplicate/reordered original record frames')
 by_id={r['id']:r for r in rows};result=[]
 for identity,variant,engine,source_hex,js_hex,raw_hex,warmup_hex in records:
  source,javascript,raw=[bytes.fromhex(v).decode() for v in (source_hex,js_hex,raw_hex)]
  row=by_id[identity]
  if source!=module(row,variant.split('-')[0]) or javascript.count(' = // Original bounded runtime')!=1:
   raise ValueError('Changed original declaration/body/header/runtime')
  if (warmup_hex=='-')!=variant.endswith('fresh') or (variant.endswith('reused') and not bytes.fromhex(warmup_hex)):
   raise ValueError('Missing actual reused-host warmup')
  observed=packets.strict_json(raw)
  if not sci.report_module().same_json(observed,row['expected']):raise ValueError('Changed original result/effects/output')
  if engine=='Node' and raw!=json.dumps(observed,separators=(',',':'))+'\n':raise ValueError('Extra/malformed Node output')
  result.append((identity,variant,engine,source,javascript,raw,warmup_hex,observed))
 for left,right in zip(result[::2],result[1::2]):
  if left[3:5]!=right[3:5] or left[6]!=right[6]:raise ValueError('Engine frames detached from one compilation')
 return result

def validate_control(layer,kind):
 expected={'status':'threw','error':{'category':'type-error'}}
 if (layer.get('status')!='mismatch' or not sci.report_module().same_json(layer.get('observed',{}).get('outcome'),expected)
     or layer.get('observed',{}).get('trace')!=[] or layer.get('observed',{}).get('stdout')!=''
     or layer.get('reference_identity')!=kind+' '+(sci.report_module().RELEASE if kind=='release' else sci.report_module().REVISION)
     or not sci.report_module().same_json(layer.get('settings'),{'optimizations':'none','static-fns':False,'def-emits-var':False})):
  raise ValueError('Original default-header collision control did not reject faithfully')
 return layer

def snapshot():
 paths=[Path(__file__),DRIVER,Path(__file__).with_name('test_sci_record_source_context.py'),Path(__file__).with_name('sci-record-source-context.md'),
   sci.CORPUS,sci.DISPOSITIONS,ROOT/'tests/support/sci_execution_observation.js',ROOT/'tests/support/execution.js',ROOT/'tests/reference/execution.clj',
   Path(runner.__file__),Path(sci.__file__),Path(context.__file__),Path(packets.__file__),ROOT/'tests/reference/compiler-language/report.py']
 return {**context.compiler_sources(),**{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}

def run_local(directory,timeout=240):
 rows=load();directory=Path(directory).resolve()
 if directory.exists():raise ValueError('Fresh source audit required')
 directory.mkdir(parents=True);before=snapshot();request=directory/'input.tsv';output=directory/'observations.tsv'
 request.write_text(''.join(r['id']+'\t'+r['source'].encode().hex()+'\t'+json.dumps(r['expected'],separators=(',',':')).encode().hex()+'\n' for r in rows))
 build,_=runner.run(['cargo','test','--locked','--test','compiler_sci_record_source_context','--no-run','--message-format=json'],directory/'build',timeout)
 artifacts=[json.loads(line) for line in build.splitlines() if line.startswith('{')]
 binaries=[Path(r['executable']) for r in artifacts if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')=='compiler_sci_record_source_context' and r.get('executable')]
 if len(binaries)!=1:raise ValueError('Missing/duplicate exact audit executable')
 binary=binaries[0];binary_hash=sci.digest(binary.read_bytes());node=Path(shutil.which(os.environ.get('NODE','node')) or '').resolve()
 if not node.is_file():raise ValueError('Node unavailable')
 node_hash=sci.digest(node.read_bytes());env={**os.environ,'LIGHT_SCI_RECORD_CONTEXT_INPUT':str(request),'LIGHT_SCI_RECORD_CONTEXT_OUTPUT':str(output),'LIGHT_SCI_RECORD_CONTEXT_ROOT':str(directory/'modules')}
 stdout,_=runner.run([str(binary),TEST,'--ignored','--exact','--nocapture'],directory/'audit',timeout,environment=env)
 if 'running 1 test' not in stdout or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in stdout:raise ValueError('Exact audit not executed')
 frames={r['id']:{} for r in rows}
 for identity,variant,engine,source,javascript,raw,warmup,observed in parse_raw(output.read_text(),rows):
  frames[identity][variant+'/'+engine]={'status':'passed','observed':observed,'phase_source_sha256':sci.digest(source),'compiled_js_sha256':sci.digest(javascript),
   'raw_observation_sha256':sci.digest(raw),'warmup_compiled_js_sha256':None if warmup=='-' else sci.digest(bytes.fromhex(warmup)),
   'compiler_build_producer_source_sha256':before,'audit_executable_sha256':binary_hash,'node_executable_sha256':node_hash,'build_stdout_sha256':sci.digest(build)}
 runner.unchanged(before,snapshot())
 if sci.digest(binary.read_bytes())!=binary_hash or sci.digest(node.read_bytes())!=node_hash:raise ValueError('Execution binary drift')
 result={'status':'passed','source_bindings':rows,'frames':frames,'compiler_build_producer_source_sha256':before,'raw_observations_sha256':sci.digest(output.read_bytes()),'catalog_credit':0,'generic_default_header':'not passing'}
 (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n');return result

def default_collision_control(row,artifact,kind,failed_layer,timeout):
 identity=kind+' '+(sci.report_module().RELEASE if kind=='release' else sci.report_module().REVISION)
 build=(artifact/'build/stdout').read_text()
 if 'REFERENCE IDENTITY '+identity not in build or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in build:
  raise ValueError('Default collision pin/options not freshly proved')
 name=row['source_context_profile']['excluded_core_identifiers'][0]
 compiled=(artifact/'out/user.js').read_text()
 if 'cljs.core.'+name+' =' not in compiled or 'user.'+name+'.prototype' not in compiled:
  raise ValueError('Original constructor/prototype collision not present')
 script='process.chdir('+json.dumps(str(artifact))+'); const id='+json.dumps(row['id'])+'; let observed; try { require('+json.dumps(str(artifact/'reference.js'))+'); observed={id,outcome:{status:"returned",value:true},trace:[],stdout:""}; } catch(error) { observed={id,outcome:{status:"threw",error:{category:error instanceof TypeError?"type-error":"other"}},trace:[],stdout:"",original_error:{name:error.name,message:error.message,stack:error.stack}}; } console.log(JSON.stringify(observed));'
 raw,stderr=runner.run([os.environ.get('NODE','node'),'-'],artifact/'direct-loader-control',timeout,source=script)
 if stderr:raise ValueError('Unexpected default loader output')
 observed=packets.strict_json(raw);error=observed.pop('original_error',None)
 if not error or error.get('name')!='TypeError' or 'prototype' not in error.get('message','') or str(artifact/'out/user.js') not in error.get('stack',''):
  raise ValueError('Default source initialization did not retain original prototype TypeError')
 layer={'status':'mismatch','observed':observed,'reference_identity':identity,'settings':{'optimizations':'none','static-fns':False,'def-emits-var':False},
  'original_error':error,'direct_raw_observation_sha256':sci.digest(raw),'direct_control_source_sha256':sci.digest(script),'default_compiled_namespace_sha256':sci.digest(compiled),
  'program_observer_failure':failed_layer}
 return validate_control(layer,kind)

def run(directory,timeout=240):
 directory=Path(directory).resolve()
 if directory.exists():raise ValueError('Fresh complete evidence required')
 directory.mkdir(parents=True);before=snapshot();rows=load()
 result={'source_bindings':rows,'local':run_local(directory/'local',timeout),'references':{},'default_header_controls':{}}
 for kind in ('release','checkout'):
  result['references'][kind]={};result['default_header_controls'][kind]={}
  for row in rows:
   for mode,default in [('profile',False),('default',True)]:
    root=directory/'reference-source'/kind/mode/row['id'];root.mkdir(parents=True);source=module(row,'application',default);(root/'user.cljs').write_text(source)
    case={'id':row['id'],'source':source,'source_root':str(root),'entry':'user','export':'run','expected':row['expected']}
    before_source=sci.digest((root/'user.cljs').read_bytes())
    layer=sci.report_module().reference_cases([case],kind,ROOT/'research/clojurescript',directory/kind/mode,timeout)[row['id']]
    artifact=directory/kind/mode/row['id']
    if default:layer=default_collision_control(row,artifact,kind,layer,timeout)
    elif layer.get('status')!='passed':raise ValueError('Exact original record profile failed: '+kind+' '+row['id'])
    layer.update(source_sha256=row['source_sha256'],namespace_profile=({'schema_version':1,'profile':'original-default-header-collision-control','namespace':'user','excluded_core_identifiers':[]} if default else copy.deepcopy(row['source_context_profile'])),phase_source_sha256=before_source,
     build_stdout_sha256=sci.digest((artifact/'build/stdout').read_bytes()),build_stderr_sha256=sci.digest((artifact/'build/stderr').read_bytes()),
     raw_observation_sha256=sci.digest((artifact/'node/stdout').read_bytes()),reference_javascript_sha256=sci.digest((artifact/'reference.js').read_bytes()))
    if before_source!=sci.digest((root/'user.cljs').read_bytes()):raise ValueError('Reference source drift')
    result['default_header_controls' if default else 'references'][kind][row['id']]=layer
  print(kind,'three exact profiles and three rejecting controls',flush=True)
 runner.unchanged(before,snapshot());result.update(status='passed',catalog_credit=0,generic_default_header='rejected: no pass inferred',compiler_build_producer_source_sha256=before)
 (directory/'evidence.json').write_text(json.dumps(result,indent=2)+'\n');return result

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path);p.add_argument('--timeout',type=int,default=240);a=p.parse_args()
 if a.output:run(a.output,a.timeout);print('Three unchanged originals,24frames,bothpins and6rejecting controls passed')
 else:load();print('Exact record source profiles validated; no execution credit')
if __name__=='__main__':main()
