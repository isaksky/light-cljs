#!/usr/bin/env python3
"""Complete core-helper candidates requiring actual phases and both pins; no shared writes."""
from pathlib import Path
import argparse
import json
import re
from arity_resume_candidates import digest, direct_calls
from extract_inventory import Reader
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'tests/reference/compiler-language'
FIXTURE = 'tests/fixtures/compiler-core-helpers.tsv'
HELPER = 'tests/reference/compiler-language/arity_resume_extended.py'
SCANNER = 'tests/reference/compiler-language/arity_resume_candidates.py'
SCANNER_SHA256 = '31457848fe6d2f08e92b3ad5b4a6f99362a97be66e4819c9d45cf3a1b9ecbcc3'
DRIVER = 'tests/compiler_arity_resume_extended.rs'
TEST = 'complete_core_helpers_execute_in_actual_phases_and_lifecycles'
PREFIX = 'arity-extended-'

REVIEWED = [{'id': 'trampoline-and-comparator',
  'row_sha256': '5080ef02d921a975c2f58f34258095f6296a3738be0aa5cc952b6bf19879b61f',
  'source_sha256': '3d17de2e9b6ffc79dab6338c4195b8f0918601f47925183777590f5a71236031',
  'json_literal_sha256': '3dd620dd2aaacb6c140b8b83e781354252d29ea2c35ad6b9b27f89caa188516e',
  'demand_basis': 'Returned comparators are invoked with less/greater/equal inputs; the original '
                  '50000-step trampoline, callback effects and coercive comparator control '
                  'remain.'},
 {'id': 'update-map-metadata',
  'row_sha256': 'bf705ff5db882f53b9485662d701bff784a9b94b3300f2213491e3831d6cba3a',
  'source_sha256': '1d88de2c49de63f8db1cd97e00ba8a67798bd6baf885df2657a6cf815e652a00',
  'json_literal_sha256': 'c18126723e74d9b67f28c511ccfdb9f1572ac367ba4e7a70d60c4378ae87b870',
  'demand_basis': 'Returned maps are compared against original literals and metadata; empty/nil '
                  'inputs and unchanged original map are checked.'},
 {'id': 'language-helper-partition-vectors',
  'row_sha256': '35edbbaa12adc2af5b83e6e2dc751487e4d915af7fc1f626f128c54711ea229c',
  'source_sha256': 'b4f89acb0318075916a4df126dfbd542e9134afe9a587d46f02cd62c368f24b3',
  'json_literal_sha256': '5bbd39b2feb02cfeae1d4c320b08084404e0eb13788fe3a4af72903865091308',
  'demand_basis': 'All partition outputs are fully printed, both split results are demanded, and '
                  'vectors plus original padding literals are checked.'},
 {'id': 'language-helper-iteration-order',
  'row_sha256': '9dff0dd72d60873b9d2a12a3fec73106f25fad13b9c856ddd404fb6d1006e05c',
  'source_sha256': 'c8314dd41d4c2c5e6785391d34c7c110aa0e1eecabc7affd4f487affcd00afd3',
  'json_literal_sha256': 'c8c5e8caf92070eaef43b451893fdea7c232630beb36769666b5a61603700457',
  'demand_basis': 'Step/value/key callbacks are consumed through reduced termination and a fresh '
                  'full realization with both complete original ordered effect lists.'},
 {'id': 'language-helper-tagged-values',
  'row_sha256': '0d8c26f2518f576f6669a998b6cc4c3fdd32b29763f50031cff00a27ce8e83c3',
  'source_sha256': 'c2dbafd3babb527db81b85d703749fb927c1000b1f4eed5f0ff9414edb490cf8',
  'json_literal_sha256': 'd2043e551687a641d1b85a1a78eb1a5fccb9e5f6ccb5c703f1e2966378f6278d',
  'demand_basis': 'The constructed TaggedLiteral is observed through predicates, equality, '
                  'hashing, printing, keyword/default lookup and full original literal outputs.'},
 {'id': 'language-helper-printing',
  'row_sha256': '9f8bb919d0010ef9b6d0a30e66915aae742377628f621a72f59944964c23798a',
  'source_sha256': '4bba4eee16fef0354344aad2965591092fa4781f1764243cb01b4cc027c458f2',
  'json_literal_sha256': '7cda6d56665f67fba87c12b24203c0d50fc577bfd91902baeca4c8f89f18d40e',
  'demand_basis': 'Nested printing, namespace-map settings, depth settings and newline binding '
                  'preserve every original returned string.'},
 {'id': 'language-helper-js-conversions',
  'row_sha256': 'ab8f20e7629ad4e8739a0697701fd94c00afeddd273c1d3c700e1d5bbfcb1ecb',
  'source_sha256': '68d252f9fd438d34994d7f836bc0ed88a32814ae5eb9e8800037e8f08a20b50d',
  'json_literal_sha256': '996cb335ee02bb7594d49056b68e1712c943466c44965303aaa65feddf8cd15f',
  'demand_basis': 'Both option and no-option converters are demanded, including '
                  'metadata-preserving nested values, opaque Date identity and genuine reify '
                  'conversion receiver results.'},
 {'id': 'language-helper-munging',
  'row_sha256': 'fc8f6c65bc70c9c8df2d5a51caca12757674c84355c1249f27a59641484a5ecd',
  'source_sha256': '68e0f8e4a6ce42d417d8444d9eae29f06849d3f857071f02127f58a7036d5d4f',
  'json_literal_sha256': '05fce65c74d0d66d99020b7fa12dbfb13985f909c90f3d4559da11014ce866d1',
  'demand_basis': 'All original reserved-name, symbol round-trip, Closure name, dot and astral '
                  'Unicode literal results remain.'},
 {'id': 'language-helper-cat-early-stop',
  'row_sha256': '227a70edd2d0ac16964a652a927c577cd9d3319b4b2bccb0a5eef5377df80356',
  'source_sha256': 'b28329f0073551abd30923bd3e2bbe5156a654a3d5ed265fb7c2e7cd6b6b6c73',
  'json_literal_sha256': '5844c88f6e6e54e1ea097750cf495694dbb6dd26a263fba31a833df514b45188',
  'demand_basis': 'The returned reducing function is consumed by reduce; original reduced '
                  'termination preserves result6 and ordered [1 2 3] effects.'},
 {'id': 'language-helper-subvec-metadata-source',
  'row_sha256': 'b54a6430b244545e95a45f1b6614d48305f9a9f155bfda1c1121bf444552aecf',
  'source_sha256': 'dbcc9e78db6003ea82aed595d9be3464f3f758989b1b248d207ef73052976d93',
  'json_literal_sha256': '07992e8cb501c42f56a579fa0818cbe8562cb6decffcfb6a6e122cb4d9084dcd',
  'demand_basis': 'The two-argument nested view is demanded by source-vector identity, with '
                  'metadata, clone and original value controls retained.'}]
