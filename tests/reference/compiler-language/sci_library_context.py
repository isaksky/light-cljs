#!/usr/bin/env python3
"""Bounded complete SCI string/set/walk/EDN import cohort, with untouched originals."""
from __future__ import annotations
import argparse, collections, json, os, zipfile
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
VERSION_DIFFERENCE_IDS = (
    'sci-data-walk-walk-list-meta', 'sci-data-walk-walk-seq-meta',
    'sci-data-walk-prewalk-list-meta', 'sci-data-walk-prewalk-seq-meta',
    'sci-data-walk-postwalk-list-meta', 'sci-data-walk-postwalk-seq-meta',
)
WALK_SOURCE_HASHES = {
    'release': '79859e8103b3a5c0ea6b8be5ed3bab7f6b2c17569f6770182f77480aec4e5234',
    'checkout': '52d33a5c841ce959eb2cb10aacb76b2d1bf06e93ef3ae8d30a05dc69ceb34378',
}


def reference_walk_source(kind):
    if kind == 'release':
        release = sci.report_module().RELEASE
        jar = Path.home() / '.m2/repository/org/clojure/clojurescript' / release / ('clojurescript-' + release + '.jar')
        with zipfile.ZipFile(jar) as archive:
            return sci.digest(archive.read('clojure/walk.cljs'))
    if kind == 'checkout':
        return sci.digest((sci.ROOT / 'research/clojurescript/src/main/cljs/clojure/walk.cljs').read_bytes())
    raise ValueError('Unknown pinned library reference kind')


def version_contract(identity):
    if identity not in VERSION_DIFFERENCE_IDS:
        return None
    case_id = 'sci-execution-' + identity
    metadata = {'$map': [[{'$keyword': {'namespace': None, 'name': 'm', 'spelling': 'm'}},
                          {'$keyword': {'namespace': None, 'name': 'ok', 'spelling': 'ok'}}]], 'count': 1}
    def returned(value):
        return {'id': case_id, 'outcome': {'status': 'returned', 'value': value}, 'trace': [], 'stdout': ''}
    return {'release': returned(None), 'checkout': returned(metadata), 'matching_pin': 'checkout',
            'source_basis': {'file': 'clojure/walk.cljs', 'source_sha256': WALK_SOURCE_HASHES,
                'reason': 'The release walk list/seq branches rebuild without metadata; the checked checkout wraps both rebuilt forms in with-meta using the original metadata. prewalk and postwalk recurse through those branches.'}}

