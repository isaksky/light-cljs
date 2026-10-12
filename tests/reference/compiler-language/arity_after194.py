#!/usr/bin/env python3
"""Complete original typed outcomes for disjoint newly demanded public/local calls."""
from pathlib import Path
import argparse,json
from arity_resume_candidates import digest,direct_calls
ROOT=Path(__file__).resolve().parents[3]
HERE=ROOT/'tests/reference/compiler-language'
HELPER='tests/reference/compiler-language/arity_after194.py'
SCANNER='tests/reference/compiler-language/arity_resume_candidates.py'
DRIVER='tests/compiler_arity_after194.rs'
TEST='remaining_whole_arity_sources_execute_in_actual_phases_and_lifecycles'
CATALOG=HERE/'arity-after194-catalog.json'
CATALOG_SHA256='f10ca161341c6631e128e0912c3d7e679f49acc126e633c637ddf0f209d2df0e'
SCANNER_SHA256='31457848fe6d2f08e92b3ad5b4a6f99362a97be66e4819c9d45cf3a1b9ecbcc3'
PRINT_SUPPORT=[]
CONTEXT_SUPPORT=[]
_catalog=json.loads(CATALOG.read_text())
REVIEWED=_catalog['reviewed']
SELECTION=_catalog['selection']


def original_case(row,line):
    f=line.split('\t');identity=row['id']
    if row['original_fixture'].endswith('/execution-outcomes.tsv'):
        expected={'id':identity,'outcome':json.loads(f[2]),'trace':json.loads(f[3]),'stdout':json.loads(f[4])}
    elif row['original_fixture'].endswith('/declare-values.tsv'):
        expected=json.loads(f[2]);expected['id']=identity
    else:
        expected={'id':identity,'outcome':{'status':'returned','value':json.loads(f[2])},'trace':json.loads(f[3]) if row['original_fixture'].endswith('/execution.tsv') else [],'stdout':''}
    return {'id':identity,'source':f[1],'expected':expected,'native_stdout':None}


def rows():
    if digest(CATALOG.read_bytes())!=CATALOG_SHA256 or digest((ROOT/SCANNER).read_bytes())!=SCANNER_SHA256:
        raise ValueError('frozen whole-source catalog/scanner drift')
    catalog=json.loads(CATALOG.read_text())
    for relative,sha in {**catalog.get('preserved_prior_arity_source_sha256',{}),**catalog.get('preserved_discovery_history',{})}.items():
        if digest((ROOT/relative).read_bytes())!=sha:raise ValueError('previous arity producer/source changed')
    driver=(ROOT/DRIVER).read_text()
    for required in [TEST,'MacroHost::new()','compile_namespace_with_macros','defmacro arity-witness','js* {macro_observer:?} arity-original','fresh','reused','arity-after194-observation','sci_execution_observation.js','(0, eval)({script:?})','quickjs.stdin','node(&input)']:
        if required not in driver:raise ValueError('actual whole original phase driver detached')
    result=[]
    for row in REVIEWED:
        lines=[line for line in (ROOT/row['original_fixture']).read_text().splitlines() if line.startswith(row['original_id']+'\t')]
        if len(lines)!=1:raise ValueError('original outcome identity missing or duplicated')
        original=lines[0];case=original_case(row,original)
        if digest(original)!=row['original_row_sha256'] or (ROOT/row['fixture']).read_text()!=case['source'] or digest(case['source'])!=row['source_sha256'] or case!=row['case'] or digest(json.dumps(case,sort_keys=True))!=row['case_sha256']:
            raise ValueError('whole original source/result/effects/stdout drift')
        if json.dumps(row['id']) not in driver or Path(row['fixture']).name not in driver:
            raise ValueError('whole original fixture omitted by Rust driver')
        result.append({**row,'source':case['source']})
    return result


def supplemental_reference_cases():
    return {row['id']:row['case'] for row in rows()}


def demand_basis(row):
    return row['demand_basis']


def operation_evidence(source,selected,target):
    spelling=target['name']+('.' if target.get('declaration') in ['deftype','defrecord'] else '')
    if selected['call_name']!=spelling:raise ValueError('operation or public constructor spelling drift')
    calls=[call for call in direct_calls(source,spelling) if call['argument_count'] in selected['direct_arities']]
    if {call['argument_count'] for call in calls}!=set(selected['direct_arities']):raise ValueError('exact executing original direct call omitted')
    signature=target.get('signature')
    if signature and not all(call['argument_count']==signature['minimum'] if signature['kind']=='fixed' else signature['kind']=='variadic' and call['argument_count']>signature['minimum'] for call in calls):
        raise ValueError('exact public signature demand omitted')
    return calls


