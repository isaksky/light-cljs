#!/usr/bin/env python3
"""Bind the complete after190 source calls to saved actual and required-pin runs."""
from pathlib import Path
import argparse
import json
import shutil
import subprocess
import arity_after190 as cohort


def frozen_snapshot():
    root = cohort.ROOT
    paths = set(subprocess.check_output(['git', 'ls-files', 'src', 'build.rs', 'Cargo.toml', 'Cargo.lock'], cwd=root, text=True).splitlines())
    paths.update([cohort.HELPER, cohort.DRIVER, cohort.SCANNER,
        'tests/reference/compiler-language/arity_after190_reference.py',
        'tests/reference/compiler-language/arity_after190_proof.py',
        'tests/reference/compiler-language/arity_after190_raw_guard.py',
        'tests/reference/compiler-language/test_arity_after190.py',
        'tests/reference/compiler-language/report.py',
        'tests/reference/compiler-language/extract_inventory.py',
        'tests/reference/execution.clj', 'tests/support/execution.js',
        'tests/fixtures/compiler-language.tsv',
        'tests/reference/compiler-residual-source-context-proposals.json'])
    paths.update('tests/reference/compiler-language/' + filename for filename in [
        'full_ledger.py', 'full-manifest.json', 'full-inventory.jsonl', 'full-bodies.jsonl',
        'obligations.jsonl', 'support-sources.jsonl', 'full-cases.jsonl', 'full-witnesses.jsonl',
        'witnesses.jsonl', 'obligation-witnesses.jsonl', 'phase-witnesses.jsonl',
        'sci-source-context-receipts.jsonl', 'sci-library-context-receipts.jsonl'])
    paths.update(cohort.PRINT_SUPPORT + cohort.CONTEXT_SUPPORT)
    paths.update(row['fixture'] for row in cohort.REVIEWED)
    for row in map(json.loads, (cohort.HERE / 'phase-witnesses.jsonl').read_text().splitlines()):
        paths.add(row['test_file'])
        paths.update(row.get('support_source_sha256', {}))
    return {'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
        'source_sha256': {path: cohort.digest((root / path).read_bytes()) for path in sorted(paths)}}


