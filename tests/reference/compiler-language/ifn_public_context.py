#!/usr/bin/env python3
"""Source-bound genuine IFn receiver contracts; export only, never shared writes.

Public -invoke rows denote demanded IFn method dispatch, with the receiver in
canonical signatures. Explicit -invoke calls and native function controls are
not inferred as equivalent. Actual named tests and both pin oracles must pass.
"""
from pathlib import Path
import argparse
import hashlib
import json
import re
from extract_inventory import Reader
ROOT=Path(__file__).resolve().parents[3]
HERE=ROOT/'tests/reference/compiler-language'
DRIVER='tests/compiler_ifn_public_context.rs'
FIXTURE='tests/fixtures/compiler-ifn-public-context.tsv'
HELPER='tests/reference/compiler-language/ifn_public_context.py'
PREFIX='ifn-public-context-'
TESTS={
 'callable-fixed-0-6':'ifn_receiver_zero_through_six_execute_in_both_phases',
 'callable-fixed-7-14':'ifn_receiver_seven_through_fourteen_execute_in_both_phases',
 'callable-fixed-15-20':'ifn_receiver_fifteen_through_twenty_execute_in_both_phases',
 'callable-receiver-rest-22':'ifn_receiver_terminal_rest_slot_executes_in_both_phases',
 'callable-receiver-copy-failure':'ifn_receiver_metadata_failure_and_recovery_execute_in_both_phases',
}
VERIFIED=[('callable-fixed-0-6', 'eb324e43f79be577a87f1b4246b002e68ff888a679c1eae86365c1d15c5c2163', 'f1b556555306069ebba7a8513b99af1dd7375948bd25eba46d64d945b2dd49a1', [1, 2, 3, 4, 5, 6, 7]), ('callable-fixed-7-14', '152ddc1adfe0cfb550e914154ff2f08a8928f9409a5d73b82a284ff599205784', '4a2bea155f683330e461e16583af7a836e101458142d3b1b620f08b5b69fd8b6', [8, 9, 10, 11, 12, 13, 14, 15]), ('callable-fixed-15-20', '0ee71e37528c20a78ea14c4d73ca7f2915ff5fae26e92cee7521163b97d45b9b', '2628c4bf2ba545d27b273920a06bf71596598ccf72d87990bdeca7adae30b710', [16, 17, 18, 19, 20, 21]), ('callable-receiver-rest-22', '3019a3bfcd83702186f37eea23448f41fa8484bc470e01e2d492d237e2f6c75b', '6836a8f7568373b0a20bb0fa8b5a1eb169f281fc1dda6fdc09ac03b511c9072e', [22]), ('callable-receiver-copy-failure', '214ba8207269178115f1e73cbed8440dc80a25234d602b2ac296609f27f455bb', '6034b73a79e26f495b6a293f338cb46e40dfd7f1b119b9c1011978615420280d', [2])]
def digest(value):return hashlib.sha256(value if isinstance(value,bytes) else value.encode()).hexdigest()
def head(node):return node.children[0].token if node.kind=='list' and node.children else ''
def receiver_demands(source):
 """Review only known genuine IFn receivers and their saved/metadata aliases.
 Declarations establish available slots, never demanded dispatch by themselves.
 """
 aliases={};methods={};calls=[]
 def walk(node):
  if node.kind in {'quote','syntax-quote','discard','string','regex','tagged'}:return
  if node.kind=='metadata':walk(node.children[-1]);return
  name=head(node)
  if name=='quote':return
  if name in {'let','let*'}:
   binding=node.children[1]
   if binding.kind!='vector' or len(binding.children)%2:raise ValueError('invalid reviewed receiver bindings')
   for key,value in zip(binding.children[::2],binding.children[1::2]):
    if head(value)=='reify' and value.children[1].token=='IFn':
     declared={}
     for method in value.children[2:]:
      if head(method)!='-invoke':continue
      params=method.children[1]
      if params.kind!='vector' or not params.children or any(c.token=='&' for c in params.children):raise ValueError('unreviewed IFn method shape')
      n=len(params.children)-1
      if n in declared:raise ValueError('duplicate receiver method')
      declared[n]=digest(source[method.start:method.end])
     aliases[key.token]=key.token;methods[key.token]=declared
    elif value.kind=='atom' and value.token in aliases:aliases[key.token]=aliases[value.token]
    elif head(value)=='with-meta' and value.children[1].token in aliases:aliases[key.token]=aliases[value.children[1].token]
    else:walk(value)
   for child in node.children[2:]:walk(child)
   return
  if name in {'reify','deftype','defrecord','fn','fn*','defmacro'}:return
  if name in aliases:
   receiver=aliases[name];n=len(node.children)-1
   if n not in methods[receiver]:raise ValueError('call lacks exact receiver method')
   calls.append({'receiver':receiver,'alias':name,'kind':'direct-or-saved','user_argument_count':n,'public_receiver_arity':n+1,'method_source_sha256':methods[receiver][n],'call_source_sha256':digest(source[node.start:node.end])})
  elif name=='.call' and len(node.children)>=3 and head(node.children[1])=='aget':
   getter=node.children[1];alias=getter.children[1].token;bound=node.children[2].token
   if alias not in aliases or bound not in aliases or aliases[alias]!=aliases[bound]:raise ValueError('slot lacks the genuine bound receiver')
   if getter.children[2].kind!='string':raise ValueError('dynamic unreviewed IFn slot')
   key=json.loads(source[getter.children[2].start:getter.children[2].end]);match=re.fullmatch(r'cljs\$core\$IFn\$_invoke\$arity\$(\d+)',key)
   if not match:raise ValueError('wrong canonical IFn slot')
   n=int(match.group(1));receiver=aliases[alias]
   if n not in methods[receiver] or len(node.children)-3!=n:raise ValueError('wrong canonical IFn slot formal count')
   calls.append({'receiver':receiver,'alias':alias,'kind':'explicit-compiler-slot','compiler_dispatch_slot':key,'user_argument_count':n,'public_receiver_arity':n+1,'method_source_sha256':methods[receiver][n],'call_source_sha256':digest(source[node.start:node.end])})
  elif name in {'apply','cljs.core/apply'} and len(node.children)==3 and node.children[1].token in aliases:
   receiver=aliases[node.children[1].token];args=node.children[2]
   if args.kind!='vector':raise ValueError('unreviewed receiver apply tail')
   n=len(args.children);slot=n if n<=20 else 21
   if n>20 and any(c.kind!='atom' or not c.token.isdigit() for c in args.children):raise ValueError('terminal apply tail must preserve reviewed flattened inputs')
   if slot not in methods[receiver]:raise ValueError('apply lacks exact receiver slot')
   calls.append({'receiver':receiver,'alias':node.children[1].token,'kind':'apply','user_argument_count':n,'public_receiver_arity':slot+1,'method_source_sha256':methods[receiver][slot],'call_source_sha256':digest(source[node.start:node.end])})
  for child in node.children:walk(child)
 for node in Reader(source).all():walk(node)
 return calls

