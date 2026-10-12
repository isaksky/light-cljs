#!/usr/bin/env python3
"""New demanded public/local operations after188; complete original sources unchanged."""
from pathlib import Path
import argparse
import json
import re
from extract_inventory import Reader
from arity_resume_candidates import digest, direct_calls, head
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'tests/reference/compiler-language'
HELPER = 'tests/reference/compiler-language/arity_after188.py'
SCANNER = 'tests/reference/compiler-language/arity_resume_candidates.py'
DRIVER = 'tests/compiler_arity_after188.rs'
TEST = 'new_arity_oracles_execute_in_actual_phases_and_lifecycles'
SCANNER_SHA256 = '31457848fe6d2f08e92b3ad5b4a6f99362a97be66e4819c9d45cf3a1b9ecbcc3'
REVIEWED = [{'case_sha256': 'ce102332c4c7e1eb751f2024dc962c217b8566b06b7d3bf4066fa4686bde7a4f',
  'fixture': 'tests/fixtures/arity-after188/cljs-extra-core-test-cljs-2944.cljs',
  'id': 'cljs-extra-core-test-cljs-2944',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '34d9bef246a176da58de1319a2e77e17846ce4e43742b56392971bcf2386eafc'},
 {'case_sha256': 'e9887d0eb87fada34e3be5a7b33866c6aa72f31534f1c3e4a02cd27d15e635cb',
  'fixture': 'tests/fixtures/arity-after188/cljs-deftype-core.cljs',
  'id': 'cljs-deftype-core',
  'invoke': '(run nil)',
  'namespace': 'cljs.core-test',
  'source_sha256': '025aae2c8df87d759d4c90b0b282d5250b6030e56be034b37c243c062b6d6f10'},
 {'case_sha256': 'ecc33d8aae4501e83a324fc3070e89265ed41f28fca9732b0037a26a73c59eab',
  'fixture': 'tests/fixtures/arity-after188/local-api-source-sci-lazy-probe-doall-nil.cljs',
  'id': 'local-api-source-sci-lazy-probe-doall-nil',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '56ea61417435e61d1f81229bbb177839c273244b9d4f70c0348348b81177af36'},
 {'case_sha256': '2219e88deea01a1a15f32341738ed82e4c0683df55434e9b7fd20f0c22e6b6e6',
  'fixture': 'tests/fixtures/arity-after188/cljs-extension-test-cljs-2812.cljs',
  'id': 'cljs-extension-test-cljs-2812',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'c4362e731578fa8c620d8eb91e71aa719731640e3d2c790b57a8e08f98ec3d41'},
 {'case_sha256': 'c42c36c8d587a92ad4841a1ec487d66b660f90deb509aa46d5a0f86cda6e3a4c',
  'fixture': 'tests/fixtures/arity-after188/cljs-final-core-uri-predicate.cljs',
  'id': 'cljs-final-core-uri-predicate',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'cf27fc89a1117596e806a2ca7745dc5ad3377532d0acdf9c6d3911fa02cc6abb'},
 {'case_sha256': '9fab190e64581c902256cff3bc109b722c4949a831795f1862ff6dc3441faf25',
  'fixture': 'tests/fixtures/arity-after188/cljs-extra-seqs-test-iteration-opts.cljs',
  'id': 'cljs-extra-seqs-test-iteration-opts',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '1c0948f8d2015af7fdb2742f0e202334a7658c7bd905fed93c6fa05e6e9dc209'},
 {'case_sha256': '88d8e8938eb698e7680ec7cb0454b41a02f7247bc37cc4abf48d85d3fb83a506',
  'fixture': 'tests/fixtures/arity-after188/cljs-final-seqs-test-iteration.cljs',
  'id': 'cljs-final-seqs-test-iteration',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '484ac18de4e6f5cd55e326e1757614e93e0a919d7ef9f7d8cc3bc19e719ecbf1'},
 {'case_sha256': '4fca8ba2d85701fc3a34da508bc3afd87ab4eece380c4112d4f3c78a11ebe490',
  'fixture': 'tests/fixtures/arity-after188/cljs-public-gate-core-factory-multifn.cljs',
  'id': 'cljs-public-gate-core-factory-multifn',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '1ce4500d472c752bb9a52dc982c69278c31ea7af712e879692bd2b45c30b4201'},
 {'case_sha256': 'd33eb8bacd7091d9b3c2692c239367dbd879f028e1d3db27d752e13a5884648a',
  'fixture': 'tests/fixtures/arity-after188/public-string-buffer-writer.cljs',
  'id': 'public-string-buffer-writer',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '29f024c19a38f31f9db240ce473b5172b181338eb70ac3f62d9edf2f3010678c'},
 {'case_sha256': '1093cea6098e0b8feb62a31df5559a1192a273c45130e2e5aa493bfb98be1764',
  'fixture': 'tests/fixtures/arity-after188/cljs-extra-seqs-test-rearrange-sequential.cljs',
  'id': 'cljs-extra-seqs-test-rearrange-sequential',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'e04a7cc47f70c02c1261c26d3d0afa88e1628aaacc087f501b7eaf4f11267b1a'},
 {'case_sha256': '0ea6f0dd67c762ff7f8d5c9e07fdbf821775c68b96e4fef903fd0f2f8707ad76',
  'fixture': 'tests/fixtures/arity-after188/cljs-sequence-completion-core-test-test-transducers.cljs',
  'id': 'cljs-sequence-completion-core-test-test-transducers',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '9c6de567811c6f7a581fdc2b443b07f0a1f0587275784a89962e6be962f1b73e'},
 {'case_sha256': '29b39b8ce3696022a9d8490a8805e2e349afd3f1e3a930091566a1ae62eccd0c',
  'fixture': 'tests/fixtures/arity-after188/public-print-writer-factories.cljs',
  'id': 'public-print-writer-factories',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '8b97cb1883bf841d750601560c35c81d3c4a37db7ad59cc13d8ff9125c893c4b'},
 {'case_sha256': '31085e8c507e7e948b48b0dd4193f4a91cad84ef9aaa70059b1caa1aeacd2d22',
  'fixture': 'tests/fixtures/arity-after188/cljs-control-const-emission.cljs',
  'id': 'cljs-control-const-emission',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'f14b2c3e4fca586d938b52eb6a5758a471585ad2568f53ef2590306d7e54d4a6'}]
