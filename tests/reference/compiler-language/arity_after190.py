#!/usr/bin/env python3
"""New demanded public/local operations after190; complete original sources unchanged."""
from pathlib import Path
import argparse
import json
import re
from extract_inventory import Reader
from arity_resume_candidates import digest, direct_calls, head
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'tests/reference/compiler-language'
HELPER = 'tests/reference/compiler-language/arity_after190.py'
SCANNER = 'tests/reference/compiler-language/arity_resume_candidates.py'
DRIVER = 'tests/compiler_arity_after190.rs'
TEST = 'new_arity_oracles_execute_in_actual_phases_and_lifecycles'
SCANNER_SHA256 = '31457848fe6d2f08e92b3ad5b4a6f99362a97be66e4819c9d45cf3a1b9ecbcc3'
PRINT_ID = 'arity-after190-canonical-print-meta-unreadable'
PRINT_SCI_ID = 'sci-string-pr-meta-unreadable'
PRINT_SUPPORT = ['tests/reference/compiler-language/sci_print_context.py',
    'tests/reference/compiler-language/sci_print_candidates.py',
    'tests/reference/compiler-language/sci-print-canonical-contracts.jsonl',
    'tests/reference/sci/evaluator-dispositions.tsv',
    'tests/fixtures/sci-outcomes.tsv',
    'tests/reference/compiler-language/sci_execution.py']


def supplemental_reference_cases():
    import sci_print_context
    rows = [row for row in sci_print_context.load() if row['sci_id'] == PRINT_SCI_ID]
    if len(rows) != 1:
        raise ValueError('original accepted canonical printing context missing')
    row = rows[0]
    import sci_execution
    original_line, original_fields = sci_execution.rows(sci_execution.CORPUS)[PRINT_SCI_ID]
    retained_line, retained_fields = sci_execution.rows(sci_execution.DISPOSITIONS)[PRINT_SCI_ID]
    if row['source_sha256'] != '04876926079dacc7d995a80304ca21884ed6c7d8c8009dfa24f3257a16800ccd' or row['canonical_expected']['outcome'] != {'status': 'returned', 'value': '^{:x true} [x]'}:
        raise ValueError('original complete printing source/canonical literal drift')
    return {PRINT_ID: {'id': PRINT_ID, 'source': row['source'],
        'expected': {**row['canonical_expected'], 'id': PRINT_ID}, 'native_stdout': None,
        'canonical_printing_provenance': {
            'sci_id': row['sci_id'], 'original_expected': row['original_expected'],
            'original_row': original_line, 'original_fields': original_fields,
            'retained_disposition_row': retained_line, 'retained_disposition_fields': retained_fields,
            'original_row_sha256': row['original_row_sha256'], 'body_sha256': row['body_sha256'],
            'original_source_sha256': row['original_source_sha256'],
            'original_semantic_pass_credit': 0, 'original_expectation_status': row['original_expectation_status'],
            'scope_basis': row['scope_basis']}}}


