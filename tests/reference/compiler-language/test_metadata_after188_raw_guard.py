#!/usr/bin/env python3
"""Reject restored raw source/pin/phase/recovery mutations for the :test gap."""
from pathlib import Path
import argparse
import json
import tempfile
import test_metadata_after188 as proof


def run(receipt, output):
    original = json.loads(Path(receipt).read_text())
    actual = Path(original['actual_log_path'])
    modules = Path(original['actual_command'].split('ARTIFACTS=',1)[1].split(' cargo test',1)[0])
    references = Path(original['references']['release']['receipt_path']).parent.parent
    with tempfile.TemporaryDirectory(prefix='light-test-metadata-after188-raw-') as tmp:
        target = Path(tmp)/'receipt.json'
        replay = lambda: proof.produce(actual,modules,references,target)
        if replay() != original:
            raise ValueError('pristine current diagnostic receipt differs from raw proof')
        mutations = [
            ('original-source',proof.ROOT/proof.SOURCE,lambda x:x.replace(b'(test cljs-3411-function)',b':ok',1)),
            ('application-frame',actual,lambda x:x.replace(b'\tQuickJS\ttrue',b'\tQuickJS\tfalse',1)),
            ('same-host-recovery-frame',actual,lambda x:x.replace(b'\tQuickJS\t42',b'\tQuickJS\t43',1)),
            ('original-macro-diagnostic',modules/'fresh/macro/diagnostic.txt',lambda x:x.replace(b'Assert failed',b'Unrelated failure',1)),
            ('original-emitted-input',modules/'fresh/application/compiled.js',lambda x:x+b'\n// altered input\n'),
            ('canonical-release-observation',references/'release/node/stdout',lambda x:x.replace(b'"value":true',b'"value":false',1)),
            ('canonical-checkout-full-source',references/'checkout/source/cljs/var_test.cljs',lambda x:x.replace(b'(test cljs-3411-function)',b':ok',1)),
            ('original-macro-source',modules/'reused/macro/cljs/var_test.cljs',lambda x:x.replace(b'(run nil)',b'true',1)),
            ('same-host-recovery-emitted-input',modules/'reused/macro/recovery/compiled.js',lambda x:x+b'\n// altered recovery\n'),
        ]
        rejected = []
        for name,path,mutate in mutations:
            saved = path.read_bytes()
            changed = mutate(saved)
            if changed == saved:
                raise ValueError('raw mutation did not change artifact: '+name)
            try:
                path.write_bytes(changed)
                try: replay()
                except (ValueError,FileNotFoundError) as e:
                    rejected.append({'mutation':name,'status':'rejected','reason':str(e)})
                else: raise ValueError('mutated diagnostic proof accepted: '+name)
            finally: path.write_bytes(saved)
        path = modules/'reused/macro/diagnostic.txt'
        saved = path.read_bytes()
        try:
            path.unlink()
            try: replay()
            except (ValueError,FileNotFoundError) as e:
                rejected.append({'mutation':'missing-original-diagnostic','status':'rejected','reason':str(e)})
            else: raise ValueError('missing actual diagnostic was accepted')
        finally: path.write_bytes(saved)
        if replay() != original:
            raise ValueError('restored current raw proof differs from original receipt')
    value = {'receipt_path':str(Path(receipt).resolve()),'receipt_sha256':proof.cohort.digest(Path(receipt).read_bytes()),
        'raw_guard_source_sha256':proof.cohort.digest(Path(__file__).read_bytes()),
        'rejected_mutations':rejected,'restored_pristine_proof':'passed','operation_credit_added':0,'strict_complete':False}
    Path(output).write_text(json.dumps(value,indent=2)+'\n')
    print('Ten source/phase/diagnostic/recovery/emitted/pin/missing-artifact mutations rejected; restored exact proof passed.')
    return value


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--receipt',required=True,type=Path)
    p.add_argument('--output',required=True,type=Path)
    a=p.parse_args()
    run(a.receipt,a.output)
