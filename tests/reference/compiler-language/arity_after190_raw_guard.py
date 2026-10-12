#!/usr/bin/env python3
"""Restore raw artifact mutations and require the current complete proof to reject each."""
import argparse
import json
from pathlib import Path
import shutil
import tempfile
import arity_after190 as cohort
import arity_after190_proof as proof


def check(receipt, output):
    path=Path(receipt).resolve()
    original=json.loads(path.read_text())
    root=path.parent
    references=Path(original['references']['release']['receipt_path']).parent.parent
    actual=root/'actual.log'
    guards=root/'guards.log'
    with tempfile.TemporaryDirectory(prefix='light-arity-after190-raw-guard-') as temporary:
        replay=Path(temporary)
        shutil.copytree(root/'modules',replay/'modules')
        proof.produce(replay,actual,guards,references,original.get('frozen_before_snapshot_path'))
        row=cohort.rows()[0]
        module=replay/'modules'/row['id']/'fresh'/'application'
        source=module/(row['namespace'].replace('.','/').replace('-','_')+'.cljs')
        tests=[
            ('actual-frame',actual,lambda data:data.replace(b'\tQuickJS\ttrue',b'\tQuickJS\tfalse',1)),
            ('actual-input-hash',actual,lambda data:data.replace(b'arity-after190-artifacts\t',b'invalid-artifacts\t',1)),
            ('module-source',source,lambda data:data+b'\ntrue\n'),
            ('emitted-input',module/'compiled.js',lambda data:data+b'\n// altered emitted input\n'),
            ('release-stdout',references/'release/node/stdout',lambda data:data.replace(b'"value":true',b'"value":false',1)),
            ('checkout-source',next((references/'checkout/source').rglob('*.cljs')),lambda data:data+b'\ntrue\n'),
            ('release-build',references/'release/build.clj',lambda data:data+b'\n;; changed build\n'),
            ('checkout-identity',references/'checkout/build/stdout',lambda data:data.replace(b'REFERENCE IDENTITY',b'CHANGED IDENTITY',1)),
            ('release-emitted',references/'release/reference.js',lambda data:data+b'\n// changed reference\n'),
            ('namespace-companion',next(replay.glob('modules/cljs-namespace-completion-require-alias/fresh/application/clojure/set.cljs')),lambda data:data+b'\ntrue\n'),
        ]
        observations=[]
        for name,target,mutate in tests:
            saved=target.read_bytes()
            changed=mutate(saved)
            if saved==changed:raise ValueError('mutation failed to change exact artifact: '+name)
            try:
                target.write_bytes(changed)
                try:proof.produce(replay,actual,guards,references,original.get('frozen_before_snapshot_path'))
                except (ValueError,FileNotFoundError) as error:
                    observations.append({'mutation':name,'result':'rejected','reason':str(error)})
                else:raise ValueError('raw mutation incorrectly accepted: '+name)
            finally:target.write_bytes(saved)
        saved=(module/'compiled.js').read_bytes()
        try:
            (module/'compiled.js').unlink()
            try:proof.produce(replay,actual,guards,references,original.get('frozen_before_snapshot_path'))
            except (ValueError,FileNotFoundError) as error:
                observations.append({'mutation':'missing-emitted-input','result':'rejected','reason':str(error)})
            else:raise ValueError('missing emitted input incorrectly accepted')
        finally:(module/'compiled.js').write_bytes(saved)
        proof.produce(replay,actual,guards,references,original.get('frozen_before_snapshot_path'))
    result={'receipt_path':str(path),'receipt_sha256':cohort.digest(path.read_bytes()),'mutations':observations,'restored_pristine_replay':'passed','strict_complete':False}
    Path(output).write_text(json.dumps(result,indent=2)+'\n')
    print('eleven raw/source/emitted/frame/pin/build/companion/missing-artifact mutations rejected; pristine replay passed')
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipt',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();check(args.receipt,args.output)
