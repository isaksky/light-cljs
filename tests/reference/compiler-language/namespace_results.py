"""Source-bound complete namespace results; exported callbacks stay separate."""
import hashlib
import json
import os
from pathlib import Path
import re
import report

IDS = {'syntax-context-namespace-header-context', 'syntax-context-namespace-nsstar-alias'}
DRIVER = 'tests/compiler_syntax_context.rs'
DRIVER_HASH = 'bd572ecd2ac504175707824acb5d20f9aab8477d75a155602ae619f2185fca99'
ENGINE_DRIVER = 'tests/compiler_language_namespace.rs'


def digest(value):
    return hashlib.sha256(value if isinstance(value, bytes) else value.encode()).hexdigest()


def validate(case):
    if case.get('id') not in IDS:
        return False
    import syntax_context
    original = next(row for row in syntax_context.cases() if row['id'] == case['id'])
    if not report.same_json(case, original):
        raise ValueError('complete namespace source/result contract drift')
    source = (report.ROOT / DRIVER).read_bytes()
    name = syntax_context.TESTS[case['id'].removeprefix(syntax_context.PREFIX)]
    if digest(source) != DRIVER_HASH or not re.search(r'#\[test\]\s*fn\s+' + name + r'\s*\(', source.decode()):
        raise ValueError('complete namespace actual macro driver drift')
    return name


def local(case, binary, directory, timeout):
    """Compare the actual final pr-str string, never call it as an export."""
    name = validate(case)
    if not name:
        raise ValueError('unreviewed scalar namespace result')
    directory.mkdir(parents=True, exist_ok=True)
    before = snapshot(case)
    layers = {'native': report.result('historical-not-executed',
        'legacy fixture observation; current evaluation is independent', observed=case['native_stdout'])}
    for layer, flags in [('lexer', []), ('parser', ['--parse'])]:
        run = report.invoke([binary, *flags, '-'], directory / layer, source=case['source'], timeout=timeout)
        layers[layer] = report.result('accepted') if run['status'] == 'completed' else run
    root = report.ROOT / case['source_root']
    compiled = report.invoke([binary, '--compile', '--entry', case['entry'], '--source-root', root], directory / 'compile', timeout=timeout)
    if compiled['status'] != 'completed':
        layers['javascript'] = compiled
        return layers
    runtime = (report.ROOT / 'src/js_runtime.js').read_text().strip()
    if compiled['stdout'].count(runtime) != 1:
        layers['javascript'] = report.result('invalid-output', 'namespace runtime observation anchor drift')
        return layers
    # Only substitute the test's lexical runtime. Every emitted source form,
    # including the final original pr-str, remains in its original order.
    javascript = compiled['stdout'].replace(runtime, '__namespaceObservationRuntime', 1)
    observer = (report.ROOT / 'tests/support/sci_execution_observation.js').read_text()
    script = f"(() => {{ const __namespaceObservationRuntime = {runtime}; return ({observer})(__namespaceObservationRuntime)({json.dumps(case['id'])}, observe => {javascript}); }})()"
    script_path = directory / 'complete-program.js'
    script_path.write_text(script)
    raw_path = directory / 'quickjs.json'
    command = ['env', 'LIGHT_CLJS_NAMESPACE_SCRIPT=' + str(script_path), 'LIGHT_CLJS_NAMESPACE_OUTPUT=' + str(raw_path),
        'cargo', 'test', '--locked', '--test', 'compiler_language_namespace', '--', '--ignored', '--exact',
        'complete_namespace_observation_executes_in_quickjs', '--nocapture']
    quickjs = report.invoke(command, directory / 'quickjs', timeout=max(180, timeout))
    node = report.invoke([os.environ.get('NODE', 'node'), '-'], directory / 'node', source='console.log(' + script + ');', timeout=timeout)
    scalar = {**case, 'expected': {**case['expected'], 'outcome': {'status': 'returned', 'value': 'true'}}}
    engines = {}
    for engine, run in [('Node', node), ('QuickJS', quickjs)]:
        if run['status'] != 'completed':
            engines[engine] = run
        elif engine == 'QuickJS' and (not exact_test(run['stdout'], 'complete_namespace_observation_executes_in_quickjs') or not raw_path.is_file()):
            engines[engine] = report.result('invalid-output', 'missing exact QuickJS execution/output')
        else:
            output = raw_path.read_text() if engine == 'QuickJS' else run['stdout']
            engines[engine] = report.observations(output, [scalar])[case['id']]
    layers['evaluation'] = {**engines['QuickJS'], 'api': 'complete emitted namespace in QuickJS',
        'source_sha256': digest(case['source'])}
    driver = report.invoke(['cargo', 'test', '--locked', '--test', 'compiler_syntax_context', '--', '--exact', name, '--nocapture'],
        directory / 'actual-macro-driver', timeout=max(180, timeout))
    if driver['status'] == 'completed':
        driver = report.result('passed' if exact_test(driver['stdout'], name) else 'invalid-output',
            test_file=DRIVER, test_name=name, test_source_sha256=DRIVER_HASH)
    unchanged = before == snapshot(case)
    layers['javascript'] = report.result('passed' if unchanged and driver['status'] == 'passed' and all(v['status'] == 'passed' for v in engines.values()) else 'failed',
        'Complete source returns its serialized scalar; pinned references separately observe the original export contract.',
        engines=engines, actual_macro_driver=driver, complete_result_expected='true', original_expected=case['expected'],
        source_sha256=digest(case['source']), compiled_js_sha256=digest(compiled['stdout']), producer_source_sha256=before)
    if not unchanged:
        layers['javascript'] = report.result('stale-build-input', 'namespace source/result/driver drift during execution')
    return layers


def exact_test(output, name):
    return ('running 1 test\n' in output and 'test ' + name + ' ... ok' in output
        and 'test result: ok. 1 passed; 0 failed; 0 ignored;' in output)


def verified_local(case, layer):
    name = validate(case)
    scalar = {**case['expected'], 'outcome': {'status': 'returned', 'value': 'true'}}
    driver = layer.get('actual_macro_driver', {})
    engines = layer.get('engines', {})
    return (bool(name) and layer.get('status') == 'passed'
        and layer.get('source_sha256') == digest(case['source'])
        and layer.get('producer_source_sha256') == snapshot(case)
        and bool(re.fullmatch(r'[0-9a-f]{64}', layer.get('compiled_js_sha256', '')))
        and layer.get('complete_result_expected') == 'true'
        and report.same_json(layer.get('original_expected'), case['expected'])
        and driver == {'status': 'passed', 'test_file': DRIVER, 'test_name': name, 'test_source_sha256': DRIVER_HASH}
        and set(engines) == {'Node', 'QuickJS'}
        and all(v.get('status') == 'passed' and report.same_json(v.get('observed'), scalar) for v in engines.values()))


def snapshot(case):
    validate(case)
    paths = [report.ROOT / DRIVER, report.ROOT / ENGINE_DRIVER, Path(__file__), Path(report.__file__),
        report.ROOT / 'src/js_runtime.js', report.ROOT / 'tests/support/sci_execution_observation.js']
    paths.extend(p for p in (report.ROOT / 'src').rglob('*') if p.is_file())
    paths.extend(report.ROOT / p for p in ('Cargo.toml', 'Cargo.lock'))
    for key in ('source_root', 'reference_source_root'):
        if key in case:
            paths.extend(p for p in (report.ROOT / case[key]).rglob('*') if p.is_file())
    return {str(p.relative_to(report.ROOT)): digest(p.read_bytes()) for p in sorted(set(paths))}
