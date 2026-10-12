import copy
import json
import os
from pathlib import Path
import unittest
from unittest import mock
import cljc_rejections_after191 as proof

class RejectionGuards(unittest.TestCase):
    def test_complete_originals_and_terminal_source_shapes_are_preserved(self):
        rows=proof.load()
        self.assertEqual(4,len(rows))
        self.assertEqual(3,sum(row['canonical_reference_rejection'] is not None for row in rows))
        for row in rows:
            for variant in proof.historical.VARIANTS:
                self.assertEqual(1,proof.source(row['source'],variant).count(row['source']))

    def test_unreviewed_literal_catalog_is_rejected(self):
        with mock.patch.object(proof,'TABLE_SHA','0'*64):
            with self.assertRaisesRegex(ValueError,'Unreviewed rejection catalog'):proof.load()

    @unittest.skipUnless(os.environ.get('LIGHT_CLJC_REJECTIONS_AFTER191_RECEIPT'),'fresh source rejection producer required')
    def test_coherent_raw_source_recovery_and_pin_mutations_reject_then_restore(self):
        document=json.loads(Path(os.environ['LIGHT_CLJC_REJECTIONS_AFTER191_RECEIPT']).read_text())
        root=Path(document['root']);originals={p:p.read_bytes() for p in root.rglob('*') if p.is_file()};results=[]
        try:
            for mutation in ['duplicate-warmup','missing-macro-rejection','changed-original-source','changed-recovery-result','coherent-recovery-unit','coherent-pin-input','coherent-pin-unit','changed-pin-identity']:
                for path,raw in originals.items():path.write_bytes(raw)
                changed=copy.deepcopy(document);raw_path=root/'local/observations.tsv';lines=raw_path.read_text().splitlines()
                if mutation=='duplicate-warmup':lines.insert(1,lines[0]);raw_path.write_text('\n'.join(lines)+'\n')
                elif mutation=='missing-macro-rejection':
                    index=next(i for i,line in enumerate(lines) if line.startswith('rejected\t') and '\tmacro-fresh\t' in line)
                    del lines[index];raw_path.write_text('\n'.join(lines)+'\n')
                elif mutation=='changed-original-source':
                    path=next((root/'local').rglob('user.cljc'));path.write_text(path.read_text()+'\n')
                elif mutation=='changed-recovery-result':
                    index=next(i for i,line in enumerate(lines) if line.startswith('recovery\t'));fields=lines[index].split('\t');fields[-1]='43'.encode().hex();lines[index]='\t'.join(fields);raw_path.write_text('\n'.join(lines)+'\n')
                elif mutation=='coherent-recovery-unit':
                    path=next((root/'local').rglob('recovery.js'));old=path.read_text();new=old+'\n/* coherent unexecuted mutation */';path.write_text(new)
                    raw_path.write_text(raw_path.read_text().replace(old.encode().hex(),new.encode().hex()))
                    for frame in changed['local']['frames']:
                        if frame.get('emitted_sha256')==proof.sci.digest(old):frame['emitted_sha256']=proof.sci.digest(new)
                elif mutation=='coherent-pin-input':
                    path=root/'release/input.tsv';path.write_text(path.read_text().replace('28','29',1))
                elif mutation=='coherent-pin-unit':
                    path=next((root/'release').rglob('*.js'));path.write_text(path.read_text()+'\n/* coherent unexecuted pin unit */')
                elif mutation=='changed-pin-identity':
                    first=next(iter(changed['references']['release']));changed['references']['release'][first]['reference_identity']='release altered'
                changed['local']['artifact_sha256']=proof.tree(root/'local')
                for pin in ['release','checkout']:changed['reference_artifact_sha256'][pin]=proof.tree(root/pin)
                with self.subTest(mutation=mutation):
                    with self.assertRaises((ValueError,AssertionError)):proof.replay(changed)
                results.append({'mutation':mutation,'result':'rejected'})
        finally:
            for path,raw in originals.items():path.write_bytes(raw)
        self.assertTrue(proof.replay(document))
        if os.environ.get('LIGHT_CLJC_REJECTIONS_AFTER191_RAW_RECEIPT'):
            Path(os.environ['LIGHT_CLJC_REJECTIONS_AFTER191_RAW_RECEIPT']).write_text(json.dumps({'mutations':results,'pristine_restored_replay':'passed','original_semantic_pass_credit':0,'original_macro_execution_pass_credit':0},indent=2)+'\n')

    @unittest.skipUnless(os.environ.get('LIGHT_CLJC_REJECTIONS_AFTER191_RECEIPT'),'fresh source rejection producer required')
    def test_current_original_rejections_replay_without_semantic_credit(self):
        document=json.loads(Path(os.environ['LIGHT_CLJC_REJECTIONS_AFTER191_RECEIPT']).read_text())
        self.assertEqual(0,document['original_semantic_pass_credit'])
        self.assertEqual(0,document['original_macro_execution_pass_credit'])
        self.assertFalse(document['strict_complete'])
        self.assertTrue(proof.replay(document))

    @unittest.skipUnless(os.environ.get('LIGHT_CLJC_REJECTIONS_AFTER191_RECEIPT'),'fresh source rejection producer required')
    def test_exact_negative_accounting_keeps_original_macro_nonpassing(self):
        import cljc_rejection_after191_accounting as accounting
        document=json.loads(Path(os.environ['LIGHT_CLJC_REJECTIONS_AFTER191_RECEIPT']).read_text())
        contexts=proof.reconcile(document)
        # Structural accounting controls use explicit synthetic prior rows;
        # the comprehensive gate must supply its actual fresh 30-source prior.
        priors={i:{'status':'missing','source_binding':copy.deepcopy(p['source_binding']),
                   'layers':copy.deepcopy(p['layers'])} for i,p in contexts.items()}
        evidence={p['id']:copy.deepcopy(priors[i]['layers']) for i,p in contexts.items()}
        with mock.patch.object(proof,'reconcile',return_value=copy.deepcopy(contexts)):
            result=accounting.verified(document,priors,evidence)
        self.assertEqual(3,len(result))
        report={'assertions':[{'status':'verified-rejection',
          'canonical_source_rejection_reconciliation':p,
          'columns':{'quickjs_macro':{'status':'rejected-before-original-computation'}}}
          for p in result.values()]}
        self.assertEqual(set(evidence),accounting.explained_ids(report,evidence))
        for mutation in ('missing-prior','accepted-prior','changed-source','changed-expectation','detached-evidence','accepted-pin'):
            bad=copy.deepcopy(priors);layers=copy.deepcopy(evidence);identity=next(iter(bad))
            if mutation=='missing-prior':del bad[identity]
            elif mutation=='accepted-prior':bad[identity]['status']='covered'
            elif mutation=='changed-source':bad[identity]['source_binding']['source']+=' '
            elif mutation=='changed-expectation':bad[identity]['source_binding']['original_expected']['trace']=[1]
            elif mutation=='detached-evidence':layers[next(iter(layers))]={}
            elif mutation=='accepted-pin':
                bad[identity]['layers']['release']['status']='passed'
                layers[bad[identity]['source_binding']['id']]=copy.deepcopy(bad[identity]['layers'])
            with self.subTest(mutation=mutation),mock.patch.object(proof,'reconcile',return_value=copy.deepcopy(contexts)):
                with self.assertRaises(ValueError):accounting.verified(document,bad,layers)
        for mutation in ('original-credit','macro-credit','macro-pass','accepted-pin','missing-phase','missing-recovery','changed-source','detached-prior'):
            bad=copy.deepcopy(report);row=bad['assertions'][0];negative=row['canonical_source_rejection_reconciliation']
            if mutation=='original-credit':negative['original_semantic_pass_credit']=1
            elif mutation=='macro-credit':negative['original_macro_execution_pass_credit']=1
            elif mutation=='macro-pass':row['columns']['quickjs_macro']['status']='passed'
            elif mutation=='accepted-pin':negative['layers']['release']['status']='passed'
            elif mutation=='missing-phase':negative['actual_phase_frames'].pop(next(iter(negative['actual_phase_frames'])))
            elif mutation=='missing-recovery':negative['same_host_recovery_frames'].pop()
            elif mutation=='changed-source':negative['source_binding']['source']+=' '
            elif mutation=='detached-prior':negative['prior_observer_reconciliation']['layers']={}
            with self.subTest(mutation=mutation):self.assertNotIn(negative['id'],accounting.explained_ids(bad,evidence))
        for p in result.values():
            self.assertEqual(0,p['original_semantic_pass_credit'])
            self.assertEqual(0,p['original_macro_execution_pass_credit'])
            self.assertEqual('rejected-before-original-computation',p['actual_macro_contract_status'])

if __name__=='__main__':unittest.main()