SELECTION = [{'obligation_id': 'api::src/main/cljs/cljs/core.cljs::comparator::arity-1',
  'workflow': 'trampoline-and-comparator',
  'body_sha256': '64009e21276ba7f195ea3e3d8e0327c0a1f5624d9123f0a79bc566a53776e412',
  'signature_sha256': '965b44486ce0ec36c7138165e350c9ca57d8fb9028fdd102ffa1ac758df06a9a',
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::comparator',
  'workflow': 'trampoline-and-comparator',
  'body_sha256': 'ccc7c3ec97c838419f4045f37f574dab1e638654c4b4250fae4e19f6315ecc39',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::update-vals::arity-1',
  'workflow': 'update-map-metadata',
  'body_sha256': 'c10ab0f8a025bba2b1624b5ab68c5053313ba3a4d7d265a6815c6e5fa8b57bcf',
  'signature_sha256': '6f73b296d16f53ea8349b8f5e39fa7f88f2530fee57bd63ad787c2ed2adbd8dd',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::update-keys::arity-1',
  'workflow': 'update-map-metadata',
  'body_sha256': '6f37ee96ee4431d1b3e22224d0af28b1c3dacf76f4e35109d013d5a67ca88d29',
  'signature_sha256': '6f73b296d16f53ea8349b8f5e39fa7f88f2530fee57bd63ad787c2ed2adbd8dd',
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::update-keys',
  'workflow': 'update-map-metadata',
  'body_sha256': 'e5028f71efc7bf86fb9e9358ad77e3689b4d85abc2aaaffb7ad2ff4854e9bf9e',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::update-vals',
  'workflow': 'update-map-metadata',
  'body_sha256': '49e1bc10ea8d1282e366ba117f6d00a5561a62ec3e5e2329a95ed1a065909bff',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::splitv-at::arity-1',
  'workflow': 'language-helper-partition-vectors',
  'body_sha256': '81ed93bbede19af35e9b44ea5e3d2b6fe36685f4206ba5f99e897247f46f4811',
  'signature_sha256': 'a8a0e292ddd7348613ab2e8c8f42e0afc0e0f6b17189d835d52075d81dc3241f',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::partitionv::arity-1',
  'workflow': 'language-helper-partition-vectors',
  'body_sha256': 'f9d1b2621a007c686aad3d5d8eaa5dc9198c10a8b788b4e57de8141ca9157b69',
  'signature_sha256': 'a8a0e292ddd7348613ab2e8c8f42e0afc0e0f6b17189d835d52075d81dc3241f',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::partitionv::arity-3',
  'workflow': 'language-helper-partition-vectors',
  'body_sha256': 'f9d1b2621a007c686aad3d5d8eaa5dc9198c10a8b788b4e57de8141ca9157b69',
  'signature_sha256': '0b2c104b57430548c0cd9a6961e6a67cb123ef73866f9609cfea086f6be3dfb2',
  'direct_arities': [4]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::partitionv-all::arity-2',
  'workflow': 'language-helper-partition-vectors',
  'body_sha256': 'a5e651f4da4d64b075c0e717dc1e8f3d843e92a19a228247ab2318c0871b6693',
  'signature_sha256': 'a8a0e292ddd7348613ab2e8c8f42e0afc0e0f6b17189d835d52075d81dc3241f',
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::partitionv',
  'workflow': 'language-helper-partition-vectors',
  'body_sha256': 'cff14ff489264892e56d11f306705f46372b9842e9cb5e1258dda10e036a82bc',
  'signature_sha256': None,
  'direct_arities': [2, 4]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::partitionv-all',
  'workflow': 'language-helper-partition-vectors',
  'body_sha256': 'b0ec80425ffdce2e848f615d3389a8a45299e5664e6f025dccb49f0190a6f0e8',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::splitv-at',
  'workflow': 'language-helper-partition-vectors',
  'body_sha256': '9d5d18052bbe147a74d3938aa365fde017c655246c8c88ad2110e7acbff28662',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::iteration::arity-1',
  'workflow': 'language-helper-iteration-order',
  'body_sha256': '63c63d0eb3b126a9a76dd9116679f40d5c444895fceeb5e8f0893305d00f2dc2',
  'signature_sha256': 'ac944a1f9fe27613db7243a76d888f951f40c4b4718c1a26ad381d0592316ead',
  'direct_arities': [7]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::iteration',
  'workflow': 'language-helper-iteration-order',
  'body_sha256': '0da08e6ac80c5c45d477188d2db71f169188f2d632471dfae78b3b9fd2ebc2e5',
  'signature_sha256': None,
  'direct_arities': [7]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::tagged-literal',
  'workflow': 'language-helper-tagged-values',
  'body_sha256': '50be60b91f68f0e876ee81d1252216c20b78140227b75364aeef46fbe6939fb7',
  'signature_sha256': None,
  'direct_arities': [2]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::tagged-literal?',
  'workflow': 'language-helper-tagged-values',
  'body_sha256': 'a8f8436a1bfc5081a9f04021d1ce4be88c8c944b537ff045e367d84970671b86',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/clojure/cljs/core.cljc::with-out-str::arity-1',
  'workflow': 'language-helper-printing',
  'body_sha256': 'fa2785334b6a52bce3242ccc5c93f46f1ffc7142a4d4149fa23a52b21b214ea5',
  'signature_sha256': 'f8b06b4d585bd5b035d36143f6e9428f98026ba06e5ded1f8979709a522732ad',
  'direct_arities': [1, 2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::clj->js::arity-1',
  'workflow': 'language-helper-js-conversions',
  'body_sha256': 'ae76cdcbab2bcc9573034d88488ec3d49ed783de6b53ef93776cadcfc34caf29',
  'signature_sha256': '264cd31a40cfe85d8a650d27db1cf9ffa51c90c21cde3c2aaf7620244e1fdfa4',
  'direct_arities': [3]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::js->clj::arity-1',
  'workflow': 'language-helper-js-conversions',
  'body_sha256': '7b10309da16c74b7948cacea5857552f6fb1eb8fb4123e3750314b8b2d059819',
  'signature_sha256': '57825001ab734a2f9387d427d94f3d4486d027e222ba0ff2737b638c9e9921c6',
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::js->clj::arity-2',
  'workflow': 'language-helper-js-conversions',
  'body_sha256': '7b10309da16c74b7948cacea5857552f6fb1eb8fb4123e3750314b8b2d059819',
  'signature_sha256': '595415bd86e3d6b1b10e5457b434aafae04d1f7048793c103d1f8d43801611ac',
  'direct_arities': [3]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::clj->js',
  'workflow': 'language-helper-js-conversions',
  'body_sha256': '4753d50b4c8a7e7139d452d391f4d78cb3b3c738ee503156859cad8d558a6309',
  'signature_sha256': None,
  'direct_arities': [1, 3]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::js->clj',
  'workflow': 'language-helper-js-conversions',
  'body_sha256': '215a6b49ca0b711979a11b307f04ad36b98eb7433aae896f181afb982977e49a',
  'signature_sha256': None,
  'direct_arities': [1, 3]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::munge-str::arity-1',
  'workflow': 'language-helper-munging',
  'body_sha256': '8c763f39baa4c080ae0079e09f3af8beb3f03bc8f171f31c399fac7e6d99856c',
  'signature_sha256': '549743d6605e7eac241cc55ab36280c41bf69db064cda0d2fe39cb0c610cb086',
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::munge::arity-1',
  'workflow': 'language-helper-munging',
  'body_sha256': '6ab195a27cca68b89a0ebf52f53acdac8e77504657f0ba69e528ff03e5c1f37c',
  'signature_sha256': '549743d6605e7eac241cc55ab36280c41bf69db064cda0d2fe39cb0c610cb086',
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::demunge::arity-1',
  'workflow': 'language-helper-munging',
  'body_sha256': '6444da8f67b7dc0bd4e1a9e0f0c4e5e45aa0300292f2ccd51c4a8a8819a899dc',
  'signature_sha256': '549743d6605e7eac241cc55ab36280c41bf69db064cda0d2fe39cb0c610cb086',
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::demunge',
  'workflow': 'language-helper-munging',
  'body_sha256': 'd5b6eb4ce20db4864ec42c74efb9ef318ab0f214c7621a62cffe5e7cdc5189e0',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::munge',
  'workflow': 'language-helper-munging',
  'body_sha256': 'a4007aa5d324dccec4e492e0ac8bc7f7bcd4d41d0d35c67a6f027be9adb38fae',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::munge-str',
  'workflow': 'language-helper-munging',
  'body_sha256': 'dba1b60b3dc7399ddf33d244b6df64bcd22aea8eddaef86262b839e99fec4e66',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::cat::arity-1',
  'workflow': 'language-helper-cat-early-stop',
  'body_sha256': '8a773318923c8e580f238ebdd2d37f026dcad62a3ec05c543bfd3329f08a3904',
  'signature_sha256': 'dcb22433c7234032f1f1e9a5fac6aaaa263abafbe2f9a1e6f9f35fdf9c2cf706',
  'direct_arities': [1]},
 {'obligation_id': 'local::src/core_var_metadata.tsv::cat',
  'workflow': 'language-helper-cat-early-stop',
  'body_sha256': '3bdcf0508873e519e9c112eb13ca9516311290b7fed6167a47a4cafb00100d63',
  'signature_sha256': None,
  'direct_arities': [1]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::subvec::arity-1',
  'workflow': 'language-helper-subvec-metadata-source',
  'body_sha256': 'afe5c911525ef228ccd942a2774ab5d33ab6d980e46166bf5b29606046d3921a',
  'signature_sha256': 'a993b380f9f78b6fc0ca35279e8a30b7b25b26bb5d1a1ddfb12b2f79524c268f',
  'direct_arities': [2]},
 {'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-js->clj::arity-1',
  'workflow': 'language-helper-js-conversions',
  'body_sha256': '664834c1d9d8161561589df9218caa2575d14ec0e826eeaedac3ed2caa58ff54',
  'signature_sha256': '1e29e0e3c827392d9367e12e46a10c02872ac0d1015f41a6450df295cb801229',
  'receiver_dispatch': {'method': '-js->clj',
                        'caller': 'js->clj',
                        'receiver': 'custom',
                        'caller_argument_count': 3}},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-js->clj',
  'workflow': 'language-helper-js-conversions',
  'body_sha256': '0e909b9280fcf5b87d173f068abfab71210f83c3392642fd6def6c2fb4d56dda',
  'signature_sha256': None,
  'receiver_dispatch': {'method': '-js->clj',
                        'caller': 'js->clj',
                        'receiver': 'custom',
                        'caller_argument_count': 3}},
 {'obligation_id': 'local::src/core_var_metadata.tsv::-clj->js',
  'workflow': 'language-helper-js-conversions',
  'body_sha256': '85051f00b5546c1644f9ad1fad9d789c5b58d07ae4b659cd70df34e25d24b16e',
  'signature_sha256': None,
  'receiver_dispatch': {'method': '-clj->js',
                        'caller': 'clj->js',
                        'receiver': 'out',
                        'caller_argument_count': 1}}]


