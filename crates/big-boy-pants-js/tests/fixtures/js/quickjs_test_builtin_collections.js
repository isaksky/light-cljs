/* Adapted QuickJS test text; synthetic fixture, not light-cljs output.
Source: research/quickjs/tests/test_builtin.js at
7d1c13c5d8f8c6b32893786ed35cdcebd88194d2 (VERSION 2026-06-04).
Selected lines: 1-71, 456-522, 845-906, 908-934, 936-959, 961-982, 984-1005.
Changes: prepend this notice; append one call per case; adapt test_weak_ref:
- Lines 976-977 and 979-980: replace each assert(wN.deref(), undefined);
  with assert(wN.deref() !== undefined); for N = 1, 2, preserving indentation.
- After line 980 insert the following block, capturing each iteration separately:
        let refs = [w1, w2];
        os.setTimeout(() => {
            std.gc();
            assert(refs[0].deref(), undefined);
            assert(refs[1].deref(), undefined);
        }, 0);
Same-job targets stay live; timer jobs clear kept objects before collection.
Every other case and the assertion helpers are unmodified.
The Rust harness installs std.gc/os.setTimeout and retains WeakMaps through a
task-local constructor wrapper outside these unchanged upstream definitions.

QuickJS Javascript Engine

Copyright (c) 2017-2021 Fabrice Bellard
Copyright (c) 2017-2021 Charlie Gordon

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/
"use strict";

var status = 0;
var throw_errors = true;

function throw_error(msg) {
    if (throw_errors)
        throw Error(msg);
    console.log(msg);
    status = 1;
}

function assert(actual, expected, message) {
    function get_full_type(o) {
        var type = typeof(o);
        if (type === 'object') {
            if (o === null)
                return 'null';
            if (o.constructor && o.constructor.name)
                return o.constructor.name;
        }
        return type;
    }

    if (arguments.length == 1)
        expected = true;

    if (typeof actual === typeof expected) {
        if (actual === expected) {
            if (actual !== 0 || (1 / actual) === (1 / expected))
                return;
        }
        if (typeof actual === 'number') {
            if (isNaN(actual) && isNaN(expected))
                return true;
        }
        if (typeof actual === 'object') {
            if (actual !== null && expected !== null
            &&  actual.constructor === expected.constructor
            &&  actual.toString() === expected.toString())
                return;
        }
    }
    // Should output the source file and line number and extract
    //   the expression from the assert call
    throw_error("assertion failed: got " +
                get_full_type(actual) + ":|" + actual + "|, expected " +
                get_full_type(expected) + ":|" + expected + "|" +
                (message ? " (" + message + ")" : ""));
}

function assert_throws(expected_error, func)
{
    var err = false;
    try {
        func();
    } catch(e) {
        err = true;
        if (!(e instanceof expected_error)) {
            // Should output the source file and line number and extract
            //   the expression from the assert_throws() call
            throw_error("unexpected exception type");
            return;
        }
    }
    if (!err) {
        // Should output the source file and line number and extract
        //   the expression from the assert_throws() call
        throw_error("expected exception");
    }
}
function test_typed_array()
{
    var buffer, a, i, str;

    a = new Uint8Array(4);
    assert(a.length, 4);
    for(i = 0; i < a.length; i++)
        a[i] = i;
    assert(a.join(","), "0,1,2,3");
    a[0] = -1;
    assert(a[0], 255);

    a = new Int8Array(3);
    a[0] = 255;
    assert(a[0], -1);

    a = new Int32Array(3);
    a[0] = Math.pow(2, 32) - 1;
    assert(a[0], -1);
    assert(a.BYTES_PER_ELEMENT, 4);

    a = new Uint8ClampedArray(4);
    a[0] = -100;
    a[1] = 1.5;
    a[2] = 0.5;
    a[3] = 1233.5;
    assert(a.toString(), "0,2,0,255");

    buffer = new ArrayBuffer(16);
    assert(buffer.byteLength, 16);
    a = new Uint32Array(buffer, 12, 1);
    assert(a.length, 1);
    a[0] = -1;

    a = new Uint16Array(buffer, 2);
    a[0] = -1;

    a = new Float16Array(buffer, 8, 1);
    a[0] = 1;

    a = new Float32Array(buffer, 8, 1);
    a[0] = 1;

    a = new Uint8Array(buffer);

    str = a.toString();
    /* test little and big endian cases */
    if (str !== "0,0,255,255,0,0,0,0,0,0,128,63,255,255,255,255" &&
        str !== "0,0,255,255,0,0,0,0,63,128,0,0,255,255,255,255") {
        assert(false);
    }

    assert(a.buffer, buffer);

    a = new Uint8Array([1, 2, 3, 4]);
    assert(a.toString(), "1,2,3,4");
    a.set([10, 11], 2);
    assert(a.toString(), "1,2,10,11");

    // https://github.com/quickjs-ng/quickjs/issues/1208
    buffer = new ArrayBuffer(16);
    a = new Uint8Array(buffer);
    a.fill(42);
    assert(a[0], 42);
    buffer.transfer();
    assert(a[0], undefined);
}
function test_map1(key_type, n)
{
    var a, i, tab, o, v;
    a = new Map();
    tab = [];
    for(i = 0; i < n; i++) {
        v = { };
        switch(key_type) {
        case "small_bigint":
            o = BigInt(i);
            break;
        case "bigint":
            o = BigInt(i) + (1n << 128n);
            break;
        case "object":
            o = { id: i };
            break;
        default:
            assert(false);
        }
        tab[i] = [o, v];
        a.set(o, v);
    }

    assert(a.size, n);
    for(i = 0; i < n; i++) {
        assert(a.get(tab[i][0]), tab[i][1]);
    }

    i = 0;
    a.forEach(function (v, o) {
        assert(o, tab[i++][0]);
        assert(a.has(o));
        assert(a.delete(o));
        assert(!a.has(o));
    });

    assert(a.size, 0);
}

