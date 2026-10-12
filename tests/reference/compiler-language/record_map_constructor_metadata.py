#!/usr/bin/env python3
"""Fresh whole-source both-pin and actual phase proof for unary record map factories."""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sci_execution as sci
import sci_source_context as context
from extract_inventory import Reader, short_head

DRIVER = sci.ROOT / 'tests/compiler_record_map_constructor_metadata.rs'
CONTROL = sci.ROOT / 'tests/fixtures/record-map-constructor-metadata/controls.cljs'
IDENTITY = 'sci-record-constructor-metadata'
ORIGINAL = '(defrecord R [x y]) [(:arglists (meta (var ->R))) (mapv count (:arglists (meta (var map->R)))) (ns-name (:ns (meta (var ->R))))]'
CANONICAL = {'id': 'sci-execution-' + IDENTITY, 'outcome': {'status': 'returned', 'value': {'$vector': [
    {'$list': [{'$vector': [{'$symbol': {'namespace': None, 'name': n, 'spelling': n}} for n in ('x', 'y')]}]},
    {'$vector': [1]}, 'user']}}, 'trace': [], 'stdout': ''}


def load():
    original_row, fields = sci.rows(sci.CORPUS)[IDENTITY]
    disposition_row, disposition = sci.rows(sci.DISPOSITIONS)[IDENTITY]
    assert fields == disposition[:7] and fields[2] == ORIGINAL
    rows = []
    for identity, source, expected in [(CANONICAL['id'], ORIGINAL, CANONICAL),
            ('record-map-constructor-controls', CONTROL.read_text(),
             {'id': 'record-map-constructor-controls', 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''})]:
        forms = [{'kind': 'macro' if short_head(n) == 'defmacro' else 'runtime',
                  'source': source[n.start:n.end], 'utf8_byte_start': len(source[:n.start].encode()),
                  'utf8_byte_end': len(source[:n.end].encode())} for n in Reader(source).all()]
        rows.append({'id': identity, 'source': source, 'source_sha256': sci.digest(source),
                     'expected': expected, 'namespace': 'user', 'source_name': 'sci-original.cljs', 'forms': forms})
    return rows, {'original_fields': fields, 'original_row_sha256': sci.digest(original_row),
                  'disposition_row_sha256': sci.digest(disposition_row), 'historical_expected': sci.expected(fields)}


def snapshot():
    return {**context.compiler_sources(), **{str(p.relative_to(sci.ROOT)): sci.digest(p.read_bytes())
        for p in (Path(__file__), DRIVER, CONTROL, sci.CORPUS, sci.DISPOSITIONS)}}


def run(root):
    root = root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    rows, original = load()
    before = snapshot()
    document = {'source_bindings': rows, 'original_provenance': original, 'producer_sha256': before,
                'historical_sci_expectations_modified': False, 'original_sci_pass_credit': 0,
                'canonical_top_level': {}}
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
        env={**os.environ, 'LIGHT_RECORD_METADATA_ARTIFACTS': str(phase_root)},
        text=True, capture_output=True, timeout=600)
    (root/'actual-command.json').write_text(json.dumps(command)+'\n')
    (root/'actual.stdout').write_text(ran.stdout)
    (root/'actual.stderr').write_text(ran.stderr)
    assert ran.returncode == 0 and '2 passed; 0 failed;' in ran.stdout, ran.stdout+ran.stderr
    observations = []
    for line in ran.stdout.splitlines():
        if not line.startswith('record-metadata-observation\t'): continue
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
    assert snapshot() == before
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
