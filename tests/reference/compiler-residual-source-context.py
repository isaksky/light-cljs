#!/usr/bin/env python3
"""Preserve complete residual bodies with the namespace context they observe.
Copyright (c) Rich Hickey. Eclipse Public License 1.0; retain original notices.
"""
from pathlib import Path
import importlib.util
import json
import re
import shutil
import subprocess
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import extract_inventory as inv
import report
import hosted_source_dispatch as hosted
spec = importlib.util.spec_from_file_location('completion', ROOT / 'tests/reference/compiler-residual-completion.py')
completion = importlib.util.module_from_spec(spec)
spec.loader.exec_module(completion)
residual = completion.residual
PREFIX = 'cljs-residual-source-context-'
BASE = 'tests/fixtures/compiler-residual-source-context'
FIXTURE = BASE + '.tsv'
TEST = 'tests/compiler_residual_source_context.rs'
TEST_NAME = 'complete_residual_namespace_bodies_execute_in_both_phases'
# All non-test support forms are copied from the exact source module, including
# top-level let closures and namespace/name shadowing. Namespace clauses remove
# only cljs.test harness imports after assertions become executing assertions.
SPECS = [
 ('keyword', 'src/test/cljs/cljs/keyword_test.cljs', ['test-keyword'],
  '(ns cljs.keyword-test (:require [cljs.keyword-other :as other] [cljs.keyword-macros :as macros]))',
  '(ns cljs.keyword-test (:require-macros [cljs.keyword-macros :as macros]) (:require [cljs.keyword-other :as other]))'),
 ('syntax-quote', 'src/test/cljs/cljs/syntax_quote_test.cljs', ['test-syntax-quote'],
  '(ns cljs.syntax-quote-test (:require [cljs.test :as test]))', None),
 ('ns-foo', 'src/test/cljs/cljs/ns_test/foo.cljs', ['test-namespaced-keywords', 'test-as-alias-keywords'],
  '(ns cljs.ns-test.foo (:require [made.up.lib :as-alias lib]))', None),
 ('cljs-shadow', 'src/test/cljs/cljs/foo/ns_shadow_test.cljs', ['test-shadow'],
  '(ns foo.ns-shadow-test (:require baz))', None),
 ('foo-shadow', 'src/test/cljs/foo/ns_shadow_test.cljs', ['test-shadow'],
  '(ns foo.ns-shadow-test (:require baz))', None),
 ('top-level', 'src/test/cljs/cljs/top_level.cljs', ['test'],
  '(ns cljs.top-level (:refer-clojure :exclude [test]))', None),
 ('js-keys', 'src/test/cljs/cljs/macro_test.cljs', ['test-cljs-2283'],
  '(ns cljs.macro-test (:refer-clojure :exclude [==]))', None),
 ('macro-source', 'src/test/cljs/cljs/macro_test.cljs', ['test-macros', 'test-cljs-2852', 'test-cljs-3027', 'test-cljs-3413'],
  '(ns cljs.macro-test (:refer-clojure :exclude [==]) (:require [cljs.macro-test.macros :refer [== sm-cljs-3027]] [cljs.macro-test.cljs2852] [single-seg-macros]))',
  '(ns cljs.macro-test (:refer-clojure :exclude [==]) (:require-macros [cljs.macro-test.macros :refer [== sm-cljs-3027]] [cljs.macro-test.cljs2852] [single-seg-macros]))'),
 ('ns-test', 'src/test/cljs/cljs/ns_test.cljs', ['test-ns', 'test-cljs-1508', 'test-cljs-3399'],
  '(ns cljs.ns-test (:refer-clojure :exclude [+ for] :rename {mapv core-mapv}) (:require [cljs.core :as lang :refer [when when-let] :rename {when always when-let always-let}] [cljs.ns-test.foo :refer [baz]] [clojure.set :as s :refer [intersection] :rename {intersection itsc}] [fake.ns :as-alias fake]) (:use [cljs.ns-test.bar :only [quux]]))',
  '(ns cljs.ns-test (:refer-clojure :exclude [+ for] :rename {mapv core-mapv}) (:require-macros [clojure.core :as lang :refer [when when-let] :rename {when always when-let always-let}]) (:require [cljs.ns-test.foo :refer [baz]] [clojure.set :as s :refer [intersection] :rename {intersection itsc}] [fake.ns :as-alias fake]) (:use [cljs.ns-test.bar :only [quux]]))'),
 ('ns-exists', 'src/test/cljs/cljs/ns_test.cljs', ['test-cljs-2292'],
  '(ns cljs.ns-test (:refer-clojure :exclude [+ for] :rename {mapv core-mapv}) (:require [cljs.core :as lang :refer [when when-let] :rename {when always when-let always-let}] [cljs.ns-test.foo :refer [baz]] [clojure.set :as s :refer [intersection] :rename {intersection itsc}] [fake.ns :as-alias fake]) (:use [cljs.ns-test.bar :only [quux]]))',
  '(ns cljs.ns-test (:refer-clojure :exclude [+ for] :rename {mapv core-mapv}) (:require-macros [clojure.core :as lang :refer [when when-let] :rename {when always when-let always-let}]) (:require [cljs.ns-test.foo :refer [baz]] [clojure.set :as s :refer [intersection] :rename {intersection itsc}] [fake.ns :as-alias fake]) (:use [cljs.ns-test.bar :only [quux]]))'),
 ('static', 'src/test/cljs/static/core_test.cljs', ['foo-test'], '(ns static.core-test)', None),
 ('tagged-runtime', 'src/test/cljs/cljs/tagged_literals_test.cljs', ['test-runtime-reader'],
  '(ns cljs.tagged-literals-test (:require [cljs.reader :as reader] [clojure.set :as set]))', None),
 ('tagged-source', 'src/test/cljs/cljs/tagged_literals_test.cljs', ['test-identity-custom-literal'],
  '(ns cljs.tagged-literals-test (:require [cljs.reader :as reader]))', None),
 ('edn-stream', 'src/test/cljs/clojure/edn_test.cljs', ['test-read'],
  '(ns clojure.edn-test (:require [clojure.edn :as edn] [cljs.reader :as reader] [cljs.reader :as reader-types]))',
  '(ns clojure.edn-test (:require [clojure.edn :as edn] [cljs.reader :as reader] [cljs.tools.reader.reader-types :as reader-types]))'),
 ('apply', 'src/test/cljs/cljs/apply_test.cljs',
  ['js-fns-test', 'data-structures-test', 'meta-fn-test', 'multi-arity-test',
   'single-arity-variadic-test', 'multi-arity-variadic-test'],
  '(ns cljs.apply-test)', None),
 ('binding', 'src/test/cljs/cljs/binding_test.cljs',
  ['test-binding', 'test-with-redefs', 'test-binding-parallel', 'test-redefs-parallel'],
  '(ns cljs.binding-test (:require [cljs.binding-test-other-ns :as o]))', None),
 ('letfn', 'src/test/cljs/cljs/letfn_test.cljs', ['test-letfn'],
  '(ns cljs.letfn-test)', None),
 ('record-literals', 'src/test/cljs/data_readers_test/records.cljc',
  ['top-level-003', 'top-level-004', 'top-level-005', 'top-level-006', 'top-level-007'],
  '(ns data-readers-test.records)',
  '(ns data-readers-test.records (:require-macros [data-readers-test.records]))'),
]
RATIONALE = ('Complete original body, every relevant original setup form, namespace identity, aliases, literal expectations and effects retained. cljs.test harness imports are removed only after is/deftest become executing assertions; reader-only cljs.test alias uses an empty local namespace marker, while references use the actual library. Original source-macro .clj modules are byte-identical as local .cljs modules and remain .clj for the reference JVM macro compiler; local ordinary require and canonical require-macros differ explicitly, with the original use-macros :only becoming the equivalent refer import. Unused macro_test macro imports are omitted only for its independent js-keys body, which calls none of them. These contexts establish these complete bodies, not general cljs.test or unchanged .clj companion compatibility.')


