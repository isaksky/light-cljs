// Adapted QuickJS test text; synthetic fixture, not light-cljs output.
// Source: research/quickjs/tests/test_builtin.js at
// 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2 (VERSION 2026-06-04).
// Lines 1-71, 612-713 and 715-811 are copied byte for byte below.
// Fixture changes: prepend this notice; append one call per case.
// Upstream lines below are unchanged. The Rust test runs an adapted RegExp
// variant: line 744, `a = eval("/\0a/");`, becomes `a = new RegExp("\0a");`.
// Expected-failure list:
// - test_regexp, upstream line 744: direct eval is rejected during compilation
//   (clj-quickjs-107). The test asserts this rejection before running the
//   constructor variant. No assertions or expected values are changed.
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
function test_date()
{
    // Date Time String format is YYYY-MM-DDTHH:mm:ss.sssZ
    // accepted date formats are: YYYY, YYYY-MM and YYYY-MM-DD
    // accepted time formats are: THH:mm, THH:mm:ss, THH:mm:ss.sss
    // expanded years are represented with 6 digits prefixed by + or -
    // -000000 is invalid.
    // A string containing out-of-bounds or nonconforming elements
    //   is not a valid instance of this format.
    // Hence the fractional part after . should have 3 digits and how
    // a different number of digits is handled is implementation defined.
    assert(Date.parse(""), NaN);
    assert(Date.parse("2000"), 946684800000);
    assert(Date.parse("2000-01"), 946684800000);
    assert(Date.parse("2000-01-01"), 946684800000);
    //assert(Date.parse("2000-01-01T"), NaN);
    //assert(Date.parse("2000-01-01T00Z"), NaN);
    assert(Date.parse("2000-01-01T00:00Z"), 946684800000);
    assert(Date.parse("2000-01-01T00:00:00Z"), 946684800000);
    assert(Date.parse("2000-01-01T00:00:00.1Z"), 946684800100);
    assert(Date.parse("2000-01-01T00:00:00.10Z"), 946684800100);
    assert(Date.parse("2000-01-01T00:00:00.100Z"), 946684800100);
    assert(Date.parse("2000-01-01T00:00:00.1000Z"), 946684800100);
    assert(Date.parse("2000-01-01T00:00:00+00:00"), 946684800000);
    //assert(Date.parse("2000-01-01T00:00:00+00:30"), 946686600000);
    var d = new Date("2000T00:00");  // Jan 1st 2000, 0:00:00 local time
    assert(typeof d === 'object' && d.toString() != 'Invalid Date');
    assert((new Date('Jan 1 2000')).toISOString(),
           d.toISOString());
    assert((new Date('Jan 1 2000 00:00')).toISOString(),
           d.toISOString());
    assert((new Date('Jan 1 2000 00:00:00')).toISOString(),
           d.toISOString());
    assert((new Date('Jan 1 2000 00:00:00 GMT+0100')).toISOString(),
           '1999-12-31T23:00:00.000Z');
    assert((new Date('Jan 1 2000 00:00:00 GMT+0200')).toISOString(),
           '1999-12-31T22:00:00.000Z');
    assert((new Date('Sat Jan 1 2000')).toISOString(),
           d.toISOString());
    assert((new Date('Sat Jan 1 2000 00:00')).toISOString(),
           d.toISOString());
    assert((new Date('Sat Jan 1 2000 00:00:00')).toISOString(),
           d.toISOString());
    assert((new Date('Sat Jan 1 2000 00:00:00 GMT+0100')).toISOString(),
           '1999-12-31T23:00:00.000Z');
    assert((new Date('Sat Jan 1 2000 00:00:00 GMT+0200')).toISOString(),
           '1999-12-31T22:00:00.000Z');

    var d = new Date(1506098258091);
    assert(d.toISOString(), "2017-09-22T16:37:38.091Z");
    d.setUTCHours(18, 10, 11);
    assert(d.toISOString(), "2017-09-22T18:10:11.091Z");
    var a = Date.parse(d.toISOString());
    assert((new Date(a)).toISOString(), d.toISOString());

    assert((new Date("2020-01-01T01:01:01.123Z")).toISOString(),
                     "2020-01-01T01:01:01.123Z");
    /* implementation defined behavior */
    assert((new Date("2020-01-01T01:01:01.1Z")).toISOString(),
                     "2020-01-01T01:01:01.100Z");
    assert((new Date("2020-01-01T01:01:01.12Z")).toISOString(),
                     "2020-01-01T01:01:01.120Z");
    assert((new Date("2020-01-01T01:01:01.1234Z")).toISOString(),
                     "2020-01-01T01:01:01.123Z");
    assert((new Date("2020-01-01T01:01:01.12345Z")).toISOString(),
                     "2020-01-01T01:01:01.123Z");
    assert((new Date("2020-01-01T01:01:01.1235Z")).toISOString(),
                     "2020-01-01T01:01:01.123Z");
    assert((new Date("2020-01-01T01:01:01.9999Z")).toISOString(),
                     "2020-01-01T01:01:01.999Z");

    assert(Date.UTC(2017), 1483228800000);
    assert(Date.UTC(2017, 9), 1506816000000);
    assert(Date.UTC(2017, 9, 22), 1508630400000);
    assert(Date.UTC(2017, 9, 22, 18), 1508695200000);
    assert(Date.UTC(2017, 9, 22, 18, 10), 1508695800000);
    assert(Date.UTC(2017, 9, 22, 18, 10, 11), 1508695811000);
    assert(Date.UTC(2017, 9, 22, 18, 10, 11, 91), 1508695811091);

    assert(Date.UTC(NaN), NaN);
    assert(Date.UTC(2017, NaN), NaN);
    assert(Date.UTC(2017, 9, NaN), NaN);
    assert(Date.UTC(2017, 9, 22, NaN), NaN);
    assert(Date.UTC(2017, 9, 22, 18, NaN), NaN);
    assert(Date.UTC(2017, 9, 22, 18, 10, NaN), NaN);
    assert(Date.UTC(2017, 9, 22, 18, 10, 11, NaN), NaN);
    assert(Date.UTC(2017, 9, 22, 18, 10, 11, 91, NaN), 1508695811091);

    // TODO: Fix rounding errors on Windows/Cygwin.
    if (!(typeof os !== 'undefined' && ['win32', 'cygwin'].includes(os.platform))) {
        // from test262/test/built-ins/Date/UTC/fp-evaluation-order.js
        assert(Date.UTC(1970, 0, 1, 80063993375, 29, 1, -288230376151711740), 29312,
               'order of operations / precision in MakeTime');
        assert(Date.UTC(1970, 0, 213503982336, 0, 0, 0, -18446744073709552000), 34447360,
               'precision in MakeDate');
    }
    //assert(Date.UTC(2017 - 1e9, 9 + 12e9), 1506816000000);  // node fails this
    assert(Date.UTC(2017, 9, 22 - 1e10, 18 + 24e10), 1508695200000);
    assert(Date.UTC(2017, 9, 22, 18 - 1e10, 10 + 60e10), 1508695800000);
    assert(Date.UTC(2017, 9, 22, 18, 10 - 1e10, 11 + 60e10), 1508695811000);
    assert(Date.UTC(2017, 9, 22, 18, 10, 11 - 1e12, 91 + 1000e12), 1508695811091);
}
function test_regexp()
{
    var a, str;
    str = "abbbbbc";
    a = /(b+)c/.exec(str);
    assert(a[0], "bbbbbc");
    assert(a[1], "bbbbb");
    assert(a.index, 1);
    assert(a.input, str);
    a = /(b+)c/.test(str);
    assert(a, true);
    assert(/\x61/.exec("a")[0], "a");
    assert(/\u0061/.exec("a")[0], "a");
    assert(/\ca/.exec("\x01")[0], "\x01");
    assert(/\\a/.exec("\\a")[0], "\\a");
    assert(/\c0/.exec("\\c0")[0], "\\c0");

    a = /(\.(?=com|org)|\/)/.exec("ah.com");
    assert(a.index === 2 && a[0] === ".");

    a = /(\.(?!com|org)|\/)/.exec("ah.com");
    assert(a, null);

    a = /(?=(a+))/.exec("baaabac");
    assert(a.index === 1 && a[0] === "" && a[1] === "aaa");

    a = /(z)((a+)?(b+)?(c))*/.exec("zaacbbbcac");
    assert(a, ["zaacbbbcac","z","ac","a",,"c"]);

    a = eval("/\0a/");
    assert(a.toString(), "/\0a/");
    assert(a.exec("\0a")[0], "\0a");

    assert(/{1a}/.toString(), "/{1a}/");
    a = /a{1+/.exec("a{11");
    assert(a, ["a{11"]);

    /* test zero length matches */
    a = /(?:(?=(abc)))a/.exec("abc");
    assert(a, ["a", "abc"]);
    a = /(?:(?=(abc)))?a/.exec("abc");
    assert(a, ["a", undefined]);
    a = /(?:(?=(abc))){0,2}a/.exec("abc");
    assert(a, ["a", undefined]);
    a = /(?:|[\w])+([0-9])/.exec("123a23");
    assert(a, ["123a23", "3"]);
    a = /()*?a/.exec(",");
    assert(a, null);

    /* test \b escape */
    assert(/[\q{a\b}]/.test("a\b"), true);
    assert(/[\b]/.test("\b"), true);
    
    /* test case insensitive matching (test262 hardly tests it) */
    assert("aAbBcC#4".replace(/\p{Lower}/gu,"X"), "XAXBXC#4");

    assert("aAbBcC#4".replace(/\p{Lower}/gui,"X"), "XXXXXX#4");
    assert("aAbBcC#4".replace(/\p{Upper}/gui,"X"), "XXXXXX#4");
    assert("aAbBcC#4".replace(/\P{Lower}/gui,"X"), "XXXXXXXX");
    assert("aAbBcC#4".replace(/\P{Upper}/gui,"X"), "XXXXXXXX");
    assert("aAbBcC".replace(/[^b]/gui, "X"), "XXbBXX");
    assert("aAbBcC".replace(/[^A-B]/gui, "X"), "aAbBXX");

    assert("aAbBcC#4".replace(/\p{Lower}/gvi,"X"), "XXXXXX#4");
    assert("aAbBcC#4".replace(/\P{Lower}/gvi,"X"), "aAbBcCXX");
    assert("aAbBcC#4".replace(/[^\P{Lower}]/gvi,"X"), "XXXXXX#4");
    assert("aAbBcC#4".replace(/\P{Upper}/gvi,"X"), "aAbBcCXX");
    assert("aAbBcC".replace(/[^b]/gvi, "X"), "XXbBXX");
    assert("aAbBcC".replace(/[^A-B]/gvi, "X"), "aAbBXX");
    assert("aAbBcC".replace(/[[a-c]&&B]/gvi, "X"), "aAXXcC");
    assert("aAbBcC".replace(/[[a-c]--B]/gvi, "X"), "XXbBXX");
    
    assert("abcAbC".replace(/[\q{AbC}]/gvi,"X"), "XX");
    /* Note: SpiderMonkey and v8 may not be correct */
    assert("abcAbC".replace(/[\q{BC|A}]/gvi,"X"), "XXXX");
    assert("abcAbC".replace(/[\q{BC|A}--a]/gvi,"X"), "aXAX");

    /* case where lastIndex points to the second element of a
       surrogate pair */
    a = /(?:)/gu;
    a.lastIndex = 1;
    a.exec("🐱");
    assert(a.lastIndex, 0);

    a.lastIndex = 1;
    a.exec("a\udc00");
    assert(a.lastIndex, 1);

    a = /\u{10000}/vgd;
    a.lastIndex = 1;
    a = a.exec("\u{10000}_\u{10000}");
    assert(a.indices[0][0], 0);
    assert(a.indices[0][1], 2);

    /* syntax check */
    assert(/-&&/v.test("-&&"), true);
}

test_date();
test_regexp();
