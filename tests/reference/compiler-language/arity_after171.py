#!/usr/bin/env python3
"""Export frozen whole-oracle arity requirements without shared ledger writes."""
from pathlib import Path
import argparse
import hashlib
import json
import re
from arity_resume_candidates import digest, direct_calls
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'tests/reference/compiler-language'
HELPER = 'tests/reference/compiler-language/arity_after171.py'
DRIVER = 'tests/compiler_arity_after171.rs'
TEST = 'complete_arity_oracles_execute_in_actual_phases_and_lifecycles'
SCANNER = 'tests/reference/compiler-language/arity_resume_candidates.py'
SCANNER_SHA256 = '31457848fe6d2f08e92b3ad5b4a6f99362a97be66e4819c9d45cf3a1b9ecbcc3'
DEMAND = {
 'cljs-equality-hashing-test-test-962-empty-literal-hashes': 'Both complete empty-collection equality groups demand each hash helper, including the original apply == and map hash observations.',
 'cljs-equality-hashing-test-test-nil-hashing-cljs-1649': 'Both original nil and null string hash comparisons are demanded by the complete independent [true true] literal.',
 'cljs-equality-hashing-test-test-mumur-support': 'Every original rotation and signed multiplication argument pair is observed against its original numeric literal, including negative and hexadecimal cases.',
 'cljs-equality-hashing-test-test-cljs-3410': 'Original distinct negative doubles and repeated hash controls are evaluated against the complete original five-value literal.',
 'cljs-printing-completion-test-print-with-opts': 'All nine original writer-option assertions run. The real writer passed to both alt-impl callbacks is actually written, and the exact <[1 2 3]> and nested keyword rendering literals demand those callbacks. The final empty writer callback remains an independent control.',
 'cljs-public-gate-iterator-helper-contract': 'The original iterator-helper-contract is called; its complete ranged-iterator boundary loops demand js-mod for every original start/end pair and assert the actual base slot, preserving lazy effects, next/hasNext loops, array mutations, errors and metadata controls.',
 'cljs-multimethods-test-hierarchy': 'The unchanged run function is called. All derive mutations occur inside its original body and are demanded by literal parents/ancestors/descendants and isa? comparisons, with original class and vector dispatch controls.',
 'arity-resume-core-factory-body-core-test-test-multimethods-1': 'The complete original setup/body expression executes. Its original preference mutation changes the literal preference count and competing multimethod result; the retrieved method remains called through the original apply.',
 'cljs-multimethods-test-multimethods-2': 'The unchanged run function is called. The original two-to-one-to-zero method table counts demand methods, remove-method and remove-all-methods on the genuine MultiFn receiver; every unrelated dispatch, apply, hierarchy and caught error assertion remains.',
}
REVIEWED = [{'case_sha256': 'a5b41f182b478253e5927e2e4533fa1ad03a533aaa5bd8f206bae7093432587b',
  'fixture': 'tests/fixtures/arity-after171/cljs-equality-hashing-test-test-962-empty-literal-hashes.cljs',
  'id': 'cljs-equality-hashing-test-test-962-empty-literal-hashes',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '73fb720b7913e3c51528131630594bd25ab13714f508d88e313a33d40dec6756'},
 {'case_sha256': '83ae0d98428e56f8912928a73a5ef5a4d9bc9a793c7eba5aaea8ded64206e56e',
  'fixture': 'tests/fixtures/arity-after171/cljs-equality-hashing-test-test-nil-hashing-cljs-1649.cljs',
  'id': 'cljs-equality-hashing-test-test-nil-hashing-cljs-1649',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '15bd845a36a15c090d1246731b51a458aa098ab09278c821b8730a48d1872da1'},
 {'case_sha256': '4a1bf71a1db2480bd0a82943ada5b70876af9e4513f23a772bec3cfe8d224705',
  'fixture': 'tests/fixtures/arity-after171/cljs-equality-hashing-test-test-mumur-support.cljs',
  'id': 'cljs-equality-hashing-test-test-mumur-support',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'f79f146ffaca736b0cb7b8dd1490e470ca6de1cfd5c12396ef592551474cb7d7'},
 {'case_sha256': 'c486855b1adbc0d99d7e1d1d477ffa3e32f20f827327f80301335733fd28595e',
  'fixture': 'tests/fixtures/arity-after171/cljs-equality-hashing-test-test-cljs-3410.cljs',
  'id': 'cljs-equality-hashing-test-test-cljs-3410',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'b7d4e123247414fe4f342823aa1c54bfb03b22e20183ef842c677fc6cab3d91d'},
 {'case_sha256': 'e53db7cc559b66853586018497882081b3763804e478c4b6321d6f157ff574f1',
  'fixture': 'tests/fixtures/arity-after171/cljs-printing-completion-test-print-with-opts.cljs',
  'id': 'cljs-printing-completion-test-print-with-opts',
  'invoke': 'run',
  'namespace': 'cljs.printing-test',
  'source_sha256': '430ae9ef0c7ea2ecc6df7a916f072dbb21f93cc23c3abd5309ddca22c107a6e3'},
 {'case_sha256': 'c9f7b0ab6c6853c384702c50931ae06eee2fd6a68f80a35b6c28ed7ab2a5c6d0',
  'fixture': 'tests/fixtures/arity-after171/cljs-public-gate-iterator-helper-contract.cljs',
  'id': 'cljs-public-gate-iterator-helper-contract',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '43846851896124a048a253839df7719b974e0fe6e52ece5bee34f6497483ee10'},
 {'case_sha256': 'e286120e9a334fb6f3b09af1a790a626122eed74a4b332857844f8ad0d02ed19',
  'fixture': 'tests/fixtures/arity-after171/cljs-multimethods-test-hierarchy.cljs',
  'id': 'cljs-multimethods-test-hierarchy',
  'invoke': 'run',
  'namespace': 'cljs.core-test',
  'source_sha256': '711f72473a4305cdc2e541c053d080fd34b499a7d169bc61759a327b01fbb333'},
 {'case_sha256': 'e247b06e1feb34fe1bda007e44e0e0215b52245304af308df10b80caeb1186ce',
  'fixture': 'tests/fixtures/arity-after171/arity-resume-core-factory-body-core-test-test-multimethods-1.cljs',
  'id': 'arity-resume-core-factory-body-core-test-test-multimethods-1',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '85aa73a4ae8299a5d4501ec9b733f2004aab15d4a558f391379fd81208724a8d'}]
