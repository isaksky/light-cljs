#!/usr/bin/env python3
"""Export narrow arity candidates from unchanged, demanded whole workflows.

No shared catalog writes. Exports are execution requirements, never passes.
"""
from pathlib import Path
import argparse
import hashlib
import json
import re
from extract_inventory import Reader
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'tests/reference/compiler-language'
HELPER = 'tests/reference/compiler-language/arity_resume_candidates.py'
PREFIX = 'arity-resume-'
TESTS = {
    'factories': ('tests/compiler_core_factories.rs', 'core_factories_execute_in_applications_and_macros'),
    'protocols': ('tests/compiler_public_protocols.rs', 'public_protocol_api_arities_and_macro_phase_execute'),
    'constructors': ('tests/compiler_public_constructors.rs', 'public_constructor_api_arities_and_macro_phase_execute'),
}
REVIEWED = [{'fixture': 'tests/fixtures/compiler-core-factories.tsv',
  'id': 'core-factory-multifn',
  'group': 'factories',
  'source_sha256': '1ce4500d472c752bb9a52dc982c69278c31ea7af712e879692bd2b45c30b4201',
  'row_sha256': '651d7b7fdd05e5b647984ee255e796807d870343107e9f7f0b1ed64aa65c5fd3',
  'demand_basis': 'Every constructed MultiFn receiver is called, then its retrieved method is '
                  'invoked with nil and must return 41.'},
 {'fixture': 'tests/fixtures/compiler-core-factories.tsv',
  'id': 'core-factory-slot-effects',
  'group': 'factories',
  'source_sha256': 'e346455b6bc96b5ba50c9dc54fe7c80a84e523b3ddc3a861dc59f97d713576d6',
  'row_sha256': '1a4545f02f3a03e7cabb3598c3aae6a6c11196573cdb033026165107bed9be21',
  'demand_basis': 'The custom comparator is demanded by get on the supplied tree; the loop '
                  'constructs a consumed tree stack; ordered effects and complete range/iterator '
                  'observations remain.'},
 {'fixture': 'tests/fixtures/compiler-core-factories.tsv',
  'id': 'core-factory-construction-effects',
  'group': 'factories',
  'source_sha256': '4e44c46c4a1e7de435b2a1946625c7a1cff65d57edeea4c974e857e0caf35793',
  'row_sha256': 'cfcd96b8908efb5d694a0e5a778ddafee1365d56436d6cb548531a330344b032',
  'demand_basis': 'The constructed JS object becomes the supplied tree; getters, comparator '
                  'effects, lookup and cached hashes are observed.'},
 {'fixture': 'tests/fixtures/compiler-core-factory-bodies.tsv',
  'id': 'core-factory-body-core-test-test-transducers',
  'group': 'factories',
  'source_sha256': '7d0cd6f86c0e98272a0b54cfa8348826d6e4ce6c9c0f08134261d327485a21a3',
  'row_sha256': 'bb73cddf506432f40ea455a6105202bb7c613d4a5d659569170a0a93201820f5',
  'demand_basis': 'The replacement transducer is consumed by the complete original sequence '
                  'workflow; numeric hash equality is asserted.'},
 {'fixture': 'tests/fixtures/compiler-core-factory-bodies.tsv',
  'id': 'core-factory-body-core-test-test-multimethods-1',
  'group': 'factories',
  'source_sha256': '85aa73a4ae8299a5d4501ec9b733f2004aab15d4a558f391379fd81208724a8d',
  'row_sha256': '656e701a796717e37b594b1ba65df868713b9c84bac53b1c722492894cec74c3',
  'demand_basis': 'The actual IMultiFn receiver retains hierarchy and competing methods; the '
                  'retrieved method is applied to the original two arguments and asserted.'},
 {'fixture': 'tests/fixtures/compiler-core-factory-bodies.tsv',
  'id': 'core-factory-body-src-test-cljs-cljs-iterator-test-coll-iter-seq-match',
  'group': 'factories',
  'source_sha256': '2fd292e05196ebd844c6263afb955e363e538dbe49f9d3c9587f80b90bcccefb',
  'row_sha256': '65a1136ac6700b306ec63bf28261ac330121d4ff1a04073d5fd40cb45662ef9e',
  'demand_basis': 'The original seq-iter-match helper is invoked for map, set, queue and record '
                  'receivers; its entire original next/hasNext loop compares each iterator '
                  'result.'},
 {'fixture': 'tests/fixtures/compiler-public-protocol-bodies.tsv',
  'id': 'public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds',
  'group': 'protocols',
  'source_sha256': '41312406a04718f83766e075768d62ae3724ab9818fb15ca5b69cba80d1ff4eb',
  'row_sha256': 'f42ff5c61f45ca321c37b3ed7774193ad4b8e7ee6002ffea831b37aa0f9e0b9e',
  'demand_basis': 'The UUID constructed during the complete original predicate table setup is '
                  'consumed by every predicate-table comparison and original loop.'},
 {'fixture': 'tests/fixtures/compiler-public-constructor-bodies.tsv',
  'id': 'public-constructor-body-core-test-test-reductions-obeys-reduced',
  'group': 'constructors',
  'source_sha256': '6d1a202fc4275429c818454a8604e974a0316986830b5b19dd0bf4d48120fc54',
  'row_sha256': '4fa055807a58565a3a4138298b4ff2ced838b2a1d5ce84fe2a15655641dfeee7',
  'demand_basis': 'Both reductions arities are realized against complete original independent '
                  'literals, retaining infinite inputs, initially reduced values and early '
                  'termination.'},
 {'fixture': 'tests/fixtures/compiler-public-constructor-bodies.tsv',
  'id': 'public-constructor-body-collections-test-test-vectors',
  'group': 'constructors',
  'source_sha256': 'fdd8130f5659c2079a423c329eeca4d7e4c801bbe97f7bd72cb3a3c9ba0050a9',
  'row_sha256': '54cc9357c5a73a35add77e2b754095b38aa614b567bcdb66276b54d1bd92e71e',
  'demand_basis': 'The zero-argument JS object is demanded as an identity sentinel in caught '
                  'indexed errors; three-argument subvec retains the complete original vector '
                  'body, metadata, nested views, lookup, reduction and invalid-index controls.'}]
