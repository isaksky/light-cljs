#!/usr/bin/env python3
"""Exact original cljs/tag identity body via configured compiler embedding.

Only this reviewed pure profile is accepted. Generic CLI UnknownTag remains a
separate capability boundary. No raw configured-reader result grants API/program
credit; prospective assertion credit is the sole retained original assertion.
"""
from __future__ import annotations
import argparse, hashlib, json, os, shutil, subprocess
from pathlib import Path
import report

ROOT = Path(__file__).resolve().parents[3]
PROPOSALS = ROOT / 'tests/reference/compiler-residual-source-context-proposals.json'
DRIVER = ROOT / 'tests/compiler_configured_source_context.rs'
OBSERVER = ROOT / 'tests/support/sci_execution_observation.js'
NODE_OBSERVER = ROOT / 'tests/support/execution.js'
TEST = 'configured_original_body_executes_in_fresh_and_reused_hosts'
ID = 'cljs-residual-source-context-tagged-source'
VARIANTS = ('application-fresh', 'macro-fresh', 'application-reused', 'macro-reused')
SOURCE_HASH = 'c930ca1c14e49238919acb92cbac85c0eae2c8f3674df99d6d277b48e95a777e'
CONFIG_HASH = '344170f2e2b2b00dd8e21d4450f33b951eb48c9da62ef4857e175282a1f7f9d9'
REF_CONFIG_HASH = '2f353c072cbe6cdc4683e4447e4e10472cdc7f8ffafe1116c06856b589346b57'
MODULE_HASH = '8776d67a34a9a492b9c72975c6203bd863b2330a23ec73cd7edee4ded03d7cf8'
BODY_HASH = 'e44f87cf162a70c7deba9146fac8bf6db796ee6be1770dc54f6d955ea495fed3'
ASSERTION_HASH = 'b3736cb88cceca8fc292d000bc3387d263755608c7d5ea923ed77bfe6ef5a44a'
BODY = '(deftest test-identity-custom-literal\n  (is (= #cljs/tag [1 2 3] [1 2 3])))'
EXPECTED = {'id': ID, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}

def digest(value):
    return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()

def tree(root):
    return {str(p.relative_to(root)): digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file()}

def validate(case):
    if (case.get('id') != ID or case.get('entry') != 'cljs.tagged-literals-test' or case.get('export') != 'run'
            or digest(case.get('source', '')) != SOURCE_HASH
            or case.get('compiler_data_readers') != [{'tag': 'cljs/tag', 'operation': 'identity'}]
            or not report.same_json(case.get('expected'), EXPECTED)):
        raise ValueError('unreviewed configured source/profile/expectation')
    config = case.get('original_data_reader_configuration', {})
    if config.get('source') != 'src/test/cljs/data_readers.cljc' or config.get('sha256') != CONFIG_HASH or digest(config.get('text', '')) != CONFIG_HASH:
        raise ValueError('missing or changed original reader configuration')
    upstream = ROOT / 'research/clojurescript'
    if digest((upstream / config['source']).read_bytes()) != CONFIG_HASH:
        raise ValueError('original reader configuration drift')
    if (case.get('module_source') != 'src/test/cljs/cljs/tagged_literals_test.cljs'
            or case.get('module_sha256') != MODULE_HASH
            or digest((upstream / case['module_source']).read_bytes()) != MODULE_HASH
            or BODY not in (upstream / case['module_source']).read_text()
            or digest(BODY) != BODY_HASH
            or case.get('complete_original_provenance') != [{'body_id': case['module_source'] + '::test-identity-custom-literal', 'source_sha256': BODY_HASH}]):
        raise ValueError('original complete body provenance drift')
    expected_local = {'cljs/tagged_literals_test.cljs': SOURCE_HASH}
    expected_reference = {**expected_local, 'data_readers.cljc': REF_CONFIG_HASH}
    if case.get('source_root') != 'tests/fixtures/compiler-residual-source-context/tagged-source' or tree(ROOT / case['source_root']) != expected_local:
        raise ValueError('local source tree drift')
    if case.get('reference_source_root') != 'tests/fixtures/compiler-residual-source-context-reference/tagged-source' or tree(ROOT / case['reference_source_root']) != expected_reference or case.get('reference_files_sha256') != expected_reference:
        raise ValueError('reference source/configuration tree drift')
    witnesses = case.get('proposed_original_assertion_witnesses', [])
    if (len(witnesses) != 1 or witnesses[0].get('assertion_id') != case['module_source'] + '::test-identity-custom-literal::assertion-001'
            or witnesses[0].get('assertion_sha256') != ASSERTION_HASH or witnesses[0].get('fixture_id') != ID):
        raise ValueError('missing/changed sole original assertion provenance')
    return case

