#!/usr/bin/env python3
"""Execute focused public prim-seq contracts on both identity-checked CLJS pins.

Pinned source excerpts are Copyright (c) Rich Hickey, EPL-1.0; see
research/clojurescript/epl-v10.html. Only three focused public/local witness
bindings are reconciled; this runner does not promote the broad language gate.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import subprocess
import zipfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / 'tests/reference/compiler-language'
sys.path.insert(0, str(LEDGER))
import extract_inventory as inv
import report


def sha(data):
    return hashlib.sha256(data).hexdigest()


def case(identity, source, value=True):
    return {'id': identity, 'source': ' '.join(source.splitlines()),
            'expected': {'id': identity, 'outcome': {'status': 'returned', 'value': value},
                         'trace': [], 'stdout': ''}}


def focused_cases():
    return [
        case('prim-seq-invocation-arities', '''
          (let [a #js [4 5] f prim-seq q cljs.core/prim-seq c clojure.core/prim-seq]
            (= '[[4 5] [5] [4 5] [5] [4 5] [5] [4 5] [5] [4 5] [5]
                 [4 5] [5] [4 5] [5] [4 5] [5]]
               [(vec (prim-seq a)) (vec (prim-seq a 1))
                (vec (cljs.core/prim-seq a)) (vec (cljs.core/prim-seq a 1))
                (vec (clojure.core/prim-seq a)) (vec (clojure.core/prim-seq a 1))
                (vec (f a)) (vec (f a 1)) (vec (q a)) (vec (q a 1))
                (vec (c a)) (vec (c a 1))
                (vec (apply prim-seq [a])) (vec (apply prim-seq [a 1]))
                (vec (apply f [a])) (vec (apply f [a 1]))]))'''),
        case('prim-seq-arraylike-and-utf16', '''
          (let [o (js-obj "0" 4 "1" 5 "2" 6 "length" 3)
                text (str (js/String.fromCharCode 55357 56832) "x")
                units (fn [s] (vec (map (fn [x] (.charCodeAt x 0)) s)))]
            (and (= '[4 5 6] (vec (prim-seq o)))
                 (= '[5 6] (vec (prim-seq o 1)))
                 (= '[4 nil 6] (vec (prim-seq #js [4 nil 6])))
                 (= '[55357 56832 120] (units (prim-seq text)))
                 (= '[56832 120] (units (prim-seq text 1)))
                 (= 3 (count (prim-seq text)))
                 (= "b" (first (prim-seq "ab" 1)))
                 (nil? (prim-seq (js-obj "0" 4)))
                 (nil? (prim-seq (js-obj "length" 0)))
                 (nil? (prim-seq ""))))'''),
        case('prim-seq-boundaries-and-uncoerced-offsets', '''
          (let [a #js [4 5] n (prim-seq a -1) f (prim-seq a 0.5)
                z (prim-seq a nil) q (prim-seq a 1)]
            (and (nil? (prim-seq a 2)) (nil? (prim-seq a 3))
                 (nil? (prim-seq #js [])) (nil? (prim-seq #js [] 0))
                 (instance? cljs.core/IndexedSeq n)
                 (= 3 (count n)) (nil? (first n))
                 (= 4 (nth n 1)) (= :missing (nth n 0 :missing))
                 (= "Index out of bounds" (try (nth n 0) (catch js/Error e (.-message e))))
                 (= '[4 5] (vec (rest n)))
                 (= 1.5 (count f)) (nil? (first f)) (nil? (nth f 0))
                 (= 0.5 (count (rest f))) (nil? (first (rest f)))
                 (nil? (next (rest f))) (empty? (rest (rest f)))
                 (= 2 (count z)) (nil? (first z)) (= 4 (nth z 0))
                 (= 5 (first (rest z)))
                 (= 4 (nth q -1)) (= :missing (nth q -2 :missing))))'''),
        case('prim-seq-fractional-properties', '''
          (let [o (js-obj "-1" 3 "0" 4 "1" 5 "0.5" 7 "1.5" 8 "length" 2)
                n (prim-seq o -1) f (prim-seq o 0.5)]
            (and (= 3 (first n)) (= 4 (first (rest n)))
                 (= :missing (nth n 0 :missing))
                 (= 7 (first f)) (= 8 (first (rest f)))
                 (= 7 (nth f 0)) (= 8 (nth f 1))
                 (= :missing (nth f 2 :missing))
                 (= 1.5 (count f)) (nil? (next (rest f)))))'''),
        case('prim-seq-indexedseq-protocols', '''
          (let [s (prim-seq #js [4 5 6] 1) m (with-meta s {:tag true})
                tail (next m) last-seq (prim-seq #js [9])
                effects (atom [])
                sum (reduce (fn [acc x] (swap! effects conj x) (+ acc x)) 0 s)
                early (reduce (fn [acc x] (swap! effects conj x) (reduced (+ acc x))) 0 s)]
            (and (instance? cljs.core/IndexedSeq s)
                 (seq? s) (sequential? s) (counted? s) (indexed? s) (reversible? s)
                 (identical? s (seq s)) (nil? (meta s))
                 (= {:tag true} (meta m)) (= '[5 6] (vec m))
                 (nil? (meta tail)) (nil? (meta (rest m)))
                 (nil? (next tail)) (list? (rest tail)) (empty? (rest tail))
                 (nil? (next last-seq)) (list? (rest last-seq))
                 (= 5 (first s)) (= 6 (nth s 1)) (= :missing (nth s 2 :missing))
                 (= "Index out of bounds" (try (nth s 2) (catch js/Error e (.-message e))))
                 (= '(5 6) s) (= s '(5 6)) (= (hash '(5 6)) (hash s))
                 (= '(3 5 6) (conj s 3)) (empty? (empty m)) (nil? (meta (empty m)))
                 (= '[6 5] (vec (rseq s))) (= 11 (reduce + s))
                 (= 11 sum) (= 5 early) (= '[5 6 5] @effects)))'''),
        case('prim-seq-evaluation-once-and-live-array', '''
          (let [effects (atom []) a #js [4 5]
                p (fn [] (swap! effects conj :primitive) a)
                i (fn [] (swap! effects conj :offset) 1)
                f prim-seq
                s (prim-seq (p) (i))
                t (f (p))
                u (apply f [(p) (i)])
                v (prim-seq (p) (do (swap! effects conj :end) 2))]
            (aset a 1 9)
            (and (= '[:primitive :offset :primitive :primitive :offset :primitive :end] @effects)
                 (= 9 (first s)) (= '[4 9] (vec t)) (= 9 (first u)) (nil? v)
                 (= 2 (count t))
                 (do (.push a 10) (= '[9 10] (vec s)))
                 (= 2 (count s))))'''),
        case('prim-seq-var-metadata', '''
          (let [v #'prim-seq m (meta v)]
            (and (identical? prim-seq @v)
                 (= 'prim-seq (:name m)) (= 'cljs.core (:ns m))
                 (= "Create seq from a primitive JavaScript Array-like." (:doc m))
                 (= '([prim] [prim i]) (:arglists m))
                 (= "cljs/core.cljs" (:file m)) (= PRIM_SEQ_LINE (:line m))))'''),
    ]


def reconcile(write):
    """Bind only prim-seq's two public arities and local metadata obligation."""
    read = lambda name: [json.loads(line) for line in (LEDGER / name).read_text().splitlines() if line]
    test = 'tests/compiler_prim_seq.rs'
    support = 'tests/fixtures/compiler-prim-seq.cljs'
    execution = {'test_file': test,
                 'test_name': 'public_prim_seq_runs_full_literal_contract_in_applications_and_macros',
                 'test_source_sha256': sha((ROOT / test).read_bytes())}
    rationale = ('The complete literal prim-seq contract executes public fixed arities 1 and 2 '
                 'through direct, qualified, saved and apply calls in QuickJS/Node applications '
                 'and fresh/reused canonical macro hosts. It retains arrays, JavaScript array-like '
                 'objects, UTF-16 string units, empty/end/past-end, negative/fractional/nil offsets, '
                 'IndexedSeq nominal/slot/protocol identity, metadata, first/rest/next/count/nth, '
                 'reverse/hash/equality/reduce/iterator semantics, live array mutation, exactly-once '
                 'ordered operand effects and literal Var source/doc/arglists metadata. '
                 'Both pinned references independently execute the complete contract; the release '
                 'source-location assertion is explicitly 1751 rather than checkout/local 1752.')
    targets = [row for row in read('obligations.jsonl')
               if row['name'] == 'prim-seq' and row['disposition'] == 'intended']
    assert len(targets) == 3 and {row['obligation_kind'] for row in targets} == {'public-api', 'local-api'}
    mappings, phases = [], []
    for target in targets:
        common = {'obligation_id': target['id'], 'body_sha256': target['body_sha256'], 'rationale': rationale}
        if target.get('signature'):
            common['signature_sha256'] = sha(json.dumps(target['signature'], sort_keys=True).encode())
            assert sha(target['source_span']['text'].encode()) == target['body_sha256']
        mappings.append({**common, 'local_execution': execution, 'assertion_ids': []})
        phases.append({**common, **execution, 'support_source_sha256': {support: sha((ROOT / support).read_bytes())}})
    for name, rows in [('obligation-witnesses.jsonl', mappings), ('phase-witnesses.jsonl', phases)]:
        path = LEDGER / name
        identities = {row['obligation_id'] for row in rows}
        if write:
            retained = [line for line in path.read_text().splitlines()
                        if json.loads(line)['obligation_id'] not in identities]
            path.write_text('\n'.join(retained + [json.dumps(row, separators=(',', ':')) for row in rows]) + '\n')
        else:
            current = {row['obligation_id']: row for row in read(name)}
            for row in rows:
                assert current.get(row['obligation_id']) == row, (name, row['obligation_id'])
    print('Three focused prim-seq public/local execution mappings verified', flush=True)
    return {test: execution['test_source_sha256'], support: sha((ROOT / support).read_bytes())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true', help='Reconcile exactly three focused witness rows')
    parser.add_argument('--output', '--artifacts', type=Path, help='Fresh directory for actual both-pin compilation/execution')
    parser.add_argument('--local-binary', type=Path, help='Also execute both original workflows in local applications/macros')
    args = parser.parse_args()
    local_witnesses = reconcile(args.write)
    if not args.output:
        return
    args.output.mkdir(parents=True, exist_ok=False)
    original_path = ROOT / 'tests/reference/compiler-core-unresolved-probes.json'
    original_bytes = original_path.read_bytes()
    original = json.loads(original_bytes)
    probe = original['probe']
    # Execute the exact original literal expression as well as its unchanged
    # historical equality wrapper and both complete independent workflows.
    literal = case('prim-seq-original-literal', probe['source'],
                   {'$vector': [{'$vector': [4, 5]}, {'$vector': [5]}, None, None,
                                {'$vector': ['a', 'b']}, {'$vector': ['b']}]})
    assert literal['source'] == probe['source']
    originals = [literal, original['reference_probe'], *original['original_full_independent_workflows']]
    cases = [*originals, *focused_cases()]
    receipt = {'owner': 'light-cljs-157', 'scope': 'focused public prim-seq arities 1 and 2; no broad gate promotion',
               'original_probe_file': str(original_path.relative_to(ROOT)),
               'original_probe_file_sha256': sha(original_bytes),
               'original_literal_source_sha256': sha(probe['source'].encode()),
               'original_literal_expected_edn': probe['expected_edn'],
               'original_full_independent_workflows': original['original_full_independent_workflows'],
               'cases': cases, 'complete_upstream_declarations': {}, 'references': {},
               'local_witnesses': local_witnesses}
    if args.local_binary:
        binary = args.local_binary.resolve()
        local = {'binary_sha256': sha(binary.read_bytes()), 'results': []}
        for workflow in original['original_full_independent_workflows']:
            for phase in ['application', 'macro']:
                source = workflow['source'] if phase == 'application' else (
                    '(defmacro original-witness [] ' + workflow['source'] + ') (original-witness)')
                process = subprocess.run([str(binary), '--eval', '--text', source],
                                         capture_output=True, text=True, timeout=60)
                row = {'id': workflow['id'], 'phase': phase, 'source': source,
                       'source_sha256': sha(source.encode()), 'exit': process.returncode,
                       'stdout': process.stdout, 'stderr': process.stderr}
                local['results'].append(row)
                assert process.returncode == 0 and process.stdout == workflow['native_stdout'] and not process.stderr, row
        receipt['original_workflows_local_execution'] = local
        print('Both complete original workflows passed local applications and macros', flush=True)
    metadata_row = next(line for line in (ROOT / 'src/core_var_metadata.tsv').read_text().splitlines()
                        if line.startswith('prim-seq\t'))
    receipt['local_metadata_provenance'] = {'source': 'src/core_var_metadata.tsv', 'row': metadata_row,
                                           'row_sha256': sha(metadata_row.encode())}
    obligations = [json.loads(line) for line in (LEDGER / 'obligations.jsonl').read_text().splitlines()]
    public = [row for row in obligations if row['name'] == 'prim-seq' and row['obligation_kind'] == 'public-api']
    assert [row['signature']['minimum'] for row in public] == [1, 2]
    receipt['focused_arity_receipts'] = [{'obligation_id': row['id'], 'body_sha256': row['body_sha256'],
        'signature': row['signature'], 'signature_sha256': sha(json.dumps(row['signature'], sort_keys=True).encode()),
        'reference_case_ids': [c['id'] for c in cases]} for row in public]
    failures = {}
    for kind in ['release', 'checkout']:
        if kind == 'release':
            jar = Path.home() / '.m2/repository/org/clojure/clojurescript/1.12.134/clojurescript-1.12.134.jar'
            with zipfile.ZipFile(jar) as archive:
                data = archive.read('cljs/core.cljs')
            origin = 'Maven org.clojure/clojurescript 1.12.134 cljs/core.cljs'
        else:
            data = (ROOT / 'research/clojurescript/src/main/cljs/cljs/core.cljs').read_bytes()
            origin = inv.REVISION + ' src/main/cljs/cljs/core.cljs'
        text = data.decode()
        bodies = []
        for node in inv.Reader(text).all():
            if node.kind == 'list' and len(node.children) > 1 and node.children[1].token in {'prim-seq', 'IndexedSeq', 'IndexedSeqIterator'}:
                body = text[node.start:node.end]
                bodies.append({'name': node.children[1].token, 'source': origin, 'text': body,
                               'sha256': sha(body.encode()), 'start_line': text.count('\n', 0, node.start) + 1})
        declaration = next(body for body in bodies if body['name'] == 'prim-seq')
        assert declaration['sha256'] == original['original_declaration']['body_sha256']
        receipt['complete_upstream_declarations'][kind] = bodies
        pin_cases = [{**c, 'source': c['source'].replace('PRIM_SEQ_LINE', str(declaration['start_line']))}
                     for c in cases]
        contract_path = ROOT / 'tests/fixtures/compiler-prim-seq.cljs'
        contract = contract_path.read_text()
        if declaration['start_line'] != 1752:
            assert contract.count('(= 1752 (:line metadata))') == 1
            contract = contract.replace('(= 1752 (:line metadata))',
                                        f'(= {declaration["start_line"]} (:line metadata))')
        contract_source = '(do ' + ' '.join(line for line in contract.splitlines()
                                           if not line.lstrip().startswith(';')) + ' (prim-seq-contract))'
        pin_cases.append(case('prim-seq-local-literal-contract', contract_source))
        receipt.setdefault('local_contract', {})[kind] = {
            'source': str(contract_path.relative_to(ROOT)), 'source_sha256': sha(contract_path.read_bytes()),
            'adaptation': 'Release metadata :line is 1751; checkout/local metadata :line is 1752. Only that explicit source-location assertion is adapted.',
            'reference_source_sha256': sha(contract_source.encode())}
        receipt.setdefault('executed_cases', {})[kind] = pin_cases
        observed = report.reference_cases(pin_cases, kind, ROOT / 'research/clojurescript', args.output / kind, 180)
        receipt['references'][kind] = observed
        receipt.setdefault('fixture_sha256', {})[kind] = {
            str(path.relative_to(args.output / kind)): sha(path.read_bytes())
            for path in sorted((args.output / kind / 'src').rglob('*.cljs'))}
        failures.update({kind + ':' + identity: result for identity, result in observed.items()
                         if result['status'] != 'passed'})
        print(kind, json.dumps({identity: result['status'] for identity, result in observed.items()}), flush=True)
    assert original_path.read_bytes() == original_bytes, 'Original probe file changed during reference execution'
    (args.output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    assert not failures, failures
    assert all(len(result) == len(cases) + 1 for result in receipt['references'].values())
    if args.write:
        (ROOT / 'tests/reference/compiler-prim-seq-receipts.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(f'{len(cases) + 1} prim-seq cases passed both checked pins; receipt: {args.output / "receipt.json"}')


if __name__ == '__main__':
    main()
