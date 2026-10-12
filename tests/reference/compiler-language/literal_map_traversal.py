"""Independent two-pin, eight-frame literal map traversal proof."""
from pathlib import Path
from unittest.mock import patch
import argparse
import copy
import json
import os

import sci_cljc_accepted_context as proof
import sci_execution as sci

ROOT = sci.ROOT
PROFILE = 'canonical-literal-map-traversal-v1'
DRIVER = ROOT/'tests/compiler_literal_map_traversal.rs'
TEST = 'literal_map_traversal_sources_execute_in_fresh_and_reused_macro_hosts'
CONTRACTS = Path(__file__).with_name('literal-map-traversal-contracts.json')
CONTRACTS_SHA256 = 'e5e11c4ba856d8864673ee2eafe5d72c0a5fef392ca17943a99c730f42adcb16'
ORIGINAL = '(let [events (atom []) m {:a (do (swap! events conj :a) 1) :b (do (swap! events conj :b) 2) :c (do (swap! events conj :c) 3) :d (do (swap! events conj :d) 4) :e (do (swap! events conj :e) 5) :f (do (swap! events conj :f) 6) :g (do (swap! events conj :g) 7) :h (do (swap! events conj :h) 8) :i (do (swap! events conj :i) 9)}] [(count m) @events])'
ORIGINAL_SHA256 = '253f3e88e82fbcb2c6d7378281fbf75bcb1143a3def580e3476824c568101433'
SOURCE_COUNT = 39


def exact_observation(row, observed):
    """Retain the full raw typed packet; apply no SCI comparison projection."""
    proof.same(set(observed), {'id','outcome','trace','stdout'}, 'Invalid complete typed map observation')
    proof.same(observed['id'], row['id'], 'Wrong exact map observation identity')
    if not isinstance(observed['outcome'],dict) or not isinstance(observed['trace'],list) or not isinstance(observed['stdout'],str):
        raise ValueError('Invalid complete typed map observation fields')
    equal = proof.report.same_json
    return {'status':'passed' if equal(observed,row['canonical_expected']) else 'mismatch',
            'observed':observed,
            'original_status':'passed' if equal(observed,row['original_expected']) else 'mismatch'}


def load():
    raw = CONTRACTS.read_bytes()
    proof.same(sci.digest(raw), CONTRACTS_SHA256, 'Unreviewed independently derived map traversal contract')
    frozen = json.loads(raw)
    proof.same(frozen['profile'], PROFILE, 'Wrong map traversal contract profile')
    proof.same(frozen['comparison'], 'exact-typed-packets-no-projection', 'Map traversal comparison normalization drift')
    proof.same(frozen['pins'], {'release':'1.12.134','checkout':'a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e'}, 'Wrong pinned references')
    proof.same(len(frozen['sources']), SOURCE_COUNT, 'Missing complete map traversal source catalog')
    proof.same(len(frozen['canonical_expected']), SOURCE_COUNT, 'Missing independent expected map packets')
    identities = {r['id'] for r in frozen['sources']}
    for name in ('checkout_expected','local_expected','nonpassing_neighbors'):
        if set(frozen[name])-identities: raise ValueError('Detached map neighboring contract')
    rows = []
    for source, expected in zip(frozen['sources'], frozen['canonical_expected']):
        identity = source['id']
        proof.same(sci.digest(source['source']), source['source_sha256'], 'Changed exact map source bytes')
        proof.same(set(expected), {'id','outcome','trace','stdout'}, 'Incomplete typed map pin packet')
        proof.same(expected['id'], identity, 'Detached expected map packet')
        proof.same([expected['trace'], expected['stdout']], [[], ''], 'Wrong exact effects/stdout packet')
        checkout = frozen['checkout_expected'].get(identity, expected)
        nonpassing = frozen['nonpassing_neighbors'].get(identity)
        proof.same(checkout != expected, identity in frozen['checkout_expected'], 'Unclassified map two-pin difference')
        proof.same(bool(nonpassing), identity in frozen['checkout_expected'] or identity in frozen['local_expected'], 'Unclassified nonpassing map control')
        local_expected = frozen['local_expected'].get(identity,checkout if identity in frozen['checkout_expected'] else expected)
        for packet in (checkout,local_expected):
            proof.same(set(packet), {'id','outcome','trace','stdout'}, 'Incomplete neighboring map packet')
            proof.same(packet['id'], identity, 'Detached neighboring map packet')
        rows.append({**source, 'original_expected':copy.deepcopy(expected),
                     'canonical_expected':copy.deepcopy(expected),
                     'checkout_expected':copy.deepcopy(checkout),
                     'local_expected':copy.deepcopy(local_expected),
                     'two_pin_agreement':checkout == expected,
                     'canonical_acceptance':not bool(nonpassing),
                     'nonpassing_reason':nonpassing})
    proof.same(len(identities), len(rows), 'Duplicate map source identity')
    proof.same(rows[0]['source'], ORIGINAL, 'Changed exact original nine-entry map source')
    proof.same(rows[0]['source_sha256'], ORIGINAL_SHA256, 'Changed original map source SHA256')
    original = rows[0]['canonical_expected']['outcome']['value']['$vector']
    proof.same(original[0],9,'Original count drift')
    proof.same([k['$keyword']['name'] for k in original[1]['$vector']],list('egchbdfia'),'Original complete map effect order drift')
    proof.same([sum(not r['two_pin_agreement'] for r in rows),sum(not r['canonical_acceptance'] for r in rows)], [0,15], 'Missing map traversal neighboring controls')
    controls = frozen['rejected_derivation_controls']
    proof.same(len(controls),1,'Missing exact rejected derivation control')
    control=controls[0]
    proof.same(sci.digest(control['source']),control['source_sha256'],'Rejected derivation source drift')
    proof.same([control['canonical_acceptance'],control['two_pin_agreement_credit']],[False,0],'Rejected derivation granted credit')
    for kind in ('release','checkout'):
        proof.same([control['references'][kind]['status'],control['references'][kind]['detail']],['rejected','Duplicate key: (1 2)'],'Changed independently rejected derivation diagnostic')
    return rows


