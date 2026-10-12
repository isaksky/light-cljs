import copy,json,unittest
from unittest.mock import patch
import configured_custom_source_context as audit
class CustomSourceGuards(unittest.TestCase):
 def records(self):
  rows=[]
  for variant in audit.VARIANTS:
   for engine in ('QuickJS','Node'):
    raw=json.dumps(audit.expected(),separators=(',',':'))+ ('\n' if engine=='Node' else '')
    rows.append('\t'.join((variant,engine,audit.module(variant.split('-')[0]).encode().hex(),' = // Original bounded runtime'.encode().hex(),raw.encode().hex(),'-' if variant.endswith('fresh') else '74727565')))
  return rows
 def test_exact_whole_module_configuration_and_form_reader(self):
  row=audit.load();audit.validate(row)
  for field,value in [('source',row['source'].replace(audit.ASSERTION,'')),('configured_readers',[{'tag':'test/custom-form','source':'identity'}]),('expected',{'outcome':True})]:
   changed=copy.deepcopy(row);changed[field]=value
   with self.assertRaises(ValueError):audit.validate(changed)
  changed=copy.deepcopy(row);changed['configuration']['text']='{test/custom-form identity}'
  with self.assertRaises(ValueError):audit.validate(changed)
  changed=copy.deepcopy(row);changed['source_context_profile']['schema_version']=True
  with self.assertRaises(ValueError):audit.validate(changed)
 def test_complete_ordered_eight_frames(self):
  rows=self.records();self.assertEqual(len(audit.parse_raw('\n'.join(rows),audit.load())),8)
  for wrong in [rows[:-1],rows+[rows[0]],rows[::-1]]:
   with self.assertRaises(ValueError):audit.parse_raw('\n'.join(wrong),audit.load())
 def test_original_assertion_and_conditional_runtime_branch_not_reduced(self):
  rows=self.records();fields=rows[0].split('\t');source=audit.module('application').replace(audit.ASSERTION,'true');fields[2]=source.encode().hex();rows[0]='\t'.join(fields)
  with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
 def test_actual_reuse_warmup_required(self):
  rows=self.records();fields=rows[4].split('\t');fields[-1]='-';rows[4]='\t'.join(fields)
  with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
 def test_duplicate_json_and_extra_effect_rejected(self):
  with self.assertRaises(ValueError):audit.strict_json('{"a":1,"a":2}')
  rows=self.records();fields=rows[0].split('\t');actual=audit.expected();actual['stdout']='fake';fields[4]=json.dumps(actual).encode().hex();rows[0]='\t'.join(fields)
  with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
 def test_engine_compilation_provenance_cannot_diverge(self):
  rows=self.records();fields=rows[1].split('\t');fields[3]+='20';rows[1]='\t'.join(fields)
  with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
 def test_hosted_fixture_preserves_sole_assertion_and_exact_config(self):
  value=audit.case();self.assertTrue(audit.validate_case(value))
  self.assertEqual(value['complete_original_provenance'],[{'body_id':audit.load()['original_assertion']['body_id'],'source_sha256':audit.BODY_HASH}])
  for key,changed in [('source','true'),('complete_original_provenance',[]),('native_stdout','true\n'),('hosted_source_adapter',{'profile':audit.PROFILE,'schema_version':True})]:
   wrong=copy.deepcopy(value);wrong[key]=changed
   with self.assertRaises(ValueError):audit.validate_case(wrong)
 def test_hosted_execution_requires_fresh_complete_phase_frames(self):
  with patch.object(audit,'run',return_value={'local':{'frames':{}},'references':{}}) as run:
   with self.assertRaises(ValueError):audit.execute([audit.case()],'/tmp/unexecuted-test-hosted-profile')
   run.assert_called_once()
  for cases in [[],[audit.case(),audit.case()]]:
   with patch.object(audit,'run') as run:
    with self.assertRaises(ValueError):audit.execute(cases,'/tmp/unexecuted-test-hosted-profile')
    run.assert_not_called()
if __name__=='__main__':unittest.main()
