import copy,json,unittest
import sci_record_source_context as audit
class RecordContextGuards(unittest.TestCase):
 def records(self):
  rows=[]
  for row in audit.load():
   for variant in audit.VARIANTS:
    for engine in ('QuickJS','Node'):
     raw=json.dumps(row['expected'],separators=(',',':'))+ ('\n' if engine=='Node' else '')
     rows.append('\t'.join((row['id'],variant,engine,audit.module(row,variant.split('-')[0]).encode().hex(),' = // Original bounded runtime'.encode().hex(),raw.encode().hex(),'-' if variant.endswith('fresh') else '74727565')))
  return rows
 def test_exact_originals_and_narrow_exclusions(self):
  original=audit.load();audit.validate(original)
  for field,value in [('source','true'),('expected',{}),('setup_source',''),('source_context_profile',{'schema_version':1,'profile':audit.PROFILE,'namespace':'user','excluded_core_identifiers':['Box','Empty','map']})]:
   changed=copy.deepcopy(original);changed[0][field]=value
   with self.assertRaises(ValueError):audit.validate(changed)
  changed=copy.deepcopy(original);changed[0]['source_context_profile']['schema_version']=True
  with self.assertRaises(ValueError):audit.validate(changed)
 def test_all24_ordered_independent_frames_required(self):
  rows=self.records();self.assertEqual(len(audit.parse_raw('\n'.join(rows),audit.load())),24)
  for wrong in [rows[:-1],rows+[rows[0]],rows[::-1]]:
   with self.assertRaises(ValueError):audit.parse_raw('\n'.join(wrong),audit.load())
 def test_rejects_renamed_original_constructor(self):
  rows=self.records();fields=rows[0].split('\t');fields[3]=bytes.fromhex(fields[3]).decode().replace('Box','RenamedBox').encode().hex();rows[0]='\t'.join(fields)
  with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
 def test_default_header_cannot_receive_profile_credit(self):
  rows=self.records();fields=rows[0].split('\t');fields[3]=audit.module(audit.load()[0],'application',True).encode().hex();rows[0]='\t'.join(fields)
  with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
 def test_reused_host_requires_real_warmup(self):
  rows=self.records();fields=rows[4].split('\t');fields[-1]='-';rows[4]='\t'.join(fields)
  with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
 def test_control_must_be_exact_bothpin_type_error(self):
  r=audit.sci.report_module();layer={'status':'mismatch','observed':{'outcome':{'status':'threw','error':{'category':'type-error'}},'trace':[],'stdout':''},'reference_identity':'release '+r.RELEASE,'settings':{'optimizations':'none','static-fns':False,'def-emits-var':False}}
  audit.validate_control(layer,'release')
  for key,value in [('status','passed'),('reference_identity','release unknown'),('observed',{'outcome':{'status':'returned','value':True},'trace':[],'stdout':''})]:
   changed=copy.deepcopy(layer);changed[key]=value
   with self.assertRaises(ValueError):audit.validate_control(changed,'release')
 def test_engines_cannot_be_detached_from_same_compilation(self):
  rows=self.records();fields=rows[1].split('\t');fields[4]+='20';rows[1]='\t'.join(fields)
  with self.assertRaises(ValueError):audit.parse_raw('\n'.join(rows),audit.load())
if __name__=='__main__':unittest.main()
