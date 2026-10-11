#!/usr/bin/env python3
"""Bounded complete SCI string/set/walk/EDN import cohort, with untouched originals."""
from __future__ import annotations
import argparse, collections, json, os
from pathlib import Path
import sci_execution as sci
from extract_inventory import Reader, head

HERE = Path(__file__).resolve().parent
CATALOG = HERE / 'sci-library-context-catalog.jsonl'
DRIVER = 'tests/compiler_sci_library_context.rs'
TEST = 'retained_library_sources_execute_in_node_and_quickjs'
ORACLE = 'tests/reference/compiler-language/sci_library_context.clj'
INSPECTOR = 'tests/reference/compiler-language/sci_library_inspection.cljs'
LIBRARIES = ('clojure.string', 'clojure.set', 'clojure.walk', 'clojure.edn')
IDS = ('sci-string-join-adapted', 'sci-string-join-lazy', 'sci-string-replace-literal', 'sci-string-escape-function',
       'sci-data-set-require', 'sci-data-set-join-1', 'sci-data-set-index-0', 'sci-data-set-select-1',
       'sci-data-walk-require', 'sci-data-walk-prewalk-order', 'sci-data-walk-postwalk-order', 'sci-data-walk-postwalk-transform',
       'sci-data-edn-require', 'sci-data-edn-reader-precedence', 'sci-data-edn-tag-nested', 'sci-data-edn-tag-discard')


def split_declarations(source):
    """Keep leading require forms in compiler declaration context; retain all bytes."""
    forms = Reader(source).all()
    leading = []
    for node in forms:
        if head(node) != 'require':
            break
        leading.append(node)
    end = leading[-1].end if leading else 0
    return source[:end], source[end:]


def local_libraries():
    return {library: sci.digest((sci.ROOT / ('src/compiler_libraries/' + library.replace('.', '/') + '.cljs')).read_bytes())
            for library in LIBRARIES}


def generated_catalog():
    corpus, dispositions = sci.rows(sci.CORPUS), sci.rows(sci.DISPOSITIONS)
    rows = []
    for identity in IDS:
        raw, original = corpus[identity]
        disposition_raw, disposition = dispositions[identity]
        if original != disposition[:7] or original[1] != 'portable':
            raise ValueError('Original SCI library fixture changed: ' + identity)
        adapted, edits = sci.catch_adapter(original[2])
        declarations, runtime = split_declarations(adapted)
        rows.append({'id': 'sci-execution-' + identity, 'sci_id': identity,
            'obligation_id': 'local::tests/reference/sci/evaluator-dispositions.tsv::' + identity,
            'source': original[2], 'source_sha256': sci.digest(original[2]),
            'original_source_sha256': sci.digest(original[2]), 'original_row_sha256': sci.digest(raw),
            'body_sha256': sci.digest(disposition_raw), 'expected': sci.expected(original),
            'adapted_source': adapted, 'adapted_source_sha256': sci.digest(adapted), 'source_edits': edits,
            'declaration_source': declarations, 'runtime_source': runtime,
            'namespace': 'user', 'reference_namespace': 'sci.library.case' + str(len(rows)),
            'libraries': list(LIBRARIES), 'fixture_profile': original[1],
            'comparison': sci.PROJECTION,
            'setup': 'Portable SCI fixture libraries are explicitly preinstalled. Leading original require declarations stay compiler-top-level; the complete remaining original body executes in a lexical observe callback with captured printing. Local namespace user; reference per-case namespace is explicit and no returned identity is renamed.'})
    return rows


def load():
    rows = [json.loads(line) for line in CATALOG.read_text().splitlines() if line]
    if json.dumps(rows, sort_keys=True) != json.dumps(generated_catalog(), sort_keys=True):
        raise ValueError('SCI library-context catalog stale or incomplete')
    return rows


