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
HELPER = 'tests/reference/compiler-language/arity_after172.py'
DRIVER = 'tests/compiler_arity_after172.rs'
TEST = 'complete_arity_oracles_execute_in_actual_phases_and_lifecycles'
SCANNER = 'tests/reference/compiler-language/arity_resume_candidates.py'
SCANNER_SHA256 = '31457848fe6d2f08e92b3ad5b4a6f99362a97be66e4819c9d45cf3a1b9ecbcc3'
DEMAND = {
 'arity-resume-core-factory-multifn': 'All genuine MultiFn receivers are called and their default dispatch values are compared to the original :default literal; function-valued dispatch-fn identity controls supply zero credit.',
 'arity-resume-core-factory-body-core-test-test-transducers': 'Every original eduction form is consumed by the complete transducer/reducer workflow and its independent result/effect assertions, including all original three eduction invocation counts.',
 'cljs-sequence-completion-src-test-cljs-cljs-interop-test-test-es6-interfaces': 'The actual ES6 iterator is consumed through three next values and the final done flag, retaining every other original map/set interface loop.',
 'cljs-utilities-completion-src-test-cljs-cljs-parse-test-test-parse-uuid': 'The actual random UUID value is stringified and parsed successfully. Its generated text must satisfy the original UUID format parser; every original invalid-type, bogus-string, zero UUID and explicit UUID version control remains.',
 'cljs-residual-body-0': 'Both halt-when forms are consumed by the original into/transducer calls, retaining the literal anomaly and full partial-results map.',
 'cljs-residual-body-2': 'The original reader-produced JS arrays/objects are inspected by array?, full sequences/lengths, literal complete property-key sets, nested keys and values, preserving all original reader inputs.',
 'cljs-residual-body-10': 'The zero-argument distinct transducer is consumed by the original transduce and compared to [1 2 3]; original interpose and indexed mapping controls remain.',
 'cljs-multimethods-test-multimethods-2': 'The complete unchanged run function demands the actual MultiFn method table and both removals through original two-to-one-to-zero counts; all original setup, nested dispatch, apply, hierarchy and error paths remain.',
 'arity-resume-public-constructor-body-collections-test-test-vectors': 'The entire original vector workflow actually expands its threading macros and successful assert checks; literal nested vector, metadata, subvec, lookup/reduction and caught invalid-index controls remain.',
 'arity-resume-public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds': 'The original complete predicate table is evaluated through every row and every dotimes index; the original two-argument assert checks the actual predicate result against each independent table boolean.',
 'cljs-sorted-seqs-test-test-empy-and-seq': 'The original three-argument if-not selects the demanded lazy metadata literal with LITE_MODE false; every remaining original empty/sorted comparator/metadata collection control stays present.',
 'arity-resume-core-factory-body-core-test-test-multimethods-1': 'The fn macro produces the actual two-argument dispatch function, demanded by competing multimethod calls and the literal :rect-shape result; the complete original derive setup and apply call remain.',
}
LIBRARY_DEMAND = {
 'walk': 'The complete original walk body runs every original recursive callback, replacement/map-entry/metadata/order comparison and independent literal. No callback declaration alone is credited.',
 'set': 'The complete original set body demands the exact selected operation in every original result comparison, preserving variadic/empty inputs, full relation/map values, metadata and any original loops.',
 'string': 'The complete original API body demands every selected string operation against original string/index/boolean literals, preserving Unicode, regex and callback inputs, all options and every original assertion.',
}


