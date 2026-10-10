"use strict";
// Emitted functions supply every language operation. JS only drives loops,
// prepares host arrays and independently validates shallow branded observations.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const [file, samplesText, iterationsText] = process.argv.slice(2);
const samples = Number(samplesText), requested = Number(iterationsText);
assert(samples >= 9 && requested >= 1);
const names = ['vector','list','map','set','nth','assoc','conj','get','dissoc','disj',
  'seq','first','next','count','equal','contains','keyword','reduce','add',
  'vectorCall','keywordCall','mapCall','setCall','nestedUpdate','nestedLookup','jsFold'];
let c;
globalThis.benchAdd = (a, b) => a + b;
globalThis.benchExport = (...functions) => {
  assert.equal(functions.length, names.length);
  c = Object.fromEntries(names.map((name, i) => [name, functions[i]]));
  return null;
};
const source = fs.readFileSync(file, 'utf8');
assert.equal(source.split('\n  const rt = ').length, 2);
(0, eval)(source.replace('\n  const rt = ', '\n  const rt = globalThis.benchRuntime = '));
const rt = globalThis.benchRuntime;
let sink = 0;
const rows = [];
const info = value => { const result = rt.inspect(value); assert(result); return result; };
const seqItems = (value, kind) => { const result = info(value); assert.equal(result.kind, kind); return result.items; };
const numberOrder = (a, b) => a - b;
function measure(name, size, operation, validate, checksum, slow = false) {
  // Bulk O(n^2) scenarios explicitly scale repeats down at the largest sizes.
  const iterations = slow ? Math.max(1, Math.min(requested, Math.floor(16384 / (size * size)))) : requested;
  validate(operation());
  function batch(timed) {
    const retained = new Array(iterations);
    let sum = 0;
    const start = timed ? process.hrtime.bigint() : 0n;
    for (let i = 0; i < iterations; i++) { const value = operation(); retained[i] = value; sum += checksum(value); }
    const elapsed = timed ? Number(process.hrtime.bigint() - start) : 0;
    sink += sum;
    // Full validation and snapshot independence are deliberately outside timing.
    for (const value of retained) validate(value);
    return {elapsed, sum};
  }
  batch(false); batch(false);
  for (let sample = 1; sample <= samples; sample++) {
    const {elapsed, sum} = batch(true);
    rows.push({scenario: name, size, sample, iterations, elapsed_ns: elapsed, checksum: sum});
  }
}
for (const size of [8, 64, 512, 4096]) {
  const numbers = Array.from({length: size}, (_, i) => i);
  const vector = c.vector(...numbers), list = c.list(...numbers);
  const originalVector = [...numbers], originalList = [...numbers];
  const at = size - 1;
  const vectorCheck = xs => value => assert.deepEqual(seqItems(value, 'vector'), xs);
  const listCheck = xs => value => assert.deepEqual(seqItems(value, 'list'), xs);
  measure('vector/construct', size, () => c.vector(...numbers), vectorCheck(numbers), c.count);
  measure('vector/nth', size, () => c.nth(vector, at), v => assert.equal(v, at), v => v);
  measure('vector/assoc-retained', size, () => c.assoc(vector, at, -1), vectorCheck([...numbers.slice(0,-1), -1]), c.count);
  measure('vector/conj-retained', size, () => c.conj(vector, size), vectorCheck([...numbers, size]), c.count);
  measure('list/prepend-retained', size, () => c.conj(list, -1), listCheck([-1, ...numbers]), c.count);
  const sum = size * (size - 1) / 2;
  measure('list/traverse', size, () => {
    let total = 0; for (let xs = c.seq(list); xs !== null; xs = c.next(xs)) total += c.first(xs); return total;
  }, v => assert.equal(v, sum), v => v);
  measure('calls/saved-core-nth', size, () => c.nth.call(null, vector, at), v => assert.equal(v, at), v => v);
  measure('calls/vector', size, () => c.vectorCall(vector, at), v => assert.equal(v, at), v => v);
  const kw = c.keyword('leaf'), kwMap = c.map(kw, size);
  measure('calls/keyword', size, () => c.keywordCall(kw, kwMap), v => assert.equal(v, size), v => v);

  for (const collision of [false, true]) {
    const group = collision ? 'collision' : 'structural';
    // All length-12 Aa/BB products have the same original runtime string hash.
    const rawKeys = numbers.map(i => collision ? Array.from({length: 12}, (_, bit) => (i >> bit & 1) ? 'Aa' : 'BB').join('') : i);
    const keys = rawKeys.map(k => collision ? k : c.vector(k));
    const probe = collision ? rawKeys[at] : c.list(at);
    const missing = collision ? 'missing' : c.list(size);
    if (collision) for (const key of keys) assert.equal(rt.hash(key), rt.hash(keys[0]));
    const pairs = keys.flatMap((key, i) => [key, i]);
    const map = c.map(...pairs), set = c.set(...keys);
    const keyValue = key => collision ? key : seqItems(key, 'vector')[0];
    const sortKeys = values => values.sort(collision ? undefined : numberOrder);
    const expectedKeys = sortKeys([...rawKeys]);
    const expectedEntries = rawKeys.map((key, i) => [key, i]).sort((a,b) => collision ? a[0].localeCompare(b[0]) : a[0]-b[0]);
    const mapCheck = (expected, count = expected.length) => value => {
      const result = info(value); assert.equal(result.kind, 'map'); assert.equal(result.count, count);
      assert.deepEqual(result.entries.map(([k,v]) => [keyValue(k),v]).sort((a,b) => collision ? a[0].localeCompare(b[0]) : a[0]-b[0]), expected);
    };
    const setCheck = expected => value => {
      const result = info(value); assert.equal(result.kind, 'set'); assert.equal(result.count, expected.length);
      assert.deepEqual(sortKeys(result.items.map(keyValue)), expected);
    };
    measure(`map/${group}/construct`, size, () => c.map(...pairs), mapCheck(expectedEntries), c.count, true);
    measure(`set/${group}/construct`, size, () => c.set(...keys), setCheck(expectedKeys), c.count, true);
    measure(`map/${group}/lookup-last`, size, () => c.get(map, probe), v => assert.equal(v, at), v => v);
    measure(`set/${group}/lookup-last`, size, () => c.get(set, probe), v => assert.equal(v, keys[at]), () => 1);
    measure(`map/${group}/miss`, size, () => c.get(map, missing, -1), v => assert.equal(v, -1), v => v);
    measure(`set/${group}/miss`, size, () => c.get(set, missing, -1), v => assert.equal(v, -1), v => v);
    const changedEntries = expectedEntries.map(([k,v]) => [k, v === at ? -1 : v]);
    measure(`map/${group}/assoc-retained`, size, () => c.assoc(map, probe, -1), mapCheck(changedEntries), c.count);
    measure(`map/${group}/dissoc-retained`, size, () => c.dissoc(map, probe), mapCheck(expectedEntries.filter(([,v]) => v !== at)), c.count);
    measure(`set/${group}/disj-retained`, size, () => c.disj(set, probe), setCheck(expectedKeys.filter(k => k !== rawKeys[at])), c.count);
    const extra = collision ? 'extra' : c.vector(size);
    measure(`set/${group}/conj-retained`, size, () => c.conj(set, extra), setCheck(sortKeys([...rawKeys, collision ? 'extra' : size])), c.count);
    measure(`calls/map-${group}`, size, () => c.mapCall(map, probe), v => assert.equal(v, at), v => v);
    measure(`calls/set-${group}`, size, () => c.setCall(set, probe), v => assert.equal(v, keys[at]), () => 1);
    mapCheck(expectedEntries)(map); setCheck(expectedKeys)(set);
  }
  const outerKey = c.vector(1,2), inner = c.map(kw,size), nested = c.map(outerKey,inner);
  const equalKey = c.list(1,2);
  measure('nested/update-retained', size, () => c.nestedUpdate(nested,equalKey,-1), value => {
    const outer = info(value); assert.equal(outer.kind,'map'); assert.equal(outer.count,1);
    assert.equal(c.equal(outer.entries[0][0],outerKey),true);
    const inside = info(outer.entries[0][1]); assert.equal(inside.kind,'map'); assert.equal(inside.count,1);
    assert.equal(inside.entries[0][0],kw); assert.equal(inside.entries[0][1],-1);
  }, c.count);
  measure('nested/lookup', size, () => c.nestedLookup(nested,equalKey), v => assert.equal(v,size), v=>v);
  assert.equal(c.get(nested,outerKey),inner); assert.equal(c.get(inner,kw),size);
  vectorCheck(originalVector)(vector); listCheck(originalList)(list);
}
const array1000 = Array.from({length: 1000}, (_,i) => i), vector1000 = c.vector(...array1000);
measure('fold/core-reduce-add',1000,()=>c.reduce(c.add,0,vector1000),v=>assert.equal(v,499500),v=>v);
measure('fold/compiled-js-interop',1000,()=>c.jsFold(array1000),v=>assert.equal(v,499500),v=>v);
assert.deepEqual(seqItems(vector1000,'vector'),array1000);
assert.deepEqual(array1000,Array.from({length:1000},(_,i)=>i));
assert(Number.isFinite(sink));
console.log(JSON.stringify({method:'Resident compiled core/functions; two untimed warmup batches; full results checked before/after every batch; outputs retained per sample; JS checksum and result-slot store included; no forced GC',sink,rows}));
