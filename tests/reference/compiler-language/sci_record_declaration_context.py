"""Three whole original SCI declaration replacement/marker sources, explicit pin differences.

The immutable SCI outcomes remain unresolved. No original semantic pass, broad
defrecord exclusion, public API/body/program/assertion credit is inferred.
"""
from __future__ import annotations
import argparse
import copy
import json
import os
from pathlib import Path
import shutil
import configured_source_context as runner
import sci_execution as sci
import sci_source_context as context
import sci_symbol_keyword_context as packets
from extract_inventory import Reader

ROOT = sci.ROOT
PROFILE = 'original-sci-record-declaration-canonical-difference-v1'
STATUS = 'verified-canonical-sci-difference'
DRIVER = ROOT/'tests/compiler_sci_record_declaration_context.rs'
TEST = 'original_record_declarations_execute_in_both_phases_and_lifecycles'
VARIANTS = ('application-fresh', 'macro-fresh', 'application-reused', 'macro-reused')
SPECS = (('sci-record-redefine', 'ba409db7254584aff60bd8f13a4949b5f017ddf4d96ad44fa9aeb28508f1c5d3', 'a43e1a195ba55d3187e2acf2630d93cec36517c9da1cb4c8b715b7e1e26e4922', '0029e7547d41b5ee2ce7df157e062a262e3c75eef362d6da28fa301b2c4b20d0', {'$vector': [False, True, True]}), ('sci-record-method-same-shape', 'b562b3b1f32e68b88feffb3f4529c558b0c3d0cda56b01d29537c05e1ea3e29b', '98de8f608da2668e3f1a5846af7762227d454340a7d7f9df91a63affd07dd158', '423c4021276c037ba6a267ea9b93663f75c9a0e087ac128cb210958201f6ccd4', {'$vector': [False, True, 2, 2]}), ('sci-protocol-marker-return', 'c80d7ea143741fdec45717e8b43b90e57ea7b66f2cc52fcde6065b5ab0947f6e', 'f2f7e1bd040542a309c9e46d185117362ddafe593bd5ba5930b60bc5ec0810f4', '774713a61cdae4bf95aa8ef852e915b27eea2e685f94ce9ef758940520505cba', {'$vector': [False, False, False]}))


def same(actual, expected, reason):
    if not sci.report_module().same_json(actual, expected): raise ValueError(reason)


def load():
    corpus = sci.rows(sci.CORPUS); dispositions = sci.rows(sci.DISPOSITIONS); rows = []
    for identity, source_hash, row_hash, body_hash, value in SPECS:
        raw, fields = corpus[identity]; body, disposition = dispositions[identity]
        if (fields != disposition[:7] or sci.digest(raw) != row_hash or sci.digest(body) != body_hash
                or fields[1] != 'portable' or sci.digest(fields[2]) != source_hash):
            raise ValueError('Original full source/expectation/disposition drift')
        original = fields[2]; last = Reader(original).all()[-1]; source_id = 'sci-execution-'+identity
        canonical = {'id': source_id, 'outcome': {'status': 'returned', 'value': copy.deepcopy(value)}, 'trace': [], 'stdout': ''}
        original_expected = sci.expected(fields)
        if sci.report_module().same_json(original_expected, canonical):
            raise ValueError('Canonical difference collapsed into an original SCI pass')
        rows.append({'id': source_id, 'sci_id': identity,
            'obligation_id': 'local::tests/reference/sci/evaluator-dispositions.tsv::'+identity,
            'source': original, 'source_sha256': source_hash, 'original_source_sha256': source_hash,
            'original_row_sha256': row_hash, 'body_sha256': body_hash,
            'original_expected': original_expected, 'canonical_expected': canonical,
            'retained_historical_observation': disposition[9:13], 'retained_disposition': disposition[7],
            'setup_source': original[:last.start], 'runtime_source': original[last.start:last.end],
            'source_context_profile': {'schema_version': 1, 'profile': PROFILE, 'namespace': 'user', 'excluded_core_identifiers': [], 'macro_execution': 'entire-unchanged-original-sequential-do'},
            'original_expectation_status': 'unresolved'})
    return rows


def validate(rows):
    same(rows, load(), 'Missing/reordered/drifted full original or explicit canonical contract')
    return rows


def module(row, phase):
    if not any(sci.report_module().same_json(row, candidate) for candidate in load()): raise ValueError('Unreviewed original source binding')
    if row['setup_source']+row['runtime_source'] != row['source']: raise ValueError('Reduced original source')
    if phase == 'application': return '(ns user)\n'+row['setup_source']+'(defn run [] '+row['runtime_source']+')\n'
    if phase == 'macro': return "(ns user)\n(defmacro upstream [] (list 'quote (do "+row['source']+')))\n(defn run [] (upstream))\n'
    raise ValueError('Unknown source phase')


