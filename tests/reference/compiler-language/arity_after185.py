#!/usr/bin/env python3
"""Export frozen whole-oracle arity requirements without shared ledger writes."""
from pathlib import Path
import argparse
import hashlib
import json
import re
from arity_resume_candidates import digest, direct_calls
from extract_inventory import Reader
ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'tests/reference/compiler-language'
HELPER = 'tests/reference/compiler-language/arity_after185.py'
DRIVER = 'tests/compiler_arity_after185.rs'
TEST = 'complete_arity_oracles_execute_in_actual_phases_and_lifecycles'
SCANNER = 'tests/reference/compiler-language/arity_resume_candidates.py'
SCANNER_SHA256 = '31457848fe6d2f08e92b3ad5b4a6f99362a97be66e4819c9d45cf3a1b9ecbcc3'
REVIEWED = [{'case_sha256': 'b2dd1dea966e8633492601273d66a4e95c135be18b6a981277bbd176ad26d25f',
  'fixture': 'tests/fixtures/arity-after185/cljs-transients-collections-test-test-transient-edge-case-2.cljs',
  'id': 'cljs-transients-collections-test-test-transient-edge-case-2',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'f557427359c3ab2ec8a12a4e8b982ddfb030b7771907d79710bfa0a23a111984'},
 {'case_sha256': '3ca4c20373f2ef5585b4d655804c3741dbc46ef3ccbfebb54a3ecfb8b61c06ae',
  'fixture': 'tests/fixtures/arity-after185/cljs-residual-completion-cljs-2866-test.cljs',
  'id': 'cljs-residual-completion-cljs-2866-test',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '7d7dc1dc4ce22689a5dced635786b03453c33482482543b3bb214a8e1f8ce426'},
 {'case_sha256': 'df25ac5efb081e0d9c00911d61e7d7f225b76ef50c08dfe42b05a588bf31fea0',
  'fixture': 'tests/fixtures/arity-after185/cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aget-test.cljs',
  'id': 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aget-test',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '357b28a17ea2f46218d3addc7251e384ed62a0707a335a592c65289c29f3ace8'},
 {'case_sha256': 'e2ed44286f73beb6db39c953ceb0731cce3d4783dc5dc540746c0c8ccccde8b3',
  'fixture': 'tests/fixtures/arity-after185/cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aset-test.cljs',
  'id': 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aset-test',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '89fdd54ec1666798035881f4f9429e42e77d7f84519b5ec25d48142248d1f8af'},
 {'case_sha256': 'dfb3759467971700614e424022490dbc5a59198951aa597820b0ed443cd36ab1',
  'fixture': 'tests/fixtures/arity-after185/cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-aget-test.cljs',
  'id': 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-aget-test',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '5d8dae8081dd8403870e714a0b9182c18854bc5baa251f8ed548aff48a98c72e'},
 {'case_sha256': '676e0165142d9166965fe0d3fd79ef893a5b5d6b627545d701b065ac8a891d9d',
  'fixture': 'tests/fixtures/arity-after185/cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-set-test.cljs',
  'id': 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-set-test',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '3b679d9429bb167c643784c189f3bb2a7995a812f3033fdcbb760446d5f9a31d'},
 {'case_sha256': 'fd187c2792d87079e3fb6733431ca42206ef61dc29579660028ac414a134963f',
  'fixture': 'tests/fixtures/arity-after185/cljs-printing-completion-test-print-knobs.cljs',
  'id': 'cljs-printing-completion-test-print-knobs',
  'invoke': '(run nil)',
  'namespace': 'cljs.printing-test',
  'source_sha256': '0d8d8223c015e44b28db6553a3ab041c2e30ab794835f40e99b5ade6da24b439'},
 {'case_sha256': 'e64296866d5f93293548faded7a4b45529e4f650d6f0cc74c0f827f83c6ea2bf',
  'fixture': 'tests/fixtures/arity-after185/arity-extended-language-helper-printing.cljs',
  'id': 'arity-extended-language-helper-printing',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '6712490b0053b42babbadfb301fcdfa371d2f122827ed3c73497f89b80040db0'},
 {'case_sha256': 'c71f8749639ddf5cf3f98d89d083e35641912c6aebc1ac56c911dba24dce4484',
  'fixture': 'tests/fixtures/arity-after185/cljs-residual-source-context-apply.cljs',
  'id': 'cljs-residual-source-context-apply',
  'invoke': '(run)',
  'namespace': 'cljs.apply-test',
  'source_sha256': 'c57862a28018871e133538c6319a8d3737878c8ff0662a585cd1ca3aadb9ddf7'},
 {'case_sha256': '805221c48a30f94b497906ebed0e918ebdd796ee754e09fd4789cf1ea11fc78d',
  'fixture': 'tests/fixtures/arity-after185/cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests.cljs',
  'id': 'cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '88f31d492e2dd53c2b0b00f0d3940607c487492eadb2cf4aa5af7cf0b5ee21ff'},
 {'case_sha256': '3c3b770462efdf5a44010d515391b7cd3a0327fb15a7bf1b72104913b111167c',
  'fixture': 'tests/fixtures/arity-after185/cljs-core-gate-literal-print-callback-failure.cljs',
  'id': 'cljs-core-gate-literal-print-callback-failure',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '26492e04157f83fc8ed358190d531f97ea9d23ec1939bf0e620f4c47bfc39faa'},
 {'case_sha256': '4dabecb390dc5cda24e6e8dc024ecf8293d78796a38d2f9824bd7925ad0b29dd',
  'fixture': 'tests/fixtures/arity-after185/cljs-destructure-trailing-map-destructuring.cljs',
  'id': 'cljs-destructure-trailing-map-destructuring',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '2d6a29f8c90297dfc868a38455f43554b67afcd5c2f50136f5438b3a2163bd73'},
 {'case_sha256': 'b54cb8da46f00f785b56a5925827c587ef3f2e55a507776ba899557031b6c3fd',
  'fixture': 'tests/fixtures/arity-after185/cljs-exceptions-throwable-map.cljs',
  'id': 'cljs-exceptions-throwable-map',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '6d4da01d8c8cfe6a264b94c6ed262bf931dcb1f5b298cb957c549c6bd0eef736'},
 {'case_sha256': '49ffaac78837191b59c6b717bc4673da98cfc76452bd50ee85527782aa29f658',
  'fixture': 'tests/fixtures/arity-after185/cljs-state-test-atoms-and-volatile.cljs',
  'id': 'cljs-state-test-atoms-and-volatile',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '12ef8327ec23966b86e8560c308fecb8fd60e2c5e1f869bff492003c007f2cd8'},
 {'case_sha256': '1fc7cab64315cbb1da8c7b022a4115835e5434ddaa0baab563ac5f82d8af4759',
  'fixture': 'tests/fixtures/arity-after185/cljs-definitions-test-var-predicate.cljs',
  'id': 'cljs-definitions-test-var-predicate',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '7e287c6fe5b418bba9bd66a9675beccf3028ddde9a365732e8405c7c90260293'},
 {'case_sha256': '34c6403ccb170c79fe71c2a0de095a6ee3fd8fd956a061cd2220c25a2a73ab01',
  'fixture': 'tests/fixtures/arity-after185/cljs-definitions-test-resolve.cljs',
  'id': 'cljs-definitions-test-resolve',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '619f0c9d9dda15ce021b4d1e6432d6daad54c4d294eb50621cb0662f39e569c7'},
 {'case_sha256': '2ce2dac044ef9d7decce3f5c98419d5bea53c00e9b5b562f86ed6742c16d7009',
  'fixture': 'tests/fixtures/arity-after185/cljs-definitions-test-cljs-3333.cljs',
  'id': 'cljs-definitions-test-cljs-3333',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'b1ebad6f07e1a8279333b79a17427972cad8fcec105a84a2341770a09ca323e1'},
 {'case_sha256': 'b7b7debf3f663c16b5bfe37454320fddc7558e2dcc6c26b65da4d776ff221800',
  'fixture': 'tests/fixtures/arity-after185/cljs-interop-core-test-memfn-test.cljs',
  'id': 'cljs-interop-core-test-memfn-test',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '671dd3c6c649f5f937336356878047036b58ea1d98f56ea5082eb9a326081caa'},
 {'case_sha256': 'dad9b50ec6d250ee1b3a2b92aae8c78f2f343dde552668d57c7d644c6fdd93a4',
  'fixture': 'tests/fixtures/arity-after185/cljs-interop-primitives-test-test-arrays.cljs',
  'id': 'cljs-interop-primitives-test-test-arrays',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '01994357b0f43b06956e64e96721a58eb087bf19a09fc4c561e5c5d4439c2b0f'},
 {'case_sha256': '701398af2638b23461dd60be9f7e220c6844e44271cc939098fd6fb904f0a196',
  'fixture': 'tests/fixtures/arity-after185/cljs-extension-test-cljs-2974.cljs',
  'id': 'cljs-extension-test-cljs-2974',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'e6a11616ec3901b1d060d6c6ff8cf977f4c668fb95e9abc42c0ff9e28c8f7f1a'},
 {'case_sha256': '0d4035afaf7d6e48395ea1d320d9323fb219fd3f5bb9a61f38ec8071069589ce',
  'fixture': 'tests/fixtures/arity-after185/cljs-extension-test-414.cljs',
  'id': 'cljs-extension-test-414',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'd02977d3d4f79c18f4f126863ea4b322432fdfd2ab7f14fde23119a15658b93a'},
 {'case_sha256': '285bead379663aa360eb3944329168ee9b28644f2673188076291e959d3a0bfc',
  'fixture': 'tests/fixtures/arity-after185/cljs-extra-seqs-test-distinct.cljs',
  'id': 'cljs-extra-seqs-test-distinct',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'ade8441320939e9dfbb0e58a4f8df8f03cc4f0e284a3e14f0a377d3a4f2f00bd'},
 {'case_sha256': 'c259b9c5bf365e7ecbf8c53a397f9f9b2c3472e5cbaf21de6c647ba4233adbec',
  'fixture': 'tests/fixtures/arity-after185/cljs-extra-primitives-test-misc.cljs',
  'id': 'cljs-extra-primitives-test-misc',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '8fe1ddd01699ce44d18670148ae75957b27d1fc7ba149fe158fe86438df8e028'},
 {'case_sha256': '732be07d32f7db5f56af527857f1c2774dd813e010eed62db7f41897b0599cb5',
  'fixture': 'tests/fixtures/arity-after185/cljs-extra-seqs-test-partitionv-all.cljs',
  'id': 'cljs-extra-seqs-test-partitionv-all',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'c7f8e5442cf95535ec3307bb5fde0453411e08e538911726ad542f5fe9a04db8'},
 {'case_sha256': 'f2a9612413fd2c14ced32ee19447da909e12062f47afb24062ba57763ca61c51',
  'fixture': 'tests/fixtures/arity-after185/cljs-extra-seqs-test-partitionv.cljs',
  'id': 'cljs-extra-seqs-test-partitionv',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '0204c3db608ffe1127ae98b62a33d7ea7a487735b72896b242788f7281d8861e'},
 {'case_sha256': '5917037bfb415bd953340c350a4ee9c3ec64e58bae715af97ead8d03002e2fd1',
  'fixture': 'tests/fixtures/arity-after185/cljs-extra-primitives-test-496.cljs',
  'id': 'cljs-extra-primitives-test-496',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'c2de800847ba3ae082951ea9583c6c5bfafe211bd63b0391457d929c5e44fabf'},
 {'case_sha256': '710efbfa02d293a69d551f06e4c8fa7338f7983f6de1d657c7558712b9adcef0',
  'fixture': 'tests/fixtures/arity-after185/cljs-integration-test-run-bang.cljs',
  'id': 'cljs-integration-test-run-bang',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '249be5f47d98ee0a4566961fffbf06ab8634f29df13b6bd1cfb74d91e3a60400'},
 {'case_sha256': '5e3c6ce680bd268726d1332b551f0112770c31c7dde6037dca359447b857da75',
  'fixture': 'tests/fixtures/arity-after185/cljs-extra-primitives-test-arithmetic.cljs',
  'id': 'cljs-extra-primitives-test-arithmetic',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '29be765d74cf31cbecee6e62c083c6b563d7174198afe92dba7c125c097933a5'},
 {'case_sha256': 'f49fc02e6620a423ca45a260f5fd9ad3b5a04c0284a9a829a16c471c81c7ed9b',
  'fixture': 'tests/fixtures/arity-after185/cljs-extra-core-js-invoke-test.cljs',
  'id': 'cljs-extra-core-js-invoke-test',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'd418fe06891f737eef04d3908e32c17a205c0fcbffa586957b68d8e1b2897ced'},
 {'case_sha256': '78f7d559d24b32f9f57479c5d910364cd67186676334427fbea8f07348bfe48e',
  'fixture': 'tests/fixtures/arity-after185/cljs-extra-core-test-cljs-3400.cljs',
  'id': 'cljs-extra-core-test-cljs-3400',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '09fd50c04e73fc7dc42502c991c4453a64fe022f79afdcc1119c7dddee0dc162'},
 {'case_sha256': 'a5d75d5173373043edb82903472014e3f69c6b413d0288cd315816d4a922bf95',
  'fixture': 'tests/fixtures/arity-after185/cljs-final-seqs-iteration-seq-equals-reduce.cljs',
  'id': 'cljs-final-seqs-iteration-seq-equals-reduce',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '83f428bd07cf71f156cceef357a329faf954b7174c57822147c4bed09cf21b56'},
 {'case_sha256': '5bc090c2a0deea92b0fd4a4406a15d2f957e9932454f32c72050a2654163a10c',
  'fixture': 'tests/fixtures/arity-after185/cljs-protocol-test-extend-to-object.cljs',
  'id': 'cljs-protocol-test-extend-to-object',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'e3fa8fc1458a1abe311734f74a0c358a23026938efeae99dc8accb6fa1993469'}]
DEMAND = {'arity-extended-language-helper-printing': 'The complete original JSON string comparison demands '
                                            'print-level, print-namespace-maps and print-newline '
                                            'bindings through actual printing and captured stdout.',
 'cljs-core-gate-literal-print-callback-failure': 'The original binding installs the actual error '
                                                  'sink as *print-fn*. println invokes it; ordered '
                                                  'callback effects and the exact thrown ex-info '
                                                  'message/data are independently asserted, with '
                                                  'restoration in finally and the original active '
                                                  'sink checks preserved.',
 'cljs-definitions-test-cljs-3333': 'The original defonce root is read and must equal17; '
                                    'declaration parsing alone grants no credit.',
 'cljs-definitions-test-resolve': 'The original resolved first Var is actually called on the '
                                  'original vector and must return the literal1.',
 'cljs-definitions-test-var-predicate': 'Original Var predicate checks observe a real core Var and '
                                        'the independent non-Var numeric control.',
 'cljs-destructure-trailing-map-destructuring': 'Complete original trailing-map destructuring '
                                                'exercises actual variadic callbacks, partial '
                                                'calls and explicit seq-to-map conversions against '
                                                'original independent maps/results.',
 'cljs-exceptions-throwable-map': 'Complete original nested ex-info and js/Error chain setups are '
                                  'converted and every original cause/data/via/type/message and '
                                  'trace control is asserted.',
 'cljs-extension-test-414': 'Every original specified vector receiver is dispatched through -woz; '
                            'original equality/nonidentity and self-return controls remain.',
 'cljs-extension-test-cljs-2974': 'The actual extended array empty protocol is dispatched and the '
                                  'original array?/empty? checks must both hold.',
 'cljs-extra-core-js-invoke-test': 'The original doto inserts the actual receiver into '
                                   'fixture-set; js-invoke then invokes the installed two-argument '
                                   'callback and must return the independent literal5. Unused '
                                   'declaration sites supply no unchecked-get/set credit.',
 'cljs-extra-core-test-cljs-3400': 'Both original zero-argument logical macros are demanded by '
                                   'original true?/nil? assertions; there are no operand callbacks '
                                   'or identity surrogates.',
 'cljs-extra-primitives-test-496': 'Both original char numeric and character inputs must yield the '
                                   'independent original A literal.',
 'cljs-extra-primitives-test-arithmetic': 'The entire original arithmetic body retains all direct '
                                          'and apply operations, closure/iteration setup, integer '
                                          'controls and original long conversion literals.',
 'cljs-extra-primitives-test-misc': 'The whole original primitives body consumes both replacement '
                                    'workflows and the lazy-cat output through independent '
                                    'complete literals; all other original '
                                    'arithmetic/collection/interop controls remain.',
 'cljs-extra-seqs-test-distinct': 'The whole original distinct body realizes every original '
                                  'finite/lazy input, duplicate/equality case and independent '
                                  'expected sequence.',
 'cljs-extra-seqs-test-partitionv': 'All original partitionv inputs/steps/padding forms are '
                                    'realized against original groups and empty controls; unused '
                                    'fixture-get/set declarations supply no credit.',
 'cljs-extra-seqs-test-partitionv-all': 'Both original partitionv-all forms are completely '
                                        'realized and compared against original independent vector '
                                        'groups; unused fixture-get/set declarations supply no '
                                        'credit.',
 'cljs-final-seqs-iteration-seq-equals-reduce': 'The original RNG and all49 seed/initk '
                                                'combinations remain; unchecked-add executes '
                                                'inside each demanded iteration callback and '
                                                'original complete seq/reduce results agree.',
 'cljs-integration-test-run-bang': 'The actual original run! callback mutates its atom to10; '
                                   'completion returns nil and the original reduced/infinite input '
                                   'control remains.',
 'cljs-interop-core-test-memfn-test': 'Both original memfn callbacks are actually invoked; substr '
                                      'has its original two parameters and trim is consumed by the '
                                      'complete original map comparison.',
 'cljs-interop-primitives-test-test-arrays': 'The entire original array body executes '
                                             'index-dependent amap multiplication and areduce '
                                             'addition, requiring independent [10 20 30]/6 results '
                                             'and preserving array mutation and conversion '
                                             'controls.',
 'cljs-printing-completion-test-print-knobs': 'The original run callback is called with nil; all '
                                              'eight print-length bindings must produce the '
                                              'original independent vector/map strings and '
                                              'alternatives.',
 'cljs-protocol-test-extend-to-object': 'The original object receiver dispatches all original '
                                        'ISeqable/ILookup methods through '
                                        'seq/get/missing-key/reduce observations; original '
                                        'callback bodies, arities and independent literals are '
                                        'preserved.',
 'cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests': 'The entire original '
                                                                        'map-entry-interface-tests '
                                                                        'helper is called on both '
                                                                        'original sorted-map '
                                                                        'receivers and the '
                                                                        'explicit MapEntry '
                                                                        'receiver. Both -equiv '
                                                                        'comparisons execute for '
                                                                        'every receiver; the '
                                                                        'complete165-element '
                                                                        'independent expected '
                                                                        'boolean vector observes '
                                                                        'every protocol, lookup, '
                                                                        'reduction, caught failure '
                                                                        'and callable control '
                                                                        'unchanged.',
 'cljs-residual-completion-cljs-2866-test': 'The original anonymous callback is called on (atom '
                                            '1); while mutates it to zero and the original '
                                            'dereferenced result is asserted.',
 'cljs-residual-source-context-apply': 'The original run callback executes every apply arity, the '
                                       'callback using js-arguments is invoked with original '
                                       '3/30-element arrays, and both copy-arguments callbacks are '
                                       'invoked and must preserve the complete original 30-element '
                                       'independent result. Original metadata functions, receiver '
                                       'identity, failures, doseq loops and all apply forms '
                                       'remain.',
 'cljs-state-test-atoms-and-volatile': 'Complete original atom and volatile workflow mutates and '
                                       'dereferences actual cells; volatile? observes both true '
                                       'volatile and false atom receivers with original '
                                       'state/metadata checks retained.',
 'cljs-transients-collections-test-test-transient-edge-case-2': 'The complete original transient '
                                                                'loop runs all 100 elements; '
                                                                "condp's predicate selects actual "
                                                                'conj!/assoc! branches and the '
                                                                'realized result equals the '
                                                                'independent original range.',
 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aget-test': 'The original '
                                                                                     'checked-aget-alias '
                                                                                     'function '
                                                                                     'remains '
                                                                                     'variadic and '
                                                                                     'applies '
                                                                                     'checked-aget '
                                                                                     'to unchanged '
                                                                                     'arguments; '
                                                                                     'every '
                                                                                     'original '
                                                                                     'call, error '
                                                                                     'catch, '
                                                                                     'array/object/index '
                                                                                     'case, and '
                                                                                     'with-redefs '
                                                                                     'body '
                                                                                     'executes.',
 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aset-test': 'The original '
                                                                                     'checked-aset-alias '
                                                                                     'function '
                                                                                     'remains '
                                                                                     'variadic and '
                                                                                     'applies '
                                                                                     'checked-aset '
                                                                                     'to unchanged '
                                                                                     'arguments; '
                                                                                     'every '
                                                                                     'original '
                                                                                     'mutation, '
                                                                                     'returned '
                                                                                     'value, error '
                                                                                     'catch and '
                                                                                     'nested index '
                                                                                     'is asserted.',
 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-aget-test': 'Every '
                                                                                       'original '
                                                                                       'unchecked-get '
                                                                                       'call '
                                                                                       'remains in '
                                                                                       'the '
                                                                                       'complete '
                                                                                       'source; '
                                                                                       'nil '
                                                                                       'failures, '
                                                                                       'absent '
                                                                                       'properties '
                                                                                       'and '
                                                                                       'independent '
                                                                                       'indexed '
                                                                                       'values are '
                                                                                       'asserted.',
 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-set-test': 'Every '
                                                                                      'original '
                                                                                      'unchecked-set '
                                                                                      'call '
                                                                                      'remains in '
                                                                                      'the '
                                                                                      'complete '
                                                                                      'source; '
                                                                                      'mutations '
                                                                                      'and '
                                                                                      'independent '
                                                                                      'indexed '
                                                                                      'values are '
                                                                                      'asserted.'}
SELECTION = [{'body_sha256': '3683ea5162c71aa7a678217d1565f80cd0a131c3ba73e1dc09f51c9bb0ffcf82',
  'call_name': '*print-newline*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::*print-newline*::arity-1',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-extended-language-helper-printing'},
 {'body_sha256': '08d935a5cd887cf2da3d590db823b314700e7bc5369480f758fa772899eea109',
  'call_name': '*print-namespace-maps*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::*print-namespace-maps*::arity-1',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-extended-language-helper-printing'},
 {'body_sha256': '1c9b8ee32617d9efd340f25828391d0dfa5d0486dea8a3be725ae0f0842aa2be',
  'call_name': '*print-length*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::*print-length*::arity-1',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-printing-completion-test-print-knobs'},
 {'body_sha256': '15f34288bd3b9560dd0d1f6ce0c566ec93b6a4991585223204b3602c7f7f8ea0',
  'call_name': '*print-level*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::*print-level*::arity-1',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'arity-extended-language-helper-printing'},
 {'body_sha256': '6176bf6f7b6658c5a5e7428773b513a5ac02fac64d2d66967db73397e100bf59',
  'call_name': 'unchecked-get',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::unchecked-get::arity-1',
  'signature_sha256': '772c0487ff6df5a1fab64ac54d7c8cdc409bc56eb95a39c22005ff591a704783',
  'workflow': 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-aget-test'},
 {'body_sha256': 'b7ab595af1fb952a1ca2677c6badf10f34c25c0401025f43364b3f101ff94b91',
  'call_name': 'unchecked-set',
  'direct_arities': [3],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::unchecked-set::arity-1',
  'signature_sha256': '8798d7a3b2d9532b8958678813e4cec0c20306042a4aa980d2eb26027a102440',
  'workflow': 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-set-test'},
 {'body_sha256': 'a939fe037a20fc79caf0d4eeafa4c2e9039ce8a65eeca36dae2046feb4e1c837',
  'call_name': 'with-redefs',
  'direct_arities': [16],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::with-redefs::arity-1',
  'signature_sha256': 'bbce2142210ff6d05831b75efcba8523d8c57f11c137ed69448700c494e521ad',
  'workflow': 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aget-test'},
 {'body_sha256': '809b222e204dba35aff2a9a0b887542994fd4e008a53b45d42692bad784be30c',
  'call_name': 'condp',
  'direct_arities': [6],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::condp::arity-1',
  'signature_sha256': 'a86c123101764cc0dc24d4012be9d5c2b762ae03a7529a6020c7313c06da704c',
  'workflow': 'cljs-transients-collections-test-test-transient-edge-case-2'},
 {'body_sha256': '67a88df5e6560582c9f6fab1c467310ff809a3cc24d6e7bb249d6acf3ef8add0',
  'call_name': '*print-length*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'local::src/core_var_metadata.tsv::*print-length*',
  'signature_sha256': None,
  'workflow': 'cljs-printing-completion-test-print-knobs'},
 {'body_sha256': 'f16d1d268c2ce38f971036de72832b419fe07a1608d1ea82a4f9791a14b27b33',
  'call_name': '*print-level*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'local::src/core_var_metadata.tsv::*print-level*',
  'signature_sha256': None,
  'workflow': 'arity-extended-language-helper-printing'},
 {'body_sha256': '8a81e947720e489ec42ec9c99c0428a90570febbd47b722ad399f4a2fe75a0b6',
  'call_name': '*print-namespace-maps*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'local::src/core_var_metadata.tsv::*print-namespace-maps*',
  'signature_sha256': None,
  'workflow': 'arity-extended-language-helper-printing'},
 {'body_sha256': '9d0f62a8adef1268495b51a92c3e06473dff1638a9f0d06b458f26fbf6298700',
  'call_name': '*print-newline*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'local::src/core_var_metadata.tsv::*print-newline*',
  'signature_sha256': None,
  'workflow': 'arity-extended-language-helper-printing'},
 {'body_sha256': '0d19eecc39ed361dccd97fd8b12f084f282a4e0c0a5b2073457478968b61ae2e',
  'call_name': 'checked-aget-alias',
  'direct_arities': [2, 3],
  'evidence_mode': 'applied-alias',
  'obligation_id': 'local::src/core_var_metadata.tsv::checked-aget',
  'signature_sha256': None,
  'workflow': 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aget-test'},
 {'body_sha256': 'f651f9889b143d60ac18311c1e20cc9aaa25e81193feddba7bf48479fc504701',
  'call_name': 'checked-aset-alias',
  'direct_arities': [3, 4],
  'evidence_mode': 'applied-alias',
  'obligation_id': 'local::src/core_var_metadata.tsv::checked-aset',
  'signature_sha256': None,
  'workflow': 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aset-test'},
 {'body_sha256': '61f6c9b7b245452e0ade5d6b4ae6aecd27127bae44c7ed07533ad731128629f9',
  'call_name': 'while',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::while~2::arity-1',
  'signature_sha256': 'c32eb1a647be266f5f6fcee0babee364593fa3206a1e72051d39cc4607ab3378',
  'workflow': 'cljs-residual-completion-cljs-2866-test'},
 {'body_sha256': '659be75b3bd855c32a18e08334010d88d1a7caa76788101a427ee4b543bff054',
  'call_name': 'js-arguments',
  'direct_arities': [0],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::js-arguments::arity-1',
  'signature_sha256': 'c04963f87d82d63dd1d5239f487499f7c8f6ac7f34ad0470e03c0c5f4f28a5ec',
  'workflow': 'cljs-residual-source-context-apply'},
 {'body_sha256': 'a75456308dc292bf8d4d0fefec3e9d60e64c3291edbf534144c87c8f89fac4c3',
  'call_name': 'copy-arguments',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::copy-arguments::arity-1',
  'signature_sha256': '517baedc51a76764721c621e64dec82a9463246b0d4c574d774cc56898ac0995',
  'workflow': 'cljs-residual-source-context-apply'},
 {'body_sha256': '84667a2dd901b6a2f5d75518a34c3d7f30fb58fc0044a7414ca3a298fb6e3e1b',
  'call_name': '-equiv',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::-equiv::arity-1',
  'signature_sha256': '3d25c5ad54dfe4edd1d97dfe0da655c9eb67405b234c9632bd85078405b31e54',
  'workflow': 'cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests'},
 {'body_sha256': '8cc85514265426fabd6a4bbf95f4524c28dd215b2d0ab9b11e48f6c62133c084',
  'call_name': '-equiv',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::-equiv',
  'signature_sha256': None,
  'workflow': 'cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests'},
 {'body_sha256': '21ade17aeec085f0eee205d55b61a9ce037445410fd81f111bb9b4bd5e0ccd38',
  'call_name': '*print-fn*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::*print-fn*::arity-1',
  'signature_sha256': '1213ea8939d9097389fda65beeee9b719ea267aacfc57a7ae4fb6eb42d73a382',
  'workflow': 'cljs-core-gate-literal-print-callback-failure'},
 {'body_sha256': 'b04c251b039c7aa1b8f136e8b4e4a5f76ac4afcf169f7bf18bcf860105450591',
  'call_name': '*print-fn*',
  'direct_arities': [],
  'evidence_mode': 'binding-control',
  'obligation_id': 'local::src/core_var_metadata.tsv::*print-fn*',
  'signature_sha256': None,
  'workflow': 'cljs-core-gate-literal-print-callback-failure'},
 {'body_sha256': 'f9f87ae0ecd58926cc456e2035ecb730eb5ab8baefd245e42c032e4dce26ae52',
  'call_name': 'seq-to-map-for-destructuring',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::seq-to-map-for-destructuring::arity-1',
  'signature_sha256': 'cf54978776ee23317c8af7fb2d95c45168beeeef8358b0efd630f7a9744c8f04',
  'workflow': 'cljs-destructure-trailing-map-destructuring'},
 {'body_sha256': 'df475ca1272ea96d1cc46026585534759acf8582f233c2fd3a0d60e55d2f4ce2',
  'call_name': 'seq-to-map-for-destructuring',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::seq-to-map-for-destructuring',
  'signature_sha256': None,
  'workflow': 'cljs-destructure-trailing-map-destructuring'},
 {'body_sha256': '8c6941cc7fd3756c8992d3f2d4ffa89e7b16d5c5ade39ce3b8f5ba5e81dbb4a9',
  'call_name': 'Throwable->map',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::Throwable->map::arity-1',
  'signature_sha256': '9d93ec0f77d8661e172c929cfa88316c729ff463b17edb3fd0bcd38a88cc76ae',
  'workflow': 'cljs-exceptions-throwable-map'},
 {'body_sha256': '220c49fee5c467e8e62521b925a1d9042cb2648bb4f2d7f7e05b7a7e19bca500',
  'call_name': 'Throwable->map',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::Throwable->map',
  'signature_sha256': None,
  'workflow': 'cljs-exceptions-throwable-map'},
 {'body_sha256': '6d80da1be7ba97440dc307ff2937fbab3931b2035834fb3012321cf7a2550f1d',
  'call_name': 'volatile?',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::volatile?::arity-1',
  'signature_sha256': '57825001ab734a2f9387d427d94f3d4486d027e222ba0ff2737b638c9e9921c6',
  'workflow': 'cljs-state-test-atoms-and-volatile'},
 {'body_sha256': 'da519823fd9a9e9afe1618b2d40cc1379020a6830555043f42097f9e1fa4d4c0',
  'call_name': 'volatile?',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::volatile?',
  'signature_sha256': None,
  'workflow': 'cljs-state-test-atoms-and-volatile'},
 {'body_sha256': '0a9c23320fd575756ec652d6ea71e44759efde674c6fb55ca841c019d976dda4',
  'call_name': 'var?',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::var?::arity-1',
  'signature_sha256': 'c36cd365c19c27aafad3a2c8b38fe680298a1092cb7d2ad4ecc5ec845122e745',
  'workflow': 'cljs-definitions-test-var-predicate'},
 {'body_sha256': '4b5b53dca4286e25c6da5d5b79001ce9780786223432af6b66217feb0aac22b2',
  'call_name': 'var?',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::var?',
  'signature_sha256': None,
  'workflow': 'cljs-definitions-test-var-predicate'},
 {'body_sha256': 'a4992c8496855a343e00c1f22189b6f0b35ed18b62fae1804e5d1fb953464e0f',
  'call_name': 'resolve',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::resolve::arity-1',
  'signature_sha256': 'd0010f872ab5287809dd02534a6433dcb856834f5690011fde1b389b08761926',
  'workflow': 'cljs-definitions-test-resolve'},
 {'body_sha256': 'e6c4c9fac32029e2acbc116f0e09c92c4a1763b5342d27e3fef60422d18b56a2',
  'call_name': 'defonce',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::defonce::arity-1',
  'signature_sha256': '233cd43dc6734a67a74a3b0d9944b0c62d3197a29343ee50f5b697c01c08953c',
  'workflow': 'cljs-definitions-test-cljs-3333'},
 {'body_sha256': '43f19a7d43c209e58c388a8bc491018a5aa9ed27ced731ca534bdd81c363b290',
  'call_name': 'memfn',
  'direct_arities': [1, 3],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::memfn~2::arity-1',
  'signature_sha256': '60e6849420e0b1891f19202ba0ce6937a84d6905dddb0b994a99e66b8717ae15',
  'workflow': 'cljs-interop-core-test-memfn-test'},
 {'body_sha256': '31f8974c3eb67c4fca8326a870c8583630110f6e69e40c64818543675919c199',
  'call_name': 'amap',
  'direct_arities': [4],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::amap::arity-1',
  'signature_sha256': '3c24c2b22f4d27ec70249c42d2e1ba9e8792a1fab98c4b76d7a0946d2be796a6',
  'workflow': 'cljs-interop-primitives-test-test-arrays'},
 {'body_sha256': 'def60d92ea3d6bb8ae071b006c5b6528700da531f9672d96d4854b21d24e8914',
  'call_name': 'areduce',
  'direct_arities': [5],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::areduce::arity-1',
  'signature_sha256': 'd8971d351fb7da1e9fa7340b21eb5d70f0c4861533b63aa610fbce8e040be5ad',
  'workflow': 'cljs-interop-primitives-test-test-arrays'},
 {'body_sha256': '0491face2050dc43aed2a66480bb539f483bd666d05fa9f5d3f9edbf2bb6b0ae',
  'call_name': 'extend-protocol',
  'direct_arities': [3],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::extend-protocol~2::arity-1',
  'signature_sha256': 'e8a4f53c7f845c169fd2e7d5c2325fc83fefc2599b2bd98393897a5b53661880',
  'workflow': 'cljs-extension-test-cljs-2974'},
 {'body_sha256': 'a31437df7cffa3477ec2cda5b09a0a6d49c75daa6ce7d989dfecd743e94f6f60',
  'call_name': 'specify',
  'direct_arities': [3],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::specify::arity-1',
  'signature_sha256': 'b7b396081cb180f929858a50442768dbeb7cb37ff5a688affe9ac31585ca330c',
  'workflow': 'cljs-extension-test-414'},
 {'body_sha256': 'ecf17944984ddb617b34b61613cba80bb9fd2517e1629ec86410a21a43274786',
  'call_name': 'distinct',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::distinct::arity-2',
  'signature_sha256': '7e858f7b7ea7a24bf6340aa43b8567afa8e81dcdda941b22e94e72485515449d',
  'workflow': 'cljs-extra-seqs-test-distinct'},
 {'body_sha256': 'bf1a9863d51d7d8e945710b95093256d960354b733fe7c02d63527c9fe89db4c',
  'call_name': 'replace',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::replace::arity-2',
  'signature_sha256': 'c4fce9ac0cd149a6e7fdecc063912ba9313582b38d668de7da04124774fdadd1',
  'workflow': 'cljs-extra-primitives-test-misc'},
 {'body_sha256': '792d20c1e277dc345fe228e4e188693a49bffd61db5471178ac230d1f1003b24',
  'call_name': 'lazy-cat',
  'direct_arities': [3],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::lazy-cat::arity-1',
  'signature_sha256': '8029ce0cade5bffa280267586918906a59218e3cee79d049ae93c453a5efb1bb',
  'workflow': 'cljs-extra-primitives-test-misc'},
 {'body_sha256': 'a5e651f4da4d64b075c0e717dc1e8f3d843e92a19a228247ab2318c0871b6693',
  'call_name': 'partitionv-all',
  'direct_arities': [2, 3],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::partitionv-all::arity-3',
  'signature_sha256': 'e414c7d9e366af37cb0e9336c51a3e934c48e06dd7a2e8bf44cf67e76a76bc82',
  'workflow': 'cljs-extra-seqs-test-partitionv-all'},
 {'body_sha256': 'f9d1b2621a007c686aad3d5d8eaa5dc9198c10a8b788b4e57de8141ca9157b69',
  'call_name': 'partitionv',
  'direct_arities': [2, 3],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::partitionv::arity-2',
  'signature_sha256': 'e414c7d9e366af37cb0e9336c51a3e934c48e06dd7a2e8bf44cf67e76a76bc82',
  'workflow': 'cljs-extra-seqs-test-partitionv'},
 {'body_sha256': '21eeab09f45a64570b03a1f07bc07464ee49ee22e53e8fa21ea0b81a7f31d868',
  'call_name': 'char',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::char::arity-1',
  'signature_sha256': '57825001ab734a2f9387d427d94f3d4486d027e222ba0ff2737b638c9e9921c6',
  'workflow': 'cljs-extra-primitives-test-496'},
 {'body_sha256': '54770d4f06fc9eee200c7dec81d0736985e2cb4d7b265b71bb62cbc945519652',
  'call_name': 'char',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::char',
  'signature_sha256': None,
  'workflow': 'cljs-extra-primitives-test-496'},
 {'body_sha256': '9490aeaa1d1ec425680594d5158cddf2186c6f3c6b9cb83fbc853c611b534da7',
  'call_name': 'run!',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::run!::arity-1',
  'signature_sha256': 'ede51c510a83893254047479d53133d05b81557ec7f1590ec1b81bb8c76f27d2',
  'workflow': 'cljs-integration-test-run-bang'},
 {'body_sha256': '56af5b7f256a00e5c07cb9ba0534ea1442db7bca9c26bd5ec8d4d6eee39a9807',
  'call_name': 'run!',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::run!',
  'signature_sha256': None,
  'workflow': 'cljs-integration-test-run-bang'},
 {'body_sha256': '1d562c316078f8dbdf54612a236b04edddc2ed0fa16e368087ca1fc32614de83',
  'call_name': 'long',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::long::arity-1',
  'signature_sha256': '57825001ab734a2f9387d427d94f3d4486d027e222ba0ff2737b638c9e9921c6',
  'workflow': 'cljs-extra-primitives-test-arithmetic'},
 {'body_sha256': '026dd2b33fd7d70da0bddb237ed467948325c40e7e1204f6b48db0822334eee7',
  'call_name': 'long',
  'direct_arities': [1],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::long',
  'signature_sha256': None,
  'workflow': 'cljs-extra-primitives-test-arithmetic'},
 {'body_sha256': 'f320bfdf5e5994d0e7d0b1c6f6ef71a8b83eb5ef70370554f149fa274bfdd4bc',
  'call_name': 'doto',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::doto::arity-1',
  'signature_sha256': 'c84b54d05b6efefdcdb0e2c625c39581ed4be384969569977453aff6743bf255',
  'workflow': 'cljs-extra-core-js-invoke-test'},
 {'body_sha256': '47294062e14f9f3c1a423172b781387e1318d2a27356765141bbd43ebe161b68',
  'call_name': 'and',
  'direct_arities': [0],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::and::arity-1',
  'signature_sha256': 'c04963f87d82d63dd1d5239f487499f7c8f6ac7f34ad0470e03c0c5f4f28a5ec',
  'workflow': 'cljs-extra-core-test-cljs-3400'},
 {'body_sha256': '4bc31939598ab425dd1fa28c0abe246ceba6d4abd3e598db29253a951432f566',
  'call_name': 'or',
  'direct_arities': [0],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::or::arity-1',
  'signature_sha256': 'c04963f87d82d63dd1d5239f487499f7c8f6ac7f34ad0470e03c0c5f4f28a5ec',
  'workflow': 'cljs-extra-core-test-cljs-3400'},
 {'body_sha256': 'c904915ea6c48bd1cce808b3a9c23482dd8e9311126ad4d65d70af9ffb94de52',
  'call_name': 'extend-type',
  'direct_arities': [5],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::extend-type::arity-1',
  'signature_sha256': '6dbd759d1fec44145e47f3d59cbd5627c3345694bd4968a3264b23f169614463',
  'workflow': 'cljs-protocol-test-extend-to-object'},
 {'body_sha256': 'bde68a16c15e01bc9fa7ab225ee16921d3b86b8d9bff09c42f9ea719f17b93a9',
  'call_name': 'unchecked-add',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::unchecked-add::arity-3',
  'signature_sha256': 'ef2c01168fb717272a4131e897c54e6f74baaf751c7a389d84b8ecd96e1637eb',
  'workflow': 'cljs-final-seqs-iteration-seq-equals-reduce'},
 {'body_sha256': '0d835d61268b9275aea0e3366e1c20d380af970e61ea257153ed7414c1e16912',
  'call_name': 'unchecked-add',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::unchecked-add::arity-1',
  'signature_sha256': '9311ff2c6c5473b1bfb1d1f632051af1d9d0389b2fcaf8f91a2ae7518063e1ad',
  'workflow': 'cljs-final-seqs-iteration-seq-equals-reduce'},
 {'body_sha256': 'c1e589f8a02c609db6de852308229d02a6da225d87e78222c14828b0d76bd37e',
  'call_name': 'unchecked-add',
  'direct_arities': [2],
  'evidence_mode': 'direct',
  'obligation_id': 'local::src/core_var_metadata.tsv::unchecked-add',
  'signature_sha256': None,
  'workflow': 'cljs-final-seqs-iteration-seq-equals-reduce'}]
EXPECTED_LITERAL_RAW = {'arity-extended-language-helper-printing': 'true',
 'cljs-core-gate-literal-print-callback-failure': 'true',
 'cljs-definitions-test-cljs-3333': 'true',
 'cljs-definitions-test-resolve': 'true',
 'cljs-definitions-test-var-predicate': 'true',
 'cljs-destructure-trailing-map-destructuring': 'true',
 'cljs-exceptions-throwable-map': 'true',
 'cljs-extension-test-414': 'true',
 'cljs-extension-test-cljs-2974': 'true',
 'cljs-extra-core-js-invoke-test': 'true',
 'cljs-extra-core-test-cljs-3400': 'true',
 'cljs-extra-primitives-test-496': 'true',
 'cljs-extra-primitives-test-arithmetic': 'true',
 'cljs-extra-primitives-test-misc': 'true',
 'cljs-extra-seqs-test-distinct': 'true',
 'cljs-extra-seqs-test-partitionv': 'true',
 'cljs-extra-seqs-test-partitionv-all': 'true',
 'cljs-final-seqs-iteration-seq-equals-reduce': 'true',
 'cljs-integration-test-run-bang': 'true',
 'cljs-interop-core-test-memfn-test': 'true',
 'cljs-interop-primitives-test-test-arrays': 'true',
 'cljs-printing-completion-test-print-knobs': 'true',
 'cljs-protocol-test-extend-to-object': 'true',
 'cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests': '[true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true '
                                                                        'true true true true true]',
 'cljs-residual-completion-cljs-2866-test': 'true',
 'cljs-residual-source-context-apply': 'true',
 'cljs-state-test-atoms-and-volatile': 'true',
 'cljs-transients-collections-test-test-transient-edge-case-2': 'true',
 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aget-test': 'true',
 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aset-test': 'true',
 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-aget-test': 'true',
 'cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-set-test': 'true'}


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
    for required in ['compile_namespace_with_macros', 'MacroHost::new()', 'defmacro arity-witness [] {invoke}', 'pr-str {invoke}', 'defmacro arity-witness [] (do {source})', 'fresh', 'reused', 'arity-after185-artifacts', 'from_secs(120)']:
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
        expected_value = {'$vector': [True]*165} if reviewed['id'] == 'cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests' else True
        if case['expected'] != {'id': case['id'], 'outcome': {'status': 'returned', 'value': expected_value}, 'trace': [], 'stdout': ''}:
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
    if signature and signature['kind'] in {'fixed','variadic'} and not any(call['argument_count'] == signature['minimum'] if signature['kind']=='fixed' else call['argument_count']>signature['minimum'] for call in calls):
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
    return {'obligation-witnesses.jsonl': ordinary, 'phase-witnesses.jsonl': phases, 'arity-after185-call-receipts.jsonl': receipts}


def parse_frames(output):
    expected = [(row['id'], host, phase, engine) for row in rows() for host in ['fresh', 'reused'] for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
    frames = []
    for line in output.splitlines():
        if not line.startswith('arity-after185-observation\t'):
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