SELECTION = [{'body_sha256': '00e951c7a6a281bc3a940c541b2909f8811e2320e8c0610d0b7fb95c98505ea0',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::hash-ordered-coll',
  'signature_sha256': None,
  'workflow': 'cljs-equality-hashing-test-test-962-empty-literal-hashes'},
 {'body_sha256': 'd8817cabf4aa098526546cc8d3cfa3f2b2083ada431717853d5806371a6b6c1d',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::hash-unordered-coll',
  'signature_sha256': None,
  'workflow': 'cljs-equality-hashing-test-test-962-empty-literal-hashes'},
 {'body_sha256': 'f71bdeac4821bccb3ac1c03469c35f52fe8a21d61e2150b64661edbd9e8f0e1c',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::hash-string',
  'signature_sha256': None,
  'workflow': 'cljs-equality-hashing-test-test-nil-hashing-cljs-1649'},
 {'body_sha256': 'f5c126a74bc3a0127c685117550b14606941d8823ded622421df4d6fe17541e0',
  'direct_arities': [2],
  'obligation_id': 'local::src/core_var_metadata.tsv::imul',
  'signature_sha256': None,
  'workflow': 'cljs-equality-hashing-test-test-mumur-support'},
 {'body_sha256': '79117ef18ddbff77a1989de71bdc812ca56df1cbc703878971e069bdd1cb915f',
  'direct_arities': [2],
  'obligation_id': 'local::src/core_var_metadata.tsv::int-rotate-left',
  'signature_sha256': None,
  'workflow': 'cljs-equality-hashing-test-test-mumur-support'},
 {'body_sha256': 'd78fca67406e8d884bb1e222789fe682180d69001a03070ba90f3235203c2f25',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::hash-double',
  'signature_sha256': None,
  'workflow': 'cljs-equality-hashing-test-test-cljs-3410'},
 {'body_sha256': 'ba22278cdf258642a42f381052e468105ecd567bbb33c811d4da88e9786dc4d2',
  'direct_arities': [2],
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-write::arity-1',
  'signature_sha256': '616cd2823e6c418f6d439a65c9a887e46b3fbe04a596e9cde1a398f84eee465a',
  'workflow': 'cljs-printing-completion-test-print-with-opts'},
 {'body_sha256': 'c25aeaed574809ff85afd2dcf91dc0fc787f3685bf52c619d36d079d533fd10b',
  'direct_arities': [2],
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::pr-str-with-opts::arity-1',
  'signature_sha256': 'e551f1e3c0abcb9d4f2de76bef79f8ea8c7d1986f50d2e2d51a10a66bfb4835e',
  'workflow': 'cljs-printing-completion-test-print-with-opts'},
 {'body_sha256': '4cb69bcacf38d4d4f07c980efad85f9a5a09656a77d2f8266eac6c00be24b48e',
  'direct_arities': [2],
  'obligation_id': 'local::src/core_var_metadata.tsv::-write',
  'signature_sha256': None,
  'workflow': 'cljs-printing-completion-test-print-with-opts'},
 {'body_sha256': '38f63efa4b009a8248a4d158454f43fdca15fb1a5d450d8ecce7587df69f83ec',
  'direct_arities': [2],
  'obligation_id': 'local::src/core_var_metadata.tsv::pr-str-with-opts',
  'signature_sha256': None,
  'workflow': 'cljs-printing-completion-test-print-with-opts'},
 {'body_sha256': '28fb8bfcc4d4674152954edf54d2a9b9bfd135b573d42e556c844d5e88b7ce7d',
  'direct_arities': [2],
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::js-mod::arity-1',
  'signature_sha256': '5e489b352780eea17f8227251ef17dfa2e6dac38020dd63ca263421cbb7154b6',
  'workflow': 'cljs-public-gate-iterator-helper-contract'},
 {'body_sha256': '84be24970d373be91412569ac27498fc15d07485b43ef5444db4cd1f949a967e',
  'direct_arities': [2],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::js-mod::arity-1',
  'signature_sha256': '7106c533f5e74a7a88f4893eb52e0c6b194cff0e645b295c45307bc3ac8a772c',
  'workflow': 'cljs-public-gate-iterator-helper-contract'},
 {'body_sha256': 'b39a44fe983196fa40186c452c20c6fef3b3bab595859b07d6a256e99414ff85',
  'direct_arities': [2],
  'obligation_id': 'local::src/core_var_metadata.tsv::js-mod',
  'signature_sha256': None,
  'workflow': 'cljs-public-gate-iterator-helper-contract'},
 {'body_sha256': 'fa84ef14d79ee803494d5ad1a2091ddec24e5fc40d0c1c22407b5c025c2c5b39',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::ancestors',
  'signature_sha256': None,
  'workflow': 'cljs-multimethods-test-hierarchy'},
 {'body_sha256': '3f418aee247b9c81737a621b456d7c3cf1b0a96661641828976aa5991ed5a2f6',
  'direct_arities': [2],
  'obligation_id': 'local::src/core_var_metadata.tsv::derive',
  'signature_sha256': None,
  'workflow': 'cljs-multimethods-test-hierarchy'},
 {'body_sha256': '2e67f030eaa93c896c958401ba6abe232626f838f689cfe9e7adbebbf5f1e3d4',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::descendants',
  'signature_sha256': None,
  'workflow': 'cljs-multimethods-test-hierarchy'},
 {'body_sha256': 'fad128e06d13508e48e42f05108ab09431586f21b2891dda7efc77da2ed34d69',
  'direct_arities': [2],
  'obligation_id': 'local::src/core_var_metadata.tsv::isa?',
  'signature_sha256': None,
  'workflow': 'cljs-multimethods-test-hierarchy'},
 {'body_sha256': '9ff2c860c9ee32fa8c8c1682e063ebd93908c1424dc75743a8baedf0e964bc81',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::parents',
  'signature_sha256': None,
  'workflow': 'cljs-multimethods-test-hierarchy'},
 {'body_sha256': 'e667fe999056d741fae017a9e72992bd231e3a9befb8ca305999ae3e1e8d19ff',
  'direct_arities': [3],
  'obligation_id': 'local::src/core_var_metadata.tsv::prefer-method',
  'signature_sha256': None,
  'workflow': 'arity-resume-core-factory-body-core-test-test-multimethods-1'},
 {'body_sha256': '6d0d0eb2ed1910a2ed29572717036a2528ad7cd5e69c2d0b6d9c2d4bbcb683bb',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::prefers',
  'signature_sha256': None,
  'workflow': 'arity-resume-core-factory-body-core-test-test-multimethods-1'}]



