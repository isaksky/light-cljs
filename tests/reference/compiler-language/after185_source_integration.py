#!/usr/bin/env python3
"""Replay current raw whole-source proofs and retain exact original observers."""
import argparse
import copy
import hashlib
import importlib
import json
from collections import Counter
from pathlib import Path
import full_ledger
import report

HERE=Path(__file__).resolve().parent
COHORTS=(('gap','sci_gap_canonical_context','gap_sci_context_evidence'),
         ('print','sci_print_context','print_sci_context_evidence'),
         ('cljc','sci_cljc_accepted_context','cljc_sci_context_evidence'),
         ('lazy','sci_lazy_context','lazy_sci_context_evidence'),
         ('char','case_char_string_accepted','case_sci_context_evidence'))

def run(roots, transport_path, output):
    documents={}; observed={}; identities=set(); inputs={}
    for name,module_name,keyword in COHORTS:
        path=Path(roots[name]).resolve(); document=json.loads(path.read_text())
        contexts=importlib.import_module(module_name).reconcile(document)
        if identities & set(contexts): raise ValueError('source cohorts overlap')
        identities.update(contexts)
        for proof in contexts.values(): observed[proof['id']]=proof['layers']
        documents[keyword]=document
        inputs[name]={'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
    path=Path(transport_path).resolve(); transport=json.loads(path.read_text())
    documents['transport_sci_context_evidence']=transport
    inputs['transport']={'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
    inv,witnesses,_=report.load_contract(); historical=report.assemble(inv,witnesses,{})
    _,witnesses,_=report.load_contract('intended-language')
    result=full_ledger.expand_report(historical,witnesses,{},identities=sorted(identities),sci_evidence=observed,**documents)
    selected=[row for row in result['assertions'] if row['id'] in identities]
    counts=dict(Counter(row['status'] for row in selected))
    if len(selected)!=96 or counts!={'covered':5,'version-difference':87,'missing':4}:
        raise ValueError('whole-source cohort census/result changed: '+repr(counts))
    passed=sum(row['columns']['quickjs_macro']['status']=='passed' for row in selected)
    if passed!=92:raise ValueError('complete actual macro proof census changed')
    typed=[row for row in selected if row.get('gap_sci_context_reconciliation',{}).get('intermediate_value_transport_profile')]
    if len(typed)!=3:raise ValueError('exact typed transport source census changed')
    prior={row['gap_sci_context_reconciliation']['id']:observed[row['gap_sci_context_reconciliation']['id']] for row in typed}
    failure_statuses={'mismatch','invalid-output','invalid-reference','failed','unavailable','reference-unavailable','timed-out','timeout','rejected','stale-build-input','unverified'}
    if full_ledger.sci_execution_failed({'assertions':typed},prior,failure_statuses):
        raise ValueError('verified typed canonical differences remain falsely detached from original observers')
    detached=copy.deepcopy(typed)
    detached[0]['gap_sci_context_reconciliation']['prior_observer_reconciliation']['layers']={}
    if not full_ledger.sci_execution_failed({'assertions':detached},prior,failure_statuses):
        raise ValueError('detached prior observer incorrectly explained')
    output=Path(output); output.parent.mkdir(parents=True,exist_ok=True)
    source_report=output.with_name('source-context-report.json')
    source_report.write_text(json.dumps({'selection':result['selection'],'strict_complete':False,'counts':counts,'assertions':selected},indent=2)+'\n')
    receipt={'schema_version':1,'profile':'after185-raw-whole-source-integration-v1','inputs':inputs,
             'source_cohort_counts':counts,'source_cohort_total':96,'actual_macro_passed_sources':passed,
             'original_sci_semantic_pass_credit':5,'typed_transport_original_sci_credit':0,
             'typed_transport_retains_original_observers':True,'detached_observer_control':'rejected',
             'remaining_source_ids':[row['id'] for row in selected if row['status']=='missing'],
             'source_report':{'path':str(source_report.resolve()),'sha256':hashlib.sha256(source_report.read_bytes()).hexdigest()},
             'integration_inputs':{name:hashlib.sha256((HERE/name).read_bytes()).hexdigest() for name in
                ('after185_source_integration.py','full_ledger.py','report.py','full-manifest.json','obligation-witnesses.jsonl','phase-witnesses.jsonl')},
             'strict_complete':False,'complete_unfiltered_gate':False}
    output.write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps({'receipt':str(output.resolve()),'counts':counts,'actual_macro_passed_sources':passed}))
    return receipt

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name,_,_ in COHORTS:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--transport',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();run({name:getattr(args,name) for name,_,_ in COHORTS},args.transport,args.output)