def exports():
    workflows={row['id']:row for row in rows()}
    targets={row['id']:row for row in map(json.loads,(HERE/'obligations.jsonl').read_text().splitlines())}
    catalog=json.loads(CATALOG.read_text())
    execution={'test_file':DRIVER,'test_name':TEST,'test_source_sha256':digest((ROOT/DRIVER).read_bytes())}
    ordinary,phases,receipts=[],[],[]
    for selected in SELECTION:
        target=targets[selected['obligation_id']];row=workflows[selected['workflow']];signature=target.get('signature')
        if target['id'] not in catalog['baseline_missing_ids'] or target['body_sha256']!=selected['body_sha256'] or target['disposition']!='intended':raise ValueError('duplicate, nonmissing or detached original obligation')
        if signature and digest(json.dumps(signature,sort_keys=True))!=selected['signature_sha256']:raise ValueError('original exact signature drift')
        calls=operation_evidence(row['source'],selected,target)
        rationale=demand_basis(row)+' Every original source byte and complete literal typed outcome, ordered effects and stdout is retained. Fresh/reused actual MacroHost application and demanded macro computations execute in QuickJS and Node; both default required pins execute the entire original. The demanded macro computes the full observer packet and transports only that finite string; this makes no arbitrary source-result expansion-domain claim. Local operation mapping makes no full metadata-value claim. No source-site, shadowed-call or identity-only credit is added.'
        common={'obligation_id':target['id'],'body_sha256':target['body_sha256'],'rationale':rationale}
        if signature:common['signature_sha256']=selected['signature_sha256']
        ordinary.append({**common,'assertion_ids':[],'local_execution':execution,'reference_fixture_id':row['id'],'reference_source_sha256':row['source_sha256'],'reference_contract_sha256':digest(json.dumps(row['case']['expected'],sort_keys=True))})
        support=[HELPER,SCANNER,str(CATALOG.relative_to(ROOT)),row['fixture'],row['original_fixture'],'tests/support/sci_execution_observation.js']
        phases.append({**common,**execution,'support_source_sha256':{path:digest((ROOT/path).read_bytes()) for path in support}})
        receipts.append({**selected,'direct_calls':calls,'whole_source_sha256':row['source_sha256'],'original_case_sha256':row['case_sha256'],'original_fixture':row['original_fixture'],'original_id':row['original_id'],'original_row_sha256':row['original_row_sha256'],'demand_basis':demand_basis(row),'oracle_id':row['id'],'phase_transport':catalog['phase_transport']})
    result={'obligation-witnesses.jsonl':ordinary,'phase-witnesses.jsonl':phases,'arity-after194-call-receipts.jsonl':receipts}
    audit_installation(result)
    return result


def audit_installation(exported):
    owned={row['obligation_id'] for row in exported['obligation-witnesses.jsonl']};installed={}
    catalog=json.loads(CATALOG.read_text())
    for name in ['obligation-witnesses.jsonl','phase-witnesses.jsonl']:
        current=list(map(json.loads,(HERE/name).read_text().splitlines()))
        for id,sha in catalog.get('preserved_prior_arity_rows',{}).get(name,{}).items():
            prior=[row for row in current if row['obligation_id']==id]
            if len(prior)!=1 or digest(json.dumps(prior[0],sort_keys=True))!=sha:raise ValueError('previous arity cohort changed or duplicated: '+name)
        expected={row['obligation_id']:row for row in exported[name]};rows=[row for row in current if row['obligation_id'] in owned];ids=[row['obligation_id']for row in rows]
        if len(ids)!=len(set(ids)):raise ValueError('duplicate own installed loader row: '+name)
        if any(row!=expected[row['obligation_id']] for row in rows):raise ValueError('own installed loader overlap or whole-row drift: '+name)
        installed[name]=set(ids)
    if any(installed.values()) and any(ids!=owned for ids in installed.values()):raise ValueError('partial or unpaired own cohort installation')
    return {'state':'installed-exact-paired'if any(installed.values())else'not-installed','ordinary_rows':len(installed['obligation-witnesses.jsonl']),'phase_rows':len(installed['phase-witnesses.jsonl']),'expected_owned_rows':len(owned),'overlap_or_drift':'rejected by exact whole-row comparison'}


def audit_installed_loaders(exported):
    state=audit_installation(exported)
    if state['state']=='not-installed':return {**state,'ordinary_loader':'pending-integration','phase_loader':'pending-integration'}
    import full_ledger,report
    assertions,obligations,_=full_ledger.load();_,witnesses,_=report.load_contract('intended-language')
    installed={'obligation-witnesses.jsonl':full_ledger.load_obligation_witnesses(obligations,assertions,witnesses),'phase-witnesses.jsonl':full_ledger.load_phase_witnesses(obligations)}
    for name in installed:
        if [installed[name].get(row['obligation_id'])for row in exported[name]]!=exported[name]:raise ValueError('installed permanent loader whole-row drift: '+name)
    return {**state,'ordinary_loader':'passed-exact-own-cohort','phase_loader':'passed-exact-own-cohort'}


def parse_frames(output):
    expected=[(row['id'],host,phase,engine)for row in rows()for host in ['fresh','reused']for phase in ['application','macro']for engine in ['QuickJS','Node']]
    contracts={row['id']:row['case']['expected']for row in rows()};frames=[]
    for line in output.splitlines():
        if not line.startswith('arity-after194-observation\t'):continue
        fields=line.split('\t')
        if len(fields)!=6 or json.loads(fields[5])!=contracts.get(fields[1]):raise ValueError('nonpassing complete actual typed packet')
        frames.append({'id':fields[1],'host':fields[2],'phase':fields[3],'engine':fields[4],'raw':fields[5]})
    if [(f['id'],f['host'],f['phase'],f['engine'])for f in frames]!=expected:raise ValueError('missing/extra/reordered actual frames')
    return frames


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--export-owned',type=Path,required=True);a=p.parse_args();a.export_owned.mkdir(parents=True,exist_ok=True)
    for name,records in exports().items():(a.export_owned/name).write_text(''.join(json.dumps(row,separators=(',',':'))+'\n'for row in records))
