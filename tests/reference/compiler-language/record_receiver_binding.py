#!/usr/bin/env python3
"""Fresh whole-source both-pin and actual phase proof for canonical receiver shadowing."""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sci_execution as sci
import sci_source_context as context
import sci_originals_after191_candidates as bindings
from extract_inventory import Reader, short_head

DRIVER = sci.ROOT / 'tests/compiler_record_receiver_binding.rs'
CONTROL = sci.ROOT / 'tests/fixtures/record-receiver-binding/controls.cljs'
IDENTITY = 'sci-record-method-duplicate-parameter'
ORIGINAL = '(defprotocol P (p [x n])) (defrecord R [x] P (p [n n] [x (if (record? n) :receiver n)])) (p (->R 3) 9)'
CANONICAL = {'id': 'sci-execution-' + IDENTITY, 'outcome': {'status': 'returned', 'value': {'$vector': [3,
    {'$keyword': {'namespace': None, 'name': 'receiver', 'spelling': 'receiver'}}]}}, 'trace': [], 'stdout': ''}



def load():
    original_row, fields = sci.rows(sci.CORPUS)[IDENTITY]
    disposition_row, disposition = sci.rows(sci.DISPOSITIONS)[IDENTITY]
    assert fields == disposition[:7] and fields[2] == ORIGINAL
    assert sci.expected(fields) == CANONICAL
    rows = []
    for identity, source, expected in [(CANONICAL['id'], ORIGINAL, CANONICAL),
            ('record-receiver-controls', CONTROL.read_text(),
             {'id': 'record-receiver-controls', 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''})]:
        forms = [{'kind': 'macro' if short_head(n) == 'defmacro' else 'runtime',
                  'source': source[n.start:n.end], 'utf8_byte_start': len(source[:n.start].encode()),
                  'utf8_byte_end': len(source[:n.end].encode())} for n in Reader(source).all()]
        rows.append({'id': identity, 'source': source, 'source_sha256': sci.digest(source),
                     'expected': expected, 'namespace': 'user', 'source_name': 'sci-original.cljs', 'forms': forms})
    return rows, {'original_fields': fields, 'original_row_sha256': sci.digest(original_row),
                  'disposition_row_sha256': sci.digest(disposition_row), 'historical_expected': sci.expected(fields)}


def snapshot():
    return {**context.compiler_sources(), **bindings.pin_inputs.reference_inputs(),
        **{str(p.relative_to(sci.ROOT)): sci.digest(p.read_bytes()) for p in (
        Path(__file__), DRIVER, CONTROL, sci.CORPUS, sci.DISPOSITIONS, Path(sci.__file__),
        Path(context.__file__), Path(bindings.__file__), Path(bindings.pin_inputs.__file__),
        Path(sci.report_module().__file__), sci.ROOT/'tests/compiler_sci_source_context.rs',
        sci.ROOT/context.ORACLE, sci.ROOT/'tests/reference/execution.clj',
        sci.ROOT/'tests/support/execution.js', sci.ROOT/'tests/support/sci_execution_observation.js')}}