def snapshot():
    paths = [Path(__file__), DRIVER, CONTRACTS, Path(__file__).with_name('test_literal_map_traversal.py')]
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



def rejected_derivation_controls(programs):
    controls=json.loads(CONTRACTS.read_text())['rejected_derivation_controls']
    original_rows=load()[:33]
    for control in controls:
        for row in original_rows:
            if row['id']==control['id']:
                row.update(source=control['source'],source_sha256=control['source_sha256'],
                           canonical_expected=None,original_expected=None)
        for kind in ('release','checkout'):
            captured=control['references'][kind]
            root=Path(captured['artifact_root'])
            for name,digest in captured['artifact_sha256'].items():
                proof.same(sci.digest((root/name).read_bytes()),digest,'Rejected control raw artifact drift')
            with patch.object(proof,'observation',exact_observation):
                frames=proof.verify_reference(kind,root,original_rows,programs)
            proof.same(frames[control['id']]['status'],'rejected','Rejected derivation granted credit')
            proof.same(frames[control['id']]['detail'],captured['detail'],'Rejected control diagnostic drift')
            for row in original_rows:
                if row['id']!=control['id']:
                    proof.same(frames[row['id']]['status'],'passed','Original derivation sibling reference drift')
    return controls


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
    controls=rejected_derivation_controls(binding['programs'])
    local_rows = copy.deepcopy(rows)
    for row in local_rows:
        row['canonical_expected'] = row['local_expected']
    with patch.object(proof,'DRIVER',DRIVER), patch.object(proof,'TEST',TEST), patch.object(proof,'observation',exact_observation):
        frames = proof.verify_local(root/'phases',local_rows,binding)
    for row in rows:
        proof.same(list(frames[row['id']]),[v+'/'+e for v in proof.VARIANTS for e in ('QuickJS','Node')], 'Missing eight actual contexts')
        for frame in frames[row['id']].values():
            proof.same(frame['status'],'passed','Actual map traversal mismatch: '+row['id'])
            frame['local_regression_status'] = 'passed'
            if not row['canonical_acceptance']:
                frame['status'] = 'canonical-version-difference' if not row['two_pin_agreement'] else 'canonical-local-mismatch'
                frame['nonpassing_reason'] = row['nonpassing_reason']
            frame['two_pin_agreement_credit'] = 1 if row['canonical_acceptance'] else 0
    proof.same(before,snapshot(),'Producer changed during replay')
    return {'schema_version':1,'profile':PROFILE,'artifacts_root':str(root),'source_bindings':rows,
            'execution_binding':binding,'compiler_build_producer_source_sha256':before,
            'references':refs,'frames':frames,'rejected_derivation_controls':controls,'canonical_contract_status':'passed',
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
        print('Fresh '+kind+' map traversal observations completed',flush=True)
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
    print('Verified '+str(len(rows)*8)+' actual map traversal source-phase engine frames',flush=True)
    return document


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    parser.add_argument('--timeout',default=600,type=int)
    args=parser.parse_args()
    run(args.output,args.timeout)