def head(node):
    return node.children[0].token if node.kind == 'list' and node.children else ''

def rows():
    if digest((ROOT/SCANNER).read_bytes()) != SCANNER_SHA256:
        raise ValueError('reviewed structural scanner drift')
    source = (ROOT/FIXTURE).read_text()
    result = []
    for reviewed in REVIEWED:
        matches = [line for line in source.splitlines() if line.startswith(reviewed['id']+'\t')]
        if len(matches) != 1 or digest(matches[0]) != reviewed['row_sha256']:
            raise ValueError('whole original helper row drift')
        identity, original, expected = matches[0].split('\t')
        if digest(original) != reviewed['source_sha256'] or digest(expected) != reviewed['json_literal_sha256']:
            raise ValueError('complete helper source/literal drift')
        value = json.loads(expected)
        if not isinstance(value, list) or json.dumps(value, ensure_ascii=False, separators=(',', ':')) != expected:
            raise ValueError('original strict JSON literal drift')
        result.append({**reviewed, 'source': original, 'expected_json': expected})
    driver = (ROOT/DRIVER).read_text()
    if not re.search(r'#\[test\]\s*fn\s+'+TEST+r'\s*\(', driver) or 'compile_namespace_with_macros' not in driver or 'MacroHost::new()' not in driver or 'defmacro' not in driver:
        raise ValueError('detached actual phase driver')
    for row in result:
        if json.dumps(row['id']) not in driver:
            raise ValueError('whole helper source omitted by driver')
    return result