SELECTION = [{'obligation_id': 'api::src/main/cljs/cljs/core.cljs::get-method::arity-1',
  'workflow': 'core-factory-multifn',
  'body_sha256': '94148d4b618074b0b4f1365cfa9d95a63fa6a72071335284ffb288903aac51ad',
  'signature_sha256': '2457330a99d4d0d47b5d18faf0fa65f4c32e3b5cda239c9a19cb89c1138e9ceb',
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::get-method',
  'workflow': 'core-factory-multifn',
  'body_sha256': '630b12e3c8686ac4236ad830a9b9d8fcc8a7165ff652f452715144cbb8881c9e',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::compare::arity-1',
  'workflow': 'core-factory-slot-effects',
  'body_sha256': '9f7d9f48983e13389705d5883db9910a21146c05b3d16c530cbef44dcd05b7f6',
  'signature_sha256': 'ef2c01168fb717272a4131e897c54e6f74baaf751c7a389d84b8ecd96e1637eb',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/clojure/cljs/core.cljc::let::arity-1',
  'workflow': 'core-factory-slot-effects',
  'body_sha256': '6fef5cb8bdbc45547ba5df6d2b251be0eee42e8a78347f8fba7f44b1a8c190e7',
  'signature_sha256': 'bbce2142210ff6d05831b75efcba8523d8c57f11c137ed69448700c494e521ad',
  'direct_arities': [14]},
 {'obligation_id': 'api::src/main/clojure/cljs/core.cljc::loop::arity-1',
  'workflow': 'core-factory-slot-effects',
  'body_sha256': 'a4c836b16d46e83d687644d234524bf202404f9bbf0cb503fdf01dac2a0a52e8',
  'signature_sha256': 'bbce2142210ff6d05831b75efcba8523d8c57f11c137ed69448700c494e521ad',
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::compare',
  'workflow': 'core-factory-slot-effects',
  'body_sha256': 'dbbb5769dc8a190bdf6ff62f0ee1b7e62f3663a7b6a7bb95495840e9ee6dbf07',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::js-obj::arity-2',
  'workflow': 'core-factory-construction-effects',
  'body_sha256': 'ac50f5ef05cbceeec7227d1284b2f9f7ddbce55b06cd374f9dd8a43458cff19e',
  'signature_sha256': '468ccb8a7bdc5d8e3e189c6fd075e137f4a541e72e09b4a6e96f2a818fafd240',
  'direct_arities': [4]},
 {'obligation_id': 'api::src/main/clojure/cljs/core.cljc::js-obj::arity-1',
  'workflow': 'core-factory-construction-effects',
  'body_sha256': '6eefe89a2ef3ac5f62add2ece4b79a6bdec1aad9272469b1c75913f7831ebea3',
  'signature_sha256': 'ce7315a764e5e1250a7d2a72da86be9ab310b0096ad0022f177edef66727fe8f',
  'direct_arities': [4]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::js-obj',
  'workflow': 'core-factory-construction-effects',
  'body_sha256': 'e12bc9b26bb80b7e246cbbec7a46056e8723026267932067dd0976882dfe04bc',
  'signature_sha256': None,
  'direct_arities': [4]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::replace::arity-1',
  'workflow': 'core-factory-body-core-test-test-transducers',
  'body_sha256': 'bf1a9863d51d7d8e945710b95093256d960354b733fe7c02d63527c9fe89db4c',
  'signature_sha256': '1a65a5078a9e396e2e6dd08c1a94288287856ab0845311dc7f39153903d115e7',
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/clojure/cljs/core.cljc::==::arity-2',
  'workflow': 'core-factory-body-core-test-test-transducers',
  'body_sha256': '5dff4d7c81f019be4ec89363dfad53ec5bcccc38efdbe9906d74a87a404c2045',
  'signature_sha256': 'ef2c01168fb717272a4131e897c54e6f74baaf751c7a389d84b8ecd96e1637eb',
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::replace',
  'workflow': 'core-factory-body-core-test-test-transducers',
  'body_sha256': '41786df75120871b6aae0033870d939fbaeae04def62e3f8217950e0d0f16561',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-get-method::arity-1',
  'workflow': 'core-factory-body-core-test-test-multimethods-1',
  'body_sha256': 'b8d01df32f4a96966b38cbbef68e64fdbed78d2e578a7a2f05b1d49f59c58761',
  'signature_sha256': '13eb140ceda67d008235f7440e87ae3d702f3923dae4171032f88dc26b1dbb56',
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-get-method',
  'workflow': 'core-factory-body-core-test-test-multimethods-1',
  'body_sha256': '051befd2fb28cdb334378f1e3d4b5c506b3687cdc7691ae51d44272d67d8636c',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-iterator::arity-1',
  'workflow': 'core-factory-body-src-test-cljs-cljs-iterator-test-coll-iter-seq-match',
  'body_sha256': 'bbdc3d32eda16461317de57b6cfc12767a345e3dca2e3dbe55e20b064d68fa31',
  'signature_sha256': '7e858f7b7ea7a24bf6340aa43b8567afa8e81dcdda941b22e94e72485515449d',
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-iterator',
  'workflow': 'core-factory-body-src-test-cljs-cljs-iterator-test-coll-iter-seq-match',
  'body_sha256': 'ef4706b0a1a002428f2233cd914638df05baba138d31e4f06e5a59913aad7cb7',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::uuid::arity-1',
  'workflow': 'public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds',
  'body_sha256': '6453f2bec08ef10572d9bc162051bebd1cfbd79e86f0e83b0913f7a7ad7d02b4',
  'signature_sha256': 'cf54978776ee23317c8af7fb2d95c45168beeeef8358b0efd630f7a9744c8f04',
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::uuid',
  'workflow': 'public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds',
  'body_sha256': '491a71a1049985fcebaf2caecb1850c2ba4d34076e66df97429c72239d65227f',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::reductions::arity-1',
  'workflow': 'public-constructor-body-core-test-test-reductions-obeys-reduced',
  'body_sha256': 'a4416d2133c1ca5725d6ce7cbc87d6fa64b72d58d1d7d9086ad825d8d50f8019',
  'signature_sha256': 'cf8f9b52bc35a082e0600f4bf1a67ce2bd6c61a57f4a6e496021fdddf3dcff77',
  'direct_arities': [2, 3]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::reductions::arity-2',
  'workflow': 'public-constructor-body-core-test-test-reductions-obeys-reduced',
  'body_sha256': 'a4416d2133c1ca5725d6ce7cbc87d6fa64b72d58d1d7d9086ad825d8d50f8019',
  'signature_sha256': '43750673cbee0163ea9a403faa2c0e0673e1511649fd03d28b118f1d9cbae2de',
  'direct_arities': [2, 3]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::reductions',
  'workflow': 'public-constructor-body-core-test-test-reductions-obeys-reduced',
  'body_sha256': '3ffed6685e7dd8cdc6f33a378cfa4e039cb3448d7afb82b251b7de7b7067c5ac',
  'signature_sha256': None,
  'direct_arities': [2, 3]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::js-obj::arity-1',
  'workflow': 'public-constructor-body-collections-test-test-vectors',
  'body_sha256': 'ac50f5ef05cbceeec7227d1284b2f9f7ddbce55b06cd374f9dd8a43458cff19e',
  'signature_sha256': 'c04963f87d82d63dd1d5239f487499f7c8f6ac7f34ad0470e03c0c5f4f28a5ec',
  'direct_arities': [0]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::subvec::arity-2',
  'workflow': 'public-constructor-body-collections-test-test-vectors',
  'body_sha256': 'afe5c911525ef228ccd942a2774ab5d33ab6d980e46166bf5b29606046d3921a',
  'signature_sha256': 'c2c041c2b4a85db01d331c1f817d767104109f0d094aa121bc846ba53f3e18de',
  'direct_arities': [3]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::subvec',
  'workflow': 'public-constructor-body-collections-test-test-vectors',
  'body_sha256': '6e4a6956ae30fae5abdc165526f1d43e0eec329b47ca61968d76fbfe8ebd1271',
  'signature_sha256': None,
  'direct_arities': [3]}]

