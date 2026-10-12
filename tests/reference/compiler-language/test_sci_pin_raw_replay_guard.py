"""Synthetic raw-artifact mutation controls; no execution or ledger credit.

The fixtures model a verified literal observation and source markers, then
reproduce the rejected-preflight/empty-execution/census-rehash stale observation
exploit. Only the guard parser is exercised; no fixture claims real pin runs.
"""
from copy import deepcopy
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
import sci_gap_canonical_context as proof


class PinRawReplayMutationGuards(unittest.TestCase):
    def fixture(self, root):
        identity='guard-source';source='(+ 0 1)'
        canonical={'id':identity,'outcome':{'status':'returned','value':1},'trace':[],'stdout':''}
        original={**deepcopy(canonical),'outcome':{'status':'returned','value':2}}
        row={'id':identity,'source':source,'expected':original}
        selected={**row,'canonical_expected':canonical}
        cohort=SimpleNamespace(candidates=SimpleNamespace(load=lambda:[deepcopy(row)],snapshot=lambda:{'synthetic':'guard'}),
                               load=lambda:[deepcopy(selected)])
        document={'cases':[row],'producer_sha256':{'synthetic':'guard'},'evidence':{identity:{}}}
        for kind in ('release','checkout'):
            pin=root/kind
            for path in ('preflight','execution/build','execution/node-0'):(pin/path).mkdir(parents=True)
            full=proof.sci.REFERENCE_SETUP+source
            (pin/'preflight/analysis.tsv').write_text(identity+'\taccepted\tguard\n')
            (pin/'preflight/cases.tsv').write_text(identity+'\texpr\t\t(fn* [observe] '+full+')\t\n')
            (pin/'execution/cases.tsv').write_text(identity+'\t'+full+'\n')
            reference=kind+' '+(proof.sci.report_module().RELEASE if kind=='release' else proof.sci.report_module().REVISION)
            (pin/'execution/build/stdout').write_text('REFERENCE IDENTITY '+reference+'\nREFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}\n')
            (pin/'execution/node-0/stdout').write_text(json.dumps(canonical)+'\n')
            (pin/'execution/node-0/stderr').write_text('CASE START '+json.dumps({'id':identity,'source':full})+'\nCASE END '+json.dumps({'id':identity})+'\n')
            document['evidence'][identity][kind]={'status':'mismatch','observed':deepcopy(canonical),
                'reference_identity':reference,'settings':{'optimizations':'none','static-fns':False,'def-emits-var':False},'stderr':''}
        self.write(root,document)
        return cohort,document

    def write(self,root,document):
        document['artifact_sha256']={str(p.relative_to(root)):proof.sci.digest(p.read_bytes())
            for p in sorted(root.rglob('*')) if p.is_file() and p!=root/'evidence.json'}
        (root/'evidence.json').write_text(json.dumps(document))

    def test_rejected_preflight_empty_execution_rehash_cannot_reuse_stale_literal_pass(self):
        for rejected in (('release',),('checkout',),('release','checkout')):
            with self.subTest(rejected=rejected),tempfile.TemporaryDirectory() as temporary:
                root=Path(temporary)/'originals';root.mkdir();cohort,document=self.fixture(root)
                proof.reference_proof(root/'evidence.json',cohort=cohort)
                unchanged=deepcopy(document['evidence'])
                for kind in rejected:
                    pin=root/kind
                    (pin/'preflight/analysis.tsv').write_text('guard-source\trejected\tguard\n')
                    (pin/'execution/cases.tsv').write_text('')
                self.write(root,document)
                self.assertEqual(unchanged,document['evidence'])
                with self.assertRaisesRegex(ValueError,'requires accepted pin analysis and raw execution'):
                    proof.reference_proof(root/'evidence.json',cohort=cohort)

    def test_accepted_preflight_cannot_omit_actual_execution_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)/'originals';root.mkdir();cohort,document=self.fixture(root)
            (root/'release/execution/cases.tsv').write_text('')
            self.write(root,document)
            with self.assertRaisesRegex(ValueError,'accepted reference sources'):
                proof.reference_proof(root/'evidence.json',cohort=cohort)

    def test_accepted_preflight_requires_raw_node_result(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)/'originals';root.mkdir();cohort,document=self.fixture(root)
            (root/'checkout/execution/node-0/stdout').unlink()
            self.write(root,document)
            with self.assertRaises(FileNotFoundError):proof.reference_proof(root/'evidence.json',cohort=cohort)

    def test_noncanonical_diagnostic_rejection_remains_uncredited(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)/'originals';root.mkdir();cohort,document=self.fixture(root)
            extra={'id':'diagnostic-only','source':'unavailable','expected':{'id':'diagnostic-only'}}
            originals=cohort.candidates.load()+[extra]
            cohort.candidates.load=lambda:deepcopy(originals)
            document['cases']=deepcopy(originals)
            document['evidence']['diagnostic-only']={kind:{'status':'rejected'} for kind in ('release','checkout')}
            for kind in ('release','checkout'):
                pin=root/kind
                with (pin/'preflight/analysis.tsv').open('a') as output:output.write('diagnostic-only\trejected\tdiagnostic\n')
                with (pin/'preflight/cases.tsv').open('a') as output:output.write('diagnostic-only\texpr\t\t(fn* [observe] '+proof.sci.REFERENCE_SETUP+extra['source']+')\t\n')
            self.write(root,document)
            replay=proof.reference_proof(root/'evidence.json',cohort=cohort)
            self.assertEqual({'release':{'status':'rejected'},'checkout':{'status':'rejected'}},replay['evidence']['diagnostic-only'])

    def test_warning_preflight_still_requires_actual_raw_result(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)/'originals';root.mkdir();cohort,document=self.fixture(root)
            (root/'release/preflight/analysis.tsv').write_text('guard-source\twarning\tcanonical-warning\n')
            self.write(root,document)
            proof.reference_proof(root/'evidence.json',cohort=cohort)
            (root/'release/execution/node-0/stdout').unlink()
            self.write(root,document)
            with self.assertRaises(FileNotFoundError):proof.reference_proof(root/'evidence.json',cohort=cohort)

    def test_malformed_preflight_fails_before_stale_literal_observation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)/'originals';root.mkdir();cohort,document=self.fixture(root)
            (root/'release/preflight/analysis.tsv').write_text('guard-source\n')
            self.write(root,document)
            with self.assertRaisesRegex(ValueError,'Malformed actual canonical pin analysis'):
                proof.reference_proof(root/'evidence.json',cohort=cohort)


if __name__=='__main__':unittest.main()
