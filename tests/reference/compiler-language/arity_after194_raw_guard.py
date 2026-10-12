#!/usr/bin/env python3
"""Mutate actual retained evidence, reject each tamper, restore pristine replay."""
from pathlib import Path
import argparse,json,tempfile,shutil
import arity_after194 as cohort
import arity_after194_proof as proof


def guard(receipt_path,output):
    receipt_path=Path(receipt_path).resolve();receipt=json.loads(receipt_path.read_text())
    before=receipt['frozen_source_snapshot']
    if before!=proof.frozen_snapshot():raise ValueError('raw controls require current pristine producer/production snapshot')
    source_actual=Path(receipt['actual_log_path']);source_modules=Path(receipt['modules_path']);source_references=Path(receipt['references_path'])
    pristine=proof.validate(source_actual,source_references,source_modules)
    for field in ['exports','installed_loader_state','actual_frames','actual_input_hash_frames','actual_log_sha256','actual_executable_sha256','module_source_emitted_js_and_raw_sha256','references']:
        if pristine[field]!=receipt[field]:raise ValueError('saved receipt differs from current raw proof: '+field)
    with tempfile.TemporaryDirectory(prefix='light-arity-after194-guards-') as temp:
        # Absolute canonical build/Node paths are independently checked, so copy
        # only mutable actual modules/log. Reference artifacts are restored in place.
        root=Path(temp);actual=root/'actual.log';shutil.copyfile(source_actual,actual)
        modules=root/'modules';shutil.copytree(source_modules,modules)
        controls=[]
        first=cohort.rows()[0];base=modules/first['id']/'fresh'/'application'
        def check(name,path,mutate):
            original=path.read_bytes()
            try:
                path.write_bytes(mutate(original))
                try:proof.validate(actual,source_references,modules)
                except (ValueError,KeyError,FileNotFoundError,json.JSONDecodeError) as e:controls.append({'id':name,'status':'rejected','reason':str(e)})
                else:raise AssertionError('raw mutation accepted: '+name)
            finally:path.write_bytes(original)
            proof.validate(actual,source_references,modules)
        check('missing-engine-frame',actual,lambda raw:b'\n'.join(line for line in raw.split(b'\n') if not line.startswith(b'arity-after194-observation\t'+first['id'].encode()+b'\tfresh\tapplication\tQuickJS')))
        check('changed-typed-result',actual,lambda raw:raw.replace(b'"value":true',b'"value":false',1))
        check('changed-actual-node-input-hash',actual,lambda raw:raw.replace(b'arity-after194-artifacts\t'+first['id'].encode(),b'arity-after194-artifacts\twrong-source',1))
        for name,relative in [('changed-whole-source','user.cljs'),('changed-emitted-js','compiled.js'),('changed-engine-observer-input','script.js'),('changed-node-stdin','node.stdin'),('changed-quickjs-indirect-eval-input','quickjs.stdin'),('changed-quickjs-raw','quickjs.raw'),('changed-node-raw','node.stdout')]:
            check(name,base/relative,lambda raw:raw+b' ')
        for kind in ['release','checkout']:
            path=source_references/kind
            check('changed-'+kind+'-raw-result',path/'node/stdout',lambda raw:raw.replace(b'"value":true',b'"value":false',1))
            check('changed-'+kind+'-identity',path/'build/stdout',lambda raw:raw.replace(b'REFERENCE IDENTITY',b'WRONG IDENTITY',1))
            check('changed-'+kind+'-original-source',path/'source/arityreference/case0.cljs',lambda raw:raw+b' ')
        victim=base/'compiled.js';original=victim.read_bytes();victim.unlink()
        try:
            try:proof.validate(actual,source_references,modules)
            except (ValueError,FileNotFoundError) as e:controls.append({'id':'missing-actual-artifact','status':'rejected','reason':str(e)})
            else:raise AssertionError('missing actual artifact accepted')
        finally:victim.write_bytes(original)
        restored=proof.validate(source_actual,source_references,source_modules)
        if restored!=pristine:raise ValueError('pristine restored replay changed')
    result={'receipt_path':str(receipt_path),'receipt_sha256':cohort.digest(receipt_path.read_bytes()),'controls':controls,'restored_pristine_replay':'passed','source_site_credit_added':0,'strict_complete':False}
    Path(output).write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--receipt',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();guard(a.receipt,a.output)
