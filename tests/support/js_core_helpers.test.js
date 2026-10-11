"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const r = (0, eval)(fs.readFileSync(path.join(__dirname, "../../src/js_runtime.js"), "utf8"));
const c = r.core;
const equivalent = (actual, expected) => assert.equal(c.equal(actual, expected), true);

// Check the public callable ABI, return values and order separately from the
// compiler. Strict callbacks observe the same null receiver as compiled calls.
for (const constructor of [c.someFn, c.everyPred]) {
  const calls = [];
  const make = label => function (x) { assert.equal(this, null); calls.push(label + x); return constructor === c.everyPred; };
  constructor(make("a"), make("b"), make("c"))(1, 2, 3, 4, 5);
  assert.deepEqual(calls, ["a1", "a2", "a3", "b1", "b2", "b3", "c1", "c2", "c3", "a4", "b4", "c4", "a5", "b5", "c5"]);
  calls.length = 0;
  c.apply(constructor, c.list(make("a"), make("b"), make("c"), make("d")))(1, 2, 3, 4, 5);
  assert.deepEqual(calls, ["a1", "a2", "a3", "b1", "b2", "b3", "c1", "c2", "c3", "d1", "d2", "d3", "a4", "a5", "b4", "b5", "c4", "c5", "d4", "d5"]);
}
assert.equal(c.someFn(() => false)(), null);
assert.equal(c.someFn(() => false)(1, 2, 3), false);
assert.equal(c.someFn(() => false)(1, 2, 3, 4), null);
assert.equal(c.someFn(() => false, () => false, () => false, () => false)(1), null);
assert.equal(c.someFn(() => 0, () => { throw Error("past short circuit"); })(1), 0);
assert.equal(c.someFn(() => "")(1), "");
assert.equal(c.everyPred(() => 0)(1), true);
assert.equal(c.everyPred(() => null, () => { throw Error("past short circuit"); })(1), false);
assert.equal(c.someFn()(), null);
assert.equal(c.everyPred()(), true);
assert.throws(() => c.someFn()(1), TypeError);
assert.throws(() => c.everyPred()(1), TypeError);

function guarded(i = 0, value = x => x) {
  return new c.LazySeq(null, () => {
    if (i > 12) throw Error("a short-circuiting apply forced the unused tail");
    return c.cons(value(i), guarded(i + 1, value));
  }, null, null);
}
assert.equal(c.apply(c.someFn(x => x === 6 ? "found" : null), guarded()), "found");
assert.equal(c.apply(c.everyPred(x => x < 6), guarded()), false);
assert.equal(c.apply(c.someFn, guarded(0, () => () => "found"))(1), "found");
assert.equal(c.apply(c.everyPred, guarded(0, () => () => false))(1), false);
// Callable data participates through .call just like a function.
assert.equal(c.someFn(c.hashMap("x", 0))("x"), 0);

// Applied constructors capture unbounded inputs after the fixed prefix. The
// returned function need not consume that tail until it is actually invoked.
const partialEffects = [], juxtEffects = [];
function observedTail(effects, limit = Infinity, value = x => x, i = 0) {
  return new c.LazySeq(null, () => {
    if (i === limit) return null;
    if (i > 8 && limit === Infinity) throw Error("constructor consumed an unbounded tail");
    effects.push(i);
    return c.cons(value(i), observedTail(effects, limit, value, i + 1));
  }, null, null);
}
assert.equal(typeof c.apply(c.partial, c.vector, observedTail(partialEffects)), "function");
assert.equal(typeof c.apply(c.juxt, observedTail(juxtEffects, Infinity, i => () => i)), "function");
assert.deepEqual(partialEffects, [0, 1, 2, 3, 4]);
assert.deepEqual(juxtEffects, [0, 1, 2, 3, 4]);
partialEffects.length = 0;
const deferredPartial = c.apply(c.partial, c.vector, observedTail(partialEffects, 8));
assert.deepEqual(partialEffects, [0, 1, 2, 3, 4]);
equivalent(deferredPartial(9), c.vector(0, 1, 2, 3, 4, 5, 6, 7, 9));
equivalent(c.apply(deferredPartial, c.list(10, 11)), c.vector(0, 1, 2, 3, 4, 5, 6, 7, 10, 11));
assert.deepEqual(partialEffects, [0, 1, 2, 3, 4, 5, 6, 7]);
juxtEffects.length = 0;
const juxtCalls = [];
const deferredJuxt = c.apply(c.juxt, observedTail(juxtEffects, 6, i => (...args) => {
  juxtCalls.push(i);
  return i + args.reduce((a, b) => a + b, 0);
}));
assert.deepEqual(juxtEffects, [0, 1, 2, 3, 4]);
equivalent(deferredJuxt(10), c.vector(10, 11, 12, 13, 14, 15));
equivalent(c.apply(deferredJuxt, c.list(1, 2, 3, 4)), c.vector(10, 11, 12, 13, 14, 15));
assert.deepEqual(juxtEffects, [0, 1, 2, 3, 4, 5]);
assert.deepEqual(juxtCalls, [0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5]);
assert.equal(c.partial(c.vector), c.vector);
assert.throws(() => c.partial()(), TypeError);
assert.throws(() => c.juxt()(), TypeError);
for (let n = 0; n <= 6; n++) {
  const prefix = Array.from({length: n}, (_, i) => i);
  for (let argc = 0; argc <= 6; argc++) {
    const args = Array.from({length: argc}, (_, i) => 10 + i);
    equivalent(c.partial(c.vector, ...prefix)(...args), c.vector(...prefix, ...args));
    equivalent(c.apply(c.partial, c.list(c.vector, ...prefix))(...args), c.vector(...prefix, ...args));
    if (n) {
      const fns = prefix.map(i => (...xs) => i + xs.reduce((a, b) => a + b, 0));
      const expected = c.vector(...prefix.map(i => i + args.reduce((a, b) => a + b, 0)));
      equivalent(c.juxt(...fns)(...args), expected);
      equivalent(c.apply(c.juxt, c.list(...fns))(...args), expected);
    }
  }
}

