// Synthetic fixture, not light-cljs output.
// Replacement rows adapted from QuickJS tests/test_builtin.js (7d1c13c5d8f8).
// Copyright (c) 2017-2021 Fabrice Bellard
// Copyright (c) 2017-2021 Charlie Gordon
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.
function check(v) { if (!v) throw new Error('RegExp case folding assertion'); }
check("aAbBcC".replace(/[^b]/gui, "X") === "XXbBXX");
check("aAbBcC".replace(/[^A-B]/gui, "X") === "aAbBXX");
check(/k/iu.test("\u212A"));
check(/[a-z]/iu.test("\u017F"));
check(/\w/iu.test("\u017F"));
check(!/[^a]/iu.test("A"));
check(!/\W/iu.test("\u212A"));
check(/[^\W]/iu.test("\u017F"));
check(/[\W]/iu.test("!"));
check(!/[\W]/iu.test("\u017F"));
check(/\bſ\b/iu.test("s"));
check(!/\Bſ/iu.test("s"));
check(/Σ/iu.test("ς") && /ς/iu.test("σ"));
check(/ẞ/iu.test("ß") && !/ß/iu.test("ss"));
check(!/İ/iu.test("i"));
check(/[A-Z]/iu.test("k") && /[\u212A-\u212A]/iu.test("K"));
check(/(k)\1/iu.test("k\u212A"));
check(/(\u{10400})\1/iu.test("\u{10400}\u{10428}"));
check(/\uD800/iu.test("\uD800"));
check(/(?i:a)(?-i:b)/u.test("Ab") && !/(?i:a)(?-i:b)/u.test("AB"));
check(/(?-i:a)b/iu.test("aB") && !/(?-i:a)b/iu.test("AB"));
let caught = false;
try { /a/i; } catch(e) { caught = e instanceof SyntaxError && e.message.indexOf("'i'") >= 0; }
check(caught);
check(/a/iu.ignoreCase && /a/iu.unicode && /a/iu.flags === 'iu');
true;