def oracle(row):
    identity = PREFIX+row['id']
    source = '(= '+json.dumps(row['expected_json'])+' (js/JSON.stringify (do '+row['source']+')))'
    return {'id': identity, 'source': source, 'expected': {'id': identity,
        'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}, 'native_stdout': None}

def receiver_demand(row, proof):
    """Check a specifically reviewed genuine receiver and demanded converter.

    The key encoder named 'unused' remains uncredited. Method declarations
    cannot supply execution by themselves.
    """
    top = Reader(row['source']).all()
    if len(top) != 1 or head(top[0]) != 'let':
        raise ValueError('changed conversion receiver setup')
    bindings = top[0].children[1].children
    values = {key.token: value for key, value in zip(bindings[::2], bindings[1::2])}
    receiver = values[proof['receiver']]
    if head(receiver) != 'reify':
        raise ValueError('conversion receiver is not a genuine reify')
    method = next((node for node in receiver.children[2:] if head(node) == proof['method']), None)
    if method is None or method.children[1].kind != 'vector':
        raise ValueError('absent genuine receiver method')
    form = '('+proof['caller']+' '+proof['receiver']+(' :keywordize-keys true' if proof['caller'] == 'js->clj' else '')+')'
    calls = [call for call in direct_calls(row['source'], proof['caller']) if call['call_source'] == form and call['argument_count'] == proof['caller_argument_count']]
    if len(calls) != 1:
        raise ValueError('receiver method is not actually demanded')
    expected = '(-js->clj [_ opts] (:keywordize-keys opts))' if proof['caller'] == 'js->clj' else '(-clj->js [_] #js {:x 7})'
    text = row['source'][method.start:method.end]
    if text != expected:
        raise ValueError('receiver result/forwarded options changed')
    return {'caller': calls[0], 'receiver': proof['receiver'], 'method_source': text,
        'method_source_sha256': digest(text), 'receiver_inclusive_arity': len(method.children[1].children)}

def exports():
    workflows = {row['id']: row for row in rows()}
    targets = {row['id']: row for row in map(json.loads, (HERE/'obligations.jsonl').read_text().splitlines())}
    execution = {'test_file': DRIVER, 'test_name': TEST, 'test_source_sha256': digest((ROOT/DRIVER).read_bytes())}
    support = {path: digest((ROOT/path).read_bytes()) for path in [FIXTURE, HELPER, SCANNER]}
    ordinary, phases, receipts, cases = [], [], [], {}
    for selected in SELECTION:
        target = targets[selected['obligation_id']]
        row = workflows[selected['workflow']]
        signature = target.get('signature')
        if target['body_sha256'] != selected['body_sha256'] or target['disposition'] != 'intended':
            raise ValueError('reviewed operation/body drift')
        if signature and digest(json.dumps(signature, sort_keys=True)) != selected['signature_sha256']:
            raise ValueError('exact public signature drift')
        if 'receiver_dispatch' in selected:
            demand = receiver_demand(row, selected['receiver_dispatch'])
            calls = []
            if signature and signature['minimum'] != demand['receiver_inclusive_arity']:
                raise ValueError('genuine receiver signature mismatch')
        else:
            calls = [call for call in direct_calls(row['source'], target['name']) if call['argument_count'] in selected['direct_arities']]
            demand = None
            if {call['argument_count'] for call in calls} != set(selected['direct_arities']):
                raise ValueError('absent or nonexecuting direct source calls')
        case = oracle(row)
        cases[case['id']] = case
        rationale = ('Complete unchanged '+row['id']+' demands '+target['name']+'. '+row['demand_basis']+
            ' The full original source, exact strict JSON result string, setup/options, effects and errors execute in fresh/reused applications and actual MacroHost computations, in QuickJS/Node, and in both checked pins. No native-only, quoted, metadata-value, method-definition-only or undemanded callback credit is granted.')
        if demand:
            rationale += ' The genuine '+demand['receiver']+' reify receiver method is demanded by the exact converter call and its returned literal value; the unrelated unused key-encoder method is excluded.'
        common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'], 'rationale': rationale}
        if signature:
            common['signature_sha256'] = selected['signature_sha256']
        ordinary.append({**common, 'assertion_ids': [], 'local_execution': execution,
            'reference_fixture_id': case['id'], 'reference_source_sha256': digest(case['source']),
            'reference_contract_sha256': digest(json.dumps(case['expected'], sort_keys=True))})
        phases.append({**common, **execution, 'support_source_sha256': support})
        receipts.append({**selected, 'direct_calls': calls, 'receiver_demand': demand,
            'whole_source_sha256': row['source_sha256'], 'json_literal_sha256': row['json_literal_sha256'],
            'original_row_sha256': row['row_sha256'], 'demand_basis': row['demand_basis']})
    if len(ordinary) != len(SELECTION) or len(cases) != len(REVIEWED):
        raise ValueError('incomplete reviewed helper cohort')
    return {'obligation-witnesses.jsonl': ordinary, 'phase-witnesses.jsonl': phases,
        'full-cases.jsonl': list(cases.values()), 'arity-extended-call-receipts.jsonl': receipts}

def parse_frames(output):
    expected = [(row['id'], host, phase, engine) for row in rows() for host in ['fresh', 'reused']
        for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
    frames = []
    for line in output.splitlines():
        if not line.startswith('arity-extended-observation\t'):
            continue
        fields = line.split('\t')
        if len(fields) != 6 or fields[5] != 'true':
            raise ValueError('nonpassing actual whole-helper frame')
        frames.append({'id': fields[1], 'host': fields[2], 'phase': fields[3], 'engine': fields[4], 'raw': fields[5]})
    if [(frame['id'], frame['host'], frame['phase'], frame['engine']) for frame in frames] != expected:
        raise ValueError('missing/extra/reordered actual helper phase frames')
    return frames

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export-owned', type=Path, required=True)
    args = parser.parse_args()
    args.export_owned.mkdir(parents=True, exist_ok=True)
    generated = exports()
    for name, records in generated.items():
        (args.export_owned/name).write_text(''.join(json.dumps(row, separators=(',', ':'))+'\n' for row in records))
    print(f'{len(SELECTION)} exact helper/receiver candidates;{len(REVIEWED)} complete original workflows;no shared writes')


if __name__ == '__main__':
    main()