def pinned(path):
    data = subprocess.check_output(['git', '-C', str(ROOT / 'research/clojurescript'), 'show', inv.REVISION + ':' + path])
    assert (ROOT / 'research/clojurescript' / path).read_bytes() == data
    return data.decode()


def namespace_assertions(text, node):
    """Keep aliases and evaluate original cljs.test assertion messages eagerly.

    test-cljs-3399 nests a second is in the first is's message. Both original
    sites must execute even when the first condition is true. Binding names
    are reserved here and are absent from every original selected source.
    """
    assert 'source-context-value__' not in text and 'source-context-message__' not in text
    head = inv.short_head(node)
    if head == 'deftest':
        return '(do ' + ' '.join(namespace_assertions(text, child) for child in node.children[2:]) + ' true)'
    if head == 'testing':
        return '(do ' + ' '.join(namespace_assertions(text, child) for child in node.children[2:]) + ')'
    if head in ('thrown?', 'thrown-with-msg?'):
        return residual.adapt(text, node)
    if head == 'is':
        forms = [namespace_assertions(text, child) for child in node.children[1:]]
        assert 1 <= len(forms) <= 2
        if len(forms) == 1:
            return '(assert ' + forms[0] + ')'
        return '(let [source-context-value__ ' + forms[0] + ' source-context-message__ ' + forms[1] + '] (assert source-context-value__ source-context-message__))'
    if not node.children:
        return text[node.start:node.end]
    parts, offset = [], node.start
    for child in node.children:
        parts.extend([re.sub(r';[^\n]*', '', text[offset:child.start]), namespace_assertions(text, child)])
        offset = child.end
    return ''.join(parts) + re.sub(r';[^\n]*', '', text[offset:node.end])


