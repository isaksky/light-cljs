// Adapted from research/quickjs/tests/test_loop.js (the assert function and
// test_try_catch1 through test_try_catch8, called once each in upstream
// order), QuickJS revision 7d1c13c5d8f8 (VERSION 2026-06-04). Modifications,
// each for a form the documented subset lacks yet:
// - `let` instead of `var` (until clj-quickjs-94);
// - `s = s + x` instead of `s += x` (until clj-quickjs-84);
// - assert() tests `expected === undefined` instead of
//   `arguments.length == 1` (until clj-quickjs-97), throws its message
//   string instead of `Error(...)` (until clj-quickjs-228), and drops the
//   `typeof ... == 'object'` / `toString()` comparison, which no case here
//   reaches (until clj-quickjs-69 and clj-quickjs-70);
// - test_try_catch8 iterates "x" and "y" with a counted `for` instead of
//   `for (var i in {x:1, y:2})` (until clj-quickjs-85, clj-quickjs-89 and
//   clj-quickjs-114);
// - the script completes with "ok" after the last case.
//
// QuickJS Javascript Engine
//
// Copyright (c) 2017-2021 Fabrice Bellard
// Copyright (c) 2017-2021 Charlie Gordon
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

function assert(actual, expected, message) {
    if (expected === undefined)
        expected = true;

    if (actual === expected)
        return;

    throw "assertion failed: got |" + actual + "|" +
          ", expected |" + expected + "|" +
          (message ? " (" + message + ")" : "");
}

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
    let a;
    try {
        a = 1;
    } catch (e) {
        a = 2;
    }
    assert(a, 1, "catch");
}

function test_try_catch3()
{
    let s;
    s = "";
    try {
        s = s + "t";
    } catch (e) {
        s = s + "c";
    } finally {
        s = s + "f";
    }
    assert(s, "tf", "catch");
}

function test_try_catch4()
{
    let s;
    s = "";
    try {
        s = s + "t";
        throw "c";
    } catch (e) {
        s = s + e;
    } finally {
        s = s + "f";
    }
    assert(s, "tcf", "catch");
}

function test_try_catch5()
{
    let s;
    s = "";
    for(;;) {
        try {
            s = s + "t";
            break;
            s = s + "b";
        } finally {
            s = s + "f";
        }
    }
    assert(s, "tf", "catch");
}

function test_try_catch6()
{
    function f() {
        try {
            s = s + 't';
            return 1;
        } finally {
            s = s + "f";
        }
    }
    let s = "";
    assert(f() === 1);
    assert(s, "tf", "catch6");
}

function test_try_catch7()
{
    let s;
    s = "";

    try {
        try {
            s = s + "t";
            throw "a";
        } finally {
            s = s + "f";
        }
    } catch(e) {
        s = s + e;
    } finally {
        s = s + "g";
    }
    assert(s, "tfag", "catch");
}

function test_try_catch8()
{
    let s;

    s = "";
    for (let n = 0; n < 2; n++) {
        let i = n === 0 ? "x" : "y";
        try {
            s = s + i;
            throw "a";
        } catch (e) {
            s = s + e;
        } finally {
            s = s + "f";
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

"ok"
