#!/usr/bin/env python3
"""Measure current compiler output and fresh V8 parse/evaluation, validating values."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import platform
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--protocols', action='store_true', help='Include user protocol direct and metadata dispatch samples')
parser.add_argument('--records', action='store_true', help='Include record construction, updates and method samples')
parser.add_argument('--types', action='store_true', help='Include mutable user types and custom reduction samples')
parser.add_argument('--reify', action='store_true', help='Include captured reify construction and metadata-copy samples')
parser.add_argument('--extensions', action='store_true', help='Include native and instance extension samples')
parser.add_argument('--collection-helpers', action='store_true', help='Include persistent helper and transforming into samples')
parser.add_argument('--sequences', action='store_true', help='Include lazy pipeline and nested comprehension samples')
parser.add_argument('--deferred', action='store_true', help='Include delay and bounded lazy reduction samples')
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
subprocess.run(['cargo', 'build', '--quiet'], cwd=ROOT, check=True)
binary = ROOT / 'target/debug/light-cljs'
# The embedded runtime: the source without its `//@` manifest header lines (see build.rs).
runtime = b''.join(line for line in (ROOT / 'src/js_runtime.js').read_bytes().splitlines(keepends=True)
                   if not line.lstrip().startswith(b'//@'))
sha = lambda value: hashlib.sha256(value).hexdigest()
report = {'node': subprocess.check_output(['node', '--version'], text=True).strip(),
          'platform': platform.platform(), 'machine': platform.machine(),
          'binary_sha256': sha(binary.read_bytes()),
          'runtime_sha256': sha(runtime), 'runtime_bytes': len(runtime),
          'runtime_gzip_bytes': len(gzip.compress(runtime, mtime=0)),
          'method': 'Nine fresh Node processes per program. Time vm.Script compilation plus runInThisContext in milliseconds; validate the numeric result after timing. Excludes process launch and source reading. OS caches/scheduling uncontrolled; historical baseline is not paired.',
          'programs': {}}
programs = {'primitive': ('42', 42),
            'dot': ('(. "abcd" indexOf "d")', 3),
            'constructor': ('(.-x (new (fn [x] (this-as self (set! (.-x self) x))) 3))', 3),
            'template': ('(js* "(~{} + ~{})" 1 2)', 3),
            'collections': ('(count [1 2 3])', 3),
            'array-helpers': ('(aget (array 1 2 3) 2)', 3)}
if args.protocols:
    programs.update({
        'protocol-direct': ('(defprotocol P (f [this x])) (let [v (reify P (f [_ x] (+ x 2))) saved f] (saved v 5))', 7),
        'protocol-metadata': ('(defprotocol P :extend-via-metadata true (f [this])) (f (with-meta {} {`f (fn [_] 7)}))', 7)})
if args.reify:
    programs.update({
        'reify-captures': ('(defprotocol P (f [this])) (let [make (fn [x] (reify P (f [_] x))) a (make 3) b (make 4)] (+ (f a) (f b)))', 7),
        'reify-metadata': ('(let [x 7 a (reify IFn (-invoke [_] x) Object (toString [_] (str x))) b (with-meta a {:m 1})] (b))', 7)})
if args.types:
    programs.update({
        'type-mutable': ('(deftype T [^:mutable x] Object (increment [_] (set! x (inc x)))) (let [v (T. 6)] (.increment v))', 7),
        'type-reduce': ('(deftype T [xs] IReduce (-reduce [_ f] (reduce f xs)) (-reduce [_ f init] (reduce f init xs))) (reduce + (T. [1 2 4]))', 7)})
if args.records:
    programs.update({
        'record-updates': ('(defrecord R [a b]) (let [r (assoc (->R 1 2) :c 4)] (reduce-kv (fn [acc _ value] (+ acc value)) 0 r))', 7),
        'record-methods': ('(defprotocol P (value [this])) (defrecord R [x] P (value [_] x)) (value (with-meta (map->R {:x 7 :extra 2}) {:m 1}))', 7)})
if args.extensions:
    programs.update({
        'extension-native': ('(defprotocol P (f [this])) (extend-type number P (f [n] (+ n 2))) (f 5)', 7),
        'extension-instance': ('(defprotocol P (f [this])) (let [v (specify [1 2] P (f [_] 7))] (f v))', 7)})
if args.collection_helpers:
    programs.update({
        'nested-helpers': ('(get-in (merge {:a {:b 2}} (update-in {:a {:b 3}} [:a :b] + 4)) [:a :b])', 7),
        'transforming-into': ('(reduce + (into [] (fn [rf] (fn ([r] (rf r)) ([r x] (rf r (inc x))))) [1 2]))', 5)})
if args.deferred:
    programs.update({
        'delay-force': ('(let [d (delay 7)] (force d))', 7),
        'lazy-prefix': ('(reduce + (take 4 ((fn step [n] (lazy-seq (cons n (step (inc n))))) 0)))', 6)})
if args.sequences:
    programs.update({
        'sequence-pipeline': ('(reduce + (take 100 (map inc (filter even? (range)))))', 10000),
        'comprehension': ('(reduce + (for [x (range 10) y (range 10) :when (odd? y)] (+ x y)))', 475)})
for name, (source, expected) in programs.items():
    javascript = subprocess.check_output([str(binary), '--compile', '--text', source], cwd=ROOT)
    emitted = args.output / (name + '.js')
    emitted.write_bytes(javascript)
    script = '''const fs = require('node:fs'), vm = require('node:vm');
const source = fs.readFileSync(process.argv[1], 'utf8');
const before = performance.now();
const result = new vm.Script(source).runInThisContext();
const elapsed = performance.now() - before;
if (result !== Number(process.argv[2])) throw Error('incorrect benchmark result');
console.log(elapsed);'''
    samples = [float(subprocess.check_output(['node', '-e', script, str(emitted.resolve()), str(expected)], text=True)) for _ in range(9)]
    report['programs'][name] = {'source': source, 'expected': expected,
                              'sha256': sha(javascript), 'bytes': len(javascript),
                              'gzip_bytes': len(gzip.compress(javascript, mtime=0)),
                              'samples_ms': samples, 'median_ms': statistics.median(samples)}
(args.output / 'output-startup.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({name: {'bytes': row['bytes'], 'median_ms': row['median_ms']} for name, row in report['programs'].items()}))