def generate(catalogs=False):
    bodies = residual.read('full-bodies.jsonl')
    inventory = residual.read('full-inventory.jsonl')
    rows, cases, witnesses, phases = [], [], [], []
    existing_sites = {row['assertion_id'] for name in ('witnesses.jsonl', 'full-witnesses.jsonl')
                      for row in residual.read(name)
                      if not row['fixture_id'].startswith(PREFIX) and row['fixture_id'] not in hosted.PROFILES}
    for label, path, selected, namespace, reference_namespace in SPECS:
        original = pinned(path)
        original_bodies = [body for body in bodies if body['source'] == path]
        selected_form = 'assert' if label == 'record-literals' else 'deftest'
        wanted = [body for body in original_bodies if body['name'] in selected and body['form'] == selected_form]
        assert len(wanted) == len(selected)
        setup = [body for body in original_bodies if body['form'] not in ('ns', 'deftest', selected_form)]
        # macro_test's other macro tests have no setup forms in this .cljs file.
        forms, origins = [], []
        for body in setup + wanted:
            text = body['source_span']['text']
            assert inv.digest(text.encode()) == body['source_span']['sha256']
            span = body['source_span']
            assert original.encode()[span['start_byte']:span['end_byte']].decode() == text
            adapt = namespace_assertions if label.startswith('ns-') or label in ('edn-stream', 'apply', 'binding', 'letfn') else residual.adapt
            forms.append(adapt(text, inv.Reader(text).all()[0]))
            origins.append({'body_id': body['id'], 'source_sha256': span['sha256']})
        expression = '(do ' + ' '.join(forms[len(setup):]) + ')'
        if label == 'record-literals':
            # Each complete original top-level assertion must execute before
            # the driver reports success; none is rewritten as plain data.
            expression = '(do ' + ' '.join(forms[len(setup):]) + ' true)'
        configuration = None
        if label in ('tagged-runtime', 'tagged-source'):
            # Upstream cljs.reader/add-data-readers initializes these handlers
            # from this exact configuration. The complete original map is
            # retained as provenance; only this body's three used handlers
            # need public registry installation in the explicit embedding.
            config = pinned('src/test/cljs/data_readers.cljc')
            configuration = {'source': 'src/test/cljs/data_readers.cljc', 'sha256': inv.digest(config.encode()), 'text': config,
                             'adapter': 'Install the three exact original cljs/tag=identity, cljs/inc=inc and cljs/union=clojure.set/union handlers through the public reader registry in each independently executed phase. Unused custom test handlers are not claimed.'}
            if label == 'tagged-source':
                configuration['adapter'] = 'Install the exact original cljs/tag=clojure.core/identity entry through the public compiler-data-reader registry before reading each fresh/reused phase source. Reference data_readers.cljc selects this exact original entry; the entire original map is retained here as provenance. Unused custom handlers require separate companion namespaces and are not supplied or claimed.'
            if label == 'tagged-runtime':
                expression = '(do (reader/register-tag-parser! (quote cljs/tag) identity) (reader/register-tag-parser! (quote cljs/inc) inc) (reader/register-tag-parser! (quote cljs/union) set/union) ' + expression + ')'
        name_node = inv.Reader(namespace).all()[0].children[1]
        entry = namespace[name_node.start:name_node.end]
        relpath = entry.replace('.', '/').replace('-', '_') + '.cljs'
        source_root = BASE + '/' + label
        reference_root = BASE + '-reference/' + label
        program = completion.NOTICE + namespace + '\n' + '\n'.join(forms[:len(setup)]) + '\n(defn run [] ' + expression + ')\n'
        reference_program = program.replace(namespace, reference_namespace or namespace, 1)
        completion.persist(ROOT / source_root / relpath, program)
        completion.persist(ROOT / reference_root / relpath, reference_program)
        support = {source_root + '/' + relpath: inv.digest(program.encode())}
        reference_files = {relpath: inv.digest(reference_program.encode())}
        dependencies = {}
        if label == 'keyword':
            dependencies['cljs/keyword_other.cljs'] = pinned('src/test/cljs/cljs/keyword_other.cljs')
            macro = pinned('src/test/cljs/cljs/keyword_macros.clj')
            dependencies['cljs/keyword_macros.cljs'] = macro
            completion.persist(ROOT / reference_root / 'cljs/keyword_macros.clj', macro)
            reference_files['cljs/keyword_macros.clj'] = inv.digest(macro.encode())
        if label == 'macro-source':
            for relative in ('cljs/macro_test/macros', 'cljs/macro_test/cljs2852', 'single_seg_macros'):
                macro = pinned('src/test/cljs/' + relative + '.clj')
                dependencies[relative + '.cljs'] = macro
                completion.persist(ROOT / reference_root / (relative + '.clj'), macro)
                reference_files[relative + '.clj'] = inv.digest(macro.encode())
        if label == 'tagged-source':
            # Select the exact original entry used by this complete body.
            # Unused custom handler namespaces have separate JVM companions.
            assert 'cljs/tag clojure.core/identity' in configuration['text']
            reader_config = completion.NOTICE + '{cljs/tag clojure.core/identity}\n'
            completion.persist(ROOT / reference_root / 'data_readers.cljc', reader_config)
            reference_files['data_readers.cljc'] = inv.digest(reader_config.encode())
        if label == 'binding':
            dependencies['cljs/binding_test_other_ns.cljs'] = pinned('src/test/cljs/cljs/binding_test_other_ns.cljs')
        if label.endswith('shadow'):
            dependencies['baz.cljs'] = pinned('src/test/cljs/baz.cljs')
        if label == 'syntax-quote':
            # The test observes only qualified symbols. No test function or
            # test macro is called from this reader context marker.
            dependencies['cljs/test.cljs'] = completion.NOTICE + '(ns cljs.test)\n'
        if label in ('ns-test', 'ns-exists'):
            # Complete already-proven foo support retains baz/kw/qkw and its
            # alias-only context. Its test driver is declared but not invoked.
            dependencies['cljs/ns_test/foo.cljs'] = (ROOT / BASE / 'ns-foo/cljs/ns_test/foo.cljs').read_text()
            dependencies['cljs/ns_test/bar.cljs'] = pinned('src/test/cljs/cljs/ns_test/bar.cljs')
        if label == 'record-literals':
            # Canonical record literal reading uses the unchanged CLJC record
            # declaration through its original self require-macros clause.
            # This companion is reference-only; no JVM execution is supplied
            # locally and no passing local record-reader contract is claimed.
            completion.persist(ROOT / reference_root / 'data_readers_test/records.cljc', original)
            reference_files['data_readers_test/records.cljc'] = inv.digest(original.encode())
        for relative, text in dependencies.items():
            completion.persist(ROOT / source_root / relative, text)
            support[source_root + '/' + relative] = inv.digest(text.encode())
            if relative not in ('cljs/test.cljs', 'cljs/keyword_macros.cljs') and label != 'macro-source':
                completion.persist(ROOT / reference_root / relative, text)
                reference_files[relative] = inv.digest(text.encode())
        identity = PREFIX + label
        adaptation = RATIONALE
        if label in ('ns-test', 'ns-exists'):
            adaptation += (' Original clojure.core JVM macro namespace aliases use the corresponding cljs.core source macro namespace locally; canonical references retain clojure.core require-macros. All observed alias/refer/rename/exclude/as-alias/use clauses and the complete original + definition remain. Unused goog/cljs.analyzer imports are omitted only because these selected bodies call neither; the separate goog body remains unwitnessed. Original assertion messages evaluate eagerly, including the nested original is site in test-cljs-3399. The complete already-proven ns-test.foo helper and byte-identical ns-test.bar dependency retain their original declarations.')
        if label == 'tagged-runtime':
            adaptation += (' Original runtime-reader aliases are preserved. The exact three used handlers from the unchanged full data_readers.cljc configuration are installed through the public registry independently inside each phase; no compiled source tag or unrelated custom test handler is claimed. All four original read-string inputs and results stay unchanged.')
        if label == 'tagged-source':
            adaptation += (' The complete original identity-custom-literal body retains its exact #cljs/tag source form and vector expectation. The local compiler embedding installs the original configuration entry cljs/tag=identity through register_compiler_data_reader in each fresh/reused MacroHost before source reading; references select the exact original cljs/tag entry in data_readers.cljc, retaining the full map as provenance. Unused custom reader entries and their separate JVM companions are omitted from this independent context. This is the explicit promised compiler-reader capability, not a fabricated tag replacement, runtime register-tag-parser substitute or JVM companion implementation.')
        if label in ('apply', 'binding', 'letfn'):
            adaptation += (' This complete namespace context adds independently executed full-body phase evidence beyond the historical flattened assertion witnesses; those sites are not duplicated in the proposed witness catalog. All original setup and eager assertion messages remain, without reduced inputs or substituted calls.')
        if label == 'apply':
            adaptation += (' All six original active bodies retain fn-returning-this/meta-f setup, both complete six-function doseq candidate lists, all argument counts including the thirty-argument copy-arguments paths, callable data arity errors and original message computations. The two upstream discarded assertion-free tests remain independent and are not counted as active assertions.')
        if label == 'binding':
            adaptation += (' All original local Var/function setup and the complete original binding-test-other-ns module remain. Four contiguous complete bodies retain cross-namespace binding/redefs, restoration and parallel initializer semantics. The separate type-hint inference body is not selected; this context makes no assertion about upstream static type-hint optimization.')
        if label == 'letfn':
            adaptation += (' The complete original mutually recursive ev?/od? letfn definitions and all eight original calls remain in their original body; no recursive helper is replaced or lifted out of the lexical scope.')
        if label == 'edn-stream':
            adaptation += (' The complete original test-reader helper and all three stream read assertions remain unchanged. Its reader-types alias selects the existing first-party cljs.reader/string-push-back-reader constructor locally; pinned references retain cljs.tools.reader.reader-types/string-push-back-reader. Both provide a real mutable pushback stream consumed by the unchanged edn/read and reader/read calls at one, two and four arguments. This explicit constructor namespace adaptation establishes the promised EDN stream operations, not general cljs.tools.reader compatibility; no stream implementation or read-string replacement is supplied by the fixture.')
        if label == 'record-literals':
            adaptation += (' The entire original Foo record setup and all five top-level assertions remain, including ordinary/quoted/doubly quoted tags, symbol-valued fields inside lexical shadowing and both original assertion messages. Assertions execute inside the driver after the unchanged record declaration. Canonical references retain the original self require-macros and a byte-identical CLJC companion; locally that JVM companion clause is omitted under the explicit .clj companion boundary. No fabricated record data reader or constructor expansion is supplied, and this candidate remains unwitnessed pending source-contract reconciliation.')
        provenance = {'module_source': path, 'module_sha256': inv.digest(original.encode()), 'complete_original_provenance': origins,
                      'namespace_source_sha256': next(body['source_span']['sha256'] for body in original_bodies if body['form'] == 'ns')}
        if configuration:
            provenance['original_data_reader_configuration'] = configuration
        rows.append([identity, entry, source_root, relpath, json.dumps(provenance, separators=(',', ':'))])
        case = {'id': identity, 'source': program, 'source_root': source_root, 'reference_source_root': reference_root,
                'entry': entry, 'export': 'run', 'reference_files_sha256': reference_files,
                'reference_adaptation': adaptation, **provenance,
                'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}, 'native_stdout': None}
        local = {'test_file': TEST, 'test_name': TEST_NAME, 'test_source_sha256': inv.digest((ROOT / TEST).read_bytes()),
                 'support_source_sha256': support}
        for body in wanted:
            mapping = {'body_id': body['id'], 'body_sha256': body['source_span']['sha256'], 'reference_fixture_id': identity,
                       'reference_source_sha256': inv.digest(program.encode()),
                       'reference_contract_sha256': inv.digest(json.dumps(case['expected'], sort_keys=True).encode()),
                       'rationale': adaptation, 'local_execution': {k:local[k] for k in ('test_file', 'test_name', 'test_source_sha256')},
                       'support_source_sha256': support}
            phases.append(mapping)
            witnesses.extend({'assertion_id': row['id'], 'assertion_sha256': row['assertion']['sha256'], 'fixture_id': identity,
                              'rationale': adaptation} for row in inventory if row['body_id'] == body['id'] and row['disposition'] == 'in-scope' and row['id'] not in existing_sites)
        case['complete_body_phase_mappings'] = [mapping for mapping in phases if mapping['reference_fixture_id'] == identity]
        if label == 'tagged-source':
            case['compiler_data_readers'] = [{'tag': 'cljs/tag', 'operation': 'identity'}]
            case['local_execution_boundary'] = 'The public compiler embedding installs the original reader operation before source reading. The ordinary CLI cannot install this capability; generic report integration requires an explicit hosted compiler adapter. The Rust suite executes all phases/engines, but CLI compilation is not claimed.'
        if label == 'record-literals':
            case['source_contract_review'] = {
                'status': 'retained-unwitnessed-no-blanket-exclusion',
                'canonical_companion_control': 'Both identity-checked pins pass the complete Foo setup and five original assertions with the original self require-macros/CLJC companion. Both reject the same complete local-header source without that companion with ClassNotFoundException: data_readers_test.records.Foo, before the original assertions execute.',
                'local_observation': 'Both ordinary and actual-macro source reading reject the first original record tag with located UnknownTag; no record assertion executes.',
                'scope_basis': 'The exact unchanged-source oracle uses a JVM record class supplied by the original self require-macros companion, explicitly excluded by the adopted group scope. This control cannot establish an automatic JVM-free record-tag constructor facility. Ordinary tagged-reader capabilities and defrecord/constructor/record value semantics remain intended; retain the source sites unresolved until the adopted local constructor-reader contract is reconciled. No original assertion is retired or counted as passing.',
                'reference_receipt': '/tmp/light-cljs-125-source-next-reference/evidence.json',
                'reference_receipt_sha256': '1febb5f675bf4222000af4130507da8f271dc76b836d5910b02d9cf9787c5bcc',
                'local_receipt': '/tmp/light-cljs-125-source-next-local/evidence.json',
                'local_receipt_sha256': '15224ccb15e53dfae48c664ee4ba071a06c22dcc3af9679a5db97fd6cdea154c',
                'companion_control_receipt': '/tmp/light-cljs-125-record-companion-control-current/evidence.json',
                'companion_control_receipt_sha256': 'b6ed1388e0b30caf0bbf31201e4b7c405cac2983e10dc030ff98d97d1b5a7637',
                'reproduce_control': 'python3 -B tests/reference/compiler-residual-source-context.py --audit-record-companion --output /tmp/light-cljs-125-record-companion-control',
            }
        cases.append(case)
    pending_reasons = {
        PREFIX + 'record-literals': 'All five original record literal assertions and complete Foo setup are retained. No implicit record-tag constructor or JVM companion implementation is supplied locally. This source-contract candidate grants no passing assertion or phase witness; reconcile the ordinary record-reader obligation separately from the excluded JVM companion facility.',
    }
    pending = set(pending_reasons)
    fixture = '# Copyright (c) Rich Hickey. Eclipse Public License 1.0.\n# Complete original namespace contexts executed in both phases; unresolved source retained in proposals.\n# id\tentry\tsource root\tentry path\tcomplete original provenance\n' + ''.join('\t'.join(row) + '\n' for row in rows if row[0] not in pending)
    completion.persist(ROOT / FIXTURE, fixture)
    for row in phases:
        row['support_source_sha256'][FIXTURE] = inv.digest(fixture.encode())
    for case in cases:
        if case['id'] in pending:
            case['unwitnessed_reason'] = pending_reasons[case['id']]
            case['proposed_complete_body_phase_mappings'] = case.pop('complete_body_phase_mappings')
    embedding = {PREFIX + 'tagged-source'}
    for case in cases:
        if case['id'] in embedding:
            case['proposed_original_assertion_witnesses'] = [row for row in witnesses if row['fixture_id'] == case['id']]
            case.update(hosted.register(case))
            case['local_execution_boundary'] = 'Exact configured-reader profile is executed by hosted_source_dispatch with eight current raw application/actual-MacroHost fresh/reused Node/QuickJS frames. Generic CLI UnknownTag remains a separate boundary.'
    hosted_cases = hosted.ordinary_profiles()
    for case in hosted_cases:
        for origin in case['complete_original_provenance']:
            body = next(row for row in bodies if row['id'] == origin['body_id'])
            assert body['source_span']['sha256'] == origin['source_sha256']
            sites = [row for row in inventory if row['body_id'] == body['id'] and row['disposition'] == 'in-scope']
            assert not any(row['id'] in existing_sites for row in sites)
            witnesses.extend({'assertion_id': row['id'], 'assertion_sha256': row['assertion']['sha256'],
                              'fixture_id': case['id'], 'rationale': case['reference_adaptation']} for row in sites)
    cases.extend(hosted_cases)
    proposal = {'full-cases.jsonl': [case for case in cases if case['id'] not in pending],
                'full-witnesses.jsonl': [row for row in witnesses if row['fixture_id'] not in pending],
                'complete-body-phase-mappings': [row for row in phases if row['reference_fixture_id'] not in pending | embedding],
                'unwitnessed-source-cases': [case for case in cases if case['id'] in pending],
                'embedding-source-cases': [case for case in cases if case['id'] in embedding]}
    operations, operation_phases, operation_reviews, history = context_operation_proposals(cases)
    proposal['obligation-witnesses.jsonl'] = operations
    proposal['phase-witnesses.jsonl'] = operation_phases
    proposal['complete-context-operation-reviews'] = operation_reviews
    proposal['superseded-context-operation-mappings'] = history
    completion.persist(ROOT / 'tests/reference/compiler-residual-source-context-proposals.json', json.dumps(proposal, indent=2) + '\n')
    if catalogs:
        for name, field in [('full-cases.jsonl', 'id'), ('full-witnesses.jsonl', 'fixture_id')]:
            residual.update_owned(name, lambda row: row.get(field, '').startswith(PREFIX) or row.get(field) in hosted.PROFILES, proposal[name])
        reconcile_exists(cases)
        reconcile_edn_read(cases)
        for name in ('obligation-witnesses.jsonl', 'phase-witnesses.jsonl'):
            targets = {row['obligation_id'] for row in proposal[name]}
            residual.update_owned(name, lambda row: row['obligation_id'] in targets, proposal[name])
    print(f'{len(proposal["full-cases.jsonl"])} supported contexts, {len(proposal["complete-body-phase-mappings"])} complete bodies, {len(proposal["full-witnesses.jsonl"])} original sites proposed; {len(hosted.PROFILES)} explicit hosted profiles registered; {len(pending)} complete sources remain unwitnessed', flush=True)
    return cases


def context_operation_proposals(cases):
    """Anchor complete contexts to exact executed public signatures, not sites.

    The imported letfn declaration has an unresolved signature and is never
    used here. Its actual defmacro declaration has the reviewed fnspecs/body
    signature. apply::arity-3 names the third declaration branch (four actual
    arguments); its ordinal is not a call-argument count.
    """
    specs = [
        ('apply', 'api::src/main/cljs/cljs/core.cljs::apply::arity-3',
         {'kind': 'fixed', 'minimum': 4, 'parameters': ['f', 'x', 'y', 'args']},
         'apply_test.cljs::single-arity-variadic-test',
         '(apply f 0 1 (range 2 10))',
         'Each of the six original function candidates executes this four-argument apply call inside the complete original doseq, asserting the literal range 0 through 9. The separate original multi-arity-variadic doseq repeats the call across all six candidates. All six complete active apply bodies, both original setup forms, every other argument path, errors and eager messages remain in the namespace oracle.'),
        ('binding', 'api::src/main/clojure/cljs/core.cljc::binding::arity-1',
         {'kind': 'variadic', 'minimum': 1, 'parameters': ['bindings', '&', 'body']},
         'binding_test.cljs::test-binding',
         '(binding [o/*foo* 2]\n        (= o/*foo* 2))',
         'The complete original test-binding body executes binding with its original vector and body, asserts the cross-namespace value is 2 inside and restored to 1 afterward. The complete original parallel binding body asserts initializer visibility remains 2. The complete companion namespace and every original local setup form remain; separate redefs bodies also execute unchanged.'),
        ('letfn', 'api::src/main/clojure/cljs/core.cljc::letfn~2::arity-1',
         {'kind': 'variadic', 'minimum': 1, 'parameters': ['fnspecs', '&', 'body']},
         'letfn_test.cljs::test-letfn', None,
         'The complete original test-letfn body executes the actual public letfn macro with its original vector of mutually recursive ev?/od? function specs and all eight original assertion body forms. Every recursive call and literal zero/even/odd result remains in lexical scope. This maps only the resolved defmacro signature; the distinct imported letfn declaration with unresolved signature receives no credit.'),
    ]
    targets = {row['id']: row for row in residual.read('obligations.jsonl')}
    originals = {row['id']: row for row in residual.read('full-bodies.jsonl')}
    operations, phases, reviews = [], [], []
    for label, identity, signature, body_id, call_text, detail in specs:
        target = targets[identity]
        assert target['disposition'] == 'intended' and target['signature'] == signature
        declaration = target['source_span']
        assert inv.digest(declaration['text'].encode()) == target['body_sha256']
        source = pinned(target['source']).encode()
        assert source[declaration['start_byte']:declaration['end_byte']].decode() == declaration['text']
        body = originals[body_id]
        completion.original_form(body) # verifies complete original pin/byte span
        if call_text is None:
            form = inv.Reader(body['source_span']['text']).all()[0]
            call = form.children[2]
            assert inv.short_head(call) == 'letfn' and call.children[1].kind == 'vector'
            assert len(call.children) == 10 # vector plus all eight original is forms
            call_text = body['source_span']['text'][call.start:call.end]
        assert call_text in body['source_span']['text']
        call = inv.Reader(call_text).all()[0]
        assert inv.short_head(call) == label
        count = len(call.children) - 1
        assert count == signature['minimum'] if signature['kind'] == 'fixed' else count >= signature['minimum']
        case = next(row for row in cases if row['id'] == PREFIX + label)
        assert any(row['body_id'] == body_id for row in case['complete_original_provenance'])
        rationale = (detail + ' The entire namespace returns literal true with empty trace/stdout in both identity/settings-checked pins, Node/QuickJS applications and actual macro computations using fresh/reused MacroHosts. Historical assertion witnesses remain byte-identical and are not duplicated. The declaration body/signature binding is source evidence; fresh execution remains required for report coverage.')
        common = {'obligation_id': identity, 'body_sha256': target['body_sha256'],
                  'signature_sha256': inv.digest(json.dumps(signature, sort_keys=True).encode()),
                  'rationale': rationale}
        execution = {'test_file': TEST, 'test_name': TEST_NAME,
                     'test_source_sha256': inv.digest((ROOT / TEST).read_bytes())}
        operations.append({**common, 'assertion_ids': [], 'local_execution': execution,
                           'reference_fixture_id': case['id'],
                           'reference_source_sha256': inv.digest(case['source'].encode()),
                           'reference_contract_sha256': inv.digest(json.dumps(case['expected'], sort_keys=True).encode())})
        phases.append({**common, **execution,
                       'support_source_sha256': case['complete_body_phase_mappings'][0]['support_source_sha256']})
        reviews.append({'obligation_id': identity, 'declaration': target['declaration'],
                        'original_declaration_span': declaration, 'signature': signature,
                        'complete_original_test_body_id': body_id,
                        'complete_original_test_body_sha256': body['source_span']['sha256'],
                        'executed_original_call': call_text, 'actual_call_argument_count': count,
                        'reference_fixture_id': case['id'], 'review': rationale})
    # Preserve replaced evidence rather than treating the new mapping as an
    # erasure of earlier contracts. Existing self-owned rows are not history.
    path = ROOT / 'tests/reference/compiler-residual-source-context-proposals.json'
    history = json.loads(path.read_text()).get('superseded-context-operation-mappings', []) if path.exists() else []
    seen = {(row['catalog'], row['row_sha256']) for row in history}
    ids = {row['obligation_id'] for row in operations}
    for name in ('obligation-witnesses.jsonl', 'phase-witnesses.jsonl'):
        for row in residual.read(name):
            if row['obligation_id'] not in ids:
                continue
            owned = (row.get('reference_fixture_id', '').startswith(PREFIX)
                     if name == 'obligation-witnesses.jsonl' else row.get('test_file') == TEST)
            if not owned:
                digest = inv.digest(json.dumps(row, sort_keys=True).encode())
                if (name, digest) not in seen:
                    history.append({'catalog': name, 'row_sha256': digest, 'row': row})
                    seen.add((name, digest))
    return operations, phases, reviews, history


def reconcile_exists(cases):
    """Bind only the executed exists? arity and its unchanged original body."""
    target = next(row for row in residual.read('obligations.jsonl')
                  if row['id'] == 'api::src/main/clojure/cljs/core.cljc::exists?::arity-1')
    case = next(row for row in cases if row['id'] == PREFIX + 'ns-exists')
    assert target['signature'] == {'kind': 'fixed', 'minimum': 1, 'parameters': ['x']}
    rationale = ('The complete original test-cljs-2292 body retains both false/true exists? assertions '
                 'and its namespace rename/exclude/alias/use setup. The one-argument public operation '
                 'runs through the complete namespace driver in fresh/reused actual macros and '
                 'Node/QuickJS applications. Both identity-checked pinned references execute the '
                 'same complete original source contract. Additional macro-demand tests retain '
                 'undefined versus nil/false, missing/declared roots, conditional products, lexical '
                 'shadowing, nested ECMAScript paths, exactly-once effects, source visibility, '
                 'phase isolation and genuine located initializer/cycle failures.')
    common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'],
              'signature_sha256': inv.digest(json.dumps(target['signature'], sort_keys=True).encode()),
              'rationale': rationale}
    execution = {'test_file': TEST, 'test_name': TEST_NAME,
                 'test_source_sha256': inv.digest((ROOT / TEST).read_bytes())}
    phase = {**common, **execution, 'support_source_sha256':
             case['complete_body_phase_mappings'][0]['support_source_sha256']}
    operation = {**common, 'assertion_ids': [], 'local_execution': execution,
                 'reference_fixture_id': case['id'],
                 'reference_source_sha256': inv.digest(case['source'].encode()),
                 'reference_contract_sha256': inv.digest(json.dumps(case['expected'], sort_keys=True).encode())}
    for name, row in [('obligation-witnesses.jsonl', operation), ('phase-witnesses.jsonl', phase)]:
        residual.update_owned(name, lambda item: item['obligation_id'] == target['id'], [row])