def run(root):
    root = root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    rows, original = load()
    before = snapshot()
    binding = bindings.reference_execution_binding()
    document = {'source_bindings': rows, 'original_provenance': original, 'producer_sha256': before,
                'historical_sci_expectations_modified': False, 'original_sci_pass_credit': 0,
                'canonical_top_level': {}, 'reference_execution_binding': binding}
    # These adapters compile every original form in order, preserving the user namespace.
    document['canonical_top_level']['local'] = context.local_cases(rows, root/'top-level-local', 600)
    for kind in ('release', 'checkout'):
        document['canonical_top_level'][kind] = context.reference_cases(rows[:1], kind, root/kind, 600)
        print(kind, [r['status'] for r in document['canonical_top_level'][kind].values()], flush=True)
    for layers in document['canonical_top_level'].values():
        assert list(layers) == [r['id'] for r in (rows if layers is document['canonical_top_level']['local'] else rows[:1])]
        assert all(v['status'] == 'passed' for v in layers.values()), layers
    control_root = root/'control-source'
    control_root.mkdir()
    control_source = '(ns user)\n(defn run [observe] (do '+rows[1]['source']+'))\nrun\n'
    (control_root/'user.cljs').write_text(control_source)
    control_case = {**rows[1], 'source': control_source, 'source_root': str(control_root), 'entry': 'user', 'export': 'run'}
    document['control_namespace_references'] = {}
    for kind in ('release', 'checkout'):
        layers = sci.report_module().reference_cases([control_case], kind, sci.ROOT/'research/clojurescript', root/('controls-'+kind), 600)
        layer = sci.verify(rows[1], layers[rows[1]['id']], sci.report_module().same_json)
        assert layer['status'] == 'passed', layer
        document['control_namespace_references'][kind] = layer
        print(kind, 'complete namespace controls passed', flush=True)
    phase_root = root/'actual-phases'
    command = ['cargo', 'test', '--locked', '--test', DRIVER.stem, '--', '--nocapture']
    ran = subprocess.run(command, cwd=sci.ROOT,
        env={**os.environ, 'LIGHT_RECORD_RECEIVER_ARTIFACTS': str(phase_root)},
        text=True, capture_output=True, timeout=600)
    (root/'actual-command.json').write_text(json.dumps(command)+'\n')
    (root/'actual.stdout').write_text(ran.stdout)
    (root/'actual.stderr').write_text(ran.stderr)
    assert ran.returncode == 0 and '2 passed; 0 failed;' in ran.stdout, ran.stdout+ran.stderr
    observations = []
    for line in ran.stdout.splitlines():
        if not line.startswith('record-receiver-observation\t'): continue
        _, identity, lifecycle, phase, engine, raw = line.split('\t')
        row = next(r for r in rows if r['id'] == identity)
        assert json.loads(raw) == row['expected']
        folder = phase_root/identity/lifecycle/phase
        source = (folder/'user.cljs').read_text()
        assert '(do '+row['source']+')' in source
        if phase == 'macro': assert '(arity-witness)' in source and 'js*' in source
        observations.append({'id': identity, 'lifecycle': lifecycle, 'phase': phase, 'engine': engine,
            'packet': json.loads(raw), 'source_sha256': sci.digest(source),
            'compiled_js_sha256': sci.digest((folder/'compiled.js').read_bytes())})
    assert [(f['id'], f['lifecycle'], f['phase'], f['engine']) for f in observations] == [
        (r['id'], l, p, e) for r in rows for l in ('fresh', 'reused')
        for p in ('application', 'macro') for e in ('QuickJS', 'Node')]
    # Reexecute the exact retained Node inputs independently of the collector.
    independent = []
    for row in rows:
        for lifecycle in ('fresh', 'reused'):
            for phase in ('application', 'macro'):
                folder = phase_root/row['id']/lifecycle/phase
                replay = subprocess.run([binding['programs']['node'], '-'], input=(folder/'node.stdin').read_text(),
                    text=True, capture_output=True, timeout=120)
                assert replay.returncode == 0 and replay.stderr == ''
                assert replay.stdout == (folder/'node.stdout').read_text()
                hashes, raw = replay.stdout.rstrip('\n').split('\n')
                assert hashes.split('\t') == [sci.digest((folder/n).read_bytes()) for n in ('compiled.js', 'user.cljs', 'script.js')]
                assert json.loads(raw) == row['expected']
                independent.append({'id': row['id'], 'lifecycle': lifecycle, 'phase': phase, 'packet': json.loads(raw)})
    replay_command = ['cargo', 'test', '--locked', '--test', DRIVER.stem, 'replay_actual_quickjs_inputs', '--', '--ignored', '--exact', '--nocapture']
    replay = subprocess.run(replay_command, cwd=sci.ROOT,
        env={**os.environ, 'LIGHT_RECORD_RECEIVER_ARTIFACTS': str(phase_root)}, text=True, capture_output=True, timeout=600)
    (root/'independent-quickjs.stdout').write_text(replay.stdout)
    (root/'independent-quickjs.stderr').write_text(replay.stderr)
    assert replay.returncode == 0 and '1 passed; 0 failed;' in replay.stdout
    frames = [line.split('\t') for line in replay.stdout.splitlines() if line.startswith('receiver-independent-quickjs\t')]
    assert len(frames) == 8
    for (_, identity, lifecycle, phase, raw), expected in zip(frames, independent):
        assert (identity, lifecycle, phase) == (expected['id'], expected['lifecycle'], expected['phase'])
        assert json.loads(raw) == expected['packet']
    document['independent_actual_node_replays'] = independent
    document['independent_actual_quickjs_replays'] = frames
    # The top-level pins execute the complete original in user, without a wrapper.
    document['independent_original_pin_replays'] = {}
    for kind in ('release', 'checkout'):
        folder = root/kind/rows[0]['id']
        replay = subprocess.run([binding['programs']['node'], '-'], input=(folder/'stdin').read_text(),
            text=True, capture_output=True, timeout=120)
        assert replay.returncode == 0
        assert replay.stdout == (folder/'stdout').read_text()
        assert replay.stderr == (folder/'stderr').read_text()
        packets = [json.loads(line) for line in replay.stdout.splitlines() if line.startswith('{')]
        assert len(packets) == 1 and packets[0] == CANONICAL
        document['independent_original_pin_replays'][kind] = packets[0]
    assert snapshot() == before and bindings.reference_execution_binding() == binding
    document.update(actual_frames=observations, actual_macro_same_host_failure_recovery='passed',
        deferred_unrelated_root_laziness='passed', artifact_root=str(root), status='canonical-passed')
    document['artifact_sha256'] = {str(p.relative_to(root)): sci.digest(p.read_bytes())
        for p in sorted(root.rglob('*')) if p.is_file()}
    (root/'evidence.json').write_text(json.dumps(document, indent=2)+'\n')
    return document


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    run(parser.parse_args().output)