def rows():
    if digest((ROOT / SCANNER).read_bytes()) != SCANNER_SHA256:
        raise ValueError('reviewed structural scanner drift')
    catalog = {row['id']: row for row in map(json.loads, (HERE / 'full-cases.jsonl').read_text().splitlines())}
    driver = (ROOT / DRIVER).read_text()
    if not re.search(r'#\[test\]\s*fn\s+' + TEST + r'\s*\(', driver):
        raise ValueError('detached actual phase driver')
    for required in ['compile_namespace_with_macros', 'MacroHost::new()', 'defmacro arity-witness [] (run nil)', 'pr-str (run nil)', 'defmacro arity-witness [] {source}', 'fresh', 'reused']:
        if required not in driver:
            raise ValueError('missing actual whole source invocation')
    result = []
    for reviewed in REVIEWED:
        case = catalog[reviewed['id']]
        source = (ROOT / reviewed['fixture']).read_text()
        if digest(source) != reviewed['source_sha256'] or source != case['source'] or digest(json.dumps(case, sort_keys=True)) != reviewed['case_sha256']:
            raise ValueError('unchanged complete original oracle drift')
        if json.dumps(reviewed['id']) not in driver or Path(reviewed['fixture']).name not in driver:
            raise ValueError('whole source omitted from driver')
        if case['expected'] != {'id': case['id'], 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}:
            raise ValueError('original literal result/effect contract drift')
        result.append({**reviewed, 'case': case, 'source': source})
    return result