def reconcile_edn_read(cases):
    """Bind the public stream operation to its complete source and phase tests."""
    target = next(row for row in residual.read('obligations.jsonl')
                  if row['name'] == 'clojure.edn/read' and row['obligation_kind'] == 'local-api')
    case = next(row for row in cases if row['id'] == PREFIX + 'edn-stream')
    test = 'tests/compiler_edn_stream.rs'
    execution = {'test_file': test,
                 'test_name': 'edn_stream_contract_executes_in_fresh_and_reused_application_and_macro_phases',
                 'test_source_sha256': inv.digest((ROOT / test).read_bytes())}
    rationale = ('The complete original test-reader helper and all three test-read assertions '
                 'execute unchanged in Node/QuickJS applications and fresh/reused actual macros, '
                 'with both identity/settings-checked reference pins. The separate strict EDN '
                 'contract retains literal cursor results, zero-argument explicit input, '
                 'one/two/four arities, exact EOF values and error-flag precedence, ordered '
                 'tag/provider effects, local reader/default precedence, malformed/unsupported '
                 'input and located host/reader failures. Unsupported macro result values '
                 'remain distinct diagnostics. The constructor namespace adaptation does '
                 'not promise general tools.reader compatibility or ambient strict readers.')
    common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'],
              'rationale': rationale}
    supports = {**case['complete_body_phase_mappings'][0]['support_source_sha256'],
                'tests/fixtures/compiler-edn-stream.cljs': inv.digest((ROOT / 'tests/fixtures/compiler-edn-stream.cljs').read_bytes()),
                'tests/fixtures/compiler-edn-stream-parity.cljs': inv.digest((ROOT / 'tests/fixtures/compiler-edn-stream-parity.cljs').read_bytes()),
                TEST: inv.digest((ROOT / TEST).read_bytes())}
    phase = {**common, **execution, 'support_source_sha256': supports}
    operation = {**common, 'assertion_ids': [], 'local_execution': execution,
                 'reference_fixture_id': case['id'],
                 'reference_source_sha256': inv.digest(case['source'].encode()),
                 'reference_contract_sha256': inv.digest(json.dumps(case['expected'], sort_keys=True).encode())}
    for name, row in [('obligation-witnesses.jsonl', operation), ('phase-witnesses.jsonl', phase)]:
        residual.update_owned(name, lambda item: item['obligation_id'] == target['id'], [row])
    canonical_path = 'src/main/cljs/clojure/edn.cljs'
    local_path = 'src/compiler_libraries/clojure/edn.cljs'
    def read_contract(source):
        form = next(node for node in inv.Reader(source).all()
                    if inv.short_head(node) == 'defn' and source[node.children[1].start:node.children[1].end] == 'read')
        text = source[form.start:form.end]
        arities = [len(node.children[0].children) for node in form.children
                   if node.kind == 'list' and node.children and node.children[0].kind == 'vector']
        return {'text': text, 'sha256': inv.digest(text.encode()), 'arities': arities}
    canonical = read_contract(pinned(canonical_path))
    local = read_contract((ROOT / local_path).read_text())
    assert canonical['arities'] == [1, 2, 4] and local['arities'] == [0, 1, 2, 4]
    receipt = {'public_name': 'clojure.edn/read', 'obligation_id': target['id'],
               'canonical_source': canonical_path, 'canonical_read': canonical,
               'local_source': local_path, 'local_read': local,
               'runtime_sha256': inv.digest((ROOT / 'src/js_runtime.js').read_bytes()),
               'original_body_reference_fixture': case['id'],
               'original_body_source_sha256': inv.digest(case['source'].encode()),
               'complete_original_provenance': case['complete_original_provenance'],
               'local_execution': execution, 'support_source_sha256': supports,
               'rationale': rationale,
               'evidence_boundary': 'Source and arity bindings are not execution results. Fresh literal reference and both-engine/phase observations are required.'}
    completion.persist(ROOT / 'tests/reference/compiler-edn-stream-contract.json', json.dumps(receipt, indent=2) + '\n')


