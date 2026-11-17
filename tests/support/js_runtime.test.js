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
  // Atoms and volatiles are classes (light-cljs-1013): `Object.create` of
  // one is deliberate reflection (docs/value-layout.md, section 5), so only
  // host objects of their shape are checked.
  assert.equal(c.isVolatile({state: 1}), false);
  assert.throws(() => c.deref({state: 1, meta: null, validator: null, watches: null}), /deref/);
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
  const slotsOnly = {'cljs$core$ILookup$_lookup$arity$2$LKEY': () => 7};
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
  // A record is told by its prototype's keyed kind (light-cljs-1037, where a
  // private brand of each instance told it since light-cljs-258): a host
  // object given an instance's fields is no record, and a record moved to
  // another prototype is a host object. (An object made from the prototype
  // is one: deliberate reflection, docs/value-layout.md, section 5.)
  assert.equal(r.inspect(record).kind, 'map');
  assert.equal(r.inspect(Object.defineProperties({}, Object.getOwnPropertyDescriptors(record))), null);
  const moved = new Record('moved');
  Object.setPrototypeOf(moved, Object.prototype);
  assert.equal(r.inspect(moved), null);
  const native = {};
  assert.equal(core.hash(native), core.hash(native));
  assert.equal(core.equal(native, {}), false);
}
console.log('public core protocol consumers and reduction boundaries passed');