function test_map()
{
    var a, i, n, tab, o, v;
    n = 1000;

    a = new Map();
    for (var i = 0; i < n; i++) {
        a.set(i, i);
    }
    a.set(-2147483648, 1);
    assert(a.get(-2147483648), 1);
    assert(a.get(-2147483647 - 1), 1);
    assert(a.get(-2147483647.5 - 0.5), 1);

    a.set(1n, 1n);
    assert(a.get(1n), 1n);
    assert(a.get(2n**1000n - (2n**1000n - 1n)), 1n);

    test_map1("object", n);
    test_map1("small_bigint", n);
    test_map1("bigint", n);
}
function test_weak_map()
{
    var a, i, n, tab, o, v, n2;
    a = new WeakMap();
    n = 10;
    tab = [];
    for(i = 0; i < n; i++) {
        v = { };
        if (i & 1)
            o = Symbol("x" + i);
        else
            o = { id: i };
        tab[i] = [o, v];
        a.set(o, v);
    }
    o = null;

    n2 = 5;
    for(i = 0; i < n2; i++) {
        a.delete(tab[i][0]);
    }
    for(i = n2; i < n; i++) {
        tab[i][0] = null; /* should remove the object from the WeakMap too */
    }
    std.gc();
    /* the WeakMap should be empty here */
}
function test_weak_map_cycles()
{
    const weak1 = new WeakMap();
    const weak2 = new WeakMap();
    function createCyclicKey() {
        const parent = {};
        const child = {parent};
        parent.child = child;
        return child;
    }
    function testWeakMap() {
        const cyclicKey = createCyclicKey();
        const valueOfCyclicKey = {};
        weak1.set(cyclicKey, valueOfCyclicKey);
        weak2.set(valueOfCyclicKey, 1);
    }
    testWeakMap();
    // Force to free cyclicKey.
    std.gc();
    // Here will cause sigsegv because [cyclicKey] and [valueOfCyclicKey] in [weak1] was free,
    // but weak2's map record was not removed, and it's key refers [valueOfCyclicKey] which is free.
    weak2.get({});
    std.gc();
}
function test_weak_ref()
{
    var w1, w2, o, i;

    for(i = 0; i < 2; i++) {
        if (i == 0)
            o = { };
        else
            o = Symbol("x");
        w1 = new WeakRef(o);
        assert(w1.deref(), o);
        w2 = new WeakRef(o);
        assert(w2.deref(), o);
        
        o = null;
        assert(w1.deref() !== undefined);
        assert(w2.deref() !== undefined);
        std.gc();
        assert(w1.deref() !== undefined);
        assert(w2.deref() !== undefined);
        let refs = [w1, w2];
        os.setTimeout(() => {
            std.gc();
            assert(refs[0].deref(), undefined);
            assert(refs[1].deref(), undefined);
        }, 0);
    }
}
function test_finalization_registry()
{
    {
        let expected = {};
        let actual;
        let finrec = new FinalizationRegistry(v => { actual = v });
        finrec.register({}, expected);
        os.setTimeout(() => {
            assert(actual, expected);
        }, 0);
    }
    {
        let expected = 42;
        let actual;
        let finrec = new FinalizationRegistry(v => { actual = v });
        finrec.register({}, expected);
        os.setTimeout(() => {
            assert(actual, expected);
        }, 0);
    }
    std.gc();
}
test_typed_array();
test_map();
test_weak_map();
test_weak_map_cycles();
test_weak_ref();
test_finalization_registry();