# Original 16 retain reference namespace identities; append complete self-contained
# fixtures in corpus order. Reflection, stream and record setup remain unexecuted.
IDS = (
    'sci-string-join-adapted',
    'sci-string-join-lazy',
    'sci-string-replace-literal',
    'sci-string-escape-function',
    'sci-data-set-require',
    'sci-data-set-join-1',
    'sci-data-set-index-0',
    'sci-data-set-select-1',
    'sci-data-walk-require',
    'sci-data-walk-prewalk-order',
    'sci-data-walk-postwalk-order',
    'sci-data-walk-postwalk-transform',
    'sci-data-edn-require',
    'sci-data-edn-reader-precedence',
    'sci-data-edn-tag-nested',
    'sci-data-edn-tag-discard',
    'sci-string-gensym-relations',
    'sci-string-gensym-prefix-coercion',
    'sci-string-join-empty',
    'sci-string-join-values',
    'sci-string-case-ascii',
    'sci-string-case-coercion',
    'sci-string-case-expansion',
    'sci-string-reverse',
    'sci-string-trim-ascii',
    'sci-string-trim-unicode',
    'sci-string-trim-newline',
    'sci-string-blank',
    'sci-string-includes',
    'sci-string-prefix-suffix',
    'sci-string-index-basic',
    'sci-string-index-start',
    'sci-string-index-empty',
    'sci-string-index-coerce',
    'sci-string-index-utf16',
    'sci-string-replace-empty',
    'sci-string-replace-overlap',
    'sci-string-replace-char-first',
    'sci-string-split-lines',
    'sci-string-escape-map',
    'sci-string-escape-empty',
    'sci-string-escape-surrogates',
    'sci-string-argument-effects',
    'sci-string-index-character-high',
    'sci-string-index-character-wrap',
    'sci-string-index-empty-wrap',
    'sci-data-set-union-0',
    'sci-data-set-union-1',
    'sci-data-set-union-2',
    'sci-data-set-intersection-0',
    'sci-data-set-intersection-1',
    'sci-data-set-difference-0',
    'sci-data-set-difference-1',
    'sci-data-set-subsetp-0',
    'sci-data-set-subsetp-1',
    'sci-data-set-supersetp-0',
    'sci-data-set-supersetp-1',
    'sci-data-set-select-0',
    'sci-data-set-rename-keys-0',
    'sci-data-set-rename-keys-1',
    'sci-data-set-rename-keys-2',
    'sci-data-set-project-0',
    'sci-data-set-project-1',
    'sci-data-set-join-0',
    'sci-data-set-join-2',
    'sci-data-set-join-3',
    'sci-data-set-join-4',
    'sci-data-set-join-5',
    'sci-data-set-join-6',
    'sci-data-set-join-7',
    'sci-data-set-union-3',
    'sci-data-set-union-4',
    'sci-data-set-intersection-2',
    'sci-data-set-difference-2',
    'sci-data-set-select-2',
    'sci-data-set-subsetp-2',
    'sci-data-set-supersetp-2',
    'sci-data-set-union-meta-2',
    'sci-data-set-union-meta-3',
    'sci-data-set-intersection-meta-2',
    'sci-data-set-intersection-meta-3',
    'sci-data-set-difference-meta',
    'sci-data-set-select-meta',
    'sci-data-set-rename-keys-meta',
    'sci-data-set-project-meta',
    'sci-data-set-index-meta',
    'sci-data-walk-stringify-adapted',
    'sci-data-walk-keywordize',
    'sci-data-walk-stringify',
    'sci-data-walk-keywordize-keys-only',
    'sci-data-walk-stringify-keys-only',
    'sci-data-walk-prewalk-entries',
    'sci-data-walk-prewalk-replace',
    'sci-data-walk-prewalk-false',
    'sci-data-walk-prewalk-transform',
    'sci-data-walk-prewalk-mapentry',
    'sci-data-walk-postwalk-entries',
    'sci-data-walk-postwalk-replace',
    'sci-data-walk-postwalk-false',
    'sci-data-walk-postwalk-mapentry',
    'sci-data-walk-walk-list-meta',
    'sci-data-walk-walk-vector-meta',
    'sci-data-walk-walk-map-meta',
    'sci-data-walk-walk-set-meta',
    'sci-data-walk-walk-seq-meta',
    'sci-data-walk-prewalk-list-meta',
    'sci-data-walk-prewalk-vector-meta',
    'sci-data-walk-prewalk-map-meta',
    'sci-data-walk-prewalk-set-meta',
    'sci-data-walk-prewalk-seq-meta',
    'sci-data-walk-postwalk-list-meta',
    'sci-data-walk-postwalk-vector-meta',
    'sci-data-walk-postwalk-map-meta',
    'sci-data-walk-postwalk-set-meta',
    'sci-data-walk-postwalk-seq-meta',
    'sci-data-walk-shallow',
    'sci-data-walk-scalar',
    'sci-data-walk-empty-seq',
    'sci-data-walk-lazy-order',
    'sci-data-walk-keywordize-keys-meta',
    'sci-data-walk-stringify-keys-meta',
    'sci-data-edn-collections',
    'sci-data-edn-unevaluated',
    'sci-data-edn-metadata',
    'sci-data-edn-legacy-meta',
    'sci-data-edn-nsmap',
    'sci-data-edn-first-only',
    'sci-data-edn-empty',
    'sci-data-edn-comments',
    'sci-data-edn-discard',
    'sci-data-edn-false-literal',
    'sci-data-edn-nil-input',
    'sci-data-edn-false-input',
    'sci-data-edn-nil-bad-options',
    'sci-data-edn-eof-nil',
    'sci-data-edn-eof-false',
    'sci-data-edn-eof-keyword',
    'sci-data-edn-eof-throw-name',
    'sci-data-edn-nil-not-eof',
    'sci-data-edn-reader',
    'sci-data-edn-default',
    'sci-data-edn-reader-nil-fallback',
    'sci-data-edn-tag-nil',
    'sci-data-edn-tag-no-eval',
    'sci-data-edn-reject-deref',
    'sci-data-edn-reject-syntaxquote',
    'sci-data-edn-reject-unquote',
    'sci-data-edn-reject-anonymous',
    'sci-data-edn-reject-varquote',
    'sci-data-edn-reject-eval',
    'sci-data-edn-reject-regex',
    'sci-data-edn-reject-conditional',
    'sci-data-edn-reject-auto-keyword',
    'sci-data-edn-reject-auto-map',
    'sci-data-edn-reject-truncated',
    'sci-data-edn-reject-unknown-tag',
    'sci-data-edn-reject-discard-tag',
    'sci-data-edn-eof-error',
    'sci-data-set-join-empty-lazy',
    'sci-data-set-join-right-empty-lazy',
    'sci-data-set-join-empty-unread',
    'sci-data-set-join-right-empty',
    'sci-data-set-join-meta',
    'sci-data-walk-conj-failure-order',
    'sci-data-walk-pre-replace-nil',
    'sci-data-walk-post-replace-nil',
    'sci-data-walk-pre-new-children',
    'sci-data-walk-post-new-children',
    'sci-data-edn-meta-value',
    'sci-data-edn-readers-nil',
    'sci-data-edn-default-nil-result',
    'sci-data-edn-default-false-result',
    'sci-data-edn-reader-failure-identity',
    'sci-data-edn-eof-discard',
    'sci-data-edn-eof-comment',
    'sci-data-edn-eof-truncated',
    'sci-data-set-join-nil-rows',
    'sci-data-set-join-nil-rows-mapped',
    'sci-data-set-join-nil-empty-map',
    'sci-data-set-rename-self-metadata',
    'sci-data-set-rename-self',
    'sci-data-edn-reject-tag-plus',
    'sci-data-edn-reject-tag-minus',
    'sci-data-edn-reject-tag-space',
    'sci-data-edn-reject-constituent-deref',
    'sci-data-edn-reject-constituent-syntaxquote',
    'sci-data-edn-reject-constituent-unquote',
    'sci-data-edn-reject-dispatch-comment',
    'sci-data-set-project-row-meta',
    'sci-data-set-index-key-meta',
    'sci-data-set-project-key-representative',
)


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