def load():
    rows = json.loads(PROPOSALS.read_text())['embedding-source-cases']
    matches = [row for row in rows if row['id'] == ID]
    if len(matches) != 1: raise ValueError('missing/duplicate reviewed profile')
    return validate(matches[0])

def snapshot(case):
    validate(case)
    paths = sorted(p for p in (ROOT / 'src').rglob('*') if p.is_file())
    paths += [ROOT / n for n in ('Cargo.toml', 'Cargo.lock', 'build.rs') if (ROOT / n).is_file()]
    paths += [DRIVER, Path(__file__).resolve(), Path(__file__).with_name('test_configured_source_context.py'), OBSERVER, NODE_OBSERVER, Path(report.__file__).resolve(), ROOT / 'tests/reference/execution.clj']
    paths += [p for key in ('source_root', 'reference_source_root') for p in (ROOT / case[key]).rglob('*') if p.is_file()]
    paths += [ROOT / 'research/clojurescript' / case['module_source'], ROOT / 'research/clojurescript' / case['original_data_reader_configuration']['source']]
    return {str(p.relative_to(ROOT)): digest(p.read_bytes()) for p in sorted(set(paths))}

def unchanged(before, after):
    if before != after: raise ValueError('source/compiler/build/producer drift during audit')

def macro_source(source):
    prefix, body = source.split('\n(defn run [] ', 1)
    return prefix + '\n(defmacro upstream [] ' + body + '\n(defn run [] (upstream))\n'

def parse_raw(text, case):
    rows = [line.split('\t') for line in text.splitlines()]
    if len(rows) != 4 or any(len(row) != 5 for row in rows) or [row[0] for row in rows] != list(VARIANTS):
        raise ValueError('missing/duplicate/partial/reordered raw embedding rows')
    parsed = []
    for variant, source_hex, js_hex, observed_hex, warmup_hex in rows:
        source, js, observed = [bytes.fromhex(value).decode() for value in (source_hex, js_hex, observed_hex)]
        if source != (macro_source(case['source']) if variant.startswith('macro') else case['source']):
            raise ValueError('changed complete phase source')
        if js.count(' = // Original bounded runtime') != 1:
            raise ValueError('runtime observation anchor drift')
        if (warmup_hex == '-') != variant.endswith('fresh') or (variant.endswith('reused') and not bytes.fromhex(warmup_hex)):
            raise ValueError('missing actual prior reused-host macro compilation')
        layer = report.observations(observed, [case])[case['id']]
        if layer['status'] != 'passed': raise ValueError('invalid/mismatching QuickJS raw frame: ' + str(layer))
        parsed.append((variant, source, js, observed, warmup_hex, layer))
    return parsed

def run(command, directory, timeout, *, environment=None, source=None):
    directory.mkdir(parents=True, exist_ok=True)
    completed = subprocess.run(command, cwd=ROOT, env=environment, input=source, text=True, capture_output=True, timeout=timeout)
    (directory / 'stdout').write_text(completed.stdout); (directory / 'stderr').write_text(completed.stderr)
    (directory / 'command.json').write_text(json.dumps(command) + '\n')
    if completed.returncode: raise ValueError('audit command failed: ' + str(directory))
    return completed.stdout, completed.stderr