def parse_raw(text, rows):
    validate(rows); lines = [line.split('\t') for line in text.splitlines()]
    order = [(r['id'], variant, engine) for r in rows for variant in VARIANTS for engine in ('QuickJS', 'Node')]
    if len(lines) != 24 or any(len(r) != 7 for r in lines) or [tuple(r[:3]) for r in lines] != order:
        raise ValueError('Missing/duplicate/reordered original source phase frames')
    by_id = {r['id']: r for r in rows}; result = []
    for identity, variant, engine, source_hex, javascript_hex, raw_hex, warmup_hex in lines:
        source, javascript, raw = [bytes.fromhex(v).decode() for v in (source_hex, javascript_hex, raw_hex)]
        if source != module(by_id[identity], variant.split('-')[0]) or javascript.count(' = // Original bounded runtime') != 1:
            raise ValueError('Original whole source/setup/phase/runtime drift')
        if (warmup_hex == '-') != variant.endswith('fresh') or (variant.endswith('reused') and not bytes.fromhex(warmup_hex)):
            raise ValueError('Missing actual reused MacroHost warmup')
        observed = packets.strict_json(raw)
        same(observed, by_id[identity]['canonical_expected'], 'Actual phase disagrees with literal pinned canonical contract')
        if engine == 'Node' and raw != json.dumps(observed, separators=(',', ':'))+'\n':
            raise ValueError('Extra/malformed Node output')
        result.append((identity, variant, engine, source, javascript, raw, warmup_hex, observed))
    for left, right in zip(result[::2], result[1::2]):
        if left[3:5] != right[3:5] or left[6] != right[6]: raise ValueError('Engines detached from compilation')
    return result


def snapshot():
    paths = [Path(__file__), DRIVER, Path(__file__).with_name('test_sci_record_declaration_context.py'),
        Path(__file__).with_name('sci-record-declaration-context.md'), sci.CORPUS, sci.DISPOSITIONS,
        ROOT/'tests/support/sci_execution_observation.js', ROOT/'tests/support/execution.js',
        ROOT/'tests/reference/execution.clj', Path(runner.__file__), Path(sci.__file__),
        Path(context.__file__), Path(packets.__file__), ROOT/'tests/reference/compiler-language/report.py']
    return {**context.compiler_sources(), **{str(p.relative_to(ROOT)): sci.digest(p.read_bytes()) for p in paths}}


def artifact_manifest(directory):
    directory = Path(directory)
    return {str(p.relative_to(directory)): sci.digest(p.read_bytes()) for p in sorted(directory.rglob('*')) if p.is_file()}


def run_local(directory, timeout=240):
    directory = Path(directory).resolve(); directory.mkdir(parents=True, exist_ok=False)
    before = snapshot(); rows = load(); request = directory/'input.tsv'; output = directory/'observations.tsv'
    request.write_text(''.join(r['id']+'\t'+r['source'].encode().hex()+'\t'+json.dumps(r['canonical_expected'], separators=(',', ':')).encode().hex()+'\n' for r in rows))
    build, _ = runner.run(['cargo', 'test', '--locked', '--test', DRIVER.stem, '--no-run', '--message-format=json'], directory/'build', timeout)
    artifacts = [packets.strict_json(line) for line in build.splitlines() if line.startswith('{')]
    binaries = [Path(r['executable']) for r in artifacts if r.get('reason') == 'compiler-artifact'
        and r.get('target', {}).get('name') == DRIVER.stem and r.get('executable')]
    if len(binaries) != 1: raise ValueError('Missing exact audit executable')
    binary = binaries[0]; node = Path(shutil.which(os.environ.get('NODE', 'node')) or '').resolve()
    binary_hash = sci.digest(binary.read_bytes()); node_hash = sci.digest(node.read_bytes())
    env = {**os.environ, 'LIGHT_SCI_RECORD_DECLARATION_INPUT': str(request),
        'LIGHT_SCI_RECORD_DECLARATION_OUTPUT': str(output), 'LIGHT_SCI_RECORD_DECLARATION_ROOT': str(directory/'modules')}
    stdout, _ = runner.run([str(binary), TEST, '--ignored', '--exact', '--nocapture'], directory/'audit', timeout, environment=env)
    if 'running 1 test' not in stdout or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in stdout:
        raise ValueError('Actual source audit not executed')
    frames = {r['id']: {} for r in rows}
    for identity, variant, engine, source, javascript, raw, warmup, observed in parse_raw(output.read_text(), rows):
        frames[identity][variant+'/'+engine] = {'canonical_contract_status': 'passed',
            'original_expectation_status': 'unresolved', 'observed': observed,
            'phase_source_sha256': sci.digest(source), 'compiled_js_sha256': sci.digest(javascript),
            'raw_observation_sha256': sci.digest(raw),
            'warmup_compiled_js_sha256': None if warmup == '-' else sci.digest(bytes.fromhex(warmup)),
            'compiler_build_producer_source_sha256': before, 'audit_executable_sha256': binary_hash,
            'node_executable_sha256': node_hash, 'build_stdout_sha256': sci.digest(build)}
    runner.unchanged(before, snapshot())
    if sci.digest(binary.read_bytes()) != binary_hash or sci.digest(node.read_bytes()) != node_hash: raise ValueError('Actual binary drift')
    result = {'status': STATUS, 'source_bindings': rows, 'frames': frames,
        'compiler_build_producer_source_sha256': before, 'raw_observations_sha256': sci.digest(output.read_bytes()),
        'original_expectation_status': 'unresolved', 'original_semantic_pass_credit': 0}
    (directory/'evidence.json').write_text(json.dumps(result, indent=2)+'\n'); return result


