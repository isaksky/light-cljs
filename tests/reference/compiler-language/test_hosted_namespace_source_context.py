"""Fail closed on changed original callers, providers and observation receipts."""
import copy,json,tempfile,unittest
from pathlib import Path
import hosted_namespace_source_context as adapter
class HostedNamespaceProtocol(unittest.TestCase):
    def setUp(self):self.rows=adapter.load();self.loaders={row['id']:'true' for row in self.rows}
    def raw(self):
        result=[]
        for row in self.rows:
            for variant in adapter.VARIANTS:
                source=adapter.macro_source(row['source'],'true') if variant.startswith('macro') else row['source']
                result.append('\t'.join([row['id'],variant,source.encode().hex(),'const rt = // Original bounded runtime\nnull'.encode().hex(),json.dumps(row['expected']).encode().hex(),'31' if variant.endswith('reused') else '-']))
        return '\n'.join(result)+'\n'
    def test_missing_reordered_or_changed_profile_rejected(self):
        for rows in (self.rows[:-1],list(reversed(self.rows)),self.rows+[self.rows[0]]):
            with self.assertRaises(ValueError):adapter.validate(rows)
        for key,value in [('provider_source','(ns goog) (def isArrayLike (fn [_] true))'),('source','(defn run [] true)'),('original_assertion_sha256',[]),('dependency_source_sha256',{})]:
            rows=copy.deepcopy(self.rows);rows[0][key]=value
            with self.assertRaises(ValueError):adapter.validate(rows)
    def test_changed_original_repl_function_rejected(self):
        with self.assertRaises(ValueError):adapter.repl_function('(defn Error->map [o] {:via [{:type (quote js/TypeError)}]})')
    def test_missing_or_fake_real_goog_dependency_rejected(self):
        with tempfile.TemporaryDirectory() as root:
            base=Path(root)/'out/goog/base.js';base.parent.mkdir(parents=True);base.write_text('fake goog predicate')
            with self.assertRaises(ValueError):adapter.goog_loader(Path(root))
            base.unlink()
            with self.assertRaises(FileNotFoundError):adapter.goog_loader(Path(root))
    def test_raw_order_complete_frames_and_reused_host_proof(self):
        self.assertEqual(len(adapter.parse_raw(self.raw(),self.rows,self.loaders)),8)
        lines=self.raw().splitlines()
        for changed in (lines[:-1],lines+[lines[0]],list(reversed(lines)),[line.rsplit('\t',1)[0]+'\t-' for line in lines]):
            with self.assertRaises(ValueError):adapter.parse_raw('\n'.join(changed),self.rows,self.loaders)
    def test_partial_changed_effect_or_phase_source_rejected(self):
        lines=self.raw().splitlines();first=lines[0].split('\t')
        for frame in ({'id':self.rows[0]['id'],'outcome':{'status':'returned','value':True}},{**self.rows[0]['expected'],'outcome':{'status':'returned','value':1}},{**self.rows[0]['expected'],'stdout':'hidden effect'},{**self.rows[0]['expected'],'trace':[1]}):
            changed=first.copy();changed[4]=json.dumps(frame).encode().hex()
            with self.assertRaises(ValueError):adapter.parse_raw('\n'.join(['\t'.join(changed)]+lines[1:]),self.rows,self.loaders)
        changed=first.copy();changed[2]='(defn run [] true)'.encode().hex()
        with self.assertRaises(ValueError):adapter.parse_raw('\n'.join(['\t'.join(changed)]+lines[1:]),self.rows,self.loaders)
if __name__=='__main__':unittest.main()
