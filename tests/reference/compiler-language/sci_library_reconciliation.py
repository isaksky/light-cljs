#!/usr/bin/env python3
"""Exact SCI/library dialect differences; no stored receipt grants a fresh pass.

The contract table retains the entire original row, including the SCI result and
any existing catch adapter. Actual MacroHost observations are a separate layer.
Both checked pins must come from the unchanged full 191-context oracle.
"""
from __future__ import annotations
import argparse
import json
import subprocess
from pathlib import Path
import sci_library_context as context
import sci_library_context_receipts as receipts
import sci_execution as sci

TABLE = context.HERE / 'sci-library-reconciliation-contracts.jsonl'
DRIVER = 'tests/compiler_sci_library_reconciliation.rs'
TEST = 'exact_library_contexts_execute_in_both_phases_and_host_lifecycles'
IDS = (
    'sci-execution-sci-data-edn-tag-discard',
    'sci-execution-sci-string-join-values',
    'sci-execution-sci-string-case-coercion',
    'sci-execution-sci-string-trim-unicode',
    'sci-execution-sci-string-index-empty',
    'sci-execution-sci-string-index-coerce',
    'sci-execution-sci-string-escape-map',
    'sci-execution-sci-string-escape-surrogates',
    'sci-execution-sci-string-index-character-wrap',
    'sci-execution-sci-string-index-empty-wrap',
    'sci-execution-sci-data-set-project-meta',
    'sci-execution-sci-data-edn-nil-bad-options',
    'sci-execution-sci-data-edn-eof-false',
    'sci-execution-sci-data-edn-eof-keyword',
    'sci-execution-sci-data-edn-eof-throw-name',
    'sci-execution-sci-data-edn-eof-error',
    'sci-execution-sci-data-edn-reader-failure-identity',
    'sci-execution-sci-data-edn-reject-tag-plus',
    'sci-execution-sci-data-edn-reject-tag-minus',
    'sci-execution-sci-data-edn-reject-tag-space',
    'sci-execution-sci-data-edn-reject-dispatch-comment',
    'sci-execution-sci-data-set-project-key-representative',
)
HEADER = '(ns user (:require [clojure.string] [clojure.set] [clojure.walk] [clojure.edn]))\n'
ENCODER = '''(letfn [(encode [v]
 (cond
  (nil? v) nil
  (or (boolean? v) (number? v) (string? v)) v
  (keyword? v) (js-obj "$keyword" (js-obj "namespace" (namespace v) "name" (name v) "spelling" (subs (str v) 1)))
  (symbol? v) (js-obj "$symbol" (js-obj "namespace" (namespace v) "name" (name v) "spelling" (str v)))
  (vector? v) (js-obj "$vector" (into-array (map encode v)))
  (map? v) (js-obj "$map" (into-array (map (fn [[k value]] (array (encode k) (encode value))) v)) "count" (count v))
  (set? v) (js-obj "$set" (into-array (map encode v)) "count" (count v))
  (list? v) (js-obj "$list" (into-array (map encode v)))
  (seq? v) (js-obj "$seq" (into-array (map encode v)))
  :else (throw (js/Error. "Unsupported exact library observation"))))]
(let [outcome (try
  (js-obj "status" "returned" "value" (encode (do ORIGINAL_RUNTIME)))
  (catch :default e (js-obj "status" "threw" "error" (js-obj "category" (cond (instance? js/TypeError e) "type-error" (instance? js/RangeError e) "range-error" (instance? js/ReferenceError e) "reference-error" :else "error")))))]
(when (not= "" @stderr) (throw (js/Error. "Unexpected original stderr")))
(js/JSON.stringify (js-obj "id" ORIGINAL_ID "outcome" outcome "trace" (into-array (map encode @trace)) "stdout" @stdout))))'''


def same(left, right):
    return sci.report_module().same_json(left, right)


def strict_json(text):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('Duplicate observation field')
            result[key] = value
        return result
    def constant(value):
        raise ValueError('Non-finite JSON observation: ' + value)
    return json.loads(text, object_pairs_hook=pairs, parse_constant=constant)