let evaluations = 0;
const memoized = c.memoize(function (...args) { return ++evaluations % 2 ? null : false; });
assert.equal(memoized(c.vector(1, 2)), null);
assert.equal(memoized(c.list(1, 2)), null);
assert.equal(memoized(c.vector(3)), false);
assert.equal(c.apply(memoized, c.list(c.vector(3))), false);
assert.equal(evaluations, 2);
let attempts = 0;
const retry = c.memoize(() => { if (++attempts === 1) throw Error("retry"); return "ok"; });
assert.throws(() => retry()); assert.equal(retry(), "ok"); assert.equal(retry(), "ok"); assert.equal(attempts, 2);
const receiver = function () { return this; };
assert.equal(c.memoize(receiver)(), receiver); // memoize invokes f through apply.

const savedRandom = Math.random;
try {
  Math.random = function () { assert.equal(this, Math); return 0.25; };
  assert.equal(c.rand(), 0.25); assert.equal(c.rand(8), 2); assert.equal(c.rand(-4), -1); assert.equal(c.rand(null), 0);
  assert.throws(() => c.rand(1, 2), /Invalid arity: 2/);
} finally { Math.random = savedRandom; }
const order = [];
const grouped = c.groupBy(function (x) { assert.equal(this, null); order.push(x); return x % 2; }, c.list(3, 1, 2, 5, 4));
equivalent(grouped, c.hashMap(1, c.vector(3, 1, 5), 0, c.vector(2, 4)));
assert.deepEqual(order, [3, 1, 2, 5, 4]);
equivalent(c.groupBy(x => x, null), c.hashMap());
equivalent(c.frequencies(c.list(c.vector(1), c.list(1), null, false, null)), c.hashMap(c.vector(1), 2, null, 2, false, 1));
equivalent(c.frequencies(null), c.hashMap());

const start = function (n, sum) { assert.equal(this, start); return function step() { return n-- > 0 ? () => { sum++; return step; } : sum; }; };
assert.equal(c.trampoline(start, 50000, 0), 50000);
equivalent(c.trampoline(function () { assert.equal(this, null); return c.keyword("done"); }), c.keyword("done"));
assert.throws(() => c.trampoline(), TypeError);
const delegate = function () { throw Error("apply should retain the variadic ABI"); };
delegate.cljs$lang$maxFixedArity = 0;
delegate.cljs$lang$applyTo = args => c.first(args);
assert.equal(c.apply(c.trampoline, delegate, guarded(8)), 8);

