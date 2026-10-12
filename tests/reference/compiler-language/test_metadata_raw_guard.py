#!/usr/bin/env python3
"""Reject restored raw original-source, actual phase and canonical pin mutations."""
from pathlib import Path
import argparse
import json
import test_metadata as proof


def run(receipt, output):
    original = json.loads(Path(receipt).read_text())
    actual = Path(original['actual_log_path'])
    modules = Path(original['actual_command'].split('ARTIFACTS=', 1)[1].split(' cargo test', 1)[0])
    references = Path(original['references']['release']['receipt_path']).parent.parent
    def replay():
        candidate = proof.produce(actual, modules, references)
        if candidate != original:
            raise ValueError('raw replay differs from saved exact acceptance receipt')
    replay()
    mutations = [
        ('original-callback-source', proof.ROOT/proof.historical.SOURCE, lambda x:x.replace(b'(test cljs-3411-function)', b':ok', 1)),
        ('actual-phase-observation', actual, lambda x:x.replace(b'\tQuickJS\ttrue', b'\tQuickJS\tfalse', 1)),
        ('identical-host-recovery', actual, lambda x:x.replace(b'\tQuickJS\t42', b'\tQuickJS\t43', 1)),
        ('actual-macro-emitted-input', modules/'fresh/macro/compiled.js', lambda x:x+b'\n// altered input\n'),
        ('actual-macro-whole-source', modules/'reused/macro/cljs/var_test.cljs', lambda x:x.replace(b'(run nil)', b'true', 1)),
        ('canonical-release-output', references/'release/node/stdout', lambda x:x.replace(b'"value":true', b'"value":false', 1)),
        ('canonical-checkout-whole-source', references/'checkout/source/cljs/var_test.cljs', lambda x:x.replace(b'(test cljs-3411-function)', b':ok', 1)),
        ('actual-test-executable', Path(original['actual_executable_snapshot']), lambda x:x+b'altered-executable'),
        ('recovery-emitted-input', modules/'reused/macro/recovery/compiled.js', lambda x:x+b'\n// altered recovery\n'),
    ]
    rejected = []
    for name, path, mutate in mutations:
        saved = path.read_bytes()
        changed = mutate(saved)
        if changed == saved:
            raise ValueError('mutation did not change artifact: '+name)
        try:
            path.write_bytes(changed)
            try:
                replay()
            except (ValueError, FileNotFoundError) as error:
                rejected.append({'mutation':name, 'status':'rejected', 'reason':str(error)})
            else:
                raise ValueError('mutated raw proof accepted: '+name)
        finally:
            path.write_bytes(saved)
    path = modules/'reused/macro/compiled.js'
    saved = path.read_bytes()
    try:
        path.unlink()
        try:
            replay()
        except (ValueError, FileNotFoundError) as error:
            rejected.append({'mutation':'missing-actual-macro-emission', 'status':'rejected', 'reason':str(error)})
        else:
            raise ValueError('missing actual macro emission accepted')
    finally:
        path.write_bytes(saved)
    replay()
    value = {'receipt_path':str(Path(receipt).resolve()),
             'receipt_sha256':proof.historical.cohort.digest(Path(receipt).read_bytes()),
             'raw_guard_source_sha256':proof.historical.cohort.digest(Path(__file__).read_bytes()),
             'rejected_mutations':rejected, 'restored_pristine_proof':'passed',
             'operation_credit_added':0, 'strict_complete':False}
    Path(output).write_text(json.dumps(value, indent=2)+'\n')
    print('Ten raw/source/phase/emitted/executable/pin/missing-artifact mutations rejected; pristine restoration passed.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipt', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    run(args.receipt, args.output)
