#!/usr/bin/env python3
"""Parent integration API for only the complete original integer assertion site.

Registration never supplies a pass. Every reconciliation reruns current whole
source/compiler/actual executable/both pin replay and checks detached mappings.
"""
from pathlib import Path
import argparse,copy,json
import integer_original_source_after194 as whole
import integer_predicate_after192 as actual

ROOT=whole.ROOT
SITE=whole.SITE
PROFILE=whole.PROFILE


def current_binding():
    paths=[Path(__file__),Path(__file__).with_name('test_integer_source_context_after194.py')]
    return {'source_snapshot':whole.snapshot(),
      'reconciliation_source_sha256':{str(p.relative_to(ROOT)):actual.digest(p.read_bytes()) for p in paths},
      'execution_binding':whole.programs(),'source_binding':whole.binding()}


def mapping():
    return {'assertion_id':SITE,'assertion_sha256':whole.SITE_HASH,'body_id':whole.BODY,
      'body_sha256':whole.BODY_HASH,'complete_setup_sha256':whole.TABLE_HASH,
      'fixture_id':whole.ID,'profile':PROFILE,'new_public_or_local_arity_links':0}


def precheck(document):
    whole.same(document.get('profile'),PROFILE,'Wrong complete original integer source profile')
    whole.same(document.get('mapping'),mapping(),'Detached original integer site/body/table mapping')
    whole.same(document.get('current_binding'),current_binding(),'Current original integer producer/source/executable binding drift')
    proof=document.get('whole_original_proof',{})
    executable=proof.get('complete192_acceptance',{})
    binary=Path(executable.get('actual_executable',''))
    if not binary.is_file():raise ValueError('Current original integer actual executable unavailable')
    whole.same(executable.get('actual_executable_sha256'),actual.digest(binary.read_bytes()),'Current original integer actual executable changed')
    whole.same(proof.get('source_binding'),whole.binding(),'Changed original integer source body/site/table contract')
    whole.same(proof.get('source_snapshot'),whole.snapshot(),'Current original integer proof producer drift')
    if document.get('strict_complete') is not False:raise ValueError('Narrow original source reconciliation claimed full gate completion')


def bind(proof):
    return {'schema_version':1,'profile':PROFILE,'status':'passed-source-reconciliation',
      'mapping':mapping(),'current_binding':current_binding(),'whole_original_proof':proof,
      'original_site_count':1,'original_loop_comparisons':112,'public_local_arity_links_added':0,'strict_complete':False}


def run(directory,timeout=600,*,controls=True):
    if timeout<600:raise ValueError('Whole original source requires full current compiler/pin replay budget')
    directory=Path(directory).resolve();before=current_binding();proof=whole.execute(directory)
    whole.same(current_binding(),before,'Original source binding changed during parent collector')
    document=bind(proof)
    if controls:
        raw=whole.controls(directory);(directory/'raw-controls.json').write_text(json.dumps(raw,indent=2)+'\n')
        proof_controls=document_controls(document);(directory/'reconciliation-controls.json').write_text(json.dumps(proof_controls,indent=2)+'\n')
    (directory/'source-context.json').write_text(json.dumps(document,indent=2)+'\n');return document


