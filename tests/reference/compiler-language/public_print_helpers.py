"""Independent two-pin, eight-frame public print helpers proof."""
from pathlib import Path
from unittest.mock import patch
import argparse
import copy
import json
import os

import sci_cljc_accepted_context as proof
import sci_execution as sci

ROOT = sci.ROOT
PROFILE = 'canonical-public-print-helpers-v1'
DRIVER = ROOT/'tests/compiler_public_print_helpers.rs'
TEST = 'public_print_helpers_execute_in_fresh_and_reused_macro_hosts'
CONTRACTS = Path(__file__).with_name('public-print-helper-accepted-contracts.json')
CONTRACTS_SHA256 = 'fa36591bcf2ba870793b3bb76ca24ba750e5f24f001680c71cc019d22af5df52'


def exact_observation(row, observed):
    """Compare complete typed packets without any sequence/set projection."""
    proof.same(set(observed), {'id','outcome','trace','stdout'}, 'Incomplete exact printing packet')
    proof.same(observed['id'], row['id'], 'Wrong exact printing packet identity')
    equal = proof.report.same_json
    return {'status':'passed' if equal(observed,row['canonical_expected']) else 'mismatch',
            'observed':observed,
            'original_status':'passed' if equal(observed,row['original_expected']) else 'mismatch'}


def load():
    raw = CONTRACTS.read_bytes()
    proof.same(sci.digest(raw), CONTRACTS_SHA256, 'Unreviewed independent public printing contract')
    frozen = json.loads(raw)
    proof.same(frozen['profile'], PROFILE, 'Wrong printing profile')
    historical = Path(__file__).with_name('public-print-helper-gap-contracts.json')
    proof.same(sci.digest(historical.read_bytes()), frozen['historical_catalog_sha256'], 'Changed historical printing caller catalog')
    rows = frozen['sources']
    proof.same(len(rows), 10, 'Missing complete public caller/control census')
    proof.same(len({r['id'] for r in rows}), 10, 'Duplicate printing caller')
    proof.same([{k:r[k] for k in ('id','source')} for r in rows[:2]], [{k:r[k] for k in ('id','source')} for r in json.loads(historical.read_text())], 'Changed original complete callers')
    for row in rows:
        proof.same(sci.digest(row['source']), row['source_sha256'], 'Changed complete caller source')
        proof.same(row['canonical_acceptance'], row['canonical_expected']==row['checkout_expected'], 'Unclassified pin difference')
        row.update(original_expected=copy.deepcopy(row['canonical_expected']), local_expected=copy.deepcopy(row['checkout_expected']), two_pin_agreement=row['canonical_acceptance'], local_rejections={})
    proof.same(sum(r['canonical_acceptance'] for r in rows), 9, 'Changed accepted printing census')
    return rows


