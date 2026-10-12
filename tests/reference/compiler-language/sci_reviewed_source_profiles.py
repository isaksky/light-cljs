"""Fresh dispatcher for exactly four unchanged, reviewed SCI source profiles.

The report must call run(), then pass that invocation's sidecar to reconcile().
There is deliberately no stored-receipt loader or generic source fallback.
"""
from __future__ import annotations
import argparse
import copy
import json
import os
from pathlib import Path
import shutil
import sci_symbol_keyword_context as symbols
import sci_record_source_context as records

sci = symbols.sci
ROOT = sci.ROOT
PROFILE = 'reviewed-original-sci-source-profiles-v1'
SETTINGS = {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False}
KEYS = [v+'/'+e for v in symbols.VARIANTS for e in ('QuickJS', 'Node')]


def same(actual, expected, message):
    if not sci.report_module().same_json(actual, expected):
        raise ValueError(message)


def load():
    return [symbols.load(), *records.load()]


def partition(rows):
    """Remove these exact original IDs from the ordinary expression runner."""
    originals = {r['sci_id']: r for r in load()}
    ordinary, selected = [], []
    for row in rows:
        if row.get('sci_id') not in originals:
            ordinary.append(row)
            continue
        expected = originals[row['sci_id']]
        for key in ('id', 'obligation_id', 'source_sha256',
                    'original_source_sha256', 'original_row_sha256', 'body_sha256', 'expected'):
            same(row.get(key), expected[key], 'Original expression/source profile binding drift')
        # Ordinary registered SCI expression mappings bind original corpus
        # bytes by the immutable source hash and omit a duplicated source field.
        # Source-bearing mappings must additionally retain all literal bytes.
        if 'source' in row:
            same(row['source'], expected['source'], 'Reduced original source profile')
        selected.append(expected)
    if len({r['id'] for r in selected}) != len(selected):
        raise ValueError('Duplicate reviewed source identity')
    return ordinary, selected


def snapshot():
    return {**symbols.snapshot(), **records.snapshot(),
        **{str(p.relative_to(ROOT)): sci.digest(p.read_bytes()) for p in (
            Path(__file__), Path(__file__).with_name('test_sci_reviewed_source_profiles.py'),
            Path(__file__).with_name('sci-reviewed-source-profiles.md'))}}


def file_hash(path, expected, message):
    if not Path(path).is_file() or sci.digest(Path(path).read_bytes()) != expected:
        raise ValueError(message)


def read_document(path):
    return symbols.strict_json(Path(path).read_text())


def artifact_manifest(directory):
    """Retain emitted pin dependencies, namespace bytes and all raw producers."""
    directory = Path(directory)
    return {str(p.relative_to(directory)): sci.digest(p.read_bytes())
            for p in sorted(directory.rglob('*')) if p.is_file()}


def local_frames(module, document, directory):
    """Replay every raw packet and rebind the actual cargo build and producer."""
    before = module.snapshot()
    same(document['compiler_build_producer_source_sha256'], before, 'Local producer drift')
    raw_path = directory/'observations.tsv'
    raw = raw_path.read_text()
    file_hash(raw_path, document['raw_observations_sha256'], 'Local raw observations drift')
    source_rows = [symbols.load()] if module is symbols else records.load()
    same(document['source_binding'] if module is symbols else document['source_bindings'],
         source_rows[0] if module is symbols else source_rows, 'Local complete source binding drift')
    parsed = module.parse_raw(raw, source_rows[0] if module is symbols else source_rows)
    build = (directory/'build/stdout').read_text()
    binaries = [Path(r['executable']) for line in build.splitlines() if line.startswith('{')
        for r in [symbols.strict_json(line)] if r.get('reason') == 'compiler-artifact'
        and r.get('target', {}).get('name') == module.DRIVER.stem and r.get('executable')]
    if len(binaries) != 1:
        raise ValueError('Missing exact audited executable')
    binary_hash = sci.digest(binaries[0].read_bytes())
    node = Path(shutil.which(os.environ.get('NODE', 'node')) or '').resolve()
    node_hash = sci.digest(node.read_bytes())
    audit_stdout = (directory/'audit/stdout').read_text()
    if ('running 1 test' not in audit_stdout or
        'test result: ok. 1 passed; 0 failed; 0 ignored;' not in audit_stdout):
        raise ValueError('Actual audit did not execute')
    same(read_document(directory/'audit/command.json'),
        [str(binaries[0]), module.TEST, '--ignored', '--exact', '--nocapture'],
        'Different actual audit invocation')
    same(read_document(directory/'build/command.json'),
        ['cargo', 'test', '--locked', '--test', module.DRIVER.stem,
         '--no-run', '--message-format=json'], 'Different actual build invocation')
    frames = {r['id']: {} for r in source_rows}
    for values in parsed:
        if module is symbols:
            variant, engine, source, javascript, raw_observation, warmup, observed = values
            identity = symbols.ID
        else:
            identity, variant, engine, source, javascript, raw_observation, warmup, observed = values
        frame = {'status': 'passed', 'observed': observed,
            'phase_source_sha256': sci.digest(source), 'compiled_js_sha256': sci.digest(javascript),
            'raw_observation_sha256': sci.digest(raw_observation),
            'warmup_compiled_js_sha256': None if warmup == '-' else sci.digest(bytes.fromhex(warmup)),
            'compiler_build_producer_source_sha256': before, 'audit_executable_sha256': binary_hash,
            'node_executable_sha256': node_hash, 'build_stdout_sha256': sci.digest(build)}
        if module is symbols:
            frame.update(original_source_sha256=symbols.SOURCE_HASH,
                         original_row_sha256=symbols.ROW_HASH, body_sha256=symbols.BODY_HASH)
        frames[identity][variant+'/'+engine] = frame
    same(document['frames'], frames[symbols.ID] if module is symbols else frames,
         'Retained phase frames detached from raw execution/build')
    if document.get('status') != 'passed' or document.get('catalog_credit') != 0:
        raise ValueError('Local execution failed or invented catalog credit')
    return frames