def rows():
 result=[]
 for line in (ROOT/FIXTURE).read_text().splitlines():
  if not line or line.startswith('#'):continue
  identity,arities,source,expected,metadata=line.split('\t');source=bytes.fromhex(source).decode();expected=bytes.fromhex(expected).decode();metadata=json.loads(metadata)
  if metadata!={'source_sha256':digest(source)}:raise ValueError('whole receiver source provenance drift')
  declared=list(map(int,arities.split(',')));demands=receiver_demands(source)
  if sorted({r['public_receiver_arity'] for r in demands if r['kind']=='explicit-compiler-slot'})!=declared:raise ValueError('exact IFn compiler slot was not executed')
  if sorted({r['public_receiver_arity'] for r in demands})!=declared:raise ValueError('declared receiver arity lacks actual demands')
  result.append({'id':identity,'source':source,'expected_edn':expected,'public_arities':declared,'receiver_demands':demands})
 actual=[(r['id'],digest(r['source']),digest(r['expected_edn']),r['public_arities']) for r in result]
 if actual!=VERIFIED:raise ValueError('independently checked complete receiver workflow drift')
 return result

def cases():
 result=[]
 for row in rows():
  identity=PREFIX+row['id'];source="(= '"+row['expected_edn']+' (do '+row['source']+'))'
  result.append({'id':identity,'source':source,'expected':{'id':identity,'outcome':{'status':'returned','value':True},'trace':[],'stdout':''},'native_stdout':'true\n'})
 return result

