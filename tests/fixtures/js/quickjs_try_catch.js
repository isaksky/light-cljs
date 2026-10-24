// Adapted from research/quickjs/tests/test_loop.js (test_try_catch1,
// test_try_catch2, test_try_catch3, test_try_catch4, test_try_catch5,
// test_try_catch6 and test_try_catch7), QuickJS revision 7d1c13c5d8f8
// (VERSION 2026-06-04). Modifications: rewritten for the documented subset
// (`let` instead of `var`, `while (true)` instead of `for(;;)`; strings are
// not values yet, so each logged letter is a digit appended with
// `s = s * 10 + d`: "t" = 1, "c" = 2, "f" = 3, "a" = 4, "g" = 5, "b" = 6,
// and the thrown strings are those digits); no assert() function: a failing
// check records its number in `failed`, and the script completes with
// `failed`, so 0 means every check passed. test_try_catch8 (`for`-`in`,
// object literals) is omitted.
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

// test_try_catch1
function test_try_catch1() {
  try {
    throw 7;
  } catch (e) {
    if (e !== 7) { failed = failed || 1; }
    return;
  }
  failed = failed || 2;
}
test_try_catch1();

// test_try_catch2
{
  let a;
  try {
    a = 1;
  } catch (e) {
    a = 2;
  }
  if (a !== 1) { failed = failed || 3; }
}

// test_try_catch3: "tf"
{
  let s = 0;
  try {
    s = s * 10 + 1;
  } catch (e) {
    s = s * 10 + 2;
  } finally {
    s = s * 10 + 3;
  }
  if (s !== 13) { failed = failed || 4; }
}

// test_try_catch4: "tcf"
{
  let s = 0;
  try {
    s = s * 10 + 1;
    throw 2;
  } catch (e) {
    s = s * 10 + e;
  } finally {
    s = s * 10 + 3;
  }
  if (s !== 123) { failed = failed || 5; }
}

// test_try_catch6: "tf", and f() returns 1 through the finally block
function test_try_catch6() {
  function f() {
    try {
      s = s * 10 + 1;
      return 1;
    } finally {
      s = s * 10 + 3;
    }
  }
  let s = 0;
  if (!(f() === 1)) { failed = failed || 6; }
  if (s !== 13) { failed = failed || 7; }
}
test_try_catch6();

// test_try_catch5: "tf" (`while (true)` for `for(;;)`; "b" = 6)
{
  let s = 0;
  while (true) {
    try {
      s = s * 10 + 1;
      break;
      s = s * 10 + 6;
    } finally {
      s = s * 10 + 3;
    }
  }
  if (s !== 13) { failed = failed || 9; }
}

// test_try_catch7: "tfag"
{
  let s = 0;
  try {
    try {
      s = s * 10 + 1;
      throw 4;
    } finally {
      s = s * 10 + 3;
    }
  } catch (e) {
    s = s * 10 + e;
  } finally {
    s = s * 10 + 5;
  }
  if (s !== 1345) { failed = failed || 8; }
}

failed
