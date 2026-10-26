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
{
    function hidden(target, key) {return new Proxy(target, {has(t, k) {return k !== key && k in t;}});}
    let p = hidden([1, 2, 3], '1');
    let mapped = Array.prototype.map.call(p, x => x * 2);
    assert(mapped.length === 3 && mapped[0] === 2 && !(1 in mapped) && mapped[2] === 6, 'map presence');
    assert(Array.prototype.filter.call(p, () => true).join(',') === '1,3', 'filter presence');
    assert(Array.prototype.reduce.call(p, (a, b) => a + b, 0) === 4, 'reduce presence');
    assert(Array.prototype.reduceRight.call(p, (a, b) => a + b, 0) === 4, 'reduceRight presence');
    let visited = '';
    Array.prototype.forEach.call(p, x => {visited += x;});
    assert(visited === '13', 'forEach presence');
    assert(Array.prototype.every.call(p, x => x !== 2) && !Array.prototype.some.call(p, x => x === 2), 'predicate presence');
    assert(Array.prototype.indexOf.call(p, 2) === -1 && Array.prototype.lastIndexOf.call(p, 2) === -1, 'search presence');
    assert(Array.prototype.flatMap.call(p, x => [x]).join(',') === '1,3', 'flatMap presence');
    assert([1].flatMap(() => p).join(',') === '1,3', 'flatMap inner presence');
    assert([p].flat().join(',') === '1,3', 'flat inner presence');
    assert(Array.prototype.slice.call(p).join(',') === '1,,3', 'slice presence');
    assert(Array.prototype.concat.call(p).join(',') === '1,,3', 'concat presence');
    let target = [3, 2, 1];
    Array.prototype.sort.call(hidden(target, '1'));
    assert(target.length === 3 && target[0] === 1 && target[1] === 3 && !(2 in target), 'sort presence');
    target = [1, 2, 3];
    Array.prototype.reverse.call(hidden(target, '0'));
    assert(target[0] === 3 && target[1] === 2 && !(2 in target), 'reverse presence');
    target = [1, 2, 3];
    assert(Array.prototype.shift.call(hidden(target, '1')) === 1 && target.length === 2 && !(0 in target) && target[1] === 3, 'shift presence');
    target = [1, 2, 3];
    Array.prototype.unshift.call(hidden(target, '1'), 0);
    assert(target.join(',') === '0,1,,3', 'unshift presence');
    target = [1, 2, 3];
    let removed = Array.prototype.splice.call(hidden(target, '1'), 0, 2);
    assert(removed.length === 2 && removed[0] === 1 && !(1 in removed) && target.join(',') === '3', 'splice presence');
    target = [1, 2, 3];
    Array.prototype.copyWithin.call(hidden(target, '1'), 0, 1);
    assert(!(0 in target) && target[1] === 3, 'copyWithin presence');
    assert(Intl.getCanonicalLocales(hidden(['en', 'fr'], '1')).join(',') === 'en', 'locale list presence');
    let options = hidden({cause: 42}, 'cause');
    assert(!('cause' in new Error('x', options)), 'error cause absence');
    assert(new Error('x', new Proxy({cause: 42}, {has() {return true;}})).cause === 42, 'error cause presence');
    let desc = new Proxy({value: 42, writable: true}, {has(t, k) {return k !== 'writable' && k in t;}});
    let object = Object.defineProperty({}, 'x', desc);
    assert(object.x === 42 && !Object.getOwnPropertyDescriptor(object, 'x').writable, 'descriptor presence');
    desc = new Proxy({}, {has(t, k) {return k === 'value';}});
    assert(Object.getOwnPropertyDescriptor(Object.defineProperty({}, 'x', desc), 'x').value === undefined, 'virtual descriptor presence');
}
true;