def produce(output, actual, guards, references, before=None):
    root = cohort.ROOT
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    rows = cohort.rows()
    exports = cohort.exports()
    before_path = Path(before).resolve() if before else None
    frozen_after = frozen_snapshot() if before_path else None
    if before_path and json.loads(before_path.read_text()) != frozen_after:
        raise ValueError('frozen before/current producer, catalog, integration or compiler source drift')
    raw = Path(actual).read_text()
    frames = cohort.parse_frames(raw)
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in raw:
        raise ValueError('actual named Rust test did not pass exactly once')
    artifacts = [json.loads(line) for line in raw.splitlines() if line.startswith('{')]
    executables = [a['executable'] for a in artifacts if a.get('reason') == 'compiler-artifact' and a.get('target', {}).get('name') == 'compiler_arity_after190' and a.get('executable')]
    if len(executables) != 1:
        raise ValueError('exact test executable metadata unavailable')
    guard_raw = Path(guards).read_text()
    if 'Ran 22 tests' not in guard_raw or not guard_raw.rstrip().endswith('OK'):
        raise ValueError('all twenty-two source and actual-frame guards must pass')
    producer_paths = [cohort.HELPER, cohort.DRIVER, cohort.SCANNER,
        'tests/reference/compiler-language/arity_after190_reference.py',
        'tests/reference/compiler-language/arity_after190_proof.py',
        'tests/reference/compiler-language/test_arity_after190.py',
        'tests/reference/compiler-language/report.py',
        'tests/reference/compiler-language/extract_inventory.py',
        'tests/reference/execution.clj', 'tests/support/execution.js',
        'tests/fixtures/compiler-language.tsv',
        'tests/reference/compiler-language/full-cases.jsonl']
    producer_paths += cohort.PRINT_SUPPORT + cohort.CONTEXT_SUPPORT
    source_hashes = {p: cohort.digest((root / p).read_bytes()) for p in producer_paths}
    pin_proof = {}
    for kind in ['release', 'checkout']:
        path = Path(references) / kind / 'receipt.json'
        pin = json.loads(path.read_text())
        if set(pin['observations']) != {row['id'] for row in rows} or any(value['status'] != 'passed' for value in pin['observations'].values()):
            raise ValueError('pin did not pass every exact original contract')
        if pin['original_case_sha256'] != {row['id']: row['case_sha256'] for row in rows}:
            raise ValueError('pin original whole-case contract drift')
        for row in rows:
            if pin['observations'][row['id']]['observed'] != row['case']['expected']:
                raise ValueError('pin independent original result/effect contract drift')
        identity = 'release '+__import__('report').RELEASE if kind=='release' else 'checkout '+__import__('report').REVISION
        if pin['reference_identity'] != identity:
            raise ValueError('wrong required pin identity')
        build_stdout = (path.parent / 'build/stdout').read_text()
        if build_stdout.count('REFERENCE IDENTITY '+identity)!=2 or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in build_stdout:
            raise ValueError('actual pin build identities/settings unavailable')
        build_command=json.loads((path.parent/'build/invocation.json').read_text())
        node_command=json.loads((path.parent/'node/invocation.json').read_text())
        canonical_root=path.parent.resolve()
        expected_input='process.chdir('+json.dumps(str(canonical_root))+');require('+json.dumps(str(canonical_root/'reference.js'))+');\n'+(root/'tests/support/execution.js').read_text()
        if node_command!=pin['node_command'] or node_command[-1:]!=['-'] or (path.parent/'node/stdin').read_text()!=expected_input:
            raise ValueError('exact reference Node command/input drift')
        if build_command!=pin['build_command'] or build_command[:3]!=['clojure','-Srepro','-Sdeps'] or build_command[-6:]!=[str(canonical_root/'build.clj'),kind,identity.split(' ',1)[1],'-' if kind=='release' else str(root/'research/clojurescript'),str(canonical_root),str(canonical_root/'source')]:
            raise ValueError('exact reference canonical build command drift')
        for relative, field in [('build/invocation.json','build_invocation_sha256'),('node/invocation.json','node_invocation_sha256'),('node/stdin','node_stdin_sha256')]:
            if cohort.digest((path.parent/relative).read_bytes())!=pin[field]:
                raise ValueError('actual reference command/input artifact drift')
        for relative, field in [('build.clj', 'build_script_sha256'),
                ('build/stdout', 'build_stdout_sha256'), ('reference.js', 'reference_js_sha256'),
                ('node/stdout', 'raw_stdout_sha256')]:
            if cohort.digest((path.parent / relative).read_bytes()) != pin[field]:
                raise ValueError('actual raw reference output/build drift')
        actual_sources = {str(p.relative_to(path.parent / 'source')): cohort.digest(p.read_bytes())
            for p in sorted((path.parent / 'source').rglob('*')) if p.is_file()}
        if actual_sources != pin['source_file_sha256']:
            raise ValueError('actual canonical source files drift')
        observations = [json.loads(line) for line in (path.parent / 'node/stdout').read_text().splitlines()]
        if observations != [row['case']['expected'] for row in rows]:
            raise ValueError('raw pin observations differ from exact original independent contracts')
        for name, digest in pin['producer_source_sha256'].items():
            if source_hashes[name] != digest:
                raise ValueError('actual pin producer source drift: ' + name)
        pin_proof[kind] = {'receipt_path': str(path.resolve()), 'receipt_sha256': cohort.digest(path.read_bytes()), **pin}
    production_paths = subprocess.check_output(['git', 'ls-files', 'src', 'build.rs', 'Cargo.toml', 'Cargo.lock'], cwd=root, text=True).splitlines()
    module_root = output / 'modules'
    module_hashes = {str(path.relative_to(module_root)): cohort.digest(path.read_bytes()) for path in sorted(module_root.rglob('*')) if path.is_file()}
    artifacts = [line.split('\t') for line in raw.splitlines() if line.startswith('arity-after190-artifacts\t')]
    artifact_keys=[(row['id'],host,phase) for row in rows for host in ['fresh','reused'] for phase in ['application','macro']]
    if len(artifacts)!=len(artifact_keys) or any(len(fields)!=6 for fields in artifacts) or [(fields[1],fields[2],fields[3]) for fields in artifacts]!=artifact_keys:
        raise ValueError('complete actual source/emitted input hash frames required')
    for fields in artifacts:
        row=next(row for row in rows if row['id']==fields[1])
        base='/'.join(fields[1:4])+'/'
        relative=row['namespace'].replace('.', '/').replace('-', '_')+'.cljs'
        if module_hashes.get(base+'compiled.js')!=fields[4] or module_hashes.get(base+relative)!=fields[5]:
            raise ValueError('saved artifacts differ from actual executed source/emitted input')
    companion_frames = [line.split('\t') for line in raw.splitlines() if line.startswith('arity-after190-companion-artifacts\t')]
    expected_companions = [(row['id'], host, phase, relative, companion['source_sha256']) for row in rows for host in ['fresh', 'reused'] for phase in ['application', 'macro'] for relative, companion in row.get('companion_sources', {}).items()]
    if any(len(fields) != 6 for fields in companion_frames) or [tuple(fields[1:]) for fields in companion_frames] != expected_companions:
        raise ValueError('complete actual namespace companion input hash frames required')
    for id, host, phase, relative, digest in expected_companions:
        if module_hashes.get('/'.join([id,host,phase,relative])) != digest:
            raise ValueError('saved companion differs from actual compiler input')
    expected_files = {'warmup/warm.cljs', 'warmup.js'}
    for row in rows:
        for host in ['fresh', 'reused']:
            for phase in ['application', 'macro']:
                relative = row['namespace'].replace('.', '/').replace('-', '_') + '.cljs'
                base = row['id'] + '/' + host + '/' + phase + '/'
                expected_files.update([base + relative, base + 'compiled.js'])
                expected_files.update(base + path for path in row.get('companion_sources', {}))
                source, invoke = row['source'], row['invoke']
                if invoke:
                    program = source + ('\n(defmacro arity-witness [] ' + invoke + ')\n(pr-str (arity-witness))\n' if phase == 'macro' else '\n(pr-str ' + invoke + ')\n')
                else:
                    program = '(ns user)\n' + ('(defmacro arity-witness [] (do ' + source + '))\n(pr-str (arity-witness))\n' if phase == 'macro' else '(pr-str (do ' + source + '))\n')
                if (module_root / (base + relative)).read_text() != program:
                    raise ValueError('actual saved whole source or original callback drift')
    if set(module_hashes) != expected_files:
        raise ValueError('every actual phase must retain its module source and emitted JavaScript')
    for name, records in exports.items():
        (output / name).write_text(''.join(json.dumps(row, separators=(',', ':')) + '\n' for row in records))
    shutil.copyfile(actual, output / 'actual.log')
    shutil.copyfile(guards, output / 'guards.log')
    receipt = {
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
        'compiler_source_sha256': {p: cohort.digest((root / p).read_bytes()) for p in production_paths},
        'producer_source_sha256': source_hashes,
        'integration_snapshot_sha256': {p: cohort.digest((root / p).read_bytes()) for p in [
            'tests/reference/compiler-language/full_ledger.py',
            'tests/reference/compiler-language/obligation-witnesses.jsonl',
            'tests/reference/compiler-language/phase-witnesses.jsonl',
            'tests/reference/compiler-language/full-manifest.json']},
        'fixture_source_sha256': {row['fixture']: row['source_sha256'] for row in rows},
        'original_case_sha256': {row['id']: row['case_sha256'] for row in rows},
        'actual_command': 'LIGHT_ARITY_AFTER190_ARTIFACTS=' + str(module_root) + ' cargo test --locked --message-format=json --test compiler_arity_after190 ' + cohort.TEST + ' -- --exact --nocapture',
        'actual_log_sha256': cohort.digest(raw), 'actual_frames': frames,
        'actual_executable': executables[0], 'actual_executable_sha256': cohort.digest(Path(executables[0]).read_bytes()),
        'guard_log_sha256': cohort.digest(guard_raw), 'guard_tests_passed': 22,
        'actual_namespace_companion_input_frames': expected_companions,
        'module_source_and_emitted_js_sha256': module_hashes,
        'exports': exports, 'references': pin_proof,
        'strict_complete': False, 'source_site_credit_added': 0,
    }
    if before_path:
        after_path = output / 'snapshot-after.json'
        after_path.write_text(json.dumps(frozen_after, indent=2) + '\n')
        receipt.update(frozen_before_snapshot_path=str(before_path),
            frozen_before_snapshot_sha256=cohort.digest(before_path.read_bytes()),
            frozen_after_snapshot_sha256=cohort.digest(after_path.read_bytes()),
            frozen_source_snapshot=frozen_after,
            frozen_before_after_equal=True,
            installed_ordinary_and_phase_loaders='passed in exact census guard; all 94 rows matched generated cohort')
    path = output / 'receipt.json'
    path.write_text(json.dumps(receipt, indent=2) + '\n')
    print(str(path), cohort.digest(path.read_bytes()))
    return receipt


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--actual', type=Path, required=True)
    parser.add_argument('--guards', type=Path, required=True)
    parser.add_argument('--references', type=Path, required=True)
    parser.add_argument('--before', type=Path)
    args = parser.parse_args()
    produce(args.output, args.actual, args.guards, args.references, args.before)
