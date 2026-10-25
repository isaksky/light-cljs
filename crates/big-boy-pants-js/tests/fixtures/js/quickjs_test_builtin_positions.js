/*
 * Adapted QuickJS text; synthetic fixture, not light-cljs output.
 * Source: research/quickjs/tests/test_builtin.js, lines 1-71, 524-560,
 * 1092-1130 and 1138-1174, revision
 * 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2 (VERSION 2026-06-04).
 * Selected lines and assertions are byte-for-byte unchanged.
 * Intended omission: lines 1131-1137 exercise QuickJS-only function
 * lineNumber/columnNumber metadata. No selected assertion is adapted.
 * Only this descriptive/license header and a trailing case call are added.
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
/* return [s, line_num, col_num] where line_num and col_num are the
   position of the '@' character in 'str'. 's' is str without the '@'
   character */
function get_string_pos(str)
{
    var p, line_num, col_num, s, q, r;
    p = str.indexOf('@');
    assert(p >= 0, true);
    q = 0;
    line_num = 1;
    for(;;) {
        r = str.indexOf('\n', q);
        if (r < 0 || r >= p)
            break;
        q = r + 1;
        line_num++;
    }
    col_num = p - q + 1;
    s = str.slice(0, p) + str.slice(p + 1);
    return [s, line_num, col_num];
}

function check_error_pos(e, expected_error, line_num, col_num, level)
{
    var expected_pos, tab, line;
    level |= 0;
    expected_pos = ":" + line_num + ":" + col_num;
    tab = e.stack.split("\n");
    line = tab[level];
    if (line.slice(-1) == ')')
        line = line.slice(0, -1);
    if (line.indexOf(expected_pos) < 0) {
        throw_error("unexpected line or column number. error=" + e.message +
                    ".got |" + line + "|, expected |" + expected_pos + "|");
    }
}

function eval_error(eval_str, expected_error, level)
{
    var err = false;
    var expected_pos, tab;

    tab = get_string_pos(eval_str);
    
    try {
        eval(tab[0]);
    } catch(e) {
        err = true;
        if (!(e instanceof expected_error)) {
            throw_error("unexpected exception type");
            return;
        }
        check_error_pos(e, expected_error, tab[1], tab[2], level);
    }
    if (!err) {
        throw_error("expected exception");
    }
}

var poisoned_number = {
    valueOf: function() { throw Error("poisoned number") },
};

function test_line_column_numbers()
{
    var f, e, tab;

    /* The '@' character provides the expected position of the
       error. It is removed before evaluating the string. */
    
    /* parsing */
    eval_error("\n 123 @a ", SyntaxError);
    eval_error("\n  @/*  ", SyntaxError);
    eval_error("function f  @a", SyntaxError);
    /* currently regexp syntax errors point to the start of the regexp */
    eval_error("\n  @/aaa]/u", SyntaxError); 

    /* errors */
    tab = get_string_pos('\n  Error@("hello");');
    e = eval(tab[0]);
    check_error_pos(e, Error, tab[1], tab[2]);
    
    eval_error('\n  throw Error@("hello");', Error);

    /* operators */
    eval_error('\n  1 + 2 @* poisoned_number;', Error, 1);
    eval_error('\n  1 + "café" @* poisoned_number;', Error, 1);
    eval_error('\n  1 + 2 @** poisoned_number;', Error, 1);
    eval_error('\n  2 * @+ poisoned_number;', Error, 1);
    eval_error('\n  2 * @- poisoned_number;', Error, 1);
    eval_error('\n  2 * @~ poisoned_number;', Error, 1);
    eval_error('\n  2 * @++ poisoned_number;', Error, 1);
    eval_error('\n  2 * @-- poisoned_number;', Error, 1);
    eval_error('\n  2 * poisoned_number @++;', Error, 1);
    eval_error('\n  2 * poisoned_number @--;', Error, 1);

    /* accessors */
    eval_error('\n 1 + null@[0];', TypeError); 
    eval_error('\n 1 + null @. abcd;', TypeError); 
    eval_error('\n 1 + null @( 1234 );', TypeError);
    eval_error('var obj = { get a() { throw Error("test"); } }\n 1 + obj @. a;',
               Error, 1);
    eval_error('var obj = { set a(b) { throw Error("test"); } }\n obj @. a = 1;',
               Error, 1);

    /* variables reference */
    eval_error('\n  1 + @not_def', ReferenceError, 0);

    /* assignments */
    eval_error('1 + (@not_def = 1)', ReferenceError, 0);
    eval_error('1 + (@not_def += 2)', ReferenceError, 0);
    eval_error('var a;\n 1 + (a @+= poisoned_number);', Error, 1);
}
test_line_column_numbers();
