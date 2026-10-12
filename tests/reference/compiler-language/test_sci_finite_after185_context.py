"""Original binding and raw executable mutation guards for the finite cohort."""
import copy,json,os
from pathlib import Path
import unittest
from unittest.mock import patch
import sci_finite_after185_context as cohort

class FiniteOriginalGuards(unittest.TestCase):
    def test_entire_originals_and_literal_contracts_stay_distinct(self):
        self.assertEqual(14,len(cohort.candidates.load()));self.assertEqual(7,len(cohort.load()))
        for row in cohort.load():
            self.assertNotEqual(row['original_expected'],row['canonical_expected'])
            self.assertEqual(0,row['original_semantic_pass_credit'])
            for phase in ('application','macro'):self.assertIn(row['source'],cohort.proof.module(row,phase))

    def test_changed_original_expectation_and_source_reject(self):
        rows=json.loads(cohort.candidates.SOURCES.read_text())
        for key,value in (('source','42'),('original_expected',{}),('source_sha256','0'*64)):
            changed=copy.deepcopy(rows);changed[0][key]=value
            original=Path.read_text
            def read(path,*args,**kwargs):return json.dumps(changed) if path==cohort.candidates.SOURCES else original(path,*args,**kwargs)
            with patch.object(Path,'read_text',read),self.assertRaisesRegex(ValueError,'complete source/profile/expectation drift'):cohort.candidates.load()

    def test_complete_typed_contract_cannot_project_sequence_brands(self):
        row=cohort.load()[0];altered=copy.deepcopy(row['canonical_expected'])
        altered['outcome']['value']={'$seq':altered['outcome']['value']['$vector']}
        with self.assertRaises(ValueError):cohort.proof.same(altered,row['canonical_expected'],'Exact finite typed contract')

    def test_executing_helpers_driver_and_contract_are_snapshotted(self):
        snapshot=cohort.snapshot()
        for name in ('sci_finite_after185_candidates.py','sci_finite_after185_context.py','test_sci_finite_after185_context.py','sci-finite-after185-contracts.jsonl','sci-finite-after185-sources.json','sci_gap_canonical_context.py'):
            path=cohort.sci.HERE/name;self.assertEqual(cohort.sci.digest(path.read_bytes()),snapshot[str(path.relative_to(cohort.sci.ROOT))])

    @unittest.skipUnless(os.environ.get('LIGHT_FINITE_AFTER185_RECEIPT'),'Explicit fresh executable raw guard fixture')
    def test_rehashed_raw_and_emitted_mutations_reject_then_pristine_replays(self):
        receipt=Path(os.environ['LIGHT_FINITE_AFTER185_RECEIPT']);document=json.loads(receipt.read_text());root=Path(document['phase_artifacts_root'])
        cohort.reconcile(document)
        binding_path=root/'execution-binding.json';raw_path=root/'observations.tsv';binding_bytes=binding_path.read_bytes();raw_bytes=raw_path.read_bytes()
        try:
            for column in (3,4,5):
                records=[line.split('\t') for line in raw_bytes.decode().splitlines()]
                if column==3:records[0][column]="(fn* [observe] 42)".encode().hex()
                elif column==4:
                    records[0][column]=(bytes.fromhex(records[0][column]).decode().replace('return','return /* raw mutation */',1)).encode().hex()
                    records[1][column]=records[0][column]
                else:
                    packet=json.loads(bytes.fromhex(records[0][column]).decode());packet['outcome']={'status':'returned','value':None};records[0][column]=json.dumps(packet).encode().hex()
                raw_path.write_text('\n'.join('\t'.join(r) for r in records)+'\n')
                binding=json.loads(binding_bytes);binding['artifact_sha256']['observations.tsv']=cohort.sci.digest(raw_path.read_bytes());binding_path.write_text(json.dumps(binding))
                with self.assertRaises(ValueError):cohort.verified_document(Path(document['reference_artifacts_root'])/'evidence.json',root)
                raw_path.write_bytes(raw_bytes);binding_path.write_bytes(binding_bytes)
        finally:raw_path.write_bytes(raw_bytes);binding_path.write_bytes(binding_bytes)
        cohort.reconcile(document)

if __name__=='__main__':unittest.main()