def digest(value):
    return hashlib.sha256(value if isinstance(value, bytes) else value.encode()).hexdigest()

def head(node):
    return node.children[0].token if node.kind == 'list' and node.children else ''

def direct_calls(source, name):
    """Recover exact source call sites; explicit table reviews demand separately.

    Method vectors, quoted/discarded data and thread-injected arguments cannot
    establish calls. Enclosing callbacks/helpers are not presumed demanded:
    every selected workflow has an independently frozen demand basis.
    """
    result = []
    containers = {'reify', 'deftype', 'defrecord', 'extend-type', 'extend-protocol', 'specify', 'specify!'}
    threads = {'->', '->>', 'some->', 'some->>', 'cond->', 'cond->>', 'as->'}
    def visit(node):
        if node.kind in {'quote', 'syntax-quote', 'discard', 'var'} or head(node) == 'quote': return
        if head(node) == name:
            text = source[node.start:node.end]
            result.append({'argument_count': len(node.children)-1, 'call_source': text, 'call_source_sha256': digest(text)})
        if head(node) in threads: return
        if head(node) in containers:
            for child in node.children[1:]:
                if child.kind == 'list' and len(child.children)>1 and child.children[1].kind == 'vector':
                    for form in child.children[2:]: visit(form)
                elif child.kind != 'list': visit(child)
            return
        for child in node.children: visit(child)
    for node in Reader(source).all(): visit(node)
    return result

