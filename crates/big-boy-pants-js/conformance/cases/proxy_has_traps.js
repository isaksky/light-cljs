// bbp-authored conformance case (light-cljs-890).
// Derives from: crates/big-boy-pants-js/rust/tests/fixtures/js/proxy_has.js lines 9-174
// (run by rust/tests/proxy_has.rs has_traps_follow_ecmascript,
// builtin_presence_checks_resume_without_replaying_indices and
// every_descriptor_reader_resumes_presence_and_value_callbacks): the rows
// of its blocks at lines 9-22, 23-38, 39-63, 64-80, 81-106, 107-151 and
// 154-174 that the Pants engine and QuickJS agree on (ECMAScript 10.5.7
// [[HasProperty]]: the `has` trap with its target and key, ToBoolean of its
// result, absent traps forwarding, the trap read through a getter, the
// invariants of a false result, and the presence checks of built-ins and of
// ToPropertyDescriptor through a proxy), and light-cljs-766's planner row
// `'foo' in p` of a `has` trap returning true. Rows left out, each with its
// reason:
//   'callable Proxy trap' (line 50): calling a proxy is its [[Call]],
//     light-cljs-891's.
//   'false trap descriptor then extensibility' (line 88): QuickJS throws
//     where the Pants engine passes (a disagreement); its isExtensible trap
//     is light-cljs-891's, as are 'target extensibility invariant' (line
//     94) and the isExtensible handler of 'invariant callback exception'
//     (lines 101-105).
//   The presence rows of Array.prototype.filter, reduce, reduceRight,
//     every, some, indexOf, lastIndexOf, flatMap, flat, slice, concat,
//     sort, reverse, shift, unshift, splice and copyWithin (lines 112-141)
//     and Intl.getCanonicalLocales (line 142): those built-ins are the core
//     built-ins and Intl areas' (light-cljs-767, -774).
//   'descriptor reader has exception' (lines 169-171): QuickJS calls no `has`
//     trap that throws there, where the Pants engine does (a disagreement).
//   The Object.getOwnPropertyDescriptor and Reflect.getOwnPropertyDescriptor
//     readers (lines 159-160): those built-ins on a proxy fail by name at
//     its [[GetOwnProperty]] (light-cljs-845 runs it for for-in and the key
//     walks only; light-cljs-895).
//   The block at lines 175-195 (isExtensible traps): light-cljs-891's.
function assert(value, label) { if (!value) throw new Error(label); }
function typeError(f, label) {
    let caught;
    try { f(); } catch (e) { caught = e; }
    assert(caught instanceof TypeError, label);
}
{
    let p=new Proxy({},{has(t,k){return true}});
    assert('foo' in p, 'planner row');
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
    assert(!('absent' in p) && log === 'hd', 'missing target avoids extensibility');
    log = '';
    assert('x' in new Proxy(inner, {has() {return true;}}) && log === '', 'true avoids invariants');
    let fixed = {};
    Object.defineProperty(fixed, 'x', {value: 1});
    log = '';
    let protectedTarget = new Proxy(fixed, {isExtensible() {log += 'e'; throw 99;}});
    typeError(() => 'x' in new Proxy(protectedTarget, {has() {return false;}}), 'nonconfigurable avoids extensibility');
    assert(log === '', 'nonconfigurable check order');
    let caught;
    try {'x' in new Proxy(new Proxy(target, {getOwnPropertyDescriptor() {throw 42;}}), {has() {return false;}});} catch(e) {caught = e;}
    assert(caught === 42, 'invariant callback exception');
}
{
    function hidden(target, key) {return new Proxy(target, {has(t, k) {return k !== key && k in t;}});}
    let p = hidden([1, 2, 3], '1');
    let mapped = Array.prototype.map.call(p, x => x * 2);
    assert(mapped.length === 3 && mapped[0] === 2 && !(1 in mapped) && mapped[2] === 6, 'map presence');
    let visited = '';
    Array.prototype.forEach.call(p, x => {visited += x;});
    assert(visited === '13', 'forEach presence');
    let options = hidden({cause: 42}, 'cause');
    assert(!('cause' in new Error('x', options)), 'error cause absence');
    assert(new Error('x', new Proxy({cause: 42}, {has() {return true;}})).cause === 42, 'error cause presence');
    let desc = new Proxy({value: 42, writable: true}, {has(t, k) {return k !== 'writable' && k in t;}});
    let object = Object.defineProperty({}, 'x', desc);
    assert(object.x === 42 && !Object.getOwnPropertyDescriptor(object, 'x').writable, 'descriptor presence');
    desc = new Proxy({}, {has(t, k) {return k === 'value';}});
    assert(Object.getOwnPropertyDescriptor(Object.defineProperty({}, 'x', desc), 'x').value === undefined, 'virtual descriptor presence');
}
// Every supported ToPropertyDescriptor reader uses HasProperty before Get.
{
    let readers=[
        d=>Object.defineProperty({},'x',d).x===42,
        d=>Object.defineProperties({},{x:d}).x===42,
        d=>{let o={};return Reflect.defineProperty(o,'x',d)&&o.x===42;}
    ];
    for (let read of readers) {
        let log='', desc=new Proxy({value:42,configurable:true},{
            has(t,k){log+='h'+k+';';return k in t;},
            get(t,k,r){log+='g'+k+';';return Reflect.get(t,k,r);}
        });
        assert(read(desc),'descriptor reader value');
        assert(log==='henumerable;hconfigurable;gconfigurable;hvalue;gvalue;hwritable;hget;hset;','descriptor reader order');
        typeError(()=>read(new Proxy({},{has(t,k){return k==='value'||k==='get';}})),'virtual undefined descriptor fields still conflict');
    }
}
true;
