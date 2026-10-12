"""Whole original SCI sources bind advertised local APIs to fresh execution.

This exact reviewed cohort preserves every original outcome/effect literal.
Qualified aliases are additional controls, never replacements for body execution.
No stored receipt grants coverage. The parent coordinates exported shared rows.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import os
import sci_execution as sci
import sci_source_context as context
from extract_inventory import Reader, head
ROOT = sci.ROOT
HERE = sci.HERE
FIXTURE = 'tests/fixtures/compiler-local-api-source-context.tsv'
DRIVER = 'tests/compiler_local_api_source_context.rs'
TESTS = ('original_local_api_sources_001_through_040_execute_in_both_phases',
         'original_local_api_sources_041_through_080_execute_in_both_phases',
         'original_local_api_sources_081_through_121_execute_in_both_phases')
PROFILE = 'local-api-original-sci-v1'
REVIEWED = [('sci-iteration-for-basic', ['clojure.core/*'], '1e1de9912c9a4766c6bd92cce3ab6a81cf8658a6c8f10e2d4749d9c313fd4923'), ('sci-vector', ['clojure.core/+'], '215b01cfdde30bc3d165e4ada2aadeb6e4c38538fdfa1d6349836b856098b915'), ('sci-seq-reduce-order', ['clojure.core/-'], '186e8f5e964f9d3283c6b08f10118b342bb58dd0ff98a9847c689b5b5bc5b4d4'), ('sci-lazy-infinite-takewhile', ['clojure.core/<'], '7c1f2763c82f3d028872104b083c38bef9e0db3758faa5f7c62714ad8187d8db'), ('sci-native-short-types', ['clojure.core/==', 'clojure.core/max', 'clojure.core/min'], '6f0dcfbb3cb82f5c13d56090e2a6b65e784e976af50b122859b64695923667aa'), ('sci-binding-pre-success', ['clojure.core/>'], '20bb86130b91fb01113b257785e4754bd5998841948deec422d40cef6b3a2b49'), ('sci-binding-pre-recur', ['clojure.core/>='], '4fcc3d1f98f381e559dc2dbc9ce76453801f6201a0d74628f5e1acf8febb0210'), ('sci-state-watch-self-remove', ['clojure.core/add-watch'], 'd2664050d4527c373ab7c7299e8f93a047b42b2532401eda9912bb676d531254'), ('sci-deferred-apply-fixed', ['clojure.core/apply'], '305500833f45f7e69fb5a074bc7dc4efdd37d242bc066c6a790a9f881b453296'), ('sci-coverage-array-map', ['clojure.core/array-map'], '53a390afab4cd656d76f4bf9caa466b09961b3f9c14b3035b83e66a343c05bae'), ('sci-coll-assoc-nil', ['clojure.core/assoc'], 'f9c8f78ba8a2cb8370da99bcc8e64968efae1aa7fc058829f9d3034757759a4b'), ('sci-coll-assoc-in-value', ['clojure.core/assoc-in'], 'cfa6a06f0661917e8f7271fde5e469bc91b3f20e456023d3add5e3b2faa69e30'), ('sci-state-atom-meta', ['clojure.core/atom'], '3395e668bdea40dde854a473b3bfa1b262a03eddbbfc107595f204e115d2aae6'), ('sci-native-general-predicates', ['clojure.core/boolean', 'clojure.core/boolean?', 'clojure.core/char?', 'clojure.core/false?', 'clojure.core/identity', 'clojure.core/keyword?', 'clojure.core/qualified-keyword?', 'clojure.core/qualified-symbol?', 'clojure.core/simple-keyword?', 'clojure.core/simple-symbol?', 'clojure.core/true?'], 'fdaa201cbb85da92122cf3c50a2a55dbde9f41c82628aa4277da687ec9a4af70'), ('sci-lazy-probe-butlast-construction', ['clojure.core/butlast'], '8b9e03e1533b6e6fc537083492b713a08edfbf7e5c1832b04f101829846db63a'), ('sci-seq-comp-many', ['clojure.core/comp'], 'ae937fce8de0f007fb3b5e03ed8ed625f425446a3e13dbb10aef5a60d4c1a3bc'), ('sci-seq-complement-truth', ['clojure.core/complement'], 'c03f7bb138c82f63ef71f4955a4aab5bb3836cc205648809bb22ee3751a65974'), ('sci-transduce-completing-direct', ['clojure.core/completing'], 'ec5e24be29a8b8e4fe0ddcc553534550cdda400a1741e5647e43c0ffbe7bce80'), ('sci-lazy-concat-zero', ['clojure.core/concat'], '05cf24f1016aad1bcbd84243dcb58b25ce5caa024047093b93a51390310363c3'), ('sci-coll-conj-entry-sequence', ['clojure.core/conj', 'clojure.core/list*'], '33020c13b605ecd24f0767c10fb837e49fddcda72595a07b9ca423e95eca42bf'), ('sci-seq-search-early-projection', ['clojure.core/cons', 'clojure.core/some'], '620133e561b491e96bb26c0d313d78687c1683dbba4ecaff8940d967270acfbe'), ('sci-contains-nil', ['clojure.core/contains?'], '46a60cd5304fc9747c689963f8cb136980ff10b21cbd6681a3da67a9fd0140f2'), ('sci-coll-count-nil', ['clojure.core/count'], 'bf4e73268c24c7142bcc38026f7ac6e16c5952bc983f98a313b854ce19ac763f'), ('sci-lazy-probe-cycle-empty', ['clojure.core/cycle', 'clojure.core/nil?'], '394a83230545fa01ab1706b2ed4849a44e817d4a776474e50a4c1ec1327a0280'), ('sci-loop-value-position', ['clojure.core/dec'], 'f1bb659913c51de96b7698851c190c228bd5be97e0336820fc3f2a20033b39aa'), ('sci-deferred-delay-predicates', ['clojure.core/delay?', 'clojure.core/ifn?'], '85fbe6d3395efb7f68701ca3615e8e684437aaf2faffdc0b43b8621dc8324c3a'), ('sci-coll-disj-nil', ['clojure.core/disj'], 'cc7943c89a7e0b560c489d25b9a7a90e603d5f14390f35b970e9e710ba7f8ac4'), ('sci-coll-dissoc-nil', ['clojure.core/dissoc'], '0fe887b0dc6b067b9b6a7025c795c78cd6771e4cf12f9e3dfe9aab5598a8e880'), ('sci-lazy-probe-doall-nil', ['clojure.core/doall'], 'b00e54f35c6ccc6c11b959cf98c93f090424ba3061047ecb4bd9df29a18c2626'), ('sci-lazy-probe-dorun-invalid-empty', ['clojure.core/dorun'], '2f3076499b6fef624805126c18192627694d5e56ba81ce6b40e559dc13f88471'), ('sci-xf-bounds-reuse-drop', ['clojure.core/drop'], '226cea2dafcd752fe030a43690b854e190fab30a4b03b11fd9427e27e2c6b9f1'), ('sci-lazy-infinite-dropwhile', ['clojure.core/drop-while'], '7a0eb570473c38159a92ea4073dbfb472a20fcfdf923a105b91b16872c4cd6e2'), ('sci-coll-empty-nil', ['clojure.core/empty'], '952eafefdf0832efb0deb239914c2b4219d9be79e99133b16f01d443a0e482a5'), ('sci-coll-empty-p-nil', ['clojure.core/empty?'], 'b68bc1183b98a4f3a771457ddfcd37124197b86a3935dfc47c5a1ee968ad5851'), ('sci-seq-ensure-reduced', ['clojure.core/ensure-reduced'], 'c8f700c432261fa36b58a5cae6b715e2011d6bbd15a17499b4622f62d1d266f5'), ('sci-iteration-for-upstream', ['clojure.core/even?'], '489b348c06228b8da906f9653be997fbad7e5850921edac6e76f0e9c727fe718'), ('sci-seq-every-effects', ['clojure.core/every?'], 'efa62494e66412a37395a82737cb7f6f6d4b3c8e8870f6cd0849c512be80407d'), ('sci-lazy-nested-first', ['clojure.core/ffirst', 'clojure.core/fnext', 'clojure.core/nfirst', 'clojure.core/nnext'], 'bb5697f4fdc12b3baa6a4ce0c54aa6c3bc26d4572b448d1a07e37798e4e2ee92'), ('sci-xf-basic-pipeline', ['clojure.core/filter'], 'fd1357855b8b7baced3c0e9eaf54897e6578629060f4361adfdf6a4d47aada33'), ('sci-deferred-lazy-filterv', ['clojure.core/filterv'], '6a802272125f432387e4b14ce763fdee632f44b6c0926a7eb5dbf000395326d1'), ('sci-coll-find-value', ['clojure.core/find'], '78681a0661aa9b1f7fa1926c9b563c38cdc79f95ccad39fdf5a80924f22578b1'), ('sci-syntax-quote-deferred-head', ['clojure.core/first'], '3d33b66b8784b4c06820921224ab489bc4c39cad3c00519ddd97f9e24fcbf33b'), ('sci-get-string', ['clojure.core/get'], 'fee95b57a717b9388cf4a06c773823fd3e1c12539b5ded1a229b5c08c493a45e'), ('sci-coll-projection-get-in-short-circuit', ['clojure.core/get-in'], 'fe983d8c51e3e0071b0aed5039d89f32b93c8c52b8338497afa39a43a4b9f101'), ('sci-coll-hash-map-value', ['clojure.core/hash-map'], 'a93300b39c0a5a8e76dcf4c3ff18b3aae036253748b8671913a30f53472daad2'), ('sci-coll-hash-set-value', ['clojure.core/hash-set'], '891aea5a75b057e4bfdcad5ddb706b1e48793bdd730fd96e01a705f9302313e3'), ('sci-lazy-doall-infinite-prefix', ['clojure.core/identical?'], '3ac0a69debb1b1ed49faa972b75d7bec5ce81e1d51c322849362c4d96296e2a3'), ('sci-coll-indexed-p-nil', ['clojure.core/indexed?'], '0405e32c939d8f981732375dc2d3be9cdb0bc4e3e7e7207ae10becc052d9e2a9'), ('sci-native-int-predicate', ['clojure.core/int?'], '8ef9ae13448422e2e0045eb800a883fb33a547008be1569766742d967656b8ff'), ('sci-lazy-probe-interleave-zero', ['clojure.core/interleave'], '6ba35672718286bbdc0e497a3727c6a0c3a653bf650aee58abd44dfb5204b6e9'), ('sci-lazy-probe-interpose-second-demand', ['clojure.core/interpose'], 'be6685a576758849ebc76577b8166b59284cfa8aafbe7cbef19a94f6bfdfa01e'), ('sci-into-xf-destinations', ['clojure.core/into'], '05806a8dd895a65984b3315c2bac4e603f6a4c089fead69716927e943b5bc6ef'), ('sci-lazy-iterate', ['clojure.core/iterate'], 'a80b79283f8d7d78eef696174884d3ecf4b8a906955da4440d33c302e1d19c94'), ('sci-seq-combinator-predicates', ['clojure.core/juxt'], '924c2be5ac74063ecc35c44971dae80629cb3218568972647c4e761fb24841f9'), ('sci-lazy-probe-keep-false', ['clojure.core/keep'], 'cad459fc5f99fbd141817db8245d001ad79bb9db4656c902e1fdaf7a533bce4f'), ('sci-lazy-keep-indexed', ['clojure.core/keep-indexed'], '846fd5f84c172cfa488a786264defd1f08f3ef55974a604c869d99aeed2ee067'), ('sci-coll-keys-value', ['clojure.core/keys'], '8cf223aba51d09e0d3ed0c75b76f5d962df31c34e08b8ff123a4ffd22ab8a3cc'), ('sci-string-keyword-basic', ['clojure.core/keyword'], '8b20671200fbd825439acc9a78dd130bc3004a394955f8b081cfbd8e418182db'), ('sci-lazy-last-butlast', ['clojure.core/last'], '7efaf53bf9b7f146fc1de41a1a773d544314bf7ef000f37dd5ce941829e323e9'), ('sci-coll-list-nil', ['clojure.core/list'], '5c9a041b543d30207a2717eec8cc6683639395d3c7e587f34efb1a6a0d36454e'), ('sci-xf-basic-map-direct', ['clojure.core/map'], '02ff35132a6909577080610155b35bbf02aeaa56ffa5990fc97c7287b9366fb9'), ('sci-coll-map-entry-p-nil', ['clojure.core/map-entry?'], '040e9618736c0b55933d5c3ff89329704915e832419ab850aedf014863776eca'), ('sci-lazy-map-indexed', ['clojure.core/map-indexed'], 'ea35033e3fd09938a3e61b334cc44f8f7b62e3e103c2a1c8ad01eda4400ca760'), ('sci-xf-cat-mapcat-effects', ['clojure.core/mapcat'], 'fb59f6955b960dc7dd574d4ba95a8b1ffea2969d2a5dedfec51243b5735554a9'), ('sci-coll-merge-value', ['clojure.core/merge'], '19a83c44a79fe0e39a81a2f5dd6cb95973df0b12aa33893b519a30627b65efc8'), ('sci-coll-merge-with-value', ['clojure.core/merge-with'], '848bbf4a8b08f487a7078160d438c940e6f0471e46fceed5704c1173416c4ad4'), ('sci-reader-meta-symbol', ['clojure.core/meta'], 'b8d239774cda27f91bc882e30552172b877cb61da97bd59f1483377c62f43b41'), ('sci-string-names', ['clojure.core/name', 'clojure.core/namespace'], '56e7214f02ba6bae94c75f47fd3fe319e1e828d95c2469f12b7d3dffb87ecf00'), ('sci-deferred-finite-rest', ['clojure.core/next'], '7d06e4718169af0ebe1b29fd3a770d3b16481f9c6e60f46e88b79ae260937b92'), ('sci-seq-not-any-effects', ['clojure.core/not-any?'], '7760c7ac4fffc2efa08f4d6897e7182c4b2ba4d1e7a485606215f3a6d112ee84'), ('sci-seq-not-every-effects', ['clojure.core/not-every?'], '89fe625248172fcb9ba3486a9536dcbef6e5affda952bb206caa1546a74695c1'), ('sci-syntax-quote-gensym-fresh', ['clojure.core/not='], '0a8e1170c7c6a2dc424764fe8928ae203edadc793c1781a98c66814edf9773b8'), ('sci-deferred-empty-nth-default', ['clojure.core/nth'], 'e26823b2940193922bc04dba3c95a36d91bb696adf61d7af93d492ae0c8b288d'), ('sci-seq-filterv-effects', ['clojure.core/odd?'], '28fa02e6fdb0e0725539801fd206816252206013c946e7e3661a5b0bc07dd6c2'), ('sci-seq-partial-many', ['clojure.core/partial'], '2af02aa538a1c15688142d63daa7187776a7ff9d781e6b2995beca1f9b9e9292'), ('sci-lazy-partition-negative', ['clojure.core/partition'], 'd3e9c372a575d5dab971c48d40bfc8017c735ecdf537ff477c4f19819503aed6'), ('sci-xf-partition-groups', ['clojure.core/partition-all'], '037a21251bd49647891ff970f0ed780b1482b9bef0d535cc4633cfef87478c66'), ('sci-coll-peek-value', ['clojure.core/peek'], '0e74b3f1a8705d6976e0b252ab2899781fabdc2ed251d2f3ea7cd38a3e0c1bd9'), ('sci-coll-pop-nil', ['clojure.core/pop'], '2fe1dd578c3738be1708a39ae5743051295a527f1582f9ff4763bdaa7ddf5e03'), ('sci-binding-pre-param-meta', ['clojure.core/pos?'], 'a5799ac423c2abe31b4bf440c1dfa558456a4857446ebbb5270cbd3d301bf33b'), ('sci-string-pr-readably-default', ['clojure.core/pr-str'], '6c1f7612aef37fc42f9079ea5d5912e8e3ee1f38c4fb1aee855eb15d52fd121d'), ('sci-io-print-utf16', ['clojure.core/print'], 'aa056b41ae7931bcbe4656a75a3cad3a044771ebec06bb157e5ac9987dca82c4'), ('sci-io-out-adapted', ['clojure.core/println'], '926988bd26dbe351707361a711bc4f6f228d17fc29859dd1517a9aecbc2916a0'), ('sci-string-pr-empty', ['clojure.core/prn-str'], '7aa1bc1a33ea67c057d3ce0ace19a14d0137598f87912960823f931e676cf45a'), ('sci-lazy-reduce-upstream', ['clojure.core/range', 'clojure.core/reduce'], 'f08d065c15999aac3f0bbe69eb4f1f6a774645ef63cbe5a5c668484c48dd131d'), ('sci-seq-reduce-kv-map', ['clojure.core/reduce-kv'], '8ab671f598ff2913835c282e96316d3b80489351049e0324309bf3dda7ad09f7'), ('sci-seq-reduce-kv-early', ['clojure.core/reduced'], '0af480ec6ada5db001a3c4bf9259768069317ad62c7d176c80332640122006d0'), ('sci-seq-reduce-kv-marker', ['clojure.core/reduced?'], 'ea4615184ead8798e74544f8496d3d5b7dfa6f999ff2f6dec3ab050e5c992c03'), ('sci-xf-basic-remove', ['clojure.core/remove'], '25754c496c3b83e4dc9919620f3dda509d5ddd6849142cb25093f4bc70c8935e'), ('sci-state-watch-key-remove-empty', ['clojure.core/remove-watch'], '375e196d0089420eec37260524218845e33c2c17def6aeafe9cac5ee47d4400a'), ('sci-lazy-probe-repeat-neg-frac', ['clojure.core/repeat'], '4f3cf90e43fee074e13a289f8c5213a82b0861e716648815d8b1c72ef441b022'), ('sci-state-atom-reset', ['clojure.core/reset!'], '84a264513061fed932b39a16143626207bf1b2d768b9b4bda6fa1343d691b1b9'), ('sci-state-reset-vals', ['clojure.core/reset-vals!'], 'c3fbd0d6afeaea14f2ec665d82ff576ab4987928c67567ea3e8c5d743278cb13'), ('sci-syntax-quote-deferred-rest', ['clojure.core/rest', 'clojure.core/seq?'], '6a46afd5f9f27a2f28ec138a02a69a4af105a64fa73fdbe93dfb0330b9f5142d'), ('sci-coll-reversible-p-nil', ['clojure.core/reversible?'], 'b61e30bd9545545a1e28f20927d7bce584b02b8b9ef1933d8a4551bb1cfd0c59'), ('sci-lazy-second', ['clojure.core/second'], '73c8606e50e75ed7a2fac28dd78824d0253de27504a022d6fd18886b135ed4cf'), ('sci-syntax-quote-metadata-symbol', ['clojure.core/select-keys'], 'a0f41d91d4b4f3ab1d71dcb47ccc0f00d8dcdb47451f12f215a17b540962da5f'), ('sci-seq-into-entry-seq', ['clojure.core/seq'], '48ff0dd6e1af4f13c588ae41a5154d7dff449ea54fe7a5f095603def001d03c9'), ('sci-coll-seqable-p-nil', ['clojure.core/seqable?'], '62872d37884bdfd5ca2bac8e94a40e78e92eb1397e763d88a19913663f7e1f36'), ('sci-coverage-sequence', ['clojure.core/sequence'], '7367647311fa45e29f609e6e87e768063401f3e22f84aecaeaec569e7afe6e07'), ('sci-deferred-lazy-predicates', ['clojure.core/sequential?'], 'fb7a4a58ebf96d003efa2467d0df1befb4c786d38f3d6d0498e8f56fd3757917'), ('sci-coll-set-nil', ['clojure.core/set'], '4248f324c96f1d66cdce8e37ee00c63b8fcfc6a52e326ec297638dc6853de229'), ('sci-lazy-split-at', ['clojure.core/split-at'], 'd371331092b9ad3169eb15c0734a97418d15a431201c8d4bbea760dfd87dc305'), ('sci-lazy-split-with', ['clojure.core/split-with'], 'c898498d63ed75e8c592add39cb5ae7737a7c4282ff8165f7a3de748520c1733'), ('sci-string-str-empty', ['clojure.core/str'], '2b04baf5c178338ada74222c97e64ee38a7a11877bf2dd61d34247397549e5ca'), ('sci-control-cond-else', ['clojure.core/string?'], '9dbce2dfb9e02f95649e2f410b43b2f14aa4e97bc2569dc685c8c73ecfbc601a'), ('sci-string-subs-coerce-index', ['clojure.core/subs'], 'cce2f2ccb053033a5de11a7a7e446eb0978d5b3179e4a393e89ee65ac40016b5'), ('sci-syntax-quote-gensym-repeat', ['clojure.core/symbol?'], 'b715c9d51dced2237c585100706b99516cc6dc92be75e4a3aa77b2ba85dc5b8a'), ('sci-xf-bounds-pipeline', ['clojure.core/take'], '9296b668563613dc7a04cc45b476fb736b8cd1950cf1f45087a97ee2caccd6e0'), ('sci-lazy-probe-take-nth-fraction', ['clojure.core/take-nth'], '60dedb3eb32a0f6ea81dd80e4420fdc0935147e954f6aaaf0dbfa0805510bcc6'), ('sci-xf-bounds-while-direct', ['clojure.core/take-while'], '9db1486a4e9e9c06e8b29a3a127a52f1728c17b6006554ec5e2a0d40ee991e3b'), ('sci-transduce-custom-double', ['clojure.core/transduce'], 'fb41fc3353e255d66fade1d8d8bd0d62b08f60cc56f739fe34011d4609c10307'), ('sci-seq-reduced', ['clojure.core/unreduced'], 'f8e863f6022304a3774e88b20cefdd7c1ccfafe4e42f5fd8e522595e0f06f26c'), ('sci-coll-update-value', ['clojure.core/update'], '6a56460691dd147c334de7cbd32d395a27a5a26e5d3b99ba7aacf9ef98c546ae'), ('sci-coll-update-in-value', ['clojure.core/update-in'], 'd5396d5772ee0dda8d23a31b92ecb91d61634cb067b0f8a0ecb72b027bef94b8'), ('sci-coll-vals-value', ['clojure.core/vals'], '3b741b345bdc906732252cd91a032517841720f0e8dd21deea64819292716662'), ('sci-coll-vec-nil', ['clojure.core/vec'], '2e5f52466d4c5cfef6867a877c9d481b197e485aa830dea0ff328f4b4bef8cac'), ('sci-coll-vector-nil', ['clojure.core/vector'], 'ba7e92635a9f3970146c3c875fc89b6df0f02975e99c0226d0dc3127e983fe60'), ('sci-state-volatile-reset', ['clojure.core/vreset!'], '3b5897b2f718f246d680e8b97bfd58cd31ebeb8eed9f53b9d9b3850aee35741d'), ('sci-string-pr-meta-default', ['clojure.core/with-meta'], '6f0a369d8c6c093782d6c5101861407c12068beb32466260671abdfc4332642a'), ('sci-native-if-zero', ['clojure.core/zero?'], '7ed6733398e74489573cc6b591520ab9395e5ecd47ef7e5117767a763236ed92')]


def digest(value):
    return hashlib.sha256(value if isinstance(value, bytes) else value.encode()).hexdigest()


def literal(tree):
    if tree == 'nil': return 'nil'
    kind, *values = tree
    if kind == 'bool': return values[0]
    if kind == 'int':
        if abs(int(values[0])) > 9007199254740991: raise ValueError('nonportable integer')
        return values[0]
    if kind == 'string': return json.dumps(sci.string(values[0]), ensure_ascii=True)
    if kind == 'char': return json.dumps(sci.string('u' + values[0]), ensure_ascii=True)
    if kind in {'keyword', 'symbol'}:
        name = ('' if values[0] == '-' else sci.string(values[0]) + '/') + sci.string(values[1])
        return (':' if kind == 'keyword' else '') + name
    if kind == 'vector': return '[' + ' '.join(map(literal, values)) + ']'
    if kind in {'list', 'seq'}: return '(' + ' '.join(map(literal, values)) + ')'
    if kind == 'set': return '#{' + ' '.join(map(literal, values)) + '}'
    if kind == 'map': return '{' + ' '.join(literal(v[1]) + ' ' + literal(v[2]) for v in values) + '}'
    raise ValueError('unreviewed literal domain: ' + kind)


# SCI projects lists and sequential implementations together, but preserves
# vectors recursively, including map keys and set members. Ordinary `=` loses
# that distinction. This comparator retains the full portable wire contract.
MATCHER = '''(fn api-same? [expected actual]
  (cond
    (vector? expected) (and (vector? actual) (= (count expected) (count actual))
                           (every? true? (map api-same? expected actual)))
    (seq? expected) (and (sequential? actual) (not (vector? actual))
                        (= (count expected) (count actual))
                        (every? true? (map api-same? expected actual)))
    (map? expected) (and (map? actual) (= (count expected) (count actual))
                        (every? (fn [[k v]]
                          (boolean (some (fn [[a b]]
                            (and (api-same? k a) (api-same? v b))) actual))) expected))
    (set? expected) (and (set? actual) (= (count expected) (count actual))
                        (every? (fn [x] (boolean (some (fn [y] (api-same? x y)) actual))) expected))
    (nil? expected) (nil? actual)
    (boolean? expected) (and (boolean? actual) (= expected actual))
    (string? expected) (and (string? actual) (= expected actual))
    (keyword? expected) (and (keyword? actual) (= expected actual))
    (symbol? expected) (and (symbol? actual) (= expected actual))
    (number? expected) (and (number? actual) (= expected actual)
                           (not (js* "Object.is(~{}, -0)" actual)))
    :else false))'''


def complete_source(original, value, effects, names):
    aliases = ' '.join('(identical? ' + name + ' ' + name.rsplit('/', 1)[-1] + ')' for name in names)
    complete = ('(let [api-same? ' + MATCHER + ' api-effects (atom []) observe '
        '(fn [x] (swap! api-effects conj x) x) api-result (do ' + original
        + ")] [(api-same? '" + value + " api-result) (api-same? '" + effects
        + ' @api-effects) [' + aliases + ']])')
    return ('(let [api-observed (atom nil) api-stdout (with-out-str '
        '(reset! api-observed ' + complete + '))] [@api-observed api-stdout])')


def calls(source):
    result = {}
    def visit(node):
        if node.kind in {'quote', 'syntax-quote', 'discard', 'var'}: return
        name = head(node)
        if name == 'quote': return
        if name and node.kind == 'list': result.setdefault(name, set()).add(len(node.children) - 1)
        if name.rsplit('/', 1)[-1] in {'->', '->>', 'some->', 'some->>', 'cond->', 'cond->>', 'as->'}: return
        for child in node.children: visit(child)
    for node in Reader(source).all(): visit(node)
    return result


def rows():
    raw = sci.rows(sci.CORPUS)
    current = {r['sci_id']: r for r in sci.load_all()}
    advertised = {line.split('\t')[0]: line.split('\t') for line in
        (ROOT / 'tests/reference/sci/historical/supported-apis.tsv').read_text().splitlines()
        if line and not line.startswith('#')}
    result = []
    for line in (ROOT / FIXTURE).read_text().splitlines():
        if not line or line.startswith('#'): continue
        identity, source_hex, expected_hex, encoded = line.split('\t')
        row = json.loads(encoded)
        source = bytes.fromhex(source_hex).decode()
        expected = bytes.fromhex(expected_hex).decode()
        if (identity, source, expected) != (row['id'], row['source'], row['expected_edn']):
            raise ValueError('detached complete source/literal')
        sci_id = row['sci_id']
        original, fields = raw[sci_id]
        if (digest(original) != row['original_row_sha256'] or fields[2] != row['original_source']
            or digest(fields[2]) != row['original_source_sha256'] or row['sci_binding'] != current[sci_id]):
            raise ValueError('original SCI source/expectation binding drift')
        if row['sci_binding']['original_source_sha256'] != row['sci_binding']['source_sha256']:
            raise ValueError('this cohort needs unchanged original source')
        names = row['names']
        for name in names:
            if advertised[name][1:4] != ['function', 'sci-exact', sci_id]:
                raise ValueError('advertised source/API link drift')
            if name.rsplit('/', 1)[-1] not in calls(fields[2]):
                raise ValueError('quoted, indirect or absent advertised operation')
        value = literal(sci.wire(fields[3])[1]); effects = literal(sci.wire(fields[4]))
        complete = complete_source(fields[2], value, effects, names)
        frozen = '[[true true [' + ' '.join('true' for name in names) + ']] ""]'
        if source != complete or expected != frozen or fields[5:7] != ['(string u)', '(string u)']:
            raise ValueError('whole outcome/effect/alias literal contract drift')
        result.append(row)
    if [(r['sci_id'], r['names'], r['original_row_sha256']) for r in result] != REVIEWED:
        raise ValueError('whole reviewed cohort changed/reordered')
    return result


def cases():
    return [{'id': r['id'], 'source': "(= '" + r['expected_edn'] + ' ' + r['source'] + ')',
        'expected': {'id': r['id'], 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''},
        'hosted_source_adapter': {'profile': PROFILE, 'schema_version': 1}} for r in rows()]


def reference_rows():
    result = []
    for case in cases():
        source = case['source']
        result.append({**case, 'source_sha256': digest(source), 'forms': [{'kind': 'runtime', 'source': source}]})
    return result


def support():
    files = [FIXTURE, 'tests/reference/compiler-language/local_api_source_context.py',
        'tests/fixtures/sci-outcomes.tsv', 'tests/reference/sci/evaluator-dispositions.tsv',
        'tests/reference/sci/historical/supported-apis.tsv', 'src/core_var_metadata.tsv']
    return {name: digest((ROOT / name).read_bytes()) for name in files}


def validate(case):
    expected = next((row for row in cases() if row['id'] == case.get('id')), None)
    if expected is None:
        return False
    if case != expected:
        raise ValueError('whole local API source/alias profile drift')
    return True


def snapshot():
    return {**context.compiler_sources(), **support(), DRIVER: digest((ROOT / DRIVER).read_bytes()),
        context.ORACLE: digest((ROOT / context.ORACLE).read_bytes()),
        'tests/reference/execution.clj': digest((ROOT / 'tests/reference/execution.clj').read_bytes()),
        'tests/support/execution.js': digest((ROOT / 'tests/support/execution.js').read_bytes())}


def parse_frames(output):
    records = rows(); expected = []
    for row in records:
        expected.extend((row['id'], host, phase, engine) for host in ['fresh', 'reused']
            for phase in ['application', 'macro'] for engine in ['QuickJS', 'Node'])
    frames = []
    for line in output.splitlines():
        if not line.startswith('local-api-source-observation\t'): continue
        fields = line.split('\t')
        if len(fields) != 6: raise ValueError('invalid raw local API frame')
        _, identity, host, phase, engine, actual = fields
        # The driver serialized the equality of complete independent literals,
        # original outcome/effects and alias checks inside the actual phase.
        if json.loads(actual) is not True: raise ValueError('nonpassing whole source result')
        frames.append({'id': identity, 'host': host, 'phase': phase, 'engine': engine, 'raw': actual})
    if [(f['id'], f['host'], f['phase'], f['engine']) for f in frames] != expected:
        raise ValueError('missing/extra/reordered source/phase/host frames')
    return frames


def local(directory, timeout=300):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=False)
    before = snapshot(); runner = sci.report_module()
    artifacts = directory / 'modules'
    runs = []
    for test in TESTS:
        command = ['env', 'LIGHT_LOCAL_API_SOURCE_ARTIFACTS=' + str(artifacts.resolve()),
            'cargo', 'test', '--locked', '--test', 'compiler_local_api_source_context',
            '--message-format=json', '--', '--exact', test, '--nocapture']
        run = runner.invoke(command, directory / 'cargo' / test, timeout=max(180, timeout))
        if run['status'] != 'completed' or 'test result: ok. 1 passed; 0 failed; 0 ignored;' not in run.get('stdout', ''):
            raise ValueError('actual whole-source driver failed; raw invocation retained: ' + str(directory))
        runs.append(run)
    run = {'stdout': '\n'.join(r['stdout'] for r in runs)}
    executable = []
    for line in run['stdout'].splitlines():
        if not line.startswith('{'): continue
        try: record = json.loads(line)
        except ValueError: continue
        if record.get('reason') == 'compiler-artifact' and record.get('target', {}).get('name') == 'compiler_local_api_source_context' and record.get('executable'):
            executable.append(record['executable'])
    if len(executable) != 3 or len(set(executable)) != 1: raise ValueError('missing exact actual audit executable')
    executable_hash = digest(Path(executable[0]).read_bytes())
    frames = parse_frames(run['stdout']); result = {}
    for case in cases():
        observations = {}
        for phase, host in [('application', 'fresh'), ('application', 'reused'), ('macro', 'fresh'), ('macro', 'reused')]:
            root = artifacts / case['id'] / host / phase
            for engine in ['QuickJS', 'Node']:
                frame = next(f for f in frames if (f['id'], f['phase'], f['host'], f['engine']) == (case['id'], phase, host, engine))
                observed = {'id': case['id'], 'outcome': {'status': 'returned', 'value': json.loads(frame['raw'])}, 'trace': [], 'stdout': ''}
                observations[phase + '-' + host + '/' + engine] = {'status': 'passed', 'observed': observed,
                    'raw_observation_sha256': digest(frame['raw']), 'compiled_js_sha256': digest((root / 'compiled.js').read_bytes()),
                    'module_source_sha256': digest((root / 'user.cljs').read_bytes()),
                    'compiler_build_producer_source_sha256': before, 'audit_executable_sha256': executable_hash,
                    'actual_frame': frame}
        result[case['id']] = {'status': 'passed', 'observations': observations}
    if snapshot() != before or digest(Path(executable[0]).read_bytes()) != executable_hash:
        raise ValueError('actual source/driver/compiler drift during audit')
    document = {'status': 'passed', 'observations': {k: v['observations'] for k, v in result.items()},
        'compiler_build_producer_source_sha256': before, 'audit_executable_sha256': executable_hash,
        'frames': frames, 'raw_output_sha256': digest(run['stdout'])}
    (directory / 'evidence.json').write_text(json.dumps(document, indent=2) + '\n')
    return document


def execute(selected, directory, timeout=300, reference='both', checkout=None):
    if reference not in ['none', 'release', 'checkout', 'both'] or any(not validate(c) for c in selected):
        raise ValueError('invalid explicit local API source embedding')
    if checkout is not None and Path(checkout).resolve() != (ROOT / 'research/clojurescript').resolve():
        raise ValueError('exact reviewed checkout required')
    directory = Path(directory); directory.mkdir(parents=True, exist_ok=False)
    before = snapshot(); document = local(directory / 'local', timeout); references = {}
    for kind in ['release', 'checkout']:
        if reference in [kind, 'both']:
            references[kind] = context.reference_cases(reference_rows(), kind, directory / kind, max(300, timeout))
    if snapshot() != before: raise ValueError('reference/adapter drift during complete cohort')
    result = {}
    for case in selected:
        identity = case['id']; observations = document['observations'][identity]
        result[identity] = {'javascript': {'status': 'passed', 'observed': next(iter(observations.values()))['observed'],
            'hosted_source_adapter': PROFILE, 'observations': observations, 'generic_cli': 'not claimed'}}
        for kind, records in references.items(): result[identity][kind] = records[identity]
    (directory / 'evidence.json').write_text(json.dumps({'local': document, 'references': references, 'cases': selected,
        'compiler_build_producer_source_sha256': before}, indent=2) + '\n')
    return result


def exports():
    records = rows(); oracles = cases()
    obligations = [json.loads(line) for line in (HERE / 'obligations.jsonl').read_text().splitlines()]
    old = {json.loads(line)['obligation_id'] for line in (HERE / 'obligation-witnesses.jsonl').read_text().splitlines()}
    old_phase = {json.loads(line)['obligation_id'] for line in (HERE / 'phase-witnesses.jsonl').read_text().splitlines()}
    ordinary = []; phase = []; receipts = []
    for row_number, (row, oracle) in enumerate(zip(records, oracles)):
        observed = calls(row['original_source'])
        for target in obligations:
            name = target['name']; bare = name.rsplit('/', 1)[-1]
            if target['disposition'] != 'intended' or target['obligation_kind'] not in {'local-api', 'public-api'}: continue
            if ('clojure.core/' + bare) not in row['names']: continue
            if target['obligation_kind'] == 'public-api' and target['source'] not in {'src/main/cljs/cljs/core.cljs', 'src/main/clojure/cljs/core.cljc'}: continue
            signature = target.get('signature'); arities = observed.get(bare, set())
            if signature:
                if signature['kind'] == 'fixed': arities = {n for n in arities if n == signature['minimum']}
                elif signature['kind'] == 'variadic': arities = {n for n in arities if n > signature['minimum']}
                else: continue
            if not arities: continue
            if target['id'] in old or target['id'] in old_phase: continue
            rationale = ('Complete original ' + row['sci_id'] + ' retains the independently captured outcome/effect literals, '
                'executes ' + bare + ' at exact reviewed direct arities ' + str(sorted(arities)) + ', and separately checks '
                'the qualified clojure.core alias. Fresh/reused applications and actual MacroHost computations execute '
                'the full body in original user namespace, in QuickJS and Node; both required canonical pins check the same source. '
                'Historical API/metadata registration grants no pass or complete metadata-value coverage.')
            common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'], 'rationale': rationale}
            if signature: common['signature_sha256'] = digest(json.dumps(signature, sort_keys=True))
            execution = {'test_file': DRIVER, 'test_name': TESTS[min(row_number // 40, 2)], 'test_source_sha256': digest((ROOT / DRIVER).read_bytes())}
            ordinary.append({**common, 'assertion_ids': [], 'local_execution': execution,
                'reference_fixture_id': oracle['id'], 'reference_source_sha256': digest(oracle['source']),
                'reference_contract_sha256': digest(json.dumps(oracle['expected'], sort_keys=True))})
            phase.append({**common, **execution, 'support_source_sha256': support()})
            receipts.append({'obligation_id': target['id'], 'sci_id': row['sci_id'], 'original_source_sha256': row['original_source_sha256'],
                'original_row_sha256': row['original_row_sha256'], 'direct_arities': sorted(arities), 'qualified_alias': 'clojure.core/' + bare})
            old.add(target['id']); old_phase.add(target['id'])
    linked = {r['reference_fixture_id'] for r in ordinary}
    return {'obligation-witnesses.jsonl': ordinary, 'phase-witnesses.jsonl': phase,
        'full-cases.jsonl': [r for r in oracles if r['id'] in linked], 'local-api-source-receipts.jsonl': receipts}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export-owned', type=Path)
    parser.add_argument('--references', type=Path)
    args = parser.parse_args()
    if args.export_owned:
        args.export_owned.mkdir(parents=True, exist_ok=True)
        for filename, records in exports().items():
            (args.export_owned / filename).write_text(''.join(json.dumps(r, separators=(',', ':')) + '\n' for r in records))
    if args.references:
        args.references.mkdir(parents=True, exist_ok=False)
        rows = reference_rows(); result = {}
        for kind in ['release', 'checkout']:
            result[kind] = context.reference_cases(rows, kind, args.references / kind, 600)
        receipt = {'cases': cases(), 'rows': rows, 'references': result, 'driver_sha256': digest((ROOT / DRIVER).read_bytes()),
            'support_source_sha256': support()}
        (args.references / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
        if not all(layer.get('status') == 'passed' for pin in result.values() for layer in pin.values()):
            raise SystemExit('canonical contract remains nonpassing; complete raw receipts retained')
    print('121 original sources;143 explicit qualified API alias contracts; candidate exports require fresh execution')

if __name__ == '__main__': main()
