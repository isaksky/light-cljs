#!/usr/bin/env python3
"""Fail closed on lost bodies, false form evidence, and adapter widening."""
import copy
from pathlib import Path
import unittest
from unittest.mock import patch
import syntax_context as context

class SyntaxContextTests(unittest.TestCase):
 def test_complete_frozen_sources_and_independent_literals(self):
  self.assertEqual(len(context.rows()),7)
  result=context.exports()
  ids=[r['obligation_id'] for r in result['obligation-witnesses.jsonl']]
  self.assertEqual(set(ids),{'syntax::'+n for n in ['if','def','fn*','do','let*','loop*','letfn*','throw','try','recur','new','set!','ns','deftype*','defrecord*','.','js*','&','quote','case*','var']})
  self.assertEqual(len(ids),21)
  self.assertNotIn('syntax::ns*',ids)
 def test_quoted_or_discarded_heads_cannot_supply_execution(self):
  self.assertEqual(context.active_syntax("'(if true (throw 1)) #_(new js/Error) \"(ns* x)\""),{'quote'})
  self.assertEqual(context.active_syntax('(quote (recur 1))'),{'quote'})
 def test_metadata_and_free_rest_symbol_cannot_supply_bindings(self):
  self.assertNotIn('&',context.active_syntax('[& :x]'))
  self.assertNotIn('throw',context.active_syntax('^{:tag (throw 1)} (do 2)'))
  self.assertIn('&',context.active_syntax('((fn* [x & xs] (vec xs)) 1 2)'))
 def test_changed_whole_source_or_expected_is_rejected(self):
  original=(context.ROOT/context.FIXTURE).read_text()
  lines=original.splitlines();fields=lines[1].split('\t');metadata=__import__('json').loads(fields[4])
  for field,key,value in [(2,'source','(do 1)'),(3,'expected_edn','1')]:
   changed=fields[:];changed[field]=value.encode().hex();record=copy.deepcopy(metadata);record[key]=value
   if key=='source':record['source_sha256']=context.digest(value)
   changed[4]=__import__('json').dumps(record)
   text='\n'.join([lines[0],'\t'.join(changed),*lines[2:]])+'\n'
   real=Path.read_text
   def read(path,*args,**kwargs):
    return text if path==context.ROOT/context.FIXTURE else real(path,*args,**kwargs)
   with patch.object(Path,'read_text',read),self.assertRaises(ValueError):context.rows()
 def test_reference_adapter_preserves_all_runtime_body_and_macro_companion(self):
  case=context.cases()[-1]
  self.assertEqual(len(case['reference_files_sha256']),3)
  real=Path.read_text;target=context.ROOT/case['reference_source_root']/'syntax/header.cljs'
  def read(path,*args,**kwargs):
   source=real(path,*args,**kwargs)
   return source.replace('(def value 4)','(def value 5)') if path==target else source
  with patch.object(Path,'read_text',read),self.assertRaises(ValueError):context.cases()
 def test_declared_names_cannot_reassign_unreviewed_paths(self):
  real=Path.read_text
  def read(path,*args,**kwargs):
   value=real(path,*args,**kwargs)
   if path!=context.ROOT/context.FIXTURE:return value
   lines=value.splitlines();fields=lines[1].split("\t");record=__import__("json").loads(fields[4]);record["syntax_names"][0],record["syntax_names"][2]=record["syntax_names"][2],record["syntax_names"][0];fields[1]=",".join(record["syntax_names"]);fields[4]=__import__("json").dumps(record);lines[1]="\t".join(fields);return "\n".join(lines)+"\n"
  with patch.object(Path,"read_text",read),self.assertRaises(ValueError):context.rows()
 def test_phase_and_application_bind_exact_driver_and_supports(self):
  exported=context.exports()
  for ordinary,phase in zip(exported['obligation-witnesses.jsonl'],exported['phase-witnesses.jsonl']):
   self.assertEqual(ordinary['local_execution'],{k:phase[k] for k in ['test_file','test_name','test_source_sha256']})
   self.assertEqual(phase['test_source_sha256'],context.digest((context.ROOT/context.DRIVER).read_bytes()))
   self.assertIn(context.FIXTURE,phase['support_source_sha256'])
   self.assertIn('tests/fixtures/compiler-syntax-context/local/syntax/observer.cljs',phase['support_source_sha256'])
if __name__=='__main__':unittest.main()