REVIEWED = [{'case_sha256': 'ba0f8526d6a7da4acec44ba0de3e781e0529e2e88664ba705e66e0c3ac5fcd86',
  'fixture': 'tests/fixtures/arity-after190/cljs-definitions-var-test.cljs',
  'id': 'cljs-definitions-var-test',
  'invoke': '(run nil)',
  'namespace': 'cljs.var-test',
  'source_sha256': 'a6aea8026522804962ec49a6f5e2531c482eb9830c1c16b5962cf21f22b847b0'},
 {'case_sha256': '4fca8ba2d85701fc3a34da508bc3afd87ab4eece380c4112d4f3c78a11ebe490',
  'fixture': 'tests/fixtures/arity-after190/cljs-public-gate-core-factory-multifn.cljs',
  'id': 'cljs-public-gate-core-factory-multifn',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '1ce4500d472c752bb9a52dc982c69278c31ea7af712e879692bd2b45c30b4201'},
 {'case_sha256': '2e0c98842a15caf22ea572abfa1bed2d6e4940575adfb896d21ddeaeab96b92e',
  'fixture': 'tests/fixtures/arity-after190/cljs-helpers-map-entry-test-map-entry-interface-tests.cljs',
  'id': 'cljs-helpers-map-entry-test-map-entry-interface-tests',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '2392cae169f6a888cc5bfd59922962c7d9b5dee00c9312aff72f1aa93cf7b537'},
 {'case_sha256': 'b3cd21e4d58106db32d9d83fbaa228ccc84b06f71320a272eff015bb43603df5',
  'fixture': 'tests/fixtures/arity-after190/arity-receiver-transient.cljs',
  'id': 'arity-receiver-transient',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'b82b28e41eb87ad82cf154cb698e54b5a1c5444a1f07656dc942e08acf4da312'},
 {'case_sha256': '16f785748af7192cd9f6524cabe5eb5fe1a20cb08c962e1bb7b192e97fbf5846',
  'fixture': 'tests/fixtures/arity-after190/cljs-core-gate-literal-protocol-public.cljs',
  'id': 'cljs-core-gate-literal-protocol-public',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'a3a23c6d76d6025a78ebbb0576ee2a3ec20397936dcd2abf917db9132f4dd838'},
 {'case_sha256': '15c78805f2b3a6de689aa761860a93807ad501bda1058aa35da73885df4c4c46',
  'fixture': 'tests/fixtures/arity-after190/cljs-core-gate-literal-protocol-custom-state.cljs',
  'id': 'cljs-core-gate-literal-protocol-custom-state',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '107f95a1f1f6ca08d54701d9ca7820f8cc22ca8f135c851ab48ce04300c12d8b'},
 {'case_sha256': '685ce12484396dbf2329d01c6502f4c3777c06bf6a1e85709ac7619c1c41cd7b',
  'fixture': 'tests/fixtures/arity-after190/cljs-core-gate-literal-clone-find-protocol-workflow.cljs',
  'id': 'cljs-core-gate-literal-clone-find-protocol-workflow',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '3d3f97342b6da7466315ebf60445c53632761c92f5cf29010dc2a115e7244cb0'},
 {'case_sha256': 'fe788de8eb3985e1a2981a12d11ac672d342a8847b074c8bdf7cad189759418d',
  'fixture': 'tests/fixtures/arity-after190/cljs-core-gate-literal-chunk-public-factories.cljs',
  'id': 'cljs-core-gate-literal-chunk-public-factories',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '94b1bbfd90944bb09b77286e59307a447171c14566f1ed2ff526f5a136ef9492'},
 {'case_sha256': 'a6402f204a4bde3cc178cd8b02b1017826655766619c8a57c1b310b8d107d55d',
  'fixture': 'tests/fixtures/arity-after190/cljs-core-gate-literal-chunk-public-pipeline.cljs',
  'id': 'cljs-core-gate-literal-chunk-public-pipeline',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'e6e04b318e013694b3f420fc75ade115c5adf461eeaef99f2416cad3ec6edeb3'},
 {'case_sha256': 'a0ed23051e7847fefd522627714a9852b500a3791e6af4646d8dc288b7808fe0',
  'fixture': 'tests/fixtures/arity-after190/cljs-sequence-completion-src-test-cljs-cljs-iterator-test-coll-iter-seq-match.cljs',
  'id': 'cljs-sequence-completion-src-test-cljs-cljs-iterator-test-coll-iter-seq-match',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '2fd292e05196ebd844c6263afb955e363e538dbe49f9d3c9587f80b90bcccefb'},
 {'case_sha256': 'ef036ff6bf8f5f4577547d0813f6abf9a8bbf170845816f00c937da95559aac3',
  'fixture': 'tests/fixtures/arity-after190/arity-after190-canonical-print-meta-unreadable.cljs',
  'id': 'arity-after190-canonical-print-meta-unreadable',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '04876926079dacc7d995a80304ca21884ed6c7d8c8009dfa24f3257a16800ccd'},
 {'case_sha256': 'e5f3c3d6d3f326bbd13a1b991f520e48d0efed2dad34a901cb21395fe431c358',
  'fixture': 'tests/fixtures/arity-after190/arity-receiver-sorted-full.cljs',
  'id': 'arity-receiver-sorted-full',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'dc24ea48b6a2338cd7920d58d9445e3880df41c5bc7223156d2083dabe66b04c'},
 {'case_sha256': '2219e88deea01a1a15f32341738ed82e4c0683df55434e9b7fd20f0c22e6b6e6',
  'fixture': 'tests/fixtures/arity-after190/cljs-extension-test-cljs-2812.cljs',
  'id': 'cljs-extension-test-cljs-2812',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'c4362e731578fa8c620d8eb91e71aa719731640e3d2c790b57a8e08f98ec3d41'},
 {'case_sha256': 'e9887d0eb87fada34e3be5a7b33866c6aa72f31534f1c3e4a02cd27d15e635cb',
  'fixture': 'tests/fixtures/arity-after190/cljs-deftype-core.cljs',
  'id': 'cljs-deftype-core',
  'invoke': '(run nil)',
  'namespace': 'cljs.core-test',
  'source_sha256': '025aae2c8df87d759d4c90b0b282d5250b6030e56be034b37c243c062b6d6f10'},
 {'case_sha256': '8141189b072b79822572ac7654da5f85bb02d83f2e1d7ce99f1740e6cc7b7b92',
  'fixture': 'tests/fixtures/arity-after190/cljs-sequences-collections-test-test-string.cljs',
  'id': 'cljs-sequences-collections-test-test-string',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'f6f27f4ee93b3deca37e4abd4300c8ffc73b56dd5cc4b6957176eb9cff7e5de7'},
 {'case_sha256': '9c996ae7b14a0f92764765f41d999190cbdbeb979d464d8f98a0da2d810b3b5f',
  'fixture': 'tests/fixtures/arity-after190/arity-extended-language-helper-js-conversions.cljs',
  'id': 'arity-extended-language-helper-js-conversions',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': '7cd3195fb8cc052319b3bcbe518d5242821e4bb2d277d8d9ffa959f101578912'},
 {'case_sha256': 'a0864e85a4306d611dc5619b4d4e464bca563519a3ce430184c6536d2e688466',
  'fixture': 'tests/fixtures/arity-after190/cljs-extra-collections-test-phm-fixed-hash.cljs',
  'id': 'cljs-extra-collections-test-phm-fixed-hash',
  'invoke': '',
  'namespace': 'user',
  'source_sha256': 'bf8b4b017436bf08a84b638b110f9ebd405765e8e465d9fd5cd5c159c751f649'},
 {'case_sha256': '6fab4832762673b76f70a754771b5fadf36424c927b0c183c060bd1e180fdb90',
  'companion_sources': {'clojure/set.cljs': {'fixture': 'tests/fixtures/arity-after190/source-context/cljs-namespace-completion-require-alias/clojure/set.cljs',
                                             'original_fixture': 'tests/fixtures/compiler-namespaces/namespace-completion/require-alias/clojure/set.cljs',
                                             'source_sha256': '052c80d95b5caabb9f7dca8947ea3d0451cea3fb2a527c963061fdad6d158654'}},
  'fixture': 'tests/fixtures/arity-after190/cljs-namespace-completion-require-alias.cljs',
  'id': 'cljs-namespace-completion-require-alias',
  'invoke': '(run nil)',
  'namespace': 'probe.require-alias',
  'source_sha256': '7a23866ebf7b65bb388f3346e1b82584ccb37c6ab75bf2915e8ed978fb034ef7'},
 {'case_sha256': '9940cab87db18423e986ceece77509c29b37ac416dcaef3448a2511d9ae64aa2',
  'companion_sources': {'clojure/set.cljs': {'fixture': 'tests/fixtures/arity-after190/source-context/cljs-namespace-completion-use/clojure/set.cljs',
                                             'original_fixture': 'tests/fixtures/compiler-namespaces/namespace-completion/use/clojure/set.cljs',
                                             'source_sha256': '052c80d95b5caabb9f7dca8947ea3d0451cea3fb2a527c963061fdad6d158654'}},
  'fixture': 'tests/fixtures/arity-after190/cljs-namespace-completion-use.cljs',
  'id': 'cljs-namespace-completion-use',
  'invoke': '(run nil)',
  'namespace': 'use.probe',
  'source_sha256': 'a29dcac53e0c2da849a8df00e4771b6614139c6def48d224ee891c30975d29d8'},
 {'case_sha256': '4c16c62ca58d0c31542f770d7c58d8721d257a68055c9334d4e493a74e4498cb',
  'companion_sources': {},
  'fixture': 'tests/fixtures/arity-after190/cljs-namespace-completion-refer-core-exclude.cljs',
  'id': 'cljs-namespace-completion-refer-core-exclude',
  'invoke': '(run nil)',
  'namespace': 'refer.exclude',
  'source_sha256': 'c06f3ec88bb6e96ea4d43d3bec565fbb1f745352642137b433726623cfe793fb'},
 {'case_sha256': '5151291124a42201caf2224e144184815703720dd93288e903e706c4ceb42e09',
  'companion_sources': {},
  'fixture': 'tests/fixtures/arity-after190/cljs-namespace-completion-refer-global.cljs',
  'id': 'cljs-namespace-completion-refer-global',
  'invoke': '(run nil)',
  'namespace': 'global.probe',
  'source_sha256': '3653ced173383e356c8dfd501a4412d6a364d4e6229ed2cec4d554fb62dfd12a'}]
SELECTION = [{'body_sha256': 'cb90c7a0018fa171249cc89399b844b1ee3841ed859ab96d8ee071cdcdbc858c',
  'call_name': 'test',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::test::arity-1',
  'signature_sha256': 'c36cd365c19c27aafad3a2c8b38fe680298a1092cb7d2ad4ecc5ec845122e745',
  'workflow': 'cljs-definitions-var-test'},
 {'body_sha256': '06b6629193d00596290ed96790200f1ee802f1e358d9da83a642f32217e504e8',
  'call_name': 'test',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::test',
  'signature_sha256': None,
  'workflow': 'cljs-definitions-var-test'},
 {'body_sha256': 'd78a9c163e26e950f2ecc34655cd87117331663f1e6910e00c6ad1c420b9a64f',
  'call_name': 'dispatch-fn',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::dispatch-fn',
  'signature_sha256': None,
  'workflow': 'cljs-public-gate-core-factory-multifn'},
 {'body_sha256': 'b8d01df32f4a96966b38cbbef68e64fdbed78d2e578a7a2f05b1d49f59c58761',
  'call_name': 'satisfies?',
  'direct_arities': [],
  'evidence_mode': 'protocol-receiver',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IMultiFn::arity-1',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-public-gate-core-factory-multifn'},
 {'body_sha256': '59bb6bd1f1c78b3cb660b54d3d072e6d267a9488982d5c366c31e001da14ec2e',
  'call_name': 'satisfies?',
  'direct_arities': [],
  'evidence_mode': 'protocol-receiver',
  'obligation_id': 'local::src/core_var_metadata.tsv::IMultiFn',
  'signature_sha256': None,
  'workflow': 'cljs-public-gate-core-factory-multifn'},
 {'body_sha256': '275ee54cd85042a469f25e31bf748dab6a706fc199209cb15f196dc869d7f240',
  'call_name': '-lookup',
  'direct_arities': [2, 3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ILookup::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ILookup::arity-1',
  'protocol_body_sha256': '275ee54cd85042a469f25e31bf748dab6a706fc199209cb15f196dc869d7f240',
  'protocol_name': 'ILookup',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '8e4bf24fc661edbbb7d77cf8b768fcc8a781023b32eea8790c17a201c619d1c6',
  'call_name': '-lookup',
  'direct_arities': [2, 3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ILookup',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ILookup::arity-1',
  'protocol_body_sha256': '275ee54cd85042a469f25e31bf748dab6a706fc199209cb15f196dc869d7f240',
  'protocol_name': 'ILookup',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '6e15b47b8228c88c91c4dc857bb95d56984663a9011fcc29940cd4209229ee2f',
  'call_name': '-peek',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IStack::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IStack::arity-1',
  'protocol_body_sha256': '6e15b47b8228c88c91c4dc857bb95d56984663a9011fcc29940cd4209229ee2f',
  'protocol_name': 'IStack',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '490d6422a5222f0ed36a8b7d709c420fcec9d3fd5e4f344e3deff59f01912a45',
  'call_name': '-peek',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IStack',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IStack::arity-1',
  'protocol_body_sha256': '6e15b47b8228c88c91c4dc857bb95d56984663a9011fcc29940cd4209229ee2f',
  'protocol_name': 'IStack',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': 'd859caa3b42c3044cd83deffcc65783255d01e4df70bf29ff1edf50e7147d111',
  'call_name': '-conj',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ICollection::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ICollection::arity-1',
  'protocol_body_sha256': 'd859caa3b42c3044cd83deffcc65783255d01e4df70bf29ff1edf50e7147d111',
  'protocol_name': 'ICollection',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': 'd1d92884d3e5a214001b4cd4daf979bfe111ee7c518efe3c0ca29e8e4e5cc92a',
  'call_name': '-conj',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ICollection',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ICollection::arity-1',
  'protocol_body_sha256': 'd859caa3b42c3044cd83deffcc65783255d01e4df70bf29ff1edf50e7147d111',
  'protocol_name': 'ICollection',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '71975c1a52326c8a4dca6392f54bce241501207e74fbc926338eeaecfa7b03b0',
  'call_name': '-count',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ICounted::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ICounted::arity-1',
  'protocol_body_sha256': '71975c1a52326c8a4dca6392f54bce241501207e74fbc926338eeaecfa7b03b0',
  'protocol_name': 'ICounted',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '60f3c1b17f86f1452900a736f68111777b3f44d83ff0698db5f5f700f22132d4',
  'call_name': '-count',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ICounted',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ICounted::arity-1',
  'protocol_body_sha256': '71975c1a52326c8a4dca6392f54bce241501207e74fbc926338eeaecfa7b03b0',
  'protocol_name': 'ICounted',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '82c66d215a92cd1303649660aeb16ddf15ede1cd2fbefa8d821648bc6da82ec8',
  'call_name': '-nth',
  'direct_arities': [2, 3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IIndexed::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IIndexed::arity-1',
  'protocol_body_sha256': '82c66d215a92cd1303649660aeb16ddf15ede1cd2fbefa8d821648bc6da82ec8',
  'protocol_name': 'IIndexed',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': 'ed72fb6b0bc545ec57184cf4d0e37e20ad2a6dfd9571464d975d7ce416ee2c90',
  'call_name': '-nth',
  'direct_arities': [2, 3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IIndexed',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IIndexed::arity-1',
  'protocol_body_sha256': '82c66d215a92cd1303649660aeb16ddf15ede1cd2fbefa8d821648bc6da82ec8',
  'protocol_name': 'IIndexed',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '3f6870b2db2e5474e432dfe1cf2f84d158ac218c60d8231a35a3049d7add2714',
  'call_name': '-assoc',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IAssociative::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IAssociative::arity-1',
  'protocol_body_sha256': '3f6870b2db2e5474e432dfe1cf2f84d158ac218c60d8231a35a3049d7add2714',
  'protocol_name': 'IAssociative',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': 'daaf09135bd7ea2725601eeebd65cd5dbcb697b5152637c159b0cf6de0d63921',
  'call_name': '-assoc',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IAssociative',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IAssociative::arity-1',
  'protocol_body_sha256': '3f6870b2db2e5474e432dfe1cf2f84d158ac218c60d8231a35a3049d7add2714',
  'protocol_name': 'IAssociative',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '11ba143537d3bff1a6301f2dc615f23821727698d0689a19fcc15629ee30b674',
  'call_name': '-assoc-n',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IVector::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IVector::arity-1',
  'protocol_body_sha256': '11ba143537d3bff1a6301f2dc615f23821727698d0689a19fcc15629ee30b674',
  'protocol_name': 'IVector',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '4fabc0c998edd236b9f98afc3a26961919647e6c92cc8f4dde1e12cea1099b61',
  'call_name': '-assoc-n',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IVector',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IVector::arity-1',
  'protocol_body_sha256': '11ba143537d3bff1a6301f2dc615f23821727698d0689a19fcc15629ee30b674',
  'protocol_name': 'IVector',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '34ebd535cd77954f9e52867a81e45d73d689ed69a23ff7c5acff253783ee94c3',
  'call_name': '-reduce',
  'direct_arities': [2, 3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IReduce::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IReduce::arity-1',
  'protocol_body_sha256': '34ebd535cd77954f9e52867a81e45d73d689ed69a23ff7c5acff253783ee94c3',
  'protocol_name': 'IReduce',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '496a6be8701298b415dd478a2561aff995082715204352c653a8342c63ec9d97',
  'call_name': '-reduce',
  'direct_arities': [2, 3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IReduce',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IReduce::arity-1',
  'protocol_body_sha256': '34ebd535cd77954f9e52867a81e45d73d689ed69a23ff7c5acff253783ee94c3',
  'protocol_name': 'IReduce',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': 'c895c1930ab96a50b9bef061e299930168ac5901cefd417aaab752e67a23f123',
  'call_name': '-find',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IFind::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IFind::arity-1',
  'protocol_body_sha256': 'c895c1930ab96a50b9bef061e299930168ac5901cefd417aaab752e67a23f123',
  'protocol_name': 'IFind',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '21886a76249a0a4b2947a9827128548f1f38775ac526ad9050154d93ed462179',
  'call_name': '-find',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IFind',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IFind::arity-1',
  'protocol_body_sha256': 'c895c1930ab96a50b9bef061e299930168ac5901cefd417aaab752e67a23f123',
  'protocol_name': 'IFind',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '66a501ee1c396921fa51782064c2e6f9e979b1416299ec29d225255fd7098d91',
  'call_name': '-rseq',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IReversible::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IReversible::arity-1',
  'protocol_body_sha256': '66a501ee1c396921fa51782064c2e6f9e979b1416299ec29d225255fd7098d91',
  'protocol_name': 'IReversible',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '10e58d2a6136b70b07e1e72161db6f95930d5db2e225666d7326fb84dc81d6bf',
  'call_name': '-rseq',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IReversible',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IReversible::arity-1',
  'protocol_body_sha256': '66a501ee1c396921fa51782064c2e6f9e979b1416299ec29d225255fd7098d91',
  'protocol_name': 'IReversible',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '3c27b5f35861117d58755c1b264c9e9d741ad04b1dd1a92bcc68c6fe70bbb211',
  'call_name': 'satisfies?',
  'consumption_arities': [1],
  'consumption_name': '-seq',
  'direct_arities': [],
  'evidence_mode': 'protocol-marker-receiver',
  'marker': True,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ISequential::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISequential::arity-1',
  'protocol_body_sha256': '3c27b5f35861117d58755c1b264c9e9d741ad04b1dd1a92bcc68c6fe70bbb211',
  'protocol_name': 'ISequential',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': 'f877b83037ac65747edd253731b7e864b36350c2f91da175dcf3fd321aa5376c',
  'call_name': 'satisfies?',
  'consumption_arities': [1],
  'consumption_name': '-seq',
  'direct_arities': [],
  'evidence_mode': 'protocol-marker-receiver',
  'marker': True,
  'obligation_id': 'local::src/core_var_metadata.tsv::ISequential',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISequential::arity-1',
  'protocol_body_sha256': '3c27b5f35861117d58755c1b264c9e9d741ad04b1dd1a92bcc68c6fe70bbb211',
  'protocol_name': 'ISequential',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': 'fbad3ba281216492870e17bd13e832af8b27cca0d808981ef64c5364a71f258d',
  'call_name': '-equiv',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IEquiv',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IEquiv::arity-1',
  'protocol_body_sha256': '84667a2dd901b6a2f5d75518a34c3d7f30fb58fc0044a7414ca3a298fb6e3e1b',
  'protocol_name': 'IEquiv',
  'receiver': 'x',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': 'ff1eeefbd0e4d2ecf8756b4ca658be9b718c494e5cd5eee8f1dd6564a52635f9',
  'call_name': '-seq',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ISeqable',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISeqable::arity-1',
  'protocol_body_sha256': '0e2ceddeb51407ef720b7555233407e49ef4103c6d9eb2dbc77d9838bde7aa69',
  'protocol_name': 'ISeqable',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '224e74ae2594ed199e6d75284f2c50578243b4721ab906c4e1050ff88025857e',
  'call_name': '-as-transient',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IEditableCollection::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IEditableCollection::arity-1',
  'protocol_body_sha256': '224e74ae2594ed199e6d75284f2c50578243b4721ab906c4e1050ff88025857e',
  'protocol_name': 'IEditableCollection',
  'receiver': 'custom',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': 'bb5c3127c8ddac4d417da43affc70c607f93f78e0f3592ea5b7c013f6dd5733b',
  'call_name': '-as-transient',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IEditableCollection',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IEditableCollection::arity-1',
  'protocol_body_sha256': '224e74ae2594ed199e6d75284f2c50578243b4721ab906c4e1050ff88025857e',
  'protocol_name': 'IEditableCollection',
  'receiver': 'custom',
  'signature_sha256': None,
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': '8da0a1d06ad99fd6f778f1eb5ab53be6a50cb7a9927f286e29fa4c02fcbb4c5d',
  'call_name': '-conj!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ITransientCollection::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientCollection::arity-1',
  'protocol_body_sha256': '8da0a1d06ad99fd6f778f1eb5ab53be6a50cb7a9927f286e29fa4c02fcbb4c5d',
  'protocol_name': 'ITransientCollection',
  'receiver': 'custom',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': '4b227131f66d2c8b4b5c7450c37e6fcdc23d4e1381072a04715f69197b6b6e94',
  'call_name': '-conj!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ITransientCollection',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientCollection::arity-1',
  'protocol_body_sha256': '8da0a1d06ad99fd6f778f1eb5ab53be6a50cb7a9927f286e29fa4c02fcbb4c5d',
  'protocol_name': 'ITransientCollection',
  'receiver': 'custom',
  'signature_sha256': None,
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': '4a125d21134326f4d165048296682b8e94d9b99fb5dc3ee7f6bf9170f1a05d60',
  'call_name': '-assoc!',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ITransientAssociative::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientAssociative::arity-1',
  'protocol_body_sha256': '4a125d21134326f4d165048296682b8e94d9b99fb5dc3ee7f6bf9170f1a05d60',
  'protocol_name': 'ITransientAssociative',
  'receiver': 'custom',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': '9e0e10bebca3459648028eb906e73e60adae5535bd8fe9350757c4fd0a7e509c',
  'call_name': '-assoc!',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ITransientAssociative',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientAssociative::arity-1',
  'protocol_body_sha256': '4a125d21134326f4d165048296682b8e94d9b99fb5dc3ee7f6bf9170f1a05d60',
  'protocol_name': 'ITransientAssociative',
  'receiver': 'custom',
  'signature_sha256': None,
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': '9aedbc443395aee3bb157f3fbeab91205c89ec8c5234f55ad2b671122b765bfb',
  'call_name': '-dissoc!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ITransientMap::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientMap::arity-1',
  'protocol_body_sha256': '9aedbc443395aee3bb157f3fbeab91205c89ec8c5234f55ad2b671122b765bfb',
  'protocol_name': 'ITransientMap',
  'receiver': 'custom',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': '66932fd68f00b68c213f883c2649084f56d7aadced8dba07a68b854bd6833067',
  'call_name': '-dissoc!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ITransientMap',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientMap::arity-1',
  'protocol_body_sha256': '9aedbc443395aee3bb157f3fbeab91205c89ec8c5234f55ad2b671122b765bfb',
  'protocol_name': 'ITransientMap',
  'receiver': 'custom',
  'signature_sha256': None,
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': '1664c2f3b4eabe68c2ab11e4f0c05ec1d2e343421f489c3551a2fd3b944e704d',
  'call_name': '-assoc-n!',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ITransientVector::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientVector::arity-1',
  'protocol_body_sha256': '1664c2f3b4eabe68c2ab11e4f0c05ec1d2e343421f489c3551a2fd3b944e704d',
  'protocol_name': 'ITransientVector',
  'receiver': 'custom',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': '74cb38fa8840ee68bcfc9e1d5ab6676fe11160b21287a8581754ffd3c2e9fd96',
  'call_name': '-assoc-n!',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ITransientVector',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientVector::arity-1',
  'protocol_body_sha256': '1664c2f3b4eabe68c2ab11e4f0c05ec1d2e343421f489c3551a2fd3b944e704d',
  'protocol_name': 'ITransientVector',
  'receiver': 'custom',
  'signature_sha256': None,
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': 'ccae9ee350c3e7bbb71d98b0af1b5736d4f0a2cff9f9bf0a90e964175786d2e1',
  'call_name': '-disjoin!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ITransientSet::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientSet::arity-1',
  'protocol_body_sha256': 'ccae9ee350c3e7bbb71d98b0af1b5736d4f0a2cff9f9bf0a90e964175786d2e1',
  'protocol_name': 'ITransientSet',
  'receiver': 'custom',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': '8c1ae951bca48b97d80ec3f02ceff3555da92c7eaf9e8d58bab1dacc36321ac8',
  'call_name': '-disjoin!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ITransientSet',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ITransientSet::arity-1',
  'protocol_body_sha256': 'ccae9ee350c3e7bbb71d98b0af1b5736d4f0a2cff9f9bf0a90e964175786d2e1',
  'protocol_name': 'ITransientSet',
  'receiver': 'custom',
  'signature_sha256': None,
  'workflow': 'arity-receiver-transient'},
 {'body_sha256': 'c87f30b5c43f8b83064f1bc9f55ce559cff143cb7a0aed034d01542c055b6828',
  'call_name': '-empty',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IEmptyableCollection::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IEmptyableCollection::arity-1',
  'protocol_body_sha256': 'c87f30b5c43f8b83064f1bc9f55ce559cff143cb7a0aed034d01542c055b6828',
  'protocol_name': 'IEmptyableCollection',
  'receiver': 'xs',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': 'c76aa04bf5f53ed95f716fed68ae41f15ee56fe4e9683b4f51ad921c633a33cf',
  'call_name': '-empty',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IEmptyableCollection',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IEmptyableCollection::arity-1',
  'protocol_body_sha256': 'c87f30b5c43f8b83064f1bc9f55ce559cff143cb7a0aed034d01542c055b6828',
  'protocol_name': 'IEmptyableCollection',
  'receiver': 'xs',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': '3af4538e9e0ada81787751a9b454dad3144468071d0436205c01a93b51fadaca',
  'call_name': '-first',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ISeq',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISeq::arity-1',
  'protocol_body_sha256': '1aa5bf12a8efbcfbaa355ee10ce4f6014c7b5dfafe48b626a42e16c1bea60ddb',
  'protocol_name': 'ISeq',
  'receiver': '(list 1 2)',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': '6cfe3852875871148bae804264bf6e6b4ee8633926fc5c21b994ddf2c2dcf19c',
  'call_name': '-next',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::INext::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::INext::arity-1',
  'protocol_body_sha256': '6cfe3852875871148bae804264bf6e6b4ee8633926fc5c21b994ddf2c2dcf19c',
  'protocol_name': 'INext',
  'receiver': '(list 1 2)',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': '845390bbf6893119741694c9a053943a574b5635920528e31f8a0f2376556a0d',
  'call_name': '-next',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::INext',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::INext::arity-1',
  'protocol_body_sha256': '6cfe3852875871148bae804264bf6e6b4ee8633926fc5c21b994ddf2c2dcf19c',
  'protocol_name': 'INext',
  'receiver': '(list 1 2)',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': 'ea524a99cbc3a2a63a20d9fde06cc2df85b561e45a933c5d641d44954b9a0200',
  'call_name': '-dissoc',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IMap::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IMap::arity-1',
  'protocol_body_sha256': 'ea524a99cbc3a2a63a20d9fde06cc2df85b561e45a933c5d641d44954b9a0200',
  'protocol_name': 'IMap',
  'receiver': '{:a 1 :b 2}',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': 'f4f07bf7ded72f69b097f26ca468ed7168d9b0349a6ccbdc1d9ec1369557aabd',
  'call_name': '-dissoc',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IMap',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IMap::arity-1',
  'protocol_body_sha256': 'ea524a99cbc3a2a63a20d9fde06cc2df85b561e45a933c5d641d44954b9a0200',
  'protocol_name': 'IMap',
  'receiver': '{:a 1 :b 2}',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': 'fc214ee9f3f4fca4632116e9f28e3d51a2f540c2acceecf8d63f0e4ea7845e6d',
  'call_name': '-disjoin',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ISet::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISet::arity-1',
  'protocol_body_sha256': 'fc214ee9f3f4fca4632116e9f28e3d51a2f540c2acceecf8d63f0e4ea7845e6d',
  'protocol_name': 'ISet',
  'receiver': '#{1 2}',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': 'cd4fe110b5b924374ecba6d094f8b18bf417e055002731a4434505b41ea8e0b7',
  'call_name': '-disjoin',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ISet',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISet::arity-1',
  'protocol_body_sha256': 'fc214ee9f3f4fca4632116e9f28e3d51a2f540c2acceecf8d63f0e4ea7845e6d',
  'protocol_name': 'ISet',
  'receiver': '#{1 2}',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': 'ee9f827657e98068d4628e76fe98f82d279b46ef7745efac02ee529a826bb018',
  'call_name': '-key',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IMapEntry::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IMapEntry::arity-1',
  'protocol_body_sha256': 'ee9f827657e98068d4628e76fe98f82d279b46ef7745efac02ee529a826bb018',
  'protocol_name': 'IMapEntry',
  'receiver': 'e',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': '4c0eada95db622950a9254533f7edf29634f66bacd0f2946b7024d5c1fb46f39',
  'call_name': '-key',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IMapEntry',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IMapEntry::arity-1',
  'protocol_body_sha256': 'ee9f827657e98068d4628e76fe98f82d279b46ef7745efac02ee529a826bb018',
  'protocol_name': 'IMapEntry',
  'receiver': 'e',
  'signature_sha256': None,
  'workflow': 'cljs-helpers-map-entry-test-map-entry-interface-tests'},
 {'body_sha256': 'd3242dd4d16577be7c93e207212209f5f5b8b383f31ae7bf93fb274147438a34',
  'call_name': '-deref',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IDeref::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IDeref::arity-1',
  'protocol_body_sha256': 'd3242dd4d16577be7c93e207212209f5f5b8b383f31ae7bf93fb274147438a34',
  'protocol_name': 'IDeref',
  'receiver': 'a',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': '53d9e0964ba58b62e055bd24a0b5a956269a780613d27159c7469b15888738e9',
  'call_name': '-deref',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IDeref',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IDeref::arity-1',
  'protocol_body_sha256': 'd3242dd4d16577be7c93e207212209f5f5b8b383f31ae7bf93fb274147438a34',
  'protocol_name': 'IDeref',
  'receiver': 'a',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': '49b36177303c37298dc202935b553eb937fa21113e94edc3316c5bcd6b665288',
  'call_name': '-kv-reduce',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IKVReduce::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IKVReduce::arity-1',
  'protocol_body_sha256': '49b36177303c37298dc202935b553eb937fa21113e94edc3316c5bcd6b665288',
  'protocol_name': 'IKVReduce',
  'receiver': '(sorted-map :b 2 :a 1)',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': '144522c6f89e48ea4089436be5db17c145256d8ad6d7b3810024726c64d5a9ae',
  'call_name': '-kv-reduce',
  'direct_arities': [3],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IKVReduce',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IKVReduce::arity-1',
  'protocol_body_sha256': '49b36177303c37298dc202935b553eb937fa21113e94edc3316c5bcd6b665288',
  'protocol_name': 'IKVReduce',
  'receiver': '(sorted-map :b 2 :a 1)',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': '88f0015b03c6b1abe54df3846a812e146076e59cb95105c6fe39bd1d2678ed6f',
  'call_name': '-compare',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IComparable::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IComparable::arity-1',
  'protocol_body_sha256': '88f0015b03c6b1abe54df3846a812e146076e59cb95105c6fe39bd1d2678ed6f',
  'protocol_name': 'IComparable',
  'receiver': '[1]',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': 'f366aa38c1d0a2bd2a22945410409c590c91fd902e36942fff60759820bef631',
  'call_name': '-compare',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IComparable',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IComparable::arity-1',
  'protocol_body_sha256': '88f0015b03c6b1abe54df3846a812e146076e59cb95105c6fe39bd1d2678ed6f',
  'protocol_name': 'IComparable',
  'receiver': '[1]',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-public'},
 {'body_sha256': '4d95f491cfb78df552974f49b5e9eb8c2ad2756816a0371844f9cc19ed794711',
  'call_name': '-realized?',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IPending::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IPending::arity-1',
  'protocol_body_sha256': '4d95f491cfb78df552974f49b5e9eb8c2ad2756816a0371844f9cc19ed794711',
  'protocol_name': 'IPending',
  'receiver': 'd',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-custom-state'},
 {'body_sha256': 'afa6c2c6cf7aabb8c1b18ecff2d4e58c68031229f7398efc9414ec135e20894c',
  'call_name': '-realized?',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IPending',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IPending::arity-1',
  'protocol_body_sha256': '4d95f491cfb78df552974f49b5e9eb8c2ad2756816a0371844f9cc19ed794711',
  'protocol_name': 'IPending',
  'receiver': 'd',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-custom-state'},
 {'body_sha256': '40dbb461cedbaf792e0e4c79b299c531daa02747df96a9b3da1f5d67dcaaeacb',
  'call_name': '-reset!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IReset::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IReset::arity-1',
  'protocol_body_sha256': '40dbb461cedbaf792e0e4c79b299c531daa02747df96a9b3da1f5d67dcaaeacb',
  'protocol_name': 'IReset',
  'receiver': 'r',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-custom-state'},
 {'body_sha256': '86a385631c65e1b3d3f2111c5f061427006207c31d18a1fde06b8c0f7576cf8b',
  'call_name': '-reset!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IReset',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IReset::arity-1',
  'protocol_body_sha256': '40dbb461cedbaf792e0e4c79b299c531daa02747df96a9b3da1f5d67dcaaeacb',
  'protocol_name': 'IReset',
  'receiver': 'r',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-custom-state'},
 {'body_sha256': 'c06bc375b34f2914b6aa3f6ccebef3b73d79af55a20e12df3ac2a941bdb71ed6',
  'call_name': '-swap!',
  'direct_arities': [2, 3, 4, 5],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ISwap::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISwap::arity-1',
  'protocol_body_sha256': 'c06bc375b34f2914b6aa3f6ccebef3b73d79af55a20e12df3ac2a941bdb71ed6',
  'protocol_name': 'ISwap',
  'receiver': 'r',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-custom-state'},
 {'body_sha256': 'a88409e460c97f3a856542621991682da203af2b88215fba87de6b8f918d49e3',
  'call_name': '-swap!',
  'direct_arities': [2, 3, 4, 5],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ISwap',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISwap::arity-1',
  'protocol_body_sha256': 'c06bc375b34f2914b6aa3f6ccebef3b73d79af55a20e12df3ac2a941bdb71ed6',
  'protocol_name': 'ISwap',
  'receiver': 'r',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-custom-state'},
 {'body_sha256': '8aab7f29fc0d21246ad8f6845a07b1b3ff4ed2854d7e319fea569b22aa8935fc',
  'call_name': '-vreset!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IVolatile::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IVolatile::arity-1',
  'protocol_body_sha256': '8aab7f29fc0d21246ad8f6845a07b1b3ff4ed2854d7e319fea569b22aa8935fc',
  'protocol_name': 'IVolatile',
  'receiver': 'v',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-protocol-custom-state'},
 {'body_sha256': '13a452fe446dec7fec44f7938ab10e8b7607af1fc84186577463c7470f667a38',
  'call_name': '-vreset!',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IVolatile',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IVolatile::arity-1',
  'protocol_body_sha256': '8aab7f29fc0d21246ad8f6845a07b1b3ff4ed2854d7e319fea569b22aa8935fc',
  'protocol_name': 'IVolatile',
  'receiver': 'v',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-protocol-custom-state'},
 {'body_sha256': 'bd5647df0b14eb824f8859d66ac75f3b8ba1a7b03742908a85cbb8df1ee89a1a',
  'call_name': '-clone',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ICloneable::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ICloneable::arity-1',
  'protocol_body_sha256': 'bd5647df0b14eb824f8859d66ac75f3b8ba1a7b03742908a85cbb8df1ee89a1a',
  'protocol_name': 'ICloneable',
  'receiver': 'custom',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-clone-find-protocol-workflow'},
 {'body_sha256': 'e12b2addddba6f50f20e6cf3ebfae46253d54c1507f5c0fedd2b06ec678f3027',
  'call_name': '-clone',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ICloneable',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ICloneable::arity-1',
  'protocol_body_sha256': 'bd5647df0b14eb824f8859d66ac75f3b8ba1a7b03742908a85cbb8df1ee89a1a',
  'protocol_name': 'ICloneable',
  'receiver': 'custom',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-clone-find-protocol-workflow'},
 {'body_sha256': '5a24af064e9a414a5631b1e67fef539621bdabe84b1fed48d9e3961a60531dc7',
  'call_name': '-chunked-first',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IChunkedSeq',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IChunkedSeq::arity-1',
  'protocol_body_sha256': '550ec3c46000e3a23e175c5f01fd50264d436737f47baf4eec2c43c8e8420ed2',
  'protocol_name': 'IChunkedSeq',
  'receiver': 's',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-chunk-public-factories'},
 {'body_sha256': '295fff12d9297b1ef55d35fe399cde0233428570cc10b522009438bd90728490',
  'call_name': '-chunked-next',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IChunkedNext',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IChunkedNext::arity-1',
  'protocol_body_sha256': '5fbc4536f7003e21ef83d0200c3dfb36a47d72c8aa2890795881d2cbe130c333',
  'protocol_name': 'IChunkedNext',
  'receiver': 's',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-chunk-public-factories'},
 {'body_sha256': '00f3cbba75cb4404e011b682631864dcd5d14f07f106fed682ead2f1123b2b29',
  'call_name': '-drop-first',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IChunk',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IChunk::arity-1',
  'protocol_body_sha256': '9fbb6fa1b28e4ac82306b82e0417e3369dcc1d070cdc97003560c62ba4928805',
  'protocol_name': 'IChunk',
  'receiver': 'c',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-chunk-public-pipeline'},
 {'body_sha256': 'bbdc3d32eda16461317de57b6cfc12767a345e3dca2e3dbe55e20b064d68fa31',
  'call_name': '-iterator',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IIterable::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IIterable::arity-1',
  'protocol_body_sha256': 'bbdc3d32eda16461317de57b6cfc12767a345e3dca2e3dbe55e20b064d68fa31',
  'protocol_name': 'IIterable',
  'receiver': 'coll',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-sequence-completion-src-test-cljs-cljs-iterator-test-coll-iter-seq-match'},
 {'body_sha256': '4733f3e0f0db8852f0223fa810b2fa4a42b06d08bf6ff55ec3e9f9d23a66b417',
  'call_name': '-iterator',
  'direct_arities': [1],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IIterable',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IIterable::arity-1',
  'protocol_body_sha256': 'bbdc3d32eda16461317de57b6cfc12767a345e3dca2e3dbe55e20b064d68fa31',
  'protocol_name': 'IIterable',
  'receiver': 'coll',
  'signature_sha256': None,
  'workflow': 'cljs-sequence-completion-src-test-cljs-cljs-iterator-test-coll-iter-seq-match'},
 {'body_sha256': '6273282bc799a6ab34f00202b63382500c561fd75733fdfefd26467c5865009f',
  'bound_value_source': 'true',
  'call_name': '*print-meta*',
  'direct_arities': [],
  'evidence_mode': 'printing-root',
  'expected_root_sensitive_result': '^{:x true} [x]',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::*print-meta*::arity-1',
  'printer_call': '(pr-str (with-meta ["x"] {:x true}))',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-after190-canonical-print-meta-unreadable'},
 {'body_sha256': '43e7c565485588d0b8d4d68afc1f301e84601f1f6efae78704a7f5351cb6354b',
  'bound_value_source': 'true',
  'call_name': '*print-meta*',
  'direct_arities': [],
  'evidence_mode': 'printing-root',
  'expected_root_sensitive_result': '^{:x true} [x]',
  'obligation_id': 'local::src/core_var_metadata.tsv::*print-meta*',
  'printer_call': '(pr-str (with-meta ["x"] {:x true}))',
  'signature_sha256': None,
  'workflow': 'arity-after190-canonical-print-meta-unreadable'},
 {'body_sha256': '564ca4ebe618bb3e8c380bb134fa5cd55b65306469dc0c9c4c9e22bfb2e50d51',
  'bound_value_source': 'false',
  'call_name': '*print-readably*',
  'direct_arities': [],
  'evidence_mode': 'printing-root',
  'expected_root_sensitive_result': '^{:x true} [x]',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::*print-readably*::arity-1',
  'printer_call': '(pr-str (with-meta ["x"] {:x true}))',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-after190-canonical-print-meta-unreadable'},
 {'body_sha256': 'e7adb9b5fc021d43b71ea43257c05945748dd2155998d29249aa4d6570a4c24f',
  'bound_value_source': 'false',
  'call_name': '*print-readably*',
  'direct_arities': [],
  'evidence_mode': 'printing-root',
  'expected_root_sensitive_result': '^{:x true} [x]',
  'obligation_id': 'local::src/core_var_metadata.tsv::*print-readably*',
  'printer_call': '(pr-str (with-meta ["x"] {:x true}))',
  'signature_sha256': None,
  'workflow': 'arity-after190-canonical-print-meta-unreadable'},
 {'body_sha256': 'b1e207d553f428b54033a078b2093aa2b18babcd27020f13607938b070b0c7d1',
  'call_name': '-sorted-seq',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::ISorted::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISorted::arity-1',
  'protocol_body_sha256': 'b1e207d553f428b54033a078b2093aa2b18babcd27020f13607938b070b0c7d1',
  'protocol_name': 'ISorted',
  'receiver': 's',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-receiver-sorted-full'},
 {'body_sha256': 'd1c4dc185f3adb5e39c033239fa899b6a00702ff0812366bc1f1fd7f01058831',
  'call_name': '-sorted-seq',
  'direct_arities': [2],
  'evidence_mode': 'protocol-method',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::ISorted',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::ISorted::arity-1',
  'protocol_body_sha256': 'b1e207d553f428b54033a078b2093aa2b18babcd27020f13607938b070b0c7d1',
  'protocol_name': 'ISorted',
  'receiver': 's',
  'signature_sha256': None,
  'workflow': 'arity-receiver-sorted-full'},
 {'body_sha256': '0e200947029ff81edafacb508d8c170c933e90678ae7dd1f95b5f8299972ff8c',
  'call_name': 'pr-str',
  'direct_arities': [],
  'evidence_mode': 'protocol-printer-receiver',
  'marker': False,
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IPrintWithWriter::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IPrintWithWriter::arity-1',
  'protocol_body_sha256': '0e200947029ff81edafacb508d8c170c933e90678ae7dd1f95b5f8299972ff8c',
  'protocol_name': 'IPrintWithWriter',
  'receiver': None,
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-extension-test-cljs-2812'},
 {'body_sha256': '856d8c4793a5f8cc67fb1749f0c01bb9b58fe28f9c8d258d87f9600d613b9a4d',
  'call_name': 'pr-str',
  'direct_arities': [],
  'evidence_mode': 'protocol-printer-receiver',
  'marker': False,
  'obligation_id': 'local::src/core_var_metadata.tsv::IPrintWithWriter',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IPrintWithWriter::arity-1',
  'protocol_body_sha256': '0e200947029ff81edafacb508d8c170c933e90678ae7dd1f95b5f8299972ff8c',
  'protocol_name': 'IPrintWithWriter',
  'receiver': None,
  'signature_sha256': None,
  'workflow': 'cljs-extension-test-cljs-2812'},
 {'body_sha256': '129155e71344259109a0af4de48172ebb927e3074241b5c73462fd36b5bd5c08',
  'call_name': '-add-watch',
  'consumer_calls': [{'arity': 3, 'name': 'add-watch', 'source': '(add-watch w :foo (fn []))'},
                     {'arity': 2, 'name': 'remove-watch', 'source': '(remove-watch w :foo)'}],
  'demanded_methods': [{'arities': [3], 'name': '-add-watch'},
                       {'arities': [2], 'name': '-remove-watch'}],
  'direct_arities': [],
  'evidence_mode': 'protocol-implemented-consumer',
  'implementation_source': '(deftype MyWatchable []\n'
                           '  IWatchable\n'
                           '  (-notify-watches [this oldval newval])\n'
                           '  (-add-watch [this key f])\n'
                           '  (-remove-watch [this key]))',
  'literal_assertions': ['(assert (identical? (add-watch w :foo (fn [])) w))',
                         '(assert (identical? (remove-watch w :foo) w))'],
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IWatchable::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IWatchable::arity-1',
  'protocol_body_sha256': '129155e71344259109a0af4de48172ebb927e3074241b5c73462fd36b5bd5c08',
  'protocol_name': 'IWatchable',
  'receiver_setup_source': 'w (MyWatchable.)',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-deftype-core'},
 {'body_sha256': 'eec7cb0fa37bf36b311a4d7464b12633190ce9ee0bc7f3d6e0d324f4dc0edc6c',
  'call_name': '-add-watch',
  'consumer_calls': [{'arity': 3, 'name': 'add-watch', 'source': '(add-watch w :foo (fn []))'},
                     {'arity': 2, 'name': 'remove-watch', 'source': '(remove-watch w :foo)'}],
  'demanded_methods': [{'arities': [3], 'name': '-add-watch'},
                       {'arities': [2], 'name': '-remove-watch'}],
  'direct_arities': [],
  'evidence_mode': 'protocol-implemented-consumer',
  'implementation_source': '(deftype MyWatchable []\n'
                           '  IWatchable\n'
                           '  (-notify-watches [this oldval newval])\n'
                           '  (-add-watch [this key f])\n'
                           '  (-remove-watch [this key]))',
  'literal_assertions': ['(assert (identical? (add-watch w :foo (fn [])) w))',
                         '(assert (identical? (remove-watch w :foo) w))'],
  'obligation_id': 'local::src/core_var_metadata.tsv::IWatchable',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IWatchable::arity-1',
  'protocol_body_sha256': '129155e71344259109a0af4de48172ebb927e3074241b5c73462fd36b5bd5c08',
  'protocol_name': 'IWatchable',
  'receiver_setup_source': 'w (MyWatchable.)',
  'signature_sha256': None,
  'workflow': 'cljs-deftype-core'},
 {'body_sha256': '664834c1d9d8161561589df9218caa2575d14ec0e826eeaedac3ed2caa58ff54',
  'call_name': '-js->clj',
  'consumer_calls': [{'arity': 3,
                      'name': 'js->clj',
                      'source': '(js->clj custom :keywordize-keys true)'}],
  'demanded_methods': [{'arities': [2], 'name': '-js->clj'}],
  'direct_arities': [],
  'evidence_mode': 'protocol-implemented-consumer',
  'implementation_source': '(reify IEncodeClojure (-js->clj [_ opts] (:keywordize-keys opts)))',
  'literal_assertions': [],
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IEncodeClojure::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IEncodeClojure::arity-1',
  'protocol_body_sha256': '664834c1d9d8161561589df9218caa2575d14ec0e826eeaedac3ed2caa58ff54',
  'protocol_name': 'IEncodeClojure',
  'receiver_setup_source': 'custom (reify IEncodeClojure (-js->clj [_ opts] (:keywordize-keys '
                           'opts)))',
  'root_equality_literal_source': '"[\\"[{:a [1 '
                                  'nil]}]\\",true,true,true,7,\\"{\\\\\\":n/key\\\\\\":[\\\\\\":n/value\\\\\\",\\\\\\"n/symbol\\\\\\"]}\\"]"',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-extended-language-helper-js-conversions'},
 {'body_sha256': '2d819a6c6efdf8145f246832417affacd2b1702ba492c5452dd9b3fcf72edff7',
  'call_name': '-js->clj',
  'consumer_calls': [{'arity': 3,
                      'name': 'js->clj',
                      'source': '(js->clj custom :keywordize-keys true)'}],
  'demanded_methods': [{'arities': [2], 'name': '-js->clj'}],
  'direct_arities': [],
  'evidence_mode': 'protocol-implemented-consumer',
  'implementation_source': '(reify IEncodeClojure (-js->clj [_ opts] (:keywordize-keys opts)))',
  'literal_assertions': [],
  'obligation_id': 'local::src/core_var_metadata.tsv::IEncodeClojure',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IEncodeClojure::arity-1',
  'protocol_body_sha256': '664834c1d9d8161561589df9218caa2575d14ec0e826eeaedac3ed2caa58ff54',
  'protocol_name': 'IEncodeClojure',
  'receiver_setup_source': 'custom (reify IEncodeClojure (-js->clj [_ opts] (:keywordize-keys '
                           'opts)))',
  'root_equality_literal_source': '"[\\"[{:a [1 '
                                  'nil]}]\\",true,true,true,7,\\"{\\\\\\":n/key\\\\\\":[\\\\\\":n/value\\\\\\",\\\\\\"n/symbol\\\\\\"]}\\"]"',
  'signature_sha256': None,
  'workflow': 'arity-extended-language-helper-js-conversions'},
 {'body_sha256': 'd712c41d32f9bcac89351c97f304145233cd83d1aff8c93d1de6bddb250d3a78',
  'call_name': '-clj->js',
  'consumer_calls': [{'arity': 1, 'name': 'clj->js', 'source': '(clj->js out)'}],
  'demanded_methods': [{'arities': [1], 'name': '-clj->js'}],
  'direct_arities': [],
  'evidence_mode': 'protocol-implemented-consumer',
  'implementation_source': '(reify IEncodeJS (-clj->js [_] #js {:x 7}) (-key->js [_] "unused"))',
  'literal_assertions': [],
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IEncodeJS::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IEncodeJS::arity-1',
  'protocol_body_sha256': 'd712c41d32f9bcac89351c97f304145233cd83d1aff8c93d1de6bddb250d3a78',
  'protocol_name': 'IEncodeJS',
  'receiver_setup_source': 'out (reify IEncodeJS (-clj->js [_] #js {:x 7}) (-key->js [_] '
                           '"unused"))',
  'root_equality_literal_source': '"[\\"[{:a [1 '
                                  'nil]}]\\",true,true,true,7,\\"{\\\\\\":n/key\\\\\\":[\\\\\\":n/value\\\\\\",\\\\\\"n/symbol\\\\\\"]}\\"]"',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-extended-language-helper-js-conversions'},
 {'body_sha256': 'a6e9e00ce5cd263f2b5e84173bd2c518f9823e16eb4e7fb0e21631fe74561fa4',
  'call_name': '-clj->js',
  'consumer_calls': [{'arity': 1, 'name': 'clj->js', 'source': '(clj->js out)'}],
  'demanded_methods': [{'arities': [1], 'name': '-clj->js'}],
  'direct_arities': [],
  'evidence_mode': 'protocol-implemented-consumer',
  'implementation_source': '(reify IEncodeJS (-clj->js [_] #js {:x 7}) (-key->js [_] "unused"))',
  'literal_assertions': [],
  'obligation_id': 'local::src/core_var_metadata.tsv::IEncodeJS',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IEncodeJS::arity-1',
  'protocol_body_sha256': 'd712c41d32f9bcac89351c97f304145233cd83d1aff8c93d1de6bddb250d3a78',
  'protocol_name': 'IEncodeJS',
  'receiver_setup_source': 'out (reify IEncodeJS (-clj->js [_] #js {:x 7}) (-key->js [_] '
                           '"unused"))',
  'root_equality_literal_source': '"[\\"[{:a [1 '
                                  'nil]}]\\",true,true,true,7,\\"{\\\\\\":n/key\\\\\\":[\\\\\\":n/value\\\\\\",\\\\\\"n/symbol\\\\\\"]}\\"]"',
  'signature_sha256': None,
  'workflow': 'arity-extended-language-helper-js-conversions'},
 {'body_sha256': '65ef383d72e2e4c3442d857d9f04486b7b29281da147bb539019c74bd9276ce1',
  'call_name': '-hash',
  'consumer_calls': [{'arity': 2, 'name': 'get', 'source': '(get m fixed-hash-foo)'},
                     {'arity': 2, 'name': 'get', 'source': '(get m fixed-hash-bar)'},
                     {'arity': 2, 'name': 'dissoc', 'source': '(dissoc m fixed-hash-foo)'}],
  'demanded_methods': [{'arities': [1], 'name': '-hash'}],
  'direct_arities': [],
  'evidence_mode': 'protocol-implemented-consumer',
  'implementation_source': '(deftype FixedHash [h v] IHash (-hash [this] h) IEquiv (-equiv [this '
                           'other] (and (instance? FixedHash other) (= v (.-v other)))))',
  'literal_assertions': ['(assert (= (get m fixed-hash-foo) 1))',
                         '(assert (= (get m fixed-hash-bar) 2))',
                         '(assert (= (count m) 2))',
                         '(assert (not (contains? m fixed-hash-foo)))',
                         '(assert (= (count m) 1))',
                         '(assert (= (count m) 102))',
                         '(assert (= (count m) 98))'],
  'obligation_id': 'local::src/core_var_metadata.tsv::IHash',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IHash::arity-1',
  'protocol_body_sha256': 'd399756d9c6dcc723dd4b062a85ae127f09f0f4a9e2f14a304f73475e82c4829',
  'protocol_name': 'IHash',
  'receiver_setup_source': '(def fixed-hash-foo (FixedHash. 0 :foo)) (def fixed-hash-bar '
                           '(FixedHash. 0 :bar))',
  'signature_sha256': None,
  'workflow': 'cljs-extra-collections-test-phm-fixed-hash'},
 {'body_sha256': '3042b2f3709feb84eff6b243b8ff67238f6fed190589428686c036161670b811',
  'call_name': '-drop',
  'consumer_calls': [{'arity': 2, 'name': 'drop', 'source': '(drop 5 (seq "aaaaaaaaaa"))'},
                     {'arity': 2,
                      'name': 'drop',
                      'source': '(drop 3 (drop 5 (seq "aaaaaaaaaa")))'}],
  'demanded_methods': [{'arities': [2], 'name': '-drop'}],
  'direct_arities': [],
  'evidence_mode': 'protocol-typed-consumer',
  'literal_assertions': ['(assert (= [\\a \\a \\a \\a \\a] (drop 5 (seq "aaaaaaaaaa"))))',
                         '(assert (satisfies? IDrop (drop 5 (seq "aaaaaaaaaa"))))',
                         '(assert (= [\\a \\a] (drop 3 (drop 5 (seq "aaaaaaaaaa")))))'],
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::IDrop::arity-1',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IDrop::arity-1',
  'protocol_body_sha256': '3042b2f3709feb84eff6b243b8ff67238f6fed190589428686c036161670b811',
  'protocol_name': 'IDrop',
  'receiver_tests': ['(satisfies? IDrop (seq "aaaaaaaaaa"))',
                     '(satisfies? IDrop (drop 5 (seq "aaaaaaaaaa")))'],
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-sequences-collections-test-test-string'},
 {'body_sha256': '6994772158dcc8c4d9b79656a9606367ec7d9224ffbfecafe067a2d3575998f0',
  'call_name': '-drop',
  'consumer_calls': [{'arity': 2, 'name': 'drop', 'source': '(drop 5 (seq "aaaaaaaaaa"))'},
                     {'arity': 2,
                      'name': 'drop',
                      'source': '(drop 3 (drop 5 (seq "aaaaaaaaaa")))'}],
  'demanded_methods': [{'arities': [2], 'name': '-drop'}],
  'direct_arities': [],
  'evidence_mode': 'protocol-typed-consumer',
  'literal_assertions': ['(assert (= [\\a \\a \\a \\a \\a] (drop 5 (seq "aaaaaaaaaa"))))',
                         '(assert (satisfies? IDrop (drop 5 (seq "aaaaaaaaaa"))))',
                         '(assert (= [\\a \\a] (drop 3 (drop 5 (seq "aaaaaaaaaa")))))'],
  'obligation_id': 'local::src/core_var_metadata.tsv::IDrop',
  'protocol_authority': 'api::src/main/cljs/cljs/core.cljs::IDrop::arity-1',
  'protocol_body_sha256': '3042b2f3709feb84eff6b243b8ff67238f6fed190589428686c036161670b811',
  'protocol_name': 'IDrop',
  'receiver_tests': ['(satisfies? IDrop (seq "aaaaaaaaaa"))',
                     '(satisfies? IDrop (drop 5 (seq "aaaaaaaaaa")))'],
  'signature_sha256': None,
  'workflow': 'cljs-sequences-collections-test-test-string'},
 {'body_sha256': '3666d79af0f5ef3db38e0353402847b68cddc75e530dc9dfde18846a53b23be0',
  'call_name': 'require',
  'consumer_calls': [{'arity': 2,
                      'name': 'set/intersection',
                      'source': '(set/intersection #{:a :b} #{:b :c})'}],
  'direct_arities': [1],
  'evidence_mode': 'namespace-setup-consumer',
  'literal_assertions': ['(assert (identical? set/intersection clojure.set/intersection))',
                         '(assert (= #{:b} (set/intersection #{:a :b} #{:b :c})))'],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::require::arity-1',
  'setup_source': "(require '[clojure.set :as set])",
  'signature_sha256': '5e4b6daa4eafb9004cb9876642b7cd0bb3940ad718f60904550ef6857f6527ba',
  'workflow': 'cljs-namespace-completion-require-alias'},
 {'body_sha256': '7d2ab80c06ad5f76ade52d17e08cccf6aba7bbf205008bb2e699941665625979',
  'call_name': 'use',
  'consumer_calls': [{'arity': 2,
                      'name': 'intersection',
                      'source': '(intersection #{:a :b} #{:b :c})'}],
  'direct_arities': [1],
  'evidence_mode': 'namespace-setup-consumer',
  'literal_assertions': ['(assert (identical? intersection clojure.set/intersection))',
                         '(assert (= #{:b} (intersection #{:a :b} #{:b :c})))'],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::use::arity-1',
  'setup_source': "(use '[clojure.set :only [intersection]])",
  'signature_sha256': '5e4b6daa4eafb9004cb9876642b7cd0bb3940ad718f60904550ef6857f6527ba',
  'workflow': 'cljs-namespace-completion-use'},
 {'body_sha256': '5553576fa2c58fbf01b13a4ae8a41e1c5f44d295cadbc73a56206f2f3a6ae0a9',
  'call_name': 'refer-clojure',
  'consumer_calls': [{'arity': 0, 'name': 'map', 'source': '(map)'},
                     {'arity': 0, 'name': 'mapv', 'source': '(mapv)'}],
  'direct_arities': [2],
  'evidence_mode': 'namespace-setup-consumer',
  'literal_assertions': ['(assert (= [:own-map :own-mapv] [(map) (mapv)]))'],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::refer-clojure::arity-1',
  'setup_source': "(refer-clojure :exclude '[map mapv])",
  'signature_sha256': '5e4b6daa4eafb9004cb9876642b7cd0bb3940ad718f60904550ef6857f6527ba',
  'workflow': 'cljs-namespace-completion-refer-core-exclude'},
 {'body_sha256': '33508f622a0ef569cd03b92df07679338741d2e456d726497c26de28540ff472',
  'call_name': 'refer-global',
  'consumer_calls': [{'arity': 1, 'name': 'Date.', 'source': '(Date. 0)'}],
  'direct_arities': [2],
  'evidence_mode': 'namespace-setup-consumer',
  'literal_assertions': ['(assert (identical? Date js/Date))',
                         '(assert (= 0 (.getTime (Date. 0))))'],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::refer-global::arity-1',
  'setup_source': "(refer-global :only '[Date])",
  'signature_sha256': '5e4b6daa4eafb9004cb9876642b7cd0bb3940ad718f60904550ef6857f6527ba',
  'workflow': 'cljs-namespace-completion-refer-global'}]
DEMAND = {
'cljs-namespace-completion-refer-global': 'The complete original namespace retains top-level refer-global for Date. Its original run observes actual Date/js-Date identity, constructs Date at zero and consumes getTime against independent zero. Each compilation demands that setup, and macro computation consumes the resulting constructor; no metadata-only or runtime refer-global callback credit is claimed.',
'cljs-namespace-completion-refer-core-exclude': 'The complete original namespace retains top-level refer-clojure exclusion and both original local function definitions. The original run invokes both excluded-name functions against independent [:own-map :own-mapv]. Each compilation demands the setup; macro computation consumes its resulting functions rather than claiming an evaluated refer-clojure callback.',
'cljs-namespace-completion-use': 'The complete original namespace and companion clojure.set source remain unchanged. Each actual compilation executes the original top-level use setup; the original run invokes the actual referred intersection against independent #{:b} and function identity. Macro computation evaluates that original run using the setup values; top-level use is separately scoped as compile-time setup.',
'cljs-namespace-completion-require-alias': 'The complete original namespace and companion clojure.set source remain unchanged. Each actual compilation executes the original top-level require setup; the original run consumes set/intersection against the independent #{:b} literal and actual function identity. Macro computation evaluates that original run using the setup values; it does not pretend that top-level require is a runtime macro callback.',
'cljs-sequences-collections-test-test-string': 'The complete original string body explicitly asserts IDrop on the original string-sequence receiver and consumes that same sequence through drop 5. Independent five-character and nested two-character vector literals, positive IDrop on the returned sequence and the original negative raw-string result test remain unchanged. Declaration links use the original drop consumer; no direct -drop call is invented.',
'cljs-extra-collections-test-phm-fixed-hash': 'The unchanged complete original FixedHash collision body constructs both named zero-hash receivers and uses them in actual PersistentHashMap association, get and removal. Independent values 1/2, distinct-key counts 2/102, removed-key absence and counts 1/98 demand the implemented IHash callback together with all original equality and collision branches.',
'arity-extended-language-helper-js-conversions': 'The unchanged complete conversion contract constructs the original custom IEncodeClojure and IEncodeJS receivers. Actual js->clj passes keywordize-keys=true to the implemented callback and actual clj->js consumes the original object-producing callback. The entire independent JSON literal observes true and 7 along with unchanged metadata, opaque identity and key conversion controls. The undemanded key->js callback supplies no credit.',
'cljs-deftype-core': 'The complete original namespace invokes its unchanged run, including test-920-watch-ops-return-ref. Its MyWatchable receiver implements IWatchable; original add-watch and remove-watch calls dispatch to the declared callbacks and both independent identity assertions observe the same original receiver. The undemanded notify callback supplies no credit. All other original type, atom, map and metadata tests remain.',
'cljs-helpers-map-entry-test-map-entry-interface-tests': 'Every original MapEntry interface body executes the same constructed receiver against independent key/value, lookup, stack, collection, sequence, reverse, count, index, association, vector, reduction, find and invoke literals and error controls. Exact protocol method calls consume this receiver; ISequential marker membership is followed by actual sequence dispatch and independent ordered list assertion.',
'arity-receiver-transient': 'The original complete transient body preserves all vector/map/set mutation, persistent conversion, saved/applied calls, original persistence errors and metadata/hash controls. Its custom reify receiver implements the actual named transient protocols, each corresponding method is directly invoked and the complete independent ordered event vector is asserted.',
'cljs-core-gate-literal-protocol-public': 'The entire existing independent literal protocol vector directly dispatches count/index/sequence/reverse/lookup/association/map/set/empty/stack/reduce/kv-reduce/entry/compare/deref/metadata methods and observes every result, preserving all constructed receivers and both method arities where declared.',
'cljs-core-gate-literal-protocol-custom-state': 'The complete existing custom state literal contract directly invokes all four declared ISwap arities, reset, deref, volatile reset and unrealized/realized delay paths. Independent numeric results and the entire ordered swap/reset event vector remain.',
'cljs-core-gate-literal-clone-find-protocol-workflow': 'The complete existing clone/find workflow directly invokes the original and custom clone receivers, observes distinct identity/type/metadata and invokes the custom find receiver at present and absent keys. Its entire independent literal and ordered clone/find events remain.',
'cljs-core-gate-literal-chunk-public-factories': 'The complete existing chunk factory workflow consumes the original constructed ChunkedCons receiver through vec, metadata and direct chunked-first/rest/next calls. Every independent literal, saved constructor and terminal nil branch remains.',
'cljs-core-gate-literal-chunk-public-pipeline': 'The complete existing chunk pipeline appends all original effectful items, finishes the buffer, directly drops from the same constructed chunk and observes independent count/nth literals. Saved chunk methods, complete traversals, metadata, empty controls and original located error text remain.',
'cljs-sequence-completion-src-test-cljs-cljs-iterator-test-coll-iter-seq-match': 'The complete original seq-iter-match helper is invoked for all original map/set/queue/record receivers. Its actual -iterator coll call and entire hasNext/next/seq comparison loop preserve each demanded item and both original exhaustion/error paths; all original independent true assertions and full 200/100-element inputs remain.',
'arity-after190-canonical-print-meta-unreadable': 'The unchanged complete already accepted SCI printing-context expression binds print-meta=true and print-readably=false, constructs the original metadata-bearing string vector and invokes the original printer. The exact canonical literal ^{:x true} [x] observes both metadata prefix and unquoted string under those roots. The original SCI expected result remains separately retained and receives zero original SCI pass credit; no binding, body or canonical expected string was invented.',
'arity-receiver-sorted-full': 'The complete original sorted-map/set body retains named ISorted receiver assertions and actual comparator, sorted-seq, sorted-seq-from and entry-key dispatch. Original equality/hash/metadata, competing-key identity, reduction and early completion, subseq/rsubseq loops, public constructors and all original error paths remain unchanged.',
'cljs-extension-test-cljs-2812': 'The complete original extension body implements IPrintWithWriter on object/boolean/number/string/array/function receivers. Each of the first six original printer callbacks is consumed by its exact original pr-str assertion against an independent distinct literal; all writer forwarding and original cleanup extensions remain. Definitions alone receive no credit and no direct three-argument -pr-writer call is invented.',
'cljs-definitions-var-test': 'The original complete run invokes cljs-3411, which calls test on both the function and its Var. Each original metadata callback must execute the original additions and literal assertions 5 and 8 before :ok is asserted twice.',
'cljs-extra-core-test-req-': 'The complete original req! body checks the absent-key exception and the exact present keyword, nil-key and nil-value results against independent literals.',
'cljs-public-gate-core-factory-multifn': 'The original complete cohort constructs five MultiFn receivers; every receiver is called and its retrieved method is invoked with nil against literal 41. The actual IMultiFn receiver check and dispatch-fn identity assertion follow demanded method execution and preserve original fields and default value.'}
EXPECTED_LITERAL_RAW = {row['id']: 'true' for row in REVIEWED}
EXPECTED_LITERAL_RAW[PRINT_ID] = '"^{:x true} [x]"'
CONTEXT_SUPPORT = sorted({path for row in REVIEWED for companion in row.get('companion_sources', {}).values() for path in [companion['fixture'], companion['original_fixture']]})

def demand_basis(row):
    return DEMAND[row['id']]


def rows():
    if digest((ROOT / SCANNER).read_bytes()) != SCANNER_SHA256:
        raise ValueError('reviewed structural scanner drift')
    import report
    catalog = {**report.fixtures('intended-language'), **supplemental_reference_cases()}
    driver = (ROOT / DRIVER).read_text()
    if not re.search(r'#\[test\]\s*fn\s+' + TEST + r'\s*\(', driver):
        raise ValueError('detached actual phase driver')
    for required in ['compile_namespace_with_macros', 'MacroHost::new()', 'defmacro arity-witness [] {invoke}', 'pr-str {invoke}', 'defmacro arity-witness [] (do {source})', 'fresh', 'reused', 'arity-after190-artifacts', 'from_secs(120)']:
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
        if 'companion_sources' in reviewed:
            entry = reviewed['namespace'].replace('.', '/').replace('-', '_') + '.cljs'
            original_root = ROOT / case['source_root']
            original_companions = {str(path.relative_to(original_root)): digest(path.read_bytes()) for path in original_root.rglob('*') if path.is_file() and str(path.relative_to(original_root)) != entry}
            if original_companions != {relative: companion['source_sha256'] for relative, companion in reviewed['companion_sources'].items()}:
                raise ValueError('complete original namespace companion tree drift')
            for relative, companion in reviewed['companion_sources'].items():
                if (ROOT / companion['fixture']).read_bytes() != (ROOT / companion['original_fixture']).read_bytes() or digest((ROOT / companion['fixture']).read_bytes()) != companion['source_sha256'] or str(Path(companion['fixture']).relative_to('tests')) not in driver:
                    raise ValueError('unchanged original namespace companion omitted')
            if 'for &(relative, companion) in companions' not in driver or 'arity-after190-companion-artifacts' not in driver:
                raise ValueError('actual namespace companion capture omitted')
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
    if mode == 'namespace-setup-consumer':
        roots = Reader(source).all()
        setup = [node for node in roots if head(node) == name and source[node.start:node.end] == selected['setup_source']]
        signature = target.get('signature')
        if len(setup) != 1 or not signature or signature['kind'] != 'variadic' or signature['minimum'] != 0 or len(setup[0].children)-1 not in selected['direct_arities']:
            raise ValueError('exact top-level namespace setup/signature omitted')
        calls = []
        for consumer in selected['consumer_calls']:
            matched = [call for call in direct_calls(source, consumer['name']) if call['call_source'] == consumer['source'] and call['argument_count'] == consumer['arity']]
            if not matched:
                raise ValueError('actual original namespace setup consumer omitted')
            calls.extend(matched)
        asserted = {call['call_source'] for call in direct_calls(source, 'assert')}
        if not set(selected['literal_assertions']).issubset(asserted):
            raise ValueError('independent original namespace setup literals omitted')
        return [{'setup_source': selected['setup_source'], 'setup_source_sha256': digest(selected['setup_source']),
            'setup_argument_count': len(setup[0].children)-1, 'demanding_original_consumer_calls': calls,
            'independent_original_assertions': selected['literal_assertions'],
            'operation_scope': 'top-level compile-time namespace setup executed in each actual compilation',
            'evaluated_macro_computation_scope': 'the unchanged original run consumes setup values; setup is not claimed as an evaluated runtime macro callback',
            'source_calls_are_original_setup_and_consumers': True}]
    if mode in {'protocol-implemented-consumer', 'protocol-typed-consumer'}:
        obligations = {row['id']: row for row in map(json.loads, (HERE / 'obligations.jsonl').read_text().splitlines())}
        authority = obligations[selected['protocol_authority']]
        if authority['name'] != target['name'] or authority['name'] != selected['protocol_name'] or authority['body_sha256'] != selected['protocol_body_sha256'] or authority['declaration'] != 'defprotocol':
            raise ValueError('consumer protocol authority drift')
        signatures = {}
        for form in Reader(authority['source_span']['text']).all():
            if head(form) != 'defprotocol':
                raise ValueError('consumer protocol declaration shape drift')
            for method in form.children[2:]:
                if method.kind != 'list' or not method.children:
                    continue
                named = method.children[0]
                while named.kind == 'metadata':
                    named = named.children[-1]
                signatures[named.token] = [len(vector.children) for vector in method.children[1:] if vector.kind == 'vector']
        for method in selected['demanded_methods']:
            if signatures.get(method['name']) != method['arities']:
                raise ValueError('consumer demanded method signature drift')
        implementation = None
        if mode == 'protocol-implemented-consumer':
            matches = []
            def visit(node):
                if node.kind in {'quote', 'syntax-quote', 'discard', 'var'}:
                    return
                if head(node) in {'reify', 'deftype'} and source[node.start:node.end] == selected['implementation_source']:
                    matches.append(node)
                for child in node.children:
                    visit(child)
            for node in Reader(source).all():
                visit(node)
            if len(matches) != 1 or selected['receiver_setup_source'] not in source:
                raise ValueError('actual implemented protocol receiver setup omitted')
            implementation = matches[0]
            if not any(child.token == authority['name'] for child in implementation.children):
                raise ValueError('actual implemented protocol name omitted')
            implemented = {head(child): child for child in implementation.children if child.kind == 'list'}
            for method in selected['demanded_methods']:
                callback = implemented.get(method['name'])
                if callback is None or len(callback.children) < 2 or callback.children[1].kind != 'vector' or [len(callback.children[1].children)] != method['arities']:
                    raise ValueError('actual implemented callback signature drift')
        else:
            receiver_tests = {call['call_source'] for call in direct_calls(source, 'satisfies?')}
            if not set(selected['receiver_tests']).issubset(receiver_tests):
                raise ValueError('actual named protocol receiver tests omitted')
        calls = []
        for consumer in selected['consumer_calls']:
            matched = [call for call in direct_calls(source, consumer['name']) if call['call_source'] == consumer['source'] and call['argument_count'] == consumer['arity']]
            if not matched:
                raise ValueError('actual original protocol consumer omitted')
            calls.extend(matched)
        asserted = {call['call_source'] for call in direct_calls(source, 'assert')}
        if not set(selected['literal_assertions']).issubset(asserted):
            raise ValueError('independent original protocol consumer assertions omitted')
        equality_literal = selected.get('root_equality_literal_source')
        if equality_literal is not None:
            roots = Reader(source).all()
            if len(roots) != 1 or head(roots[0]) != '=' or source[roots[0].children[1].start:roots[0].children[1].end] != equality_literal:
                raise ValueError('independent original protocol consumer equality literal omitted')
        return [{'protocol_name': authority['name'], 'protocol_body_sha256': authority['body_sha256'],
            'protocol_declaration_source': authority['source_span']['text'],
            'demanded_declared_methods': selected['demanded_methods'],
            'implemented_callback_source': selected.get('implementation_source'),
            'implemented_callback_source_sha256': digest(selected['implementation_source']) if implementation else None,
            'original_receiver_setup_source': selected.get('receiver_setup_source'),
            'original_named_receiver_tests': selected.get('receiver_tests'),
            'demanding_original_consumer_calls': calls, 'independent_original_assertions': selected['literal_assertions'],
            'independent_root_equality_literal_source': equality_literal,
            'source_calls_are_consumers_not_invented_direct_method_calls': True}]
    if mode == 'protocol-printer-receiver':
        obligations = {row['id']: row for row in map(json.loads, (HERE / 'obligations.jsonl').read_text().splitlines())}
        authority = obligations[selected['protocol_authority']]
        if authority['name'] != target['name'] or authority['name'] != 'IPrintWithWriter' or authority['body_sha256'] != selected['protocol_body_sha256'] or '(-pr-writer [o writer opts])' not in authority['source_span']['text']:
            raise ValueError('printer protocol authority drift')
        extensions = []
        def visit(node):
            if node.kind in {'quote', 'syntax-quote', 'discard', 'var'}:
                return
            if head(node) == 'extend-protocol' and len(node.children) > 2 and node.children[1].token == 'IPrintWithWriter':
                extensions.append(node)
            for child in node.children:
                visit(child)
        for node in Reader(source).all():
            visit(node)
        if len(extensions) != 2:
            raise ValueError('original printer setup/cleanup extensions omitted')
        first = extensions[0]
        expected = [
            ('object', '(pr-str #js {})', '#object[custom-print-cljs-2812]'),
            ('boolean', '(pr-str true)', '#boolean[true]'),
            ('number', '(pr-str 11)', '#number[11]'),
            ('string', '(pr-str "hello")', '#string[hello]'),
            ('array', '(pr-str #js [1 2 3])', '#array[3]'),
            ('function', '(pr-str map)', '#function[custom-print-cljs-2812]')]
        pairs = list(zip(first.children[2::2], first.children[3::2]))
        if len(pairs) != len(expected):
            raise ValueError('whole original printer receiver cohort omitted')
        result = []
        all_calls = direct_calls(source, 'pr-str')
        for (receiver, callback), (type_name, consumer, literal) in zip(pairs, expected):
            if receiver.token != type_name or head(callback) != '-pr-writer' or source[callback.children[1].start:callback.children[1].end] != '[obj writer _]':
                raise ValueError('original implemented printer callback signature drift')
            body = source[callback.start:callback.end]
            if not direct_calls(body, 'write-all'):
                raise ValueError('implemented printer writer forwarding omitted')
            assertion = '(assert (= '+json.dumps(literal)+' '+consumer+'))'
            calls = [call for call in all_calls if call['call_source'] == consumer]
            if len(calls) != 1 or assertion not in source:
                raise ValueError('actual original printer callback demand/literal omitted')
            result.append({'protocol_name': authority['name'], 'protocol_body_sha256': authority['body_sha256'],
                'receiver_type': type_name, 'implemented_callback_source': body,
                'implemented_callback_source_sha256': digest(body), 'callback_parameter_count': 3,
                'demanding_original_printer_call': calls[0], 'independent_literal_assertion': assertion,
                'independent_literal_assertion_sha256': digest(assertion),
                'source_call_is_consumer_not_invented_direct_method_call': True})
        return result
    if mode == 'printing-root':
        result = []
        def visit(node):
            if node.kind in {'quote', 'syntax-quote', 'discard', 'var'}:
                return
            if head(node) == 'binding' and len(node.children) > 2:
                bound = node.children[1]
                if bound.kind == 'vector':
                    for index in range(0, len(bound.children)-1, 2):
                        if bound.children[index].token == target['name']:
                            value = source[bound.children[index+1].start:bound.children[index+1].end]
                            body = source[node.start:node.end]
                            calls = [call for call in direct_calls(body, 'pr-str') if call['call_source'] == selected['printer_call']]
                            if value != selected['bound_value_source'] or not calls:
                                raise ValueError('absent exact root value or demanded original printer')
                            result.append({'control_name': target['name'], 'bound_value_source': value,
                                'binding_source': body, 'binding_source_sha256': digest(body),
                                'printer_calls': calls, 'canonical_literal_result': selected['expected_root_sensitive_result'],
                                'whole_original_source_sha256': digest(source),
                                'original_sci_expected_retained': supplemental_reference_cases()[PRINT_ID]['canonical_printing_provenance']['original_expected'],
                                'original_sci_semantic_pass_credit': 0})
            for child in node.children:
                visit(child)
        for node in Reader(source).all():
            visit(node)
        if len(result) != 1 or selected['expected_root_sensitive_result'] != '^{:x true} [x]' or target['name'] not in {'*print-meta*', '*print-readably*'}:
            raise ValueError('absent exact original demanded printing-root contract')
        return result
    if mode == 'protocol-marker-receiver':
        obligations = {row['id']: row for row in map(json.loads, (HERE / 'obligations.jsonl').read_text().splitlines())}
        authority = obligations[selected['protocol_authority']]
        if authority['name'] != target['name'] or authority['body_sha256'] != selected['protocol_body_sha256'] or authority['name'] != 'ISequential' or authority['declaration'] != 'defprotocol':
            raise ValueError('marker protocol authority drift')
        declaration = Reader(authority['source_span']['text']).all()
        if len(declaration) != 1 or any(node.kind == 'list' for node in declaration[0].children[2:]):
            raise ValueError('marker unexpectedly declares methods')
        receiver = selected['receiver']
        tests = [call for call in direct_calls(source, 'satisfies?') if call['call_source'] == '(satisfies? ISequential '+receiver+')']
        consumed = [call for call in direct_calls(source, selected['consumption_name']) if call['argument_count'] in selected['consumption_arities'] and call['call_source'] == '(-seq '+receiver+')']
        if not tests or not consumed:
            raise ValueError('absent original named marker receiver or consumption')
        return [{'protocol_name': authority['name'], 'protocol_body_sha256': authority['body_sha256'],
            'receiver_test': call, 'same_receiver_consumption': consumed, 'consumption_protocol': 'ISeqable',
            'declared_methods': [], 'receiver': receiver} for call in tests]
    if mode == 'protocol-method':
        obligations = {row['id']: row for row in map(json.loads, (HERE / 'obligations.jsonl').read_text().splitlines())}
        authority = obligations[selected['protocol_authority']]
        if authority['name'] != target['name'] or authority['body_sha256'] != selected['protocol_body_sha256'] or authority['declaration'] != 'defprotocol':
            raise ValueError('protocol method authority drift')
        declaration = authority['source_span']['text']
        if selected.get('marker'):
            raise ValueError('marker requires explicit marker receiver mode')
        else:
            method_signatures = {}
            for form in Reader(declaration).all():
                if head(form) != 'defprotocol':
                    raise ValueError('protocol declaration shape drift')
                for method in form.children[2:]:
                    if method.kind != 'list' or not method.children:
                        continue
                    named = method.children[0]
                    while named.kind == 'metadata':
                        named = named.children[-1]
                    arities = [len(vector.children) for vector in method.children[1:] if vector.kind == 'vector']
                    method_signatures[named.token] = arities
            if name not in method_signatures or sorted(selected['direct_arities']) != sorted(method_signatures[name]):
                raise ValueError('protocol method declared signature drift')
        receiver = selected['receiver']
        calls = [call for call in direct_calls(source, name) if call['argument_count'] in selected['direct_arities'] and (call['call_source'].startswith('('+name+' '+receiver+' ') or call['call_source'] == '('+name+' '+receiver+')')]
        if {call['argument_count'] for call in calls} != set(selected['direct_arities']):
            raise ValueError('absent exact demanded protocol method calls')
        if receiver == 'x' and '(let [x e y' not in source:
            raise ValueError('original receiver alias binding drift')
        if receiver == 'custom' and selected['workflow'] == 'arity-receiver-transient':
            if 'custom (reify' not in source or selected['protocol_name'] not in source or '(assert (= result [:transient [:conj 1] [:assoc :a 2] [:dissoc :a] [:assoc-n 0 4] :pop [:disj 1] :persistent]))' not in source:
                raise ValueError('original implemented protocol/event demand omitted')
        for call in calls:
            call.update(protocol_name=authority['name'], protocol_body_sha256=authority['body_sha256'], protocol_declaration_source=declaration, declared_method_arities=selected['direct_arities'], receiver=receiver)
        return calls
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
        for required in ['(satisfies? IMultiFn x)', '(= 41 (x :a))', '(= 41 ((get-method x :a) nil))', '(identical? identity (dispatch-fn x))', '(apply ->MultiFn', '(.-method-table x)']:
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
        context_paths = [path for companion in row.get('companion_sources', {}).values() for path in [companion['fixture'], companion['original_fixture']]]
        phases.append({**common, **execution, 'support_source_sha256': {path: digest((ROOT / path).read_bytes()) for path in [HELPER, SCANNER, row['fixture'], *(PRINT_SUPPORT if row['id'] == PRINT_ID else []), *context_paths]}})
        receipts.append({**selected, 'direct_calls': calls, 'whole_source_sha256': row['source_sha256'], 'original_case_sha256': row['case_sha256'], 'demand_basis': basis, 'invocation': row['invoke'] or 'complete expression', 'oracle_id': row['id']})
    return {'obligation-witnesses.jsonl': ordinary, 'phase-witnesses.jsonl': phases, 'arity-after190-call-receipts.jsonl': receipts}


def parse_frames(output):
    expected = [(row['id'], host, phase, engine) for row in rows() for host in ['fresh', 'reused'] for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
    frames = []
    for line in output.splitlines():
        if not line.startswith('arity-after190-observation\t'):
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
