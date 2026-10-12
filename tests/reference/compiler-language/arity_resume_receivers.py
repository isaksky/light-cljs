#!/usr/bin/env python3
"""Whole demanded transient and sorted receiver exports; no shared catalog writes."""
from pathlib import Path
import argparse
import json
import re
from arity_resume_candidates import digest, direct_calls
from extract_inventory import Reader
ROOT=Path(__file__).resolve().parents[3]
HERE=ROOT/'tests/reference/compiler-language'
HELPER='tests/reference/compiler-language/arity_resume_receivers.py'
SCANNER='tests/reference/compiler-language/arity_resume_candidates.py'
SCANNER_SHA256='31457848fe6d2f08e92b3ad5b4a6f99362a97be66e4819c9d45cf3a1b9ecbcc3'
PREFIX='arity-receiver-'

REVIEWED = [{'id': 'transient',
  'driver': 'tests/compiler_transients.rs',
  'test': 'transient_api_arities_protocols_and_macro_phase_execute',
  'complete_source_sha256': '9954135ba16ac91e5faebdebd5a2b024321a17ed155cb0be8e1efc1109906180',
  'expected_literal': '[[1 20] :last 2 [:transient [:conj 1] [:assoc :a 2] [:dissoc :a] [:assoc-n '
                      '0 4] :pop [:disj 1] :persistent]]',
  'demand_basis': 'Every genuine custom transient receiver method is directly demanded and its '
                  'complete ordered event vector is asserted. Native transient collections, '
                  'aliases, mutation identity, after-persistent failures, hashing, metadata, '
                  'lookup and zero/one-argument conj! remain in the full original contract.'},
 {'id': 'sorted-full',
  'driver': 'tests/compiler_sorted_contracts.rs',
  'test': 'sorted_contract_executes_in_application_and_macro_phases',
  'complete_source_sha256': 'da4a60bb2578210075eb03dfac88011d31d57caf642d9b588e33bb9a796fb309',
  'expected_literal': 'true',
  'demand_basis': 'Genuine sorted map/set receivers are traversed in both directions, including '
                  'from-key and entry-key operations. Original custom '
                  'comparator/representative-key identity, callback effects, metadata, hashing, '
                  'scalar boundaries, constructor slots, caught invalid controls and saved '
                  'subseq/rsubseq loops remain in full.'}]
