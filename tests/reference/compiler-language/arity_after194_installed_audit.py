#!/usr/bin/env python3
"""Audit actual permanent installed loaders without rebinding preintegration proof."""
from pathlib import Path
import argparse,json
import arity_after194 as cohort


def audit(output):
    exported=cohort.exports()
    state=cohort.audit_installed_loaders(exported)
    if state['state']!='installed-exact-paired':raise ValueError('actual after194 cohort is not installed in both permanent loaders')
    import report,full_ledger
    cases=report.fixtures('intended-language')
    linked={row['reference_fixture_id']for row in exported['obligation-witnesses.jsonl']}
    originals=cohort.supplemental_reference_cases()
    if {id for id in cases if id.startswith('arity-after194-')}!=linked:raise ValueError('installed report added unlinked controls or omitted complete linked originals')
    for id in linked:
        if cases[id]!=originals[id]:raise ValueError('installed complete original reference oracle drift')
    preintegration=cohort.HERE/'arity-after194-context-receipt.json'
    retained=json.loads(preintegration.read_text())
    if retained['exports']!=exported:raise ValueError('installed exports differ from independently executed preintegration proof')
    if retained['installed_loader_state']['state']!='not-installed':raise ValueError('preintegration independent proof was rebound')
    paths=[cohort.HELPER,cohort.SCANNER,cohort.DRIVER,str(cohort.CATALOG.relative_to(cohort.ROOT)),str(preintegration.relative_to(cohort.ROOT)),
      'tests/reference/compiler-language/arity_after194_installed_audit.py','tests/reference/compiler-language/report.py','tests/reference/compiler-language/full_ledger.py','tests/reference/compiler-language/full-manifest.json','tests/reference/compiler-language/obligation-witnesses.jsonl','tests/reference/compiler-language/phase-witnesses.jsonl']
    result={'strict_complete':False,'source_site_credit_added':0,'ordinary_and_phase_rows':len(cohort.SELECTION),'linked_complete_original_oracles':len(linked),'installed_loader_state':state,'exports':exported,'preintegration_proof_retained_sha256':cohort.digest(preintegration.read_bytes()),'installed_source_sha256':{path:cohort.digest((cohort.ROOT/path).read_bytes())for path in paths},'execution_credit_basis':'unchanged independent current whole-packet proof; this audit grants loader/fixture installation evidence only'}
    Path(output).write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args();audit(a.output)