def load_contracts(path=TABLE):
    contracts = [strict_json(line) for line in Path(path).read_text().splitlines() if line]
    if tuple(contract['id'] for contract in contracts) != IDS:
        raise ValueError('Incomplete, extra or reordered exact contract identities')
    rows = {row['id']: row for row in context.load()}
    for contract in contracts:
        row = rows[contract['id']]
        if not same(contract.get('source_binding'), row):
            raise ValueError('Changed original source, expectation, context or catch adapter')
        if (contract.get('obligation_id') != row['obligation_id'] or
                not same(contract.get('historical_expected'), row['expected'])):
            raise ValueError('Changed original SCI expectation or obligation')
        for key in ('local_expected', 'release_expected', 'checkout_expected'):
            packet = contract[key]
            if (set(packet) != {'id', 'outcome', 'trace', 'stdout'} or packet['id'] != row['id'] or
                    not isinstance(packet['trace'], list) or not isinstance(packet['stdout'], str)):
                raise ValueError('Incomplete literal outcome/effects/output contract')
        if not same(contract['release_expected'], contract['checkout_expected']):
            raise ValueError('Exact canonical cohort requires both pins to agree')
        strict = row['sci_id'].startswith('sci-data-edn-')
        disposition = ('adopted-strict-edn-dialect-difference' if strict else
                       'canonical-sci-difference')
        if contract.get('disposition') != disposition:
            raise ValueError('Source-specific dialect classification changed')
        if strict:
            if not same(contract['local_expected'], row['expected']):
                raise ValueError('Strict EDN must retain its original literal contract')
            if same(contract['local_expected'], contract['release_expected']):
                raise ValueError('Strict EDN dialect must have distinct checked-pin observations')
        elif (not same(contract['local_expected'], contract['release_expected']) or
              same(contract['local_expected'], row['expected'])):
            raise ValueError('Canonical difference requires local/both-pin agreement distinct from SCI')
        basis = contract['scope_basis']
        source = sci.ROOT / basis['source']
        if not source.is_file() or sci.digest(source.read_bytes()) != basis['source_sha256']:
            raise ValueError('Adopted source-specific scope basis changed')
        if (contract.get('original_expectation_status_expected') != 'unresolved' or
                contract.get('local_original_expectation_status_expected') != ('passed' if strict else 'mismatch') or
                contract.get('checked_pin_original_expectation_status_expected') != {'release': 'mismatch', 'checkout': 'mismatch'}):
            raise ValueError('Original SCI result was promoted or hidden')
    return contracts


def module_source(row, phase):
    if phase not in {'application', 'macro'}:
        raise ValueError('Unknown actual compiler phase')
    body = ('(let [trace (atom []) stdout (atom "") stderr (atom "") '
            'observe (fn [v] (swap! trace conj v) v)] '
            '(set! *print-newline* true) '
            '(set-print-fn! (fn [text] (swap! stdout str text))) '
            '(set-print-err-fn! (fn [text] (swap! stderr str text))) ' +
            ENCODER.replace('ORIGINAL_RUNTIME', row['runtime_source'])
                   .replace('ORIGINAL_ID', json.dumps(row['id'])) + ')')
    execution = body if phase == 'application' else (
        '(defmacro exact-library-observation [] ' + body + ') (exact-library-observation)')
    return HEADER + row['declaration_source'] + '\n' + execution


def provenance():
    inputs = [DRIVER, str(Path(__file__).relative_to(sci.ROOT)),
              str(TABLE.relative_to(sci.ROOT)), str(context.CATALOG.relative_to(sci.ROOT)),
              context.DRIVER, context.ORACLE, context.INSPECTOR,
              'tests/reference/compiler-language/sci_library_context.py',
              'tests/reference/compiler-language/sci_library_context_receipts.py',
              'tests/reference/compiler-language/sci_execution.py',
              'tests/support/sci_execution_observation.js',
              'tests/support/execution.js', 'tests/reference/execution.clj']
    return {'production_revision': subprocess.check_output(
                ['git', 'rev-parse', 'HEAD'], cwd=sci.ROOT, text=True).strip(),
            'compiler_source_sha256': context.compiler_sources(),
            'library_source_sha256': context.local_libraries(),
            'inputs_sha256': {name: sci.digest((sci.ROOT / name).read_bytes()) for name in inputs},
            'observer_sha256': sci.digest((HEADER + ENCODER).encode()),
            'test_file': DRIVER, 'test_name': TEST,
            'phase_contract': 'actual defmacro body; full outcome/trace/stdout; fresh and reused MacroHost',
            'catalog_order': [row['id'] for row in context.load()]}


def frame_order():
    return [(identity, host, phase, engine) for identity in IDS
            for host in ('fresh', 'reused') for phase in ('application', 'macro')
            for engine in ('QuickJS', 'Node')]


