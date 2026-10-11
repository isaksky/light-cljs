#!/usr/bin/env python3
"""Export complete literal syntax execution contracts; no shared-table writes.

Candidate rows are eligibility only. The report must execute their exact named
Rust test and both checked source-bound canonical pin oracles before credit.
"""
from pathlib import Path
import argparse
import hashlib
import json
import re
from extract_inventory import Reader
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'tests/reference/compiler-language'
FIXTURE = 'tests/fixtures/compiler-syntax-context.tsv'
DRIVER = 'tests/compiler_syntax_context.rs'
PREFIX = 'syntax-context-'
TESTS = {
 'control-rest-recursion':'syntax_control_rest_and_recursion_execute_in_both_phases',
 'exception-interop-template':'syntax_exception_interop_and_template_execute_in_both_phases',
 'quote-case-star':'syntax_quote_and_case_star_execute_in_both_phases',
 'definition-var-state':'syntax_definition_and_var_state_execute_in_both_phases',
 'raw-type-constructor':'syntax_raw_type_constructor_executes_in_both_phases',
 'raw-record-constructor':'syntax_raw_record_constructor_executes_in_both_phases',
 'namespace-header-context':'syntax_namespace_context_executes_with_actual_macro_environment',
}
# Frozen against independent complete draft observations and both checked pins.
VERIFIED = [('control-rest-recursion', '997c8a7efa865b3043beaa71947eb81a81c9293d78bd86bbca6155466d49af86', '39278d46804fca2a6fe36fa26518800745db8e60a4a16bceae2f66281f5448d3'), ('exception-interop-template', 'e754658dcd21fcbfccfa0d56ea1cf84ee6b089af0801e6e8bb3ffb723a44ea50', '87d54c507160d746bd68f549aafd33abccb1527fdd60ae2a55bb97e41edb0c12'), ('quote-case-star', 'f3a288a61b8676d0e9b1a1649ee81ab8dcde22f9f396b9f1107e3942a9c99d3f', 'e0dacc314c4d83c6bcb075abb361d81db32f7f3dd382877fa2c182471678920f'), ('definition-var-state', '746b4b04eaa8fb9bad7a0102625789d2716dc18882e8c6b17648ee2800cf75e0', '4b1fccc2a930dcd8684bf27e7baa97c3f10624c6430391a46d218f3af29b94ad'), ('raw-type-constructor', 'eb86c15afca737729b2398d32a44ff41f307ca5a54d7324376295d3ea5c61970', '16f58b61c90f64c2b163b00182634fc95550247f06acdb3aaa679f4391daa0a5'), ('raw-record-constructor', '5b56418c63d21cc46ca351aa19fb2527b2cf5e92dac828391d26f246820e07b6', '365c4ec9be39188ac774622f54fe4a4a32b1d80f182c0ec45e99cedfc2e902e2'), ('namespace-header-context', 'c2204dec28a9f0c550ba7ced96bb46f85bf71058c40ecb0c49a2b4166b29a02e', 'b5bea41b6c623f7c09f1bf24dcae58ebab3c0cdd90ad966bc43a45b44867e12b')]
REVIEWED_NAMES = {'control-rest-recursion': ['if', 'fn*', 'do', 'let*', 'loop*', 'letfn*', 'recur', '&'], 'exception-interop-template': ['new', 'set!', '.', 'js*', 'try', 'throw'], 'quote-case-star': ['quote', 'case*'], 'definition-var-state': ['def', 'var'], 'raw-type-constructor': ['deftype*'], 'raw-record-constructor': ['defrecord*'], 'namespace-header-context': ['ns']}
SPECIALS_HASH = '66086cdf742d79f61db62e93a5b235e321a8e8058fdad7f0ad5bd01d16d4343d'
def digest(value):
 return hashlib.sha256(value if isinstance(value,bytes) else value.encode()).hexdigest()
def active_syntax(source):
 """Only active form heads, quote operators and actual function rest bindings.
 This structural safeguard is never execution or branch coverage evidence.
 """
 found=set()
 def visit(node):
  if node.kind in {'discard','syntax-quote','string','regex','tagged'}: return
  if node.kind=='metadata': visit(node.children[-1]); return
  if node.kind=='quote': found.add('quote'); return
  if node.kind=='var': found.add('var'); return
  if node.kind=='list' and node.children:
   head=node.children[0].token
   if head=='quote': found.add('quote'); return
   found.add(head)
   if head in {'fn','fn*'}:
    for child in node.children[1:]:
     if child.kind=='vector' and any(c.kind=='atom' and c.token=='&' for c in child.children): found.add('&')
  for child in node.children: visit(child)
 for node in Reader(source).all(): visit(node)
 return found

def rows():
 result=[]
 for line in (ROOT/FIXTURE).read_text().splitlines():
  if not line or line.startswith('#'):continue
  identity,names,source,expected,metadata=line.split('\t')
  source=bytes.fromhex(source).decode();expected=bytes.fromhex(expected).decode();metadata=json.loads(metadata)
  if metadata['id']!=identity or metadata['source']!=source or metadata['expected_edn']!=expected or metadata['source_sha256']!=digest(source) or metadata['syntax_names']!=names.split(','):raise ValueError('detached syntax source/expectation')
  if names.split(',')!=REVIEWED_NAMES[identity]:raise ValueError('reviewed contextual syntax contract drift')
  if not set(names.split(','))<=active_syntax(source):raise ValueError('syntax mapping needs active contextual forms')
  result.append(metadata)
 actual=[(r['id'],digest(r['source']),digest(r['expected_edn'])) for r in result]
 if actual!=VERIFIED:raise ValueError('whole verified syntax workflow drift')
 return result

