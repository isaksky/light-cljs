#!/usr/bin/env python3
"""Retain complete residual upstream bodies and setup; generation claims no pass.

Copyright (c) Rich Hickey. Eclipse Public License 1.0. Original notices remain
in research/clojurescript, including epl-v10.html; retain this notice.
"""
from pathlib import Path
import hashlib
import importlib.util
import json
import subprocess
import sys
ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tests/reference/compiler-language'
sys.path.insert(0, str(HERE))
import extract_inventory as inv
spec = importlib.util.spec_from_file_location('residual_adapter', ROOT / 'tests/reference/compiler-sequence-completion.py')
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)
BASE = adapter.adapt
PREFIX = 'cljs-residual-body-'
SETUP = {
 'core_test.cljs::test-into+halt-when': [],
 **{'src/test/cljs/cljs/reader_test.cljs::'+name:[] for name in ['test-reader','test-717','test-787','testing-map-type','test-slash-reading','testing-cljs-1823','testing-cljs-3278','testing-cljs-3291']},
 'core_test.cljs::test-816': [],
 'core_test.cljs::test-map-new-transducers': [],
 'core_test.cljs::test-cljs-1284': ['core_test.cljs::foo-1284'],
 'core_test.cljs::test-cljs-1590': [],
 'core_test.cljs::test-cljs-1780': ['core_test.cljs::CLJS1780'],
 'core_test.cljs::test-cljs-1837': [],
 'core_test.cljs::test-cljs-2455': ['core_test.cljs::Foo2455'],
 'core_test.cljs::test-cljs-2552': [],
 'core_test.cljs::test-cljs-3054': [],
 'src/test/cljs/cljs/specials_test.cljs::test-cljs-3125': [
  'src/test/cljs/cljs/specials_test.cljs::IFoo3125',
  'src/test/cljs/cljs/specials_test.cljs::Foo3125',
  'src/test/cljs/cljs/specials_test.cljs::*test-cljs-3125*'],
}
RATIONALE = ('Complete original body and every relevant setup form, original literal assertions, messages, loops and effects retained. '
             'Only cljs.test is/are/testing/thrown-with-msg? forms become executing assertions, and original library/reader/host aliases are fully qualified. '
             'Original test-717 Closure o/get calls for present string keys become identical ordinary ECMAScript aget reads, following the reviewed equality adapter; this proves reader results/property access, not general Closure support. No assertion, input, exception class or regular expression is replaced. Both ordinary and macro computations execute this complete adapter; canonical references execute its complete ordinary contract.')
def read(name):
 return [json.loads(l) for l in (HERE/name).read_text().splitlines() if l]
def update_owned(name,owned,new_rows):
 # Shared catalogs have independent generators. Validate only this adapter's
 # rows and preserve unrelated blocks/order when replacing its existing block.
 old=read(name)
 original=[row for row in old if owned(row)]
 if '--write' not in sys.argv:
  assert original==new_rows, 'Residual owned catalog rows drift: '+name
 elif original!=new_rows:
  first=next((i for i,row in enumerate(old) if owned(row)),len(old))
  result=old[:first]+new_rows+[row for row in old[first:] if not owned(row)]
  (HERE/name).write_text(''.join(json.dumps(row,separators=(',',':'))+'\n' for row in result))
def adapt(text,node):
 if inv.head(node)=='o/get':
  # Closure get over the existing explicit keys is the identical ordinary
  # ECMAScript property read, following the reviewed equality adapter.
  return '(aget '+' '.join(adapt(text,c) for c in node.children[1:])+')'
 if inv.short_head(node) == 'thrown-with-msg?':
  cls,pattern,*forms = node.children[1:]
  return '(try ' + ' '.join(adapt(text,c) for c in forms) + ' false (catch ' + adapt(text,cls) + ' residual-error (boolean (re-find ' + adapt(text,pattern) + ' (.-message residual-error)))))'
 if not node.children and node.kind == 'atom':
  atom=text[node.start:node.end]
  for alias,ns in [('set/','clojure.set/'),('s/','clojure.string/'),('reader/','cljs.reader/'),('o/','goog.object/')]:
   if atom.startswith(alias):return ns+atom[len(alias):]
 return BASE(text,node)
adapter.adapt=adapt

