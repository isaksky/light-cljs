"""Fresh numeric case reader identity versus Number execution in both phases."""
from pathlib import Path
from unittest.mock import patch
import argparse, json, os
import sci_cljc_accepted_context as proof
import sci_execution as sci
import case_char_string_accepted as character

ROOT = sci.ROOT
PROFILE = 'canonical-numeric-case-v1'
DRIVER = ROOT/'tests/compiler_case_numeric.rs'
TEST = 'numeric_case_constants_execute_in_fresh_and_reused_macro_hosts'
CONTRACTS = Path(__file__).with_name('case-numeric-contracts.json')
CONTRACTS_SHA256 = 'd9a2155768fbcea4850ebcd8fc89d3fa0b95fa58d430596dec123626ea8f2784'


def load():
    raw = CONTRACTS.read_bytes()
    proof.same(sci.digest(raw), CONTRACTS_SHA256, 'Unreviewed independent numeric case catalog')
    rows = json.loads(raw)['sources']
    proof.same(len(rows), 27, 'Missing numeric case source')
    proof.same(len({r['id'] for r in rows}), len(rows), 'Duplicate numeric case source')
    for row in rows:
        proof.same(sci.digest(row['source']), row['source_sha256'], 'Numeric source drift')
        if row['canonical_expected'] is not None:
            proof.same(row['canonical_expected']['id'], row['id'], 'Detached canonical packet')
            proof.same([row['canonical_expected']['trace'], row['canonical_expected']['stdout']], [[], ''], 'Wrong exact effects/stdout')
    accepted = [r for r in rows if r['canonical_expected'] is not None]
    rejected = [r for r in rows if r['canonical_expected'] is None]
    proof.same([len(accepted), len(rejected)], [16, 11], 'Missing accepted/diagnostic controls')
    return accepted, rejected


def snapshot():
    paths = [Path(__file__), DRIVER, CONTRACTS, Path(character.__file__),
             Path(__file__).with_name('test_case_numeric.py')]
    return {**proof.snapshot(), **{str(p.relative_to(ROOT)): sci.digest(p.read_bytes()) for p in paths}}


def references(root, accepted, rejected, programs):
    result = {kind: proof.verify_reference(kind, root/kind, accepted+rejected, programs)
              for kind in ('release', 'checkout')}
    for kind, frames in result.items():
        for row in accepted:
            proof.same(frames[row['id']]['status'], 'passed', 'Reviewed numeric literal differs from '+kind)
        for row in rejected:
            proof.same(frames[row['id']]['status'], 'rejected', 'True numeric duplicate accepted by '+kind)
            if row['pin_diagnostic'] not in frames[row['id']]['detail']:
                raise ValueError('Wrong numeric duplicate diagnostic')
    return result


def verified_document(directory):
    root = Path(directory).resolve(); before = snapshot(); accepted, rejected = load()
    binding = json.loads((root/'binding.json').read_text())
    proof.same(binding['producer_source_sha256'], before, 'Numeric/compiler producer drift')
    proof.same(binding['programs'], proof.executable_paths(), 'Executable path drift')
    proof.same(set(binding['executables_sha256']), set(binding['programs'].values())|{binding['audit_executable']}, 'Missing executable hash')
    for path, expected in binding['executables_sha256'].items():
        proof.same(sci.digest(Path(path).read_bytes()), expected, 'Actual executable drift')
    proof.same(binding['audit_environment'], {'LIGHT_CASE_NUMERIC_ROOT': str(root/'phases'), 'NODE': binding['programs']['node']}, 'Wrong numeric phase environment')
    proof.same(binding['artifact_sha256'], proof.tree(root, ('binding.json', 'evidence.json')), 'Raw artifact census/hash drift')
    refs = references(root, accepted, rejected, binding['programs'])
    with patch.object(proof, 'DRIVER', DRIVER), patch.object(proof, 'TEST', TEST):
        frames = proof.verify_local(root/'phases', accepted, binding)
    for row in accepted:
        proof.same(list(frames[row['id']]), [v+'/'+e for v in proof.VARIANTS for e in ('QuickJS', 'Node')], 'Missing eight actual numeric contexts')
        for frame in frames[row['id']].values():
            proof.same(frame['status'], 'passed', 'Numeric phase disagrees with independent canonical literal')
    diagnostics = character.verify_diagnostics(root/'diagnostics', rejected)
    proof.same(before, snapshot(), 'Producer changed during numeric replay')
    return {'schema_version': 1, 'profile': PROFILE, 'artifacts_root': str(root),
            'source_bindings': accepted, 'diagnostic_sources': rejected,
            'execution_binding': binding, 'references': refs, 'frames': frames,
            'diagnostics': diagnostics, 'status': 'passed', 'numeric_execution': 'JavaScript Number',
            'original_semantic_pass_credit': 0, 'full_language_completion_credit': 0}


def reconcile(document):
    proof.same(document.get('profile'), PROFILE, 'Wrong numeric proof profile')
    root = document.get('artifacts_root')
    if not isinstance(root, str) or str(Path(root).resolve()) != root:
        raise ValueError('Missing normalized numeric artifact root')
    replay = verified_document(root)
    proof.same(document, replay, 'Numeric summary differs from full raw/executable replay')
    return replay


def run(directory, timeout=600):
    root = Path(directory).resolve(); root.mkdir(parents=True, exist_ok=False)
    before = snapshot(); accepted, rejected = load(); programs = proof.executable_paths()
    hashes = {path: sci.digest(Path(path).read_bytes()) for path in programs.values()}
    for kind in ('release', 'checkout'):
        proof.produce_reference(kind, root/kind, accepted+rejected, programs, timeout)
        print('Fresh '+kind+' numeric case source observations completed', flush=True)
    references(root, accepted, rejected, programs)
    phases = root/'phases'; phases.mkdir(); (phases/'input.tsv').write_text(proof.input_text(accepted))
    diagnostics = root/'diagnostics'; diagnostics.mkdir(); (diagnostics/'input.tsv').write_text(proof.input_text(rejected))
    build = proof.command([programs['cargo'], 'test', '--locked', '--test', DRIVER.stem, '--no-run', '--message-format=json'], phases/'build', timeout)
    emitted = [r['executable'] for r in map(json.loads, build.stdout.splitlines()) if r.get('reason')=='compiler-artifact' and r.get('target', {}).get('name')==DRIVER.stem and r.get('executable')]
    proof.same(len(emitted), 1, 'Missing actual Cargo-built audit binary'); binary = emitted[0]
    hashes[binary] = sci.digest(Path(binary).read_bytes())
    env = {'LIGHT_CASE_NUMERIC_ROOT': str(phases), 'NODE': programs['node']}
    proof.command([binary, TEST, '--ignored', '--exact', '--nocapture'], phases/'audit', 300, environment={**os.environ, **env}, record_environment=env)
    proof.same(before, snapshot(), 'Source drift during numeric production')
    binding = {'producer_source_sha256': before, 'programs': programs, 'executables_sha256': hashes,
               'audit_executable': binary, 'audit_environment': env, 'artifact_sha256': proof.tree(root)}
    (root/'binding.json').write_text(json.dumps(binding, indent=2)+'\n')
    document = verified_document(root); (root/'evidence.json').write_text(json.dumps(document, indent=2)+'\n')
    reconcile(document)
    print('Verified 128 accepted numeric phase/engine frames and 88 same-host recovery outcomes after 44 true-duplicate rejections', flush=True)
    return document


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('--output', required=True, type=Path)
    run(parser.parse_args().output)