def workflows():
    result = {}
    bodies = {row['id']: row for row in map(json.loads, (HERE/'full-bodies.jsonl').read_text().splitlines())}
    for spec in REVIEWED:
        lines = [line for line in (ROOT/spec['fixture']).read_text().splitlines() if line.startswith(spec['id']+'\t')]
        if len(lines) != 1 or digest(lines[0]) != spec['row_sha256']: raise ValueError('whole reviewed fixture row drift')
        fields = lines[0].split('\t'); source = fields[1]
        if digest(source) != spec['source_sha256'] or fields[2] != 'true': raise ValueError('whole source/literal drift')
        driver, test = TESTS[spec['group']]
        raw = (ROOT/driver).read_text()
        if not re.search(r'#\[test\]\s*fn\s+'+test+r'\s*\(', raw) or Path(spec['fixture']).name not in raw:
            raise ValueError('detached exact actual application/macro driver')
        if 'analyze_with_macros' not in raw or 'MacroHost::new()' not in raw or 'defmacro' not in raw:
            raise ValueError('actual MacroHost source contract detached')
        provenance = json.loads(fields[3]) if len(fields)==4 else None
        for original in provenance or []:
            body = bodies[original['body_id']]
            if original['source'] != body['source'] or original['sha256'] != body['source_span']['sha256']:
                raise ValueError('complete original body/setup provenance drift')
            if digest(body['source_span']['text']) != original['sha256']:
                raise ValueError('original complete body text drift')
            if 'selected_setup' in original and (original['selected_setup'] not in body['source_span']['text'] or digest(original['selected_setup']) != original['selected_setup_sha256']):
                raise ValueError('original required setup drift')
        result[spec['id']] = {**spec, 'source': source, 'fixture_row_sha256': digest(lines[0]),
            'provenance': provenance}
    return result