def reconcile(document,*,execution_layers=None):
    precheck(document)
    source=document['whole_original_proof'];root=Path(source['artifact_root'])
    replay=whole.validate(root)
    whole.same(replay,source,'Supplied original integer proof detached from complete current raw/executable replay')
    whole.same(bind(replay),document,'Supplied complete original integer reconciliation changed')
    if execution_layers is not None:
        whole.same(execution_layers,source['complete192_acceptance']['references'],'Original integer pin layers detached from current report execution')
    frames={}
    for slot,packet in source['actual_complete_literal_packets'].items():
        host,phase,engine=slot.split('/');variant=phase+'-'+host
        folder=root/'actual'/host/phase
        frames[variant+'/'+engine]={'status':'passed','variant':variant+'/'+engine,'observed':copy.deepcopy(packet),
          'source_sha256':actual.digest((folder/'integer_predicate_after192.cljs').read_bytes()),
          'compiled_js_sha256':actual.digest((folder/'compiled.js').read_bytes()),
          'raw_observation_sha256':actual.digest((folder/('quickjs.stdout' if engine=='QuickJS' else 'node.stdout')).read_bytes()),
          'test_file':actual.DRIVER,'test_name':actual.TEST,
          'actual_executable_sha256':source['complete192_acceptance']['actual_executable_sha256']}
    whole.same(set(frames),{phase+'-'+host+'/'+engine for phase in ('application','macro') for host in ('fresh','reused') for engine in ('QuickJS','Node')},'Incomplete current original source phase frames')
    return {SITE:{'status':'covered','reconciliation':'verified-complete-original-integer-source',
      'fixture_id':whole.ID,'profile':PROFILE,'source_binding':copy.deepcopy(source['source_binding']),
      'mapping':mapping(),'complete_literal_outcome_effect_stdout_packets':copy.deepcopy(source['actual_complete_literal_packets']),
      'references':copy.deepcopy(source['complete192_acceptance']['references']),
      'independent_pin_replay':copy.deepcopy(source['independent_pin_replay']),
      'actual_phase_frames':frames,'original_assertion_semantic_pass_credit':1,
      'source_scope':'One original source assertion, all112comparisons; initialized genuine Closure profile',
      'new_public_or_local_arity_links':0,'strict_complete':False}}


def document_controls(document):
    precheck(document);results=[]
    tests=[]
    def changed(name,edit):
        value=copy.deepcopy(document);edit(value);tests.append((name,value))
    changed('detached-site-mapping',lambda d:d['mapping'].update(assertion_id=SITE+'-wrong'))
    changed('changed-site-hash',lambda d:d['mapping'].update(assertion_sha256='0'*64))
    changed('changed-complete-body-hash',lambda d:d['mapping'].update(body_sha256='0'*64))
    changed('changed-complete-table-hash',lambda d:d['mapping'].update(complete_setup_sha256='0'*64))
    changed('changed-current-producer',lambda d:d['current_binding']['reconciliation_source_sha256'].update({str(Path(__file__).relative_to(ROOT)):'0'*64}))
    changed('changed-current-executable',lambda d:d['whole_original_proof']['complete192_acceptance'].update(actual_executable_sha256='0'*64))
    changed('changed-original-source-contract',lambda d:d['whole_original_proof']['source_binding']['complete_original_body'].update(source='(deftest test-int-preds (is true))'))
    for name,value in tests:
        try:precheck(value)
        except (ValueError,KeyError) as error:results.append({'id':name,'status':'rejected','reason':str(error)})
        else:raise AssertionError('Detached/current original-source mutation accepted '+name)
    # Full raw/input/emitted/pin tamper controls are separate whole.controls.
    # This layer never uses a mutated upstream/production/shared ledger input.
    precheck(document)
    return {'schema_version':1,'controls':results,'pristine_binding_restored':'passed',
      'shared_original_source_modified':False,'strict_complete':False}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--run',type=Path);parser.add_argument('--reconcile',type=Path);parser.add_argument('--bind',type=Path);args=parser.parse_args()
    if args.run:document=run(args.run)
    elif args.bind:
        proof=whole.validate(args.bind);document=bind(proof);(args.bind/'source-context.json').write_text(json.dumps(document,indent=2)+'\n')
        (args.bind/'reconciliation-controls.json').write_text(json.dumps(document_controls(document),indent=2)+'\n')
    elif args.reconcile:
        document=json.loads(args.reconcile.read_text());result=reconcile(document);print(json.dumps({'site_status':{k:v['status'] for k,v in result.items()},'strict_complete':False}));raise SystemExit(0)
    else:parser.error('One --run/--bind/--reconcile input required')
    print(document['status'],flush=True)