def planning():
    selected = set(IDS)
    result = []
    for identity, (raw, fields) in sci.rows(sci.CORPUS).items():
        libraries = [library for library in LIBRARIES if library in fields[2]]
        if not libraries or fields[1] != 'portable':
            continue
        try:
            expected = sci.expected(fields)
        except ValueError:
            continue
        result.append({'sci_id': identity, 'source': fields[2], 'source_sha256': sci.digest(fields[2]),
            'original_row_sha256': sci.digest(raw), 'expected': expected, 'libraries': libraries,
            'status': 'selected-for-execution' if identity in selected else 'not-run',
            'reason': 'Bounded independent import cohort; retained original bodies outside it need fresh execution, never source-based passing credit.'})
    return result


def local_cases(rows, directory, timeout=240):
    directory.mkdir(parents=True, exist_ok=True)
    input_path, output = directory / 'input.tsv', directory / 'observations.tsv'
    input_path.write_text(''.join(row['id'] + '\t' + row['declaration_source'].encode().hex() + '\t' + row['runtime_source'].encode().hex() + '\n' for row in rows))
    command = ['cargo', 'test', '--locked', '--test', 'compiler_sci_library_context', TEST, '--', '--ignored', '--exact', '--nocapture']
    environment = {**os.environ, 'LIGHT_CLJS_SCI_LIBRARY_INPUT': str(input_path.resolve()), 'LIGHT_CLJS_SCI_LIBRARY_OUTPUT': str(output.resolve())}
    # invoke owns output files/process-group timeouts; environment is explicit
    # rather than mutating the process-wide Python environment.
    command = ['env', *[key + '=' + environment[key] for key in ('LIGHT_CLJS_SCI_LIBRARY_INPUT', 'LIGHT_CLJS_SCI_LIBRARY_OUTPUT')], *command]
    runner = sci.report_module()
    ran = runner.invoke(command, directory / 'cargo', timeout=timeout)
    if ran['status'] != 'completed' or 'running 1 test' not in ran.get('stdout', '') or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in ran.get('stdout', '') or not output.is_file():
        return {row['id']: {'status': ran['status'] if ran['status'] != 'completed' else 'failed', 'detail': ran} for row in rows}
    records = [line.split('\t') for line in output.read_text().splitlines()]
    if [record[0] for record in records] != [row['id'] for row in rows]:
        raise ValueError('Missing/extra/reordered local library observations')
    results = {}
    for row, record in zip(rows, records):
        provenance = {'source_sha256': row['source_sha256'], 'adapted_source_sha256': row['adapted_source_sha256'],
            'test_file': DRIVER, 'test_name': TEST, 'test_source_sha256': sci.digest((sci.ROOT / DRIVER).read_bytes()),
            'adapter_source_sha256': sci.digest((sci.ROOT / 'tests/support/sci_execution_observation.js').read_bytes()),
            'inspection_source_sha256': sci.digest((sci.ROOT / INSPECTOR).read_bytes()), 'library_source_sha256': local_libraries()}
        if len(record) != 4 or record[1] != 'executed':
            results[row['id']] = {'status': 'rejected', 'detail': record[2:], **provenance}
            continue
        engines = {}
        for engine, raw in zip(('Node', 'QuickJS'), record[2:]):
            try:
                observed = json.loads(raw)
                engines[engine] = sci.verify(row, {'status': 'completed', 'observed': observed}, runner.same_json)
            except ValueError:
                engines[engine] = {'status': 'invalid-output', 'detail': raw}
        results[row['id']] = {'status': 'passed' if all(value['status'] == 'passed' for value in engines.values()) else 'mismatch', 'engines': engines, **provenance}
    return results