SELECTION = [{'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-as-transient::arity-1',
  'workflow': 'transient',
  'body_sha256': '224e74ae2594ed199e6d75284f2c50578243b4721ab906c4e1050ff88025857e',
  'signature_sha256': '7e858f7b7ea7a24bf6340aa43b8567afa8e81dcdda941b22e94e72485515449d',
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-conj!::arity-1',
  'workflow': 'transient',
  'body_sha256': '8da0a1d06ad99fd6f778f1eb5ab53be6a50cb7a9927f286e29fa4c02fcbb4c5d',
  'signature_sha256': 'a4263750eb44556a6422867e3c1296d42f47159b8ee32447685c485b997ee56c',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-persistent!::arity-1',
  'workflow': 'transient',
  'body_sha256': '8da0a1d06ad99fd6f778f1eb5ab53be6a50cb7a9927f286e29fa4c02fcbb4c5d',
  'signature_sha256': '66917b332153e27cae983a24ab5edfdbfc0462ccfdeabe1c9e771489f2aded81',
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-assoc!::arity-1',
  'workflow': 'transient',
  'body_sha256': '4a125d21134326f4d165048296682b8e94d9b99fb5dc3ee7f6bf9170f1a05d60',
  'signature_sha256': '35cca0413fd4475da624795c14905dd8390092bd59b559c5453c216c056bbda5',
  'direct_arities': [3]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-dissoc!::arity-1',
  'workflow': 'transient',
  'body_sha256': '9aedbc443395aee3bb157f3fbeab91205c89ec8c5234f55ad2b671122b765bfb',
  'signature_sha256': 'e4013c998b52984f025e677a90dead21b4573dea2f8e0c2c20505357bec08af8',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-assoc-n!::arity-1',
  'workflow': 'transient',
  'body_sha256': '1664c2f3b4eabe68c2ab11e4f0c05ec1d2e343421f489c3551a2fd3b944e704d',
  'signature_sha256': '5f1d236d50866b0bac7b1fc38f51cd7693a6c0afb2b445cc4d5617aad6079005',
  'direct_arities': [3]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-pop!::arity-1',
  'workflow': 'transient',
  'body_sha256': '1664c2f3b4eabe68c2ab11e4f0c05ec1d2e343421f489c3551a2fd3b944e704d',
  'signature_sha256': '66917b332153e27cae983a24ab5edfdbfc0462ccfdeabe1c9e771489f2aded81',
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-disjoin!::arity-1',
  'workflow': 'transient',
  'body_sha256': 'ccae9ee350c3e7bbb71d98b0af1b5736d4f0a2cff9f9bf0a90e964175786d2e1',
  'signature_sha256': 'da6c3823bb40824f3aa7c45abdf452395dfaf4bf1dfffa8aa846d4ec7bd84e2c',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::conj!::arity-1',
  'workflow': 'transient',
  'body_sha256': '7268fae1f200a3f5ce62cd65efeede08eaa95fdc39f663b6ad5a182c23dd713b',
  'signature_sha256': 'c04963f87d82d63dd1d5239f487499f7c8f6ac7f34ad0470e03c0c5f4f28a5ec',
  'direct_arities': [0]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::conj!::arity-2',
  'workflow': 'transient',
  'body_sha256': '7268fae1f200a3f5ce62cd65efeede08eaa95fdc39f663b6ad5a182c23dd713b',
  'signature_sha256': '66917b332153e27cae983a24ab5edfdbfc0462ccfdeabe1c9e771489f2aded81',
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-as-transient',
  'workflow': 'transient',
  'body_sha256': '8d2cb49c55edf36ef9c9985c08bc6bf847a65d3fde8f6a7ea5d6717be0c0e445',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-assoc!',
  'workflow': 'transient',
  'body_sha256': 'a407e309c0dbcaa53c3ead032bb91ede17650bd255203ece9603b8ea11c60b1e',
  'signature_sha256': None,
  'direct_arities': [3]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-assoc-n!',
  'workflow': 'transient',
  'body_sha256': 'c25b0b596727fbad311ea130616ab2c373a0bf4ea965c65f7b7c553c93c51887',
  'signature_sha256': None,
  'direct_arities': [3]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-conj!',
  'workflow': 'transient',
  'body_sha256': 'b5b5e1d9e1fd28053b3e5fda851d1576706f796c1764f3b94795858871a44ea5',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-disjoin!',
  'workflow': 'transient',
  'body_sha256': 'bcc3d61e5daf29c6322ce4036210dea1cd1d36e0dd37031d35db4c794be9c9a9',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-dissoc!',
  'workflow': 'transient',
  'body_sha256': '97c0c455cbcc7868338f3a30cf2401b11572b757b43164348b4dba3ad157b162',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-persistent!',
  'workflow': 'transient',
  'body_sha256': '092b3762c72d508313ecd84abbbd2c79f35e9494b7225a52403a6903093475c3',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-pop!',
  'workflow': 'transient',
  'body_sha256': 'db9cf4e0dcf6a3dff1b71a865f374b29cceefd7f88847a3ea2ff478d75db0a67',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-sorted-seq::arity-1',
  'workflow': 'sorted-full',
  'body_sha256': 'b1e207d553f428b54033a078b2093aa2b18babcd27020f13607938b070b0c7d1',
  'signature_sha256': 'ab0fa60d3e669f34f52202b87184d6163f233145d6d798c46d6ab1dbbc84b965',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-sorted-seq-from::arity-1',
  'workflow': 'sorted-full',
  'body_sha256': 'b1e207d553f428b54033a078b2093aa2b18babcd27020f13607938b070b0c7d1',
  'signature_sha256': 'a6cda1e453af343aa54bb8e75190f18582159ba209cad0e4e8b0f3d98e677c8c',
  'direct_arities': [3]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-entry-key::arity-1',
  'workflow': 'sorted-full',
  'body_sha256': 'b1e207d553f428b54033a078b2093aa2b18babcd27020f13607938b070b0c7d1',
  'signature_sha256': '948debdefa1397a91f5a7eccabf69223f8e42e5c6d86c22bff783c131b59aec0',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/clojure/cljs/core.cljc::doseq::arity-1',
  'workflow': 'sorted-full',
  'body_sha256': '5a8d6a5e2a3fceeca179ec04e759e3cdb41d8512d4b270c06236b224f6f4e2b9',
  'signature_sha256': '61f291cd0427483e74b34f25c2b570305f4625e2616be8dc753bb6ef9f3c7407',
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-comparator',
  'workflow': 'sorted-full',
  'body_sha256': 'bae059124f5f82a376175fa4109cf1966d587117123f19fd85e983846d3c3d8d',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-entry-key',
  'workflow': 'sorted-full',
  'body_sha256': '096aa68a7e7638464bf07e02201f2ea31becab18772d909a664b55777fa56394',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-sorted-seq',
  'workflow': 'sorted-full',
  'body_sha256': '670dd66060bf212f178bb568ee3b272a7dcd733076e16d3bf50fa2d1a47b69f0',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-sorted-seq-from',
  'workflow': 'sorted-full',
  'body_sha256': 'b7b72d53faaa4fdcd5635255f6313d6a0280848bd3f6495c6ef422f9229ab27e',
  'signature_sha256': None,
  'direct_arities': [3]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::rsubseq',
  'workflow': 'sorted-full',
  'body_sha256': '5e24df5a028d5b888e9b3dec5a7ff94a669ee0cac9ff23cfb83f8b5b49b24505',
  'signature_sha256': None,
  'direct_arities': [3, 5],
  'saved_alias': 'rsub'},
 {'obligation_id': 'local::src/core_var_metadata.tsv::sorted-map-by',
  'workflow': 'sorted-full',
  'body_sha256': 'd65b862c4899a4061a2f0a330fb1ca3e4b6348d56f458f7fc82340869fec0c37',
  'signature_sha256': None,
  'direct_arities': [2, 7]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::sorted-set',
  'workflow': 'sorted-full',
  'body_sha256': '27723dae871b275d3cf1fc621b5b5a5c34fc632d3c9c630dce4a8b4cea089b75',
  'signature_sha256': None,
  'direct_arities': [0, 3]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::sorted-set-by',
  'workflow': 'sorted-full',
  'body_sha256': '01cafccda84d75f3d3af6e9fc4abe9f2218aec4528ccc94d2801201ae5a1366f',
  'signature_sha256': None,
  'direct_arities': [4]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::subseq',
  'workflow': 'sorted-full',
  'body_sha256': 'f701e4dbb6f89eb3af90823e735c4c28647184cc9d00c620db7bc3d26027a872',
  'signature_sha256': None,
  'direct_arities': [3, 5],
  'saved_alias': 'sub'}]


