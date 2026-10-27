// Synthetic cache/leaf regression fixture. Also run unchanged in Node.
(function () {
    function check(value) { if (!value) throw new Error('builtin cache regression'); }
    function push(a, v) { return a.push(v); }
    function pop(a) { return a.pop(); }
    function search(a, v, from) { return a.indexOf(v, from); }
    function copy(a, start, end) { return a.slice(start, end); }
    function typeError(f) {
        try { f(); } catch (e) { return e instanceof TypeError; }
        return false;
    }

    let a = [];
    for (let i = 0; i < 100; i++) check(push(a, i) === i + 1);
    check(a.length === 100 && pop(a) === 99);
    a.push = function (x) { this.changed = x; return 42; };
    check(push(a, 7) === 42 && a.changed === 7 && a.length === 99);
    delete a.push;
    check(push(a, 8) === 100 && a[99] === 8);
    let inherited = Object.create(Array.prototype);
    inherited.push = function (v) { return v + 20; };
    Object.setPrototypeOf(a, inherited);
    check(push(a, 3) === 23 && a.length === 100);
    inherited.push = function (v) { return v + 30; };
    check(push(a, 3) === 33);
    delete inherited.push;
    check(push(a, 9) === 101 && a[100] === 9);
    Object.setPrototypeOf(a, Array.prototype);
    let gets = 0, original = a.push;
    Object.defineProperty(a, 'push', {
        configurable: true,
        get() { gets++; return original; }
    });
    check(push(a, 10) === 102 && gets === 1);
    delete a.push;

    // A proxy callee must retain its apply trap after warming the builtin.
    let calls = 0;
    a.push = new Proxy(original, {
        apply(target, receiver, args) { calls++; return 77; }
    });
    check(push(a, 11) === 77 && calls === 1 && a.length === 102);
    delete a.push;
    a.push = new Proxy(function () { return 88; }, {});
    check(push(a, 12) === 88 && a.length === 102);
    a.push = 0;
    check(typeError(() => push(a, 12)));
    delete a.push;

    // Observable proxy reads and writes must not become plain array writes.
    let log = '', backing = [];
    let proxy = new Proxy(backing, {
        get(target, key, receiver) { log += 'g' + String(key) + ';'; return Reflect.get(target, key, receiver); }
    });
    check(push(proxy, 4) === 1 && backing[0] === 4);
    check(log === 'gpush;glength;');
    let setters = 0, proto = Object.create(Array.prototype), b = [];
    Object.defineProperty(proto, '0', {set(v) { setters += v; }});
    Object.setPrototypeOf(b, proto);
    check(push(b, 5) === 1 && setters === 5 && !(0 in Object.keys(b)));
    check(Object.getOwnPropertyDescriptor(b, '0') === undefined);

    for (let method of ['push', 'pop']) {
        let fixed = [1];
        Object.defineProperty(fixed, 'length', {writable: false});
        check(typeError(() => fixed[method]()));
        check(fixed.length === 1 && (method === 'push' ? fixed[0] === 1 : !(0 in fixed)));
        check(typeError(() => Object.freeze([1])[method](2)));
    }
    check(typeError(() => Object.preventExtensions([]).push(1)));
    let sealed = Object.seal([1]);
    check(typeError(() => pop(sealed)) && sealed.length === 1 && sealed[0] === 1);
    let large = [];
    large.length = 4294967295;
    let range = false;
    try { push(large, 13); } catch (e) { range = e instanceof RangeError; }
    check(range && large.length === 4294967295 && large['4294967295'] === 13);
    let many = [];
    for (let i = 0; i < 65; i++) many[i] = i;
    check([].push(...many) === 65);

    let reads = 0, c = [1, 2];
    check(pop(c) === 2);
    Object.defineProperty(c, '0', {configurable: true, get() { reads++; return 19; }});
    check(pop(c) === 19 && reads === 1 && c.length === 0);
    c = [1, , 3];
    check(pop(c) === 3 && pop(c) === undefined && pop(c) === 1 && pop(c) === undefined);
    let nullProto = [1, 2];
    Object.setPrototypeOf(nullProto, null);
    check(original.call(nullProto, 3) === 3 && Array.prototype.pop.call(nullProto) === 3);

    let values = [0, , undefined, NaN, 4];
    for (let from of [undefined, NaN, -Infinity, Infinity, -9, -2, -0, 2.7, 9]) {
        // Exercise omitted/default, fractional and infinite numeric indices.
        check(search(values, 4, from) === (from === Infinity || from >= 9 ? -1 : 4));
    }
    check(values.indexOf(undefined) === 2 && values.includes(undefined));
    check(values.indexOf(NaN) === -1 && values.includes(NaN));
    check(values.lastIndexOf(0) === 0 && values.lastIndexOf(4, undefined) === -1);
    check(values.lastIndexOf(4, Infinity) === 4 && values.lastIndexOf(4, -Infinity) === -1);
    check(values.at(-1) === 4 && values.at(NaN) === 0 && values.at(Infinity) === undefined);
    Object.defineProperty(values, '1', {configurable: true, get() { return 7; }});
    check(search(values, 7) === 1 && values.at(1) === 7);
    check(search(values, 4, {valueOf() { return 2; }}) === 4);
    check(many.indexOf(64) === 64 && many.lastIndexOf(64) === 64);

    // Expected rows are shared unchanged with Node. Object.is and reciprocals
    // distinguish a returned +0 index from the -0 allowed in fromIndex.
    let arrayLike = {
        0: -0, 1: NaN, 2: +0, 3: 3, length: 4,
        indexOf: Array.prototype.indexOf,
        lastIndexOf: Array.prototype.lastIndexOf,
        includes: Array.prototype.includes,
        findIndex: Array.prototype.findIndex
    };
    // fromIndex, first zero, last zero, includes NaN
    let indexRows = [
        [-0, 0, 0, true], [+0, 0, 0, true], [NaN, 0, 0, true],
        [undefined, 0, 0, true], [-Infinity, 0, -1, true],
        [-5, 0, -1, true], [-4, 0, 0, true], [-3, 2, 0, true],
        [-2, 2, 2, false], [-1, -1, 2, false],
        [-0.5, 0, 0, true], [0.5, 0, 0, true],
        [-1.8, -1, 2, false], [1.8, 2, 0, true], [2.8, 2, 2, false],
        [4, -1, 2, false], [Infinity, -1, 2, false]
    ];
    for (let receiver of [[-0, NaN, +0, 3], arrayLike]) {
        check(Object.is(receiver.indexOf(0), 0) && receiver.lastIndexOf(0) === 2);
        for (let row of indexRows) {
            for (let needle of [-0, +0]) {
                let first = receiver.indexOf(needle, row[0]);
                let last = receiver.lastIndexOf(needle, row[0]);
                check(Object.is(first, row[1]) && Object.is(last, row[2]));
                check((first !== 0 || 1 / first === Infinity) &&
                      (last !== 0 || 1 / last === Infinity));
                check(receiver.includes(needle, row[0]) === (row[1] !== -1));
            }
            check(receiver.indexOf(NaN, row[0]) === -1 &&
                  receiver.lastIndexOf(NaN, row[0]) === -1 &&
                  receiver.includes(NaN, row[0]) === row[3]);
            // findIndex's second argument is thisArg, never a starting index.
            let found = receiver.findIndex(value => value === 0, row[0]);
            check(Object.is(found, 0) && 1 / found === Infinity);
        }
        for (let row of [[-0, 0], [+0, 2], [NaN, 1], [-1, -1], [0.5, -1], [3, 3]]) {
            check(Object.is(receiver.findIndex((value, index, object) => {
                check(object === receiver && (index !== 0 ||
                      (Object.is(index, 0) && 1 / index === Infinity)));
                return Object.is(value, row[0]);
            }), row[1]));
        }
    }

    let source = [1, , 3], result = copy(source, 0, 3);
    check(result.length === 3 && result[0] === 1 && !(1 in result) && result[2] === 3);
    check(copy(source, -2).length === 2 && copy(source, Infinity).length === 0);
    check(copy(source, 0, NaN).length === 0 && copy(source, 2, 1).length === 0);
    Object.defineProperty(source, '1', {configurable: true, get() { return 8; }});
    check(copy(source, 0, 3)[1] === 8);
    let species = 0;
    source.constructor = {[Symbol.species]: function (n) { species++; this.length = n; }};
    result = copy(source, 0, 2);
    check(species === 1 && !Array.isArray(result) && result.length === 2 && result[1] === 8);
    check(copy(many, 0, 65).length === 65);
    let getterSource = [1, 2];
    Object.defineProperty(getterSource, 'constructor', {get() { reads++; return Array; }});
    check(copy(getterSource)[1] === 2 && reads === 2);

    // The same named instruction alternates intrinsic stores and own stores.
    function get(o) { return o.get(1); }
    let m = new Map([[1, 2]]);
    check(get(m) === 2 && get({get() { return 3; }}) === 3 && get(m) === 2);
    m.get = function () { return 4; };
    check(get(m) === 4);
    delete m.get;
    check(get(m) === 2);
    return true;
})()