def generated():
 bodies={b['id']:b for b in read('full-bodies.jsonl')}
 inventory=read('full-inventory.jsonl')
 existing={w['assertion_id'] for n in ('witnesses.jsonl','full-witnesses.jsonl') for w in read(n) if not w['fixture_id'].startswith(PREFIX)}
 rows,cases,witnesses=[],[],[]
 for i,(identity,setup) in enumerate(SETUP.items()):
  originals=[bodies[s] for s in setup]+[bodies[identity]]
  forms,provenance=[],[]
  for original in originals:
   span=original['source_span'];text=span['text']
   pinned=subprocess.check_output(['git','-C',str(ROOT/'research/clojurescript'),'show', inv.REVISION+':'+original['source']])
   assert hashlib.sha256(text.encode()).hexdigest()==span['sha256']
   assert pinned[span['start_byte']:span['end_byte']].decode()==text
   forms.append(adapt(text,inv.Reader(text).all()[0]))
   provenance.append({'body_id':original['id'],'source_sha256':span['sha256']})
  source=('(do '+' '.join(forms)+')').replace('\n',' ').replace('\t',' ')
  case_id=PREFIX+str(i)
  rows.append([case_id,source,'true',json.dumps(provenance,separators=(',',':'))])
  root='tests/fixtures/compiler-residual-bodies';entry='residualbody.case'+str(i)
  program=';; Copyright (c) Rich Hickey. Eclipse Public License 1.0.\n;; See research/clojurescript/epl-v10.html; retain this notice.\n(ns '+entry+' (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))\n(defn run [] '+source+')\n'
  path=ROOT/root/(entry.replace('.','/')+'.cljs')
  if '--write' in sys.argv:path.parent.mkdir(parents=True,exist_ok=True);path.write_text(program)
  else:assert path.read_text()==program
  cases.append({'id':case_id,'source':program,'source_root':root,'entry':entry,'export':'run','expected':{'id':case_id,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''},'native_stdout':None})
  for row in inventory:
   if row['body_id']==identity and row['disposition']=='in-scope' and row['id'] not in existing:
    witnesses.append({'assertion_id':row['id'],'assertion_sha256':row['assertion']['sha256'],'fixture_id':case_id,'rationale':RATIONALE})
 # Complete existing equality workflows retain helper assertions, every
 # repeated interface input and the reviewed private-tree storage adapters.
 # Assertion-free caller bodies must still receive real executed oracles.
 equality_provenance=json.loads((ROOT/'tests/reference/compiler-equality-provenance.json').read_text())
 for line in (ROOT/'tests/fixtures/compiler-equality.tsv').read_text().splitlines():
  if line.startswith('#') or not line:continue
  identity,source,expected,*extra=line.split('\t')
  if identity not in {'cljs-equality-metadata-test-metadata-tests','cljs-equality-map-entry-test-all-map-entry-tests'}:continue
  case_id=PREFIX+identity
  rows.append([case_id,source,expected,json.dumps({'existing_complete_fixture':identity,'fixture_row_sha256':inv.digest(line.encode()),'provenance_sha256':inv.digest((ROOT/'tests/reference/compiler-equality-provenance.json').read_bytes())},separators=(',',':'))])
  values=expected.removeprefix('[').removesuffix(']').split()
  assert all(v in {'true','false'} for v in values)
  cases.append({'id':case_id,'source':source,'expected':{'id':case_id,'outcome':{'status':'returned','value':{'$vector':[v=='true' for v in values]}},'trace':[],'stdout':''},'native_stdout':None})
 return rows,cases,witnesses

