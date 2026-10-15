"use strict";

// Independent expected-value checks for the first-party runtime. No compiler
// lowering or observation encoder participates in these assertions.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const runtime = (0, eval)(fs.readFileSync(path.join(__dirname, "../../src/js_runtime.js"), "utf8"));
const c = runtime.core;
const throwsError = thunk => assert.throws(thunk, error => error.constructor === Error);
const v = runtime.vector(10, 20, 30);
const kw = runtime.keyword("demo", "x");

// Macro evaluators share these symbols and collections with the canonical core.
{
  const first = c.gensym(), second = c.gensym('tmp__');
  assert.equal(c.isSymbol(first), true);
  assert.equal(c.namespace(first), null);
  assert.equal(c.name(first), 'G__1');
  assert.equal(c.name(second), 'tmp__2');
  assert.equal(c.equal(first, second), false);
  assert.equal(c.get(c.hashMap(first, 42), c.symbol('G__1')), 42);
  assert.equal(c.name(c.apply(c.gensym, c.list('arg__'))), 'arg__3');
  assert.throws(() => c.gensym('x', 'y'), /Invalid arity: 2/);
  const fresh = (0, eval)(fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8'));
  assert.equal(fresh.core.name(fresh.core.gensym()), 'G__1');

  const form = c.withMeta(c.list(c.symbol('sample/m'), c.vector(1)), c.hashMap('source', true));
  const expanded = c.list(c.symbol('do'), c.first(c.rest(form)));
  const expand = c.macroexpand, expand1 = c.macroexpand1;
  assert.throws(() => expand(form), /macroexpand service is not installed/);
  assert.throws(() => expand1(form), /macroexpand-1 service is not installed/);
  runtime.setMacroexpandServices({
    macroexpand: function (value) { assert.equal(this, null); assert.equal(value, form); return expanded; },
    macroexpand1: function (value) { assert.equal(this, null); return value; }
  });
  assert.equal(expand(form), expanded);
  assert.equal(c.apply(expand1, c.list(form)), form);
  assert.equal(c.meta(expand1(form)), c.meta(form));
  assert.throws(() => fresh.core.macroexpand(form), /service is not installed/);
  assert.throws(() => expand(), /Invalid arity: 0/);
  assert.throws(() => expand1(form, form), /Invalid arity: 2/);
  assert.throws(() => runtime.setMacroexpandServices({macroexpand: 1}), TypeError);
  assert.equal(expand(form), expanded);
  const failure = {};
  runtime.setMacroexpandServices({macroexpand: () => { throw failure; }});
  assert.throws(() => expand(form), error => error === failure);
  assert.throws(() => expand1(form), /service is not installed/);
  runtime.setMacroexpandServices(null);
  assert.throws(() => expand(form), /service is not installed/);
}
console.log('gensym and configurable macro expansion service assertions passed');

assert.deepEqual(runtime.inspect(kw), {kind: "keyword", namespace: "demo", name: "x", spelling: "demo/x"});
assert.deepEqual(runtime.inspect(v), {kind: "vector", items: [10, 20, 30]});
assert.equal(c.name(kw), "x");
assert.equal(c.namespace(kw), "demo");
assert.equal(c.keyword(kw), kw);
assert.deepEqual(runtime.inspect(c.keyword(null, "x")), {kind: "keyword", namespace: null, name: "x", spelling: "x"});
assert.equal(c.namespace(c.keyword(undefined, "x")), undefined);
assert.equal(c.equal(c.keyword(null, "x"), c.keyword("x")), true);
assert.equal(c.equal(kw, c.keyword("demo/x"), c.keyword("demo", "x")), true);
assert.equal(c.equal(kw, "demo/x"), false);
assert.equal(c.equal(c.keyword("a/b/c"), c.keyword(null, "a/b/c")), true);
assert.equal(c.name(c.keyword("a/b/c")), "a");
assert.equal(c.namespace(c.keyword("a/b/c")), null);
assert.equal(runtime.inspect(c.keyword("a/b/c")).spelling, "a/b/c");
assert.equal(c.equal(c.keyword("a/b/c"), c.keyword("a")), false);
assert.equal(c.name(c.keyword("/")), "/");
assert.equal(c.namespace(c.keyword("/")), null);
assert.equal(c.namespace(c.keyword("/x")), "");
assert.equal(c.name(c.keyword("ns/")), "");
assert.equal(c.namespace(c.keyword(c.keyword("other/ns"), "x")), "ns");
assert.equal(c.name(c.keyword("ns", c.keyword("other/x"))), "x");
for (const value of [null, undefined, 1, false, {}, []]) assert.equal(c.keyword(value), null);
for (const args of [[], ["a", "x", "extra"]]) throwsError(() => c.keyword(...args));
for (const args of [[1, "x"], ["ns", null]]) throwsError(() => c.keyword(...args));
assert.equal(c.name("a/b"), "a/b");
for (const value of [null, undefined, 1, false, v, {}]) throwsError(() => c.name(value));
for (const value of [null, undefined, "x", 1, false, v]) throwsError(() => c.namespace(value));
assert.equal(c.name(kw, "ignored"), "x");
assert.equal(c.namespace(kw, "ignored"), "demo");

assert.equal(c.count(), 0);
assert.equal(c.count(null), 0);
assert.equal(c.count(v, "ignored"), 3);
assert.equal(c.count(runtime.vector()), 0);
const originalItems = [1, 2];
const copiedVector = runtime.vectorFromArray(originalItems);
originalItems[0] = 99;
originalItems.push(3);
assert.equal(Object.isFrozen(originalItems), false);
assert.deepEqual(runtime.inspect(copiedVector).items, [1, 2]);
const largeVector = runtime.vectorFromArray(Array.from({length: 150000}, (_, i) => i));
assert.equal(c.count(largeVector), 150000);
assert.equal(c.nth(largeVector, 149999), 149999);
assert.equal(c.count("\ud800\udc00"), 2);
assert.equal(c.count([]), 0);
assert.equal(c.count([1, 2]), 2);
for (const value of [false, 1, kw, {}]) throwsError(() => c.count(value));
for (const [index, expected] of [[0, 10], [-0, 10], [0.9, 10], [1.8, 20], [2.9, 30]]) {
  assert.equal(c.nth(v, index), expected);
  assert.equal(c.get(v, index), expected);
  assert.equal(v.call(null, index, "ignored"), expected);
  assert.equal(c.contains(v, index), Number.isInteger(index));
}
for (const index of [-1, -0.9, 3, Infinity, -Infinity, NaN]) {
  throwsError(() => c.nth(v, index));
  throwsError(() => v.call(null, index));
  assert.equal(c.nth(v, index, "missing"), "missing");
  assert.equal(c.get(v, index, "missing"), "missing");
  assert.equal(c.contains(v, index), false);
}
for (const index of ["0", "1.8", "", true, false, null, undefined, kw]) {
  throwsError(() => c.nth(v, index));
  throwsError(() => c.nth(null, index));
  throwsError(() => c.nth(null, index, "missing"));
  throwsError(() => v.call(null, index));
  assert.equal(c.get(v, index, "missing"), "missing");
  assert.equal(c.contains(v, index), false);
}
assert.equal(c.nth(null, 0), null);
assert.equal(c.nth(undefined, 0), undefined);
assert.equal(c.nth(null, NaN), null);
assert.equal(c.nth(null, 0, "missing"), "missing");
assert.equal(c.nth(null, 0, null), null);
assert.equal(c.nth(v, 9, null), null);
assert.equal(c.get(v, 9, null), null);
for (const f of [c.get, c.nth]) {
  for (const args of [[], [v], [v, 0, "missing", "extra"]]) throwsError(() => f(...args));
}
throwsError(() => v.call(null));
assert.equal(c.contains(), false);
assert.equal(c.contains(v), false);
assert.equal(c.get(null, 0), null);
assert.equal(c.get(undefined, 0, false), false);
assert.equal(c.get(runtime.vector(null, false, undefined), 0, "missing"), null);
assert.equal(c.get(runtime.vector(null, false, undefined), 1, "missing"), false);
assert.equal(c.get(runtime.vector(null, false, undefined), 2, "missing"), undefined);

for (const [index, expected] of [[0, "a"], [-0.9, "a"], [1.8, "b"], ["0", "a"], ["1.8", "b"], ["", "a"], [false, "a"], [true, "b"]]) {
  assert.equal(c.get("abc", index, "missing"), expected);
  assert.equal(c.contains("abc", index), true);
  if (typeof index === "number") assert.equal(c.nth("abc", index), expected);
  else throwsError(() => c.nth("abc", index));
}
for (const index of [-1, 3, null, undefined, NaN, kw, runtime.vector()]) {
  assert.equal(c.get("abc", index, "missing"), "missing");
  assert.equal(c.contains("abc", index), false);
}
assert.equal(c.nth("\ud800", 0), "\ud800");
assert.equal(c.name(c.keyword("\ud800")), "\ud800");
assert.equal(c.namespace(c.keyword("\ud800", "\udfff")), "\ud800");
assert.equal(c.get("abc", 3, undefined), undefined);
throwsError(() => c.nth("abc", 3));
assert.equal(c.nth("abc", 3, "missing"), "missing");
assert.equal(kw.call(null, v), null);
assert.equal(kw.call(null, v, "missing"), "missing");
assert.equal(kw.call(null, v, null), null);
throwsError(() => kw.call(null));
throwsError(() => kw.call(null, v, null, "extra"));

assert.equal(c.equal(), true);
assert.equal(c.equal(NaN), true);
assert.equal(c.equal(NaN, NaN), false);
assert.equal(c.equal(-0, 0), true);
assert.equal(c.equal(null, undefined), true);
assert.equal(c.equal(runtime.vector(), null), false);
assert.equal(c.equal(runtime.vector(), []), false);
assert.equal(c.equal(runtime.vector(kw, runtime.vector(null)), runtime.vector(c.keyword("demo/x"), runtime.vector(undefined))), true);
assert.equal(c.equal(runtime.vector(1, 2), runtime.vector(1)), false);
assert.equal(c.equal(runtime.vector(1), runtime.vector(2)), false);
assert.equal(c.equal(runtime.vector(NaN), runtime.vector(NaN)), false);
const nanVector = runtime.vector(NaN);
assert.equal(c.equal(nanVector, nanVector), true);
const host = {kind: "keyword", namespace: "demo", name: "x"};
assert.equal(c.equal(host, host), true);
assert.equal(c.equal(host, {...host}), false);
assert.equal(c.equal(kw, host), false);
assert.equal(c.equal(runtime.vector(host), runtime.vector(host)), true);
assert.equal(runtime.inspect(host), null);
assert.equal(runtime.inspect([10, 20, 30]), null);
assert.equal(runtime.inspect(Object.create(Object.getPrototypeOf(kw))), null);
assert.equal(runtime.inspect(Object.create(Object.getPrototypeOf(v))), null);
assert.equal(c.get([10], 0, "missing"), 10);
assert.equal(c.get({0: 10}, 0, "missing"), "missing");
assert.equal(c.nth([10], 0), 10);
assert.equal(c.nth([10], 1, "missing"), "missing");
throwsError(() => c.nth([10], 1));
throwsError(() => c.nth({}, 0, "missing"));

for (const value of [Object.getPrototypeOf(Object.getPrototypeOf(kw)), Object.getPrototypeOf(Object.getPrototypeOf(v)), runtime, runtime.inspect(v), runtime.inspect(v).items]) {
  assert.equal(Object.isFrozen(value), true);
}
assert.equal(Object.isExtensible(Object.getPrototypeOf(v)), true);
// Core roots can be temporarily rebound by CLJS binding/with-redefs.
assert.equal(Object.isSealed(c), true);
kw.name = "changed";
assert.equal(c.name(kw), "x");
assert.throws(() => { Object.getPrototypeOf(v).call = () => 99; }, TypeError);
assert.throws(() => { runtime.inspect(v).items[0] = 99; }, TypeError);
assert.equal(c.nth(v, 0), 10);
assert.equal(c.name(kw), "x");
assert.equal(Object.isFrozen(host), false);
assert.equal(Object.isFrozen(runtime.inspect(runtime.vector(host)).items[0]), false);
console.log("bounded JavaScript runtime assertions passed");

// Lists own only immutable cells and share their tails; seq views retain a
// frozen snapshot and advance offsets without copying the remaining elements.
const oneList = c.list(1, 2, 3);
assert.equal(c.rest(c.conj(oneList, 0)), oneList);
assert.equal(c.seq(oneList), oneList);
assert.equal(c.conj(oneList), oneList);
assert.equal(c.next(c.rest(c.rest(oneList))), null);
const empty = c.list();
assert.equal(c.rest(empty), empty);
assert.equal(c.rest(null), empty);
assert.equal(c.seq(empty), null);
assert.equal(c.first(empty), null);
assert.equal(c.next(empty), null);
assert.deepEqual(runtime.inspect(c.conj(null, 1, 2)), {kind: 'list', items: [2, 1]});
assert.deepEqual(runtime.inspect(c.conj(c.seq(v), 0)), {kind: 'seq', items: [0, 10, 20, 30]});
const sourceVector = c.vector(1, 2), sourceSeq = c.seq(sourceVector);
assert.equal(c.rest(c.conj(sourceSeq, 0)), sourceSeq);
assert.deepEqual(runtime.inspect(c.assoc(sourceVector, 0, 9, 2, 3)).items, [9, 2, 3]);
assert.deepEqual(runtime.inspect(sourceVector).items, [1, 2]);
assert.deepEqual(runtime.inspect(sourceSeq).items, [1, 2]);
throwsError(() => c.assoc(sourceVector, 0, 8, 99, 4));
assert.deepEqual(runtime.inspect(sourceVector).items, [1, 2]);
assert.deepEqual(runtime.inspect(c.assoc(sourceVector, .5, 9, 2)).items, [9, 2, null]);
assert.deepEqual(runtime.inspect(c.assoc(sourceVector, 0)).items, [undefined, 2]);
for (const key of [-1, -.5, 3, NaN, Infinity, '0', null]) throwsError(() => c.assoc(sourceVector, key, 9));
for (const values of [[], [1, 2], [kw, c.vector(1)], [null, undefined, false]]) {
  const valuesByKind = [c.vector(...values), c.list(...values)];
  if (values.length) valuesByKind.push(c.seq(c.vector(...values)));
  for (const a of valuesByKind) for (const b of valuesByKind) {
    assert.equal(c.equal(a, b), true);
    assert.equal(runtime.hash(a), runtime.hash(b));
  }
}
assert.equal(runtime.hash(null), runtime.hash(undefined));
assert.equal(runtime.hash(0), runtime.hash(-0));
const hostFunction = () => 1;
assert.equal(runtime.hash(host), runtime.hash(host));
assert.equal(runtime.hash(hostFunction), runtime.hash(hostFunction));
assert.notEqual(runtime.hash(host), runtime.hash({...host}));
assert.equal(c.equal(c.list(NaN), c.vector(NaN)), false);
assert.equal(c.equal(c.list(), null), false);
assert.equal(c.equal(c.seq('ab'), c.list('a', 'b')), true);
const sx = c.symbol('a', 'x');
assert.equal(c.symbol(sx), sx);
assert.equal(c.equal(sx, c.symbol('a/x')), true);
assert.equal(c.equal(sx, c.keyword('a/x')), false);
assert.equal(runtime.hash(sx), runtime.hash(c.symbol('a/x')));
assert.equal(c.name(sx), 'x');
assert.equal(c.namespace(sx), 'a');
assert.equal(sx.call(null, oneList, false), false);
for (const args of [[null], [1], [false], [], ['a','x',1]]) throwsError(() => c.symbol(...args));
assert.equal(c.name(c.symbol('/x')), '/x');
assert.equal(c.namespace(c.symbol('/x')), null);
assert.equal(c.name(c.symbol('a/b/c')), 'b/c');
assert.equal(c.name(c.keyword(c.symbol('a/b/c'))), 'b/c');
assert.equal(c.name(c.symbol(c.keyword('a/b/c'))), 'b/c');
assert.equal(c.namespace(c.symbol(undefined, 'x')), undefined);
assert.equal(c.name(c.symbol('\ud800')), '\ud800');
throwsError(() => sx.call(null));
throwsError(() => sx.call(null, null, 1, 2));
assert.equal(sx.call(null, null), null);
for (const value of [oneList, sourceSeq, c.conj(sourceSeq, 0), empty, sx]) {
  assert.equal(Object.isFrozen(value), false);
  assert.equal(Object.isFrozen(Object.getPrototypeOf(Object.getPrototypeOf(value))), true);
  assert.equal(runtime.inspect(Object.create(Object.getPrototypeOf(value))), null);
  const observation = runtime.inspect(value);
  assert.equal(Object.isFrozen(observation), true);
  if (observation.items) assert.equal(Object.isFrozen(observation.items), true);
  if (observation.kind !== 'symbol') {
    assert.equal(value.call, undefined);
    assert.equal(c.get(value, 0, 'missing'), 'missing');
    assert.equal(c.contains(value, 0), false);
  }
}
const ownedListArray = [1, 2];
const ownedList = runtime.listFromArray(ownedListArray);
ownedListArray[0] = 9;
assert.deepEqual(runtime.inspect(ownedList).items, [1, 2]);
assert.equal(Object.isFrozen(ownedListArray), false);
for (const current of [oneList, empty]) {
  for (const i of [-1, -.5, .5, Infinity, NaN, 99]) {
    assert.equal(c.nth(current, i, 'missing'), 'missing');
    throwsError(() => c.nth(current, i));
  }
}
assert.equal(c.nth(c.seq(sourceVector), .5, 'missing'), undefined);
assert.equal(c.nth(c.rest(sourceVector), -1), 1);
assert.equal(c.nth(c.seq('ab'), .5, 'missing'), undefined);
assert.equal(c.nth(c.rest('ab'), -1), 'a');
assert.equal(c.nth(c.seq(largeVector), .5, 'missing'), 'missing');
assert.equal(c.nth(c.conj(c.seq(sourceVector), 0), 1.5, 'missing'), undefined);
assert.deepEqual(runtime.inspect(c.seq('\ud83d\ude00')).items, ['\ud83d', '\ude00']);
// 100,000 cells/views would expose repeated tail copying as quadratic work.
const longItems = Array.from({length: 100000}, (_, i) => i);
const longList = runtime.listFromArray(longItems);
const longVector = runtime.vectorFromArray(longItems);
for (const collection of [longList, longVector]) {
  let traversed = 0;
  for (let current = c.seq(collection); current !== null; current = c.next(current)) {
    assert.equal(c.first(current), traversed++);
    assert.equal(c.count(current), 100001 - traversed);
  }
  assert.equal(traversed, 100000);
}
assert.equal(c.nth(longList, 99999), 99999);
assert.equal(c.equal(longList, longVector), true);
assert.equal(runtime.hash(longList), runtime.hash(longVector));
console.log('symbol, list, sequence and update assertions passed');

// Immutable structural-key maps: keys, values and hashes cannot be replaced
// through escaped wrappers, entry observations or callable prototypes.
const k = c.keyword('k'), nested = c.vector(c.list(1, 2));
const m0 = c.hashMap(k, nested, c.vector(1, 2), 'pair', '__proto__', false, null, undefined);
assert.equal(c.get(m0, c.list(1, 2)), 'pair');
assert.equal(c.get(m0, c.seq(c.vector(1, 2))), 'pair');
assert.equal(c.get(m0, '__proto__', 'missing'), false);
assert.equal(c.get(m0, undefined, 'missing'), undefined);
assert.equal(c.contains(m0, undefined), true);
assert.equal(c.get(m0, 'absent', 'missing'), 'missing');
assert.equal(c.contains(m0, 'absent'), false);
assert.equal(k.call(null, m0), nested);
assert.equal(m0.call(null, k), nested);
assert.equal(m0.call(null, 'absent', false), false);
throwsError(() => m0.call(null));
throwsError(() => m0.call(null, k, null, false));
const m1 = c.assoc(m0, k, c.conj(nested, 3), c.list(1, 2), 'changed', 'new');
const m2 = c.dissoc(m1, k, c.seq(c.vector(1, 2)), null);
assert.equal(c.get(m0, k), nested);
assert.equal(c.get(m0, c.vector(1, 2)), 'pair');
assert.equal(c.get(m1, c.vector(1, 2)), 'changed');
assert.equal(c.get(m1, 'new', 'missing'), null);
assert.equal(c.count(m0), 4);
assert.equal(c.count(m1), 5);
assert.equal(c.count(m2), 2);
assert.equal(c.dissoc(m0), m0);
assert.equal(c.dissoc(null, k), null);
assert.equal(c.dissoc(undefined, k), undefined);
assert.equal(c.conj(m0, null), m0);
const originalKey = runtime.inspect(m0).entries[1][0];
assert.equal(runtime.inspect(m1).entries[1][0], originalKey);
assert.equal(c.first(c.get(m0, k)), c.first(c.get(m1, k)));
assert.equal(c.equal(c.assoc(null, k, 1), c.hashMap(k, 1)), true);
const reverse = c.hashMap(null, undefined, '__proto__', false, c.list(1, 2), 'pair', c.keyword('k'), c.vector(c.vector(1, 2)));
assert.equal(c.equal(m0, reverse), true);
assert.equal(runtime.hash(m0), runtime.hash(reverse));
assert.equal(c.get(c.hashMap(m0, 99), reverse), 99);
assert.equal(c.equal(c.hashMap(k, null), c.hashMap(k, undefined)), true);
assert.equal(c.equal(c.hashMap(k, null), c.hashMap('different', null)), false);
assert.equal(c.equal(c.hashMap(), c.vector()), false);
assert.equal(c.equal(c.hashMap(k, 1), c.hashMap(k, 2)), false);
assert.equal(c.equal(c.hashMap(k, 1), c.hashMap()), false);
const distinctNames = c.hashMap('k', 1, k, 2, c.symbol('k'), 3);
assert.equal(c.count(distinctNames), 3);
assert.equal(c.symbol('k').call(null, distinctNames), 3);
const foreign1 = {}, foreign2 = {}, foreignMap = c.hashMap(foreign1, 1, foreign2, 2, hostFunction, 3);
foreign1.value = 'changed';
assert.equal(c.get(foreignMap, foreign1), 1);
assert.equal(c.get(foreignMap, foreign2), 2);
assert.equal(c.get(foreignMap, hostFunction), 3);
assert.equal(c.get(foreignMap, {}), null);
assert.equal(Object.isFrozen(foreign1), false);
// Actual polynomial-hash collisions, including nested sequential/map keys.
assert.equal(runtime.hash('Aa'), runtime.hash('BB'));
for (const [a, b] of [['Aa', 'BB'], [c.vector('Aa'), c.list('BB')], [c.hashMap('Aa', 1), c.hashMap('BB', 1)]]) {
  assert.equal(runtime.hash(a), runtime.hash(b));
  assert.equal(c.equal(a, b), false);
  const collision = c.hashMap(a, 'a', b, 'b');
  assert.equal(c.count(collision), 2);
  assert.equal(c.get(collision, a), 'a');
  assert.equal(c.get(collision, b), 'b');
  assert.equal(c.get(c.assoc(collision, a, 'new'), b), 'b');
  assert.equal(c.get(c.dissoc(collision, a), b), 'b');
}
assert.equal(c.seq(c.hashMap()), null);
assert.equal(c.rest(c.hashMap()), empty);
assert.equal(c.next(c.hashMap()), null);
assert.equal(c.count(c.seq(m0)), c.count(m0));
for (let current = c.seq(m0); current !== null; current = c.next(current)) {
  const entry = c.first(current);
  assert.equal(runtime.inspect(entry).kind, 'vector');
  assert.equal(c.count(entry), 2);
  assert.equal(c.get(m0, c.nth(entry, 0)), c.nth(entry, 1));
}
for (const member of [c.vector(), c.vector(k), c.list(k, 1), c.list(c.vector(k, 1), 99), false, 1, 'x']) {
  throwsError(() => c.conj(m0, member));
  assert.equal(c.count(m0), 4);
  assert.equal(c.get(m0, k), nested);
}
assert.equal(c.equal(c.conj(c.hashMap(), c.vector(k, 1, 2)), c.hashMap(k, 1)), true);
assert.equal(c.equal(c.conj(c.hashMap(), c.list(c.vector(k, 1))), c.hashMap(k, 1)), true);
assert.equal(c.equal(c.conj(c.hashMap(), c.seq(c.vector(c.vector(k, 1)))), c.hashMap(k, 1)), true);
assert.equal(c.equal(c.conj(c.hashMap(), m0), m0), true);
for (const args of [[k], [k, 1, 'odd']]) throwsError(() => c.hashMap(...args));
for (const value of [v, oneList, false, 1, {}]) throwsError(() => c.dissoc(value, k));
const suppliedPairs = [k, nested];
const copiedMap = runtime.mapFromArray(suppliedPairs);
suppliedPairs[1] = 99;
assert.equal(Object.isFrozen(suppliedPairs), false);
assert.equal(c.get(copiedMap, k), nested);
const mapObservation = runtime.inspect(m0);
for (const value of [Object.getPrototypeOf(Object.getPrototypeOf(m0)), mapObservation, mapObservation.entries, ...mapObservation.entries])
  assert.equal(Object.isFrozen(value), true);
assert.throws(() => { mapObservation.entries[0][1] = 99; }, TypeError);
assert.throws(() => { mapObservation.entries.push([1, 2]); }, TypeError);
assert.throws(() => { Object.getPrototypeOf(m0).call = () => 99; }, TypeError);
m0.hash = 0;
assert.equal(c.get(c.hashMap(m0, 9), reverse), 9);
assert.equal(runtime.inspect(Object.create(Object.getPrototypeOf(m0))), null);
// Retain every version across repeated replacements and deletions.
const retained = [c.hashMap(k, 0)];
for (let i = 1; i <= 512; i++) retained.push(c.assoc(retained.at(-1), k, i, i, nested));
for (let i = 0; i < retained.length; i++) {
  assert.equal(c.get(retained[i], k), i);
  assert.equal(c.count(retained[i]), i + 1);
}
console.log('structural map, collision, sharing and mutation assertions passed');

// Hash maps are hash array mapped tries (light-cljs-235): built one `assoc`
// at a time, at once or through a transient, past array nodes and down to
// collisions, they agree on count, lookup, equality, hash and traversal, and
// every version survives later updates.
{
  const n = 3000, keys = [];
  for (let i = 0; i < n; i++) keys.push(i % 3 === 0 ? i : i % 3 === 1 ? 'k' + i : c.keyword('n', 'k' + i));
  const order = m => runtime.inspect(m).entries.map(([key]) => key);
  let built = c.hashMap();
  const versions = [];
  for (let i = 0; i < n; i++) {
    built = c.assoc(built, keys[i], i);
    if (i % 499 === 0) versions.push([built, i + 1]);
  }
  const whole = runtime.mapFromArray(keys.flatMap((key, i) => [key, i]), 'hash-map');
  let editing = c.transient(c.arrayMap());
  for (let i = 0; i < n; i++) editing = c.assocBang(editing, keys[i], i);
  const viaTransient = c.persistentBang(editing);
  for (const m of [built, whole, viaTransient]) {
    assert.equal(c.count(m), n);
    assert.equal(c.equal(m, built), true);
    assert.equal(c.hash(m), c.hash(whole));
    assert.deepEqual(order(m), order(whole));
    for (let i = 0; i < n; i++) assert.equal(c.get(m, keys[i]), i);
    assert.equal(c.get(m, 'absent', 'missing'), 'missing');
  }
  assert.equal(c.equal(built, c.assoc(built, keys[7], 'changed')), false);
  for (const [version, count] of versions) {
    assert.equal(c.count(version), count);
    assert.equal(c.get(version, keys[count - 1]), count - 1);
    assert.equal(c.get(version, keys[count], 'missing'), 'missing');
  }
  let removed = built;
  for (let i = 0; i < n; i += 2) removed = c.dissoc(removed, keys[i]);
  assert.equal(c.count(removed), n / 2);
  assert.equal(c.count(c.dissoc(removed, keys[0], 'absent')), n / 2);
  const remaining = keys.flatMap((key, i) => i % 2 ? [key, i] : []);
  assert.equal(c.equal(removed, runtime.mapFromArray(remaining, 'hash-map')), true);
  assert.deepEqual(order(removed), order(runtime.mapFromArray(remaining, 'hash-map')));
  for (let i = 0; i < n; i++) assert.equal(c.get(removed, keys[i], 'missing'), i % 2 ? i : 'missing');
  assert.equal(c.count(built), n);
  assert.equal(c.count(c.reduce((m, key) => c.dissoc(m, key), built, c.vector(...keys))), 0);
  // Keys hash as they compare: -0 is 0, nil is undefined, a list finds a
  // vector, NaN finds nothing, and same-named strings, keywords and symbols
  // stay apart.
  assert.equal(c.get(c.assoc(built, -0, 'zero'), 0), 'zero');
  assert.equal(c.count(c.assoc(built, -0, 'zero')), n);
  const withNil = c.assoc(built, undefined, 'nil');
  assert.equal(c.get(withNil, null), 'nil');
  assert.equal(c.count(withNil), n + 1);
  assert.equal(order(withNil)[0], undefined);
  assert.equal(c.count(c.dissoc(withNil, null)), n);
  assert.equal(c.get(c.assoc(built, c.vector(1, 2), 'pair'), c.list(1, 2)), 'pair');
  const nan = c.assoc(c.assoc(built, NaN, 1), NaN, 2);
  assert.equal(c.count(nan), n + 2);
  assert.equal(c.get(nan, NaN, 'missing'), 'missing');
  assert.equal(c.count(c.assoc(built, 'x', 1, c.keyword(null, 'x'), 2, c.symbol(null, 'x'), 3)), n + 3);
  // Keys of equal hash share a collision node.
  const collided = c.assoc(c.assoc(built, 'Aa', 'a'), 'BB', 'b');
  assert.equal(c.hash('Aa'), c.hash('BB'));
  assert.deepEqual([c.get(collided, 'Aa'), c.get(collided, 'BB'), c.count(collided)], ['a', 'b', n + 2]);
  assert.equal(c.get(c.dissoc(collided, 'Aa'), 'BB'), 'b');
  const marked = c.withMeta(built, c.hashMap('m', 1));
  assert.equal(c.get(c.meta(c.dissoc(c.assoc(marked, 'k', 1), 'k')), 'm'), 1);
  // A transient over a hash map edits its own copy of the trie.
  let editing2 = c.transient(built);
  for (let i = 0; i < n; i += 3) editing2 = c.assocBang(editing2, keys[i], 'x' + i);
  for (let i = 1; i < n; i += 3) editing2 = c.dissocBang(editing2, keys[i]);
  editing2 = c.assocBang(c.assocBang(editing2, null, 'nil'), 'Aa', 'a', 'BB', 'b');
  assert.equal(c.count(editing2), n - n / 3 + 3);
  const edited = c.persistentBang(editing2);
  assert.equal(c.get(editing2, keys[0]), 'x0');
  const expected = keys.flatMap((key, i) => i % 3 === 1 ? [] : [key, i % 3 ? i : 'x' + i]).concat([null, 'nil', 'Aa', 'a', 'BB', 'b']);
  assert.equal(c.equal(edited, runtime.mapFromArray(expected, 'hash-map')), true);
  assert.deepEqual(order(edited), order(runtime.mapFromArray(expected, 'hash-map')));
  assert.deepEqual([c.count(built), c.get(built, keys[3]), c.get(built, keys[1])], [n, 3, 1]);
  // A transient changes in place the nodes and payload it made (light-cljs-249).
  // What `persistent!` returns never changes: two later transients of it edit
  // their own copies, and one emptied to nothing, through array nodes packed
  // back into bitmap nodes, and filled again agrees with a map built at once.
  const unchanged = (m, count, value) => {
    assert.equal(c.count(m), count);
    for (let i = 0; i < n; i++) assert.equal(c.get(m, keys[i], 'missing'), value(i));
  };
  let growing = c.transient(c.hashMap());
  for (let i = 0; i < n; i++) growing = c.assocBang(growing, keys[i], i);
  const grown = c.persistentBang(growing);
  assert.throws(() => c.assocBang(growing, 'late', 1), /assoc! after persistent!/);
  assert.throws(() => c.dissocBang(growing, keys[0]), /dissoc! after persistent!/);
  assert.deepEqual(order(grown), order(whole));
  let halved = c.transient(grown), negated = c.transient(grown);
  for (let i = 0; i < n; i += 2) {
    halved = c.dissocBang(halved, keys[i]);
    negated = c.assocBang(negated, keys[i], -i);
  }
  unchanged(grown, n, i => i);
  assert.deepEqual(order(grown), order(whole));
  const half = c.persistentBang(halved), negative = c.persistentBang(negated);
  unchanged(half, n / 2, i => i % 2 ? i : 'missing');
  unchanged(negative, n, i => i % 2 ? i : -i);
  assert.deepEqual(order(half), order(removed));
  let emptied = c.transient(negative);
  for (const key of keys) emptied = c.dissocBang(emptied, key);
  assert.equal(c.count(emptied), 0);
  assert.equal(c.get(emptied, keys[1], 'missing'), 'missing');
  for (let i = 0; i < n; i++) emptied = c.assocBang(emptied, keys[i], i);
  const refilled = c.persistentBang(emptied);
  assert.equal(c.equal(refilled, whole), true);
  assert.deepEqual(order(refilled), order(whole));
  unchanged(negative, n, i => i % 2 ? i : -i);
  unchanged(grown, n, i => i);
}
console.log('hash array mapped trie assertions passed');

assert.equal(c.dissoc(), null);
const undefinedKeyMap = c.hashMap(undefined, 1);
assert.equal(runtime.inspect(c.assoc(undefinedKeyMap, null, 2)).entries[0][0], undefined);
assert.equal(c.get(undefinedKeyMap, null), 1);

// Set membership uses structural hashes/equality and retains the first member.
const setVector = c.vector(1, 2), setList = c.list(1, 2);
const s0 = c.hashSet(setVector, setList, null, false, c.hashMap(k, nested));
assert.equal(c.count(s0), 4);
assert.equal(c.get(s0, setList), setVector);
assert.equal(s0.call(null, setList), setVector);
assert.equal(c.get(s0, undefined, 'missing'), null);
assert.equal(c.get(s0, false, 'missing'), false);
assert.equal(s0.call(null, false, 'missing'), false);
assert.equal(c.contains(s0, undefined), true);
assert.equal(c.contains(s0, false), true);
assert.equal(c.contains(s0, 'missing'), false);
assert.equal(s0.call(null, 123), null);
assert.equal(s0.call(null, 123, false), false);
throwsError(() => s0.call(null));
throwsError(() => s0.call(null, 1, 2, 3));
const s1 = c.conj(s0, setList, 9, 9), s2 = c.disj(s1, c.list(1, 2), null, 123);
assert.equal(c.count(s0), 4);
assert.equal(c.count(s1), 5);
assert.equal(c.count(s2), 3);
assert.equal(c.get(s1, setList), setVector);
assert.equal(c.contains(s2, setVector), false);
assert.equal(c.get(s0, c.hashMap(k, c.list(c.vector(1, 2)))) , runtime.inspect(s0).items[3]);
assert.equal(c.get(s1, runtime.inspect(s0).items[3]), runtime.inspect(s0).items[3]);
assert.equal(c.disj(s0), s0);
assert.equal(c.disj(123), 123);
assert.equal(c.disj(null, 1), null);
assert.equal(c.disj(undefined), undefined);
assert.equal(c.disj(undefined, 1), null);
assert.equal(c.disj(), null);
assert.equal(c.equal(c.disj(s0, 'missing'), s0), true);
const reverseSet = c.hashSet(c.hashMap(k, c.list(c.vector(1, 2))), false, undefined, setList);
assert.equal(c.equal(s0, reverseSet), true);
assert.equal(runtime.hash(s0), runtime.hash(reverseSet));
assert.equal(c.get(c.hashMap(s0, 'set key'), reverseSet), 'set key');
assert.equal(c.get(c.hashSet(s0), reverseSet), s0);
assert.equal(c.equal(c.hashSet(1), c.hashSet(2)), false);
assert.equal(c.equal(c.hashSet(1), c.hashSet(1, 2)), false);
for (const x of [c.vector(1), c.list(1), c.hashMap(1, 1), new Set([1]), [1]]) {
  assert.equal(c.equal(c.hashSet(1), x), false);
  assert.equal(c.equal(x, c.hashSet(1)), false);
}
for (const [a, b] of [['Aa', 'BB'], [c.vector('Aa'), c.list('BB')], [c.hashSet('Aa'), c.hashSet('BB')]]) {
  assert.equal(runtime.hash(a), runtime.hash(b));
  const collision = c.hashSet(a, b);
  assert.equal(c.count(collision), 2);
  assert.equal(c.get(collision, a), a);
  assert.equal(c.get(collision, b), b);
  assert.equal(c.get(c.disj(collision, a), b), b);
  assert.equal(c.contains(c.disj(collision, a), a), false);
}
const inputMembers = [setVector, false], copiedSet = runtime.setFromArray(inputMembers);
inputMembers[0] = 99; inputMembers.push(9);
assert.equal(Object.isFrozen(inputMembers), false);
assert.equal(c.get(copiedSet, setList), setVector);
assert.equal(c.count(copiedSet), 2);
const setObservation = runtime.inspect(s0);
for (const value of [Object.getPrototypeOf(Object.getPrototypeOf(s0)), setObservation, setObservation.items])
  assert.equal(Object.isFrozen(value), true);
assert.throws(() => {setObservation.items[0] = 99;}, TypeError);
assert.throws(() => {setObservation.items.push(9);}, TypeError);
assert.throws(() => {Object.getPrototypeOf(s0).call = () => 99;}, TypeError);
s0.hash = 0;
assert.equal(runtime.inspect(Object.create(Object.getPrototypeOf(s0))), null);
const hostSet = new Set([1]), hostMember = {value: 1}, identitySet = c.hashSet(hostSet, hostMember, hostFunction);
hostSet.add(2); hostMember.value = 2;
assert.equal(c.count(identitySet), 3);
assert.equal(c.get(identitySet, hostSet), hostSet);
assert.equal(c.contains(identitySet, {value: 2}), false);
assert.equal(c.contains(identitySet, hostFunction), true);
assert.equal(Object.isFrozen(hostSet), false);
assert.equal(Object.isFrozen(hostMember), false);
assert.equal(runtime.inspect(hostSet), null);
assert.equal(c.get(hostSet, 1, 'missing'), 'missing');
for (const x of [hostSet, c.vector(), c.hashMap(), 1, false]) throwsError(() => c.disj(x, 1));
assert.equal(c.seq(c.hashSet()), null);
assert.equal(c.first(c.hashSet()), null);
assert.equal(c.rest(c.hashSet()), empty);
assert.equal(c.next(c.hashSet()), null);
assert.equal(runtime.inspect(c.seq(s0)).kind, 'seq');
let seenSet = c.hashSet(), setVisits = 0;
for (let current = c.seq(s0); current !== null; current = c.next(current)) {
  assert.equal(c.count(current), c.count(s0) - setVisits);
  assert.equal(c.contains(s0, c.first(current)), true);
  assert.equal(c.contains(seenSet, c.first(current)), false);
  seenSet = c.conj(seenSet, c.first(current)); setVisits++;
}
assert.equal(setVisits, c.count(s0));
assert.equal(c.equal(seenSet, s0), true);
const setVersions = [c.hashSet(nested)];
for (let i = 0; i < 512; i++) setVersions.push(c.conj(setVersions.at(-1), i));
for (let i = 0; i < setVersions.length; i++) {
  assert.equal(c.count(setVersions[i]), i + 1);
  assert.equal(c.get(setVersions[i], nested), nested);
  assert.equal(c.contains(setVersions[i], i), false);
  assert.equal(c.count(c.disj(setVersions[i], nested)), i);
}
const undefinedSet = c.hashSet(undefined, null);
assert.equal(c.count(undefinedSet), 1);
assert.equal(c.get(undefinedSet, null, 'missing'), undefined);
assert.equal(c.contains(undefinedSet, null), true);
assert.equal(runtime.inspect(undefinedSet).items[0], undefined);
assert.equal(runtime.hash(c.hashSet(-0)), runtime.hash(c.hashSet(0)));
assert.equal(c.equal(c.hashSet(NaN), c.hashSet(NaN)), false);
const nanSet = c.hashSet(NaN);
assert.equal(c.equal(nanSet, nanSet), true);
console.log('structural set, collision, representative, sharing and mutation assertions passed');

// Hash sets keep their members in a trie, as hash maps (light-cljs-236):
// built one `conj` at a time, at once, with `set` and `into` and through a
// transient, past array nodes and down to collisions, they agree on count,
// lookup, equality, hash and traversal, and every version survives later
// updates.
{
  const n = 3000, keys = [];
  for (let i = 0; i < n; i++) keys.push(i % 3 === 0 ? i : i % 3 === 1 ? 'k' + i : c.keyword('n', 'k' + i));
  const order = s => runtime.inspect(s).items;
  let built = c.hashSet();
  const versions = [];
  for (let i = 0; i < n; i++) {
    built = c.conj(built, keys[i]);
    if (i % 499 === 0) versions.push([built, i + 1]);
  }
  const whole = runtime.setFromArray(keys);
  let editing = c.transient(c.hashSet());
  for (let i = 0; i < n; i++) editing = c.conjBang(editing, keys[i]);
  const viaTransient = c.persistentBang(editing);
  const viaSet = c.set(c.vector(...keys)), viaInto = c.into(c.hashSet(), c.vector(...keys));
  for (const s of [built, whole, viaTransient, viaSet, viaInto]) {
    assert.equal(c.count(s), n);
    assert.equal(c.equal(s, built), true);
    assert.equal(c.equal(built, s), true);
    assert.equal(c.hash(s), c.hash(whole));
    assert.deepEqual(order(s), order(whole));
    for (let i = 0; i < n; i++) {
      assert.equal(c.get(s, keys[i]), keys[i]);
      assert.equal(s.call(null, keys[i]), keys[i]);
      assert.equal(s.has(keys[i]), true);
    }
    assert.equal(c.get(s, 'absent', 'missing'), 'missing');
    assert.equal(c.contains(s, 'absent'), false);
  }
  // The hash is that of the members, which an array set of a few also has.
  assert.equal(c.hash(c.disj(whole, ...keys.slice(3))), c.hash(c.hashSet(...keys.slice(0, 3))));
  assert.equal(c.conj(built, keys[5]), built);
  assert.equal(c.equal(built, c.conj(c.disj(built, keys[7]), 'other')), false);
  for (const [version, count] of versions) {
    assert.equal(c.count(version), count);
    assert.equal(c.contains(version, keys[count - 1]), true);
    assert.equal(c.contains(version, keys[count]), false);
  }
  let removed = built;
  for (let i = 0; i < n; i += 2) removed = c.disj(removed, keys[i]);
  assert.equal(c.count(removed), n / 2);
  assert.equal(c.disj(removed, keys[0], 'absent'), removed);
  const remaining = keys.filter((_, i) => i % 2);
  assert.equal(c.equal(removed, runtime.setFromArray(remaining)), true);
  assert.deepEqual(order(removed), order(runtime.setFromArray(remaining)));
  for (let i = 0; i < n; i++) assert.equal(c.contains(removed, keys[i]), i % 2 === 1);
  assert.equal(c.count(c.disj(built, ...keys)), 0);
  assert.equal(c.count(built), n);
  // Members hash as they compare: -0 is 0, nil is undefined, a list finds a
  // vector, NaN finds nothing, and same-named strings, keywords and symbols
  // stay apart; members of equal hash share a collision node.
  assert.equal(c.conj(built, -0), built);
  const withNil = c.conj(built, undefined);
  assert.equal(c.get(withNil, null, 'missing'), undefined);
  assert.equal(c.count(withNil), n + 1);
  assert.equal(order(withNil)[0], undefined);
  assert.equal(c.count(c.disj(withNil, null)), n);
  const pair = c.vector(1, 2);
  assert.equal(c.get(c.conj(built, pair), c.list(1, 2)), pair);
  const nan = c.conj(c.conj(built, NaN), NaN);
  assert.equal(c.count(nan), n + 2);
  assert.equal(c.contains(nan, NaN), false);
  assert.equal(c.count(c.conj(built, 'x', c.keyword(null, 'x'), c.symbol(null, 'x'))), n + 3);
  const collided = c.conj(built, 'Aa', 'BB');
  assert.deepEqual([c.get(collided, 'Aa'), c.get(collided, 'BB'), c.count(collided)], ['Aa', 'BB', n + 2]);
  assert.equal(c.get(c.disj(collided, 'Aa'), 'BB'), 'BB');
  assert.equal(c.contains(c.disj(collided, 'Aa'), 'Aa'), false);
  // Metadata survives conj, disj and empty, which keeps the set's kind.
  const marked = c.withMeta(built, c.hashMap('m', 1));
  assert.equal(c.get(c.meta(c.disj(c.conj(marked, 'k'), 'k')), 'm'), 1);
  assert.equal(c.get(c.meta(c.conj(marked, keys[3])), 'm'), 1);
  assert.equal(c.equal(marked, built), true);
  const emptied = c.empty(marked);
  assert.deepEqual([c.count(emptied), c.get(c.meta(emptied), 'm')], [0, 1]);
  assert.deepEqual(order(c.conj(emptied, ...keys)), order(whole));
  // A transient over a hash set edits its own copy of the trie; its lookup
  // returns the query, as upstream's TransientHashSet.
  let editing2 = c.transient(built);
  for (let i = 0; i < n; i += 3) editing2 = c.disjBang(editing2, keys[i]);
  editing2 = c.conjBang(editing2, null, 'Aa', 'BB', keys[1]);
  assert.equal(c.count(editing2), n - n / 3 + 3);
  assert.equal(c.get(editing2, keys[0], 'missing'), 'missing');
  const query = c.list(1, 2);
  editing2 = c.conjBang(editing2, pair);
  assert.equal(c.get(editing2, query), query);
  const edited = c.persistentBang(editing2);
  assert.throws(() => c.conjBang(editing2, 'late'), /conj! after persistent!/);
  assert.equal(c.get(editing2, keys[1]), keys[1]);
  const expected = keys.filter((_, i) => i % 3).concat([null, 'Aa', 'BB', pair]);
  assert.equal(c.equal(edited, runtime.setFromArray(expected)), true);
  assert.deepEqual(order(edited), order(runtime.setFromArray(expected)));
  assert.deepEqual([c.count(built), c.contains(built, keys[3]), c.contains(built, 'Aa')], [n, true, false]);
  // What `persistent!` returns never changes: later transients edit their own
  // copies.
  let halved = c.transient(viaTransient), refilled = c.transient(viaTransient);
  for (let i = 0; i < n; i += 2) halved = c.disjBang(halved, keys[i]);
  for (let i = 0; i < n; i++) refilled = c.disjBang(refilled, keys[i]);
  assert.equal(c.count(refilled), 0);
  for (let i = 0; i < n; i++) refilled = c.conjBang(refilled, keys[i]);
  assert.deepEqual(order(c.persistentBang(halved)), order(removed));
  assert.deepEqual(order(c.persistentBang(refilled)), order(whole));
  assert.equal(c.count(viaTransient), n);
  assert.deepEqual(order(viaTransient), order(whole));
  // From review: a set the public constructor built of nil has no kind, and
  // a lookup gives its member array a root; past 8 members its transient, or
  // `conj`, still makes a hash set, which a transient looks up after
  // persistence.
  const kindless = c.PersistentHashSet(null, null, null);
  assert.equal(c.get(kindless, 'missing'), null);
  let grown = c.transient(kindless);
  for (let i = 0; i < 9; i++) grown = c.conjBang(grown, i);
  const persisted = c.persistentBang(grown);
  assert.deepEqual([c.count(persisted), c.get(grown, 0), c.get(persisted, 8)], [9, 0, 8]);
  assert.deepEqual(order(persisted), order(runtime.setFromArray([0, 1, 2, 3, 4, 5, 6, 7, 8])));
  let conjoined = kindless;
  for (let i = 0; i < 9; i++) conjoined = c.conj(conjoined, i);
  assert.equal(c.equal(conjoined, persisted), true);
  assert.deepEqual(order(conjoined), order(persisted));
}
console.log('hash set trie assertions passed');

assert.equal(c.count(runtime.setFromArray(Array(150000).fill(nested))), 1);

// Finite reductions use the ordinary call method afresh at each step and never
// invoke a completion arity. Empty/singleton paths preserve marker identity.
const marker = c.reduced(v), nestedMarker = c.reduced(marker);
assert.equal(c.isReduced(marker), true);
assert.equal(c.isReduced(Object.create(Object.getPrototypeOf(marker))), false);
assert.equal(c.isReduced({val: v}), false);
assert.equal(c.deref(marker), v);
assert.equal(c.deref(nestedMarker), marker);
assert.equal(c.unreduced(nestedMarker), marker);
assert.equal(c.unreduced(v), v);
assert.deepEqual(runtime.inspect(marker), {kind: 'opaque'});
for (const value of [Object.getPrototypeOf(Object.getPrototypeOf(marker))]) assert.equal(Object.isFrozen(value), true);
assert.equal(c.equal(c.reduced(1), c.reduced(1)), false);
assert.equal(c.equal(marker, marker), true);
assert.equal(c.deref(c.reduced(host)), host);
assert.equal(Object.isFrozen(host), false);
// The public Reduced val slot is live, just like direct positional construction.
marker.val = 9;
assert.equal(c.deref(marker), 9);
marker.val = v;
const forgedReduced = Object.create(Object.getPrototypeOf(marker));
assert.throws(() => { forgedReduced.val = 9; }, /Expected Reduced/);
assert.equal(c.isReduced(forgedReduced), false);
for (const value of [null, undefined, 0, {}, c.vector(), c.list()]) throwsError(() => c.deref(value));
for (const collection of [null, undefined, c.vector(), c.list(), c.hashMap(), c.hashSet(), '', c.seq(c.vector())]) {
  assert.equal(c.reduce(null, marker, collection), marker);
  assert.equal(c.reduce(function() {assert.equal(arguments.length, 0); return marker;}, collection), marker);
}
for (const collection of [c.vector(marker), c.list(marker), c.hashSet(marker), c.seq(c.vector(marker))]) {
  assert.equal(c.reduce(null, collection), marker);
  const result = c.reduce((acc, item) => c.reduced(c.reduced(item)), 0, collection);
  assert.equal(c.isReduced(result), true);
  assert.equal(c.deref(result), marker);
}
const callTrace = [];
const reducer = {
  get call() {
    callTrace.push('get');
    return function(self, a, b) {
      assert.equal(this, reducer);
      assert.equal(self, null);
      assert.equal(arguments.length, 3);
      callTrace.push([a,b]);
      return a+b;
    };
  }
};
assert.equal(c.reduce(reducer, 0, c.vector(1,2,3)), 6);
assert.deepEqual(callTrace, ['get',[0,1],'get',[1,2],'get',[3,3]]);
let steps = 0;
const thrown = {exact: 'thrown identity'};
const changingReducer = {get call() {
  if (steps) throw thrown;
  return (_self,a,b) => {steps++; return a+b;};
}};
assert.throws(() => c.reduce(changingReducer, 0, c.vector(1,2,3)), e => e === thrown);
assert.equal(steps, 1);
for (const collection of [c.vector(1,2,3), c.list(1,2,3), c.seq(c.vector(1,2,3)), c.conj(c.seq(c.vector(2,3)),1), c.hashSet(1,2,3)]) {
  let seen = 0;
  assert.equal(c.reduce(function(a,b) {assert.equal(arguments.length,2); seen++; return a+b;}, 0, collection), 6);
  assert.equal(seen,3);
  seen=0;
  assert.equal(c.reduce(function(a,b) {assert.equal(arguments.length,2); seen++; return c.reduced(a+b);}, 0, collection), c.first(collection));
  assert.equal(seen,1);
}
assert.equal(c.reduce(c.add, largeVector), 11249925000);
assert.equal(c.reduce(c.add, 0, largeVector), 11249925000);
const largeList = runtime.listFromArray(Array.from({length: 150000}, (_, i) => i));
assert.equal(c.reduce(c.add, largeList), 11249925000);
assert.equal(c.reduce(c.add, 0, c.seq(largeVector)), 11249925000);
assert.equal(c.reduce((a,b) => a + b.length, 0, 'x'.repeat(150000)), 150000);
assert.equal(c.reduce(c.add, 'a\ud800\udc00'), 'a\ud800\udc00');
assert.equal(c.reduce(null, 'x'), 'x');
assert.deepEqual(runtime.inspect(c.reduce(null, c.hashMap(kw, 7))).items, [kw, 7]);
assert.equal(c.equal(c.reduce(c.conj, c.hashMap(), c.hashMap(kw, 7)), c.hashMap(kw, 7)), true);
for (const args of [[], [c.add], [c.add,0,c.vector(),9]]) throwsError(() => c.reduce(...args));
for (const value of [false, 1, kw, {}, marker]) throwsError(() => c.reduce(c.add,0,value));
assert.equal(c.add(),0);
assert.equal(c.add(marker),marker);
assert.equal(c.add(undefined),undefined);
assert.equal(c.add(1,2,3,4),10);
assert.equal(c.add(9007199254740991,1,1),9007199254740992);
assert.equal(Object.is(c.add(-0),-0),true);
assert.equal(Object.is(c.add(-0,-0),-0),true);
assert.equal(c.add(1e308,1e308),Infinity);
assert.equal(Number.isNaN(c.add(NaN,1)),true);
console.log('finite reduction assertions passed');

// Numeric primitives: expected values are independent of the shared encoder.
for (const [f, args, expected] of [
  [c.add, [], 0], [c.add, [-0], -0], [c.add, [1, 2, 3, 4], 10],
  [c.subtract, [], NaN], [c.subtract, [0], -0], [c.subtract, [10, 2, 3], 5],
  [c.multiply, [], 1], [c.multiply, [-0], -0], [c.multiply, [2, 3, 4], 24],
  [c.divide, [], NaN], [c.divide, [2], 0.5], [c.divide, [24, 2, 3], 4],
  [c.divide, [1, 0], Infinity], [c.divide, [1, -0], -Infinity], [c.divide, [0, 0], NaN],
  [c.inc, ["2"], "21"], [c.dec, ["2"], 1], [c.add, [null, 2], 2], [c.subtract, [undefined, 1], NaN],
  [c.int, [4294967297.9], 1], [c.int, [2147483648], -2147483648], [c.int, [-0], 0],
  [c.int, [Infinity], 0], [c.int, [NaN], 0], [c.long, [4294967297.9], 4294967297],
  [c.long, [-0.4], -0], [c.long, [-1.9], -1], [c.long, [Infinity], Infinity],
  [c.quot, [-9, 3], -3], [c.quot, [2, -5], -0], [c.quot, [-2, 5], 0],
  [c.quot, [1, 0], NaN], [c.quot, [1, Infinity], 0],
  [c.rem, [-4, 2], 0], [c.rem, [-0, 2], -0], [c.rem, [-4.5, 2], -0.5],
  [c.rem, [1, Infinity], NaN], [c.jsMod, [-4, 2], -0], [c.mod, [-4, 2], 0],
  [c.mod, [4, -2], -0], [c.mod, [-2, 5], 3], [c.mod, [2, -5], -3],
  [c.mod, [3216478362187432, 432143214], 120355456],
  [c.max, [], undefined], [c.max, [-0, 0], 0], [c.max, [0, -0], -0],
  [c.min, [0, -0], -0], [c.max, [1, NaN, 2], NaN], [c.min, [1, NaN, 2], NaN],
  [c.max, ["x", 2], "x"], [c.abs, [-0], 0], [c.abs, [-Infinity], Infinity],
  [c.bitXor, [], 0], [c.bitXor, [42, 1, 26], 49], [c.bitAnd, [41, 1, 27], 1],
  [c.bitOr, [41, 1, 26], 59], [c.bitAndNot, [41, 1, 27], 32],
  [c.bitNot, [0xffffffff], 0], [c.bitClear, [1000, 5], 968], [c.bitFlip, [1000, 3], 992],
  [c.bitSet, [0, 31], -2147483648], [c.bitTest, [0x80000000, 31], true],
  [c.bitShiftLeft, [1, 32], 1], [c.bitShiftLeft, [1, -1], -2147483648],
  [c.bitShiftRight, [0x80000000, 1], -1073741824], [c.unsignedBitShiftRight, [-1, 1], 2147483647],
  [c.bitShiftRightZeroFill, [-1, 0], 4294967295], [c.bitCount, [-1], 32],
  [c.bitCount, [0x100000000], 0], [c.bitCount, [0x80000000], 1],
  [c.char, [0x10041], "A"], [c.char, [NaN], "\0"], [c.char, [-1], "\uffff"],
]) assert.ok(Object.is(f(...args), expected), `${f.name}(${args}) expected ${expected}`);
for (const f of [c.less, c.lessEqual, c.greater, c.greaterEqual, c.numericEqual]) {
  assert.equal(f(), false);
  assert.equal(f(NaN), true);
  assert.equal(f(1, NaN), false);
}
assert.equal(c.less(1, 2, 3, 4), true);
assert.equal(c.less(1, 2, 2, 4), false);
assert.equal(c.lessEqual(1, 1, 2, 2), true);
assert.equal(c.greater(4, 3, 2, 1), true);
assert.equal(c.greaterEqual(4, 3, 3, 1), true);
assert.equal(c.numericEqual(0, -0, 0), true);
// Both compiler phases use Number keys, including nested collection keys.
assert.equal(c.get(c.hashMap(1, 'ok'), 1.0), 'ok');
assert.equal(c.count(c.conj(c.hashSet(1), 1.0)), 1);
for (const [left, right] of [
  [0, -0], [1, 1.0], [Infinity, 1 / 0], [-Infinity, 1 / -0],
  [c.vector(1, c.hashMap('n', 2)), c.vector(1.0, c.hashMap('n', 2.0))],
  [c.hashSet(0, Infinity), c.hashSet(-0, 1 / 0)],
]) {
  assert.equal(c.equal(left, right), true);
  assert.equal(c.hash(left), c.hash(right));
  assert.equal(c.get(c.hashMap(left, 'ok'), right), 'ok');
}
assert.equal(c.equal(NaN, NaN), false);
assert.equal(c.get(c.hashMap(NaN, 'nan'), NaN, 'missing'), 'missing');
assert.equal(c.count(c.hashSet(NaN, NaN)), 2);
assert.equal(c.get(c.hashMap(c.vector(NaN), 'nan'), c.vector(NaN), 'missing'), 'missing');
const numberBridge = runtime.compilerFormBridge();
for (const number of [-0, NaN, Infinity, -Infinity, 9007199254740992]) {
  const decoded = numberBridge.decode(['number', 0, null, number]);
  const encoded = numberBridge.encode(decoded, 100, 10);
  assert.equal(encoded[0], 'number');
  assert.ok(Object.is(encoded[3], number));
}
for (const x of [0, -0, 1, -1, 1e11]) assert.equal(c.isInteger(x), true);
for (const x of [1.5, 1e21, 1e308, Infinity, -Infinity, NaN, "0", null, undefined, false]) assert.equal(c.isInteger(x), false);
for (const x of [1.5, 1e21, Infinity, NaN, "2", null, undefined]) {
  throwsError(() => c.isEven(x));
  throwsError(() => c.isOdd(x));
}
assert.equal(c.isEven(-2), true);
assert.equal(c.isOdd(-1), true);
assert.equal(c.isEven(-0), true);
assert.equal(c.isZero("0"), false);
assert.equal(c.isPositive("2"), true);
assert.equal(c.isNegative("-2"), true);
assert.equal(c.isNaN("text"), true);
assert.equal(c.isNaN(undefined), true);
assert.equal(c.isNaN(null), false);
assert.equal(c.isInfinite("Infinity"), false);
for (const x of [null, undefined, false]) { assert.equal(c.boolean(x), false); assert.equal(c.not(x), true); }
for (const x of [0, -0, "", NaN, [], {}]) { assert.equal(c.boolean(x), true); assert.equal(c.not(x), false); }
for (const x of [true, false]) assert.equal(c.isBoolean(x), true);
assert.equal(c.isBoolean(new Boolean(false)), false);
assert.equal(c.isNumber(new Number(1)), false);
assert.equal(c.isString(new String("x")), false);
for (const x of ["a", "\n", "\ud800"]) assert.equal(c.isChar(x), true);
for (const x of ["", "ab", "😀", 65]) assert.equal(c.isChar(x), false);
for (const x of [null, "ab", {}, true]) throwsError(() => c.char(x));
for (const f of [c.byte, c.short, c.float, c.double, c.uncheckedByte, c.uncheckedChar, c.uncheckedShort, c.uncheckedFloat, c.uncheckedDouble]) {
  assert.equal(f(host), host);
  assert.equal(f(), undefined);
}
for (const [f, base] of [[c.uncheckedAdd,c.add], [c.uncheckedAddInt,c.add], [c.uncheckedSubtract,c.subtract], [c.uncheckedSubtractInt,c.subtract], [c.uncheckedMultiply,c.multiply], [c.uncheckedMultiplyInt,c.multiply], [c.uncheckedDivideInt,c.divide]]) {
  assert.notEqual(f, base);
  for (const args of [[], [-0], [2,3], [2,3,4,5]]) assert.ok(Object.is(f(...args), base(...args)));
}
assert.equal(c.uncheckedRemainderInt(-2, 5), 3);
assert.equal(c.uncheckedInt(4294967297), 4294967297);
assert.equal(c.uncheckedLong(4294967297), 4294967297);
for (const f of [c.uncheckedInc, c.uncheckedIncInt]) assert.equal(f(2147483647), 2147483648);
for (const f of [c.uncheckedDec, c.uncheckedDecInt]) assert.equal(f(-2147483648), -2147483649);
for (const f of [c.uncheckedNegate, c.uncheckedNegateInt]) assert.equal(f(0), -0);
const namedValues = [c.keyword("a"), c.keyword("ns/a"), c.symbol("a"), c.symbol("ns/a")];
assert.deepEqual(namedValues.map(c.isIdent), [true,true,true,true]);
assert.deepEqual(namedValues.map(c.isSimpleIdent), [true,false,true,false]);
assert.deepEqual(namedValues.map(c.isQualifiedIdent), [false,true,false,true]);
assert.deepEqual(namedValues.map(c.isSimpleKeyword), [true,false,false,false]);
assert.deepEqual(namedValues.map(c.isQualifiedKeyword), [false,true,false,false]);
assert.deepEqual(namedValues.map(c.isSimpleSymbol), [false,false,true,false]);
assert.deepEqual(namedValues.map(c.isQualifiedSymbol), [false,false,false,true]);
assert.equal(c.isQualifiedIdent(c.keyword("", "a")), true);
assert.equal(c.keywordIdentical(c.keyword("x"), c.keyword("x")), true);
assert.equal(c.symbolIdentical(c.symbol("x"), c.symbol("x")), true);
assert.equal(c.isIFn({call(){}}), false);
for (const x of [c.add, ...namedValues, v, c.hashMap(), c.hashSet()]) assert.equal(c.isIFn(x), true);
assert.equal(c.isFn(c.withMeta(c.add, c.hashMap())), true);
assert.equal(c.isFn(v), false);
assert.equal(c.isObject({}), true);
for (const x of [v, ...namedValues, c.hashMap(), c.hashSet(), c.list(), c.reduced(1), c.uuid("abc"), [], null, undefined]) assert.equal(c.isObject(x), false);
assert.equal(c.isMapEntry(c.first(c.hashMap("a", 1))), true);
assert.equal(c.isMapEntry(c.vector("a", 1)), false);
assert.equal(c.isSeqable(new Set([1])), true);
assert.equal(c.isSeqable({}), false);
assert.equal(c.isCounted(null), true);
assert.equal(c.isIndexed("abc"), false);
assert.equal(c.isIndexed([1]), false);
assert.equal(c.isCounted("abc"), false);
assert.equal(c.isCounted([1]), false);
assert.equal(c.isCounted(c.concat(v)), false);
assert.equal(c.isSeq(runtime.queueFromArray([1])), true);
assert.equal(c.isChunkedSeq(c.seq(runtime.vectorFromArray(Array.from({length:33}, (_,i)=>i)))), true);
assert.equal(c.isChunkedSeq(c.seq(v)), false);
assert.equal(c.isEmpty(null), true);
assert.equal(c.isEmpty(""), true);
assert.equal(c.isEmpty(c.hashSet()), true);
assert.equal(c.isEmpty(c.vector(1)), false);
throwsError(() => c.isEmpty(42));
console.log("numeric and predicate runtime assertions passed");
for (const f of [c.booleans, c.bytes, c.chars, c.shorts, c.ints, c.floats, c.doubles, c.longs]) assert.equal(f(host), host);
assert.equal(c.isCounted(c.seq(c.hashMap("a", 1))), false);
assert.equal(c.isCounted(c.seq(c.arrayMap("a", 1))), true);
assert.equal(c.isCounted(c.seq(c.dissoc(c.hashMap("a", 1, "b", 2), "b"))), false);
assert.equal(c.isCounted(c.seq(c.withMeta(c.hashMap("a", 1), c.hashMap()))), false);
assert.equal(c.isList(c.conj(c.seq(v), 0)), true);
assert.equal(c.isCounted(c.conj(c.seq(v), 0)), false);
assert.equal(c.add(c.vector(1), 2), "[1]2");
assert.equal(c.add(c.list(1), 2), "(1)2");
assert.equal(c.add(c.hashMap("a", 1), 2), '{"a" 1}2');
assert.equal(c.add(c.hashSet(1), 2), "#{1}2");
assert.equal(c.subtract(c.vector(1), 2), NaN);
assert.equal(c.isSeqable({[Symbol.iterator]: false}), true);
assert.equal(c.isSeqable({[Symbol.iterator]: null}), false);
assert.equal(c.isList(c.reSeq(/a/, "a")), true);
assert.equal(c.isList(c.concat(c.vector(1))), false);
// Unary concat wraps its input; seq retains the vector's IndexedSeq.
assert.equal(c.isList(c.seq(c.concat(c.vector(1)))), false);
const wideArrayMap = c.arrayMap(0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,8,8);
assert.equal(c.isCounted(c.seq(c.assoc(wideArrayMap, 0, 99))), true);
assert.equal(c.isCounted(c.seq(c.assoc(wideArrayMap, 9, 99))), false);
assert.equal(c.isCounted(c.seq(c.conj(wideArrayMap, c.list(c.vector(9, 99))))), false);

// Error values stay ordinary Error instances and retain exact user payloads.
const exceptionData = c.hashMap(runtime.keyword(null, 'a'), 1);
const exceptionCause = new TypeError('root');
const exception = c.exInfo('bad', exceptionData, exceptionCause);
assert.ok(exception instanceof Error);
assert.ok(exception instanceof c.ExceptionInfo);
assert.equal(exception.name, 'Error');
assert.equal(c.exData(exception), exceptionData);
assert.equal(c.exCause(exception), exceptionCause);
assert.equal(c.exMessage(exception), 'bad');
assert.equal(c.exMessage(exceptionCause), 'root');
assert.equal(c.exData(exceptionCause), null);
assert.equal(c.exCause(exceptionCause), null);
assert.equal(c.exMessage({message: 'fake'}), null);
assert.equal(c.prStr(c.exInfo('abc', c.hashMap(runtime.keyword(null, 'x'), 1), 'def')),
  '#error {:message "abc", :data {:x 1}, :cause "def"}');
assert.equal(c.prStr(c.exInfo('abc', null, exceptionCause)), '#error {:message "abc", :cause #object[TypeError TypeError: root]}');
assert.equal(c.exCause(c.exInfo('bad', null)), null);
assert.equal(c.exCause(c.exInfo('bad', null, undefined)), undefined);
assert.equal(c.exData(new c.ExceptionInfo('bad')), undefined);
assert.throws(() => c.exInfo('bad'), /Invalid arity: 1/);
console.log('exception value assertions passed');

// Metadata lives outside equality/hash payloads, and is never observed from
// ordinary host properties. These checks bypass the shared outcome encoder.
const metadata = c.hashMap(c.keyword('origin'), 'test');
for (const original of [runtime.vector(1, 2), c.list(1, 2), c.symbol('x'),
  c.hashMap(c.keyword('a'), 1), c.hashSet(1, 2), runtime.queueFromArray([1, 2]), c.seq(v)]) {
  const annotated = c.withMeta(original, metadata);
  const replaced = c.varyMeta(annotated, c.assoc, c.keyword('second'), true);
  assert.equal(c.meta(original), null);
  assert.equal(c.meta(annotated), metadata);
  assert.equal(c.equal(original, annotated, replaced), true);
  assert.equal(runtime.hash(original), runtime.hash(annotated));
  assert.equal(c.get(c.hashMap(original, 42), replaced), 42);
  assert.equal(c.get(c.hashSet(original), annotated), original);
  assert.equal(c.meta(c.withMeta(original, undefined)), undefined);
}
for (const hostValue of [{meta: metadata}, [], 's', 1, true, c.keyword('x'), new Date(), /a/]) {
  assert.equal(c.meta(hostValue), null);
  throwsError(() => c.withMeta(hostValue, metadata));
}
let realized = 0;
const lazyOriginal = c.mapSeq(x => { realized++; return x; }, c.list(1, 2));
const lazyAnnotated = c.withMeta(lazyOriginal, metadata);
assert.equal(realized, 0);
assert.equal(c.meta(lazyAnnotated), metadata);
assert.equal(c.meta(c.empty(lazyAnnotated)), metadata);
assert.equal(realized, 0);
assert.equal(c.first(lazyAnnotated), 1);
assert.equal(c.first(lazyOriginal), 1);
assert.equal(realized, 1);
const untouchedTail = c.withMeta(c.list(2), metadata);
assert.equal(c.rest(c.withMeta(c.cons(1, untouchedTail), null)), untouchedTail);
const mutableMeta = c.atom(1, c.keyword('meta'), metadata);
assert.equal(c.meta(mutableMeta), metadata);
assert.equal(c.satisfies(c.IMeta, mutableMeta), true);
assert.equal(c.satisfies("IMeta", mutableMeta), false);
assert.notEqual(c.IMeta, c.IWithMeta);
assert.equal(c.satisfies(c.IWithMeta, mutableMeta), false);
throwsError(() => c.withMeta(mutableMeta, null));
assert.equal(c.resetMeta(mutableMeta, null), null);
assert.equal(c.meta(mutableMeta), null);
assert.equal(c.deref(mutableMeta), 1);
const plainCallable = function (...args) { return [this, args]; };
const wrappedCallable = c.withMeta(plainCallable, metadata);
assert.equal(c.isFn(wrappedCallable), true);
assert.equal(c.satisfies(c.IMeta, plainCallable), true);
assert.equal(c.satisfies(c.IWithMeta, plainCallable), false);
for (const arity of [0, 1, 2, 19, 20, 21, 22, 40, 1000]) {
  const args = Array.from({length: arity}, (_, i) => i);
  const [receiver, observed] = c.apply(wrappedCallable, args);
  assert.equal(receiver, arity > 20 ? plainCallable : null);
  assert.deepEqual(observed, args);
  assert.deepEqual(wrappedCallable.apply(null, args)[1], args);
}
console.log('metadata copy, hashing, lazy sharing, protocols and invocation assertions passed');

// Synchronous reference state: literal identity/effect checks bypass the encoder.
{
  const a = c.atom(0), events = [], error = {};
  const validation = x => { if (x === 2) c.reset(a, 1); return x >= 0; };
  assert.equal(c.setValidator(a, validation), validation);
  assert.equal(c.getValidator(a), validation);
  assert.equal(c.addWatch(a, 'first', (k, ref, old, value) => {
    assert.equal(ref, a); events.push([k, old, value, c.deref(ref)]);
    c.removeWatch(a, 'second');
  }), a);
  c.addWatch(a, 'second', (k, ref, old, value) => events.push([k, old, value, c.deref(ref)]));
  assert.deepEqual(runtime.inspect(c.resetVals(a, 2)).items, [1, 2]);
  assert.deepEqual(events, [['first', 0, 1, 1], ['second', 0, 1, 1], ['first', 1, 2, 2]]);
  assert.throws(() => c.reset(a, -1), /Validator rejected/);
  assert.equal(c.deref(a), 2);
  assert.throws(() => c.setValidator(a, () => { throw error; }), e => e === error);
  assert.equal(c.getValidator(a), validation);
  c.addWatch(a, 'throw', () => { throw error; });
  assert.throws(() => c.resetVals(a, 5), e => e === error);
  assert.equal(c.deref(a), 5);
  assert.equal(c.removeWatch(a, 'throw'), a);
  const x = runtime.vector(1), y = runtime.vector(1), collection = c.atom(x);
  assert.notEqual(x, y);
  assert.equal(c.compareAndSet(collection, y, 9), true);
  assert.equal(c.deref(collection), 9);
  assert.notEqual(runtime.hash(c.atom(1)), runtime.hash(c.atom(1)));
  assert.equal(runtime.inspect(a).kind, 'opaque');
  assert.equal(c.isObject(a), false);
  assert.equal(c.meta(c.volatile(1)), null);
  assert.equal(c.satisfies(c.IReset, a), false);
  assert.throws(() => c.resetProtocol(a, 0), /IReset/);
  const volatile = c.volatile(undefined);
  assert.equal(c.deref(volatile), undefined);
  assert.equal(c.vreset(volatile, null), null);
  assert.equal(c.isVolatile(Object.create(volatile)), false);
  assert.throws(() => c.deref(Object.create(a)), /deref/);
}
{
  const target = c.atom(0), tail = c.list(3, 4), observed = [];
  const custom = runtime.makeReify([
    ['IDeref/-deref', 1, function (self) { assert.equal(this, self); observed.push('read'); return c.deref(target); }],
    ['ISwap/-swap!', 5, function (self, fn, x, y, rest) {
      assert.equal(this, self); assert.equal(rest, tail);
      return c.apply(c.swap, target, fn, x, y, rest);
    }],
    ['IReset/-reset!', 2, function (self, x) { assert.equal(this, self); return c.reset(target, x); }]
  ]);
  assert.deepEqual(runtime.inspect(c.apply(c.swapVals, custom, c.add, 1, 2, tail)).items, [0, 10]);
  assert.deepEqual(observed, ['read']);
  assert.deepEqual(runtime.inspect(c.resetVals(custom, 42)).items, [10, 42]);
  const huge = c.repeat(150000, 1);
  assert.equal(c.apply(c.swap, target, c.add, huge), 150042);
  // Fixed swap calls use the normal null receiver; variadic calls use apply.
  function receiver() { return this; }
  assert.equal(c.swap(target, receiver), null);
  assert.equal(c.swap(target, receiver, 1, 2, 3), receiver);
  assert.equal(c.get(c.update(null, "x", receiver), "x"), null);
  assert.equal(c.get(c.update(null, "x", receiver, 1, 2, 3, 4), "x"), receiver);
}

console.log("atom, volatile, validator, watch and state protocol assertions passed");

// Vars compare/hash by symbol while each construction has separate identity.
// The getter is invoked with the CLJS null receiver, never the private entry.
{
  let root = x => x + 1;
  const sym = runtime.symbol("definitions", "f");
  const getter = function () { assert.equal(this, null); return root; };
  const meta = runtime.mapFromArray([runtime.keyword(null, "doc"), "function"]);
  const a = new c.Var(getter, sym, meta);
  const b = new c.Var(getter, runtime.symbol("definitions", "f"), null);
  assert.notEqual(a, b);
  assert.equal(c.equal(a, b), true);
  assert.equal(c.hash(a), c.hash(b));
  assert.equal(c.deref(a), root);
  assert.equal(a.val, getter);
  assert.equal(a.sym, sym);
  assert.equal(a._meta, meta);
  assert.equal(a instanceof c.Var, true);
  assert.equal(a.call(null, 2), 3);
  const copied = c.withMeta(a, null);
  assert.equal(c.meta(copied), null);
  assert.equal(c.deref(copied), root);
  root = x => x * 3;
  assert.equal(copied.call(null, 2), 6);
  assert.equal(a.call(null, 2), 6);
  root.cljs$lang$macro = true;
  assert.equal(a.isMacro(), true);
  assert.equal(c.equal(c.test(a), runtime.keyword(null, "no-test")), true);
  let calls = 0;
  root.cljs$lang$test = () => { calls++; };
  const definition = runtime.definitionVar(getter, sym, meta);
  assert.equal(c.equal(c.test(root), runtime.keyword(null, "ok")), true);
  assert.equal(c.equal(c.test(definition), runtime.keyword(null, "ok")), true);
  assert.equal(calls, 2);
  assert.equal(c.meta(new c.Var(getter, sym, null)), null);
  assert.equal(c.meta(new c.Var(getter, sym, undefined)), undefined);
  assert.equal(c.meta(new c.Var(getter, sym)), undefined);
  assert.equal(c.get(runtime.mapFromArray([a, "same"]), b), "same");
  let demands = 0;
  let metadataDemands = 0;
  const callback = root.cljs$lang$test;
  const deferred = runtime.definitionVar(() => { demands++; return root; }, sym, meta, () => {
    metadataDemands++;
    return callback;
  });
  assert.equal(demands, 0);
  assert.equal(metadataDemands, 0);
  assert.equal(c.equal(deferred, a), true);
  assert.equal(c.hash(deferred), c.hash(a));
  assert.equal(metadataDemands, 0);
  const reflected = c.meta(deferred);
  assert.equal(c.get(reflected, runtime.keyword(null, "doc")), "function");
  assert.equal(c.get(reflected, runtime.keyword(null, "test")), callback);
  assert.equal(metadataDemands, 1);
  assert.equal(c.meta(deferred), reflected);
  assert.equal(deferred._meta, reflected);
  assert.equal(metadataDemands, 1);
  assert.equal(demands, 0);
  assert.equal(c.deref(deferred), root);
  assert.equal(demands, 1);
  assert.equal(c.equal(c.test(deferred), runtime.keyword(null, "ok")), true);
  assert.equal(demands, 1);
  assert.equal(metadataDemands, 1);
  assert.equal(calls, 3);
  assert.equal(c.deref(deferred), root);
  assert.equal(demands, 2);
  let overrides = 0;
  const edited = c.withMeta(deferred, runtime.mapFromArray([runtime.keyword(null, "test"), () => { overrides++; }]));
  assert.equal(c.equal(c.test(edited), runtime.keyword(null, "ok")), true);
  assert.equal(overrides, 1);
  assert.equal(demands, 2);
  assert.equal(metadataDemands, 1);
}
console.log("Var identity, metadata, live getter and callback receiver assertions passed");

// Host helpers use JavaScript keys and receivers; private collection contents
// remain immutable even when the public object receives arbitrary properties.
{
  const a = c.makeArray(null, 2, 2);
  assert.equal(c.aset(a, 0, 1, 9), 9);
  assert.deepEqual(a, [[, 9], new Array(2)]);
  assert.equal(c.aget(a, 0, 1), 9);
  const clone = c.aclone(a[0]);
  assert.equal(Object.hasOwn(clone, 0), true);
  assert.equal(Object.hasOwn(a[0], 0), false);
  const receiver = {x: 4, method(y) { return this.x + y; }};
  assert.equal(c.jsInvoke(receiver, 'method', 3), 7);
  const frozen = Object.freeze({x: 1});
  assert.equal(c.aset(frozen, 'x', 9), 9);
  assert.equal(c.jsDelete(frozen, 'x'), false);
  assert.equal(frozen.x, 1);
  const vector = c.vector(1);
  c.aset(vector, 0, 'host');
  assert.equal(c.aget(vector, 0), 'host');
  assert.equal(c.nth(vector, 0), 1);
  assert.throws(() => c.aget(null, 0), TypeError);
  assert.throws(() => c.aset(null, 0, 1), TypeError);
  assert.throws(() => c.jsDelete(null, 0), TypeError);
  assert.throws(() => c.checkedAgetStrict([1], '0'), Error);
  assert.equal(c.checkedAsetStrict([1], 0, 'x'), 'x');
}
console.log('JavaScript interop and public-object mutation assertions passed');

for (const call of [() => c.seqProtocol(null), () => c.lookupProtocol({}, kw), () => c.nthProtocol("x", 0), () => c.firstProtocol([])]) throwsError(call);
assert.equal(c.countProtocol(null), 0);
assert.equal(c.firstProtocol(c.list(1), 2), 1);

{
  const p = runtime.protocol('sample/P');
  const f = runtime.protocolMethod('sample/P/f', 'sample/f', [1, 2], true);
  const instance = runtime.makeReify([
    ['sample/P', 0, () => true],
    ['sample/P/f', 1, function (receiver) { assert.equal(this, receiver); return 1; }],
    ['sample/P/f', 2, function (receiver, x) { assert.equal(this, receiver); return x; }]
  ]);
  assert.equal(c.satisfies(p, instance), true);
  assert.equal(c.implements(p, instance), true);
  assert.equal(f(instance), 1);
  assert.equal(c.apply(f, c.list(instance, 7)), 7);
  const metadata = c.hashMap(c.symbol('sample/f'), function (receiver) { assert.equal(this, null); return receiver; });
  const copied = c.withMeta(instance, metadata);
  assert.equal(f(copied), copied);
  runtime.extendType('object', [['sample/P', 0, () => true], ['sample/P/f', 1, () => 9]]);
  assert.equal(f(instance), 1);
  assert.equal(f({}), 9);
  assert.equal(c.satisfies(p, {}), true);
  assert.equal(c.implements(p, {}), false);
  const redeclared = runtime.protocol('sample/P');
  runtime.protocolMethod('sample/P/f', 'sample/f', [1, 2], true);
  assert.equal(f(instance), 1);
  assert.throws(() => f({}), /No protocol method/);
  assert.equal(c.satisfies(p, {}), true);
  assert.equal(c.satisfies(redeclared, {}), false);
  assert.throws(() => f(), /Invalid arity: 0/);
  assert.throws(() => runtime.extendType(42, []), TypeError);
  runtime.extendType('object', [['ILookup', 0, () => true], ['ILookup/-lookup', 2, (_, k) => k]]);
  assert.equal(c.get(c.hashMap(1, 2), 1), 2);
  assert.equal(c.get({}, 3), 3);
}
console.log('protocol identities, receiver, metadata and fallback assertions passed');


{
  // A fresh runtime isolates mutable native protocol tables from earlier checks.
  const r = (0, eval)(fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8'));
  const core = r.core;
  const marker = r.makeReify([['ILookup', 0, null], ['IReduce', 0, null], ['ISeqable', 0, null]]);
  assert.equal(core.satisfies(core.ILookup, marker), true);
  assert.equal(core.isSeqable(marker), true);
  for (const op of [() => core.get(marker, 'x'), () => core.reduce(core.add, 0, marker), () => core.seq(marker)]) assert.throws(op, /No protocol method/);
  const slotsOnly = {'cljs$core$ILookup$_lookup$arity$2': () => 7};
  assert.equal(core.satisfies(core.ILookup, slotsOnly), false);
  assert.equal(core.get(slotsOnly, 'x'), null);
  assert.equal(core.lookupProtocol(slotsOnly, 'x'), 7);
  const seen = [];
  const lookup = r.makeReify([['ILookup', 0, null],
    ['ILookup/-lookup', 2, (_self, key) => { seen.push(['two', key]); return 1; }],
    ['ILookup/-lookup', 3, (_self, key, nf) => { seen.push(['three', key]); return key === 'present' ? undefined : nf; }]]);
  assert.equal(core.get(lookup, 'x'), 1);
  assert.equal(core.contains(lookup, 'present'), true);
  assert.equal(core.contains(lookup, 'absent'), false);
  assert.deepEqual(seen, [['two', 'x'], ['three', 'present'], ['three', 'absent']]);
  assert.equal(core.get([10, undefined], 1), undefined);
  assert.equal(core.contains([10, undefined], 1), true);
  assert.equal(core.contains([10], -1), false);
  const tail = r.makeReify([['ICounted', 0, null], ['ICounted/-count', 1, () => 7]]);
  const head = r.makeReify([['ISeqable', 0, null], ['ISeqable/-seq', 1, function () { return this; }],
    ['ISeq', 0, null], ['ISeq/-first', 1, () => 1], ['ISeq/-rest', 1, () => tail],
    ['INext', 0, null], ['INext/-next', 1, () => tail]]);
  assert.equal(core.count(head), 8);
  assert.equal(core.count(core.cons(0, head)), 9);
  const effects = [];
  const iterable = r.makeReify([['IIterable', 0, null], ['IIterable/-iterator', 1, () => {
    let n = 0;
    return {hasNext: () => n < 5, next: () => { effects.push(++n); return n; }};
  }]]);
  assert.equal(core.reduce((a, b) => b === 3 ? core.reduced(a + b) : a + b, 0, iterable), 6);
  assert.deepEqual(effects, [1, 2, 3]);
  const entry = r.makeReify([['IMapEntry', 0, null], ['IMapEntry/-key', 1, () => 'x'], ['IMapEntry/-val', 1, () => 7]]);
  const entries = r.makeReify([['ISeqable', 0, null], ['ISeqable/-seq', 1, () => core.list(entry)]]);
  assert.equal(core.reduceKv((acc, key, value) => acc + key + value, '', entries), 'x7');
  assert.deepEqual(core.toArray(core.keys(entries)), ['x']);
  assert.deepEqual(core.toArray(core.vals(entries)), [7]);
  const identity = value => r.makeReify([['IHash', 0, null], ['IHash/-hash', 1, () => 17],
    ['IEquiv', 0, null], ['IEquiv/-equiv', 2, (_self, other) => other.value === value]]);
  const a = identity(1), b = identity(1); a.value = b.value = 1;
  const before = core.hashMap(a, 'old'), after = core.assoc(before, b, 'new');
  assert.equal(core.count(after), 1);
  assert.equal(core.get(before, b), 'old');
  assert.equal(core.get(after, a), 'new');
  assert.equal(core.get(core.hashSet(a, b), b), a);
  assert.deepEqual(core.toArray(new Set([1, 2])), [1, 2]);
  const printer = r.makeReify([['IPrintWithWriter', 0, null], ['IPrintWithWriter/-pr-writer', 3,
    (_self, writer, opts) => { assert.equal(opts, null); core.writeAll(writer, 'a', 'b'); core.flushWriter(writer); }]]);
  assert.equal(core.prStr(printer), 'ab');
  // Since light-cljs-258 the emitter writes the constructor's field assignments.
  const Record = r.makeType(['value'], [], true, null, 'sample/R', null,
    function (value, meta, extmap, hash) { this.value = value; this.__meta = meta; this.__extmap = extmap; this.__hash = hash; });
  const record = new Record('text');
  assert.equal(core.prStr(record), '#sample.R{:value "text"}');
  core.printReadably = false;
  assert.equal(core.prStr(record), '#sample.R{:value text}');
  core.printReadably = true;
  // A type's brand is private (light-cljs-258): a host object made from the
  // prototype or given an instance's fields is no record, and a record keeps
  // its brand when its prototype changes.
  assert.equal(r.inspect(record).kind, 'map');
  assert.equal(r.inspect(Object.create(Record.prototype)), null);
  assert.equal(r.inspect(Object.defineProperties({}, Object.getOwnPropertyDescriptors(record))), null);
  assert.equal(r.inspect(Object.defineProperties(Object.create(Record.prototype), Object.getOwnPropertyDescriptors(record))), null);
  const moved = new Record('moved');
  Object.setPrototypeOf(moved, Object.prototype);
  assert.equal(r.inspect(moved).count, 1);
  const native = {};
  assert.equal(core.hash(native), core.hash(native));
  assert.equal(core.equal(native, {}), false);
}
console.log('public core protocol consumers and reduction boundaries passed');

// Collection helper checks use literal expectations and private payload inspection,
// independent of the differential encoder and compiler macro expansion.
{
  const r = (0, eval)(fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8'));
  const core = r.core, key = core.keyword('k'), keyPath = core.vector(key, 1);
  for (const makeMap of [core.arrayMap, core.hashMap])
    assert.throws(() => makeMap('missing'), error => error.message === 'No value supplied for key: missing');
  const original = core.withMeta(core.hashMap(key, core.vector(10, 20)), core.hashMap('source', true));
  const changed = core.updateIn(original, keyPath, (x, y) => x + y, 3);
  assert.deepEqual(r.inspect(core.get(original, key)).items, [10, 20]);
  assert.deepEqual(r.inspect(core.get(changed, key)).items, [10, 23]);
  assert.equal(core.meta(changed), core.meta(original));
  assert.equal(core.getIn(original, core.vector(key, 9), 'absent'), 'absent');
  assert.deepEqual(r.inspect(core.selectKeys(core.vector(1), core.vector(2))).entries, []);
  assert.deepEqual(r.inspect(core.selectKeys(core.hashMap('nil', null, 'false', false), core.vector('nil', 'false', 'absent'))).entries, [['nil', null], ['false', false]]);
  const pairs = core.zipmap(core.vector(core.vector(1), core.list(1)), core.vector(null, false));
  assert.equal(core.count(pairs), 1);
  assert.equal(core.get(pairs, core.vector(1)), false);
  assert.equal(core.hash(pairs), core.hash(core.hashMap(core.list(1), false)));
  const entry = new core.MapEntry('key', false, null);
  assert.equal(core.isMapEntry(entry), true);
  assert.equal(entry.call(null, 2, 'absent'), 'absent');
  assert.equal(core.get(entry, 0.5, 'absent'), 'absent');
  assert.equal(core.isMapEntry(core.withMeta(entry, null)), false);
  assert.deepEqual(r.inspect(core.pop(entry)).items, ['key']);
  for (const empty of [core.vector(), core.hashMap(), core.hashSet()]) {
    assert.notEqual(core.into(empty, null), empty);
    assert.equal(core.into(empty), empty);
    assert.equal(core.count(empty), 0);
  }
  const events = [];
  const input = r.makeReify([['ISeqable', 0, null], ['ISeqable/-seq', 1, function () { return this; }],
    ['ISeq', 0, null], ['ISeq/-first', 1, () => entry], ['ISeq/-rest', 1, () => null],
    ['INext', 0, null], ['INext/-next', 1, () => { events.push('next'); return null; }]]);
  const projected = core.keys(input);
  assert.equal(core.first(projected), 'key');
  assert.deepEqual(events, []);
  core.rest(projected);
  core.rest(projected);
  assert.deepEqual(events, ['next', 'next']);
  const set = core.withMeta(core.hashSet(1), core.hashMap('m', true));
  assert.equal(core.meta(core.set(set)), null);
  assert.equal(core.get(core.meta(set), 'm'), true);
}
console.log('persistent helper snapshots, metadata, map entries and traversal assertions passed');

// Transient ownership is private, independently of compiler observations.
{
  const original = c.withMeta(c.vector(1), c.hashMap('source', true));
  const t = c.transient(original);
  assert.equal(c.satisfies(c.IEditableCollection, original), true);
  assert.equal(c.satisfies(c.ITransientVector, t), true);
  for (const protocol of [c.ISeqable, c.ISequential, c.ICollection, c.IHash, c.IEquiv, c.IMeta, c.IWithMeta])
    assert.equal(c.satisfies(protocol, t), false);
  assert.equal(c.conjBang(t, 2), t);
  assert.equal(c.assocBang(t, 1, 20), t);
  const persistent = c.persistentBang(t);
  assert.equal(c.prStr(original), '[1]');
  assert.equal(c.prStr(persistent), '[1 20]');
  assert.equal(c.meta(persistent), null);
  assert.equal(c.hash(persistent), c.hash(c.vector(1,20)));
  for (const operation of [() => c.persistentBang(t), () => c.conjBang(t,3), () => c.assocBang(t,0,3), () => c.popBang(t), () => c.count(t), () => c.get(t,99), () => c.nth(t,0)])
    assert.throws(operation, /persistent!/);
  assert.equal(c.nth(t,99,'missing'),'missing');
  const retained = c.withMeta(c.vector(1),c.hashMap('key','retained')), query=c.vector(1);
  const tm=c.transient(c.hashMap(retained,10,0,20));
  c.assocBang(tm,query,11,-0,21);
  const pm=c.persistentBang(tm);
  assert.equal(c.count(pm),2);
  assert.equal(c.nth(c.find(pm,query),0),retained);
  assert.equal(c.get(tm,query),11); // Pinned hash-map lookup remains readable.
  assert.throws(() => c.count(tm), /persistent!/);
  const small=c.transient(c.arrayMap('a',1));
  c.persistentBang(small);
  assert.throws(() => c.get(small,'a'), /persistent!/);
  assert.equal(c.satisfies(c.IDrop,c.hashSet(retained)),false);
  const ts=c.transient(c.hashSet(retained));
  assert.equal(c.get(ts,query),query); // A transient set returns the query key.
  const ps=c.persistentBang(ts);
  assert.equal(c.get(ps,query),retained);
  assert.throws(() => c.get(ts,query), /persistent!/); // Small set's array map.
  const large=c.transient(c.set(c.range(10)));
  c.disjBang(large,2,4);c.persistentBang(large);
  assert.equal(c.get(large,3),3);
  assert.throws(() => c.disjBang(large,3), /persistent!/);
  let promoted=c.transient(c.arrayMap());
  const old=promoted;
  for(let i=0;i<9;i++) promoted=c.assocBang(promoted,i,i);
  assert.notEqual(promoted,old);
  assert.equal(c.count(old),8);
  assert.equal(c.count(c.persistentBang(promoted)),9);
  c.assocBang(old,0,42);
  assert.equal(c.get(c.persistentBang(old),0),42);
  const fail=c.transient(c.vector(1,2));
  assert.throws(() => c.assocBang(fail,0,5,5,9), /bounds/);
  assert.equal(c.prStr(c.persistentBang(fail)),'[5 2]');
  for(const value of [null,c.list(1),c.subvec(c.vector(1),0),c.sortedMap(1,2),{}])
    assert.throws(() => c.transient(value), /IEditableCollection/);
  // Forged prototypes and fields never confer editable storage.
  assert.throws(() => c.conjBang(Object.create(Object.getPrototypeOf(t)),3), /ITransientCollection/);
}
console.log('transient ownership, persistence, protocol and lookup assertions passed');

// Multimethod declaration rollback and source-frontier replay include table and
// cache state; JavaScript host effects (the validator call) remain observable.
{
  const k = x => runtime.keyword(null, x), empty = () => c.hashMap();
  const h = c.atom(c.derive(c.makeHierarchy(), k('child'), k('parent')));
  const methods = c.atom(empty()), preferences = c.atom(empty()), cache = c.atom(empty()), cached = c.atom(null);
  const mf = new c.MultiFn(runtime.symbol('test', 'multifn'), x => x, k('default'), h, methods, preferences, cache, cached);
  c.addMethod(mf, k('parent'), () => 'old');
  assert.equal(mf.call(null, k('child')), 'old');
  const oldTable = c.deref(methods), oldCache = c.deref(cache);
  assert.throws(() => runtime.declarationTransaction(() => {
    c.addMethod(mf, k('parent'), () => 'discarded');
    assert.equal(mf.call(null, k('child')), 'discarded');
    throw new Error('bundle failure');
  }), /bundle failure/);
  assert.equal(c.deref(methods), oldTable);
  assert.equal(c.deref(cache), oldCache);
  assert.equal(mf.call(null, k('child')), 'old');
  const install = runtime.declarationTransaction(() => c.addMethod(mf, k('parent'), () => 'new'), true);
  assert.equal(mf.call(null, k('child')), 'new');
  runtime.applyDeclarationPatches(install.patches, false);
  assert.equal(mf.call(null, k('child')), 'old');
  runtime.applyDeclarationPatches(install.patches, true);
  assert.equal(mf.call(null, k('child')), 'new');
  c.reset(h, c.makeHierarchy());
  assert.equal(c.getMethod(mf, k('child')), null);
}

// Public field accessors preserve brands even when host objects copy prototypes.
for (const [Type, fields] of [
  [c.Symbol, ['name', '_hash', '_meta']], [c.Cons, ['first', '__hash', 'meta']],
  [c.Keyword, ['_hash']], [c.List, ['__hash']], [c.Repeat, ['next', '__hash']],
  [c.Cycle, ['current', '_next']], [c.Iterate, ['seed', 'next']],
  ...['PersistentVector', 'Subvec', 'ChunkedSeq', 'PersistentQueue', 'PersistentArrayMap', 'PersistentHashMap', 'PersistentHashSet'].map(name => [c[name], ['__hash']])
]) {
  for (const field of fields) {
    const forged = Object.create(Type.prototype);
    assert.throws(() => { forged[field] = 9; }, /Expected public core value/);
    assert.equal(runtime.inspect(forged), null);
  }
}
console.log('public constructor fields preserve canonical payload brands');

// Lazy seqs, cells, views and chunks keep their payloads in private fields
// (light-cljs-259): a pipeline writes no WeakMap entry for the values its steps
// build, and a host object that inherits one's prototype, or copies its own
// properties onto that prototype, is no language value.
{
  const set = WeakMap.prototype.set;
  let writes = 0;
  WeakMap.prototype.set = function (...args) { writes++; return set.apply(this, args); };
  try {
    const steps = c.mapcat(c.identity, c.drop(1, c.partition(2, c.filter(c.isEven, c.mapSeq(c.inc, c.range(200))))));
    assert.equal(c.reduce(c.add, 0, steps), 10094);
  } finally { WeakMap.prototype.set = set; }
  assert.equal(writes, 0);
  const lazy = c.mapSeq(x => x + 1, c.range(40)), chunked = c.seq(lazy), chunk = c.chunkFirst(chunked);
  assert.equal(c.isChunkedSeq(chunked), true);
  assert.equal(c.count(chunk), 32);
  assert.equal(c.meta(lazy), null);
  assert.equal(lazy.meta, null);
  for (const value of [lazy, chunked, chunk, c.seq([1, 2, 3]), c.cons(0, lazy), c.list(1, 2)]) {
    const forged = Object.create(Object.getPrototypeOf(value));
    for (const candidate of [forged, Object.defineProperties(Object.create(Object.getPrototypeOf(value)), Object.getOwnPropertyDescriptors(value))]) {
      assert.equal(c.isSeq(candidate), false);
      assert.equal(c.isChunkedSeq(candidate), false);
      assert.equal(runtime.inspect(candidate), null);
      assert.throws(() => c.count(candidate), /count not supported/);
    }
  }
  // A chunked cons's cell defines its fields, as a literal does: a setter that
  // the cell would inherit from Object.prototype never sees it.
  const names = ['kind', 'chunk', 'more', 'chunkedState'];
  let intercepted = 0;
  for (const name of names) Object.defineProperty(Object.prototype, name, {configurable: true, set() { intercepted++; throw new Error('inherited setter'); }});
  try {
    const doubled = c.chunkCons(c.chunkFirst(c.seq(c.mapSeq(x => x * 2, c.range(3)))), null);
    assert.equal(c.first(doubled), 0);
    assert.equal(c.prStr(c.rest(doubled)), '(2 4)');
    assert.equal(c.hash(doubled), c.hash(c.list(0, 2, 4)));
  } finally { for (const name of names) delete Object.prototype[name]; }
  assert.equal(intercepted, 0);
}
console.log('seq payloads are private fields that host objects cannot copy');

// Public find and the IFind fallback consume one retained map search.
{
  const events = [];
  const retained = runtime.makeReify([['IHash', 0, null], ['IHash/-hash', 1, () => 7]]);
  const value = c.atom(42);
  const query = runtime.makeReify([
    ['IHash', 0, null], ['IHash/-hash', 1, () => { events.push('hash'); return 7; }],
    ['IEquiv', 0, null], ['IEquiv/-equiv', 2, (_self, other) => { events.push('equiv'); return other === retained; }]
  ]);
  const old = c.withMeta(c.hashMap(retained, value, null, null), c.hashMap('tag', 'kept'));
  for (const lookup of [c.find, c.findProtocol]) {
    events.length = 0;
    const entry = lookup(old, query);
    assert.equal(c.keyProtocol(entry), retained);
    assert.equal(c.valProtocol(entry), value);
    assert.deepEqual(events, ['hash', 'equiv']);
    assert.equal(lookup(old, null) && c.valProtocol(lookup(old, null)), null);
    assert.equal(lookup(c.hashMap(), query), null);
    assert.deepEqual(events, ['hash', 'equiv']);
  }
  assert.equal(c.get(c.meta(old), 'tag'), 'kept');
  let receiver;
  receiver = runtime.makeReify([
    ['IFind', 0, null], ['IFind/-find', 2, function (self, key) {
      assert.equal(this, receiver); assert.equal(self, receiver);
      return new c.MapEntry('retained', key, null);
    }],
    ['IAssociative', 0, null], ['IAssociative/-contains-key?', 2, () => { throw new Error('unexpected contains'); }]
  ]);
  assert.equal(c.valProtocol(c.find(receiver, value)), value);
  assert.equal(c.valProtocol(c.findProtocol(receiver, value)), value);
}
console.log('public find preserves one search and direct receiver dispatch');