const comparisons = [];
const cmp = c.comparator(function (a, b) { assert.equal(this, null); comparisons.push([a, b]); return a < b; });
assert.equal(cmp(1, 2), -1); assert.equal(cmp(2, 1), 1); assert.equal(cmp(2, 2), 0);
assert.deepEqual(comparisons, [[1, 2], [2, 1], [1, 2], [2, 2], [2, 2]]);
assert.equal(c.comparator(() => 0)(1, 2), -1); // zero is CLJS-truthy; this is not fn->comparator.
equivalent(c.sort((a, b) => a - b, c.vector(3, 1, 2)), c.list(1, 2, 3));
const metadata = c.hashMap("source", true), original = c.withMeta(c.hashMap(1, 10, 2, 20), metadata);
const values = c.updateVals(original, function (x) { assert.equal(this, null); return x + 1; });
const keys = c.updateKeys(original, function (x) { assert.equal(this, null); return x + 2; });
equivalent(values, c.hashMap(1, 11, 2, 21)); equivalent(keys, c.hashMap(3, 10, 4, 20));
assert.equal(c.meta(values), metadata); assert.equal(c.meta(keys), metadata);
assert.equal(c.type(values), c.PersistentHashMap); equivalent(original, c.hashMap(1, 10, 2, 20));
equivalent(c.updateVals(null, () => { throw Error("empty"); }), c.hashMap());
equivalent(c.updateKeys(null, () => { throw Error("empty"); }), c.hashMap());
assert.equal(c.dropProtocol(c.arrayMap(), 4), null);
assert.equal(new c.LazySeq(null, () => c.list(1, 2), null, null).indexOf(2), 1);
equivalent(c.PersistentVector.EMPTY, c.vector());
equivalent(c.PersistentArrayMap.EMPTY, c.hashMap());
equivalent(c.PersistentHashMap.EMPTY, c.hashMap());
assert.equal(c.type(c.PersistentArrayMap.EMPTY), c.PersistentArrayMap);
assert.equal(c.type(c.PersistentHashMap.EMPTY), c.PersistentHashMap);
assert.equal(c.PersistentArrayMap.HASHMAP_THRESHOLD, 8);
const inputPairs = ["a", 1, "b", 2, "a", 3];
const fromArray = c.PersistentHashMap.fromArray(inputPairs, true);
inputPairs[1] = 100; inputPairs.push("c", 4);
equivalent(fromArray, c.hashMap("a", 3, "b", 2));
assert.equal(c.type(fromArray), c.PersistentHashMap);
assert.equal(c.get(c.PersistentHashMap.fromArray(["odd"], false), "odd", "absent"), undefined);

// No-op set updates preserve both the original set and its stored key.
const storedMember = c.vector(1, 2), duplicateMember = c.list(1, 2);
const stableSet = c.withMeta(c.hashSet(storedMember), metadata);
assert.equal(c.conj(stableSet, duplicateMember), stableSet);
assert.equal(c.disj(stableSet, c.vector(3)), stableSet);
assert.equal(c.get(c.conj(stableSet, duplicateMember), duplicateMember), storedMember);
assert.equal(c.meta(c.conj(stableSet, duplicateMember)), metadata);
assert.throws(() => r.literalSetFromArray([storedMember, duplicateMember]), /Duplicate key/);
equivalent(c.hashSet(storedMember, duplicateMember), stableSet);
for (const Type of [c.Range, c.IntegerRange]) {
  equivalent(new Type(null, -2, 4, 2, 3, null, null), c.list(-2, 0, 2));
  const directEmpty = new Type(null, 4, -2, 2, 0, null, null);
  assert.equal(c.seq(directEmpty), directEmpty);
  assert.equal(c.first(directEmpty), 4);
}
assert.equal(c.count(new c.IntegerRange(null, -2, 4, 2, null, null)), null);
assert.equal(c.count(new c.IntegerRange(null, -2, 4, 2)), undefined);
assert.equal(c.count(new c.IntegerRange(null, -2, 4, 2, 3, null)), 3);
assert.equal(c.seq(c.range(4, -2, 2)), null);
const flattenedEffects = [];
const flattenReducing = c.cat(function (acc, value) {
  if (arguments.length === 1) return acc;
  flattenedEffects.push(value);
  return value === 3 ? c.reduced(acc + value) : acc + value;
});
assert.equal(c.reduce(flattenReducing, 0, c.vector(c.vector(1, 2), c.vector(3, 4), c.vector(5))), 6);
assert.deepEqual(flattenedEffects, [1, 2, 3]);
const oldGoog = globalThis.goog;
try {
  globalThis.goog = {Uri: class Uri {}};
  assert.equal(c.isUri(new globalThis.goog.Uri()), true);
  assert.equal(c.isUri({}), false);
} finally { if (oldGoog === undefined) delete globalThis.goog; else globalThis.goog = oldGoog; }
assert.match(c.str(c.randomUuid()), /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);

const originalVector = c.vector(0, 1, 2, 3);
const originalSlice = c.subvec(originalVector, 1, 3);
const metaSlice = c.withMeta(originalSlice, metadata);
assert.equal(metaSlice.v, originalVector);
assert.equal(c.clone(metaSlice).v, originalVector);
assert.equal(c.subvec(metaSlice, 1).v, originalVector);
equivalent(metaSlice, c.vector(1, 2));
console.log("core helper semantics passed");
