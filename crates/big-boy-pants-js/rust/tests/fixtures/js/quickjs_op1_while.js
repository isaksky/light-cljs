// Adapted from research/quickjs/tests/test_language.js (test_op1) and
// research/quickjs/tests/test_loop.js (test_while), QuickJS revision
// 7d1c13c5d8f8 (VERSION 2026-06-04). Modifications: rewritten for the
// documented subset (`let` instead of `var`, `x = x + 1` instead of `++`,
// no assert() function: a failing check records its number in `failed`,
// and the script completes with `failed`, so 0 means every check passed).
// The test_op1 cases using bitwise operators, `**` and strings are omitted.
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
let r;

// test_op1
r = 1 + 2;
if (r !== 3) { failed = failed || 1; }
r = 1 - 2;
if (r !== -1) { failed = failed || 2; }
r = -1;
if (r !== -1) { failed = failed || 3; }
r = +2;
if (r !== 2) { failed = failed || 4; }
r = 2 * 3;
if (r !== 6) { failed = failed || 5; }
r = 4 / 2;
if (r !== 2) { failed = failed || 6; }
r = 4 % 3;
if (r !== 1) { failed = failed || 7; }
r = !1;
if (r !== false) { failed = failed || 8; }
if ((1 < 2) !== true) { failed = failed || 9; }
if ((2 > 1) !== true) { failed = failed || 10; }

// test_while
{
  let i, c;
  i = 0;
  c = 0;
  while (i < 3) {
    c = c + 1;
    i = i + 1;
  }
  if (!(c === 3)) { failed = failed || 11; }
}

failed
