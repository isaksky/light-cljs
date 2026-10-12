import copy,json,unittest
import sci_symbol_keyword_context as audit
class SourceContextGuards(unittest.TestCase):
    def records(self):
        rows=[]
        for variant in audit.VARIANTS:
            for engine in ('QuickJS','Node'):
                raw=json.dumps(audit.expected(),separators=(',',':'))+ ('\n' if engine=='Node' else '')
                rows.append('\t'.join((variant,engine,audit.module(variant.split('-')[0]).encode().hex(),' = // Original bounded runtime'.encode().hex(),raw.encode().hex(),'-' if variant.endswith('fresh') else '74727565')))
        return rows
    def test_only_exact_original_profile_and_literal_result(self):
        row=audit.load();audit.validate(row)
        for key,value in [('source',audit.SOURCE.replace('::x',':x')),('namespace','other'),('expected',{'outcome':True}),('body_sha256','0'*64)]:
            changed=copy.deepcopy(row);changed[key]=value
            with self.assertRaises(ValueError):audit.validate(changed)
        changed=copy.deepcopy(row);changed['source_context_profile']['schema_version']=True
        with self.assertRaises(ValueError):audit.validate(changed)
    def test_requires_all_eight_ordered_actual_frames(self):
        rows=self.records();self.assertEqual(len(audit.parse_raw('\n'.join(rows),audit.load())),8)
        for wrong in [rows[:-1],rows+[rows[0]],rows[::-1],[rows[1],rows[0],*rows[2:]]]:
            with self.assertRaises(ValueError):audit.parse_raw('\n'.join(wrong),audit.load())
    def test_rejects_reader_namespace_drift(self):
        rows=self.records();fields=rows[0].split('\t');fields[2]=audit.module('application').replace('(ns user)','(ns other)').encode().hex();rows[0]='\t'.join(fields)
        with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
    def test_requires_prior_reused_host_compilation(self):
        rows=self.records();fields=rows[4].split('\t');fields[-1]='-';rows[4]='\t'.join(fields)
        with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
    def test_rejects_duplicate_missing_and_extra_effects(self):
        with self.assertRaises(ValueError):audit.strict_json('{"a":1,"a":2}')
        rows=self.records();fields=rows[0].split('\t');value=audit.expected();value['trace']=[1];fields[4]=json.dumps(value).encode().hex();rows[0]='\t'.join(fields)
        with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
    def test_engines_share_one_compilation_and_warmup(self):
        rows=self.records();fields=rows[1].split('\t');fields[3]+='20';rows[1]='\t'.join(fields)
        with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
if __name__=='__main__':unittest.main()