def validate_frames(contracts, frames):
    if [(f.get('id'), f.get('host'), f.get('phase'), f.get('engine')) for f in frames] != frame_order():
        raise ValueError('Missing, duplicated or reordered actual phase/host/engine frames')
    by_id = {contract['id']: contract for contract in contracts}
    for frame in frames:
        contract = by_id[frame['id']]
        row = contract['source_binding']
        source = module_source(row, frame['phase'])
        if frame.get('source_sha256') != sci.digest(source.encode()):
            raise ValueError('Actual phase omitted or changed complete original source/context')
        if frame.get('exit') != 0 or frame.get('raw_stderr') != '':
            raise ValueError('Actual compiler/engine observation did not complete cleanly')
        raw = frame.get('raw_stdout')
        if not isinstance(raw, str) or not raw.endswith('\n') or len(raw.split('\n')) != 2:
            raise ValueError('Partial or extra raw actual observation frame')
        observed = strict_json(raw)
        if not same(observed, contract['local_expected']) or not same(observed, frame.get('observed')):
            raise ValueError('Complete actual phase outcome/effects/output differs from literal local contract')
        expected_status = contract['local_original_expectation_status_expected']
        actual_status = sci.verify(row, {'status': 'completed', 'observed': observed}, same)['status']
        if actual_status != expected_status or frame.get('original_expectation_status') != actual_status:
            raise ValueError('Original SCI status changed or mismatch promoted')
    return True


def validate_local(document):
    contracts = load_contracts()
    if document.get('schema_version') != 1 or not same(document.get('provenance'), provenance()):
        raise ValueError('Stale or incomplete actual compiler/build/driver/helper/catalog provenance')
    build = document.get('build', {})
    executable = Path(build.get('executable', ''))
    if (not executable.is_file() or sci.digest(executable.read_bytes()) != build.get('executable_sha256') or
            build.get('exit') != 0 or build.get('execution_exit') != 0 or
            document.get('test_name') != TEST or
            '1 passed; 0 failed; 0 ignored' not in build.get('execution_stdout', '') or
            TEST + ' ... ok' not in build.get('execution_stdout', '')):
        raise ValueError('Unchecked actual Rust audit build/execution')
    if not same(document.get('contracts'), contracts):
        raise ValueError('Exact table/source bindings changed after actual execution')
    validate_frames(contracts, document.get('frames', []))
    for frame in document['frames']:
        source = Path(frame.get('source_file', ''))
        compiled = Path(frame.get('compiled_file', ''))
        if (not source.is_file() or source.read_text() != module_source(
                next(c['source_binding'] for c in contracts if c['id'] == frame['id']), frame['phase']) or
                not compiled.is_file() or sci.digest(compiled.read_bytes()) != frame.get('compiled_sha256')):
            raise ValueError('Actual compiled/source artifact changed or disappeared')
        if frame['engine'] == 'Node':
            node_file = compiled.with_name('node.js')
            if (not node_file.is_file() or node_file.read_text() !=
                    'const actual = ' + compiled.read_text() + '; console.log(actual);' or
                    sci.digest(node_file.read_bytes()) != frame.get('node_script_sha256')):
                raise ValueError('Actual Node application module artifact changed')
    return contracts


def reconcile(local_document, full_document):
    """Require current full-oracle layers; keep historical SCI status explicit."""
    contracts = validate_local(local_document)
    catalog = context.load()
    if (full_document.get('catalog_sha256') != sci.digest(context.CATALOG.read_bytes()) or
            list(full_document.get('evidence', {})) != [row['id'] for row in catalog]):
        raise ValueError('Both-pin evidence must retain unchanged full 191 identities/namespaces')
    result = []
    for contract in contracts:
        row = contract['source_binding']
        layers = full_document['evidence'][row['id']]
        if receipts.classification(row, layers) != 'original-contract-unresolved':
            raise ValueError('Original SCI disagreement was removed or relabeled before reconciliation')
        for engine in ('Node', 'QuickJS'):
            if not same(layers['javascript']['engines'][engine]['observed'], contract['local_expected']):
                raise ValueError('Ordinary local and actual macro/application contracts differ')
        for pin in ('release', 'checkout'):
            if not same(layers[pin]['observed'], contract[pin + '_expected']):
                raise ValueError('Executed checked-pin result differs from separate literal contract')
        result.append({'id': row['id'], 'obligation_id': row['obligation_id'],
            'reconciliation': 'verified-' + contract['disposition'],
            'original_expectation_status': 'unresolved',
            'local_original_expectation_status': contract['local_original_expectation_status_expected'],
            'checked_pin_original_expectation_status': {pin: layers[pin]['status'] for pin in ('release', 'checkout')},
            'local_contract_status': 'passed', 'actual_macro_contract_status': 'passed',
            'checked_pin_contract_status': {'release': 'passed', 'checkout': 'passed'},
            'source_binding': row, 'contract': contract, 'layers': layers})
    return {'schema_version': 1, 'provenance': provenance(), 'reconciliations': result}