def tree_hashes(root):
 return {str(p.relative_to(root)):digest(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file()}
def cases():
 result=[]
 for row in rows():
  identity=PREFIX+row['id']
  source=row['source'] if row['id']=='namespace-header-context' else "(= '"+row['expected_edn']+' (do '+row['source']+'))'
  case={'id':identity,'source':source,'expected':{'id':identity,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''},'native_stdout':'true\n'}
  if row['id']=='namespace-header-context':
   for key in ['source_root','reference_source_root','entry','export','reference_adaptation']:case[key]=row[key]
   case['reference_files_sha256']=tree_hashes(ROOT/case['reference_source_root'])
   local=ROOT/case['source_root'];reference=ROOT/case['reference_source_root']
   prefix,body=source.split('(defn run [observe] ',1);body=body.removesuffix(')\n(pr-str (run nil))\n')
   if row['macro_source']!=prefix+'(defmacro syntax-gate [] '+body+')\n(pr-str (syntax-gate))\n':raise ValueError('actual macro body drift')
   if (local/'syntax/header.cljs').read_text()!=source:raise ValueError('local namespace body drift')
   adapter=source.replace('(:require [clojure.string :as text] [syntax.observer :refer [caller]])','(:require [clojure.string :as text]) (:require-macros [syntax.observer :refer [caller]])')
   if (reference/'syntax/header.cljs').read_text()!=adapter:raise ValueError('namespace adapter changed original body')
   companion='(ns syntax.observer)\n(defmacro caller [] (str (get-in &env [:ns :name])))\n'
   for folder in [local,reference]:
    for suffix in ['cljs','clj']:
     if (folder/f'syntax/observer.{suffix}').read_text()!=companion:raise ValueError('namespace macro companion drift')
  result.append(case)
 return result

def exports():
 workflows=rows();oracles=cases();targets={r['id']:r for r in map(json.loads,(HERE/'obligations.jsonl').read_text().splitlines())}
 ordinary=[];phases=[];reviewed=[]
 support={FIXTURE:digest((ROOT/FIXTURE).read_bytes()),'tests/reference/compiler-language/syntax_context.py':digest(Path(__file__).read_bytes())}
 for root in ['local','reference']:
  folder=ROOT/'tests/fixtures/compiler-syntax-context'/root
  support.update({str(p.relative_to(ROOT)):digest(p.read_bytes()) for p in folder.rglob('*') if p.is_file()})
 for row,oracle in zip(workflows,oracles):
  test=TESTS[row['id']]
  if not re.search(r'#\[test\]\s*fn\s+'+test+r'\s*\(', (ROOT/DRIVER).read_text()):raise ValueError('missing named actual-phase driver')
  for name in row['syntax_names']:
   target=targets['syntax::'+name]
   if target['body_sha256']!=SPECIALS_HASH or target['signature']!={'kind':'syntax','minimum':None}:raise ValueError('canonical specials identity drift')
   common={'obligation_id':target['id'],'body_sha256':target['body_sha256'],'signature_sha256':digest(json.dumps(target['signature'],sort_keys=True)),'rationale':'Complete '+row['id']+' source executes '+name+' with the independent literal value and ordered effect/error observations in both application engines and an actual MacroHost body. Full setup, raw constructor/control semantics, and namespace macro environment are retained; form presence alone supplies no pass.'}
   execution={'test_file':DRIVER,'test_name':test,'test_source_sha256':digest((ROOT/DRIVER).read_bytes())}
   ordinary.append({**common,'assertion_ids':[],'local_execution':execution,'reference_fixture_id':oracle['id'],'reference_source_sha256':digest(oracle['source']),'reference_contract_sha256':digest(json.dumps(oracle['expected'],sort_keys=True))})
   phases.append({**common,**execution,'support_source_sha256':support})
   reviewed.append({'obligation_id':target['id'],'whole_workflow':row['id'],'source_sha256':digest(row['source']),'literal_expected_sha256':digest(row['expected_edn'])})
 if len(ordinary)!=21 or len({r['obligation_id'] for r in ordinary})!=21:raise ValueError('ordered complete syntax cohort drift')
 return {'obligation-witnesses.jsonl':ordinary,'phase-witnesses.jsonl':phases,'full-cases.jsonl':oracles,'syntax-context-receipts.jsonl':reviewed}

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--export-owned',type=Path,required=True);args=parser.parse_args();args.export_owned.mkdir(parents=True,exist_ok=True)
 for filename,records in exports().items():(args.export_owned/filename).write_text(''.join(json.dumps(r,separators=(',',':'))+'\n' for r in records))
 print('7 full workflows;21 contextual syntax candidates;ns* remains a separately verified gap')
if __name__=='__main__':main()
