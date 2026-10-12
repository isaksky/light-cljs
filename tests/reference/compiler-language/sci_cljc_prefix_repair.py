"""Fresh focused prefix repair proof; the original 30-source probe stays diagnostic."""
from pathlib import Path
import argparse
import json
import re
import shutil
import sci_cljc_source_probe as audit
import sci_execution as sci
import report

ROOT = report.ROOT
CONTROLS = ROOT / 'tests/compiler_cljc_prefixes.rs'


def snapshot():
    return {**audit.snapshot(), str(CONTROLS.relative_to(ROOT)): sci.digest(CONTROLS.read_bytes()),
            str(Path(__file__).relative_to(ROOT)): sci.digest(Path(__file__).read_bytes())}


def binary(test, directory):
    built = report.invoke(['cargo', 'test', '--locked', '--no-run', '--message-format=json',
                           '--test', test], directory, timeout=240)
    if built['status'] != 'completed':
        raise ValueError('focused driver did not build')
    paths = [r['executable'] for r in map(json.loads, built['stdout'].splitlines())
             if r.get('reason') == 'compiler-artifact' and r.get('target', {}).get('name') == test
             and r.get('executable')]
    if len(paths) != 1:
        raise ValueError('Cargo did not identify one exact driver')
    path = Path(paths[0]).resolve()
    return {'path': str(path), 'sha256': sci.digest(path.read_bytes())}


def control_rows():
    text = CONTROLS.read_text().split('const CASES:', 1)[1].split('\n];', 1)[0]
    pairs = re.findall(r'\(\s*("(?:\\.|[^"\\])*")\s*,\s*("(?:\\.|[^"\\])*")\s*,?\s*\)', text)
    if len(pairs) != 23:
        raise ValueError('unreviewed control catalog')
    rows = []
    for index, (original, expected) in enumerate(pairs):
        original, expected = json.loads(original), json.loads(expected)
        source = '(pr-str (do ' + original + '))'
        identity = 'cljc-prefix-control-' + str(index + 1)
        rows.append({'id': identity, 'source': source, 'original_control_source': original,
                     'source_sha256': sci.digest(source), 'source_name': 'sci-original.cljc',
                     'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': expected},
                                  'trace': [], 'stdout': ''}})
    return rows


def execute(directory):
    directory = directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    before = snapshot()
    executables = {test: binary(test, directory / ('build-' + test)) for test in
                   ('compiler_sci_cljc_source_probe', 'compiler_cljc_prefixes')}
    for name in ('node', 'clojure'):
        path = Path(shutil.which(name)).resolve()
        executables[name] = {'path': str(path), 'sha256': sci.digest(path.read_bytes())}
    ran = report.invoke(['cargo', 'test', '--locked', '--test', 'compiler_cljc_prefixes'],
                        directory / 'controls-local', timeout=240)
    if ran['status'] != 'completed' or 'test result: ok. 5 passed; 0 failed;' not in ran['stdout']:
        raise ValueError('actual focused controls failed')
    full = audit.execute(directory / 'originals')
    identities = [row['id'] for row in audit.frozen()['focused_gap_contracts']]
    focused = [row for row in full['reconciliation'] if row['id'] in identities]
    if len(focused) != 3 or not all(row['complete_eight_local_phase_frames_and_both_pins'] for row in focused):
        raise ValueError('an unchanged original failed a required phase/engine/pin')
    rows = control_rows()
    references = {}
    for kind in ('release', 'checkout'):
        references[kind] = audit.reference(rows, kind, directory / ('controls-' + kind))
        if not all(row['status'] == 'passed' for row in references[kind].values()):
            raise ValueError('canonical control mismatch: ' + kind)
        print(kind + ' 23 canonical controls passed', flush=True)
    if snapshot() != before:
        raise ValueError('source/producer drift during current proof')
    for item in executables.values():
        if sci.digest(Path(item['path']).read_bytes()) != item['sha256']:
            raise ValueError('execution binary changed during proof')
    # Retain bindings to raw source, emitted code, process commands and outcomes.
    # This receipt is scoped evidence; it does not replay or credit a shared ledger.
    raw = {str(path.relative_to(directory)): sci.digest(path.read_bytes())
           for path in sorted(directory.rglob('*')) if path.is_file()}
    receipt = {'schema_version': 1, 'artifact_root': str(directory),
               'producer_sources_sha256': before, 'executables': executables,
               'raw_artifacts_sha256': raw, 'originals': {
                   'catalog': [row for row in full['catalog'] if row['id'] in identities],
                   'local': {identity: full['local'][identity] for identity in identities},
                   'references': {kind: {identity: full['references'][kind][identity] for identity in identities}
                                  for kind in ('release', 'checkout')},
                   'reconciliation': focused},
               'controls': {'catalog': rows, 'references': references},
               'original_semantic_pass_credit': 0,
               'shared_ledger_integration': 'not performed; fail-closed raw replayer still required'}
    (directory / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    return receipt


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    execute(parser.parse_args().output)