def adapter_hash():
    return sci.digest(Path(__file__).read_bytes())


def compiler_sources():
    paths = sorted(path for path in (sci.ROOT / 'src').rglob('*') if path.is_file())
    paths += [sci.ROOT / name for name in ('Cargo.toml', 'Cargo.lock', 'build.rs') if (sci.ROOT / name).is_file()]
    return {str(path.relative_to(sci.ROOT)): sci.digest(path.read_bytes()) for path in paths}


def observations(output, rows):
    """Strict JSON newline frames; Unicode string data are never line breaks."""
    runner = sci.report_module()
    def fields(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('Duplicate JSON field: ' + key)
            result[key] = value
        return result
    def invalid_number(value):
        raise ValueError('Non-JSON numeric value: ' + value)
    try:
        lines = output.split('\n')
        if lines[-1] == '':
            lines.pop()
        values = [json.loads(line, object_pairs_hook=fields, parse_constant=invalid_number) for line in lines]
        if [value.get('id') for value in values] != [row['id'] for row in rows]:
            raise ValueError('Missing/extra/duplicate/reordered library JSON frame')
        if any(set(value) != {'id', 'outcome', 'trace', 'stdout'} for value in values):
            raise ValueError('Incomplete library outcome protocol')
    except (ValueError, TypeError, AttributeError) as error:
        return {row['id']: {'status': 'invalid-output', 'detail': str(error)} for row in rows}
    return {row['id']: sci.verify(row, {'status': 'completed', 'observed': value}, runner.same_json)
            for row, value in zip(rows, values)}


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
        canonical_contract = version_contract(identity)
        if canonical_contract is not None and sci.expected(original) != canonical_contract['checkout']:
            raise ValueError('Reviewed original walk metadata expectation changed: ' + identity)
        rows.append({'id': 'sci-execution-' + identity, 'sci_id': identity,
            'classification': 'pinned-version-difference' if canonical_contract else 'original-library-contract',
            'canonical_contract': canonical_contract,
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


def expression_rows(rows, contexts=None):
    """Specialize exact originals without dropping any unrelated expression."""
    contexts = load() if contexts is None else contexts
    by_id = {row['id']: row for row in contexts}
    if len(by_id) != len(contexts):
        raise ValueError('Duplicate SCI library specialization identity')
    for row in rows:
        context = by_id.get(row['id'])
        if context is None:
            continue
        keys = ('obligation_id', 'original_source_sha256', 'original_row_sha256', 'body_sha256', 'expected')
        if any(json.dumps(row.get(key), sort_keys=True) != json.dumps(context[key], sort_keys=True) for key in keys):
            raise ValueError('SCI library specialization detached from original expression: ' + row['id'])
    return [row for row in rows if row['id'] not in by_id]


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
            'reason': 'Complete self-contained four-library cohort; namespace reflection/repeat-require, input-stream and record fixtures outside it need additional setup and fresh execution, never source-based passing credit.'})
    return result


def local_cases(rows, directory, timeout=240):
    """Keep each invocation bounded while preserving every fresh observation."""
    result = {}
    for index in range(0, len(rows), 16):
        batch = rows[index:index + 16]
        result.update(local_batch(batch, directory / ('batch-' + str(index // 16)), timeout))
    return result


def local_batch(rows, directory, timeout=240):
    directory.mkdir(parents=True, exist_ok=True)
    compiler_hashes = compiler_sources()
    adapter_source_hash = adapter_hash()
    input_path, output = directory / 'input.tsv', directory / 'observations.tsv'
    input_path.write_text(''.join(row['id'] + '\t' + row['declaration_source'].encode().hex() + '\t' + row['runtime_source'].encode().hex() + '\n' for row in rows))
    command = ['cargo', 'test', '--locked', '--test', 'compiler_sci_library_context', TEST, '--', '--ignored', '--exact', '--nocapture']
    environment = {**os.environ, 'LIGHT_CLJS_SCI_LIBRARY_INPUT': str(input_path.resolve()), 'LIGHT_CLJS_SCI_LIBRARY_OUTPUT': str(output.resolve())}
    # invoke owns output files/process-group timeouts; environment is explicit
    # rather than mutating the process-wide Python environment.
    command = ['env', *[key + '=' + environment[key] for key in ('LIGHT_CLJS_SCI_LIBRARY_INPUT', 'LIGHT_CLJS_SCI_LIBRARY_OUTPUT')], *command]
    runner = sci.report_module()
    ran = runner.invoke(command, directory / 'cargo', timeout=timeout)
    if compiler_sources() != compiler_hashes or adapter_hash() != adapter_source_hash:
        return {row['id']: {'status': 'stale-build-input', 'detail': 'Compiler/adapter inputs changed during library execution'} for row in rows}
    if ran['status'] != 'completed' or 'running 1 test' not in ran.get('stdout', '') or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in ran.get('stdout', '') or not output.is_file():
        return {row['id']: {'status': ran['status'] if ran['status'] != 'completed' else 'failed', 'detail': ran} for row in rows}
    records = [line.split('\t') for line in output.read_text().removesuffix('\n').split('\n')]
    if [record[0] for record in records] != [row['id'] for row in rows]:
        raise ValueError('Missing/extra/reordered local library observations')
    results = {}
    for row, record in zip(rows, records):
        provenance = {'source_sha256': row['source_sha256'], 'adapted_source_sha256': row['adapted_source_sha256'],
            'library_context_adapter_sha256': adapter_source_hash, 'compiler_source_sha256': compiler_hashes,
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
                engines[engine] = {**sci.verify(row, {'status': 'completed', 'observed': observed}, runner.same_json), 'raw_output': raw}
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
    walk_source_hash = reference_walk_source(kind)
    built = runner.invoke(command, directory / 'build', timeout=timeout)
    if (built['status'] != 'completed' or 'REFERENCE IDENTITY ' + kind + ' ' + identity not in built.get('stdout', '') or
            'REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}' not in built.get('stdout', '') or
            reference_walk_source(kind) != walk_source_hash or walk_source_hash != WALK_SOURCE_HASHES[kind]):
        return {row['id']: {'status': built['status'] if built['status'] != 'completed' else 'invalid-reference', 'detail': built} for row in rows}
    results = {}
    for index, row in enumerate(rows):
        script = 'process.chdir(' + json.dumps(str(directory.resolve())) + ');\nrequire(' + json.dumps(str((directory / 'reference.js').resolve())) + ');\n' + 'globalThis.lightCases = [lightCases[' + str(index) + ']];\nglobalThis.lightCaseMetadata = [lightCaseMetadata[' + str(index) + ']];\n' + (sci.ROOT / 'tests/support/execution.js').read_text()
        ran = runner.invoke([os.environ.get('NODE', 'node'), '-'], directory / row['id'], source=script, timeout=timeout)
        provenance = {'reference_identity': kind + ' ' + identity, 'settings': {'optimizations': 'none', 'static-fns': False, 'def-emits-var': False},
            'walk_source_sha256': walk_source_hash,
            'library_context_adapter_sha256': adapter_hash(),
            'reference_build_stdout': built['stdout'], 'reference_build_stderr': built['stderr'],
            'source_sha256': row['source_sha256'], 'adapted_source_sha256': row['adapted_source_sha256'],
            'oracle_source_sha256': sci.digest((sci.ROOT / ORACLE).read_bytes()),
            'oracle_helper_source_sha256': sci.digest((sci.ROOT / 'tests/reference/execution.clj').read_bytes()),
            'observation_source_sha256': sci.digest((sci.ROOT / 'tests/support/execution.js').read_bytes()),
            'namespace': 'sci.library.case' + str(index), 'isolated': True}
        if ran['status'] != 'completed':
            results[row['id']] = {**ran, **provenance}
            continue
        layer = observations(ran['stdout'], [row])[row['id']]
        frames = ran['stderr'].removesuffix('\n').split('\n')
        valid_frames = (len(frames) == 2 and frames[0].startswith('CASE START ') and frames[1].startswith('CASE END ') and
            json.loads(frames[0][11:]) == {'id': row['id'], 'source': row['source']} and
            json.loads(frames[1][9:]) == {'id': row['id']})
        if not valid_frames:
            layer.update(status='invalid-output', detail='Library stderr/identity frames differ', stderr=ran['stderr'])
        results[row['id']] = {**layer, **provenance, 'raw_stdout': ran['stdout'], 'raw_stderr': ran['stderr']}
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
