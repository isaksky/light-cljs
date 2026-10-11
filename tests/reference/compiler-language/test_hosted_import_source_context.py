"""Reject substitutions and incomplete hosted import source/dependency receipts."""
import copy,json,tempfile,unittest
from pathlib import Path
import hosted_import_source_context as adapter
class HostedImportProtocol(unittest.TestCase):
    def setUp(self):self.case=adapter.load();self.loader='(function(){globalThis.CLOSURE_NO_DEPS = true;return true;})()'
    def raw(self):
        rows=[]
        for variant in adapter.VARIANTS:
            source=adapter.macro_source(self.case['source'],self.loader) if variant.startswith('macro') else self.case['source']
            fields=[variant,source.encode().hex(),'const rt = // Original bounded runtime\nnull'.encode().hex(),json.dumps(adapter.EXPECTED).encode().hex(),'31' if variant.endswith('reused') else '-'];rows.append('\t'.join(fields))
        return '\n'.join(rows)+'\n'
    def test_rejects_reduced_body_changed_imports_or_fake_profile(self):
        for field,value in [('source','(defn run [] true)'),('source',self.case['source'].replace('(Long. 4 6)','(js-obj)')),('hosted_import_profile','generic goog'),('closure_dependency_sha256',{}),('original_assertion_sha256',self.case['original_assertion_sha256'][:-1])]:
            case=copy.deepcopy(self.case);case[field]=value
            with self.assertRaises(ValueError):adapter.validate(case)
    def test_rejects_missing_or_drifted_real_dependencies(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for path in adapter.DEPENDENCIES:
                file=root/path;file.parent.mkdir(parents=True,exist_ok=True);file.write_text('fake library bytes')
            with self.assertRaises(ValueError):adapter.dependency_bundle(root)
            (root/'math/long.js').unlink()
            with self.assertRaises((ValueError,FileNotFoundError)):adapter.dependency_bundle(root)
    def test_raw_order_completeness_and_reused_host_proof(self):
        self.assertEqual(len(adapter.parse_raw(self.raw(),self.case,self.loader)),4)
        rows=self.raw().splitlines()
        for changed in (rows[:-1],rows+[rows[0]],list(reversed(rows)),[row.rsplit('\t',1)[0]+'\t-' for row in rows]):
            with self.assertRaises(ValueError):adapter.parse_raw('\n'.join(changed),self.case,self.loader)
    def test_rejects_substituted_phase_or_partial_mismatching_frames(self):
        rows=self.raw().splitlines();first=rows[0].split('\t')
        for frame in ({'id':adapter.ID,'outcome':{'status':'returned','value':True}},{**adapter.EXPECTED,'outcome':{'status':'returned','value':1}},{**adapter.EXPECTED,'trace':[1]},{**adapter.EXPECTED,'stdout':'hidden effect'}):
            changed=first.copy();changed[3]=json.dumps(frame).encode().hex()
            with self.assertRaises(ValueError):adapter.parse_raw('\n'.join(['\t'.join(changed)]+rows[1:]),self.case,self.loader)
        changed=first.copy();changed[1]='(defn run [] true)'.encode().hex()
        with self.assertRaises(ValueError):adapter.parse_raw('\n'.join(['\t'.join(changed)]+rows[1:]),self.case,self.loader)
if __name__=='__main__':unittest.main()