def nodes(source):
    def visit(node):
        yield node
        for child in node.children:
            yield from visit(child)
    for node in Reader(source).all():
        yield from visit(node)

def rows():
    if digest((ROOT/SCANNER).read_bytes()) != SCANNER_SHA256:
        raise ValueError('reviewed structural scanner drift')
    result=[]
    for reviewed in REVIEWED:
        text=(ROOT/reviewed['driver']).read_text()
        matches=re.findall(r'const CONTRACT: &str = r#"(.*?)"#;',text,re.S)
        if len(matches)!=1 or digest(matches[0])!=reviewed['complete_source_sha256']:
            raise ValueError('whole original receiver contract drift')
        source=matches[0]
        if not re.search(r'#\[test\]\s*fn\s+'+reviewed['test']+r'\s*\(',text):
            raise ValueError('exact actual-phase test detached')
        if 'analyze_with_macros' not in text or 'MacroHost::new()' not in text or 'defmacro' not in text:
            raise ValueError('actual MacroHost computation detached')
        if reviewed['id']=='transient':
            pattern=r'fn '+reviewed['test']+r'\(\) \{\s*let expected = "(.*?)";'
            if re.findall(pattern,text,re.S)!=[reviewed['expected_literal']]:
                raise ValueError('original independent receiver literal changed')
        # Only insignificant inter-form whitespace is normalized for the older
        # single-line reference TSV transport. No literal or form is rewritten.
        if ';' in source or any(node.kind=='string' and ('\n' in source[node.start:node.end] or '\t' in source[node.start:node.end]) for node in nodes(source)):
            raise ValueError('reference normalization would change meaningful source')
        normalized=source.replace('\n',' ').replace('\t',' ')
        result.append({**reviewed,'source':source,'reference_source':normalized})
    return result

