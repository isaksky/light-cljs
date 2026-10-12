#!/usr/bin/env python3
"""Collect current checkpoint checks without manufacturing full-gate credit."""
from pathlib import Path
import argparse
import hashlib
import json
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def read(name):
    return json.loads((HERE / name).read_text())


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def binding(files):
    for name, expected in files.items():
        require(digest(ROOT / name) == expected, 'Current binding drift: ' + name)


def rejected(document, count, restored_field=None):
    controls = document['controls']
    require(len(controls) == count and all(row['status'] == 'rejected' for row in controls),
            'Incomplete rejection controls')
    if restored_field:
        require(document[restored_field] == 'passed', 'Pristine restoration missing')


def collect(output, source_unit_log):
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    arity = read('arity-after194-installed-current-context-receipt.json')
    require(head == arity['head'], 'Checkpoint HEAD changed before final collection')
    binding(arity['frozen_source_snapshot']['source_sha256'])
    require(len(arity['actual_frames']) == 216 and len(arity['actual_input_hash_frames']) == 108,
            'Incomplete actual arity frames')
    require(len(arity['module_source_emitted_js_and_raw_sha256']) == 759,
            'Incomplete actual arity artifact census')
    for name, expected in arity['module_source_emitted_js_and_raw_sha256'].items():
        require(digest(Path(arity['modules_path']) / name) == expected, 'Actual arity artifact changed')
    require(digest(arity['actual_executable']) == arity['actual_executable_sha256'],
            'Actual arity executable changed')
    require(digest(arity['actual_log_path']) == arity['actual_log_sha256'], 'Actual arity log changed')
    require(digest(arity['guards_log_path']) == arity['guard_log_sha256'], 'Arity guard log changed')
    for pin in arity['references'].values():
        require(len(pin['observations']) == 27 and all(row['status'] == 'passed' for row in pin['observations'].values()),
                'Incomplete whole-original arity pin results')
    loader = read('arity-after194-installed-loader-audit-receipt.json')
    require(loader['ordinary_and_phase_rows'] == 16 and loader['linked_complete_original_oracles'] == 10,
            'Installed arity census changed')
    require(sum(bool(line.strip()) for line in (HERE / 'obligation-witnesses.jsonl').read_text().splitlines()) == 1963 and
            sum(bool(line.strip()) for line in (HERE / 'phase-witnesses.jsonl').read_text().splitlines()) == 1978,
            'Permanent ordinary/phase row census changed')
    history = read('arity-after194-installed-current-history-preservation-receipt.json')
    require(digest(HERE / 'arity-after194-context-receipt.json') == history['preintegration_context_sha256'],
            'Preintegration arity proof changed')
    require(digest(HERE / 'arity-after194-raw-guard-receipt.json') == history['preintegration_raw_guard_sha256'],
            'Preintegration raw controls changed')
    raw = read('arity-after194-installed-current-raw-guard-receipt.json')
    rejected(raw, 17, 'restored_pristine_replay')
    require(raw['receipt_sha256'] == digest(HERE / 'arity-after194-installed-current-context-receipt.json'),
            'Final raw controls bind another proof')

    integration = read('integer-source-context-after194-installed-report-integration.json')
    binding({str((HERE / name).relative_to(ROOT)): value
             for name, value in integration['current_consumer_binding'].items()})
    require(integration['full_report_universe_rows'] == 9991 and
            integration['original_inventory_rows'] == 4728 and
            integration['independent_obligation_rows'] == 5263 and
            integration['changed_rows'] == ['src/test/cljs/cljs/predicates_test.cljs::test-int-preds::assertion-001'] and
            integration['absent_status'] == 'missing' and integration['present_status'] == 'covered' and
            integration['public_or_local_arity_credit'] == 0 and not integration['strict_complete'],
            'Whole-source report reconciliation changed')
    integer = read('integer-source-context-after194-installed.json')
    require(hashlib.sha256(json.dumps(integer, sort_keys=True).encode()).hexdigest() ==
            integration['current_source_document_sha256'], 'Source document semantic binding changed')
    binding(integer['current_binding']['source_snapshot'])
    require(integer['original_loop_comparisons'] == 112 and integer['original_site_count'] == 1 and
            integer['public_local_arity_links_added'] == 0, 'Integer original scope changed')
    rejected(read('integer-original-source-after194-installed-raw-controls.json'), 13, 'restored_pristine_replay')
    rejected(read('integer-source-context-after194-installed-reconciliation-controls.json'), 7,
             'pristine_binding_restored')
    rejected(integration['mechanical_consumer_controls'], 9)
    gaps = read('sci-originals-after194-gap-proof.json')
    require(digest(HERE / 'sci_originals_after194_gap_proof.py') == gaps['producer_source_sha256'],
            'Focused gap proof producer changed')
    binding(read('sci-originals-after194-phase-discovery.json')['source_snapshot'])
    binding(read('sci-originals-after194-source-discovery.json')['source_snapshot'])
    require(digest(gaps['actual_executable']) == gaps['actual_executable_sha256'],
            'Focused gap actual executable changed')
    rejected(gaps['mutation_restoration_controls'], 10, 'pristine_complete_inputs_restored')
    require(set(gaps['gaps']) == {'sci-record-constructor-metadata', 'sci-record-method-duplicate-parameter'} and
            gaps['original_semantic_pass_credit'] == 0 and not gaps['strict_complete'],
            'Focused gap proof scope changed')
    source_manifest = read('source-evidence-after194-installed-files.json')
    documentation_path = 'tests/reference/compiler-language/arity-after194-installed-current.md'
    for name, expected in source_manifest['files'].items():
        retained = (HERE / 'arity-after194-installed-current-before-final-controls.md'
                    if name == documentation_path else ROOT / name)
        require(digest(retained) == expected, 'Installed source manifest history unavailable: ' + name)

    generation = read('after194-generated-manifest-review.json')
    for row in generation['outputs_byte_identical']:
        require(digest(ROOT / row['path']) == row['sha256'], 'Original generated ledger changed')
    require(len(generation['outputs_changed']) == 1 and
            generation['outputs_changed'][0]['changed_fields'] == ['generator_sha256'] and
            digest(ROOT / generation['outputs_changed'][0]['path']) == generation['outputs_changed'][0]['new_sha256'],
            'Generated manifest change exceeded generator provenance')
    manifest = json.loads((HERE / 'full-manifest.json').read_text())
    require(manifest['generator_sha256'] == digest(HERE / 'full_ledger.py'), 'Generator manifest drift')

    inventory = read('safe-source-regression-audit-after194-installed-current.json')
    require(inventory['test_count'] == 848 and inventory['registered_phase_test_count'] == 65 and
            inventory['counts'] == {'safe-ordinary-proposed': 711, 'excluded-resource-control': 33,
                                    'ignored-unexecuted': 104} and
            inventory['safe_ordinary_not_registered_in_full_gate'] == 646 and
            len(inventory['proposed_target_commands']) == 129 and inventory['executed_regressions'] == 0,
            'List-only safe proposal accounting changed')
    interrupted = read('after194-interrupted-full-gate-receipt.json')
    require(not interrupted['report_present'] and not interrupted['full_gate_complete'] and
            len(interrupted['partial_artifacts_sha256']) == 1191 and
            not interrupted['outside_processes_signaled'], 'Interrupted gate misrepresented')
    for name, expected in interrupted['partial_artifacts_sha256'].items():
        require(digest(Path('/tmp/light-cljs-125-full-after194') / name) == expected,
                'Preserved partial gate artifact changed')

    logs = {}
    for name, count in [('report', 42), ('full-ledger', 22)]:
        path = Path('/tmp/light-cljs-125-after194-integrated-' + name + '-guards.log')
        text = path.read_text()
        require('Ran ' + str(count) + ' tests' in text and text.rstrip().endswith('OK'),
                'Current permanent guard run missing: ' + name)
        logs[name] = {'path': str(path.resolve()), 'sha256': digest(path), 'tests': count}
    require('Ran 20 tests' in Path(arity['guards_log_path']).read_text(), 'Arity guard count changed')
    unit = Path(source_unit_log)
    require('Ran 8 tests' in unit.read_text() and unit.read_text().rstrip().endswith('OK'),
            'Current eight whole-source unit guards missing')
    logs['source'] = {'path': str(unit.resolve()), 'sha256': digest(unit), 'tests': 8}
    logs['arity'] = {'path': arity['guards_log_path'], 'sha256': arity['guard_log_sha256'], 'tests': 20}
    clippy = Path('/tmp/light-cljs-125-after194-integrated-clippy.log')
    require('Finished' in clippy.read_text() and 'warning:' not in clippy.read_text() and
            'error:' not in clippy.read_text(), 'Strict Clippy diagnostics present')
    rustfmt = Path('/tmp/light-cljs-125-after194-new-driver-rustfmt.log')
    require(rustfmt.read_bytes() == b'', 'New driver rustfmt diagnostics present')
    require(not subprocess.check_output(['git', 'diff', 'HEAD', '--name-only', '--', 'src', 'build.rs',
                                         'Cargo.toml', 'Cargo.lock'], cwd=ROOT), 'Production source changed')
    subprocess.run(['git', 'diff', '--check'], cwd=ROOT, check=True)
    subprocess.run(['git', 'diff', '--cached', '--check'], cwd=ROOT, check=True)
    owned = [path for path in HERE.iterdir() if path.is_file() and 'after194' in path.name
             and path.name != Path(output).name]
    result = {
        'schema_version': 1, 'head_before_checkpoint_commit': head,
        'status': 'passed-current-focused-evidence-checkpoint',
        'installed_arity_pairs': 16, 'public_pairs': 14, 'local_pairs': 2,
        'permanent_ordinary_rows': 1963, 'permanent_phase_rows': 1978,
        'complete_arity_originals': 27, 'actual_arity_frames': 216, 'whole_original_arity_pins': ['release', 'checkout'],
        'source_original_assertions_added': 1, 'source_original_comparisons': 112,
        'source_report_universe_preserved': 9991, 'source_arity_credit_added': 0,
        'permanent_guard_tests': 92, 'guard_logs': logs,
        'controls': {'arity_raw_restoration': 17, 'integer_raw_restoration': 13,
                     'integer_mapping': 7, 'consumer_mechanical_only': 9, 'gap_restoration': 10},
        'clippy': {'command': ['cargo', 'clippy', '--locked', '--all-targets', '--', '-D', 'warnings'],
                   'log_path': str(clippy.resolve()), 'log_sha256': digest(clippy), 'diagnostics': 0},
        'new_rust_driver_rustfmt': {'command': ['rustfmt', '--check', '--edition', '2024', 'tests/compiler_arity_after194.rs'],
                                  'log_path': str(rustfmt.resolve()), 'log_sha256': digest(rustfmt), 'passed': True},
        'genuine_generated_review': generation,
        'list_only_regression_proposal': {'identities': 848, 'proposed_active': 711, 'whole_exclusions': 33,
                                         'ignored_unexecuted': 104, 'target_commands': 129, 'executed_regressions': 0},
        'interrupted_full_gate': {'partial_files_preserved': 1191, 'report_completed': False, 'counts_claimed': False},
        'historical_receipts_rewritten': False, 'production_source_changed': False,
        'collector_initial_error': 'Scalar gap-producer digest was initially treated as a file map; corrected against the producer and separate unchanged phase/source snapshots before any final receipt existed.',
        'source_manifest_prior_documentation_retained': 'arity-after194-installed-current-before-final-controls.md',
        'final_arity_documentation_updated_only_after_successful_raw_controls': True,
        'performance_timing_claim': False, 'known_preexisting_rustfmt_drift_files': 77,
        'separate_focused_open_owners': ['light-cljs-195', 'light-cljs-196'],
        'external_block': '156 native/asynchronous cancellation repair rejected twice by automatic review; no retry or related test execution',
        'strict_complete': False,
        'current_owned_evidence_sha256': {str(path.relative_to(ROOT)): digest(path) for path in sorted(owned)},
    }
    Path(output).write_text(json.dumps(result, indent=2) + '\n')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--source-unit-log', type=Path, required=True)
    args = parser.parse_args()
    receipt = collect(args.output, args.source_unit_log)
    print(json.dumps({key: receipt[key] for key in ['status', 'permanent_guard_tests', 'strict_complete']}))