def local_cases(case, directory, timeout=240):
    case = validate(case); directory = Path(directory).resolve()
    if directory.exists(): raise ValueError('fresh artifact directory required')
    before = snapshot(case)
    directory.mkdir(parents=True)
    request = directory / 'input.tsv'; output = directory / 'observations.tsv'
    request.write_text(ID + '\t' + case['entry'] + '\t' + case['source'].encode().hex() + '\tcljs/tag\tidentity\n')
    build, _ = run(['cargo', 'test', '--locked', '--test', 'compiler_configured_source_context', '--no-run', '--message-format=json'], directory / 'build', timeout)
    artifacts = [json.loads(line) for line in build.splitlines() if line.startswith('{')]
    binaries = [Path(row['executable']) for row in artifacts if row.get('reason') == 'compiler-artifact' and row.get('target', {}).get('name') == 'compiler_configured_source_context' and row.get('executable')]
    if len(binaries) != 1: raise ValueError('missing/duplicate exact audit executable')
    binary = binaries[0]; binary_hash = digest(binary.read_bytes())
    node = Path(shutil.which(os.environ.get('NODE', 'node')) or '').resolve()
    if not node.is_file(): raise ValueError('Node unavailable')
    node_hash = digest(node.read_bytes())
    env = {**os.environ, 'LIGHT_CLJS_CONFIGURED_SOURCE_INPUT': str(request), 'LIGHT_CLJS_CONFIGURED_SOURCE_OUTPUT': str(output), 'LIGHT_CLJS_CONFIGURED_SOURCE_ROOT': str(directory / 'phase-sources')}
    stdout, _ = run([str(binary), TEST, '--ignored', '--exact', '--nocapture'], directory / 'audit', timeout, environment=env)
    if 'running 1 test' not in stdout or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in stdout:
        raise ValueError('audit test was not actually executed')
    parsed = parse_raw(output.read_text(), case)
    layers = {}
    for variant, source, js, raw_quickjs, warmup, quickjs in parsed:
        target = directory / variant; target.mkdir()
        (target / 'source.cljs').write_text(source); (target / 'compiled.js').write_text(js); (target / 'quickjs.json').write_text(raw_quickjs + '\n')
        captured = js.replace(' = // Original bounded runtime', ' = globalThis.lightRuntime = // Original bounded runtime', 1)
        script = ('globalThis.lightInspectValue = value => globalThis.lightRuntime?.inspect(value) ?? null;\n'
            + 'globalThis.lightCases = [observe => { const run = ' + captured + '; return run(observe); }];\n'
            + 'globalThis.lightCaseMetadata = ' + json.dumps([[ID, case['source']]]) + ';\n' + NODE_OBSERVER.read_text())
        (target / 'node-script.js').write_text(script)
        raw_node, stderr = run([str(node), '-'], target / 'node', timeout, source=script)
        node_layer = report.observations(raw_node, [case])[ID]
        frames = stderr.splitlines()
        if len(frames) != 2 or not frames[0].startswith('CASE START ') or not frames[1].startswith('CASE END ') or json.loads(frames[0][11:]).get('id') != ID or json.loads(frames[1][9:]).get('id') != ID:
            raise ValueError('invalid Node identity/stderr frames')
        if node_layer['status'] != 'passed': raise ValueError('invalid/mismatching Node raw frame')
        provenance = {'phase_source_sha256': digest(source), 'compiled_js_sha256': digest(js), 'source_sha256': SOURCE_HASH,
            'reader_configuration_sha256': CONFIG_HASH, 'reference_configuration_sha256': REF_CONFIG_HASH,
            'compiler_build_producer_source_sha256': before, 'audit_executable_sha256': binary_hash,
            'build_stdout_sha256': digest(build), 'node_executable_sha256': node_hash,
            'warmup_compiled_js_sha256': None if warmup == '-' else digest(bytes.fromhex(warmup))}
        for engine, layer, raw in [('QuickJS', quickjs, raw_quickjs), ('Node', node_layer, raw_node)]:
            layers[variant + '/' + engine] = {**layer, **provenance, 'raw_observation_sha256': digest(raw)}
    unchanged(before, snapshot(case))
    if digest(binary.read_bytes()) != binary_hash or digest(node.read_bytes()) != node_hash: raise ValueError('execution binary drift')
    if list(layers) != [v + '/' + e for v in VARIANTS for e in ('QuickJS', 'Node')]: raise ValueError('incomplete eight observations')
    result = {'status': 'passed', 'observations': layers, 'capability': 'configured compiler embedding', 'generic_cli': 'not claimed', 'prospective_original_assertion_credit': 1}
    (directory / 'evidence.json').write_text(json.dumps(result, indent=2) + '\n')
    return result

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path); parser.add_argument('--timeout', type=int, default=240); parser.add_argument('--references', action='store_true')
    args = parser.parse_args(); case = load(); before = snapshot(case)
    result = {'local': local_cases(case, args.output / 'local', args.timeout)}
    if args.references:
        for kind in ('release', 'checkout'):
            result[kind] = report.reference_cases([case], kind, ROOT / 'research/clojurescript', (args.output / kind).resolve(), args.timeout)[ID]
            if result[kind]['status'] != 'passed': raise ValueError('pinned reference did not pass: ' + kind)
            artifacts = (args.output / kind / ID).resolve()
            result[kind].update(source_sha256=SOURCE_HASH, reader_configuration_sha256=CONFIG_HASH,
                reference_configuration_sha256=REF_CONFIG_HASH, compiler_build_producer_source_sha256=before,
                raw_observation_sha256=digest((artifacts / 'node/stdout').read_bytes()),
                build_stdout_sha256=digest((artifacts / 'build/stdout').read_bytes()),
                reference_javascript_sha256=digest((artifacts / 'reference.js').read_bytes()))
            print(kind, result[kind]['status'], flush=True)
    unchanged(before, snapshot(case))
    (args.output / 'evidence.json').write_text(json.dumps(result, indent=2) + '\n')
    print('eight local observations passed', flush=True)
if __name__ == '__main__': main()
