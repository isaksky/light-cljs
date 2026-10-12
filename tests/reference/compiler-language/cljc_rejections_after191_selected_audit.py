import copy,json,sys,hashlib
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,'tests/reference/compiler-language')
import full_ledger as ledger,report,cljc_rejections_after191 as proof,cljc_rejection_after191_accounting as accounting,sci_cljc_accepted_context as prior
prior_path=Path('/tmp/light-cljs-125-cljc-prior-after191/evidence.json')
negative_path=Path('/private/tmp/light-cljs-125-cljc-rejections-after191-integrated-v1/evidence.json')
d=json.loads(prior_path.read_text());n=json.loads(negative_path.read_text())
identities=[r['obligation_id'] for r in proof.load() if r['canonical_reference_rejection'] is not None]
_,witnesses,_=report.load_contract('intended-language')
historical={'counts':dict.fromkeys(('covered','missing','version-difference'),0),'strict_complete':False,'boundary_strict_complete':True}
base=dict(identities=identities,sci_evidence=d['evidence'],cljc_sci_context_evidence=d)
absent=ledger.expand_report(historical,witnesses,{},**base)
present=ledger.expand_report(historical,witnesses,{},cljc_source_rejection_evidence=n,**base)
def own(result):return [r for r in result['assertions'] if r['id'] in identities]
assert len(own(absent))==len(own(present))==3
assert all(r['status']=='missing' for r in own(absent))
assert all(r['status']=='verified-rejection' for r in own(present))
contexts={r['id']:r['gap_sci_context_reconciliation'] for r in own(absent)}
negatives={r['id']:r['canonical_source_rejection_reconciliation'] for r in own(present)}
for row in own(present):
    negative=row['canonical_source_rejection_reconciliation']
    assert negative['original_semantic_pass_credit']==negative['original_macro_execution_pass_credit']==0
    assert row['columns']['quickjs_macro']['status']=='rejected-before-original-computation'
    assert negative['prior_observer_reconciliation']==contexts[row['id']]
    assert negative['prior_observer_reconciliation']['layers']==d['evidence'][negative['id']]
for result in (absent,present):
    assert not result['strict_complete'] and not result['source_strict_complete'] and not result['selection']['full_scope_selected']
layers={r['canonical_source_rejection_reconciliation']['id']:copy.deepcopy(d['evidence'][r['canonical_source_rejection_reconciliation']['id']]) for r in own(present)}
failures={'mismatch','rejected','observation-error','harness-error'}
assert ledger.sci_execution_failed(absent,layers,failures)
assert not ledger.sci_execution_failed(present,layers,failures)
controls=[]
for mutation in ('macro-pass','original-credit','observation-error','unrelated-source'):
    changed=copy.deepcopy(present);inputs=copy.deepcopy(layers)
    if mutation=='macro-pass':own(changed)[0]['columns']['quickjs_macro']['status']='passed'
    elif mutation=='original-credit':own(changed)[0]['canonical_source_rejection_reconciliation']['original_semantic_pass_credit']=1
    elif mutation=='observation-error':next(iter(next(iter(inputs.values())).values()))['status']='observation-error'
    else:inputs['unrelated-original']={'quickjs':{'status':'rejected'}}
    assert ledger.sci_execution_failed(changed,inputs,failures)
    controls.append({'id':mutation,'status':'rejected-as-unexplained-execution-failure'})
# Real replay has just populated both seams. These structural fault checks
# retain those actual priors, without repeating unchanged source compilation.
for mutation in ('detached-evidence','accepted-prior-pin','changed-source'):
    priors=copy.deepcopy(contexts);inputs=copy.deepcopy(d['evidence'])
    identity=next(iter(priors));source_id=priors[identity]['source_binding']['id']
    if mutation=='detached-evidence':inputs[source_id]={}
    elif mutation=='accepted-prior-pin':
        priors[identity]['layers']['release']['status']='passed';inputs[source_id]=copy.deepcopy(priors[identity]['layers'])
    else:priors[identity]['source_binding']['source']+=' '
    with patch.object(proof,'reconcile',return_value=copy.deepcopy(negatives)):
        try:accounting.verified(n,priors,inputs)
        except ValueError as e:controls.append({'id':mutation,'status':'rejected','reason':str(e)})
        else:raise AssertionError(mutation+' accepted')
result={'schema_version':1,'profile':'real-current-negative-source-selected-integration','prior_receipt':str(prior_path),'prior_sha256':hashlib.sha256(prior_path.read_bytes()).hexdigest(),'negative_receipt':str(negative_path),'negative_sha256':hashlib.sha256(negative_path.read_bytes()).hexdigest(),'prior_whole_source_count':len(d['summaries']),'absent_rows':own(absent),'present_rows':own(present),'controls':controls,'original_semantic_pass_credit':0,'original_macro_execution_pass_credit':0,'full_scope_selected':False,'strict_complete':False,'source_strict_complete':False}
Path('tests/reference/compiler-language/cljc-rejections-after191-selected-integration-receipt.json').write_text(json.dumps(result,indent=2)+'\n')
Path('tests/reference/compiler-language/cljc-rejections-after191-fresh-30-prior-receipt.json').write_bytes(prior_path.read_bytes())
print('Actual fresh30 prior and three negative source rows pass real absent/present integration; seven fail-closed controls pass; zero original or macro execution credit.')