SELECTION = [{'body_sha256': '5ca19b6580d26841019772cf2abbe7d925e6280785f44dbb93c82d398349e265',
  'call_name': '->Var',
  'direct_arities': [3],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::->Var::arity-1',
  'signature_sha256': '0569ce93e494ce24fdde0250942297080ebc2c2b90665d84cf7974bae37910ce',
  'workflow': 'cljs-extra-core-test-cljs-2944'},
 {'body_sha256': 'f624e155c995687f39e652b97a6e73b2a7ce62705cd970e97fb01ad128a27f22',
  'call_name': 'equiv-map',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::equiv-map::arity-1',
  'signature_sha256': 'ef2c01168fb717272a4131e897c54e6f74baaf751c7a389d84b8ecd96e1637eb',
  'workflow': 'cljs-deftype-core'},
 {'body_sha256': 'b0cedc52bb36eb061a6787b6e96dab7daee10ba679a5e5057f4c35327227c0ab',
  'call_name': 'equiv-map',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::equiv-map',
  'signature_sha256': None,
  'workflow': 'cljs-deftype-core'},
 {'body_sha256': 'a937da2e4cb6e2d900565f03f9f4832a62ed6bb6d65edd80a99de74287568003',
  'call_name': 'dorun',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::dorun::arity-1',
  'signature_sha256': '7e858f7b7ea7a24bf6340aa43b8567afa8e81dcdda941b22e94e72485515449d',
  'workflow': 'local-api-source-sci-lazy-probe-doall-nil'},
 {'body_sha256': '914dfc9c211e6ebd0b130d41f6c6779c5c32bc6bbf293d85f82d60e53de4b735',
  'call_name': 'write-all',
  'direct_arities': [2, 4],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::write-all::arity-1',
  'signature_sha256': '7075cf257b13b8e142b1023126113d3a5ab6da943dae545ef82e2b4d7a2044df',
  'workflow': 'cljs-extension-test-cljs-2812'},
 {'body_sha256': 'f41a5745f93cca74280211b7219053a451a995f7701b0b27a545f10c0133aed8',
  'call_name': 'write-all',
  'direct_arities': [2, 4],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::write-all',
  'signature_sha256': None,
  'workflow': 'cljs-extension-test-cljs-2812'},
 {'body_sha256': '97334365e60ccc0ce4ec3a1b41e4206d47f1d389918f59d9127e98502351d6de',
  'call_name': 'uri?',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::uri?::arity-1',
  'signature_sha256': '57825001ab734a2f9387d427d94f3d4486d027e222ba0ff2737b638c9e9921c6',
  'workflow': 'cljs-final-core-uri-predicate'},
 {'body_sha256': '4d995757211382988acf3fedfe1dc07bf302c924bea2ca917043a0222e718679',
  'call_name': 'defn',
  'direct_arities': [8],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::defn::arity-1',
  'signature_sha256': '15df313be9ca0c5ba4dd1cd843f9e1e4a861d562e954acc0e53e41221c2e0dd7',
  'workflow': 'cljs-deftype-core'},
 {'body_sha256': 'bb6b1f4383284f0fbe46a92c3b051a7ca2dd36d058b23ce0e0b0fd5efc8327ae',
  'call_name': 'uri?',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::uri?',
  'signature_sha256': None,
  'workflow': 'cljs-final-core-uri-predicate'},
 {'body_sha256': '3476eeae2090a2520a009ff0056b4aaa45abad1749fde510dcf3c86b8772b58f',
  'call_name': 'cond',
  'direct_arities': [4],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::cond~2::arity-1',
  'signature_sha256': '335525dc8005478af0d816164844473cc4d4657813b84d30afe8dbfa00390c1b',
  'workflow': 'cljs-extra-seqs-test-iteration-opts'},
 {'body_sha256': '34b39c93b4becc3e4a9c51f7ec59e461b899349b1a420b8d902c3bbc4e390bc3',
  'call_name': 'es6-iterator-seq',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::es6-iterator-seq',
  'signature_sha256': None,
  'workflow': 'cljs-final-seqs-test-iteration'},
 {'body_sha256': '84759e6eb74018c7498f07b758844b841159171128807df69db4946edffd44bf',
  'call_name': 'make-hierarchy',
  'direct_arities': [0],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::make-hierarchy',
  'signature_sha256': None,
  'workflow': 'cljs-public-gate-core-factory-multifn'},
 {'body_sha256': 'ba22278cdf258642a42f381052e468105ecd567bbb33c811d4da88e9786dc4d2',
  'call_name': '-flush',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-flush::arity-1',
  'signature_sha256': '0d0da7fefc6db853af7990a69415ba4358158570e4fbe6b1560430d497173bbe',
  'workflow': 'public-string-buffer-writer'},
 {'body_sha256': '5785003786629ba8747b8b2c639748c16028d988c8cb46e35f74d2eec6e16e3d',
  'call_name': '-flush',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::-flush',
  'signature_sha256': None,
  'workflow': 'public-string-buffer-writer'},
 {'body_sha256': '832ad5aa0e7fe14a2b03d4ff5d6b6ec2eb2b947b6ac6ee0c032e4e66969cb499',
  'call_name': 'iterate',
  'callback_position': 1,
  'consumer': 'iterate',
  'direct_arities': [1],
  'evidence_mode': 'higher-order',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::shuffle::arity-1',
  'signature_sha256': '7e858f7b7ea7a24bf6340aa43b8567afa8e81dcdda941b22e94e72485515449d',
  'workflow': 'cljs-extra-seqs-test-rearrange-sequential'},
 {'body_sha256': '0b5d73715b0c2311bc912798d3d9992781da72f5412a3ab6696ba901304085f9',
  'call_name': 'iterate',
  'callback_position': 1,
  'consumer': 'iterate',
  'direct_arities': [1],
  'evidence_mode': 'higher-order',
  'obligation_id': 'local::src/core_var_metadata.tsv::shuffle',
  'signature_sha256': None,
  'workflow': 'cljs-extra-seqs-test-rearrange-sequential'},
 {'body_sha256': 'b32231a54fe0902fe7fa8d8c2afd80c62e81d860b5c1748e1a4f2eae3ad9b2bd',
  'call_name': 'mapcat',
  'callback_position': 1,
  'consumer': 'mapcat',
  'direct_arities': [1],
  'evidence_mode': 'higher-order',
  'obligation_id': 'local::src/core_var_metadata.tsv::flatten',
  'signature_sha256': None,
  'workflow': 'cljs-sequence-completion-core-test-test-transducers'},
 {'body_sha256': 'ba22278cdf258642a42f381052e468105ecd567bbb33c811d4da88e9786dc4d2',
  'call_name': 'satisfies?',
  'direct_arities': [],
  'evidence_mode': 'protocol-receiver',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IWriter::arity-1',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'public-print-writer-factories'},
 {'body_sha256': '97b8538ba57c14cd9354a376661dfd581e50f31c0a07835af727ca8c27c4d06e',
  'call_name': 'satisfies?',
  'direct_arities': [],
  'evidence_mode': 'protocol-receiver',
  'obligation_id': 'local::src/core_var_metadata.tsv::IWriter',
  'signature_sha256': None,
  'workflow': 'public-print-writer-factories'},
 {'allow_variadic_minimum': True,
  'body_sha256': 'd3848ba1e3b0c935caebb8401ed47c0200862fc76307d1ed33fe35bd2b575d23',
  'call_name': 'if-some',
  'direct_arities': [3],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::if-some~2::arity-2',
  'signature_sha256': 'b30875b53e6b543d08e8ee7802590daf484497d4b005a7266473083cd28ac1a3',
  'workflow': 'cljs-control-const-emission'}]
DEMAND = {'cljs-control-const-emission': 'The actual original if-some three-argument source chooses its '
                                'true branch and observes the original constant literal; '
                                'definition and both original assertions execute without rewriting '
                                'or removing the alternate branch.',
 'cljs-definitions-var-test': 'Original run invokes cljs-3411. Both function and Var test calls '
                              'invoke the original metadata callback, which calls the original '
                              'defn at two explicit argument pairs and asserts literal sums before '
                              'test returns :ok.',
 'cljs-deftype-core': 'The original run invokes every complete helper. equiv-map compares the '
                      'IMap/ISeqable receiver against both equal and unequal literal maps; all '
                      'watch, custom atom, factory metadata and exception-independent setup '
                      'remains.',
 'cljs-extension-test-cljs-2812': 'All six original custom printer implementations are called by '
                                  'actual pr-str on object, boolean, number, string, array and '
                                  'function receivers; independent exact printed strings demand '
                                  'each write-all body. Original protocol restoration remains.',
 'cljs-extra-core-test-cljs-2944': 'The original ->Var receiver is consumed by symbol; its exact '
                                   'qualified symbol is asserted, together with the original '
                                   'keyword conversion and caught invalid numeric conversion.',
 'cljs-extra-seqs-test-iteration-opts': 'The original iteration callback reaches cond through both '
                                        'seq realization and into, preserving false as a '
                                        'continuation key and the complete original expected '
                                        'sequence, step counts and iterator options.',
 'cljs-extra-seqs-test-rearrange-sequential': 'The original realized take100/iterate/filter '
                                              'workflow invokes shuffle on the complete '
                                              'ten-element collection; the original independent '
                                              'nonempty inequality assertion and all '
                                              'sort/comparator bodies remain.',
 'cljs-final-core-uri-predicate': 'All original positive constructed goog.Uri receivers and '
                                  'negative primitive/vector/map controls execute; the conditional '
                                  'embedding setup and every literal predicate result remain.',
 'cljs-final-seqs-test-iteration': 'The original es6-iterator-seq consumes the same complete array '
                                   'as iteration and preserves nil/boolean elements; all original '
                                   'randomized paging, sequence/transducer realization and '
                                   'termination bodies remain.',
 'cljs-public-gate-core-factory-multifn': 'The hierarchy created by make-hierarchy is passed '
                                          'unchanged into each of five original MultiFn '
                                          'construction forms, checked by field identity, and '
                                          'consumed by actual method lookup and invocation '
                                          'returning literal41.',
 'cljs-sequence-completion-core-test-test-transducers': 'The original mapcat flatten callback is '
                                                        'consumed through the complete composed '
                                                        'transducer by actual sequence and the '
                                                        'independent literal (36 200 10). All '
                                                        'original arrays, strings, reductions, '
                                                        'completion, metadata, multi-input and '
                                                        'eduction bodies remain.',
 'local-api-source-sci-lazy-probe-doall-nil': 'The original complete probe observes all three nil '
                                              'realization results, empty ordered effects and '
                                              'captured stdout; the invoked dorun must return nil '
                                              'alongside original doall controls.',
 'public-print-writer-factories': 'The original constructed writer explicitly satisfies IWriter, '
                                  'then its saved/direct/qualified/applied write and flush methods '
                                  'consume all three receivers. Callback this identity, return '
                                  'vectors, order and exact events are retained.',
 'public-string-buffer-writer': 'The original writer consumes the original append callback, writes '
                                'the exact literal a, flushes the same receiver and returns the '
                                'complete original vector of write result, nil flush result and '
                                'captured output.'}
EXPECTED_LITERAL_RAW = {'cljs-control-const-emission': 'true',
 'cljs-definitions-var-test': 'true',
 'cljs-deftype-core': 'true',
 'cljs-extension-test-cljs-2812': 'true',
 'cljs-extra-core-test-cljs-2944': 'true',
 'cljs-extra-seqs-test-iteration-opts': 'true',
 'cljs-extra-seqs-test-rearrange-sequential': 'true',
 'cljs-final-core-uri-predicate': 'true',
 'cljs-final-seqs-test-iteration': 'true',
 'cljs-public-gate-core-factory-multifn': 'true',
 'cljs-sequence-completion-core-test-test-transducers': 'true',
 'local-api-source-sci-lazy-probe-doall-nil': 'true',
 'public-print-writer-factories': '[true true [["a" true]] [["a" true] ["b" true]] [["a" true] '
                                  '["b" true] ["c" true]] nil [["a" true] ["b" true] ["c" true]]]',
 'public-string-buffer-writer': '["a" nil "a"]'}

def demand_basis(row):
    return DEMAND[row['id']]


def rows():
    if digest((ROOT / SCANNER).read_bytes()) != SCANNER_SHA256:
        raise ValueError('reviewed structural scanner drift')
    import report
    catalog = report.fixtures('intended-language')
    driver = (ROOT / DRIVER).read_text()
    if not re.search(r'#\[test\]\s*fn\s+' + TEST + r'\s*\(', driver):
        raise ValueError('detached actual phase driver')
    for required in ['compile_namespace_with_macros', 'MacroHost::new()', 'defmacro arity-witness [] {invoke}', 'pr-str {invoke}', 'defmacro arity-witness [] (do {source})', 'fresh', 'reused', 'arity-after188-artifacts', 'from_secs(120)']:
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
        if reviewed['invoke']:
            definitions = [node for node in Reader(source).all() if node.kind == 'list' and len(node.children) > 2 and node.children[0].token == 'defn' and node.children[1].token == 'run']
            if len(definitions) != 1 or definitions[0].children[2].kind != 'vector':
                raise ValueError('original exported callback definition drift')
            arity = len(definitions[0].children[2].children)
            required = '(run)' if arity == 0 else '(run nil)' if arity == 1 else None
            if not required or reviewed['invoke'] != required:
                raise ValueError('original exported callback invocation arity drift')
        if case['expected']['outcome']['status'] != 'returned' or case['expected']['trace'] != [] or case['expected']['stdout'] != '':
            raise ValueError('original literal result/effect contract drift')
        result.append({**reviewed, 'case': case, 'source': source})
    return result


def operation_evidence(source, selected, target):
    name = selected['call_name']
    mode = selected['evidence_mode']
    if mode == 'binding-control':
        result = []
        def visit(node):
            if node.kind in {'quote', 'syntax-quote', 'discard', 'var'}: return
            if node.kind == 'list' and len(node.children) > 2 and node.children[0].token == 'binding':
                bindings = node.children[1]
                if bindings.kind == 'vector':
                    for index in range(0, len(bindings.children)-1, 2):
                        if bindings.children[index].token == name:
                            text = source[node.start:node.end]
                            result.append({'control_name': name,
                                'bound_value_source': source[bindings.children[index+1].start:bindings.children[index+1].end],
                                'binding_source': text, 'binding_source_sha256': digest(text)})
            for child in node.children: visit(child)
        for node in Reader(source).all(): visit(node)
        if not result: raise ValueError('absent demanded printing binding control')
        return result
    if mode == 'higher-order':
        calls = []
        def visit(node):
            if node.kind in {'quote','syntax-quote','discard','var'}: return
            if head(node) == selected['consumer'] and len(node.children)>selected['callback_position'] and node.children[selected['callback_position']].token == target['name']:
                text=source[node.start:node.end]
                calls.append({'argument_count':1,'consumer_source':text,'consumer_source_sha256':digest(text),'callback_name':target['name'],'consumer':selected['consumer']})
            for child in node.children: visit(child)
        for node in Reader(source).all():visit(node)
        if not calls or selected['direct_arities'] != [1]:raise ValueError('absent original demanded one-argument callback')
        signature=target.get('signature')
        if signature and (signature['kind']!='fixed' or signature['minimum']!=1):raise ValueError('callback signature is not exact unary operation')
        return calls
    if mode == 'protocol-receiver':
        calls = [call for call in direct_calls(source,'satisfies?') if call['call_source'].startswith('(satisfies? '+target['name']+' ') and call['argument_count']==2]
        if not calls or selected['direct_arities'] != []:raise ValueError('absent original explicit protocol receiver demand')
        for required in ['factory ->StringBufferWriter','write -write','flush -flush','(write a "a")','(apply write [b "b"])','(cljs.core/-write c "c")','(flush a)','@events']:
            if required not in source:raise ValueError('original protocol receiver method/callback demand omitted')
        return calls
    calls = [call for call in direct_calls(source, name) if call['argument_count'] in selected['direct_arities']]
    if {call['argument_count'] for call in calls} != set(selected['direct_arities']):
        raise ValueError('absent reviewed actual source calls')
    if mode == 'applied-alias':
        definitions = []
        def visit(node):
            if node.kind == 'list' and len(node.children)>3 and node.children[0].token == 'defn' and node.children[1].token == name:
                definitions.append(node)
            for child in node.children: visit(child)
        for node in Reader(source).all(): visit(node)
        if len(definitions)!=1:
            raise ValueError('missing original apply alias definition')
        definition=definitions[0]
        if source[definition.children[2].start:definition.children[2].end] != '[& args]' or source[definition.children[3].start:definition.children[3].end] != '(apply '+target['name']+' args)':
            raise ValueError('original alias apply argument forwarding drift')
        for call in calls:
            call['applied_operation']=target['name']
            call['alias_definition_source']=source[definition.start:definition.end]
    signature=target.get('signature')
    if signature and signature['kind'] in {'fixed','variadic'} and not any(call['argument_count'] == signature['minimum'] if signature['kind']=='fixed' else call['argument_count']>=signature['minimum'] if selected.get('allow_variadic_minimum') else call['argument_count']>signature['minimum'] for call in calls):
        raise ValueError('no exact signature invocation demand')
    return calls


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
        calls = operation_evidence(row['source'], selected, target)
        basis = demand_basis(row)
        rationale = (basis + ' The entire unchanged original oracle, setup, body, loops, literals, effects and errors execute through the exact driver in fresh/reused actual MacroHost applications and macro computations, observed by QuickJS and Node. Both required public pins execute the exact same existing oracle. Declaration, source registration, function identity and undemanded callback paths provide no credit. Local mappings cross-check the operation, with no complete metadata-value claim.')
        common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'], 'rationale': rationale}
        if signature:
            common['signature_sha256'] = selected['signature_sha256']
        ordinary.append({**common, 'assertion_ids': [], 'local_execution': execution, 'reference_fixture_id': row['id'], 'reference_source_sha256': digest(row['case']['source']), 'reference_contract_sha256': digest(json.dumps(row['case']['expected'], sort_keys=True))})
        phases.append({**common, **execution, 'support_source_sha256': {path: digest((ROOT / path).read_bytes()) for path in [HELPER, SCANNER, row['fixture']]}})
        receipts.append({**selected, 'direct_calls': calls, 'whole_source_sha256': row['source_sha256'], 'original_case_sha256': row['case_sha256'], 'demand_basis': basis, 'invocation': row['invoke'] or 'complete expression', 'oracle_id': row['id']})
    return {'obligation-witnesses.jsonl': ordinary, 'phase-witnesses.jsonl': phases, 'arity-after188-call-receipts.jsonl': receipts}


def parse_frames(output):
    expected = [(row['id'], host, phase, engine) for row in rows() for host in ['fresh', 'reused'] for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
    frames = []
    for line in output.splitlines():
        if not line.startswith('arity-after188-observation\t'):
            continue
        fields = line.split('\t')
        if len(fields) != 6 or fields[5] != EXPECTED_LITERAL_RAW.get(fields[1]):
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
