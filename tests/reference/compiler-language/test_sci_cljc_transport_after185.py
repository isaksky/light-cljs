"""Original source/typed-value and actual raw executable transport guards."""
import copy,json,os
from pathlib import Path
import unittest
from unittest.mock import patch
import sci_cljc_transport_after185 as cohort

class OriginalTransportGuards(unittest.TestCase):
    def test_exact_originals_execute_before_transport_and_retain_zero_credit(self):
        self.assertEqual(3,len(cohort.load()))
        for original,row in zip(cohort.sources(),cohort.load()):
            self.assertEqual(original['source'],row['source']);self.assertEqual(original['original_expected'],row['original_expected'])
            self.assertEqual(0,row['original_semantic_pass_credit']);self.assertNotEqual(row['original_expected'],row['canonical_expected'])
            for phase in ('application','macro'):
                source=cohort.module(row,phase);self.assertEqual(1,source.count('(do '+row['source']+')'))
                self.assertLess(source.index('(do '+row['source']+')'),source.index('(encode original-value)'))
        self.assertEqual({'$vector':[1,{'$value':'undefined'}]},cohort.load()[0]['canonical_expected']['outcome']['value'])
        self.assertEqual(2,cohort.load()[2]['canonical_expected']['outcome']['value']['count'])

    def test_unreviewed_literal_and_changed_original_binding_reject(self):
        rows=json.loads(cohort.CONTRACTS.read_text());rows['source_bindings'][0]['source']='42'
        with patch.object(Path,'read_bytes',return_value=json.dumps(rows).encode()),self.assertRaisesRegex(ValueError,'Unreviewed complete transport literals'):cohort.load()

    def test_encoder_contract_driver_and_oracle_are_snapshotted(self):
        snapshot=cohort.snapshot()
        for path in (cohort.DRIVER,cohort.ORACLE,cohort.ENCODER,cohort.CONTRACTS,Path(cohort.__file__),Path(__file__)):
            self.assertEqual(cohort.sci.digest(path.read_bytes()),snapshot[str(path.resolve().relative_to(cohort.ROOT))])

    @unittest.skipUnless(os.environ.get('LIGHT_CLJC_TRANSPORT_RECEIPT'),'Explicit fresh original transport raw guard fixture')
    def test_rehashed_raw_unit_pin_unit_and_missing_engine_fail_then_restore(self):
        document=json.loads(Path(os.environ['LIGHT_CLJC_TRANSPORT_RECEIPT']).read_text());root=Path(document['artifacts_root']);cohort.reconcile(document)
        binding_path=root/'binding.json';binding_bytes=binding_path.read_bytes();phase=root/'phases';row=cohort.load()[0];folder=phase/row['id']/'application-fresh'
        cases=[(phase/'observations.tsv',lambda raw:'\n'.join(raw.decode().splitlines()[:-1])+'\n'),
               (folder/'compiled.js',lambda raw:raw.decode().replace('return','return /* raw guard */',1)),
               (root/'release/original-sources'/row['id']/'compiled.js',lambda raw:raw.decode().replace('return','return /* pin raw guard */',1)),
               (folder/'quickjs.raw',lambda raw:json.dumps({**json.loads(raw),'outcome':{'status':'returned','value':None}}))]
        for path,mutate in cases:
            pristine=path.read_bytes();backups={path:pristine}
            try:
                path.write_text(mutate(pristine))
                # Keep every declaration of the changed unit consistent, so the
                # current compiler replay must detect the harmless comment.
                if path==folder/'compiled.js':
                    records_path=phase/'observations.tsv';backups[records_path]=records_path.read_bytes()
                    records=[line.split('\t') for line in records_path.read_text().splitlines()]
                    for fields in records:
                        if fields[:2]==[row['id'],'application-fresh']:fields[4]=path.read_bytes().hex()
                    records_path.write_text('\n'.join('\t'.join(f) for f in records)+'\n')
                    script='(() => {const run='+path.read_text()+';return run();})()'
                    for name,value in (('script.js',script),('node.stdin','console.log('+script+');')):
                        altered=folder/name;backups[altered]=altered.read_bytes();altered.write_text(value)
                if path==root/'release/original-sources'/row['id']/'compiled.js':
                    contexts=root/'release/contexts.js';backups[contexts]=contexts.read_bytes();contexts.write_text(contexts.read_text().replace(pristine.decode(),path.read_text(),1))
                    for index,current in enumerate(cohort.load()):
                        command=root/'release/node'/current['id']/'command.json';backups[command]=command.read_bytes();record=json.loads(command.read_text());record['stdin']=cohort.proof.node_script(root/'release',index);command.write_text(json.dumps(record))
                binding=json.loads(binding_bytes);binding['artifact_sha256']=cohort.proof.tree(root,('binding.json','evidence.json'));binding_path.write_text(json.dumps(binding))
                with self.subTest(path=str(path)),self.assertRaises(ValueError):cohort.verified_document(root)
            finally:
                for original,raw in backups.items():original.write_bytes(raw)
                binding_path.write_bytes(binding_bytes)
        cohort.reconcile(document)

if __name__=='__main__':unittest.main()
