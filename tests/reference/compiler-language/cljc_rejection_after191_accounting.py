"""Three exact canonical reader rejections, never original execution passes."""
import copy
import cljc_rejections_after191 as proof

def verified(document,prior_contexts,current_evidence):
    if document is None:return {}
    contexts=proof.reconcile(document)
    for identity,negative in contexts.items():
        prior=prior_contexts.get(identity)
        if not prior or prior.get('status')!='missing':
            raise ValueError('Original rejection may supplement only unresolved reader source')
        binding=prior['source_binding']
        proof.same(binding,negative['source_binding'],'Original rejection changed whole source/expectation binding')
        proof.same(binding.get('canonical_expected'),None,'Original rejection replaced result literal')
        proof.same(prior['layers'],current_evidence.get(negative['id']),'Original rejection detached from fresh original observer')
        for pin in ('release','checkout'):
            proof.same(prior['layers'][pin].get('status'),'rejected','Original rejection replaced accepted pin observer')
            proof.same(prior['layers'][pin].get('original_source_sha256'),binding['source_sha256'],'Original pin reader source drift')
        negative['prior_observer_reconciliation']=copy.deepcopy(prior)
    return contexts

def explained_ids(report,evidence):
    """Explain only retained mismatches/rejections of the three invalid sources."""
    frozen={r['id']:r for r in proof.load() if r['canonical_reference_rejection'] is not None}
    result=set()
    for row in report.get('assertions',[]):
        negative=row.get('canonical_source_rejection_reconciliation',{})
        original=frozen.get(negative.get('id'))
        if not original:continue
        prior=negative.get('prior_observer_reconciliation',{})
        macro=row.get('columns',{}).get('quickjs_macro',{})
        if (row.get('status')!='verified-rejection'
            or negative.get('reconciliation')!='verified-canonical-original-source-rejection'
            or negative.get('profile')!=proof.PROFILE
            or negative.get('original_semantic_pass_credit')!=0
            or negative.get('original_macro_execution_pass_credit')!=0
            or macro.get('status')!='rejected-before-original-computation'
            or prior.get('status')!='missing'):continue
        if negative.get('source_binding',{}).get('source')!=original['source']:continue
        if not proof.report.same_json(prior.get('layers'),evidence.get(negative['id'])):continue
        if not all(negative.get('layers',{}).get(pin,{}).get('status')=='rejected'
                   and prior.get('layers',{}).get(pin,{}).get('status')=='rejected'
                   for pin in ('release','checkout')):continue
        frames=negative.get('actual_phase_frames',{})
        if set(frames)!={v+'/rejected' for v in proof.historical.VARIANTS}:continue
        if any(f.get('status')!='rejected-before-original-computation'
               or f.get('id')!=original['id']
               or f.get('variant')+'/rejected'!=key
               or f.get('source_sha256')!=proof.sci.digest(proof.source(original['source'],f['variant']))
               for key,f in frames.items()):continue
        recovery=negative.get('same_host_recovery_frames',[])
        if len(recovery)!=8 or {(f.get('variant'),f.get('engine')) for f in recovery}!={
                (v,e) for v in proof.historical.VARIANTS for e in ('QuickJS','Node')}:continue
        if any(f.get('status')!='recovery-passed' or f.get('id')!=original['id']
               or f.get('raw')!='42' for f in recovery):continue
        result.add(negative['id'])
    return result