def reference(row, layer, kind, directory, is_record):
    runner = sci.report_module()
    if layer.get('status') != 'passed':
        raise ValueError('Exact original pin source did not pass')
    same(layer.get('observed'), row['expected'], 'Pin observation/expectation drift')
    same(layer.get('settings'), SETTINGS, 'Pin options drift')
    same(layer.get('reference_identity'), kind+' '+(runner.RELEASE if kind == 'release' else runner.REVISION), 'Pin identity drift')
    same(layer.get('source_sha256'), row['source_sha256'], 'Pin original source drift')
    build_root = directory/kind/'profile'/row['id'] if is_record else directory/kind
    raw_path = build_root/'node/stdout' if is_record else build_root/row['id']/'stdout'
    file_hash(raw_path, layer['raw_observation_sha256'], 'Pin raw observation drift')
    same(symbols.strict_json(raw_path.read_text()), row['expected'], 'Raw pin observation mismatch')
    file_hash(build_root/'build/stdout', layer['build_stdout_sha256'], 'Pin build provenance drift')
    build = (build_root/'build/stdout').read_text()
    if ('REFERENCE IDENTITY '+layer['reference_identity'] not in build or
        'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in build):
        raise ValueError('Pin identity/settings not established by actual build')
    file_hash(build_root/'reference.js', layer['reference_javascript_sha256'], 'Pin emitted observer drift')
    if is_record:
        same(layer.get('namespace_profile'), row['source_context_profile'], 'Pin header profile drift')
        source_path = directory/'reference-source'/kind/'profile'/row['id']/'user.cljs'
        same(source_path.read_text(), records.module(row, 'application'), 'Pin complete module drift')
        file_hash(source_path, layer['phase_source_sha256'], 'Pin complete module hash drift')
        file_hash(build_root/'build/stderr', layer['build_stderr_sha256'], 'Pin warning provenance drift')
    else:
        for key, path in [('oracle_source_sha256', ROOT/symbols.context.ORACLE),
                          ('oracle_helper_source_sha256', ROOT/'tests/reference/execution.clj'),
                          ('observation_source_sha256', ROOT/'tests/support/execution.js')]:
            file_hash(path, layer[key], 'Symbol source oracle drift')
        same(layer.get('reference_phase_effects'), ['[]'], 'Pin phase effects drift')
    return layer


def collision(row, layer, kind, directory):
    records.validate_control(layer, kind)
    root = directory/kind/'default'/row['id']
    source = directory/'reference-source'/kind/'default'/row['id']/'user.cljs'
    same(source.read_text(), records.module(row, 'application', True), 'Original default collision source drift')
    file_hash(source, layer['phase_source_sha256'], 'Default source hash drift')
    same(layer['namespace_profile'], {'schema_version': 1,
        'profile': 'original-default-header-collision-control', 'namespace': 'user',
        'excluded_core_identifiers': []}, 'Default source silently excluded a core name')
    file_hash(root/'out/user.js', layer['default_compiled_namespace_sha256'], 'Default mixed namespace drift')
    name = row['source_context_profile']['excluded_core_identifiers'][0]
    compiled = (root/'out/user.js').read_text()
    if 'cljs.core.'+name+' =' not in compiled or 'user.'+name+'.prototype' not in compiled:
        raise ValueError('Original constructor/prototype collision missing')
    raw = root/'direct-loader-control/stdout'
    file_hash(raw, layer['direct_raw_observation_sha256'], 'Default raw TypeError drift')
    packet = symbols.strict_json(raw.read_text()); error = packet.pop('original_error', None)
    same(error, layer['original_error'], 'Original collision error drift')
    same(packet, layer['observed'], 'Default error packet drift')
    if (not error or error.get('name') != 'TypeError' or 'prototype' not in error.get('message', '')
            or str(root/'out/user.js') not in error.get('stack', '')):
        raise ValueError('Default loader rejection is not the original collision')
    for path, key in [('build/stdout', 'build_stdout_sha256'), ('build/stderr', 'build_stderr_sha256'),
                      ('node/stdout', 'raw_observation_sha256'), ('reference.js', 'reference_javascript_sha256')]:
        file_hash(root/path, layer[key], 'Default build/observer evidence drift')
    build = (root/'build/stdout').read_text()
    if ('REFERENCE IDENTITY '+layer['reference_identity'] not in build or
        'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in build):
        raise ValueError('Default control pin/options not built')


def reconcile(document):
    """Revalidate this freshly supplied complete sidecar against raw artifacts."""
    same(document.get('schema_version'), 1, 'Reviewed source schema drift')
    same(document.get('profile'), PROFILE, 'Reviewed source dispatcher profile drift')
    same(document.get('source_bindings'), load(), 'Original four source bindings drift')
    same(document.get('compiler_build_producer_source_sha256'), snapshot(), 'Current producer drift')
    same(list(document.get('cohorts', {})), ['symbols', 'records'], 'Partial/reordered source cohorts')
    result = {}
    for name, module in [('symbols', symbols), ('records', records)]:
        cohort = document['cohorts'][name]; directory = Path(cohort['artifact_root'])
        same(cohort.get('artifact_sha256'), artifact_manifest(directory),
             'Cohort emitted source/dependency/raw artifact tree drift')
        evidence = read_document(directory/'evidence.json')
        same(cohort['evidence'], evidence, 'Detached full raw cohort receipt')
        same(evidence['compiler_build_producer_source_sha256'], module.snapshot(), 'Cohort producer drift')
        rows = [symbols.load()] if module is symbols else records.load()
        same(evidence['source_binding'] if module is symbols else evidence['source_bindings'],
             rows[0] if module is symbols else rows, 'Complete original cohort drift')
        frames = local_frames(module, evidence['local'], directory/'local')
        same(list(evidence.get('references', {})), ['release', 'checkout'], 'Missing/reordered pins')
        if module is records:
            same(list(evidence.get('default_header_controls', {})), ['release', 'checkout'], 'Missing default pin controls')
        for row in rows:
            layers = {'javascript': {'status': 'passed', 'source_sha256': row['source_sha256'],
                'engines': ['QuickJS', 'Node'], 'frames': frames[row['id']]}}
            for kind in ('release', 'checkout'):
                if module is records:
                    same(list(evidence['references'][kind]), [r['id'] for r in rows], 'Partial/reordered original record pins')
                    same(list(evidence['default_header_controls'][kind]), [r['id'] for r in rows], 'Partial/reordered collision controls')
                    collision(row, evidence['default_header_controls'][kind][row['id']], kind, directory)
                layer = evidence['references'][kind][row['id']] if module is records else evidence['references'][kind]
                layers[kind] = reference(row, layer, kind, directory, module is records)
            result[row['obligation_id']] = {'status': 'covered', 'reconciliation': 'reviewed-original-source-profile-agreement',
                'id': row['id'], 'source_binding': copy.deepcopy(row), 'layers': layers,
                'actual_phase_frames': frames[row['id']], 'original_expectation_status': 'passed',
                'source_context_profile': row['source_context_profile'],
                'default_header_status': 'rejected' if module is records else 'original-user-namespace',
                'default_header_controls': {k: evidence['default_header_controls'][k][row['id']] for k in ('release', 'checkout')} if module is records else {}}
    same(list(result), [r['obligation_id'] for r in load()], 'Invented/missing source obligation credit')
    if document.get('status') != 'passed':
        raise ValueError('Incomplete dispatcher run')
    return result


def run(directory, timeout=240):
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=False)
    before = snapshot()
    document = {'schema_version': 1, 'profile': PROFILE, 'source_bindings': load(),
        'cohorts': {}, 'compiler_build_producer_source_sha256': before}
    for name, module in [('symbols', symbols), ('records', records)]:
        root = directory/name
        evidence = module.run(root, timeout)
        document['cohorts'][name] = {'artifact_root': str(root), 'evidence': evidence,
                                    'artifact_sha256': artifact_manifest(root)}
    same(snapshot(), before, 'Dispatcher source/build drift during execution')
    document['status'] = 'passed'
    reconcile(document)
    (directory/'evidence.json').write_text(json.dumps(document, indent=2)+'\n')
    return document


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--timeout', type=int, default=240)
    args = parser.parse_args()
    run(args.output, args.timeout)
