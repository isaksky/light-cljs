#!/usr/bin/env python3
"""Validate unchanged metadata callbacks against both real pins and actual phases."""
from pathlib import Path
from unittest.mock import patch
import argparse
import json
import test_metadata_after188 as historical

ROOT = historical.ROOT
DRIVER = 'tests/compiler_test_metadata.rs'
TEST = 'metadata_test_original_source_passes_both_phases_and_recovers'
SELF = 'tests/reference/compiler-language/test_metadata.py'
BASE_SNAPSHOT = historical.snapshot


def snapshot():
    value = BASE_SNAPSHOT()
    for name in [DRIVER, SELF]:
        value[name] = historical.cohort.digest((ROOT/name).read_bytes())
    return value


def pins(directory):
    with patch.object(historical, 'snapshot', snapshot):
        historical.pins(directory)


def produce(actual, modules, references, output=None):
    row = historical.load()
    raw = Path(actual).read_text()
    root = Path(modules).resolve()
    if 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in raw or ('test '+TEST+' ... ok') not in raw:
        raise ValueError('exact complete original callback test did not execute successfully')
    records = [json.loads(line) for line in raw.splitlines() if line.startswith('{')]
    binaries = [r['executable'] for r in records if r.get('reason') == 'compiler-artifact'
                and r.get('target', {}).get('name') == 'compiler_test_metadata' and r.get('executable')]
    if len(binaries) != 1:
        raise ValueError('missing actual metadata acceptance executable')
    executed = [line.split("\t")[1:] for line in raw.splitlines() if line.startswith("test-metadata-executable\t")]
    if len(executed) != 1 or executed[0][0] != binaries[0] or Path(executed[0][1]).resolve() != root/"acceptance-test-executable":
        raise ValueError("actual executed binary snapshot binding drift")
    if historical.cohort.digest(Path(executed[0][1]).read_bytes()) != executed[0][2]:
        raise ValueError("actual executed binary snapshot content drift")
    frames, recoveries, packets, recovery_packets = [], [], [], []
    for line in raw.splitlines():
        fields = line.split('\t')
        for prefix, target in [('test-metadata-observation', frames), ('test-metadata-recovery', recoveries),
                               ('test-metadata-artifacts', packets), ('test-metadata-recovery-artifacts', recovery_packets)]:
            if fields[0] == prefix:
                target.append(fields[1:])
    contexts = [(host, phase) for host in ['fresh', 'reused'] for phase in ['application', 'macro']]
    if frames != [[h, p, e, 'true'] for h, p in contexts for e in ['QuickJS', 'Node']]:
        raise ValueError('complete original actual phase/engine observations drift')
    if recoveries != [[h, p, e, '42'] for h, p in contexts for e in ['QuickJS', 'Node']]:
        raise ValueError('same-host recovery observations drift')
    if [p[:2] for p in packets] != [list(c) for c in contexts] or [p[:2] for p in recovery_packets] != [list(c) for c in contexts]:
        raise ValueError('missing executed source/emitted input packets')
    tree = {str(p.relative_to(root)):historical.cohort.digest(p.read_bytes())
            for p in sorted(root.rglob('*')) if p.is_file()}
    expected = {'warmup/warm.cljs', 'warmup.js', 'acceptance-test-executable'}
    if (root/'warmup/warm.cljs').read_text() != '(ns warm) (defmacro initialized [] :warm) (initialized)':
        raise ValueError('actual reused-host warmup drift')
    for host, phase in contexts:
        prefix = host+'/'+phase+'/'
        program = row['source']+ ('\n(defmacro metadata-witness [] (run nil))\n(pr-str (metadata-witness))\n' if phase == 'macro' else '\n(pr-str (run nil))\n')
        if (root/prefix/'cljs/var_test.cljs').read_text() != program:
            raise ValueError('unchanged complete source/declaration/assertions/callback/phase drift')
        if (root/prefix/'recovery/user.cljs').read_text() != '(ns user) (defmacro recovered-metadata [] (+ 40 2)) (pr-str (recovered-metadata))':
            raise ValueError('identical-host demanded macro recovery source drift')
        expected |= {prefix+'cljs/var_test.cljs', prefix+'compiled.js', prefix+'recovery/user.cljs', prefix+'recovery/compiled.js'}
    if set(tree) != expected:
        raise ValueError('complete source/module artifact census drift')
    for host, phase, emitted, source in packets:
        if tree[host+'/'+phase+'/compiled.js'] != emitted or tree[host+'/'+phase+'/cljs/var_test.cljs'] != source:
            raise ValueError('actual original executed source/emitted binding drift')
    for host, phase, emitted, source in recovery_packets:
        if tree[host+'/'+phase+'/recovery/compiled.js'] != emitted or tree[host+'/'+phase+'/recovery/user.cljs'] != source:
            raise ValueError('actual recovery executed source/emitted binding drift')
    with patch.object(historical, 'snapshot', snapshot):
        refs = historical.verify_references(references)
    command = 'LIGHT_TEST_METADATA_ARTIFACTS='+str(root)+' cargo test --locked --message-format=json --test compiler_test_metadata '+TEST+' -- --exact --nocapture'
    starting_head = json.loads((ROOT/'tests/reference/compiler-language/test-metadata-before189-receipt.json').read_text())['head']
    value = {'implementation_starting_head':starting_head,
             'source_contract':row, 'producer_source_sha256':snapshot(), 'references':refs,
             'actual_command':command, 'actual_log_path':str(Path(actual).resolve()),
             'actual_log_sha256':historical.cohort.digest(raw), 'actual_executable':binaries[0],
             'actual_executable_sha256':executed[0][2], 'actual_executable_snapshot':executed[0][1],
             'module_artifact_sha256':tree, 'phase_engine_observations':frames,
             'same_host_recovery_observations':recoveries, 'actual_executed_input_hash_packets':packets+recovery_packets,
             'historical_receipt_sha256':{name:historical.cohort.digest((ROOT/'tests/reference/compiler-language'/name).read_bytes())
                                        for name in ['test-metadata-after188-receipt.json', 'test-metadata-after188-raw-guard-receipt.json',
                                                     'test-metadata-before189-receipt.json']},
             'original_expectations_changed':False, 'operation_credit_added':0, 'source_site_credit_added':0,
             'strict_complete':False}
    if output:
        Path(output).write_text(json.dumps(value, indent=2)+'\n')
        print('Both pins passed unchanged original; 8 actual phase/engine observations and 8 identical-host recoveries; receipt', historical.cohort.digest(Path(output).read_bytes()))
    return value


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-pins', type=Path)
    parser.add_argument('--actual', type=Path)
    parser.add_argument('--modules', type=Path)
    parser.add_argument('--references', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.run_pins:
        pins(args.run_pins)
    else:
        produce(args.actual, args.modules, args.references, args.output)
