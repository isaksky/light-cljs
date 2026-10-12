#!/usr/bin/env python3
"""Exact unchanged whole-source both-pin/eight-context gap replay; zero credit."""
from pathlib import Path
import argparse,copy,json,os,subprocess,tempfile
import sci_originals_after194_discovery as originals
import sci_originals_after194_phase_discovery as phases
import sci_originals_after194_source_discovery as sources
import sci_gap_canonical_context as phase
import sci_execution as sci
IDS=('sci-record-constructor-metadata','sci-record-method-duplicate-parameter')


def same(a,b,message):
    if a!=b:raise ValueError(message)


def census(root):return {str(p.relative_to(root)):sci.digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file() and p.name!='evidence.json'}


def frames_from_raw(row,raw):
    frames=[]
    for line in raw.splitlines():
        identity,variant,engine,source_hex,js_hex,packet_hex,warmup_hex=line.split('\t')
        source=bytes.fromhex(source_hex).decode();js=bytes.fromhex(js_hex)
        same(identity,row['id'],'Detached original gap raw source identity')
        same(source,phase.module(row,variant.split('-')[0]),'Whole original gap raw phase source drift')
        frames.append({'variant':variant,'engine':engine,'whole_phase_source':source,
          'whole_phase_source_sha256':sci.digest(source),'emitted_js':js.decode(),
          'emitted_js_sha256':sci.digest(js),'raw_packet':json.loads(bytes.fromhex(packet_hex).decode()),
          'warmup_js_sha256':None if warmup_hex=='-' else sci.digest(bytes.fromhex(warmup_hex))})
    same([(f['variant'],f['engine']) for f in frames],[(v,e) for v in phase.VARIANTS for e in ('QuickJS','Node')][:len(frames)],'Original gap actual frame order drift')
    return frames


def precheck(phase_root,source_root,pd,sd):
    same(pd['source_snapshot'],phases.snapshot(),'Current whole-original phase producer/source drift')
    same(sd['source_snapshot'],sources.snapshot(),'Current whole-original top-level producer/source drift')
    same(pd['artifact_sha256'],census(phase_root),'Complete31actual-phase raw artifact census drift')
    same(sd['artifact_sha256'],census(source_root),'Complete31top-level/source/pin raw artifact census drift')
    rows=originals.load();same(len(pd['evidence']),len(rows),'Full31actual-source diagnostic cohort omitted')
    same(sd['source_bindings'],sources.load(),'Complete original top-level source/expectations detached')
    same(pd['reference_execution_binding'] if 'reference_execution_binding' in pd else originals.prior.reference_execution_binding(),originals.prior.reference_execution_binding(),'Actual source reference executable drift')
    same(sd['reference_execution_binding'],originals.prior.reference_execution_binding(),'Original source pin executable drift')
    same(sci.digest(Path(pd['actual_executable']).read_bytes()),pd['actual_executable_sha256'],'Current actual source/phase executable changed')
    for row in rows:
        record=pd['evidence'][row['id']]
        same(record['source_binding'],row,'Whole-original source/expectations detached')
        frames=frames_from_raw(row,(phase_root/row['sci_id']/'observations.tsv').read_text())
        same(record['complete_raw_frames'],frames,'Complete original QuickJS/Node receipt frames detached from raw input')
        same(record['observed_frames'],len(frames),'Whole original partial frame count drift')


