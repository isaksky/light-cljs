"""Diagnostic-only complete whole-original phase and independent pin replay."""
from pathlib import Path
from types import SimpleNamespace
import importlib.util,json,sys,copy
import sci_originals_after191 as frozen
import sci_execution as sci
import sci_gap_canonical_context as proof
import sci_finite_after185_context as executable
HERE=Path(__file__).resolve().parent
SOURCES=HERE/'sci-originals-after191-collection-promising-sources.json'
SHA='3a83290523141987d41d1f9ef2a29f7fa910a2fc063fbfe42bdf7a420448b5ad'
PROFILE='collection-whole-original-after191-discovery-v1'

def original_expected(fields):
    outcome=sci.wire(fields[3])
    if outcome[0]=='return':return sci.expected(fields)
    if len(outcome)!=3 or outcome[0]!='error' or outcome[1]!='execution' or outcome[2] not in {'type','arity','bounds'}:
        raise ValueError('Changed exact SCI structured collection phase/category')
    effects=sci.value(sci.wire(fields[4]))
    if set(effects)!={'$vector'} or sci.value(sci.wire(fields[6]))!='':raise ValueError('Changed original SCI streams/effects')
    return {'id':'sci-execution-'+fields[0],'outcome':{'status':'sci-error','phase':outcome[1],'category':outcome[2]},'trace':effects['$vector'],'stdout':sci.value(sci.wire(fields[5]))}


def load():
    proof.same(sci.digest(SOURCES.read_bytes()),SHA,'Changed additional whole source diagnostic table')
    rows=json.loads(SOURCES.read_text());original=sci.rows(sci.CORPUS);dispositions=sci.rows(sci.DISPOSITIONS)
    for row in rows:
        line,fields=original[row['sci_id']];body,disp=dispositions[row['sci_id']]
        proof.same(row['original_fields'],fields,'Changed whole original sevenfields')
        proof.same(row['source'],fields[2],'Changed whole original source')
        proof.same(row['original_expected'],original_expected(fields),'Changed original typed packet/effects')
        proof.same(row['source_sha256'],sci.digest(fields[2]),'Changed original sourcehash')
        proof.same(row['body_sha256'],sci.digest(body),'Changed original disposition body')
        proof.same(row['original_row_sha256'],sci.digest(line),'Changed originalrow')
        if disp[:7]!=fields or disp[8]!=fields[2] or disp[7]!='gap':raise ValueError('Changed complete portable input')
    proof.same(len(rows),21,'Collection diagnostic cohort drift')
    if {r['sci_id']for r in rows}&set(frozen.candidates.IDS):raise ValueError('Overlap with frozen ten')
    return rows


def originals():
    return [{k:v for k,v in row.items()if k not in ('original_expected','canonical_expected','source_context_profile','original_semantic_pass_credit','original_expectation_status','reason','scope_basis','focused_owner')}for row in load()]


def snapshot():
    paths=[Path(__file__),SOURCES,HERE/'sci_originals_after191_collection_discovery.py']
    return {**frozen.snapshot(),**{str(p.relative_to(sci.ROOT)):sci.digest(p.read_bytes())for p in paths}}


candidates=SimpleNamespace(load=originals,snapshot=snapshot,reference_execution_binding=frozen.candidates.reference_execution_binding)

def run(root):
    root=Path(root).resolve();root.mkdir(exist_ok=False);before=snapshot();programs=candidates.reference_execution_binding()
    cases=originals();reference=root/'originals';reference.mkdir();evidence={r['id']:{}for r in cases}
    for identity,layer in sci.local_cases(cases,reference/'local',300).items():evidence[identity]['javascript']=layer
    for kind in ('release','checkout'):
        for identity,layer in sci.reference_cases(cases,kind,sci.ROOT/'research/clojurescript',reference/kind,600).items():evidence[identity][kind]=layer
    manifest={str(p.relative_to(reference)):sci.digest(p.read_bytes())for p in sorted(reference.rglob('*'))if p.is_file()}
    ref={'schema_version':1,'cases':cases,'producer_sha256':before,'artifact_sha256':manifest,'evidence':evidence,'reference_execution_binding':programs,'semantic_pass_credit':False,'macro_phase_status':'not-run'}
    (reference/'evidence.json').write_text(json.dumps(ref,indent=2)+'\n')
    proof.execute(root/'phases',300,cohort=sys.modules[__name__])
    document=proof.verified_document(reference/'evidence.json',root/'phases',cohort=sys.modules[__name__])
    executable.executable_replay(root/'phases',document)
    spec=importlib.util.spec_from_file_location('additional_location_replay',frozen.__file__)
    replay=importlib.util.module_from_spec(spec);spec.loader.exec_module(replay)
    replay.load=load;replay.snapshot=snapshot;replay.candidates=candidates;replay.PROFILE=PROFILE
    independent=replay.reference_replay(reference,document,create=True)
    replay.reference_replay(reference,document)
    for row in load():
        for frame in document['frames'][row['id']].values():
            frame['exact_diagnostic_canonical_status']='passed'if sci.report_module().same_json(frame['ordinary_projection'],row['canonical_expected'])else'mismatch'
    proof.same(before,snapshot(),'Diagnostic current producer changed')
    proof.same(programs,candidates.reference_execution_binding(),'Diagnostic reference executable changed')
    document.update(scope='Diagnostic only: whole originals actual phases and independent complete source/unit/raw replay; no acceptance or original credit',independent_pin_replay_binding=independent,original_semantic_pass_credit=0,canonical_acceptance_credit=0)
    (root/'diagnostic.json').write_text(json.dumps(document,indent=2)+'\n')
    print({row['sci_id']:{key:frame['exact_diagnostic_canonical_status']for key,frame in document['frames'][row['id']].items()}for row in load()},flush=True)

if __name__=='__main__':run('/tmp/light-cljs-125-original-sci-after191-collection-phase-v1')