def exports():
    rows = workflows()
    targets = {row['id']: row for row in map(json.loads, (HERE/'obligations.jsonl').read_text().splitlines())}
    ordinary, phase, receipts, cases = [], [], [], {}
    for selected in SELECTION:
        target = targets[selected['obligation_id']]; row = rows[selected['workflow']]
        signature = target.get('signature')
        if target['disposition'] != 'intended' or target['body_sha256'] != selected['body_sha256']:
            raise ValueError('selected public/local obligation drift')
        if signature and digest(json.dumps(signature, sort_keys=True)) != selected['signature_sha256']:
            raise ValueError('exact public signature drift')
        calls = [call for call in direct_calls(row['source'], target['name']) if call['argument_count'] in selected['direct_arities']]
        if not calls or {call['argument_count'] for call in calls} != set(selected['direct_arities']):
            raise ValueError('quoted, indirect or absent reviewed direct call')
        if signature:
            calls = [call for call in calls if call['argument_count']==signature['minimum']] if signature['kind']=='fixed' else [call for call in calls if call['argument_count']>signature['minimum']]
            if not calls: raise ValueError('no exact signature demand')
        driver, test = TESTS[row['group']]
        execution = {'test_file': driver, 'test_name': test, 'test_source_sha256': digest((ROOT/driver).read_bytes())}
        identity = PREFIX + row['id']
        oracle = {'id': identity, 'source': row['source'], 'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}, 'native_stdout': None}
        cases[identity] = oracle
        rationale = ('Complete unchanged '+row['id']+' demands '+target['name']+' at exact reviewed argument counts '+str(sorted({c['argument_count'] for c in calls}))+'. '+row['demand_basis']+' Every original setup, assertion, literal, loop, effect and caught error stays in the whole-source oracle. The exact existing test executes the whole source in QuickJS/Node applications and inside a genuine defmacro through MacroHost; both checked pins must execute the unchanged full oracle. No source-site, declaration-only, quoted, threaded or undemanded call credit is added.')
        if target['obligation_kind']=='local-api': rationale += ' This is an operation cross-check, with no complete metadata-value claim.'
        common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'], 'rationale': rationale}
        if signature: common['signature_sha256'] = selected['signature_sha256']
        ordinary.append({**common, 'assertion_ids': [], 'local_execution': execution, 'reference_fixture_id': identity, 'reference_source_sha256': digest(oracle['source']), 'reference_contract_sha256': digest(json.dumps(oracle['expected'],sort_keys=True))})
        phase.append({**common, **execution, 'support_source_sha256': {row['fixture']: digest((ROOT/row['fixture']).read_bytes()), HELPER: digest((ROOT/HELPER).read_bytes())}})
        receipts.append({**selected, 'direct_calls': calls, 'whole_source_sha256': row['source_sha256'], 'fixture': row['fixture'], 'fixture_row_sha256': row['fixture_row_sha256'], 'complete_body_provenance': row['provenance'], 'demand_basis': row['demand_basis'], 'oracle_id': identity})
    if len(ordinary)!=len(SELECTION) or len(cases)!=len(REVIEWED): raise ValueError('incomplete reviewed cohort')
    return {'obligation-witnesses.jsonl': ordinary, 'phase-witnesses.jsonl': phase, 'full-cases.jsonl': list(cases.values()), 'arity-resume-call-receipts.jsonl': receipts}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export-owned', type=Path, required=True)
    args = parser.parse_args(); args.export_owned.mkdir(parents=True, exist_ok=True)
    generated = exports()
    for name, records in generated.items():
        (args.export_owned/name).write_text(''.join(json.dumps(row,separators=(',',':'))+'\n' for row in records))
    print(f'{len(SELECTION)} exact public/local candidates; {len(REVIEWED)} unchanged whole workflows; no shared writes')
if __name__ == '__main__': main()