def controls(phase_root,source_root):
    phase_root=Path(phase_root);source_root=Path(source_root)
    pd=json.loads((phase_root/'evidence.json').read_text());sd=json.loads((source_root/'evidence.json').read_text())
    precheck(phase_root,source_root,pd,sd);results=[]
    row=next(r for r in originals.load() if r['sci_id']==IDS[0])
    changes=[('changed-original-expected',lambda p,s:p['evidence'][row['id']]['source_binding'].update(expected={'status':'returned','value':True})),
      ('changed-whole-source',lambda p,s:p['evidence'][row['id']]['source_binding'].update(source='true')),
      ('changed-QuickJS-full-packet',lambda p,s:p['evidence'][row['id']]['complete_raw_frames'][0].update(raw_packet={'status':'fabricated'})),
      ('missing-actual-engine-frame',lambda p,s:p['evidence'][row['id']]['complete_raw_frames'].pop()),
      ('changed-current-executable',lambda p,s:p.update(actual_executable_sha256='0'*64)),
      ('changed-original-source-pin-binding',lambda p,s:s['source_bindings'][0].update(source='true')),
      ('changed-raw-pin-census',lambda p,s:s['artifact_sha256'].update({next(iter(s['artifact_sha256'])):'0'*64})),
      ('changed-current-producer',lambda p,s:p['source_snapshot'].update({next(iter(p['source_snapshot'])):'0'*64}))]
    for name,edit in changes:
        changed_p=copy.deepcopy(pd);changed_s=copy.deepcopy(sd);edit(changed_p,changed_s)
        try:precheck(phase_root,source_root,changed_p,changed_s)
        except (ValueError,KeyError) as error:results.append({'id':name,'status':'rejected','reason':str(error)})
        else:raise ValueError('Accepted altered original gap evidence '+name)
    for label,root,relative in [('actual-raw-restoration',phase_root,row['sci_id']+'/observations.tsv'),('pin-raw-restoration',source_root,'release/'+row['id']+'/stdout')]:
        path=root/relative;original=path.read_bytes()
        try:
            path.write_bytes(original+b' ')
            try:precheck(phase_root,source_root,pd,sd)
            except ValueError as error:results.append({'id':label,'status':'rejected','reason':str(error)})
            else:raise ValueError('Accepted altered original gap raw artifact')
        finally:path.write_bytes(original)
    precheck(phase_root,source_root,pd,sd)
    return {'controls':results,'pristine_complete_inputs_restored':'passed','shared_source_modified':False,'original_semantic_pass_credit':0}