def operation_mappings(rows):
 # Each selected whole body executes every assert. Direct calls in original
 # and adapter must agree; quoted/discarded forms and thread-injected arities
 # cannot manufacture an operation witness.
 bodies={b['id']:b for b in read('full-bodies.jsonl')}
 def calls(text):
  found=set()
  def visit(node):
   if node.kind in {'quote','syntax-quote','discard','var'}:return
   head=inv.head(node)
   if head=='quote':return
   if head.startswith('set/'):head='clojure.set/'+head[4:]
   if head.startswith('s/'):head='clojure.string/'+head[2:]
   if node.kind=='list':found.add((head,len(node.children)-1))
   if head in {'->','->>','some->','some->>','cond->','cond->>','as->'}:return
   for child in node.children:visit(child)
  for node in inv.Reader(text).all():visit(node)
  return found
 existing={m['obligation_id'] for m in read('obligation-witnesses.jsonl') if not m.get('rationale','').startswith('Complete residual body ')}
 prior_phases={m['obligation_id'] for m in read('phase-witnesses.jsonl') if m.get('test_file')!='tests/compiler_residual_bodies.rs'}
 test_file='tests/compiler_residual_bodies.rs'
 test_hash=inv.digest((ROOT/test_file).read_bytes())
 fixture='tests/fixtures/compiler-residual-bodies.tsv'
 support={fixture:inv.digest((ROOT/fixture).read_bytes())}
 mappings,phases=[],[]
 for obligation in read('obligations.jsonl'):
  if obligation['disposition']!='intended' or obligation['obligation_kind']!='public-api' or obligation['id'] in existing:continue
  if obligation['source']=='src/main/cljs/cljs/core.cljs':name=obligation['name']
  elif obligation['source']=='src/main/cljs/clojure/set.cljs':name='clojure.set/'+obligation['name']
  elif obligation['source']=='src/main/cljs/clojure/string.cljs':name='clojure.string/'+obligation['name']
  else:continue
  signature=obligation['signature']
  for row,(identity,setup) in zip(rows,SETUP.items()):
   original=bodies[identity]['source_span']['text']
   direct=calls(original)&calls(row[1])
   if not any(n==name and (arity==signature['minimum'] if signature['kind']=='fixed' else arity>=signature['minimum']) for n,arity in direct):continue
   common={'obligation_id':obligation['id'],'body_sha256':obligation['body_sha256'],'signature_sha256':inv.digest(json.dumps(signature,sort_keys=True).encode()),
    'rationale':'Complete residual body '+identity+' directly executes '+name+' at this exact public signature with every original assertion/setup retained; complete ordinary reference fixture and normal Node/QuickJS application/macro test required.'}
   local={'test_file':test_file,'test_name':'complete_residual_bodies_execute_in_both_phases','test_source_sha256':test_hash,'support_source_sha256':support}
   case=next(c for c in read('full-cases.jsonl') if c['id']==row[0])
   assertion_ids=[r['id'] for r in read('full-inventory.jsonl') if r['body_id']==identity and r['disposition']=='in-scope']
   mappings.append({**common,'assertion_ids':assertion_ids})
   if obligation['id'] not in prior_phases:phases.append({**common,**local})
   break
 for identity,fixture_id in [('metadata_test.cljc::metadata-tests',PREFIX+'cljs-equality-metadata-test-metadata-tests'),('map_entry_test.cljs::all-map-entry-tests',PREFIX+'cljs-equality-map-entry-test-all-map-entry-tests')]:
  target=next(r for r in read('obligations.jsonl') if r['id']=='body::'+identity)
  phase=next(r for r in read('phase-witnesses.jsonl') if r['obligation_id']==target['id'])
  local={k:phase[k] for k in ('test_file','test_name','test_source_sha256')}
  case=next(r for r in read('full-cases.jsonl') if r['id']==fixture_id)
  mappings.append({'obligation_id':target['id'],'body_sha256':target['body_sha256'],'local_execution':local,
   'reference_fixture_id':fixture_id,'reference_source_sha256':inv.digest(case['source'].encode()),'reference_contract_sha256':inv.digest(json.dumps(case['expected'],sort_keys=True).encode()),
   'rationale':'Complete residual body '+identity+' retains all original caller/helper paths through the complete already reviewed equality adapter; exact existing application/macro test and both pinned boolean-vector oracles are required. Private tree-color construction remains the explicit public-sorted-map-entry adaptation documented in compiler-equality.md.'})
 return mappings,phases

def main():
 rows,cases,witnesses=generated()
 text='# Copyright (c) Rich Hickey. Eclipse Public License 1.0.\n# id\tsource\texpected\tcomplete original body/setup hashes\n'+''.join('\t'.join(r)+'\n' for r in rows)
 fixture=ROOT/'tests/fixtures/compiler-residual-bodies.tsv'
 if '--write' in sys.argv:fixture.write_text(text)
 else:assert fixture.read_text()==text
 for name,owned,key in [('full-cases.jsonl',cases,'id'),('full-witnesses.jsonl',witnesses,'fixture_id')]:
  update_owned(name,lambda row:row[key].startswith(PREFIX),owned)
 mappings,phases=operation_mappings(rows)
 for name,new_rows in [('obligation-witnesses.jsonl',mappings),('phase-witnesses.jsonl',phases)]:
  linked_bodies={'body::metadata_test.cljc::metadata-tests','body::map_entry_test.cljs::all-map-entry-tests'} if name=='obligation-witnesses.jsonl' else set()
  owned=lambda r:r.get('obligation_id') in linked_bodies or r.get('test_file')=='tests/compiler_residual_bodies.rs' or r.get('rationale','').startswith('Complete residual body ')
  update_owned(name,owned,new_rows)
 print(f'{len(rows)} complete bodies, {len(witnesses)} new original assertion mappings; execution required')
if __name__=='__main__':main()