def reference_cases(rows, kind, directory, timeout=240):
    runner = sci.report_module()
    directory.mkdir(parents=True, exist_ok=True)
    input_path = directory / 'originals.tsv'
    input_path.write_text(''.join(row['id'] + '\t' + '\t'.join(row[key].encode().hex() for key in ('source', 'declaration_source', 'runtime_source')) + '\n' for row in rows))
    identity = runner.RELEASE if kind == 'release' else runner.REVISION
    checkout = sci.ROOT / 'research/clojurescript'
    dependency = '{:mvn/version "' + identity + '"}' if kind == 'release' else '{:local/root ' + json.dumps(str(checkout)) + '}'
    command = [os.environ.get('CLOJURE', 'clojure'), '-Srepro', '-Sdeps', '{:paths [] :deps {org.clojure/clojurescript ' + dependency + '}}', '-M', ORACLE, kind, identity, '-' if kind == 'release' else str(checkout), str(directory.resolve()), str(input_path.resolve())]
    built = runner.invoke(command, directory / 'build', timeout=timeout)
    if built['status'] != 'completed' or 'REFERENCE IDENTITY ' + kind + ' ' + identity not in built.get('stdout', '') or 'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in built.get('stdout', ''):
        return {row['id']: {'status': built['status'] if built['status'] != 'completed' else 'invalid-reference', 'detail': built} for row in rows}
    results = {}
    for index, row in enumerate(rows):
        script = 'process.chdir(' + json.dumps(str(directory.resolve())) + ');\nrequire(' + json.dumps(str((directory / 'reference.js').resolve())) + ');\n' + 'globalThis.lightCases = [lightCases[' + str(index) + ']];\nglobalThis.lightCaseMetadata = [lightCaseMetadata[' + str(index) + ']];\n' + (sci.ROOT / 'tests/support/execution.js').read_text()
        ran = runner.invoke([os.environ.get('NODE', 'node'), '-'], directory / row['id'], source=script, timeout=timeout)
        provenance = {'reference_identity': kind + ' ' + identity, 'settings': {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False},
            'source_sha256': row['source_sha256'], 'adapted_source_sha256': row['adapted_source_sha256'],
            'oracle_source_sha256': sci.digest((sci.ROOT / ORACLE).read_bytes()),
            'oracle_helper_source_sha256': sci.digest((sci.ROOT / 'tests/reference/execution.clj').read_bytes()),
            'observation_source_sha256': sci.digest((sci.ROOT / 'tests/support/execution.js').read_bytes()),
            'namespace': 'sci.library.case' + str(index), 'isolated': True}
        if ran['status'] != 'completed':
            results[row['id']] = {**ran, **provenance}
            continue
        layer = sci.verify(row, runner.observations(ran['stdout'], [row])[row['id']], runner.same_json)
        frames = ran['stderr'].splitlines()
        valid_frames = (len(frames) == 2 and frames[0].startswith('CASE START ') and frames[1].startswith('CASE END ') and
            json.loads(frames[0][11:]) == {'id': row['id'], 'source': row['source']} and
            json.loads(frames[1][9:]) == {'id': row['id']})
        if not valid_frames:
            layer.update(status='invalid-output', detail='Library stderr/identity frames differ', stderr=ran['stderr'])
        results[row['id']] = {**layer, **provenance}
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write-catalog', action='store_true')
    parser.add_argument('--plan', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--timeout', type=int, default=240)
    parser.add_argument('--local', action='store_true')
    parser.add_argument('--references', action='store_true')
    args = parser.parse_args()
    if args.write_catalog:
        CATALOG.write_text(''.join(json.dumps(row, separators=(',', ':')) + '\n' for row in generated_catalog()))
    rows = load()
    print('Validated complete library contexts:', len(rows), flush=True)
    if args.plan:
        args.plan.write_text(''.join(json.dumps(row, separators=(',', ':')) + '\n' for row in planning()))
    if not (args.local or args.references):
        return
    if args.output is None:
        parser.error('Execution requires --output')
    evidence = {row['id']: {} for row in rows}
    if args.local:
        observed = local_cases(rows, args.output / 'local', args.timeout)
        for identity, layer in observed.items(): evidence[identity]['javascript'] = layer
        print('local', dict(collections.Counter(row['status'] for row in observed.values())), flush=True)
    if args.references:
        for kind in ('release', 'checkout'):
            observed = reference_cases(rows, kind, args.output / kind, args.timeout)
            for identity, layer in observed.items(): evidence[identity][kind] = layer
            print(kind, dict(collections.Counter(row['status'] for row in observed.values())), flush=True)
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'evidence.json').write_text(json.dumps({'catalog_sha256': sci.digest(CATALOG.read_bytes()), 'evidence': evidence}, indent=2) + '\n')

if __name__ == '__main__': main()
