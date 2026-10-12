#!/usr/bin/env python3
"""Current real full-report reconciliation for exactly one original source site.

Mechanical mock guards are labeled separately. They never replace the real
whole-source collector, executable replay or acceptance expand_report call.
"""
from pathlib import Path
from unittest.mock import patch
import argparse,copy,json
import integer_source_context_after194 as source
import integer_predicate_after192 as actual
import full_ledger as full
import report


def historical():
    return {"counts":{key:0 for key in ("covered","verified-rejection","verified-warning","missing","version-difference","unrelated")},"strict_complete":False,"boundary_strict_complete":True}


def current_consumer_binding():
    names=['full_ledger.py','report.py','full-manifest.json','full-inventory.jsonl','full-bodies.jsonl','obligations.jsonl','support-sources.jsonl','witnesses.jsonl','full-witnesses.jsonl','obligation-witnesses.jsonl','phase-witnesses.jsonl',Path(__file__).name,'test_integer_source_integration_after194.py']
    return {name:actual.digest((full.HERE/name).read_bytes()) for name in names}


def inputs():
    _,witnesses,_=report.load_contract('intended-language')
    return witnesses


def mechanical_guards(proof):
    assertions,obligations,manifest=full.load();witnesses=inputs();results=[]
    original=next(r for r in assertions if r['id']==source.SITE)
    def attempt(name,edit_proof=None,edit_source=None,overlap=False):
        proposed=copy.deepcopy(proof);rows=copy.deepcopy(assertions);mapped=copy.deepcopy(witnesses)
        if edit_proof:edit_proof(proposed)
        if edit_source:edit_source(next(r for r in rows if r['id']==source.SITE))
        if overlap:mapped[source.SITE]={'fixture_id':'mechanical-integer-overlap'}
        with patch.object(source,'reconcile',return_value=proposed),patch.object(full,'load',return_value=(rows,obligations,manifest)):
            try:full.expand_report(historical(),mapped,{},integer_source_context_evidence={'mechanical_guard':True})
            except ValueError as error:
                if str(error) not in ('Complete integer source proof changed its sole original site','Complete integer proof detached from its original assertion'):
                    raise ValueError('Consumer guard rejected before integer identity check: '+name+': '+str(error)) from error
                results.append({'id':name,'status':'rejected','reason':str(error),'proof_kind':'mechanical-consumer-guard-only'})
            else:raise ValueError('Full-report consumer accepted altered integer source '+name)
    attempt('detached-assertion-hash',lambda d:d[source.SITE]['mapping'].update(assertion_sha256='0'*64))
    attempt('detached-body-hash',lambda d:d[source.SITE]['mapping'].update(body_sha256='0'*64))
    attempt('detached-original-id',lambda d:d[source.SITE]['mapping'].update(assertion_id=source.SITE+'-wrong'))
    attempt('extra-source-site',lambda d:d.update({'unrelated-source':copy.deepcopy(d[source.SITE])}))
    attempt('overlapping-installed-source-witness',overlap=True)
    attempt('excluded-original-source',edit_source=lambda r:r.update(disposition='host'))
    attempt('changed-original-body-binding',edit_source=lambda r:r.update(body_sha256='0'*64))
    attempt('public-or-local-arity-credit',lambda d:d[source.SITE].update(new_public_or_local_arity_links=1))
    attempt('missing-original-semantic-pass',lambda d:d[source.SITE].update(original_assertion_semantic_pass_credit=0))
    return {'controls':results,'source_inventory_modified':False,'proof_kind':'mechanical-consumer-guard-only','original_semantic_pass_credit':0}


def validate(document):
    before=current_consumer_binding();witnesses=inputs();assertions,obligations,manifest=full.load()
    baseline=full.expand_report(historical(),witnesses,{})
    installed=full.expand_report(historical(),witnesses,{},integer_source_context_evidence=document)
    missing={r['id']:r for r in baseline['assertions']};present={r['id']:r for r in installed['assertions']}
    source.whole.same(set(missing),set(present),'Whole assertion/obligation universe changed by integer source integration')
    source.whole.same(set(present),{r['id'] for r in assertions+obligations},'Full-report integer integration omitted an original obligation')
    changed=[identity for identity in missing if missing[identity]!=present[identity]]
    source.whole.same(changed,[source.SITE],'Integer source integration changed another source or public/local obligation')
    source.whole.same(missing[source.SITE]['status'],'missing','Absent whole source proof supplied integer semantic credit')
    source.whole.same(present[source.SITE]['status'],'covered','Current complete original source proof did not cover exact SITE')
    source.whole.same(baseline['strict_complete'],False,'Empty-evidence full report unexpectedly complete')
    source.whole.same(installed['strict_complete'],False,'One original integer site manufactured full completion')
    covered=present[source.SITE];proof=covered['integer_source_context_reconciliation']
    source.whole.same(proof['new_public_or_local_arity_links'],0,'Original source proof supplied public/local arity credit')
    source.whole.same(len(proof['actual_phase_frames']),8,'Consumer discarded any actual original source frame')
    source.whole.same(set(proof['references']),{'release','checkout'},'Consumer discarded an original pin')
    controls=mechanical_guards({source.SITE:proof})
    source.whole.same(current_consumer_binding(),before,'Shared integer report consumer drifted during real reconciliation')
    return {'schema_version':1,'status':'passed-installed-current-full-report-integer-source-reconciliation',
      'current_consumer_binding':before,'current_source_document_sha256':actual.digest(json.dumps(document,sort_keys=True)),
      'original_inventory_rows':len(assertions),'independent_obligation_rows':len(obligations),'full_report_universe_rows':len(present),
      'universe_sha256':actual.digest(json.dumps(sorted(present))),'changed_rows':changed,'absent_status':'missing','present_status':'covered',
      'full_original_source_row':covered,'full_literal_packet_and_reference_preservation':'passed',
      'public_or_local_arity_credit':0,'mechanical_consumer_controls':controls,'strict_complete':False}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--document',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    receipt=validate(json.loads(args.document.read_text()));args.output.write_text(json.dumps(receipt,indent=2)+'\n');print(receipt['status'],flush=True)
