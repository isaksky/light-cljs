/*
 * Adapted QuickJS test text; synthetic fixture, not light-cljs output.
 * Source: research/quickjs/tests/test_loop.js, lines 1-20, 254-372 and
 * 395-402, revision 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2
 * (VERSION 2026-06-04).
 * The assert helper, __loadScript attempt, eight complete test_try_catch
 * functions and their calls are byte-for-byte unchanged. Adapted rows: none.
 * Only this descriptive/license header and separators between excerpts
 * are added. The Rust harness compiles this as a non-strict script.
 *
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
function assert(actual, expected, message) {
    if (arguments.length == 1)
        expected = true;

    if (actual === expected)
        return;

    if (actual !== null && expected !== null
    &&  typeof actual == 'object' && typeof expected == 'object'
    &&  actual.toString() === expected.toString())
        return;

    throw Error("assertion failed: got |" + actual + "|" +
                ", expected |" + expected + "|" +
                (message ? " (" + message + ")" : ""));
}

// load more elaborate version of assert if available
try { __loadScript("test_assert.js"); } catch(e) {}


function test_try_catch1()
{
    try {
        throw "hello";
    } catch (e) {
        assert(e, "hello", "catch");
        return;
    }
    assert(false, "catch");
}

function test_try_catch2()
{
    var a;
    try {
        a = 1;
    } catch (e) {
        a = 2;
    }
    assert(a, 1, "catch");
}

function test_try_catch3()
{
    var s;
    s = "";
    try {
        s += "t";
    } catch (e) {
        s += "c";
    } finally {
        s += "f";
    }
    assert(s, "tf", "catch");
}

function test_try_catch4()
{
    var s;
    s = "";
    try {
        s += "t";
        throw "c";
    } catch (e) {
        s += e;
    } finally {
        s += "f";
    }
    assert(s, "tcf", "catch");
}

function test_try_catch5()
{
    var s;
    s = "";
    for(;;) {
        try {
            s += "t";
            break;
            s += "b";
        } finally {
            s += "f";
        }
    }
    assert(s, "tf", "catch");
}

function test_try_catch6()
{
    function f() {
        try {
            s += 't';
            return 1;
        } finally {
            s += "f";
        }
    }
    var s = "";
    assert(f() === 1);
    assert(s, "tf", "catch6");
}

function test_try_catch7()
{
    var s;
    s = "";

    try {
        try {
            s += "t";
            throw "a";
        } finally {
            s += "f";
        }
    } catch(e) {
        s += e;
    } finally {
        s += "g";
    }
    assert(s, "tfag", "catch");
}

function test_try_catch8()
{
    var i, s;

    s = "";
    for(var i in {x:1, y:2}) {
        try {
            s += i;
            throw "a";
        } catch (e) {
            s += e;
        } finally {
            s += "f";
        }
    }
    assert(s === "xafyaf");
}

test_try_catch1();
test_try_catch2();
test_try_catch3();
test_try_catch4();
test_try_catch5();
test_try_catch6();
test_try_catch7();
test_try_catch8();