def snapshot():
    paths = [Path(__file__), DRIVER, CONTRACTS, Path(__file__).with_name('test_public_print_helpers.py'), ROOT/'tests/fixtures/compiler-public-print-helpers.tsv']
    return {**proof.snapshot(), **{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def references(root, rows, programs):
    result = {}
    for kind in ('release','checkout'):
        pin_rows = copy.deepcopy(rows)
        if kind == 'checkout':
            for row in pin_rows:
                row['canonical_expected'] = row['checkout_expected']
        with patch.object(proof,'observation',exact_observation):
            result[kind] = proof.verify_reference(kind,root/kind,pin_rows,programs)
        for row in rows:
            proof.same(result[kind][row['id']]['status'],'passed', 'Map traversal differs from independent '+kind+' pin: '+row['id'])
    return result


def verified_document(directory):
    root = Path(directory).resolve()
    before = snapshot()
    rows = load()
    binding = json.loads((root/'binding.json').read_text())
    proof.same(binding['producer_source_sha256'], before, 'Map traversal/compiler producer drift')
    proof.same(binding['programs'], proof.executable_paths(), 'Executable path drift')
    proof.same(set(binding['executables_sha256']), set(binding['programs'].values())|{binding['audit_executable']}, 'Missing executable hashes')
    for path, expected in binding['executables_sha256'].items():
        proof.same(sci.digest(Path(path).read_bytes()), expected, 'Executable drift')
    proof.same(binding['audit_environment'], {'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(root/'phases'),'NODE':binding['programs']['node']}, 'Phase environment drift')
    proof.same(binding['artifact_sha256'], proof.tree(root,('binding.json','evidence.json')), 'Artifact census/hash drift')
    refs = references(root, rows, binding['programs'])
    local_rows = copy.deepcopy(rows)
    for row in local_rows:
        row['canonical_expected'] = row['local_expected']
    with patch.object(proof,'DRIVER',DRIVER), patch.object(proof,'TEST',TEST), patch.object(proof,'observation',exact_observation):
        frames = proof.verify_local(root/'phases',local_rows,binding)
    for row in rows:
        expected_frames=[]
        for variant in proof.VARIANTS:
            expected_frames += [variant+'/rejected'] if variant in row['local_rejections'] else [variant+'/'+engine for engine in ('QuickJS','Node')]
        proof.same(list(frames[row['id']]),expected_frames, 'Missing exact actual context census')
        for name,frame in frames[row['id']].items():
            if name.endswith('/rejected'):
                proof.same(frame['status'],'rejected','Expected exact source-phase rejection')
                signature=row['local_rejections'][name.removesuffix('/rejected')]
                if signature not in frame['detail']:raise ValueError('Changed nonpassing diagnostic category')
            else:
                proof.same(frame['status'],'passed','Actual public print helper mismatch: '+row['id'])
                frame['local_regression_status'] = 'passed'
            if not row['canonical_acceptance']:
                frame['status'] = 'canonical-version-difference' if not row['two_pin_agreement'] else 'canonical-local-mismatch'
                frame['nonpassing_reason'] = row['nonpassing_reason']
            frame['two_pin_agreement_credit'] = 1 if row['canonical_acceptance'] else 0
    replay_actual_local(root, rows, binding)
    proof.same(before,snapshot(),'Producer changed during replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'source_bindings':rows,
            'execution_binding':binding,'compiler_build_producer_source_sha256':before,
            'references':refs,'frames':frames,'canonical_contract_status':'passed',
            'actual_macro_contract_status':'passed','status':'passed',
            'accepted_source_count':sum(r['canonical_acceptance'] for r in rows),
            'accepted_engine_frame_count':8*sum(r['canonical_acceptance'] for r in rows),
            'nonpassing_neighbor_count':sum(not r['canonical_acceptance'] for r in rows),
            'actual_engine_frame_count':sum(not n.endswith('/rejected') for fs in frames.values() for n in fs),
            'nonpassing_rejection_context_count':sum(n.endswith('/rejected') for fs in frames.values() for n in fs),
            'original_semantic_pass_credit':0,'full_language_completion_credit':0}


def replay_actual_local(root, rows, binding):
    # Rehashed raw packets/units cannot replace actual source computation.
    import tempfile
    replay = Path(tempfile.mkdtemp(prefix='light-public-print-replay-')).resolve()
    (replay/'input.tsv').write_text(proof.input_text(rows))
    env={'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(replay),'NODE':binding['programs']['node']}
    audit=proof.command([binding['audit_executable'],TEST,'--ignored','--exact','--nocapture'],replay/'audit-command',300,environment={**os.environ,**env},record_environment=env)
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in audit.stdout or audit.stderr:
        raise ValueError('Actual printing audit replay failed')
    actual=root/'phases'
    paths={'input.tsv','warmup.js','observations.tsv'}
    for row in rows:
        for variant in proof.VARIANTS:
            paths |= {row['id']+'/'+variant+'/'+name for name in ('user.cljc','compiled.js','script.js','quickjs.raw','node.stdin','node.stdout','node.stderr','node.exit')}
    proof.same(set(proof.tree(replay))-{ 'audit-command/'+n for n in ('stdout','stderr','command.json')},paths,'Actual replay artifact census changed')
    for name in paths:
        # Only the temporary source-root prefix may differ in emitted source metadata.
        proof.same((replay/name).read_text().replace(str(replay),str(actual)),(actual/name).read_text(),'Actual source/compiled/engine replay differs: '+name)


def reconcile(document):
    proof.same(document.get('profile'),PROFILE,'Wrong evidence profile')
    root = document.get('artifacts_root')
    if not isinstance(root,str) or str(Path(root).resolve()) != root:
        raise ValueError('Missing normalized raw artifact root')
    replay = verified_document(root)
    proof.same(document,replay,'Map traversal evidence differs from raw executable replay')
    return replay


def run(directory, timeout=600):
    root=Path(directory).resolve()
    root.mkdir(parents=True,exist_ok=False)
    before=snapshot()
    rows=load()
    programs=proof.executable_paths()
    hashes={path:sci.digest(Path(path).read_bytes()) for path in programs.values()}
    for kind in ('release','checkout'):
        proof.produce_reference(kind,root/kind,rows,programs,timeout)
        print('Fresh '+kind+' public print helper observations completed',flush=True)
    references(root,rows,programs)
    phases=root/'phases'
    phases.mkdir()
    (phases/'input.tsv').write_text(proof.input_text(rows))
    build=proof.command([programs['cargo'],'test','--locked','--test',DRIVER.stem,'--no-run','--message-format=json'],phases/'build',timeout)
    emitted=[r['executable'] for r in map(json.loads,build.stdout.splitlines()) if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==DRIVER.stem and r.get('executable')]
    proof.same(len(emitted),1,'Missing exact Cargo audit binary')
    binary=emitted[0]
    hashes[binary]=sci.digest(Path(binary).read_bytes())
    env={'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(phases),'NODE':programs['node']}
    proof.command([binary,TEST,'--ignored','--exact','--nocapture'],phases/'audit',min(timeout,300),environment={**os.environ,**env},record_environment=env)
    proof.same(before,snapshot(),'Source drift during fresh proof')
    binding={'producer_source_sha256':before,'programs':programs,'executables_sha256':hashes,
             'audit_executable':binary,'audit_environment':env,'artifact_sha256':proof.tree(root)}
    (root/'binding.json').write_text(json.dumps(binding,indent=2)+'\n')
    document=verified_document(root)
    (root/'evidence.json').write_text(json.dumps(document,indent=2)+'\n')
    reconcile(document)
    print('Verified '+str(document['actual_engine_frame_count'])+' actual engine frames and '+str(document['nonpassing_rejection_context_count'])+' zero-credit rejection contexts',flush=True)
    return document


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    parser.add_argument('--timeout',default=600,type=int)
    args=parser.parse_args()
    run(args.output,args.timeout)