def oracle(row):
    identity=PREFIX+row['id']
    source="(= '"+row['expected_literal']+' (do '+row['reference_source']+'))'
    return {'id':identity,'source':source,'expected':{'id':identity,
        'outcome':{'status':'returned','value':True},'trace':[],'stdout':''},'native_stdout':None}

def saved_calls(row,target,selected):
    alias=selected['saved_alias'];source=row['source']
    binding=next((node for node in nodes(source) if node.kind=='vector' and any(child.token==alias for child in node.children)),None)
    if binding is None or not any(key.token==alias and value.token==target['name'] for key,value in zip(binding.children[::2],binding.children[1::2])):
        raise ValueError('saved public operation alias detached')
    calls=[call for call in direct_calls(source,alias) if call['argument_count'] in selected['direct_arities']]
    if {call['argument_count'] for call in calls}!=set(selected['direct_arities']):
        raise ValueError('saved operation is not actually called')
    return calls

def exports():
    workflows={row['id']:row for row in rows()}
    targets={row['id']:row for row in map(json.loads,(HERE/'obligations.jsonl').read_text().splitlines())}
    ordinary,phase,receipts,cases=[],[],[],{}
    for selected in SELECTION:
        target=targets[selected['obligation_id']];row=workflows[selected['workflow']]
        signature=target.get('signature')
        if target['body_sha256']!=selected['body_sha256'] or target['disposition']!='intended':
            raise ValueError('reviewed receiver operation/body drift')
        if signature and digest(json.dumps(signature,sort_keys=True))!=selected['signature_sha256']:
            raise ValueError('exact receiver-inclusive signature drift')
        calls=saved_calls(row,target,selected) if 'saved_alias' in selected else [call for call in direct_calls(row['source'],target['name']) if call['argument_count'] in selected['direct_arities']]
        if {call['argument_count'] for call in calls}!=set(selected['direct_arities']):
            raise ValueError('quoted, declared or absent demanded receiver call')
        case=oracle(row);cases[case['id']]=case
        rationale=('Complete unchanged '+row['id']+' contract executes '+target['name']+' at exact argument counts '+str(selected['direct_arities'])+'. '+row['demand_basis']+' Both checked pins execute the complete source and unchanged literal; the exact existing test executes it through QuickJS/Node applications and actual defmacro computation through MacroHost. Reference transport changes only insignificant newlines/tabs outside literals. No method-definition-only, inactive, quoted, native-only or metadata-value credit is added.')
        if 'saved_alias' in selected:
            rationale+=' The reviewed '+selected['saved_alias']+'='+target['name']+' binding is invoked at valid three/five-argument counts; invalid direct arity controls are not used as supported-operation evidence.'
        execution={'test_file':row['driver'],'test_name':row['test'],'test_source_sha256':digest((ROOT/row['driver']).read_bytes())}
        common={'obligation_id':target['id'],'body_sha256':target['body_sha256'],'rationale':rationale}
        if signature:common['signature_sha256']=selected['signature_sha256']
        ordinary.append({**common,'assertion_ids':[],'local_execution':execution,
            'reference_fixture_id':case['id'],'reference_source_sha256':digest(case['source']),
            'reference_contract_sha256':digest(json.dumps(case['expected'],sort_keys=True))})
        phase.append({**common,**execution,'support_source_sha256':{HELPER:digest((ROOT/HELPER).read_bytes()),SCANNER:SCANNER_SHA256}})
        receipts.append({**selected,'whole_original_source_sha256':row['complete_source_sha256'],
            'whole_reference_source_sha256':digest(row['reference_source']),'unchanged_literal_sha256':digest(row['expected_literal']),
            'actual_calls':calls,'demand_basis':row['demand_basis']})
    if len(ordinary)!=len(SELECTION) or len(cases)!=len(REVIEWED):
        raise ValueError('incomplete demanded receiver cohort')
    return {'obligation-witnesses.jsonl':ordinary,'phase-witnesses.jsonl':phase,
        'full-cases.jsonl':list(cases.values()),'arity-receiver-call-receipts.jsonl':receipts}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export-owned',type=Path,required=True)
    args=parser.parse_args();args.export_owned.mkdir(parents=True,exist_ok=True)
    for name,records in exports().items():
        (args.export_owned/name).write_text(''.join(json.dumps(row,separators=(',',':'))+'\n' for row in records))
    print(f'{len(SELECTION)} exact receiver/operation candidates;two whole original contracts;no shared writes')

if __name__ == '__main__':
    main()