def checked_reference(row, layer, kind):
    report = sci.report_module()
    if layer.get('status') != 'mismatch': raise ValueError('Original SCI mismatch was hidden or incorrectly promoted')
    same(layer.get('observed'), row['canonical_expected'], 'Pin does not establish reviewed canonical literal')
    same(layer.get('settings'), {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False}, 'Pin settings drift')
    same(layer.get('reference_identity'), kind+' '+(report.RELEASE if kind == 'release' else report.REVISION), 'Pin identity drift')
    if report.same_json(layer['observed'], row['original_expected']): raise ValueError('No independently observed original SCI difference')
    return layer


def run(directory, timeout=240):
    directory = Path(directory).resolve(); directory.mkdir(parents=True, exist_ok=False)
    before = snapshot(); rows = load()
    document = {'schema_version': 1, 'profile': PROFILE, 'source_bindings': rows, 'references': {}, 'artifact_root': str(directory),
        'original_expectation_status': 'unresolved', 'original_semantic_pass_credit': 0}
    for kind in ('release', 'checkout'):
        document['references'][kind] = {}
        for row in rows:
            root = directory/'reference-source'/kind/row['id']; root.mkdir(parents=True)
            source = module(row, 'application'); (root/'user.cljs').write_text(source)
            case = {'id': row['id'], 'source': source, 'source_root': str(root), 'entry': 'user', 'export': 'run', 'expected': row['original_expected']}
            layer = sci.report_module().reference_cases([case], kind, ROOT/'research/clojurescript', directory/kind, timeout)[row['id']]
            checked_reference(row, layer, kind); artifact = directory/kind/row['id']
            layer.update(original_source_sha256=row['source_sha256'], phase_source_sha256=sci.digest(source),
                raw_observation_sha256=sci.digest((artifact/'node/stdout').read_bytes()),
                build_stdout_sha256=sci.digest((artifact/'build/stdout').read_bytes()),
                build_stderr_sha256=sci.digest((artifact/'build/stderr').read_bytes()),
                emitted_namespace_sha256=sci.digest((artifact/'out/user.js').read_bytes()))
            document['references'][kind][row['id']] = layer
            print(kind, row['sci_id'], 'original mismatch, canonical literal verified', flush=True)
    document['local'] = run_local(directory/'local', timeout)
    runner.unchanged(before, snapshot())
    document.update(status=STATUS, compiler_build_producer_source_sha256=before)
    document['artifact_sha256'] = artifact_manifest(directory)
    reconcile(document, directory)
    (directory/'evidence.json').write_text(json.dumps(document, indent=2)+'\n'); return document


