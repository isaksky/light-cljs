// Adapted from research/quickjs/tests/test_closure.js, QuickJS revision
// 7d1c13c5d8f8 (VERSION 2026-06-04): the top-level `f`/`g1` case
// ("closure1"), test_closure1, test_closure2 and test_closure3.
// Modifications: rewritten for the documented subset (upstream's `var`
// declarations are kept since clj-quickjs-94, but script-level `var`s are
// script bindings, not global object properties; the added `failed` is a
// `let`; `log` appends each logged value to a number as two decimal
// digits instead of concatenating strings; test_closure1's `{ set, get }`
// object is a vector built with the reserved intrinsic `__rt.vector` and
// read with `__rt.nth`; no assert() function: a failing check records its
// number in `failed`, and the script completes with `failed`, so 0 means
// every check passed). test_arrow_function, test_with, test_eval_closure and
// test_eval_const are omitted: arrow functions, `this`, `with`, `eval`,
// objects and exceptions are outside the subset.
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

let failed = 0;
var log_str = 0;

function log(v)
{
    log_str = log_str * 100 + v;
}

function f(a, b, c)
{
    var x = 10;
    log(a);
    function g(d) {
        function h() {
            log(d);
            log(x);
        }
        log(b);
        log(c);
        h();
    }
    g(4);
    return g;
}

var g1 = f(1, 2, 3);
if (log_str !== 102030410) { failed = failed || 1; }   // a=1,b=2,c=3,d=4,x=10
log_str = 0;
g1(5);
if (log_str !== 2030510) { failed = failed || 2; }     // b=2,c=3,d=5,x=10

function test_closure1()
{
    function f2()
    {
        var val = 1;

        function set(a) {
            val = a;
        }
        function get(a) {
            return val;
        }
        return __rt.vector(set, get);
    }

    var obj = f2();
    __rt.nth(obj, 0)(10);
    var r;
    r = __rt.nth(obj, 1)();
    if (r !== 10) { failed = failed || 3; }
}

function test_closure2()
{
    var expr_func = function myfunc1(n) {
        function myfunc2(n) {
            return myfunc1(n - 1);
        }
        if (n == 0)
            return 0;
        else
            return myfunc2(n);
    };
    var r;
    r = expr_func(1);
    if (r !== 0) { failed = failed || 4; }
}

function test_closure3()
{
    function fib(n)
    {
        if (n <= 0)
            return 0;
        else if (n == 1)
            return 1;
        else
            return fib(n - 1) + fib(n - 2);
    }

    var fib_func = function fib1(n)
    {
        if (n <= 0)
            return 0;
        else if (n == 1)
            return 1;
        else
            return fib1(n - 1) + fib1(n - 2);
    };

    if (fib(6) !== 8) { failed = failed || 5; }
    if (fib_func(6) !== 8) { failed = failed || 6; }
}

test_closure1();
test_closure2();
test_closure3();

failed