def run_local(directory):
    contracts = load_contracts()
    directory = Path(directory).resolve()
    directory.mkdir(exist_ok=False, parents=True)
    before = provenance()
    input_file, output_file = directory / 'input.tsv', directory / 'raw.tsv'
    input_file.write_text(''.join('\t'.join([c['id'],
        module_source(c['source_binding'], 'application').encode().hex(),
        module_source(c['source_binding'], 'macro').encode().hex(),
        json.dumps(c['local_expected'], separators=(',', ':'), ensure_ascii=False).encode().hex()
        ]) + '\n' for c in contracts))
    command = ['cargo', 'test', '--locked', '--test', 'compiler_sci_library_reconciliation',
               '--no-run', '--message-format=json']
    built = subprocess.run(command, cwd=sci.ROOT, capture_output=True, text=True)
    (directory / 'build.stdout').write_text(built.stdout)
    (directory / 'build.stderr').write_text(built.stderr)
    artifacts = [json.loads(line) for line in built.stdout.splitlines() if line.startswith('{')]
    executables = [a['executable'] for a in artifacts if a.get('reason') == 'compiler-artifact' and
        a.get('target', {}).get('name') == 'compiler_sci_library_reconciliation' and a.get('executable')]
    if built.returncode or len(executables) != 1:
        raise ValueError('Failed or ambiguous actual audit build; raw logs retained')
    import os
    executable = executables[0]
    env = dict(os.environ, LIGHT_LIBRARY_RECONCILIATION_INPUT=str(input_file),
               LIGHT_LIBRARY_RECONCILIATION_OUTPUT=str(output_file))
    execution = subprocess.run([executable, '--ignored', '--exact', TEST, '--nocapture'],
        cwd=sci.ROOT, env=env, capture_output=True, text=True)
    (directory / 'execution.stdout').write_text(execution.stdout)
    (directory / 'execution.stderr').write_text(execution.stderr)
    document = {'schema_version': 1, 'test_name': TEST, 'provenance': before, 'contracts': contracts,
        'build': {'command': command, 'exit': built.returncode, 'stdout': built.stdout, 'stderr': built.stderr,
            'executable': executable, 'executable_sha256': sci.digest(Path(executable).read_bytes()),
            'execution_command': [executable, '--ignored', '--exact', TEST, '--nocapture'],
            'execution_exit': execution.returncode, 'execution_stdout': execution.stdout,
            'execution_stderr': execution.stderr}, 'frames': []}
    by_id = {c['id']: c for c in contracts}
    for line in output_file.read_text().splitlines() if output_file.exists() else []:
        identity, host, phase, engine, stdout_hex, stderr_hex, compiled_file = line.split('\t')
        raw = bytes.fromhex(stdout_hex).decode()
        observed = strict_json(raw)
        compiled = Path(compiled_file)
        row = by_id[identity]['source_binding']
        frame = {'id': identity, 'host': host, 'phase': phase, 'engine': engine,
            'source_file': str(compiled.with_name('user.cljs')),
            'source_sha256': sci.digest(compiled.with_name('user.cljs').read_bytes()),
            'compiled_file': str(compiled), 'compiled_sha256': sci.digest(compiled.read_bytes()),
            'raw_stdout': raw, 'raw_stderr': bytes.fromhex(stderr_hex).decode(), 'exit': 0,
            'observed': observed,
            'original_expectation_status': sci.verify(row, {'status': 'completed', 'observed': observed}, same)['status']}
        if engine == 'Node': frame['node_script_sha256'] = sci.digest(compiled.with_name('node.js').read_bytes())
        document['frames'].append(frame)
    receipt = directory / 'receipt.json'
    receipt.write_text(json.dumps(document, indent=2, ensure_ascii=False) + '\n')
    if not same(before, provenance()):
        raise ValueError('Inputs changed during actual audit; raw history retained, no fresh pass')
    validate_local(document)
    return document


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-local', type=Path)
    parser.add_argument('--local', type=Path)
    parser.add_argument('--full-evidence', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    document = run_local(args.run_local) if args.run_local else (
        strict_json(args.local.read_text()) if args.local else None)
    if document is None:
        print('Exact contracts validated:', len(load_contracts()))
        return
    validate_local(document)
    if args.full_evidence:
        result = reconcile(document, strict_json(args.full_evidence.read_text()))
        result['local_evidence'] = document
        result['full_evidence_path'] = str(args.full_evidence.resolve())
        result['full_evidence_sha256'] = sci.digest(args.full_evidence.read_bytes())
        if args.output is None:
            parser.error('--full-evidence requires --output to retain complete provenance')
        args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
        print('Exact source-specific reconciliations verified:', len(result['reconciliations']))
    else:
        print('Fresh local frames verified:', len(document['frames']), '; no checked-pin reconciliation claimed')

if __name__ == '__main__': main()
