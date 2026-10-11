#!/usr/bin/env python3
"""Preserve complete residual bodies with the namespace context they observe.
Copyright (c) Rich Hickey. Eclipse Public License 1.0; retain original notices.
"""
from pathlib import Path
import importlib.util
import json
import shutil
import subprocess
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/reference/compiler-language'))
import extract_inventory as inv
import report
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
]
RATIONALE = ('Complete original body, every relevant original setup form, namespace identity, aliases, literal expectations and effects retained. cljs.test harness imports are removed only after is/deftest become executing assertions; reader-only cljs.test alias uses an empty local namespace marker, while references use the actual library. Original source-macro .clj modules are byte-identical as local .cljs modules and remain .clj for the reference JVM macro compiler; local ordinary require and canonical require-macros differ explicitly, with the original use-macros :only becoming the equivalent refer import. Unused macro_test macro imports are omitted only for its independent js-keys body, which calls none of them. These contexts establish these complete bodies, not general cljs.test or unchanged .clj companion compatibility.')


def pinned(path):
    data = subprocess.check_output(['git', '-C', str(ROOT / 'research/clojurescript'), 'show', inv.REVISION + ':' + path])
    assert (ROOT / 'research/clojurescript' / path).read_bytes() == data
    return data.decode()


def generate(catalogs=False):
    bodies = residual.read('full-bodies.jsonl')
    inventory = residual.read('full-inventory.jsonl')
    rows, cases, witnesses, phases = [], [], [], []
    for label, path, selected, namespace, reference_namespace in SPECS:
        original = pinned(path)
        original_bodies = [body for body in bodies if body['source'] == path]
        wanted = [body for body in original_bodies if body['name'] in selected and body['form'] == 'deftest']
        assert len(wanted) == len(selected)
        setup = [body for body in original_bodies if body['form'] not in ('ns', 'deftest')]
        # macro_test's other macro tests have no setup forms in this .cljs file.
        forms, origins = [], []
        for body in setup + wanted:
            text = body['source_span']['text']
            assert inv.digest(text.encode()) == body['source_span']['sha256']
            span = body['source_span']
            assert original.encode()[span['start_byte']:span['end_byte']].decode() == text
            forms.append(residual.adapt(text, inv.Reader(text).all()[0]))
            origins.append({'body_id': body['id'], 'source_sha256': span['sha256']})
        expression = '(do ' + ' '.join(forms[len(setup):]) + ')'
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
        if label.endswith('shadow'):
            dependencies['baz.cljs'] = pinned('src/test/cljs/baz.cljs')
        if label == 'syntax-quote':
            # The test observes only qualified symbols. No test function or
            # test macro is called from this reader context marker.
            dependencies['cljs/test.cljs'] = completion.NOTICE + '(ns cljs.test)\n'
        for relative, text in dependencies.items():
            completion.persist(ROOT / source_root / relative, text)
            support[source_root + '/' + relative] = inv.digest(text.encode())
            if relative not in ('cljs/test.cljs', 'cljs/keyword_macros.cljs') and label != 'macro-source':
                completion.persist(ROOT / reference_root / relative, text)
                reference_files[relative] = inv.digest(text.encode())
        identity = PREFIX + label
        provenance = {'module_source': path, 'module_sha256': inv.digest(original.encode()), 'complete_original_provenance': origins,
                      'namespace_source_sha256': next(body['source_span']['sha256'] for body in original_bodies if body['form'] == 'ns')}
        rows.append([identity, entry, source_root, relpath, json.dumps(provenance, separators=(',', ':'))])
        case = {'id': identity, 'source': program, 'source_root': source_root, 'reference_source_root': reference_root,
                'entry': entry, 'export': 'run', 'reference_files_sha256': reference_files,
                'reference_adaptation': RATIONALE, **provenance,
                'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}, 'native_stdout': None}
        local = {'test_file': TEST, 'test_name': TEST_NAME, 'test_source_sha256': inv.digest((ROOT / TEST).read_bytes()),
                 'support_source_sha256': support}
        for body in wanted:
            mapping = {'body_id': body['id'], 'body_sha256': body['source_span']['sha256'], 'reference_fixture_id': identity,
                       'reference_source_sha256': inv.digest(program.encode()),
                       'reference_contract_sha256': inv.digest(json.dumps(case['expected'], sort_keys=True).encode()),
                       'rationale': RATIONALE, 'local_execution': {k:local[k] for k in ('test_file', 'test_name', 'test_source_sha256')},
                       'support_source_sha256': support}
            phases.append(mapping)
            witnesses.extend({'assertion_id': row['id'], 'assertion_sha256': row['assertion']['sha256'], 'fixture_id': identity,
                              'rationale': RATIONALE} for row in inventory if row['body_id'] == body['id'] and row['disposition'] == 'in-scope')
        case['complete_body_phase_mappings'] = [mapping for mapping in phases if mapping['reference_fixture_id'] == identity]
        cases.append(case)
    pending = {PREFIX + 'top-level'}
    fixture = '# Copyright (c) Rich Hickey. Eclipse Public License 1.0.\n# Only executed supported contexts; unresolved complete sources remain in proposals.\n# id\tentry\tsource root\tentry path\tcomplete original provenance\n' + ''.join('\t'.join(row) + '\n' for row in rows if row[0] not in pending)
    completion.persist(ROOT / FIXTURE, fixture)
    for row in phases:
        row['support_source_sha256'][FIXTURE] = inv.digest(fixture.encode())
    for case in cases:
        if case['id'] in pending:
            case['unwitnessed_reason'] = ('Complete original top-level let/defn setup passes ordinary execution but fails local macro declaration demand.' if case['id'].endswith('top-level') else 'Complete original namespace/function baz shadowing source currently fails local execution.') + ' Both-pin source comparison and focused ownership required. No pass or boundary exclusion is claimed.'
            case['proposed_complete_body_phase_mappings'] = case.pop('complete_body_phase_mappings')
    proposal = {'full-cases.jsonl': [case for case in cases if case['id'] not in pending],
                'full-witnesses.jsonl': [row for row in witnesses if row['fixture_id'] not in pending],
                'complete-body-phase-mappings': [row for row in phases if row['reference_fixture_id'] not in pending],
                'unwitnessed-source-cases': [case for case in cases if case['id'] in pending]}
    completion.persist(ROOT / 'tests/reference/compiler-residual-source-context-proposals.json', json.dumps(proposal, indent=2) + '\n')
    if catalogs:
        for name, field in [('full-cases.jsonl', 'id'), ('full-witnesses.jsonl', 'fixture_id')]:
            residual.update_owned(name, lambda row: row.get(field, '').startswith(PREFIX), proposal[name])
    print(f'{len(proposal["full-cases.jsonl"])} supported contexts, {len(proposal["complete-body-phase-mappings"])} complete bodies, {len(proposal["full-witnesses.jsonl"])} original sites proposed; {len(pending)} complete sources remain unwitnessed', flush=True)
    return cases


def audit_local(cases, output):
    # Observation-only exact source probes. Failing executions are not test
    # passes and are never included among the registered witness proposals.
    output.mkdir(parents=True, exist_ok=True)
    binary = ROOT / 'target/debug/light-cljs'
    evidence = {'binary_sha256': inv.digest(binary.read_bytes()), 'observations': []}
    for case in cases:
        if 'unwitnessed_reason' not in case:
            continue
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
    if '--audit-local' in sys.argv:
        output = Path(sys.argv[sys.argv.index('--output') + 1]) if '--output' in sys.argv else Path('/tmp/light-cljs-125-shadow-source-local')
        audit_local(cases, output)
    if '--run' in sys.argv:
        if '--case' in sys.argv:
            selected_id = PREFIX + sys.argv[sys.argv.index('--case') + 1]
            cases = [case for case in cases if case['id'] == selected_id]
            assert len(cases) == 1
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