def reconcile(document, directory=None):
    """No stored sidecar lookup; recheck freshly supplied exact raw evidence."""
    directory = Path(directory if directory is not None else document.get('artifact_root', '/tmp/no-record-declaration-raw-proof')).resolve()
    same(document.get('schema_version'), 1, 'Schema drift')
    same(document.get('profile'), PROFILE, 'Unreviewed source profile')
    same(document.get('source_bindings'), load(), 'Original or canonical expectation/source drift')
    same(document.get('compiler_build_producer_source_sha256'), snapshot(), 'Current producer drift')
    if document.get('status') != STATUS or document.get('original_expectation_status') != 'unresolved':
        raise ValueError('Canonical difference promoted to original SCI pass')
    same(document.get('original_semantic_pass_credit'), 0, 'Original semantic credit drift')
    same(document.get('artifact_root'), str(directory), 'Fresh artifact root drift')
    artifacts = artifact_manifest(directory); artifacts.pop('evidence.json', None)
    same(artifacts, document.get('artifact_sha256'), 'Missing/drifted emitted source/dependency/raw artifacts')
    same(list(document.get('references', {})), ['release', 'checkout'], 'Missing/reordered both pins')
    rows = load(); local = document['local']
    same(local, packets.strict_json((directory/'local/evidence.json').read_text()), 'Detached actual local receipt')
    same(local['compiler_build_producer_source_sha256'], snapshot(), 'Actual local producer drift')
    same(local['source_bindings'], rows, 'Actual local source binding drift')
    same(local.get('original_semantic_pass_credit'), 0, 'Actual local original pass credit drift')
    same(local.get('status'), STATUS, 'Actual local canonical difference status drift')
    same(local.get('original_expectation_status'), 'unresolved', 'Actual local original expectation promoted')
    local_build = (directory/'local/build/stdout').read_text()
    binaries = [Path(r['executable']) for line in local_build.splitlines() if line.startswith('{')
        for r in [packets.strict_json(line)] if r.get('reason') == 'compiler-artifact'
        and r.get('target', {}).get('name') == DRIVER.stem and r.get('executable')]
    if len(binaries) != 1: raise ValueError('Missing exact local audited executable')
    binary_hash = sci.digest(binaries[0].read_bytes())
    node = Path(shutil.which(os.environ.get('NODE', 'node')) or '').resolve()
    node_hash = sci.digest(node.read_bytes())
    same(packets.strict_json((directory/'local/audit/command.json').read_text()),
        [str(binaries[0]), TEST, '--ignored', '--exact', '--nocapture'], 'Actual audit invocation drift')
    same(packets.strict_json((directory/'local/build/command.json').read_text()),
        ['cargo', 'test', '--locked', '--test', DRIVER.stem, '--no-run', '--message-format=json'], 'Actual build invocation drift')
    audit_stdout = (directory/'local/audit/stdout').read_text()
    if 'running 1 test' not in audit_stdout or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in audit_stdout:
        raise ValueError('Actual audit execution not retained')
    raw = (directory/'local/observations.tsv').read_text()
    if sci.digest(raw) != local['raw_observations_sha256']: raise ValueError('Local raw observation drift')
    parsed = parse_raw(raw, rows); result = {}
    same(list(local['frames']), [r['id'] for r in rows], 'Missing/reordered local whole source observations')
    for row in rows:
        frames = local['frames'][row['id']]
        same(list(frames), [v+'/'+e for v in VARIANTS for e in ('QuickJS', 'Node')], 'Missing/reordered actual phase frames')
        for identity, variant, engine, source, javascript, raw_observation, warmup, observed in parsed:
            if identity != row['id']: continue
            f = frames[variant+'/'+engine]
            for key, value in [('observed', observed), ('phase_source_sha256', sci.digest(source)),
                ('compiled_js_sha256', sci.digest(javascript)), ('raw_observation_sha256', sci.digest(raw_observation)),
                ('warmup_compiled_js_sha256', None if warmup == '-' else sci.digest(bytes.fromhex(warmup))),
                ('audit_executable_sha256', binary_hash), ('node_executable_sha256', node_hash),
                ('build_stdout_sha256', sci.digest(local_build)),
                ('compiler_build_producer_source_sha256', snapshot()), ('original_expectation_status', 'unresolved'), ('canonical_contract_status', 'passed')]:
                same(f.get(key), value, 'Actual canonical phase receipt drift')
        references = {}
        for kind in ('release', 'checkout'):
            same(list(document['references'][kind]), [r['id'] for r in rows], 'Missing/reordered complete original pin cases')
            layer = checked_reference(row, document['references'][kind][row['id']], kind)
            artifact = directory/kind/row['id']; source = directory/'reference-source'/kind/row['id']/'user.cljs'
            same(source.read_text(), module(row, 'application'), 'Reduced original pin setup/body')
            same(packets.strict_json((artifact/'node/stdout').read_text()), row['canonical_expected'], 'Pin raw packet mismatch')
            for key, path in [('phase_source_sha256', source), ('raw_observation_sha256', artifact/'node/stdout'),
                ('build_stdout_sha256', artifact/'build/stdout'), ('build_stderr_sha256', artifact/'build/stderr'),
                ('emitted_namespace_sha256', artifact/'out/user.js')]:
                same(layer.get(key), sci.digest(path.read_bytes()), 'Pin emitted source/build/raw drift')
            build = (artifact/'build/stdout').read_text()
            if ('REFERENCE IDENTITY '+layer['reference_identity'] not in build or
                'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in build):
                raise ValueError('Pin/settings not independently built')
            references[kind] = layer
        result[row['obligation_id']] = {'status': STATUS, 'original_expectation_status': 'unresolved',
            'original_semantic_pass_credit': 0, 'source_binding': copy.deepcopy(row), 'actual_phase_frames': frames,
            'references': references, 'canonical_contract_status': 'passed', 'broad_defrecord_exclusion': False}
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--output', required=True, type=Path)
    p.add_argument('--timeout', type=int, default=240); args = p.parse_args(); run(args.output, args.timeout)
