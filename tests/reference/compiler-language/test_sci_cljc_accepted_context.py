from copy import deepcopy
import json
from pathlib import Path
import os
import tempfile
import unittest
from unittest.mock import patch
import sci_cljc_accepted_context as reader


class AcceptedReaderGuards(unittest.TestCase):
    def test_whole_originals_and_twenty_three_reviewed_literals(self):
        rows=reader.load()
        self.assertEqual(30,len(rows))
        self.assertEqual(23,sum(r['canonical_expected'] is not None for r in rows))
        originals=reader.historical.catalog()
        for row, original in zip(rows,originals):
            for key in original:self.assertEqual(original[key],row[key])
            self.assertEqual(original['expected'],row['original_expected'])
            for phase in ('application','macro'):
                self.assertIn('(do '+row['source']+')',reader.historical.source(row['source'],phase))

    def test_changed_original_and_literal_catalog_reject(self):
        with patch.object(reader.historical,'catalog',return_value=[]):
            with self.assertRaises(ValueError):reader.load()
        with patch.object(reader,'CONTRACTS_SHA256','0'*64):
            with self.assertRaises(ValueError):reader.load()

    def test_typed_packet_requires_full_result_effects_and_stdout(self):
        row=reader.load()[0]; packet=deepcopy(row['canonical_expected'])
        self.assertEqual('passed',reader.observation(row,packet)['status'])
        for key in ('outcome','trace','stdout'):
            changed=deepcopy(packet);changed.pop(key)
            with self.assertRaises(ValueError):reader.observation(row,changed)
        changed=deepcopy(packet);changed['stdout']='unclaimed'
        self.assertEqual('mismatch',reader.observation(row,changed)['status'])
        changed=deepcopy(packet);changed['trace']=[False]
        self.assertEqual('mismatch',reader.observation(row,changed)['status'])

    def test_unreviewed_original_cannot_become_positive(self):
        row=next(r for r in reader.load() if r['canonical_expected'] is None)
        self.assertEqual('mismatch',reader.observation(row,row['original_expected'])['status'])

    def test_boolean_number_types_remain_distinct(self):
        with self.assertRaises(ValueError):reader.same({'v':False},{'v':0},'typed')
        with self.assertRaises(ValueError):reader.packets.strict_json('{"v":1,"v":2}')

    def test_entire_replayed_document_and_current_source_are_required(self):
        replay={'schema_version':1,'profile':reader.PROFILE,'artifacts_root':str(Path('/tmp/reader-guard').resolve()),
                'source_bindings':reader.load(),'compiler_build_producer_source_sha256':{'producer':'current'},
                'frames':{},'summaries':[],'evidence':{}}
        changed=deepcopy(replay);changed['summaries']=[{'status':'covered'}]
        with patch.object(reader,'snapshot',return_value={'producer':'current'}), patch.object(reader,'verified_document',return_value=replay):
            with self.assertRaisesRegex(ValueError,'entire current'):reader.reconcile(changed)
        changed=deepcopy(replay);changed['compiler_build_producer_source_sha256']={'producer':'cached'}
        with patch.object(reader,'snapshot',return_value={'producer':'current'}):
            with self.assertRaisesRegex(ValueError,'producer drift'):reader.reconcile(changed)
        with self.assertRaises(ValueError):reader.reconcile({'status':'covered'})

    def test_command_cannot_substitute_test_or_cwd_or_stdin(self):
        with tempfile.TemporaryDirectory() as path:
            root=Path(path)
            record={'command':['audit','exact'],'cwd':str(reader.ROOT),'returncode':0,'stdin':None,'environment':{}}
            (root/'command.json').write_text(json.dumps(record))
            reader.check_command(root,['audit','exact'])
            for field,value in (('command',['audit','other']),('cwd','/tmp'),('stdin','rewritten'),('returncode',1)):
                changed=deepcopy(record);changed[field]=value;(root/'command.json').write_text(json.dumps(changed))
                with self.assertRaises(ValueError):reader.check_command(root,['audit','exact'])

    def test_raw_census_and_executables_have_no_cached_fallback(self):
        with tempfile.TemporaryDirectory() as path:
            root=Path(path)
            binding={'producer_source_sha256':reader.snapshot(),'programs':reader.executable_paths(),
                     'audit_executable':str(root/'missing-audit'),'executables_sha256':{},
                     'audit_environment':{'LIGHT_SCI_CLJC_ACCEPTED_ROOT':str(root/'phases'),'NODE':reader.executable_paths()['node']},
                     'artifact_sha256':{}}
            with self.assertRaisesRegex(ValueError,'hashes'):reader.verify_binding(root,binding)
            with self.assertRaises(FileNotFoundError):reader.verified_document(root)

    def test_each_pin_and_every_actual_frame_are_required(self):
        rows=reader.load()
        originals={r['id']:{'status':'passed' if r['canonical_expected'] is not None else 'mismatch',
                              'observed':deepcopy(r['canonical_expected'] or r['original_expected']),
                              'original_status':'mismatch'} for r in rows}
        complete={r['id']:{v+'/'+e:{'status':'passed','original_status':'mismatch'}
                          for v in reader.VARIANTS for e in ('QuickJS','Node')} for r in rows}
        identity=next(r['id'] for r in rows if r['canonical_expected'] is not None)
        def document(frames, pins):
            with tempfile.TemporaryDirectory() as path:
                root=Path(path);(root/'binding.json').write_text('{"programs":{}}')
                with patch.object(reader,'snapshot',return_value={}), patch.object(reader,'verify_binding'), \
                     patch.object(reader,'verify_reference',side_effect=lambda kind,*args:pins[kind]), \
                     patch.object(reader,'verify_local',return_value=frames):
                    return reader.verified_document(root)
        pins={kind:deepcopy(originals) for kind in ('release','checkout')}
        baseline=document(complete,pins)
        self.assertEqual(23,sum(r['status']!='missing' for r in baseline['summaries']))
        for key in complete[identity]:
            for failure in ('missing','mismatch','observation-error'):
                frames=deepcopy(complete)
                if failure=='missing':frames[identity].pop(key)
                else:frames[identity][key]['status']=failure
                actual=next(r for r in document(frames,pins)['summaries'] if r['id']==identity)
                self.assertEqual('missing',actual['status'])
                self.assertEqual(0,actual['original_semantic_pass_credit'])
        for kind in pins:
            changed=deepcopy(pins);changed[kind][identity]['status']='mismatch'
            self.assertEqual('missing',next(r for r in document(complete,changed)['summaries'] if r['id']==identity)['status'])

    def test_phase_parser_requires_exact_frame_order_and_original_module(self):
        receipt=os.environ.get('SCI_CLJC_ACCEPTED_RECEIPT')
        if not receipt:self.skipTest('requires a fresh actual receipt')
        doc=json.loads(Path(receipt).read_text());root=Path(doc['artifacts_root'])/'phases'
        original=(root/'observations.tsv').read_bytes()
        records=original.decode().splitlines()
        for altered in (records[:-1],records+[records[-1]],list(reversed(records))):
            try:
                (root/'observations.tsv').write_text('\n'.join(altered)+'\n')
                with self.assertRaises(ValueError):reader.verify_local(root,reader.load(),doc['execution_binding'])
            finally:(root/'observations.tsv').write_bytes(original)
        reader.verify_local(root,reader.load(),doc['execution_binding'])

    def test_live_receipt_artifact_and_executable_mutations_fail_closed(self):
        receipt=os.environ.get('SCI_CLJC_ACCEPTED_RECEIPT')
        if not receipt:self.skipTest('requires a fresh actual receipt')
        document=json.loads(Path(receipt).read_text());root=Path(document['artifacts_root'])
        verified=reader.reconcile(document)
        self.assertEqual(30,len(verified))
        for field in ('summaries','frames','references','source_bindings','evidence','execution_binding'):
            changed=deepcopy(document);changed[field]={}
            with self.assertRaises((ValueError,KeyError,TypeError)):reader.reconcile(changed)
        files=['phases/observations.tsv','phases/warmup.js','phases/input.tsv','phases/audit/command.json',
               'release/contexts.js','release/build/stdout','checkout/contexts.tsv']
        for name in files:
            path=root/name;original=path.read_bytes()
            try:
                path.write_bytes(original+b'changed')
                with self.assertRaises(ValueError):reader.reconcile(document)
            finally:path.write_bytes(original)
        binary=next(iter(document['execution_binding']['executables_sha256']))
        changed=deepcopy(document['execution_binding']);changed['executables_sha256'][binary]='0'*64
        with self.assertRaisesRegex(ValueError,'executable hash drift'):reader.verify_binding(root,changed)
        extra=root/'unclaimed-artifact'
        try:
            extra.write_text('unclaimed')
            with self.assertRaisesRegex(ValueError,'artifact census'):reader.reconcile(document)
        finally:extra.unlink(missing_ok=True)
        reader.reconcile(document)

    def test_fresh_producer_never_overwrites_existing_receipt(self):
        with tempfile.TemporaryDirectory() as path:
            with self.assertRaises(FileExistsError):reader.run(path)


if __name__=='__main__':unittest.main()
