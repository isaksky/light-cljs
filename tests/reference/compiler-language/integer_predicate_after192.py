#!/usr/bin/env python3
"""Complete original integer predicate acceptance with genuine Closure bytes."""
from pathlib import Path
import argparse,json,shutil,sys
import extract_inventory as inventory
import hosted_import_source_context as hosted
from original_integer_predicate_after190 import digest
ROOT=Path(__file__).resolve().parents[3]
MODULE='research/clojurescript/src/test/cljs/cljs/predicates_test.cljs'
DRIVER='tests/integer_predicate_after192.rs'
TEST='whole_original_integer_table_passes_in_actual_hosts'
Genuine=Path('/tmp/light-cljs-125-hosted-import-adapter-v1/release/cljs-hosted-original-import-context/out/goog')


def original():
    source=(ROOT/MODULE).read_text();forms=inventory.Reader(source).all();retained={}
    for name in ['int-val-table','test-int-preds']:
        matches=[form for form in forms if len(form.children)>1 and form.children[1].token==name]
        if len(matches)!=1:raise ValueError('original full integer forms unavailable')
        form=matches[0];text=source[form.start:form.end];retained[name]={'source':text,'sha256':digest(text)}
    return {'module':MODULE,'module_sha256':digest(source),'forms':retained}


def program(original,loader,phase):
    # These harness definitions are external to both byte-exact original forms.
    prefix='''(ns integer-predicate-after192 (:import [goog.math Integer]))
(js* '''+json.dumps(loader)+''')
(def LongImpl (js* "goog.module.get('goog.math.Long')"))
(def checks (atom []))
(defn record-check [passed i message] (swap! checks conj [i passed message]) nil)
(defmacro is [form message] (list 'record-check form 'i message))
(defmacro deftest [name & body] (list 'defn name [] (cons 'do body)))
'''
    units='\n'.join(form['source'] for form in original['forms'].values())+'\n'
    controls='''(defn receiver-controls []
 [(instance? LongImpl (first (nth int-val-table 9)))
  (instance? LongImpl (first (nth int-val-table 10)))
  (instance? LongImpl (first (nth int-val-table 11)))
  (instance? Integer (first (nth int-val-table 12)))
  (instance? Integer (first (nth int-val-table 13)))
  (instance? Integer (first (nth int-val-table 14)))
  (.toNumber (first (nth int-val-table 9)))
  (.toNumber (first (nth int-val-table 10)))
  (.toNumber (first (nth int-val-table 11)))
  (.toNumber (first (nth int-val-table 12)))
  (.toNumber (first (nth int-val-table 13)))
  (.toNumber (first (nth int-val-table 14)))])
'''
    focused='(def control-hits (atom 0))\n(defn counted [x] (swap! control-hits inc) x)\n(defn predicate-row [x] [(integer? x) (int? x) (pos-int? x) (neg-int? x) (nat-int? x)])\n(defn focused-controls []\n (reset! control-hits 0)\n (let [spoof (js* "({constructor: {name: \'Integer\'}, bits_: [0], sign_: 0, isNegative() {throw Error(\'spoof sign called\')}, isZero() {throw Error(\'spoof zero called\')}, toNumber() {return 0}})")\n       positive (.fromString Integer "10000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000")\n       negative (.negate positive)\n       zero (.fromNumber Integer 0)\n       long-zero (.fromNumber LongImpl 0)\n       long-positive (.fromNumber LongImpl 1)\n       long-negative (.fromNumber LongImpl -1)\n       int-alias int?]\n  [(mapv predicate-row [0 -0.0 1 -1 1.5 js/NaN js/Infinity (- js/Infinity) 1e21 nil {} spoof zero positive negative long-zero long-positive long-negative])\n   [(int? (counted zero)) (pos-int? (counted positive))\n    (neg-int? (counted negative)) (nat-int? (counted zero))\n    (int-alias (counted long-zero)) (apply nat-int? [(counted long-positive)])]\n   @control-hits]))\n'
    controls+=focused
    run="(do (reset! checks []) (test-int-preds) [(receiver-controls) @checks (focused-controls)])"
    suffix='(defmacro integer-predicate-run [] (do (js* '+json.dumps(loader)+') (list \'quote '+run+')))\n(defn run [] (integer-predicate-run))\nrun\n' if phase=='macro' else '(defn run [] '+run+')\nrun\n'
    return prefix+units+controls+suffix


def prepare(output):
    output=Path(output).resolve();output.mkdir(exist_ok=False,parents=True)
    loader,dependencies=hosted.dependency_bundle(Genuine);origin=hosted.library_origin(Genuine)
    data=original();data.update(genuine_compiled_dependency_root=str(Genuine),compiled_dependency_sha256=dependencies,library_origin=origin,closure_loader_sha256=digest(loader),adapter_boundaries=['explicit audited genuine Closure bytes','LongImpl alias to goog.module.get(goog.math.Long)','external deftest/is assertion collector; original forms unchanged'],original_site_credit=0,ordinary_credit=0,phase_credit=0,strict_complete=False)
    (output/'source-catalog.json').write_text(json.dumps(data,indent=2)+'\n');(output/'genuine-loader.js').write_text(loader)
    for phase in ['application','macro']:(output/(phase+'.cljs')).write_text(program(data,loader,phase))
    paths=[Path(__file__),ROOT/DRIVER,ROOT/MODULE,ROOT/'tests/support/sci_execution_observation.js',ROOT/'tests/reference/compiler-language/hosted_import_source_context.py',ROOT/'tests/reference/compiler-language/original_integer_predicate_after190.py',ROOT/'tests/reference/compiler-language/integer_predicate_after192_proof.py',*(p for p in (ROOT/'src').rglob('*') if p.is_file())]
    (output/'snapshot-before.json').write_text(json.dumps({str(p.relative_to(ROOT)):digest(p.read_bytes()) for p in paths},indent=2)+'\n')
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--prepare',type=Path,required=True);a=p.parse_args();prepare(a.prepare)