def validate(phase_root,source_root):
    phase_root=Path(phase_root).resolve();source_root=Path(source_root).resolve();pd=json.loads((phase_root/'evidence.json').read_text());sd=json.loads((source_root/'evidence.json').read_text())
    precheck(phase_root,source_root,pd,sd)
    rows={r['sci_id']:r for r in originals.load()};same(len(pd['evidence']),len(rows),'Full31actual-source diagnostic cohort omitted')
    same(len(sd['source_bindings']),len(rows),'Full31top-level-source diagnostic cohort omitted')
    binary=pd['actual_executable'];same(sci.digest(Path(binary).read_bytes()),pd['actual_executable_sha256'],'Current actual source/phase executable changed')
    built=subprocess.run(['cargo','test','--locked','--test',phase.DRIVER.stem,'--no-run','--message-format=json'],cwd=sci.ROOT,capture_output=True,text=True,timeout=600)
    if built.returncode:raise ValueError('Current Cargo gap executable replay build failed')
    artifacts=[json.loads(l) for l in built.stdout.splitlines() if l.startswith('{')]
    same([a['executable'] for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')==phase.DRIVER.stem and a.get('executable')],[binary],'Stored gap audit not current Cargo executable')
    programs=originals.prior.reference_execution_binding();result={}
    with tempfile.TemporaryDirectory(prefix='sci-original-gap-after194-independent-') as temporary:
        for identity in IDS:
            row=rows[identity];record=pd['evidence'][row['id']];same(record['source_binding'],row,'Whole-original gap source/expectations detached')
            same(record['status'],'observed-all-eight-contexts','Incomplete actual original gap contexts')
            fresh=Path(temporary)/identity;fresh.mkdir();(fresh/'input.tsv').write_text(row['id']+'\t'+row['source'].encode().hex()+'\n')
            command=[binary,phase.TEST,'--ignored','--exact','--nocapture'];ran=subprocess.run(command,cwd=sci.ROOT,env={**os.environ,'PYTHONDONTWRITEBYTECODE':'1','LIGHT_SCI_GAP_PHASE_ROOT':str(fresh),'NODE':programs['programs']['node']},capture_output=True,text=True,timeout=300)
            if ran.returncode or ran.stdout.count('test result: ok. 1 passed; 0 failed; 0 ignored;')!=1:raise ValueError('Independent complete original gap phase execution failed')
            for name in ('observations.tsv','warmup.js'):same((fresh/name).read_bytes(),(phase_root/identity/name).read_bytes(),'Whole original gap emitted/source/raw replay differs')
            frames=record['complete_raw_frames'];same(len(frames),8,'Whole original gap missing any actual context')
            actual_packets={}
            for frame in frames:
                source=phase.module(row,frame['variant'].split('-')[0]);same(frame['whole_phase_source'],source,'Whole original gap phase source changed')
                actual_packets[frame['variant']+'/'+frame['engine']]=frame['raw_packet']
                if frame['engine']=='Node':
                    js=frame['emitted_js'];invocation='('+js+')(rt)' if frame['variant'].startswith('macro') else '('+js+')(rt)(observe)'
                    script='(() => {const rt='+(sci.ROOT/'src/js_runtime.js').read_text()+"; return ("+(sci.ROOT/'tests/support/sci_execution_observation.js').read_text()+")(rt)('"+row['id']+"', observe=>"+invocation+");})()"
                    raw=subprocess.run([programs['programs']['node'],'-'],cwd=sci.ROOT,input='console.log('+script+');',capture_output=True,text=True,timeout=60)
                    same(raw.returncode,0,'Actual whole original gap Node input replay failed');same(raw.stderr,'','Actual gap Node stderr changed');same(json.loads(raw.stdout),frame['raw_packet'],'Actual whole original gap complete packet differs from raw Node replay')
            pins={}
            for kind in ('release','checkout'):
                folder=source_root/kind/row['id'];command=json.loads((folder/'invocation.json').read_text())
                same(command,[os.environ.get('NODE','node'),'-'],'Changed original source pin Node invocation')
                raw=subprocess.run([programs['programs']['node'],'-'],cwd=sci.ROOT,input=(folder/'stdin').read_text(),capture_output=True,text=True,timeout=60)
                same(raw.returncode,0,'Original source pin independent Node replay failed');same(raw.stdout,(folder/'stdout').read_text(),'Complete original pin raw result/effects/stdout differs');same(raw.stderr,(folder/'stderr').read_text(),'Complete original pin stderr/source frames differ')
                packet=json.loads(raw.stdout)
                pins[kind]={'layer':sd['evidence'][row['id']][kind],'complete_raw_packet':packet,'raw_stdout':raw.stdout,'raw_stderr':raw.stderr,'raw_script_sha256':sci.digest((folder/'stdin').read_bytes())}
            same(pins['release']['complete_raw_packet'],pins['checkout']['complete_raw_packet'],'Both original user/top-level pin complete packets differ')
            result[identity]={'original_source_binding':row,'complete_actual_contexts':frames,'complete_actual_packets':actual_packets,'both_default_pins':pins,
              'complete_top_level_local_layer':sd['evidence'][row['id']]['javascript'],'original_semantic_pass_credit':0,
              'scope':'Distinct observed original public behavior; full source and all historical expectations immutable; independent compiler executable and exact raw replay. No repair or focused owner created.'}
    same(pd['source_snapshot'],phases.snapshot(),'Actual original source/phase inputs changed during independent replay');same(sd['source_snapshot'],sources.snapshot(),'Original source pin inputs changed during independent replay')
    return {'schema_version':1,'scope':'Two distinct complete original public behavior discrepancies; diagnostic/zero SCI credit',
      'producer_source_sha256':sci.digest(Path(__file__).read_bytes()),'complete31_phase_receipt_sha256':sci.digest((phase_root/'evidence.json').read_bytes()),
      'complete31_source_receipt_sha256':sci.digest((source_root/'evidence.json').read_bytes()),'phase_root':str(phase_root),'source_root':str(source_root),
      'actual_executable':binary,'actual_executable_sha256':pd['actual_executable_sha256'],'reference_execution_binding':programs,
      'gaps':result,'mutation_restoration_controls':controls(phase_root,source_root),'original_semantic_pass_credit':0,'strict_complete':False}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('phases','sources','output'):parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args();receipt=validate(args.phases,args.sources);args.output.write_text(json.dumps(receipt,indent=2)+'\n');print('Both unchanged original gaps independently replayed; zero credit',flush=True)
