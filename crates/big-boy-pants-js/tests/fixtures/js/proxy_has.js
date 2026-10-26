// First-party synthetic ECMAScript Proxy [[HasProperty]] regressions.
// Run unchanged in Node/V8 and big-boy-pants-js.
function assert(value, label) { if (!value) throw new Error(label); }
function typeError(f, label) {
    let caught;
    try { f(); } catch (e) { caught = e; }
    assert(caught instanceof TypeError, label);
}
{
    let target = {x: 1}, calls = 0, handler = {
        has(t, key) {
            assert(this === handler && t === target && arguments.length === 2, 'receiver and target');
            calls++;
            return key === 'virtual';
        }
    };
    let p = new Proxy(target, handler);
    assert('virtual' in p && !('x' in p) && !('missing' in p), 'trap presence');
    assert(Reflect.has(p, 'virtual') && !Reflect.has(p, 'x') && calls === 5, 'Reflect.has');
    assert('virtual' in Object.create(p), 'prototype Proxy');
    assert('x' in {__proto__: p, x: undefined} && calls === 6, 'own properties precede Proxy');
}
{
    let symbol = Symbol('key'), key;
    let p = new Proxy({}, {has(t, k) { key = k; return true; }});
    assert(symbol in p && key === symbol, 'symbol key');
    assert(12 in p && key === '12', 'numeric key');
    assert('\ud800' in p && key === '\ud800', 'UTF-16 key');
    let log = '', h = {get has() {log += 'g'; return function(t, k) {log += 'h'; return k === 'x';};}};
    let q = new Proxy({}, h), objectKey = {[Symbol.toPrimitive](hint) {log += hint; return 'x';}};
    assert(objectKey in q && log === 'stringgh', 'key conversion and getter order');
    log = '';
    assert(Reflect.has(q, objectKey) && log === 'stringgh', 'Reflect key conversion once');
    log = '';
    typeError(() => objectKey in null, 'invalid in base');
    typeError(() => Reflect.has(1, objectKey), 'invalid Reflect base');
    assert(log === '', 'invalid base does not convert key');
}
{
    let n = 0, target = Object.create({inherited: 1});
    Object.defineProperty(target, 'x', {get() {n++; throw 99;}});
    for (let h of [{}, {has: null}, {has: undefined}, {get has() {n++; return null;}}]) {
        let p = new Proxy(new Proxy(target, h), {});
        assert('x' in p && 'inherited' in p && !('absent' in p), 'absent trap forwarding');
    }
    assert(n === 3, 'has never invokes target getter');
    let h = Object.create({has(t, k) {return k === 'virtual';}});
    assert('virtual' in new Proxy({}, h), 'inherited trap');
    let f = function() {return true;};
    assert('x' in new Proxy({}, {has: new Proxy(f, {})}), 'callable Proxy trap');
    for (let v of [false, 0, '', NaN, null, undefined]) {
        assert(!('x' in new Proxy({}, {has() {return v;}})), 'false result conversion');
    }
    for (let v of [true, 1, 'yes', {}, {valueOf() {throw 99;}}]) {
        assert('x' in new Proxy({}, {has() {return v;}}), 'true result conversion');
    }
    for (let has of [false, 1, 'has', {}]) typeError(() => 'x' in new Proxy({}, {has}), 'noncallable trap');
    for (let h of [{get has() {throw 42;}}, {has() {throw 42;}}]) {
        let caught;
        try {'x' in new Proxy({}, h);} catch(e) {caught = e;}
        assert(caught === 42, 'trap/getter exception');
    }
}
{
    let fixed = {};
    Object.defineProperty(fixed, 'x', {value: 1});
    typeError(() => 'x' in new Proxy(fixed, {has() {return false;}}), 'nonconfigurable data');
    let accessor = {};
    Object.defineProperty(accessor, 'x', {get() {throw 99;}});
    typeError(() => Reflect.has(new Proxy(accessor, {has() {return 0;}}), 'x'), 'nonconfigurable accessor');
    let sealed = Object.preventExtensions({x: 1});
    typeError(() => 'x' in new Proxy(sealed, {has() {return false;}}), 'nonextensible own property');
    assert(!('absent' in new Proxy(sealed, {has() {return false;}})), 'absent nonextensible property');
    let inherited = Object.preventExtensions(Object.create(fixed));
    assert(!('x' in new Proxy(inherited, {has() {return false;}})), 'inherited property may be hidden');
    assert('absent' in new Proxy(sealed, {has() {return true;}}), 'truthy trap may invent presence');
    let target = {x: 1};
    assert(!('x' in new Proxy(target, {has(t) {delete t.x; return false;}})), 'invariants after trap mutation');
    typeError(() => 'x' in new Proxy({}, {has(t) {Object.defineProperty(t, 'x', {value: 1}); return false;}}), 'trap creates protected property');
}
{
    let log = '', target = {x: 1};
    let inner = new Proxy(target, {
        getOwnPropertyDescriptor(t, k) {log += 'd'; return Object.getOwnPropertyDescriptor(t, k);},
        isExtensible(t) {log += 'e'; return true;}
    });
    let p = new Proxy(inner, {has() {log += 'h'; return false;}});
    assert(!('x' in p) && log === 'hde', 'false trap descriptor then extensibility');
    log = '';
    assert(!('absent' in p) && log === 'hd', 'missing target avoids extensibility');
    log = '';
    assert('x' in new Proxy(inner, {has() {return true;}}) && log === '', 'true avoids invariants');
    let liar = new Proxy(target, {isExtensible() {return false;}});
    typeError(() => 'x' in new Proxy(liar, {has() {return false;}}), 'target extensibility invariant');
    let fixed = {};
    Object.defineProperty(fixed, 'x', {value: 1});
    log = '';
    let protectedTarget = new Proxy(fixed, {isExtensible() {log += 'e'; throw 99;}});
    typeError(() => 'x' in new Proxy(protectedTarget, {has() {return false;}}), 'nonconfigurable avoids extensibility');
    assert(log === '', 'nonconfigurable check order');
    for (let h of [{getOwnPropertyDescriptor() {throw 42;}}, {isExtensible() {throw 42;}}]) {
        let caught;
        try {'x' in new Proxy(new Proxy(target, h), {has() {return false;}});} catch(e) {caught = e;}
        assert(caught === 42, 'invariant callback exception');
    }
}
true;