def exports():
    workflows = {row['id']: row for row in rows()}
    targets = {row['id']: row for row in map(json.loads, (HERE / 'obligations.jsonl').read_text().splitlines())}
    execution = {'test_file': DRIVER, 'test_name': TEST, 'test_source_sha256': digest((ROOT / DRIVER).read_bytes())}
    ordinary, phases, receipts = [], [], []
    for selected in SELECTION:
        target, row = targets[selected['obligation_id']], workflows[selected['workflow']]
        signature = target.get('signature')
        if target['body_sha256'] != selected['body_sha256'] or target['disposition'] != 'intended':
            raise ValueError('reviewed operation/body drift')
        if signature and digest(json.dumps(signature, sort_keys=True)) != selected['signature_sha256']:
            raise ValueError('exact public signature drift')
        calls = [call for call in direct_calls(row['source'], target['name']) if call['argument_count'] in selected['direct_arities']]
        if {call['argument_count'] for call in calls} != set(selected['direct_arities']):
            raise ValueError('absent reviewed actual source calls')
        if signature and not any(call['argument_count'] == signature['minimum'] if signature['kind'] == 'fixed' else call['argument_count'] > signature['minimum'] for call in calls):
            raise ValueError('no exact signature invocation demand')
        rationale = (DEMAND[row['id']] + ' The entire unchanged original oracle, setup, body, loops, literals, effects and errors execute through the exact driver in fresh/reused actual MacroHost applications and macro computations, observed by QuickJS and Node. Both required public pins execute the exact same existing oracle. Declaration, source registration, function identity and undemanded callback paths provide no credit. Local mappings cross-check the operation, with no complete metadata-value claim.')
        common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'], 'rationale': rationale}
        if signature:
            common['signature_sha256'] = selected['signature_sha256']
        ordinary.append({**common, 'assertion_ids': [], 'local_execution': execution, 'reference_fixture_id': row['id'], 'reference_source_sha256': digest(row['case']['source']), 'reference_contract_sha256': digest(json.dumps(row['case']['expected'], sort_keys=True))})
        phases.append({**common, **execution, 'support_source_sha256': {path: digest((ROOT / path).read_bytes()) for path in [HELPER, SCANNER, row['fixture']]}})
        receipts.append({**selected, 'direct_calls': calls, 'whole_source_sha256': row['source_sha256'], 'original_case_sha256': row['case_sha256'], 'demand_basis': DEMAND[row['id']], 'invocation': '(run nil)' if row['invoke'] else 'complete expression', 'oracle_id': row['id']})
    return {'obligation-witnesses.jsonl': ordinary, 'phase-witnesses.jsonl': phases, 'arity-after171-call-receipts.jsonl': receipts}


def parse_frames(output):
    expected = [(row['id'], host, phase, engine) for row in rows() for host in ['fresh', 'reused'] for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
    frames = []
    for line in output.splitlines():
        if not line.startswith('arity-after171-observation\t'):
            continue
        fields = line.split('\t')
        if len(fields) != 6 or fields[5] != 'true':
            raise ValueError('nonpassing actual whole-oracle frame')
        frames.append({'id': fields[1], 'host': fields[2], 'phase': fields[3], 'engine': fields[4], 'raw': fields[5]})
    if [(f['id'], f['host'], f['phase'], f['engine']) for f in frames] != expected:
        raise ValueError('missing/extra/reordered actual whole-oracle frames')
    return frames


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export-owned', type=Path, required=True)
    args = parser.parse_args()
    args.export_owned.mkdir(parents=True, exist_ok=True)
    for name, records in exports().items():
        (args.export_owned / name).write_text(''.join(json.dumps(row, separators=(',', ':')) + '\n' for row in records))
    print(f'{len(SELECTION)} exact public/local candidates; {len(REVIEWED)} unchanged whole oracles; no shared writes')


if __name__ == '__main__':
    main()
