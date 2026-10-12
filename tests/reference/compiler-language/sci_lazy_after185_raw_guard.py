"""Strict supplementary finite packets and raw mutations of existing lazy21.

Usage: python3 -B sci_lazy_after185_raw_guard.py CURRENT_LAZY_RECEIPT OUTPUT
The current receipt is restored byte-for-byte after every guarded mutation.
"""
import json,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
import sci_finite_after185_context as finite
import sci_lazy_context as lazy
def source_snapshot():
    return {**finite.snapshot(),str(Path(__file__).resolve().relative_to(finite.sci.ROOT)):finite.sci.digest(Path(__file__).read_bytes())}
before=source_snapshot()
receipt=Path(sys.argv[1]).resolve();document=json.loads(receipt.read_text())
receipt_sha256=finite.sci.digest(receipt.read_bytes())
result=finite.guard_existing_lazy(document)
phase=Path(document['phase_artifacts_root']);reference=Path(document['reference_artifacts_root'])/'evidence.json'
raw_path=phase/'observations.tsv';binding_path=phase/'execution-binding.json'
raw_bytes=raw_path.read_bytes();binding_bytes=binding_path.read_bytes();row=finite.load()[0]
checks=[]
try:
    for mutation in ('missing-frame','changed-original-source','coherent-emitted-unit','changed-raw-packet'):
        records=[line.split('\t') for line in raw_bytes.decode().splitlines()]
        selected=next(i for i,r in enumerate(records) if r[0]==row['id'])
        if mutation=='missing-frame':records.pop()
        elif mutation=='changed-original-source':records[selected][3]='42'.encode().hex()
        elif mutation=='coherent-emitted-unit':
            unit=bytes.fromhex(records[selected][4]).decode();changed=unit.replace('return','return /* supplementary replay guard */',1)
            if unit==changed:raise AssertionError('Missing emitted unit mutation target')
            records[selected][4]=changed.encode().hex();records[selected+1][4]=records[selected][4]
        else:
            packet=json.loads(bytes.fromhex(records[selected][5]).decode());packet['outcome']={'status':'returned','value':None};records[selected][5]=json.dumps(packet).encode().hex()
        raw_path.write_text('\n'.join('\t'.join(r) for r in records)+'\n')
        binding=json.loads(binding_bytes);binding['artifact_sha256']['observations.tsv']=finite.sci.digest(raw_path.read_bytes());binding_path.write_text(json.dumps(binding))
        try:
            rehashed=finite.proof.verified_document(reference,phase,cohort=lazy)
            finite.guard_existing_lazy(rehashed)
        except ValueError as error:
            reason=str(error)
            if mutation=='coherent-emitted-unit' and 'Finite emitted unit/raw observation differs from executable replay' not in reason:raise AssertionError('Coherent mutation did not reach executable replay: '+reason)
            checks.append({'mutation':mutation,'status':'rejected','reason':reason})
        else:raise AssertionError('Accepted mutated '+mutation)
        finally:raw_path.write_bytes(raw_bytes);binding_path.write_bytes(binding_bytes)
finally:raw_path.write_bytes(raw_bytes);binding_path.write_bytes(binding_bytes)
finite.guard_existing_lazy(document)
finite.proof.same(before,source_snapshot(),'Supplementary lazy guard source changed during execution')
finite.proof.same(receipt_sha256,finite.sci.digest(receipt.read_bytes()),'Current lazy receipt changed during supplementary guard')
output={'profile':'existing-lazy21-finite7-strict-replay-after185-v1','status':'verified','original_semantic_pass_credit':0,'new_ledger_rows':0,'lazy_receipt':str(receipt),'lazy_receipt_sha256':receipt_sha256,'phase_artifacts_root':str(phase),'reference_artifacts_root':str(reference.parent),'source_sha256':before,'result':result,'raw_mutation_checks':checks,'pristine_restored_and_replayed':True}
Path(sys.argv[2]).write_text(json.dumps(output,indent=2)+'\n')
print(json.dumps({'status':'verified','checked_finite_originals':len(result['checked_original_ids']),'existing_lazy_sources':len(document['source_bindings']),'existing_lazy_frames':sum(len(v) for v in document['frames'].values()),'raw_mutations_rejected':len(checks),'new_ledger_rows':0}))
