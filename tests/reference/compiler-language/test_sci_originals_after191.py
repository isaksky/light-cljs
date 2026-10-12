"""Exact original SCI fields and fresh whole-source executable replay guards."""
from pathlib import Path
import copy,json,os,unittest
from unittest.mock import patch
import sci_originals_after191 as cohort

class OriginalSCIGuards(unittest.TestCase):
    def test_original_fields_and_complete_sources_are_unchanged(self):
        self.assertEqual(31,len(cohort.load()))
        for row in cohort.load():
            self.assertEqual(row['source'],row['original_fields'][2])
            self.assertEqual(cohort.candidates.original_expected(row['original_fields']),row['original_expected'])
            self.assertEqual('returned' if row['sci_id'] in cohort.candidates.IDS[:10] else 'sci-error',row['original_expected']['outcome']['status'])
            if row['sci_id'] not in cohort.candidates.IDS[:10]:
                raw=cohort.sci.wire(row['original_fields'][3])
                self.assertEqual(raw[1],row['original_expected']['outcome']['phase'])
                self.assertEqual(raw[2],row['original_expected']['outcome']['category'])
            self.assertNotEqual(row['original_expected'],row['canonical_expected'])
            self.assertEqual('threw' if row['sci_id'] in cohort.CANONICAL_ERRORS else 'returned',row['canonical_expected']['outcome']['status'])
            self.assertEqual(0,row['original_semantic_pass_credit'])
            for phase in ('application','macro'):
                self.assertEqual(1,cohort.proof.module(row,phase).count(row['source']))
        self.assertEqual([False,None,None],cohort.load()[0]['canonical_expected']['outcome']['value']['$vector'])
        self.assertEqual([False,7,None],cohort.load()[1]['canonical_expected']['outcome']['value']['$vector'])
        self.assertEqual(['meta','init','meta','next'],[v['$keyword']['spelling'] for v in cohort.load()[2]['original_expected']['trace']])
        self.assertEqual(['init'],[v['$keyword']['spelling'] for v in cohort.load()[2]['canonical_expected']['trace']])
        self.assertEqual([1,2],cohort.load()[6]['canonical_expected']['trace'])
        self.assertEqual({'status':'threw','error':{'category':'error'}},cohort.load()[9]['canonical_expected']['outcome'])
        self.assertEqual([],cohort.load()[9]['canonical_expected']['trace'])
        vector=next(r for r in cohort.load() if r['sci_id']=='sci-coll-call-vector-arity')
        self.assertEqual([0,2],vector['original_expected']['trace'])
        self.assertEqual([0,2],vector['canonical_expected']['trace'])
        self.assertEqual('arity',vector['original_expected']['outcome']['category'])
        merge=next(r for r in cohort.load() if r['sci_id']=='sci-coll-merge-with-invalid')
        self.assertEqual('type',merge['original_expected']['outcome']['category'])
        self.assertEqual('type-error',merge['canonical_expected']['outcome']['error']['category'])

    def test_unreviewed_source_or_canonical_literal_reject(self):
        changed=json.loads(cohort.candidates.SOURCES.read_text());changed[0]['original_fields'][3]='(return (int 42))'
        with patch.object(Path,'read_bytes',return_value=json.dumps(changed).encode()),self.assertRaisesRegex(ValueError,'Unreviewed complete original source table'):cohort.candidates.load()
        changed=json.loads(cohort.CONTRACTS.read_text());changed[0]['canonical_expected']['trace']=[0,1]
        with patch.object(Path,'read_bytes',return_value=json.dumps(changed).encode()),self.assertRaisesRegex(ValueError,'Unreviewed whole-source complete literal contracts'):cohort.load()

    def test_new_sources_do_not_overlap_existing_source_adapters(self):
        import sci_execution as generic,sci_source_context as sources,sci_library_context as library
        import reviewed_sci_context_dispatch as reviewed,sci_gap_canonical_context as gaps
        import sci_print_context as printing,sci_lazy_context as lazy,sci_cljc_accepted_context as cljc
        import sci_cljc_transport_after185 as transport,case_char_string_accepted as characters
        existing=[generic.load_all(),sources.load(),library.load(),reviewed.bindings(),gaps.load(),printing.load(),lazy.load(),cljc.load(),transport.load(),characters.load()[0]]
        import sci_numeric_error_after188 as numeric
        existing.append(numeric.load())
        ids={r['obligation_id'] for rows in existing for r in rows if 'obligation_id' in r}
        self.assertFalse(ids & {r['obligation_id'] for r in cohort.load()})

    def test_reference_inputs_and_executing_helpers_are_snapshotted(self):
        snapshot=cohort.snapshot()
        for path in (Path(cohort.__file__),Path(cohort.candidates.__file__),cohort.candidates.SOURCES,cohort.CONTRACTS,Path(__file__),Path(cohort.executable.__file__),cohort.proof.DRIVER,Path(cohort.pin_inputs.__file__)):
            self.assertEqual(cohort.sci.digest(path.read_bytes()),snapshot[str(path.resolve().relative_to(cohort.sci.ROOT))])
        for key,value in cohort.pin_inputs.reference_inputs().items():self.assertEqual(value,snapshot[key])

    @unittest.skipUnless(os.environ.get('LIGHT_ORIGINAL_SCI_AFTER191_RECEIPT'),'Explicit fresh whole-source original raw fixture')
    def test_rehashed_missing_source_unit_packet_and_pin_mutations_reject_then_restore(self):
        document=json.loads(Path(os.environ['LIGHT_ORIGINAL_SCI_AFTER191_RECEIPT']).read_text());cohort.reconcile(document)
        phase=Path(document['phase_artifacts_root']);reference=Path(document['reference_artifacts_root'])
        raw_path=phase/'observations.tsv';binding_path=phase/'execution-binding.json';reference_path=reference/'evidence.json'
        raw_bytes=raw_path.read_bytes();binding_bytes=binding_path.read_bytes();reference_bytes=reference_path.read_bytes()
        pin_unit=reference/'release/execution/out/semantic/case0.js';pin_bytes=pin_unit.read_bytes()
        pin_source=reference/'release/execution/src/semantic/case0.cljs';pin_source_bytes=pin_source.read_bytes()
        input_path=phase/'input.tsv';input_bytes=input_path.read_bytes()
        independent=reference.parent/'independent-pin-replay';independent_binding=independent/'binding.json';independent_bytes=independent_binding.read_bytes()
        independent_unit=independent/'release/out/semantic/case0.js';independent_unit_bytes=independent_unit.read_bytes()
        try:
            for mutation in ('missing-frame','changed-original-source','coherent-emitted-unit','changed-raw-packet','changed-reference-executable','coherent-pin-unit','coherent-pin-source','changed-phase-input','coherent-independent-pin-unit'):
                records=[line.split('\t') for line in raw_bytes.decode().splitlines()]
                if mutation=='missing-frame':records.pop()
                elif mutation=='changed-original-source':records[0][3]='(fn* [observe] 42)'.encode().hex()
                elif mutation=='coherent-emitted-unit':
                    records[0][4]=bytes.fromhex(records[0][4]).decode().replace('return','return /* raw guard */',1).encode().hex();records[1][4]=records[0][4]
                elif mutation=='changed-raw-packet':
                    packet=json.loads(bytes.fromhex(records[0][5]).decode());packet['outcome']={'status':'returned','value':None};records[0][5]=json.dumps(packet).encode().hex()
                elif mutation=='changed-reference-executable':
                    value=json.loads(reference_bytes);value['reference_execution_binding']['executables_sha256'][value['reference_execution_binding']['programs']['clojure']]='0'*64;reference_path.write_text(json.dumps(value))
                elif mutation=='coherent-pin-source':
                    pin_source.write_bytes(pin_source_bytes+b'\n;; changed complete source tree\n')
                    value=json.loads(reference_bytes);value['artifact_sha256']={str(p.relative_to(reference)):cohort.sci.digest(p.read_bytes()) for p in sorted(reference.rglob('*')) if p.is_file() and p!=reference_path};reference_path.write_text(json.dumps(value))
                elif mutation=='coherent-independent-pin-unit':
                    independent_unit.write_bytes(independent_unit_bytes+b'\n/* coherent independent tree mutation */\n')
                    value=json.loads(independent_bytes);value['artifact_sha256']={str(p.relative_to(independent)):cohort.sci.digest(p.read_bytes()) for p in sorted(independent.rglob('*')) if p.is_file() and p!=independent_binding};independent_binding.write_text(json.dumps(value))
                elif mutation=='changed-phase-input':input_path.write_bytes(input_bytes+b'\n')
                else:
                    pin_unit.write_text(pin_bytes.decode().replace('return','return /* pin raw guard */',1))
                    value=json.loads(reference_bytes);value['artifact_sha256']={str(p.relative_to(reference)):cohort.sci.digest(p.read_bytes()) for p in sorted(reference.rglob('*')) if p.is_file() and p!=reference_path};reference_path.write_text(json.dumps(value))
                raw_path.write_text('\n'.join('\t'.join(r) for r in records)+'\n')
                binding=json.loads(binding_bytes);binding['artifact_sha256']['observations.tsv']=cohort.sci.digest(raw_path.read_bytes());binding['artifact_sha256']['input.tsv']=cohort.sci.digest(input_path.read_bytes());binding_path.write_text(json.dumps(binding))
                with self.subTest(mutation=mutation),self.assertRaises(ValueError) as rejected:
                    cohort.verified_document(reference_path,phase)
                if mutation=='coherent-emitted-unit':self.assertIn('differs from executable replay',str(rejected.exception))
                if mutation=='changed-reference-executable':self.assertIn('reference executable path/hash drift',str(rejected.exception))
                if mutation=='coherent-pin-unit':self.assertIn('complete emitted units differ from compiler replay',str(rejected.exception))
                if mutation=='coherent-pin-source':self.assertIn('complete source differs from compiler replay',str(rejected.exception))
                if mutation=='changed-phase-input':self.assertIn('Changed original phase input',str(rejected.exception))
                if mutation=='coherent-independent-pin-unit':self.assertIn('Independently recorded pin emitted units differ from compiler replay',str(rejected.exception))
                raw_path.write_bytes(raw_bytes);binding_path.write_bytes(binding_bytes);reference_path.write_bytes(reference_bytes);pin_unit.write_bytes(pin_bytes);pin_source.write_bytes(pin_source_bytes);input_path.write_bytes(input_bytes);independent_binding.write_bytes(independent_bytes);independent_unit.write_bytes(independent_unit_bytes)
        finally:
            raw_path.write_bytes(raw_bytes);binding_path.write_bytes(binding_bytes);reference_path.write_bytes(reference_bytes);pin_unit.write_bytes(pin_bytes);pin_source.write_bytes(pin_source_bytes);input_path.write_bytes(input_bytes);independent_binding.write_bytes(independent_bytes);independent_unit.write_bytes(independent_unit_bytes)
        cohort.reconcile(document)

    @unittest.skipUnless(os.environ.get('LIGHT_ORIGINAL_SCI_AFTER191_RECEIPT'),'Explicit fresh whole-source original raw fixture')
    def test_selected_real_ledger_integration_keeps_original_credit_zero(self):
        document=json.loads(Path(os.environ['LIGHT_ORIGINAL_SCI_AFTER191_RECEIPT']).read_text())
        result=cohort.selected_integration(document)
        self.assertEqual({'covered':0,'missing':0,'version-difference':31},result['counts'])
        self.assertEqual(31,len(result['assertions']))
        self.assertFalse(result['strict_complete'])
        if os.environ.get('LIGHT_ORIGINAL_SCI_AFTER191_INTEGRATION_RECEIPT'):
            Path(os.environ['LIGHT_ORIGINAL_SCI_AFTER191_INTEGRATION_RECEIPT']).write_text(json.dumps(result,indent=2)+'\n')

if __name__=='__main__':unittest.main()