def exports():
 workflows=rows();oracles={r['id']:r for r in cases()};byarity={}
 for row in workflows:
  for n in row['public_arities']:byarity[n]=row
 targets={r['id']:r for r in map(json.loads,(HERE/'obligations.jsonl').read_text().splitlines())}
 prefix='api::src/main/cljs/cljs/core.cljs::'
 selected=[(prefix+'-invoke::arity-'+str(n),byarity[n]) for n in range(1,23)]
 selected.extend([(prefix+'IFn::arity-1',workflows[0]),('local::src/core_var_metadata.tsv::IFn',workflows[0]),('local::src/core_var_metadata.tsv::-invoke',workflows[-1])])
 support={name:digest((ROOT/name).read_bytes()) for name in [FIXTURE,HELPER]}
 ordinary=[];phase=[];receipts=[]
 for identity,row in selected:
  target=targets[identity];test=TESTS[row['id']];oracle=oracles[PREFIX+row['id']]
  if not re.search(r'#\[test\]\s*fn\s+'+test+r'\s*\(', (ROOT/DRIVER).read_text()):raise ValueError('missing exact actual phase test')
  signature=target.get('signature')
  if target['name']=='-invoke' and signature and signature['minimum'] not in row['public_arities']:raise ValueError('canonical receiver signature drift')
  if identity.startswith(prefix) and target['body_sha256']!='229cbe05e42077c3010ce220532bce39cee20c8c7ac31328547e674001a7050c':raise ValueError('canonical IFn body drift')
  rationale='The complete '+row['id']+' workflow demands genuine reify IFn receiver methods through direct callable, saved receiver, apply and explicit receiver-bound compiler-slot paths, with exact receiver-inclusive canonical signatures. Adjacent independent literals retain full values, receiver identity, live fields, ordered effects, metadata copies and caught failure/recovery observations. Explicit -invoke/native-function controls supply no pass, and method definitions alone supply no demanded dispatch. Local API rows are operation cross-checks, not complete metadata-value claims.'
  common={'obligation_id':identity,'body_sha256':target['body_sha256'],'rationale':rationale}
  if signature:common['signature_sha256']=digest(json.dumps(signature,sort_keys=True))
  execution={'test_file':DRIVER,'test_name':test,'test_source_sha256':digest((ROOT/DRIVER).read_bytes())}
  ordinary.append({**common,'assertion_ids':[],'local_execution':execution,'reference_fixture_id':oracle['id'],'reference_source_sha256':digest(oracle['source']),'reference_contract_sha256':digest(json.dumps(oracle['expected'],sort_keys=True))})
  phase.append({**common,**execution,'support_source_sha256':support})
  receipts.append({'obligation_id':identity,'whole_workflow':row['id'],'complete_source_sha256':digest(row['source']),'literal_expected_sha256':digest(row['expected_edn']),'actual_receiver_demands':row['receiver_demands']})
 if len(ordinary)!=25 or len({r['obligation_id'] for r in ordinary})!=25:raise ValueError('incomplete receiver cohort')
 return {'obligation-witnesses.jsonl':ordinary,'phase-witnesses.jsonl':phase,'full-cases.jsonl':list(oracles.values()),'ifn-public-context-receipts.jsonl':receipts}

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--export-owned',type=Path,required=True);a=p.parse_args();a.export_owned.mkdir(parents=True,exist_ok=True)
 for name,records in exports().items():(a.export_owned/name).write_text(''.join(json.dumps(r,separators=(',',':'))+'\n' for r in records))
 print('5 complete receiver workflows;25 source-bound ordinary/actual-phase candidates;no shared table writes')
if __name__=='__main__':main()