def audit_local(cases, output):
    # Observation-only exact source probes for every selected context, including
    # restored supported sources. Failing executions are never test passes.
    output.mkdir(parents=True, exist_ok=True)
    binary = ROOT / 'target/debug/light-cljs'
    evidence = {'binary_sha256': inv.digest(binary.read_bytes()), 'observations': []}
    for case in cases:
        for macro_phase in (False, True):
            directory = output / (case['id'] + ('-macro' if macro_phase else '-application'))
            source_root = directory / 'source'
            shutil.copytree(ROOT / case['source_root'], source_root, dirs_exist_ok=True)
            relative = case['entry'].replace('.', '/').replace('-', '_') + '.cljs'
            file = source_root / relative
            source = file.read_text()
            if macro_phase:
                prefix, body = source.split('\n(defn run [] ', 1)
                source = prefix + '\n(defmacro upstream [] ' + body + '\n(defn run [] (upstream))\n'
            source += '\n(pr-str (run))\n'
            file.write_text(source)
            command = [str(binary), '--compile', '--entry', case['entry'], '--source-root', str(source_root)]
            compiled = subprocess.run(command, text=True, capture_output=True, timeout=120)
            observation = {'id': case['id'], 'macro_phase': macro_phase, 'source_sha256': inv.digest(source.encode()),
                           'command': command, 'compilation_exit': compiled.returncode, 'compilation_stderr': compiled.stderr}
            if compiled.returncode == 0:
                javascript = compiled.stdout
                (directory / 'emitted.js').write_text(javascript)
                observation['emitted_sha256'] = inv.digest(javascript.encode())
                for engine, invocation in [('node', ['node', '-e', 'console.log(' + javascript + ');']),
                                            ('quickjs', [str(binary), '--eval', '--text', '(js* ' + json.dumps(javascript) + ')'])]:
                    executed = subprocess.run(invocation, text=True, capture_output=True, timeout=120)
                    observation[engine] = {'exit': executed.returncode, 'stdout': executed.stdout, 'stderr': executed.stderr}
            evidence['observations'].append(observation)
    (output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print([(row['id'], row['macro_phase'], row['compilation_exit'], {engine: row.get(engine, {}).get('exit') for engine in ('node', 'quickjs')}) for row in evidence['observations']], flush=True)


def audit_record_companion(cases, output):
    """Observe both original and no-companion sources; failures stay failures."""
    output.mkdir(parents=True, exist_ok=True)
    original = next(case for case in cases if case['id'] == PREFIX + 'record-literals')
    root = output.resolve() / 'without-companion-source'
    file = root / 'data_readers_test/records.cljs'
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(original['source'])
    control = {key: value for key, value in original.items()
               if key not in ('reference_source_root', 'reference_files_sha256')}
    control['id'] = PREFIX + 'record-literals-without-companion-control'
    control['source_root'] = str(root)
    control['expected'] = {**original['expected'], 'id': control['id']}
    selected = [original, control]
    evidence = {'cases': selected, 'references': {},
                'boundary': 'The no-companion control omits the original JVM self-require under the explicit companion boundary and retains every original setup/assertion. Rejection supplies scope evidence only; it never counts as a semantic pass.'}
    for kind in ('release', 'checkout'):
        evidence['references'][kind] = report.reference_cases(
            selected, kind, ROOT / 'research/clojurescript', output / kind, 240)
        print(kind, [(key, row['status']) for key, row in evidence['references'][kind].items()], flush=True)
    (output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')


def retain_boundary_review():
    bodies = {body['id']: body for body in residual.read('full-bodies.jsonl')}
    reviewed = [
        ('src/test/cljs/cljs/predicates_test.cljs::test-int-preds',
         ['src/test/cljs/cljs/predicates_test.cljs::int-val-table'],
         'mixed-native-number-and-closure-inputs',
         'Original fourteen-row/eight-column table mixes eight JavaScript Number rows with six goog.math.Long/Integer rows. LongImpl is an upstream INTERNAL alias; Long/Integer are Closure library objects, not native Number or explicit JS BigInt inputs. The adopted JS-number basis and excluded Closure ecosystem do not supply these classes or their int? branches. Preserve the entire table and source assertion as unresolved; never truncate rows, coerce representations, or classify the entire predicate body outside. Core native-number predicates remain intended and require independent executed oracles.'),
        ('core_test.cljs::test-cljs-2549', [], 'private-storage-observation',
         'Both original assertions inspect the exact private cljs.core/tapset atom and its set representation after add-tap. The group excludes private storage representations. Public add-tap/remove-tap/tap> remain intended; public delivery tests cannot witness these original private-storage assertions. Do not invent or expose a tapset adapter. This review records a private source-site boundary basis for the main owner, without changing the ledger.'),
        ('src/test/cljs/cljs/inference_test.cljs::test-cljs-2825', [], 'compiler-emission-instrumentation',
         'Original JVM inference-util helper expands every predicate into an if test while instrumenting emitted cljs.core/truth_; it checks whether the upstream compiler emits that private coercion call, not the predicate returned values. Preserve all forms, the complete .clj helper and cljs.pprint dependency. Upstream emitted helper spelling/optimizer shape, JVM macro companions and general pprint are excluded. Public predicates remain intended and need ordinary and macro execution elsewhere; substituted predicate outcomes do not cover this body.'),
        ('src/test/cljs/cljs/parse_test.cljs::test-gen-parse-long', [], 'unprovided-generative-test-library',
         'The exact body requests 100000 test.check large-integer generator/property/shrinker trials. General test.check is outside the promised library set, while parse-long remains intended. The body stays unwitnessed; no reduced trial count, canned inputs, fake generator or sampled pass is proposed.'),
        ('src/test/cljs/cljs/parse_test.cljs::test-gen-parse-double', [], 'unprovided-generative-test-library',
         'The exact body requests 100000 test.check double generator/property/shrinker trials. General test.check is outside the promised library set, while parse-double remains intended. The body stays unwitnessed; no reduced trial count, canned inputs, fake generator or sampled pass is proposed.'),
        ('src/test/cljs/clojure/string_test.cljs::test-cljs-2300', [], 'unprovided-generative-test-library',
         'The unchanged defspec depends on complete test.check generators/properties and retains its original reference implementation. General test.check is outside the promised library set; supported string semantics remain intended and this original body stays unwitnessed.'),
        ('src/test/cljs/cljs/eval_test.cljs::test-eval',
         ['src/test/cljs/cljs/eval_test.cljs::addition-list-1', 'src/test/cljs/cljs/eval_test.cljs::addition-list-2',
          'src/test/cljs/cljs/eval_test.cljs::addition-list-3', 'src/test/cljs/cljs/eval_test.cljs::square', 'src/test/cljs/cljs/eval_test.cljs::cube'],
         'requires-explicit-eval-embedding',
         'The exact module states it should only load in environments setting cljs.core/*eval*. All fourteen assertions and live function/Var-containing forms remain intended embedding evidence under the compiler-service contract; standalone JS or default reference compilation has no such hook. No fabricated eval implementation or compiler-only pass is proposed.'),
    ]
    rows = []
    for identity, setup, basis, explanation in reviewed:
        originals = [bodies[name] for name in setup + [identity]]
        for body in originals:
            completion.original_form(body) # verify exact pinned source span
        module = pinned(bodies[identity]['source'])
        namespace = next((body['source_span'] for body in bodies.values()
                          if body['source'] == bodies[identity]['source'] and body['form'] == 'ns'), None)
        row = {'body_id': identity, 'basis': basis, 'review': explanation,
               'status': 'retained-unwitnessed-review-only', 'ledger_mutated': False,
               'module_source': bodies[identity]['source'], 'module_sha256': inv.digest(module.encode()),
               'original_namespace': namespace,
               'complete_original_forms': [{'body_id': body['id'], 'source_span': body['source_span']} for body in originals],
               'original_assertion_ids': bodies[identity]['assertions']}
        if basis == 'compiler-emission-instrumentation':
            helper = pinned('src/test/cljs/cljs/inference_util.clj')
            row['complete_original_jvm_helper'] = {'source': 'src/test/cljs/cljs/inference_util.clj', 'sha256': inv.digest(helper.encode()), 'text': helper}
        rows.append(row)
    invoke = pinned('src/test/cljs/cljs/invoke_test.cljs')
    rows.append({'program_id': 'program::src/test/cljs/cljs/invoke_test.cljs',
                 'basis': 'canonical-initialization-throws', 'status': 'retained-unwitnessed-review-only', 'ledger_mutated': False,
                 'original_source': invoke, 'original_source_sha256': inv.digest(invoke.encode()),
                 'review': 'Both pinned unchanged modules throw TypeError at declared-fn invocation before later record forms execute. Declared-fn is never defined. Preserve the complete source and initialization receipts; a canonical throw is neither a local execution pass nor a reason to remove this source-program obligation. The complete-module observation remains separate from independently executed public invocation behavior.'})
    completion.persist(ROOT / 'tests/reference/compiler-residual-source-boundary-review.json', json.dumps(rows, indent=2) + '\n')


if __name__ == '__main__':
    retain_boundary_review()
    cases = generate('--catalogs' in sys.argv)
    if '--case' in sys.argv:
        selected_id = PREFIX + sys.argv[sys.argv.index('--case') + 1]
        cases = [case for case in cases if case['id'] == selected_id]
        assert len(cases) == 1
    if '--audit-record-companion' in sys.argv:
        output = Path(sys.argv[sys.argv.index('--output') + 1]) if '--output' in sys.argv else Path('/tmp/light-cljs-125-record-companion-control')
        audit_record_companion(cases, output)
    if '--audit-local' in sys.argv:
        output = Path(sys.argv[sys.argv.index('--output') + 1]) if '--output' in sys.argv else Path('/tmp/light-cljs-125-shadow-source-local')
        audit_local(cases, output)
    if '--run' in sys.argv:
        output = Path(sys.argv[sys.argv.index('--output') + 1]) if '--output' in sys.argv else Path('/tmp/light-cljs-125-residual-source-context-reference')
        evidence = {'cases': cases, 'references': {}}
        for kind in ('release', 'checkout'):
            evidence['references'][kind] = report.reference_cases(cases, kind, ROOT / 'research/clojurescript', output / kind, 240)
            print(kind, [(key, value['status']) for key, value in evidence['references'][kind].items()], flush=True)
        evidence['source_sha256'] = {case['id']: inv.digest(case['source'].encode()) for case in cases}
        evidence['support_source_sha256'] = {name: inv.digest((ROOT / name).read_bytes()) for name in (TEST, FIXTURE, 'tests/reference/compiler-residual-source-context.py')}
        output.mkdir(parents=True, exist_ok=True)
        (output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
        assert all(value['status'] == 'passed' for reference in evidence['references'].values() for value in reference.values())