// light-cljs-346: where a program reads brands, `Advanced` writes each type as
// a class with a private brand of its own, whose check `defineClass` gives the
// prototype. Neither is exported, so this reads them from a copy of the
// runtime that exports them too.
{
  const source = fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8');
  const anchor = 'return Object.freeze({setDeclarationDemand, ';
  assert.equal(source.split(anchor).length, 2);
  const r = (0, eval)(source.replace(anchor, 'return Object.freeze({defineClass, TypeBrand, ' + anchor.slice(22)));
  const core = r.core;
  const Record = r.defineClass(class { #b; constructor(a0, a1, a2, a3) { this.value = a0; this.__meta = a1; this.__extmap = a2; this.__hash = a3; } static c = (a0) => #b in a0; },
    'sample/C value', [], 1);
  const Plain = r.defineClass(class { #b; constructor(a0) { this.x = a0; } static c = (a0) => #b in a0; }, 'sample/P x', []);
  assert.equal(Record.c, undefined);
  const record = new Record('text', null, null, null), plain = new Plain(1);
  assert.equal(r.TypeBrand.type(record), Record);
  assert.equal(r.TypeBrand.type(plain), null);
  assert.equal(r.inspect(record).kind, 'map');
  assert.equal(core.prStr(core.assoc(record, core.keyword('b'), 2)), '#sample.C{:value "text", :b 2}');
  // Neither the prototype, nor an instance's fields, nor an instance as a
  // prototype forge it; a host's subclass constructs through the class and
  // keeps it. A makeType instance has none since light-cljs-1037: its
  // prototype's keyed kind tells it.
  for (const forged of [Object.create(Record.prototype), Object.create(record), Object.create(Plain.prototype),
    Object.defineProperties({}, Object.getOwnPropertyDescriptors(record)),
    Object.defineProperties(Object.create(Record.prototype), Object.getOwnPropertyDescriptors(record)),
    Record.prototype, Plain.prototype, {}, [], () => 1, Object.create(null)]) {
    assert.equal(r.TypeBrand.type(forged), undefined);
  }
  assert.equal(r.TypeBrand.type(new (class extends Record {})('sub', null, null, null)), Record);
  const Made = r.makeType(['value'], [], true, null, 'sample/M', null, function (value) { this.value = value; });
  assert.equal(r.TypeBrand.type(new Made(1)), undefined);
  assert.equal(r.inspect(new Made(1)).kind, 'map');
  for (const value of [null, undefined, 1, 'text', true]) assert.equal(r.TypeBrand.type(value), undefined);
  // An instance takes its brand once: a class constructor cannot be called
  // without `new`, on an instance or otherwise.
  assert.throws(() => Record.call(record, 'again', null, null, null), TypeError);
  assert.throws(() => Plain.apply(plain, [2]), TypeError);
  assert.equal(record.value, 'text');
  // An instance moved to another prototype loses its brand.
  const moved = new Plain(2);
  Object.setPrototypeOf(moved, Object.prototype);
  assert.equal(r.TypeBrand.type(moved), undefined);
  // A proxy has no brand, and one whose trap answers with itself without end
  // reads at most 64 prototypes.
  let traps = 0;
  const cyclic = new Proxy({}, { getPrototypeOf() { traps++; return cyclic; } });
  assert.equal(r.TypeBrand.type(cyclic), undefined);
  assert.equal(traps, 65);
  assert.equal(r.TypeBrand.type(new Proxy(record, {})), undefined);
}
console.log('class type brands passed');

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
  // A seq over an array is a class value (light-cljs-1010), and so are a
  // lazy seq, a chunked cons and a chunk (light-cljs-1029): their own keyed
  // fields copied onto their prototype make one by deliberate reflection
  // (docs/value-layout.md, section 5), so only their bare prototype is
  // checked; so are a cons and a list (light-cljs-1030).
  const classValue = c.seq([1, 2, 3]), consValue = c.cons(0, lazy), listValue = c.list(1, 2);
  const classValues = [lazy, chunked, chunk, classValue, consValue, listValue];
  for (const value of classValues) {
    const forged = Object.create(Object.getPrototypeOf(value));
    const copied = Object.defineProperties(Object.create(Object.getPrototypeOf(value)), Object.getOwnPropertyDescriptors(value));
    for (const candidate of classValues.includes(value) ? [forged] : [forged, copied]) {
      assert.equal(c.isSeq(candidate), false);
      assert.equal(c.isChunkedSeq(candidate), false);
      assert.equal(runtime.inspect(candidate), null);
      assert.throws(() => c.count(candidate), /count not supported/);
    }
  }
  // A chunked cons's and a range's cell define their fields, as a literal
  // does: a setter that the cell would inherit from Object.prototype never
  // sees one (light-cljs-237 made a range's cell a class instance).
  const names = ['kind', 'chunk', 'more', 'chunkedState', 'hash', 'head', 'end', 'step', 'integer', 'count'];
  let intercepted = 0;
  for (const name of names) Object.defineProperty(Object.prototype, name, {configurable: true, set() { intercepted++; throw new Error('inherited setter'); }});
  try {
    const doubled = c.chunkCons(c.chunkFirst(c.seq(c.mapSeq(x => x * 2, c.range(3)))), null);
    assert.equal(c.first(doubled), 0);
    assert.equal(c.prStr(c.rest(doubled)), '(2 4)');
    assert.equal(c.hash(doubled), c.hash(c.list(0, 2, 4)));
    assert.equal(c.prStr(c.range(3)), '(0 1 2)');
    assert.equal(c.count(c.range(0, 2, 0.5)), 4);
  } finally { for (const name of names) delete Object.prototype[name]; }
  assert.equal(intercepted, 0);
}
console.log('seq payloads are private fields that host objects cannot copy');

// Reduced markers, transients, queues, iterator seqs and the maps and sets
// outside the hash tries keep their payloads in private fields too
// (light-cljs-291), and `repeat` stores no nil metadata or hash: building
// one writes no WeakMap entry, and a host object that inherits one's
// prototype, or copies its own properties onto that prototype, is none of
// them.
{
  const set = WeakMap.prototype.set;
  let writes = 0;
  WeakMap.prototype.set = function (...args) { writes++; return set.apply(this, args); };
  try {
    // A runtime evaluated now, as the hash tries' stores keep the `set` they
    // find (`trieBrand`).
    const fresh = (0, eval)(fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8')), f = fresh.core;
    const key = f.keyword('a'), map = f.arrayMap(key, 1), members = f.hashSet(1);
    writes = 0;
    const built = {
      reduced: f.reduced(1), transientVector: f.transient(f.vector(1)), transientMap: f.transient(map),
      transientSet: f.transient(members), queue: f.conj(fresh.queueFromArray([1]), 2), iteratorSeq: f.seq(new Set([1, 2])),
      arrayMap: f.arrayMap(key, 2), assoc: f.assoc(map, 2, 3), set: f.hashSet(1, 2), conj: f.conj(members, 2),
      persisted: f.persistentBang(f.assocBang(f.transient(map), 5, 1)), repeat: f.repeat(2, 1), repeated: f.rest(f.repeat(3, 1))
    };
    assert.equal(writes, 0);
    WeakMap.prototype.set = set;
    assert.equal(f.isReduced(built.reduced), true);
    assert.equal(f.deref(built.reduced), 1);
    assert.equal(f.prStr(f.persistentBang(built.transientVector)), '[1]');
    assert.equal(f.prStr(f.persistentBang(built.transientMap)), '{:a 1}');
    assert.equal(f.prStr(f.persistentBang(built.transientSet)), '#{1}');
    assert.equal(f.prStr(built.queue), '#queue [1 2]');
    assert.equal(f.count(built.iteratorSeq), 2);
    assert.equal(f.prStr(f.vector(built.arrayMap, built.assoc, built.persisted, built.set, built.conj, built.repeat, built.repeated)),
      '[{:a 2} {:a 1, 2 3} {:a 1, 5 1} #{1 2} #{1 2} (1 1) (1 1)]');
    assert.equal(f.meta(built.repeat), null);
    assert.equal(built.repeat.meta, null);
    assert.equal(built.repeat.__hash, null);
    assert.equal(f.hash(built.repeat), f.hash(f.list(1, 1)));
    // Only a null slot goes unstored: an undefined one reads back as given.
    const undefinedSlots = f.Repeat(undefined, 2, 1, null, undefined);
    assert.equal(undefinedSlots.__hash, undefined);
    assert.equal(f.meta(undefinedSlots), undefined);
    assert.equal(f.Repeat(null, 2, 1, null, null).__hash, null);
    for (const [name, value] of Object.entries(built)) {
      // A transient vector (light-cljs-1009) and a seq over an array
      // (light-cljs-1010) are class values whose data are their own keyed
      // fields: a copy of those onto their prototype is deliberate
      // reflection, and one (docs/value-layout.md, section 5); so are a
      // repeat and its rest (light-cljs-1030), the maps and the map
      // transient (light-cljs-1012), and the hash sets and the set transient
      // (light-cljs-1013).
      const copies = ['transientVector', 'view', 'repeat', 'repeated', 'transientMap', 'arrayMap', 'assoc', 'persisted', 'transientSet', 'set', 'conj', 'reduced', 'queue', 'iteratorSeq'].includes(name) ? [] : [Object.defineProperties(Object.create(Object.getPrototypeOf(value)), Object.getOwnPropertyDescriptors(value))];
      // A transient vector's prototype declares the protocols it implements
      // (light-cljs-1028): what the protocol tests say of `Object.create` of
      // it is deliberate reflection (docs/value-layout.md, section 5); it
      // holds no state, so it is still no value to `inspect` or `persistent!`.
      const declares = name === 'transientVector';
      for (const candidate of [Object.create(Object.getPrototypeOf(value)), ...copies]) {
        if (!declares) for (const predicate of [f.isReduced, f.isMap, f.isSet, f.isSeq, f.isCounted, f.isColl]) assert.equal(predicate(candidate), false);
        assert.equal(fresh.inspect(candidate), null);
        if (!declares) assert.throws(() => f.count(candidate), /count not supported/);
        assert.throws(() => f.persistentBang(candidate));
      }
    }
  } finally { WeakMap.prototype.set = set; }
}
console.log('reduced markers, transients, queues and array maps and sets are private fields too');

// A host that states `lightCljsWeakMapStores`, as the evaluator's QuickJS does,
// gets those stores as WeakMaps (light-cljs-292): each value then writes its
// entries, the values behave as before, and a host object that inherits a
// value's prototype or copies its own properties is still none of them.
{
  const set = WeakMap.prototype.set;
  let writes = 0;
  // Counted from before the runtime loads, as the hash tries' stores keep
  // the `set` they find (`trieBrand`).
  WeakMap.prototype.set = function (...args) { writes++; return set.apply(this, args); };
  globalThis.lightCljsWeakMapStores = true;
  let fresh, built;
  try {
    try { fresh = (0, eval)(fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8')); }
    finally { delete globalThis.lightCljsWeakMapStores; }
    const f = fresh.core, key = f.keyword('a'), map = f.arrayMap(key, 1), members = f.hashSet(1);
    const steps = f.mapcat(f.identity, f.drop(1, f.partition(2, f.filter(f.isEven, f.mapSeq(f.inc, f.range(200))))));
    assert.equal(f.reduce(f.add, 0, steps), 10094);
    const lazy = f.mapSeq(x => x + 1, f.range(3)), chunked = f.seq(f.mapSeq(x => x + 1, f.range(40)));
    built = {
      lazy, chunked, chunk: f.chunkFirst(chunked), view: f.seq([1, 2, 3]), cons: f.cons(0, lazy), list: f.list(1, 2),
      reduced: f.reduced(1), transientVector: f.transient(f.vector(1)), transientMap: f.transient(map),
      transientSet: f.transient(members), queue: f.conj(fresh.queueFromArray([1]), 2), iteratorSeq: f.seq(new Set([1, 2])),
      arrayMap: f.arrayMap(key, 2), assoc: f.assoc(map, 2, 3), set: f.hashSet(1, 2), conj: f.conj(members, 2),
      persisted: f.persistentBang(f.assocBang(f.transient(map), 5, 1)), repeat: f.repeat(2, 1)
    };
    assert.ok(writes > 0);
    // A reduced value, a queue, a hash set and a set transient are classes
    // with their own fields (light-cljs-1013), and so is a sorted set
    // (light-cljs-1038): they write no entry, where each wrote one.
    writes = 0;
    f.reduced(2); fresh.queueFromArray([1]); f.hashSet(1, 2); f.transient(members); f.sortedSet(2);
    assert.equal(writes, 0);
    // An array map is a class with its own fields (light-cljs-1012): it
    // writes no entry, where its store and its kind's wrote two.
    writes = 0;
    f.arrayMap(key, 3);
    assert.equal(writes, 0);
  } finally { WeakMap.prototype.set = set; }
  const f = fresh.core, key = f.keyword('a');
  assert.equal(f.isChunkedSeq(built.chunked), true);
  assert.equal(f.count(built.chunk), 32);
  assert.equal(f.prStr(f.vector(built.cons, built.list, built.view, built.lazy)), '[(0 1 2 3) (1 2) (1 2 3) (1 2 3)]');
  assert.equal(f.deref(built.reduced), 1);
  assert.equal(f.isReduced(built.reduced), true);
  assert.equal(f.prStr(f.persistentBang(built.transientVector)), '[1]');
  assert.equal(f.prStr(f.persistentBang(built.transientMap)), '{:a 1}');
  assert.equal(f.prStr(f.persistentBang(built.transientSet)), '#{1}');
  assert.equal(f.prStr(built.queue), '#queue [1 2]');
  assert.equal(f.count(built.iteratorSeq), 2);
  assert.equal(f.prStr(f.vector(built.arrayMap, built.assoc, built.persisted, built.set, built.conj, built.repeat)),
    '[{:a 2} {:a 1, 2 3} {:a 1, 5 1} #{1 2} #{1 2} (1 1)]');
  assert.equal(f.equal(built.assoc, f.hashMap(key, 1, 2, 3)), true);
  assert.equal(f.isMap(built.arrayMap), true);
  assert.equal(f.isSet(built.conj), true);
  for (const [name, value] of Object.entries(built)) {
    // As above: a copy of a transient vector's, a seq's, a lazy seq's, a
    // chunked cons's or a chunk's own fields onto its prototype is one
    // (light-cljs-1009, light-cljs-1010, light-cljs-1029), and so is a
    // cons's, a list's or a repeat's (light-cljs-1030), a map's or a map
    // transient's (light-cljs-1012), and a hash set's or a set transient's
    // (light-cljs-1013).
    const copies = ['transientVector', 'view', 'lazy', 'chunked', 'chunk', 'cons', 'list', 'repeat', 'transientMap', 'arrayMap', 'assoc', 'persisted', 'transientSet', 'set', 'conj', 'reduced', 'queue', 'iteratorSeq'].includes(name) ? [] : [Object.defineProperties(Object.create(Object.getPrototypeOf(value)), Object.getOwnPropertyDescriptors(value))];
    // As above, a transient vector's prototype declares its protocols
    // (light-cljs-1028).
    const declares = name === 'transientVector';
    for (const candidate of [Object.create(Object.getPrototypeOf(value)), ...copies]) {
      if (!declares) for (const predicate of [f.isReduced, f.isMap, f.isSet, f.isSeq, f.isChunkedSeq, f.isCounted, f.isColl]) assert.equal(predicate(candidate), false);
      assert.equal(fresh.inspect(candidate), null);
      if (!declares) assert.throws(() => f.count(candidate), /count not supported/);
    }
  }
}
console.log('a host that states WeakMap stores keeps its payloads in WeakMaps, as unforgeable');

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

// No runtime prototype holds a kind table's number (light-cljs-237, whose
// tables light-cljs-1015 removed): the slots and the operations answer every
// kind. A host object that inherits a lazy seq's, a cell's or a view's
// prototype is still not a seq, and one that inherits a collection's
// prototype answers every operation as it did when the tables were there.
{
  const values = [c.list(1, 2), c.cons(0, c.list(1)), c.mapSeq(c.inc, c.vector(1, 2)), c.range(3), c.seq(c.mapSeq(c.inc, c.range(40))),
    c.seq(c.vector(1, 2)), c.seq(c.vec(c.range(40)))];
  const own = prototype => Object.getOwnPropertySymbols(prototype).filter(symbol => symbol !== Symbol.iterator);
  for (const value of values) assert.deepEqual(own(Object.getPrototypeOf(value)), []);
  for (const forged of values.map(value => Object.create(Object.getPrototypeOf(value)))) {
    for (const op of [c.seq, c.first, c.rest, c.next]) assert.throws(() => op(forged));
    assert.throws(() => c.count(forged), /count not supported on this type/);
  }
}
{
  const make = () => [c.vector(1, 2), c.subvec(c.vector(1, 2, 3), 1), c.vec(c.range(40)), c.hashMap('a', 1),
    c.arrayMap('a', 1, 'b', 2), c.sortedMap('b', 2, 'a', 1), c.hashSet(1, 2), c.sortedSet(2, 1),
    c.into(c.PersistentQueue.EMPTY, c.vector(1, 2)), c.list(1, 2), c.mapSeq(c.inc, c.vector(1, 2)), c.first(c.hashMap('k', 'v'))];
  const own = prototype => Object.getOwnPropertySymbols(prototype).filter(symbol => symbol !== Symbol.iterator);
  for (const value of make()) assert.deepEqual(own(Object.getPrototypeOf(value)), []);
}
// The collections' and the lazy seqs' branches look a method that
// JavaScript gives a runtime value up once, as the operations do (the Sol
// review of light-cljs-358): a getter that counts its reads is read once and
// its method called once per operation.
{
  const sentinel = c.PROTOCOL_SENTINEL;
  const counted = (value, slot, method, marker) => {
    let reads = 0;
    if (marker) value[marker] = sentinel;
    Object.defineProperty(value, slot, {get() { const n = ++reads; return (...args) => method(n, ...args); }, configurable: true});
    return () => reads;
  };
  const cases = [
    [c.vector(1), 'cljs$core$ICollection$_conj$arity$2$LKEY', null, v => c.conj(v, 2)],
    [c.hashSet(1), 'cljs$core$ICollection$_conj$arity$2$LKEY', null, s => c.conj(s, 2)],
    [c.hashMap('a', 1), 'cljs$core$ICollection$_conj$arity$2$LKEY', null, m => c.conj(m, c.vector('b', 2))],
    [c.into(c.PersistentQueue.EMPTY, c.vector(1)), 'cljs$core$ICollection$_conj$arity$2$LKEY', null, q => c.conj(q, 2)],
    [c.vector(1), 'cljs$core$IAssociative$_assoc$arity$3$LKEY', null, v => c.assoc(v, 0, 2)],
    [c.hashMap('a', 1), 'cljs$core$IAssociative$_assoc$arity$3$LKEY', null, m => c.assoc(m, 'a', 2)],
    [c.vector(1), 'cljs$core$ILookup$_lookup$arity$2$LKEY', 'cljs$core$ILookup$$LKEY', v => c.get(v, 0)],
    [c.hashMap('a', 1), 'cljs$core$ILookup$_lookup$arity$3$LKEY', 'cljs$core$ILookup$$LKEY', m => c.get(m, 'a', 'nf')],
    [c.hashSet(1), 'cljs$core$ILookup$_lookup$arity$2$LKEY', 'cljs$core$ILookup$$LKEY', s => c.get(s, 1)],
    [c.set(c.range(20)), 'cljs$core$ILookup$_lookup$arity$3$LKEY', 'cljs$core$ILookup$$LKEY', s => c.get(s, 1, 'nf')],
    [c.first(c.hashMap('a', 1)), 'cljs$core$ILookup$_lookup$arity$2$LKEY', 'cljs$core$ILookup$$LKEY', e => c.get(e, 0)],
    [c.first(c.hashMap('a', 1)), 'cljs$core$IIndexed$_nth$arity$2$LKEY', 'cljs$core$IIndexed$$LKEY', e => c.nth(e, 0)],
    [c.cons(0, c.list(1)), 'cljs$core$ISeq$_rest$arity$1$LKEY', 'cljs$core$ISeq$$LKEY', s => c.next(s)],
    [c.mapSeq(c.inc, c.vector(1, 2)), 'cljs$core$ISeqable$_seq$arity$1$LKEY', 'cljs$core$ISeqable$$LKEY', s => c.first(s)],
    // The collections' `first`, `rest` and `next` (light-cljs-375).
    ...[() => c.vector(1, 2), () => c.vec(c.range(40)), () => c.subvec(c.vector(1, 2, 3), 1), () => c.first(c.hashMap('a', 1)),
      () => c.hashMap('a', 1), () => c.hashSet(1), () => c.sortedSet(2, 1), () => c.into(c.PersistentQueue.EMPTY, c.vector(1))]
      .flatMap(make => [c.first, c.rest, c.next].map(op => [make(), 'cljs$core$ISeqable$_seq$arity$1$LKEY', 'cljs$core$ISeqable$$LKEY', op])),
  ];
  for (const [value, slot, marker, op] of cases) {
    const calls = [];
    const reads = counted(value, slot, (n, ...args) => { calls.push(n); return c.list(n); }, marker);
    op(value);
    assert.equal(reads(), 1, `${slot} read once`);
    assert.deepEqual(calls, [1], `${slot} called once`);
  }
}
// A lookup method that answers with the item `contains?` gave it for a
// missing key, the runtime's own `missing`, is called once by `get`, which
// gives that answer (Sol review of light-cljs-373).
for (const make of [() => c.hashSet(1), () => c.set(c.range(20)), () => c.vector(1), () => c.hashMap('a', 1)]) {
  const value = make();
  let saved, calls = 0;
  value.cljs$core$ILookup$$LKEY = c.PROTOCOL_SENTINEL;
  value.cljs$core$ILookup$_lookup$arity$3$LKEY = (_, k, nf) => (saved = nf);
  c.contains(value, 9);
  value.cljs$core$ILookup$_lookup$arity$2$LKEY = () => ++calls === 1 ? saved : 42;
  assert.equal(c.get(value, 1), saved);
  assert.equal(calls, 1);
  // ... and so is a `-seq` whose seq starts with it, by `first`
  // (light-cljs-375).
  let seqs = 0;
  value.cljs$core$ISeqable$$LKEY = c.PROTOCOL_SENTINEL;
  value.cljs$core$ISeqable$_seq$arity$1$LKEY = () => (seqs++, c.list(saved));
  assert.equal(c.first(value), saved);
  assert.equal(seqs, 1);
}
// ... and by `rest` and `next` of the collections (Sol review of
// light-cljs-375): a `-seq` whose seq's rest is `missing`, or whose rest's
// seq is, is called once, and the operation gives that answer.
{
  const donor = c.hashSet(1);
  let saved;
  donor.cljs$core$ILookup$$LKEY = c.PROTOCOL_SENTINEL;
  donor.cljs$core$ILookup$_lookup$arity$3$LKEY = (_, k, nf) => (saved = nf);
  c.contains(donor, 9);
  for (const make of [() => c.vector(1, 2), () => c.vec(c.range(40)), () => c.first(c.hashMap('a', 1)), () => c.hashMap('a', 1),
    () => c.hashSet(1), () => c.into(c.PersistentQueue.EMPTY, c.vector(1))]) {
    const value = make();
    let seqs = 0;
    value.cljs$core$ISeqable$$LKEY = c.PROTOCOL_SENTINEL;
    value.cljs$core$ISeqable$_seq$arity$1$LKEY = () => (seqs++, new c.Cons(null, 1, saved, null));
    assert.equal(c.rest(value), saved);
    assert.equal(seqs, 1);
    let tails = 0;
    const tail = runtime.makeReify([['ISeqable', 0, null], ['ISeqable/-seq', 1, () => (tails++, saved)]]);
    value.cljs$core$ISeqable$_seq$arity$1$LKEY = () => (seqs++, new c.Cons(null, 1, tail, null));
    assert.equal(c.next(value), saved);
    assert.deepEqual([seqs, tails], [2, 1]);
  }
}
// No answer an operation's branch gives is taken for the marker that sends
// the call to the operation's own path (light-cljs-387): `missing` saved as
// above, as a method's answer or as an item a collection stores, comes back
// from one call of each method.
{
  const donor = c.hashSet(1);
  let saved;
  donor.cljs$core$ILookup$$LKEY = c.PROTOCOL_SENTINEL;
  donor.cljs$core$ILookup$_lookup$arity$3$LKEY = (_, k, nf) => (saved = nf);
  c.contains(donor, 9);
  for (const [value, slot, op] of [
    [c.vector(1), 'cljs$core$ICollection$_conj$arity$2$LKEY', v => c.conj(v, 2)],
    [c.hashSet(1), 'cljs$core$ICollection$_conj$arity$2$LKEY', s => c.conj(s, 2)],
    [c.hashMap('a', 1), 'cljs$core$ICollection$_conj$arity$2$LKEY', m => c.conj(m, c.vector('b', 2))],
    [c.into(c.PersistentQueue.EMPTY, c.vector(1)), 'cljs$core$ICollection$_conj$arity$2$LKEY', q => c.conj(q, 2)],
    [c.vector(1), 'cljs$core$IAssociative$_assoc$arity$3$LKEY', v => c.assoc(v, 0, 2)],
    [c.hashMap('a', 1), 'cljs$core$IAssociative$_assoc$arity$3$LKEY', m => c.assoc(m, 'a', 2)],
  ]) {
    let calls = 0;
    value[slot] = () => ++calls === 1 ? saved : 42;
    assert.equal(op(value), saved, slot);
    assert.equal(calls, 1, slot);
  }
  // A lazy seq's branches, of a realized value whose `-first` or `-seq`
  // gives it, and a cell's `next`, whose rest's `-seq` does.
  let firsts = 0, seqs = 0;
  const head = runtime.makeReify([['ISeqable', 0, null], ['ISeqable/-seq', 1, self => self],
    ['ISeq', 0, null], ['ISeq/-first', 1, () => (firsts++, saved)], ['ISeq/-rest', 1, () => c.List.EMPTY]]);
  assert.equal(c.first(new c.LazySeq(null, () => head, null, null)), saved);
  assert.equal(firsts, 1);
  const empty = runtime.makeReify([['ISeqable', 0, null], ['ISeqable/-seq', 1, () => (seqs++, saved)]]);
  assert.equal(c.seq(new c.LazySeq(null, () => empty, null, null)), saved);
  assert.equal(seqs, 1);
  assert.equal(c.next(new c.Cons(null, 1, empty, null)), saved);
  assert.equal(seqs, 2);
  // Stored items, under a key whose `-equiv` counts its calls: as many as
  // for any other item.
  let equivs = 0;
  const key = () => runtime.makeReify([['IEquiv', 0, null], ['IEquiv/-equiv', 2, () => (equivs++, true)],
    ['IHash', 0, null], ['IHash/-hash', 1, () => 7]]);
  for (const make of [item => c.arrayMap(key(), item), item => c.hashMap(key(), item)]) {
    equivs = 0;
    assert.equal(c.get(make(5), key()), 5);
    const expected = equivs;
    equivs = 0;
    assert.equal(c.get(make(saved), key()), saved);
    assert.equal(equivs, expected);
  }
  const entry = c.first(c.hashMap(saved, saved));
  for (const [value, read] of [[c.vector(saved), x => c.nth(x, 0)], [c.vector(saved), x => c.get(x, 0)], [c.vector(saved), c.first],
    [entry, x => c.nth(x, 0)], [entry, x => c.get(x, 1)], [entry, c.first], [c.list(saved), c.first], [c.seq([saved]), c.first],
    [c.hashSet(saved), x => c.get(x, saved)], [c.hashMap('a', saved), x => c.get(x, 'a')], [c.cons(1, c.list(saved)), x => c.first(c.next(x))]])
    assert.equal(read(value), saved);
  // ... and a stored item a getter gives is read once (Sol review of this
  // change): the getter gives it first, then 42.
  const once = () => {
    const items = [], reads = {n: 0};
    Object.defineProperty(items, 0, {get: () => ++reads.n === 1 ? saved : 42, enumerable: true});
    return [items, reads];
  };
  for (const read of [items => c.first(c.seq(items)),
    items => c.nth(new c.PersistentVector(null, 1, 5, c.PersistentVector.EMPTY_NODE, items, null), 0),
    items => c.get(new c.PersistentVector(null, 1, 5, c.PersistentVector.EMPTY_NODE, items, null), 0)]) {
    const [items, reads] = once();
    assert.equal(read(items), saved);
    assert.equal(reads.n, 1);
  }
}
{
  // A subvector of a vector the public constructor built over a caller's
  // array reads the array as its seq does once the array shrinks.
  const items = [7, 8, 9];
  const shared = c.subvec(new c.PersistentVector(null, 3, 5, c.PersistentVector.EMPTY_NODE, items, null), 0, 3);
  items.length = 0;
  assert.equal(c.first(shared), null);
  assert.equal(c.rest(shared), c.List.EMPTY);
  assert.equal(c.next(shared), null);
}
// The dispatch cells that resolved keys keep (light-cljs-375) follow the
// declaration journal: a key looked up before any `extend-type` of it, with
// a core dispatcher (`ICounted/-count`) or without one (`IDeref/-deref`),
// finds what a transaction installs, replaces, rolls back, undoes, redoes and
// restores, beside an unrelated extension that stays.
{
  const r = (0, eval)(fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8'));
  const core = r.core;
  const outcome = thunk => { try { return thunk(); } catch (error) { return 'error'; } };
  const answers = () => [outcome(() => core.deref(5)), outcome(() => core.count(5)), outcome(() => core.deref('s')), outcome(() => core.count(true))];
  assert.deepEqual(answers(), ['error', 'error', 'error', 'error']);
  r.extendType('boolean', [['ICounted/-count', 1, () => 'b']]);
  const before = answers();
  assert.deepEqual(before, ['error', 'error', 'error', 'b']);
  const install = r.declarationTransaction(() => r.extendType('number', [['IDeref/-deref', 1, n => n + 1], ['ICounted/-count', 1, () => 'n']]), true);
  const installed = answers();
  assert.deepEqual(installed, [6, 'n', 'error', 'b']);
  const replace = r.declarationTransaction(() => r.extendType('number', [['IDeref/-deref', 1, n => n + 2], ['ICounted/-count', 1, () => 'm']]), true);
  assert.deepEqual(answers(), [7, 'm', 'error', 'b']);
  assert.throws(() => r.declarationTransaction(() => {
    r.extendType('string', [['IDeref/-deref', 1, s => s + '!'], ['ICounted/-count', 1, () => 's']]);
    assert.deepEqual(answers(), [7, 'm', 's!', 'b']);
    throw new Error('bundle failure');
  }), /bundle failure/);
  assert.deepEqual(answers(), [7, 'm', 'error', 'b']);
  r.applyDeclarationPatches(replace.patches, false);
  assert.deepEqual(answers(), installed);
  const saved = r.snapshotDeclarationPatches(install.patches);
  r.applyDeclarationPatches(install.patches, false);
  assert.deepEqual(answers(), before);
  r.applyDeclarationPatches(install.patches, true);
  assert.deepEqual(answers(), installed);
  r.applyDeclarationPatches(install.patches, false);
  r.restoreDeclarationPatches(saved);
  assert.deepEqual(answers(), installed);
  r.applyDeclarationPatches(replace.patches, true);
  assert.deepEqual(answers(), [7, 'm', 'error', 'b']);
}
console.log('kind tables stay private to the runtime');

// runtime_value_enumeration_contract (docs/value-layout.md, question 3, which
// the user answered yes): a runtime class value's data are own fields whose
// names end in the build's key (`$LKEY` in this raw runtime), so
// `Object.keys`, `Object.entries`, `for...in` and spread list them, as
// upstream ClojureScript's values list theirs; `JSON.stringify` is
// unchanged through the class bases' `toJSON`, and properties a program
// gives a value still serialize.
{
  const r = runtime, core = r.core;
  const values = {
    vector: r.vector(1, 2), entry: core.first(core.hashMap(1, 2)), subvec: core.subvec(r.vector(1, 2, 3), 1),
    transientVector: core.transient(r.vector(1)),
    // The seqs over an array or a string, and rest arguments (light-cljs-1010).
    indexedSeq: core.seq([1, 2]), stringSeq: core.next(core.seq('ab')),
    chunkedSeq: core.next(core.seq(core.vec(Array.from({length: 40}, (_, i) => i)))),
    restArguments: r.fnRest([1, 2, 3], 1),
    // Keywords and symbols (light-cljs-1011).
    keyword: core.keyword('n', 'k'), symbol: core.symbol('n', 's'),
    // Lazy seqs, chunked conses, chunks and chunk buffers (light-cljs-1029).
    lazySeq: core.mapSeq(core.inc, r.vector(1, 2)),
    chunkedCons: core.seq(core.mapSeq(core.inc, core.vec(Array.from({length: 40}, (_, i) => i)))),
    chunk: core.arrayChunk([1, 2], 0, 2), chunkBuffer: core.chunkBuffer(2),
    // Lists, conses, ranges, repeats, iterates, cycles and key seqs
    // (light-cljs-1030); their computed `first` and `rest` are getters of
    // the prototype, which neither enumeration nor JSON runs.
    list: core.list(1, 2), emptyList: core.list(), cons: core.cons(0, core.list(1)),
    range: core.range(3), floatRange: core.range(0.5, 3), repeat: core.repeat(2, 1),
    iterate: core.iterate(x => { throw new Error('realized ' + x); }, 0), cycle: core.cycle(r.vector(1, 2)),
    keySeq: core.keys(core.arrayMap(1, 2)),
    // Array, hash and sorted maps and the map transients (light-cljs-1012).
    arrayMap: core.arrayMap(1, 2), hashMap: core.hashMap(1, 2), sortedMap: core.sortedMap(1, 2),
    transientArrayMap: core.transient(core.arrayMap(1, 2)), transientHashMap: core.transient(core.hashMap(1, 2)),
    // Hash and array sets, set transients, queues, reduced values and
    // iterator seqs (light-cljs-1013).
    arraySet: core.hashSet(1, 2), hashSet: core.set(Array.from({length: 20}, (_, i) => i)), transientSet: core.transient(core.hashSet(1)),
    queue: core.conj(r.queueFromArray([1]), 2), reduced: core.reduced(1), iteratorSeq: core.seq(new Set([1, 2])),
    // An iterator seq is a plain object to enumeration and printing, as
    // before, and so is an eduction: their fields are not enumerable.
    delay: new core.Delay(() => 1, null), uuid: core.uuid('00000000-0000-0000-0000-000000000000'), eduction: new core.Eduction(core.identity, r.vector(1)),
    // A sorted map's nodes, map entries, and its seq over them
    // (light-cljs-1035).
    treeNode: core.first(core.sortedMap(1, 2)), sortedMapSeq: core.seq(core.sortedMap(1, 2)),
    // A sorted set holds its tree map (light-cljs-1038).
    sortedSet: core.sortedSet(1, 2)
  };
  const view = ['arr$LKEY', 'i$LKEY', 'indexed$LKEY', 'view$LKEY'];
  const trie = ['meta$LKEY', 'cnt$LKEY', 'shift$LKEY', 'root$LKEY', 'tail$LKEY', 'hash$LKEY'];
  // Vectors, subvectors, map entries and transient vectors hold their store's
  // fields, their metadata and their hash themselves (light-cljs-1026).
  const fields = {vector: trie, entry: trie, subvec: ['meta$LKEY', 'cnt$LKEY', 'sub$LKEY', 'begin$LKEY', 'built$LKEY', 'hash$LKEY', 'v$LKEY'],
    transientVector: ['cnt$LKEY', 'shift$LKEY', 'root$LKEY', 'tail$LKEY', 'owned$LKEY', 'editable$LKEY'], indexedSeq: view, stringSeq: view, restArguments: view,
    chunkedSeq: [...view, 'vec$LKEY', 'base$LKEY'], keyword: ['ns$LKEY', 'name$LKEY', 'fqn$LKEY', 'hash$LKEY'],
    symbol: ['ns$LKEY', 'name$LKEY', 'str$LKEY', 'hash$LKEY', 'meta$LKEY'], lazySeq: ['fn$LKEY', 's$LKEY', 'hash$LKEY'],
    chunkedCons: ['chunk$LKEY', 'more$LKEY', 'hash$LKEY'], chunk: ['arr$LKEY', 'off$LKEY', 'end$LKEY', 'first$LKEY', 'step$LKEY'],
    chunkBuffer: ['buf$LKEY', 'end$LKEY'],
    list: ['cell$LKEY', 'first$LKEY', 'rest$LKEY', 'count$LKEY'], emptyList: ['cell$LKEY', 'count$LKEY', 'hash$LKEY'],
    cons: ['cell$LKEY', 'first$LKEY', 'rest$LKEY'],
    range: ['cell$LKEY', 'first$LKEY', 'end$LKEY', 'step$LKEY', 'integer$LKEY', 'count$LKEY'],
    floatRange: ['cell$LKEY', 'first$LKEY', 'end$LKEY', 'step$LKEY', 'integer$LKEY'],
    repeat: ['cell$LKEY', 'remaining$LKEY', 'first$LKEY', 'next$LKEY'],
    iterate: ['cell$LKEY', 'f$LKEY', 'prev$LKEY', 'seed$LKEY', 'realized$LKEY', 'next$LKEY'],
    cycle: ['cell$LKEY', 'all$LKEY', 'prev$LKEY', 'current$LKEY', 'next$LKEY'], keySeq: ['cell$LKEY', 'mseq$LKEY'],
    arrayMap: ['cnt$LKEY', 'arr$LKEY'], hashMap: ['cnt$LKEY', 'root$LKEY', 'nil$LKEY', 'lit$LKEY'], sortedMap: ['comp$LKEY', 'tree$LKEY', 'cnt$LKEY'],
    transientArrayMap: ['editable$LKEY', 'arr$LKEY'], transientHashMap: ['edit$LKEY', 'cnt$LKEY', 'root$LKEY', 'nil$LKEY'],
    arraySet: ['entries$LKEY', 'skind$LKEY'], hashSet: ['entries$LKEY', 'skind$LKEY'], transientSet: ['state$LKEY'], queue: ['arr$LKEY'],
    reduced: ['val$LKEY'], iteratorSeq: [], delay: ['state$LKEY'], uuid: ['uuid$LKEY', 'hash$LKEY'], eduction: [],
    treeNode: [...trie, 'left$LKEY', 'right$LKEY', 'red$LKEY'], sortedMapSeq: ['cell$LKEY', 'stack$LKEY', 'ascending$LKEY', 'cnt$LKEY'],
    sortedSet: ['treeMap$LKEY']};
  for (const [name, value] of Object.entries(values)) {
    const field = fields[name];
    assert.deepEqual(Object.keys(value), field, name);
    assert.deepEqual(Object.entries(value).map(([key]) => key), field, name);
    assert.deepEqual(Object.keys({...value}), field, name);
    const listed = [];
    for (const key in value) if (Object.hasOwn(value, key)) listed.push(key);
    assert.deepEqual(listed, field, name);
    assert.equal(JSON.stringify(value), '{}', name);
    assert.equal(JSON.stringify([value]), '[{}]', name);
    // The fields carry the key: none is an upstream name a host could read
    // by accident.
    for (const key of Object.keys(value)) assert.ok(key.endsWith('$LKEY'), key);
  }
  // A var's and a function with metadata's fields are not enumerable
  // (light-cljs-1038 and its Sol review): they stringify and enumerate as
  // before, when they held only their functions, a wrapped function's
  // `toJSON` included.
  const aVar = new core.Var(() => 1, core.symbol('x'), null), wrapped = () => 1;
  wrapped.toJSON = () => 7;
  const metaFn = core.withMeta(wrapped, core.hashMap(1, 2));
  for (const value of [aVar, metaFn, core.withMeta(aVar, core.hashMap(1, 2))]) {
    assert.equal(JSON.stringify(value), '{}');
    for (const key of Object.keys(value)) assert.ok(!key.endsWith('$LKEY'), key);
  }
  assert.equal(core.isVar(core.withMeta(aVar, core.hashMap(1, 2))), true);
  assert.equal(metaFn.call(null), 1);
  // A public constructor's undefined arguments stay undefined, and the value
  // stays what it is (the same review).
  const noMap = new core.PersistentTreeSet(null, undefined, null), noSym = new core.Var(() => 1, undefined, null);
  assert.equal(noMap.tree_map, undefined);
  assert.equal(core.isSet(noMap), true);
  assert.equal(core.count(noMap), 0);
  assert.equal(core.prStr(core.clone(noMap)), '#{}');
  assert.equal(noSym.sym, undefined);
  assert.equal(core.isVar(noSym), true);
  assert.equal(core.isVar(core.withMeta(noSym, core.hashMap(1, 2))), true);
  // Two loads of one build recognize each other's vars by the shared key;
  // a copy `with-meta` makes in either is callable (the second Sol review).
  const other = (0, eval)(fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8'));
  for (const [from, to] of [[r, other], [other, r]]) {
    const v = from.makeVar(() => x => x + 1, from.core.symbol('f')), copy = to.core.withMeta(v, to.core.hashMap(1, 2));
    assert.equal(to.core.isVar(copy), true);
    assert.equal(copy.call(null, 1), 2);
    assert.equal(to.core.apply(copy, [2]), 3);
    assert.equal(to.core.deref(copy)(3), 4);
    assert.equal(to.core.prStr(to.core.meta(copy)), '{1 2}');
  }
  const marked = r.vector(1);
  marked.note = 'kept';
  assert.equal(JSON.stringify(marked), '{"note":"kept"}');
  assert.equal(core.count(marked), 1);
  // An own `__proto__` property serializes as an object rest copied it, with
  // no setter of `Object.prototype` run (the Sol review of light-cljs-1026).
  // So for seqs (the Sol review of light-cljs-1030), and for maps and map
  // transients (the Sol review of light-cljs-1012).
  for (const value of [r.vector(1), core.first(core.hashMap(1, 2)), core.subvec(r.vector(1, 2), 1), core.transient(r.vector(1)),
    core.list(1), core.cons(0, core.list(1)), core.seq([1]), core.mapSeq(core.inc, r.vector(1)),
    core.arrayMap(1, 2), core.hashMap(1, 2), core.sortedMap(1, 2), core.transient(core.arrayMap(1, 2)), core.transient(core.hashMap(1, 2))]) {
    Object.defineProperty(value, '__proto__', {value: 7, enumerable: true});
    assert.equal(JSON.stringify(value), '{"__proto__":7}');
  }
  // A public constructor's vector keeps an undefined count as given, and is
  // still a vector with its metadata and hash (the Sol review of
  // light-cljs-1026).
  const undefinedCount = new core.PersistentVector(core.hashMap('m', 1), undefined, 5, null, [], 17);
  assert.equal(core.isVector(undefinedCount), true);
  assert.equal(core.equal(core.meta(undefinedCount), core.hashMap('m', 1)), true);
  assert.equal(core.hash(undefinedCount), 17);
  for (const markedMap of [core.arrayMap(1, 2), core.hashMap(1, 2), core.sortedMap(1, 2)]) {
    markedMap.note = 'kept';
    assert.equal(JSON.stringify(markedMap), '{"note":"kept"}');
    assert.equal(JSON.stringify({m: markedMap}), '{"m":{"note":"kept"}}');
    assert.equal(core.get(markedMap, 1), 2);
  }
  const markedSeq = core.seq([1]);
  markedSeq.note = 'kept';
  assert.equal(JSON.stringify(markedSeq), '{"note":"kept"}');
  assert.equal(core.first(markedSeq), 1);
  // Rest arguments are neither frozen nor copied twice: an array a call
  // built for them from 0 is the seq's own.
  const own = [1, 2], rest = r.fnRest(own, 0);
  assert.equal(Object.isFrozen(own), false);
  assert.equal(core.count(rest), 2);
  assert.equal(core.first(core.next(rest)), 2);
  assert.equal(r.fnRest(own, 2), null);
}
console.log('runtime_value_enumeration_contract: keyed fields enumerate, JSON is unchanged');

// vector_family_prototype_contract (light-cljs-1028; docs/value-layout.md,
// section 3): the prototypes of vectors, subvectors, map entries and
// transient vectors declare exactly the core protocols their values
// implement, the ones with a mask bit by the keyed masks (`protocolMaskNames`'
// bits, the last of a duplicated name) and the others by a keyed marker,
// and hold the keyed slots of the slot-first operations, each answering as
// its operation does. `implements?` and `satisfies?` give every value of
// these kinds, its prototype's or another constructor's, the same answers.
{
  const source = fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8');
  const maskNames = /const protocolMaskNames = "([^"]+)"/.exec(source)[1].split(' ');
  const masks = new Map(maskNames.map((name, index) => [name, [index >> 5, 1 << (index % 32)]]));
  const protocols = Object.keys(c).filter(name => /^I[A-Z]|^ASeq$|^Fn$|^Inst$/.test(name) && typeof c[name] === 'function');
  const vectorLike = 'IFn ICounted IEmptyableCollection ICollection IIndexed ILookup IAssociative IStack IVector IMeta IWithMeta IReduce IEquiv IHash ISeqable ISequential IReversible IPrintWithWriter IComparable IIterable IDrop IFind';
  const nth = ['cljs$core$IIndexed$_nth$arity$2$LKEY', 'cljs$core$IIndexed$_nth$arity$3$LKEY'];
  const lookup = ['cljs$core$ILookup$_lookup$arity$2$LKEY', 'cljs$core$ILookup$_lookup$arity$3$LKEY'];
  const count = 'cljs$core$ICounted$_count$arity$1$LKEY', peek = 'cljs$core$IStack$_peek$arity$1$LKEY';
  const conj = 'cljs$core$ICollection$_conj$arity$2$LKEY', assoc = 'cljs$core$IAssociative$_assoc$arity$3$LKEY';
  const seq = 'cljs$core$ISeqable$_seq$arity$1$LKEY';
  const persistentSlots = [...nth, ...lookup, count, peek, conj, assoc, seq];
  const source3 = r => r.vector(1, 2, 3, 4);
  const kinds = [
    ['vector', () => [runtime.vector(1, 2, 3), c.vector(), c.assoc(c.subvec(source3(runtime), 1, 3), 0, 9), c.PersistentVector(null, 2, 5, null, [1, 2], null)],
      vectorLike + ' IKVReduce IEditableCollection ICloneable', persistentSlots],
    ['subvec', () => [c.subvec(source3(runtime), 1, 3), c.subvec(c.vec(c.range(40)), 2, 39), c.Subvec(null, source3(runtime), 1, 3, null), c.Subvec(null, c.list(1, 2, 3), 0, 2, null), c.Subvec(null, source3(runtime), 3, 1, null)],
      vectorLike + ' IKVReduce ICloneable', persistentSlots],
    ['entry', () => [c.first(c.hashMap(1, 2)), c.find(c.hashMap('a', 'b'), 'a'), c.MapEntry('k', 'v', null)],
      vectorLike + ' IMapEntry', persistentSlots],
    // A transient vector that is no longer editable too: its slots throw as
    // the operations do.
    ['transientVector', () => { const persisted = c.transient(runtime.vector(1)); c.persistentBang(persisted); return [c.transient(runtime.vector(1, 2)), c.transient(c.vec(c.range(40))), persisted]; },
      'IFn ICounted IIndexed ILookup ITransientCollection ITransientAssociative ITransientVector', [...nth, ...lookup, count]]
  ];
  for (const [kind, make, implemented, slots] of kinds) {
    const values = make(), proto = Object.getPrototypeOf(values[0]), names = implemented.split(' ');
    const parts = [0, 0];
    for (const name of names) if (masks.has(name)) parts[masks.get(name)[0]] |= masks.get(name)[1];
    for (const value of values) assert.equal(Object.getPrototypeOf(value), proto, `${kind}: one prototype`);
    for (const part of [0, 1]) {
      const mask = 'cljs$lang$protocol_mask$partition' + part + '$$LKEY';
      assert.ok(Object.prototype.hasOwnProperty.call(proto, mask), `${kind}: own ${mask}`);
      assert.equal(proto[mask], parts[part], `${kind}: ${mask}`);
    }
    for (const name of protocols) {
      const marker = proto['cljs$core$' + name + '$$LKEY'];
      assert.equal(marker, !masks.has(name) && names.includes(name) ? c.protocolSentinel : undefined, `${kind}: marker ${name}`);
      for (const value of values) {
        assert.equal(c.implements(c[name], value), names.includes(name), `${kind}: implements? ${name}`);
        assert.equal(c.satisfies(c[name], value), names.includes(name), `${kind}: satisfies? ${name}`);
      }
    }
    for (const slot of [...persistentSlots]) {
      assert.equal(Object.prototype.hasOwnProperty.call(proto, slot), slots.includes(slot), `${kind}: own ${slot}`);
      if (slots.includes(slot)) assert.equal(typeof proto[slot], 'function', `${kind}: ${slot}`);
    }
    // Each slot answers as its operation does, for every value of the kind:
    // the compile-time runtime's protocol lookup finds and calls it.
    for (const value of values) {
      const answers = [
        [nth[0], [0], () => c.nth(value, 0)], [nth[1], [7, 'nf'], () => c.nth(value, 7, 'nf')],
        [lookup[0], [1], () => c.get(value, 1)], [lookup[1], ['k', 'nf'], () => c.get(value, 'k', 'nf')],
        [count, [], () => c.count(value)], [peek, [], () => c.peek(value)],
        [conj, [5], () => c.conj(value, 5)], [assoc, [0, 6], () => c.assoc(value, 0, 6)], [seq, [], () => c.seq(value)]];
      // An answer, or the message of what the call threw.
      const outcome = thunk => { try { const result = thunk(); return 'ok ' + c.prStr(result); } catch (error) { return 'error ' + error.message; } };
      for (const [slot, args, operation] of answers) {
        if (!slots.includes(slot)) continue;
        // What a slot gives a value with the prototype and no fields, as
        // only reflection makes, is the private answer by which the
        // operation's older path takes the value (`noBranch`).
        const forged = Object.create(proto), noBranch = proto[slot].call(forged, forged, ...args);
        assert.ok(noBranch !== null && typeof noBranch === 'object' && Object.isFrozen(noBranch), `${kind}: ${slot} of a forged value`);
        assert.equal(outcome(() => proto[slot].call(value, value, ...args)), outcome(operation), `${kind}: ${slot} of ${c.prStr(value)}`);
      }
    }
  }
}
console.log('vector_family_prototype_contract: masks, markers and slots of the vector classes');

// A hash or array set's and a queue's `-count`, `-seq` and `-conj`, a set's
// `-lookup` and a vector's, a subvector's and a map entry's
// `-contains-key?` are slots of their prototypes (light-cljs-1046): each
// answers as its operation does for every value of the class, a public
// constructor's included, and a value made by reflection gets `noBranch`.
{
  const count = 'cljs$core$ICounted$_count$arity$1$LKEY', seq = 'cljs$core$ISeqable$_seq$arity$1$LKEY';
  const conj = 'cljs$core$ICollection$_conj$arity$2$LKEY', lookup = ['cljs$core$ILookup$_lookup$arity$2$LKEY', 'cljs$core$ILookup$_lookup$arity$3$LKEY'];
  const contains = 'cljs$core$IAssociative$_contains_key_QMARK_$arity$2$LKEY';
  const outcome = thunk => { try { return 'ok ' + c.prStr(thunk()); } catch (error) { return 'error ' + error.message; } };
  const kinds = [
    ['set', [c.hashSet(), c.hashSet(1, 2), c.set(c.range(40)), c.withMeta(c.hashSet(3), c.hashMap('m', 1)), c.PersistentHashSet(null, null, null),
      c.PersistentHashSet(null, c.arrayMap('a', null, 'b', null), null), c.PersistentHashSet(null, c.PersistentArrayMap(null, 5, ['a', null], null), null),
      c.PersistentHashSet(null, c.into(c.hashMap(), c.mapSeq(x => c.vector(x, null), c.range(20))), null)],
      [[count, []], [seq, []], [conj, [7]], [conj, [1]], [lookup[0], [1]], [lookup[1], ['a', 'nf']], [lookup[1], [5, 'nf']]]],
    ['queue', [c.PersistentQueue.EMPTY, c.into(c.PersistentQueue.EMPTY, c.vector(1, 2)), c.withMeta(c.conj(c.PersistentQueue.EMPTY, 1), c.hashMap('m', 1)),
      c.PersistentQueue(null, 3, c.list(1, 2), c.vector(3), null), c.PersistentQueue(null, 9, null, c.vector(1), null)],
      [[count, []], [seq, []], [conj, [7]]]],
    ['vector', [c.vector(), c.vector(1, 2), c.vec(c.range(40)), c.subvec(c.vector(1, 2, 3), 1), c.first(c.hashMap('k', 'v')), c.PersistentVector(null, 2, 5, null, [1, 2], null)],
      [[contains, [0]], [contains, [1]], [contains, [2]], [contains, [-1]], [contains, [0.5]], [contains, ['a']], [contains, [null]]]]
  ];
  const operations = new Map([[count, v => c.count(v)], [seq, v => c.seq(v)], [conj, (v, x) => c.conj(v, x)], [lookup[0], (v, k) => c.get(v, k)],
    [lookup[1], (v, k, nf) => c.get(v, k, nf)], [contains, (v, k) => c.contains(v, k)]]);
  for (const [kind, values, slots] of kinds) {
    for (const value of values) {
      for (const [slot, args] of slots) {
        const method = value[slot];
        assert.equal(typeof method, 'function', `${kind}: ${slot}`);
        const forged = Object.create(Object.getPrototypeOf(value)), noBranch = method.call(forged, forged, ...args);
        assert.ok(noBranch !== null && typeof noBranch === 'object' && Object.isFrozen(noBranch), `${kind}: ${slot} of a forged value`);
        // The operation's own path: the slot taken off the prototype.
        const proto = Object.getPrototypeOf(value), own = () => {
          delete proto[slot];
          try { return [outcome(() => operations.get(slot)(value, ...args)), outcome(() => c.meta(operations.get(slot)(value, ...args)))]; }
          finally { proto[slot] = method; }
        };
        const [expected, expectedMeta] = own();
        assert.equal(outcome(() => method.call(value, value, ...args)), expected, `${kind}: ${slot} of ${c.prStr(value)}`);
        if (slot === conj) assert.equal(outcome(() => c.meta(method.call(value, value, ...args))), expectedMeta, `${kind}: meta of conj`);
      }
    }
  }
  // Their answers by the operations' own paths, as before the slots.
  assert.deepEqual([c.count(c.PersistentHashSet(null, c.PersistentArrayMap(null, 5, ['a', null], null), null)), c.count(c.PersistentQueue(null, 9, null, c.vector(1), null)),
    c.prStr(c.seq(c.PersistentQueue(null, 3, c.list(1, 2), c.vector(3), null))), c.contains(c.vector(1, 2), 1), c.contains(c.vector(1, 2), 2)], [5, 9, '(1 2 3)', true, false]);
}
console.log('set, queue and vector contains slots answer as their operations');

// Keywords and symbols of two copies of one build (docs/value-layout.md,
// sections 1 and 3, light-cljs-1011): each copy interns its own, so they are
// never identical, but equality reads the keyed kind and `fqn` (`str`), never
// `instanceof`, so they are equal, hash alike, and find each other's entries
// in array maps and hash maps; a host object holding the same field names
// without the keyed kind is no keyword.
{
  const other = (0, eval)(fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8'));
  const a = runtime.core, b = other.core;
  const ka = a.keyword('n', 'k'), kb = b.keyword('n', 'k'), sa = a.symbol('n', 's'), sb = b.symbol('n', 's');
  assert.notEqual(ka, kb);
  assert.equal(a.equal(ka, kb), true);
  assert.equal(b.equal(kb, ka), true);
  assert.equal(a.equal(sa, sb), true);
  assert.equal(a.equal(ka, b.keyword('n', 'j')), false);
  assert.equal(a.equal(ka, sb), false);
  assert.equal(a.hash(ka), b.hash(kb));
  assert.equal(a.get(a.arrayMap(a.keyword('x'), 0, ka, 1), kb), 1);
  assert.equal(a.get(a.hashMap(ka, 2), kb), 2);
  assert.equal(a.isKeyword(kb), true);
  assert.equal(a.isSymbol(sb), true);
  assert.equal(a.name(kb), 'k');
  assert.equal(a.namespace(sb), 'n');
  const host = {ns: 'n', name: 'k', fqn: 'n/k', ns$LKEY: 'n', name$LKEY: 'k', fqn$LKEY: 'n/k', hash$LKEY: null};
  assert.equal(a.isKeyword(host), false);
  assert.equal(a.equal(ka, host), false);
  assert.equal(a.get(a.arrayMap(ka, 1), host, 'none'), 'none');
  // A symbol holds its metadata in its keyed `meta` field (light-cljs-1031),
  // so either copy reads what the other gave it, and gives it anew.
  const sm = b.withMeta(b.symbol(null, 's'), 17);
  assert.equal(b.meta(sm), 17);
  assert.equal(a.meta(sm), 17);
  assert.equal(sm._meta, 17);
  assert.equal(b.meta(a.withMeta(sm, 18)), 18);
  assert.equal(a.meta(sm), 17);
  assert.equal(b.meta(a.withMeta(sa, 19)), 19);
  assert.equal(a.meta(sb), null);
  assert.equal(a.meta(b.withMeta(sm, null)), null);
  assert.equal(a.meta(new b.Symbol(null, 't', 't', null, 20)), 20);
  // Upstream's public `_meta` field writes the keyed field too.
  const sw = b.symbol(null, 'w');
  for (const value of [23, null, undefined]) {
    sw._meta = value;
    assert.equal(sw._meta, value);
    assert.equal(a.meta(sw), value);
    assert.equal(b.meta(sw), value);
  }
  throwsError(() => { Object.create(Object.getPrototypeOf(sw))._meta = 1; });
}
console.log('keywords and symbols of two copies of one build compare by kind and fqn');

// A lazy seq's and a chunked cons's public fields keep an undefined value as
// given, as their payloads did (the Sol review of light-cljs-1029, finding
// 2): the class tests ask whether the field is present, not what it holds.
{
  const c = runtime.core;
  const xs = new c.LazySeq(null, undefined, c.list(1), null), ys = new c.LazySeq(null, () => c.list(2, 3), null, null);
  assert.equal(xs.fn, undefined);
  assert.equal(c.prStr(c.vec(xs)), '[1]');
  ys.fn = undefined;
  assert.equal(ys.fn, undefined);
  assert.equal(c.prStr(c.vec(ys)), '[]');
  assert.equal(c.isSeq(ys), true);
  const cc = new c.ChunkedCons(undefined, c.list(9), null, null);
  assert.equal(cc.chunk, undefined);
  assert.equal(c.isChunkedSeq(cc), true);
}
console.log('lazy seq and chunked cons fields keep undefined');

// The page-wide runtime key registry (docs/value-layout.md, section 1,
// light-cljs-1025): this file keeps the placeholder as its key and
// registers nothing; a copy whose placeholders the output writer replaced
// registers its key with its hash as it loads, a load that finds its key
// with another hash throws before any value exists, naming `:runtime-key`,
// and the same text loaded twice finds its own hash.
{
  const vm = require('node:vm');
  const source = fs.readFileSync(path.join(__dirname, '../../src/js_runtime.js'), 'utf8');
  const registry = Symbol.for('light-cljs.runtime-keys');
  assert.equal(Object.getOwnPropertySymbols(globalThis).includes(registry), false);
  // Placeholders end names, as src/runtime_key.rs substitutes them.
  const written = (key, hash) =>
    source.replace(/\$LKEY(?![\p{ID_Continue}$])/gu, '$' + key).replace(/"\$LHASH"/g, JSON.stringify(hash));
  const realm = vm.createContext({});
  const load = text => vm.runInContext(text, realm);
  const a = load(written('abcd1234', 'aaaaaaaaaaaaaaaa'));
  const keys = vm.runInContext('globalThis', realm)[registry];
  assert.ok(keys instanceof vm.runInContext('Map', realm));
  assert.deepEqual(Array.from(keys, ([key, hash]) => [key, hash]), [['$abcd1234', 'aaaaaaaaaaaaaaaa']]);
  assert.equal(a.core.count(a.vector(1, 2)), 2);
  const twice = load(written('abcd1234', 'aaaaaaaaaaaaaaaa'));
  assert.equal(twice.core.count(a.vector(1, 2, 3)), 3);
  assert.throws(() => load(written('abcd1234', 'bbbbbbbbbbbbbbbb')),
    error => error.message === 'Two light-cljs builds share the runtime key abcd1234: give one a different :runtime-key');
  const other = load(written('wxyz5678', 'bbbbbbbbbbbbbbbb'));
  assert.equal(keys.size, 2);
  // Different keys: neither runtime recognises the other's vector, nor its
  // keywords and symbols (light-cljs-1011), which are no keys of its maps.
  assert.equal(other.core.isVector(a.vector(1)), false);
  assert.equal(a.core.isVector(a.vector(1)), true);
  const [ka, ko, sa, so] = [a.core.keyword('n', 'k'), other.core.keyword('n', 'k'), a.core.symbol('n', 's'), other.core.symbol('n', 's')];
  for (const [mine, theirs, isKind] of [[a, ko, 'isKeyword'], [other, ka, 'isKeyword'], [a, so, 'isSymbol'], [other, sa, 'isSymbol']])
    assert.equal(mine.core[isKind](theirs), false, isKind);
  assert.equal(a.core.equal(ka, ko), false);
  assert.equal(other.core.equal(ko, ka), false);
  assert.equal(a.core.equal(sa, so), false);
  assert.equal(a.core.get(a.core.arrayMap(ka, 1), ko, 'none'), 'none');
  assert.equal(a.core.get(a.core.hashMap(ka, 1), ko, 'none'), 'none');
  assert.equal(other.core.get(other.core.arrayMap(ko, 1), ka, 'none'), 'none');
}
console.log('runtime_key_registry: keys register page-wide and collisions throw');

// Map transients' own fields and the readers of the maps' fields
// (light-cljs-1036), at the public constructors' edges.
{
  // A public array of odd length ends in a key without a value; its
  // transient pads it, so `assoc!` keeps keys at even indexes.
  const t = c.assocBang(c.transient(new c.PersistentArrayMap(null, 1, ['a'], null)), 'b', 2);
  assert.equal(c.count(t), 2);
  const persisted = c.persistentBang(t);
  assert.equal(c.get(persisted, 'b'), 2);
  assert.equal(c.get(persisted, 'a', 'none'), undefined);
  // A public sorted set of an array map's keys.
  const sortedKeys = c.PersistentTreeSet(null, c.arrayMap(1, null), null);
  assert.equal(c.count(sortedKeys), 1);
  assert.equal(c.get(sortedKeys, 1), 1);
  // A map iterator reads a public constructor's array as it was when made.
  const backing = ['a', 1], values = new c.PersistentArrayMap(null, 1, backing, null).values();
  backing[1] = 99;
  assert.equal(values.next().value, 1);
  // An undefined public hash map count stays undefined.
  assert.equal(c.count(new c.PersistentHashMap(null, undefined, null, false, null, null)), undefined);
  // A hash map built of a public array map reads each key once.
  let reads = 0;
  const arr = [];
  Object.defineProperty(arr, 0, {enumerable: true, get: () => { reads++; return 'k'; }});
  arr[1] = 1;
  const root = new c.PersistentArrayMap(null, 1, arr, null);
  reads = 0;
  const built = new c.PersistentHashMap(null, 1, root, false, null, null);
  assert.equal(reads, 1);
  assert.equal(c.get(built, 'k'), 1);
}
console.log('map transients and public map constructors read their fields once');

// A sorted map is a red-black tree whose nodes are map entries
// (light-cljs-1035): random `assoc` and `dissoc` keep upstream's invariants
// (a black root, no red node under a red one, as many black nodes on every
// path) and give the entries, seqs, `rseq`, `subseq` and `reduce-kv` a
// sorted array of the same keys gives.
{
  const core = runtime.core;
  const check = (node, red) => {
    if (node === null) return 1;
    assert.equal(core.isMapEntry(node), true);
    if (red) assert.equal(node.red$LKEY, false, 'no red node under a red one');
    const left = check(node.left$LKEY, node.red$LKEY), right = check(node.right$LKEY, node.red$LKEY);
    assert.equal(left, right, 'black height');
    return left + (node.red$LKEY ? 0 : 1);
  };
  let seed = 7;
  const random = n => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed % n; };
  let map = core.sortedMap(), model = new Map();
  for (let step = 0; step < 4000; step++) {
    const key = random(300);
    if (random(3) === 0) { map = core.dissoc(map, key); model.delete(key); }
    else { map = core.assoc(map, key, step); model.set(key, step); }
    if (step % 97 === 0 || step > 3990) {
      const tree = map.tree$LKEY;
      if (tree !== null) assert.equal(tree.red$LKEY, false, 'black root');
      check(tree, false);
      const keys = [...model.keys()].sort((a, b) => a - b);
      assert.equal(core.count(map), keys.length);
      assert.deepEqual(core.toArray(core.keys(map) ?? core.list()), keys);
      assert.deepEqual(core.toArray(core.vals(map) ?? core.list()), keys.map(k => model.get(k)));
      assert.deepEqual(core.toArray(core.mapSeq(core.keyProtocol, core.rseq(map) ?? core.list())), [...keys].reverse());
      assert.deepEqual(core.toArray(core.mapSeq(core.keyProtocol, core.subseq(map, core.greaterEqual, 150) ?? core.list())), keys.filter(k => k >= 150));
      assert.deepEqual(core.toArray(core.mapSeq(core.keyProtocol, core.rsubseq(map, core.less, 150) ?? core.list())), keys.filter(k => k < 150).reverse());
      assert.deepEqual(core.reduceKv((xs, k, v) => (xs.push(k, v), xs), [], map), keys.flatMap(k => [k, model.get(k)]));
      for (const k of [0, 77, 150, 299, 300]) assert.equal(core.get(map, k, 'none'), model.has(k) ? model.get(k) : 'none');
    }
  }
  // An equal value keeps the map, a key not there too; `find` gives the node.
  const small = core.sortedMap(2, 'b', 1, 'a', 3, 'c');
  assert.equal(core.assoc(small, 2, 'b'), small);
  assert.equal(core.dissoc(small, 9), small);
  assert.equal(core.find(small, 2), core.first(core.rest(core.seq(small))));
  assert.equal(core.keyProtocol(core.find(small, 2)), 2);
  assert.equal(core.count(core.dissoc(core.dissoc(core.dissoc(small, 1), 2), 3)), 0);
}
console.log('sorted maps are red-black trees of map entries');

// A public constructor's tree of other objects is read by upstream's names,
// and rebuilt of nodes, balanced, before an update, which keeps the count it
// was given (the Sol review of light-cljs-1035); a write of a map entry's
// public field gives it an own field and changes neither entry nor tree.
{
  const core = runtime.core;
  const leaf = (key, val) => ({key, val, left: null, right: null});
  const tree = {key: 4, val: 'd', left: {key: 2, val: 'b', left: leaf(1, 'a'), right: leaf(3, 'c')}, right: {key: 6, val: 'f', left: leaf(5, 'e'), right: null}};
  const m = new core.PersistentTreeMap(core.compare, tree, 9, core.hashMap('m', 1), null);
  assert.equal(m.tree, tree);
  assert.equal(core.count(m), 9);
  assert.equal(core.get(m, 3), 'c');
  const black = node => {
    if (node === null) return 1;
    if (node.red$LKEY) for (const child of [node.left$LKEY, node.right$LKEY]) assert.equal(child === null || !child.red$LKEY, true);
    const left = black(node.left$LKEY), right = black(node.right$LKEY);
    assert.equal(left, right);
    return left + (node.red$LKEY ? 0 : 1);
  };
  const added = core.assoc(m, 7, 'g'), removed = core.dissoc(m, 2), replaced = core.assoc(m, 5, 'E');
  for (const [map, count, keys] of [[added, 10, [1, 2, 3, 4, 5, 6, 7]], [removed, 8, [1, 3, 4, 5, 6]], [replaced, 9, [1, 2, 3, 4, 5, 6]]]) {
    assert.equal(core.count(map), count);
    assert.deepEqual(core.toArray(core.keys(map)), keys);
    assert.equal(map.tree$LKEY.red$LKEY, false);
    black(map.tree$LKEY);
    assert.equal(core.equal(core.meta(map), core.hashMap('m', 1)), true);
  }
  assert.equal(core.get(replaced, 5), 'E');
  assert.equal(core.get(m, 5), 'e');
  assert.equal(m.tree, tree);
  assert.equal(core.assoc(m, 3, 'c') === m || core.count(core.assoc(m, 3, 'c')) === 9, true);
  const entry = core.first(core.sortedMap(1, 'a', 2, 'b'));
  for (const name of ['key', 'val', 'left', 'right']) {
    entry[name] = 'shadow';
    assert.equal(entry[name], 'shadow');
    assert.equal(Object.hasOwn(entry, name), true);
  }
  assert.equal(core.keyProtocol(entry), 1);
  assert.equal(core.valProtocol(entry), 'a');
}
console.log('public sorted map trees update as nodes and entry fields take shadow writes');
