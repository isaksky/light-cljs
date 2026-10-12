"""Independent two-pin, eight-frame quoted set traversal proof."""
from pathlib import Path
from unittest.mock import patch
import argparse
import copy
import json
import os

import sci_cljc_accepted_context as proof
import sci_execution as sci

ROOT = sci.ROOT
PROFILE = 'canonical-literal-set-traversal-v1'
DRIVER = ROOT/'tests/compiler_literal_set_traversal.rs'
TEST = 'literal_set_traversal_sources_execute_in_fresh_and_reused_macro_hosts'
CONTRACTS = Path(__file__).with_name('literal-set-traversal-contracts.json')
CONTRACTS_SHA256 = 'd5ca02524b35a39a30cab8240393dec4a27b7910f3a3d011a0ae41cb45f6a514'
ORIGINAL = r'''(let [s '#{\a "a" '\b \b}] [(count s) (contains? s \b) (contains? s '(quote \b))])'''
ORIGINAL_FULL = r'''(let [s '#{\a "a" '\b \b}] [(count s) (contains? s \b) (contains? s '(quote \b)) (vec (seq s))])'''


def load():
    raw = CONTRACTS.read_bytes()
    proof.same(sci.digest(raw), CONTRACTS_SHA256, 'Unreviewed independently derived set traversal contract')
    frozen = json.loads(raw)
    proof.same(frozen['profile'], PROFILE, 'Wrong set traversal contract profile')
    proof.same(frozen['pins'], {'release':'1.12.134','checkout':'a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e'}, 'Wrong pinned references')
    proof.same(len(frozen['sources']), 20, 'Missing complete set traversal source catalog')
    proof.same(len(frozen['canonical_expected']), 20, 'Missing independent expected packets')
    rows = []
    for source, expected in zip(frozen['sources'], frozen['canonical_expected']):
        identity = source['id']
        proof.same(sci.digest(source['source']), source['source_sha256'], 'Changed exact source bytes')
        proof.same(set(expected), {'id','outcome','trace','stdout'}, 'Incomplete typed pin packet')
        proof.same(expected['id'], identity, 'Detached expected packet')
        proof.same([expected['trace'], expected['stdout']], [[], ''], 'Wrong exact effects/stdout packet')
        checkout = frozen['checkout_expected'].get(identity, expected)
        nonpassing = frozen['nonpassing_neighbors'].get(identity)
        proof.same(checkout != expected, identity in frozen['checkout_expected'], 'Unclassified two-pin difference')
        proof.same(bool(nonpassing), identity in frozen['checkout_expected'] or identity in frozen['local_expected'], 'Unclassified nonpassing control')
        local_expected = frozen['local_expected'].get(identity,checkout if identity in frozen['checkout_expected'] else expected)
        proof.same(set(local_expected), {'id','outcome','trace','stdout'}, 'Incomplete actual local packet')
        proof.same(local_expected['id'], identity, 'Detached actual local packet')
        rows.append({**source, 'original_expected':copy.deepcopy(expected),
                     'canonical_expected':copy.deepcopy(expected),
                     'checkout_expected':copy.deepcopy(checkout),
                     'local_expected':copy.deepcopy(local_expected),
                     'two_pin_agreement':checkout == expected,
                     'canonical_acceptance':not bool(nonpassing),
                     'nonpassing_reason':nonpassing})
    proof.same(len({row['id'] for row in rows}), len(rows), 'Duplicate source identity')
    proof.same(rows[0]['source'], ORIGINAL, 'Changed exact original count/membership source')
    proof.same(rows[1]['source'], ORIGINAL_FULL, 'Changed exact original full sequence source')
    proof.same(rows[1]['canonical_expected']['outcome']['value']['$vector'],
               [4,True,True,{'$vector':['a','b',{'$list':[{'$symbol':{'namespace':None,'name':'quote','spelling':'quote'}},'b']},'a']}],
               'Original complete traversal order drift')
    proof.same([sum(not r['two_pin_agreement'] for r in rows),sum(not r['canonical_acceptance'] for r in rows)], [2,4], 'Missing set traversal neighboring controls')
    return rows


def snapshot():
    paths = [Path(__file__), DRIVER, CONTRACTS, Path(__file__).with_name('test_literal_set_traversal.py')]
    return {**proof.snapshot(), **{str(p.relative_to(ROOT)):sci.digest(p.read_bytes()) for p in paths}}


def references(root, rows, programs):
    result = {}
    for kind in ('release','checkout'):
        pin_rows = copy.deepcopy(rows)
        if kind == 'checkout':
            for row in pin_rows:
                row['canonical_expected'] = row['checkout_expected']
        result[kind] = proof.verify_reference(kind,root/kind,pin_rows,programs)
        for row in rows:
            proof.same(result[kind][row['id']]['status'],'passed', 'Set traversal differs from independent '+kind+' pin: '+row['id'])
    return result


def verified_document(directory):
    root = Path(directory).resolve()
    before = snapshot()
    rows = load()
    binding = json.loads((root/'binding.json').read_text())
    proof.same(binding['producer_source_sha256'], before, 'Set traversal/compiler producer drift')
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
    with patch.object(proof,'DRIVER',DRIVER), patch.object(proof,'TEST',TEST):
        frames = proof.verify_local(root/'phases',local_rows,binding)
    for row in rows:
        proof.same(list(frames[row['id']]),[v+'/'+e for v in proof.VARIANTS for e in ('QuickJS','Node')], 'Missing eight actual contexts')
        for frame in frames[row['id']].values():
            proof.same(frame['status'],'passed','Actual set traversal mismatch: '+row['id'])
            frame['local_regression_status'] = 'passed'
            if not row['canonical_acceptance']:
                frame['status'] = 'canonical-version-difference' if not row['two_pin_agreement'] else 'canonical-local-mismatch'
                frame['nonpassing_reason'] = row['nonpassing_reason']
            frame['two_pin_agreement_credit'] = 1 if row['canonical_acceptance'] else 0
    proof.same(before,snapshot(),'Producer changed during replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'source_bindings':rows,
            'execution_binding':binding,'compiler_build_producer_source_sha256':before,
            'references':refs,'frames':frames,'canonical_contract_status':'passed',
            'actual_macro_contract_status':'passed','status':'passed',
            'accepted_source_count':sum(r['canonical_acceptance'] for r in rows),
            'accepted_engine_frame_count':8*sum(r['canonical_acceptance'] for r in rows),
            'nonpassing_neighbor_count':sum(not r['canonical_acceptance'] for r in rows),
            'original_semantic_pass_credit':0,'full_language_completion_credit':0}


def reconcile(document):
    proof.same(document.get('profile'),PROFILE,'Wrong evidence profile')
    root = document.get('artifacts_root')
    if not isinstance(root,str) or str(Path(root).resolve()) != root:
        raise ValueError('Missing normalized raw artifact root')
    replay = verified_document(root)
    proof.same(document,replay,'Set traversal evidence differs from raw executable replay')
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
        print('Fresh '+kind+' set traversal observations completed',flush=True)
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
    print('Verified '+str(len(rows)*8)+' actual set traversal source-phase engine frames',flush=True)
    return document


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    parser.add_argument('--timeout',default=600,type=int)
    args=parser.parse_args()
    run(args.output,args.timeout)