REVIEWED = [{'case_sha256': 'da8372ca9e6ca9a8e4c35dc78370c2add2129180670dc4af998ecd2fc3db23fd',
  'fixture': 'tests/fixtures/arity-after172/arity-resume-core-factory-multifn.cljs',
  'id': 'arity-resume-core-factory-multifn',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '1ce4500d472c752bb9a52dc982c69278c31ea7af712e879692bd2b45c30b4201'},
 {'case_sha256': '250f2d24ce5192a253eee55091ea7ae7a8647a9bf2b49a5b13c11b89a754408f',
  'fixture': 'tests/fixtures/arity-after172/arity-resume-core-factory-body-core-test-test-transducers.cljs',
  'id': 'arity-resume-core-factory-body-core-test-test-transducers',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '7d0cd6f86c0e98272a0b54cfa8348826d6e4ce6c9c0f08134261d327485a21a3'},
 {'case_sha256': 'e09fdad95965a22a11301191307a2a34778f33e073448d608b9e95ec080106de',
  'fixture': 'tests/fixtures/arity-after172/cljs-sequence-completion-src-test-cljs-cljs-interop-test-test-es6-interfaces.cljs',
  'id': 'cljs-sequence-completion-src-test-cljs-cljs-interop-test-test-es6-interfaces',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'cafc831ea4f452edf0eea393c9595024b650a01d7dd3aab5e4fed56b0e06ea1a'},
 {'case_sha256': '340f893c2e4e41f4292cfd3dccdbfa36643ad7e13d37a27beec65a70c5bfe4c4',
  'fixture': 'tests/fixtures/arity-after172/cljs-utilities-completion-src-test-cljs-cljs-parse-test-test-parse-uuid.cljs',
  'id': 'cljs-utilities-completion-src-test-cljs-cljs-parse-test-test-parse-uuid',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '8bd8de50fcbbab912f36791195dc404e6d0adeabca1d8534b240b71ba221559b'},
 {'case_sha256': 'e8a0fc364043ad08e4f50e3e386c8094ab8da33721a0d7803af1c87a769c60f0',
  'fixture': 'tests/fixtures/arity-after172/cljs-residual-body-0.cljs',
  'id': 'cljs-residual-body-0',
  'invoke': '(run)',
  'namespace': 'residualbody.case0',
  'source_sha256': '13538db70e6be086d08a53fc5167af9f7bf2871aed6cb309e1a8244159fce31b'},
 {'case_sha256': '635169ef8b920b07d92d9a014064b4057422de40dcc48d957bef2a69a4919988',
  'fixture': 'tests/fixtures/arity-after172/cljs-residual-body-2.cljs',
  'id': 'cljs-residual-body-2',
  'invoke': '(run)',
  'namespace': 'residualbody.case2',
  'source_sha256': '0fa6f8a252b048e9e1d1ab234d54a2455f5031507d5534ecf08dd883648d9da5'},
 {'case_sha256': '038e4ddefe022aa8363ddde362fe05ff026f302b7a3289d5bf2e36241642f4a2',
  'fixture': 'tests/fixtures/arity-after172/cljs-residual-body-10.cljs',
  'id': 'cljs-residual-body-10',
  'invoke': '(run)',
  'namespace': 'residualbody.case10',
  'source_sha256': '3efbe25ef81a1e09c319307353bd6bbca02ebed667708f98ed29e344c37abb53'},
 {'case_sha256': 'e467aa08e90992504fabaaaa9fb82e84b67c30625d3cc53f29a2e484749cbfbc',
  'fixture': 'tests/fixtures/arity-after172/cljs-multimethods-test-multimethods-2.cljs',
  'id': 'cljs-multimethods-test-multimethods-2',
  'invoke': '(run nil)',
  'namespace': 'cljs.core-test',
  'source_sha256': 'f24b9e3cb5fe28abf22c62280cc65996ab88fadf95f009a2899afe0b4b5fef42'},
 {'case_sha256': '878958090ff1fe16d65d9767238269c50e6b0ed6f81edfba3efea305987d3521',
  'fixture': 'tests/fixtures/arity-after172/arity-resume-public-constructor-body-collections-test-test-vectors.cljs',
  'id': 'arity-resume-public-constructor-body-collections-test-test-vectors',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': 'fdd8130f5659c2079a423c329eeca4d7e4c801bbe97f7bd72cb3a3c9ba0050a9'},
 {'case_sha256': 'dc6fd4fa5f18bcf253b937ba6f2e767cf5a07d1142d8b29f15971ba27232a1ed',
  'fixture': 'tests/fixtures/arity-after172/arity-resume-public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds.cljs',
  'id': 'arity-resume-public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '41312406a04718f83766e075768d62ae3724ab9818fb15ca5b69cba80d1ff4eb'},
 {'case_sha256': 'a1b5360a8dc2b61a8a91c21a915435d2a1248bd1758f9b5849c2253a2d3886c1',
  'fixture': 'tests/fixtures/arity-after172/cljs-sorted-seqs-test-test-empy-and-seq.cljs',
  'id': 'cljs-sorted-seqs-test-test-empy-and-seq',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '22325e31e2a373af25da2ccf52ba80cec92ce170474b9194a4e4c4ba30c48ff3'},
 {'case_sha256': 'e247b06e1feb34fe1bda007e44e0e0215b52245304af308df10b80caeb1186ce',
  'fixture': 'tests/fixtures/arity-after172/arity-resume-core-factory-body-core-test-test-multimethods-1.cljs',
  'id': 'arity-resume-core-factory-body-core-test-test-multimethods-1',
  'invoke': None,
  'namespace': 'user',
  'source_sha256': '85aa73a4ae8299a5d4501ec9b733f2004aab15d4a558f391379fd81208724a8d'},
 {'case_sha256': '8cf2c8bcc39d9fc4c6319d762a30bf7a20013bc8cb782c5a0fc14897fc36aa38',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-replace.cljs',
  'id': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-replace',
  'invoke': '(run)',
  'namespace': 'librarybody.case0',
  'source_sha256': 'f2295d97c9251888471333668f466e5640ab9c025e8d30e86645afe8929fbb6e'},
 {'case_sha256': '479bc7ca0d29b2b4481609d549d81db9122709fe0d70e7a1cc591ef944ba7fa9',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-replace.cljs',
  'id': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-replace',
  'invoke': '(run)',
  'namespace': 'librarybody.case1',
  'source_sha256': 'ed5f902f9b339d5a1d548b9b6a9339936ea1fba00f26c8b3dfb861a373494022'},
 {'case_sha256': '9b988f9019f488356660e04e8734e9f20db875d4ae1e9c59630d98ec3fdf9b9e',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-stringify-keys.cljs',
  'id': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-stringify-keys',
  'invoke': '(run)',
  'namespace': 'librarybody.case2',
  'source_sha256': '926f5c5a0ad4bf06b90419253adb2009cd2aeb9cc541ef97b0a839d348755cac'},
 {'case_sha256': '7cbbbed6b6e294e84316700a60b0f9cb99ea7aeedb4c6edb813e62ab398ee42e',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-order.cljs',
  'id': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-order',
  'invoke': '(run)',
  'namespace': 'librarybody.case3',
  'source_sha256': '41dcb2fe2023026679bbe7a1c52f559781c57bfdf56ced32fb7e7103fd249ff6'},
 {'case_sha256': 'bb3492039a2d8c89e39304ea7685c11da59dbf9c916272bce2479864a322e01f',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-order.cljs',
  'id': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-order',
  'invoke': '(run)',
  'namespace': 'librarybody.case4',
  'source_sha256': '3262ce0f762cef7eb67fa53b17e0849b9792cf909c78aff06a0e6f3df1a330b4'},
 {'case_sha256': 'a203d43e465e3fd366d93a8e91bdd5fded4ccf4bb095fca2bc2c71cf841107d4',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-test-walk.cljs',
  'id': 'cljs-library-body-src-test-cljs-cljs-walk-test-test-walk',
  'invoke': '(run)',
  'namespace': 'librarybody.case5',
  'source_sha256': 'ce49d480768fec22e695d9290cf659fe33dfefcaa5fd65c95216898811c6c565'},
 {'case_sha256': '9553db2aae563bfcb55944397475402d55a2dc10690dd3f0b5e3805d6345486e',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-union.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-union',
  'invoke': '(run)',
  'namespace': 'librarybody.case8',
  'source_sha256': '4a4002f964c54333309a28a843e31d8fa142b80579c8e846456b8cb4d408eb48'},
 {'case_sha256': '98c552eeb9cfdcfb645bc86f52c2742a94a845e03cfb4f2c20e09c479f0f26f6',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-intersection.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-intersection',
  'invoke': '(run)',
  'namespace': 'librarybody.case9',
  'source_sha256': '5f49f608706b17fbf99e5ec0211cd922d393f0b6b611234449a9a90647fe7254'},
 {'case_sha256': 'fe5efd78d4ffa91c0b962e0fb1f176d3ec26a63861abe3a8795d57341ab84372',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-difference.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-difference',
  'invoke': '(run)',
  'namespace': 'librarybody.case10',
  'source_sha256': '2fe7225c50a40d3f0befe6e05010eab78632335873a5fc5dbc3b3d02952fcfb7'},
 {'case_sha256': 'a77a518c5c7fee156ce0bb86c88a0684e7bdd619bcb99a7a3454b823e25ab303',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-select.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-select',
  'invoke': '(run)',
  'namespace': 'librarybody.case11',
  'source_sha256': '9b23969fece337fad48175e8ecb2b19c90ecd3adb6b9e7d1756a67d97356d61c'},
 {'case_sha256': 'd13ad0d389580c997196d627bfe19d68c49bf2b42a3515eb3c523589b68cf753',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-project.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-project',
  'invoke': '(run)',
  'namespace': 'librarybody.case12',
  'source_sha256': '29894f273516e9ed8a2de9c4ad03d81802825e1d2b335097d68d5f646fa472ba'},
 {'case_sha256': '17153603b067386e105cf0014d264cd9b6d9bc4d9a8fb79070ad97827358875e',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-rename-keys.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-rename-keys',
  'invoke': '(run)',
  'namespace': 'librarybody.case14',
  'source_sha256': 'abe0f0aeab0f322445561d330503db7a161544ba19e29c1b04315e8cbae907a2'},
 {'case_sha256': '2c8854352eac2de36dc5c856a58cb920f3ea77a304bee1262aba6ea83fa96691',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-index.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-index',
  'invoke': '(run)',
  'namespace': 'librarybody.case15',
  'source_sha256': '3e60d346a8f292ca872749355126acdb67058c3de077c920469fffade8720370'},
 {'case_sha256': '43e8a125ac683dfc871f61de4a348d22108a4bf5064f77fa9303a9ffb3f371d1',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-join.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-join',
  'invoke': '(run)',
  'namespace': 'librarybody.case16',
  'source_sha256': 'b6e8cb4f0291b3c658af8cdbaaba87728ec8b23325a960ad7cdd05ab8ff86cdb'},
 {'case_sha256': 'daa70a45b3775ad7535294487461b0047d0a48676e61746166dc2426b4b13dff',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-subset-p.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-subset-p',
  'invoke': '(run)',
  'namespace': 'librarybody.case18',
  'source_sha256': '472b9d035533b2343210a978957607c3e6674d1d0e7ef52411065548010194cc'},
 {'case_sha256': '2ad2798558f1f0353bdf6c018867cdb83ebb7fbbb851b60bd4fdf12b9371f285',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-superset-p.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-set-test-test-superset-p',
  'invoke': '(run)',
  'namespace': 'librarybody.case19',
  'source_sha256': '4fa5adb0353e5f823a6582aecc7ff7d821ec935d867f26b4dca2006fcd6edb34'},
 {'case_sha256': '91b4313359c5d4f19b36c878e3dd28a930d402a5b795d00cc1422a0c118c4c1a',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-string-test-test-api.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api',
  'invoke': '(run)',
  'namespace': 'librarybody.case20',
  'source_sha256': '8491e711260927f5b4854bc5fb2e342fc5da8a29c4d039be7376811289954bf3'},
 {'case_sha256': '1d8df9ce61959427364581d3082611538049ebdb1a223e92835462456817efab',
  'fixture': 'tests/fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-walk-test-test-map-entry.cljs',
  'id': 'cljs-library-body-src-test-cljs-clojure-walk-test-test-map-entry',
  'invoke': '(run)',
  'namespace': 'librarybody.case23',
  'source_sha256': '1ea892161af3dcea287e260f5ed8a9255727641e69be03d287b49ec967950333'}]
SELECTION = [{'body_sha256': '2f2e9cb295526be38a9f2e97c03a1ba35935306cc77e4c1ac0fc2c1996ca83cd',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::default-dispatch-val',
  'signature_sha256': None,
  'workflow': 'arity-resume-core-factory-multifn'},
 {'body_sha256': '175bf8cae82939fb4e5cd39afdeb8fa3e16ccfba9d52a3bf8e47b761a1cd9a5d',
  'direct_arities': [1, 2, 3],
  'obligation_id': 'local::src/core_var_metadata.tsv::eduction',
  'signature_sha256': None,
  'workflow': 'arity-resume-core-factory-body-core-test-test-transducers'},
 {'body_sha256': '38f47080bdd25c1fd31ac1892e734b64507b53a3c0f6f2b782ba84f4e4486ab7',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::es6-iterator',
  'signature_sha256': None,
  'workflow': 'cljs-sequence-completion-src-test-cljs-cljs-interop-test-test-es6-interfaces'},
 {'body_sha256': '72e58ac1ca81881deb3b5c34fb78f60c6b4b691ec28b3c731b8865d8002a89e4',
  'direct_arities': [0],
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::random-uuid::arity-1',
  'signature_sha256': 'c04963f87d82d63dd1d5239f487499f7c8f6ac7f34ad0470e03c0c5f4f28a5ec',
  'workflow': 'cljs-utilities-completion-src-test-cljs-cljs-parse-test-test-parse-uuid'},
 {'body_sha256': '314d8656dfadeb90c1c500904f27c73d5abb70c45c29c5f07eecdbaa125e9622',
  'direct_arities': [0],
  'obligation_id': 'local::src/core_var_metadata.tsv::random-uuid',
  'signature_sha256': None,
  'workflow': 'cljs-utilities-completion-src-test-cljs-cljs-parse-test-test-parse-uuid'},
 {'body_sha256': 'e1d923a34d1582525a13521fcd73576d6cee4657f731e981cbcaab4573b6caf8',
  'direct_arities': [1, 2],
  'obligation_id': 'local::src/core_var_metadata.tsv::halt-when',
  'signature_sha256': None,
  'workflow': 'cljs-residual-body-0'},
 {'body_sha256': '117647ee58080b0094aab0a9beb2f44ebbf903b3aa5a6dbcbeb28852b24d29bd',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::array?',
  'signature_sha256': None,
  'workflow': 'cljs-residual-body-2'},
 {'body_sha256': '6c122b762913000240ba37448f91c3087585d5d7133b511c34d4760be491f4eb',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::js-keys',
  'signature_sha256': None,
  'workflow': 'cljs-residual-body-2'},
 {'body_sha256': '1121d8d67a42ad8927fc67c641fc4c96970d010bd4fa60238f815bdaeee47bad',
  'direct_arities': [0],
  'obligation_id': 'local::src/core_var_metadata.tsv::distinct',
  'signature_sha256': None,
  'workflow': 'cljs-residual-body-10'},
 {'body_sha256': '256b0299494270eeb4f40803b242431b9b23b06f8b3e1b92dd282061ff086eb5',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::methods',
  'signature_sha256': None,
  'workflow': 'cljs-multimethods-test-multimethods-2'},
 {'body_sha256': '4c9b58ae32d37d32eb504e9471fd83d90e02ddd3c9de04d6f2b8845451b7ff11',
  'direct_arities': [1],
  'obligation_id': 'local::src/core_var_metadata.tsv::remove-all-methods',
  'signature_sha256': None,
  'workflow': 'cljs-multimethods-test-multimethods-2'},
 {'body_sha256': '73f33b0fb849ec956086b7bf20f334fbb18172bafbf8cc39228e38d163922e82',
  'direct_arities': [2],
  'obligation_id': 'local::src/core_var_metadata.tsv::remove-method',
  'signature_sha256': None,
  'workflow': 'cljs-multimethods-test-multimethods-2'},
 {'body_sha256': '1520b2c67fec285b52db7749ee67dee1c1b4eb76af1b0fd36f6233bceec96f49',
  'direct_arities': [3, 5],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::->~2::arity-1',
  'signature_sha256': 'c84b54d05b6efefdcdb0e2c625c39581ed4be384969569977453aff6743bf255',
  'workflow': 'arity-resume-public-constructor-body-collections-test-test-vectors'},
 {'body_sha256': '0a7af66abda28dbc923ad7c1930ce3eb6ae0ccb4d7bcfa578a79ea97757c982d',
  'direct_arities': [3],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::->>~2::arity-1',
  'signature_sha256': 'c84b54d05b6efefdcdb0e2c625c39581ed4be384969569977453aff6743bf255',
  'workflow': 'arity-resume-public-constructor-body-collections-test-test-vectors'},
 {'body_sha256': '4389f4872974107fe51446e2f3b6b8f2e3d456bbc408b1b3764d1aaf1a180958',
  'direct_arities': [1],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::assert~2::arity-1',
  'signature_sha256': '57825001ab734a2f9387d427d94f3d4486d027e222ba0ff2737b638c9e9921c6',
  'workflow': 'arity-resume-public-constructor-body-collections-test-test-vectors'},
 {'body_sha256': '4389f4872974107fe51446e2f3b6b8f2e3d456bbc408b1b3764d1aaf1a180958',
  'direct_arities': [2],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::assert~2::arity-2',
  'signature_sha256': 'd583f1f6c751bc82ca3a45dd54e9aff02f3e45e63bc88bf5b2de2e97c74189e4',
  'workflow': 'arity-resume-public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds'},
 {'body_sha256': '92f0fe82899358d196b4483771d04480106fcee69a21e7058c7b39d9d128b4e2',
  'direct_arities': [2],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::dotimes::arity-1',
  'signature_sha256': 'bbce2142210ff6d05831b75efcba8523d8c57f11c137ed69448700c494e521ad',
  'workflow': 'arity-resume-public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds'},
 {'body_sha256': 'a3f9616143f42980c3510e8216faac0d309800f31747ab005277b553cf871fa9',
  'direct_arities': [3],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::if-not~2::arity-2',
  'signature_sha256': '87edca2d8fef4fb2068fe874b2a7f778a8b263eef17a0f7156e63f9bbb0a36af',
  'workflow': 'cljs-sorted-seqs-test-test-empy-and-seq'},
 {'body_sha256': '3b84c7ad4da2831f5da7b587670eba342673f8f8d2ddc5b51670bef833d1b7e8',
  'direct_arities': [2],
  'obligation_id': 'api::src/main/clojure/cljs/core.cljc::fn~2::arity-1',
  'signature_sha256': '3eda0d6e22fa067c6d0f861a22064b49abf4617abc9dfe89a18ccac731d8d328',
  'workflow': 'arity-resume-core-factory-body-core-test-test-multimethods-1'},
 {'body_sha256': '3cf780f5480965927f8386ab682d4179d058c823e41c2a041e5f539a130ab61c',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.walk/prewalk-replace',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-replace'},
 {'body_sha256': '523e5ab655ee7dfbc7ae0e94a10de76b9f0b1f9380659b92dce4423284be82e6',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.walk/postwalk-replace',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-replace'},
 {'body_sha256': 'f66f8557faaf70fc3bdb6b9dbdc8ae333bea78dd34e9cd8dc604f61ec4020799',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.walk/stringify-keys',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-stringify-keys'},
 {'body_sha256': '4b429caa6a01ca1b21831d7c699bfc10fdb0645f72dedbd40ae5df005cbd45e7',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.walk/prewalk',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-order'},
 {'body_sha256': 'dbe3e1d83126510ed6a923c2aa2a02f54f4ee3413d254c93f64c427300ebe1c1',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.walk/postwalk',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-order'},
 {'body_sha256': 'ae5ec7dfb601c9975aff44ad2d4ea641821512c5a64b24fa0ee416b0e217fa84',
  'direct_arities': [3],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.walk/walk',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-cljs-walk-test-test-walk'},
 {'body_sha256': '72d6de7f22525bb412c52e86f31fed3655dbbbba7f919c958ee41e8fec10b817',
  'direct_arities': [0, 1, 2, 3, 6, 8],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/union',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-union'},
 {'body_sha256': '8cb48ddcafce4dcc5658978d28c649aaad4a353dc24e92521b51bc2500e9156e',
  'direct_arities': [1, 2, 3, 4],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/intersection',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-intersection'},
 {'body_sha256': '02e122af3356c5dace7b57216a81fa426d85ac69a06abc92853a7398e1fa870d',
  'direct_arities': [1, 2, 3],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/difference',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-difference'},
 {'body_sha256': 'f78c5df4f245eb5690f049fa7af093c58786fe86d46b70d925d915c49a5827f5',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/select',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-select'},
 {'body_sha256': '721cce078468b54842fc3f2c090ee248b24c4a686d8ad05b833518e22ac2368d',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/project',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-project'},
 {'body_sha256': '534bd48ed81d503f08e8720fe63174f7d8cfb04f30aa060b6ed14e1dcec2fb21',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/rename-keys',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-rename-keys'},
 {'body_sha256': 'bf7d91887ddc67e3424fbd2cb0ee4d1795f1fef5c9deeb3fe6c602edaeece042',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/index',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-index'},
 {'body_sha256': 'fbb7cab5993d6810955898e81c92a335109d7eb462179c7ab5fe49c140c9bb9c',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/join',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-join'},
 {'body_sha256': '2ffb0d1e6df1ec7a1f349b65af886287dcf7471adcc526c9aaa44821b0cd7f0f',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/subset?',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-subset-p'},
 {'body_sha256': 'ad281ef79382522dbe93809fdc46082247ae261546d218f8f4d1fd08da1efe4f',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.set/superset?',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-set-test-test-superset-p'},
 {'body_sha256': 'c9eb8d9c8009cc4ccc39de20b6ff0fdf073f1aaffcf94c0ac2d369992aecb655',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/blank?',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '5a018f23eb49b05b828b5b706cd9567651e192ec3bebfcfad8d8bcf88f7ded95',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/capitalize',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '949e39e1b3936025b17bf481deef860e1fb3813a0b024c8f7baf56b6e64f9b39',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/ends-with?',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '161818fcbf1837fc380e916b592db839b6034a82a28eed8ad58306088fb6298f',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/escape',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': 'f73e0fcf59bed6306a37c42516eca5b6e5d19bcaa822f6f62e4449d1a06dab11',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/includes?',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': 'e27e4627fdcb10ad9b43b35628be9cb2479ea9ee7b8caa63227a8f7da6d02259',
  'direct_arities': [2, 3],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/index-of',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '35f5178434d55af68dc49b121802bca3468761376c46c61f6afb44cb786789ca',
  'direct_arities': [1, 2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/join',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': 'd3259941a26675bef75f73e632de9a79ed5287b172ccb8306a3cad50e394f0c5',
  'direct_arities': [2, 3],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/last-index-of',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '7e3cc1f2736e010d6e6ba82de714cd5c7984545ac2a62ce66047939a70f7b5b3',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/lower-case',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': 'b50f05c603925841489241e848e7058099b51755eb404aeaf6e2b6dcdbc0fdb4',
  'direct_arities': [3],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/replace',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '473df45d32ef8cbc55f3a969c460a3aacadf80ed216fa231a2b17976b310a4bf',
  'direct_arities': [3],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/replace-first',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '5a55a9bfa1e186756c41ff4c17195b032269d00078a1b54266c210088d4ac6b4',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/reverse',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '6a3863f1ddf0888588f95ad45d975ab39edad9a1b001502d31bf74b7345c8ba1',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/split-lines',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': 'bef2c02431937b3c744221e6759ff77ed983ab33bf28801410f890ba0f029685',
  'direct_arities': [2],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/starts-with?',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': 'ff9d28879462be3f68791e943862242e2280a9607aced6da6eff9a7415a1f823',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/trim',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': 'b9942d26060dbcc055648df761d971ff861e1c9010c91da27b7019fa9f6634c1',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/trim-newline',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '9e330188b844f5a63ea322351221d8e11b7ca9a5e4d1273de9fd15c70a98e712',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/triml',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '1dc56126b30c8193b9e8b7832f904c9e19332b9fb2115fba95eab81c62283c52',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/trimr',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': 'e493f41cd6949458f1a0bb3c74c4cc1e37f89b98193f86ae780751087d8631d7',
  'direct_arities': [1],
  'obligation_id': 'local::tests/reference/sci/historical/supported-apis.tsv::clojure.string/upper-case',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-string-test-test-api'},
 {'body_sha256': '3873948b6575cf990d4794cc91bf721481697049b7b084e02bdd9b5170e45a23',
  'direct_arities': [3],
  'obligation_id': 'api::src/main/cljs/cljs/core.cljs::->MapEntry::arity-1',
  'signature_sha256': '330c286c454639a6aa17ba82cbd47d45d1e73f0e6ea9b1c56616c1906bc91d97',
  'workflow': 'cljs-library-body-src-test-cljs-clojure-walk-test-test-map-entry'},
 {'body_sha256': 'e8818d02015bcb093ebe9c347af9b71b3a8dcc90ba3048b34716e710754ca64b',
  'direct_arities': [3],
  'obligation_id': 'local::src/core_var_metadata.tsv::->MapEntry',
  'signature_sha256': None,
  'workflow': 'cljs-library-body-src-test-cljs-clojure-walk-test-test-map-entry'}]



def demand_basis(row):
    if row['id'] in DEMAND:
        return DEMAND[row['id']]
    if row['id'].startswith('cljs-library-body-'):
        family = 'string' if '-string-test-' in row['id'] else 'set' if '-set-test-' in row['id'] else 'walk'
        return LIBRARY_DEMAND[family]
    raise ValueError('unreviewed complete oracle demand basis')


def rows():
    if digest((ROOT / SCANNER).read_bytes()) != SCANNER_SHA256:
        raise ValueError('reviewed structural scanner drift')
    catalog = {row['id']: row for row in map(json.loads, (HERE / 'full-cases.jsonl').read_text().splitlines())}
    driver = (ROOT / DRIVER).read_text()
    if not re.search(r'#\[test\]\s*fn\s+' + TEST + r'\s*\(', driver):
        raise ValueError('detached actual phase driver')
    for required in ['compile_namespace_with_macros', 'MacroHost::new()', 'defmacro arity-witness [] {invoke}', 'pr-str {invoke}', 'defmacro arity-witness [] {source}', 'fresh', 'reused']:
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
        basis = demand_basis(row)
        rationale = (basis + ' The entire unchanged original oracle, setup, body, loops, literals, effects and errors execute through the exact driver in fresh/reused actual MacroHost applications and macro computations, observed by QuickJS and Node. Both required public pins execute the exact same existing oracle. Declaration, source registration, function identity and undemanded callback paths provide no credit. Local mappings cross-check the operation, with no complete metadata-value claim.')
        common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'], 'rationale': rationale}
        if signature:
            common['signature_sha256'] = selected['signature_sha256']
        ordinary.append({**common, 'assertion_ids': [], 'local_execution': execution, 'reference_fixture_id': row['id'], 'reference_source_sha256': digest(row['case']['source']), 'reference_contract_sha256': digest(json.dumps(row['case']['expected'], sort_keys=True))})
        phases.append({**common, **execution, 'support_source_sha256': {path: digest((ROOT / path).read_bytes()) for path in [HELPER, SCANNER, row['fixture']]}})
        receipts.append({**selected, 'direct_calls': calls, 'whole_source_sha256': row['source_sha256'], 'original_case_sha256': row['case_sha256'], 'demand_basis': basis, 'invocation': row['invoke'] or 'complete expression', 'oracle_id': row['id']})
    return {'obligation-witnesses.jsonl': ordinary, 'phase-witnesses.jsonl': phases, 'arity-after172-call-receipts.jsonl': receipts}


def parse_frames(output):
    expected = [(row['id'], host, phase, engine) for row in rows() for host in ['fresh', 'reused'] for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node']]
    frames = []
    for line in output.splitlines():
        if not line.startswith('arity-after172-observation\t'):
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
