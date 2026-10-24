// Synthetic fixture, not light-cljs output. The two lastIndex rows adapt
// QuickJS tests/test_builtin.js::test_regexp, revision 7d1c13c5d8f8.
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
function check(v) { if (!v) throw new Error('Unicode RegExp assertion'); }
let a = /(?:)/gu;
a.lastIndex = 1;
a.exec("🐱");
check(a.lastIndex === 0);
a.lastIndex = 1;
a.exec("a\udc00");
check(a.lastIndex === 1);
check(/^.$/u.test("🐱") === true);
check(/^.$/.test("🐱") === false);
check(/^[🐱]$/u.test("🐱") === true);
check(/\u{1F431}/u.exec("x🐱")[0] === "🐱");
check(/\udc00/u.test("\ud800\udc00") === false);
check(/\udc00/.test("\ud800\udc00") === true);
check(/[\u{10000}-\u{10FFFF}]/u.test("\u{10400}") === true);
check("🐱".replace(/(?:)/gu, "-") === "-🐱-");
check("🐱".replace(/(?:)/g, "-").length === 5);

// Synthetic coverage for UTF-16 offsets, quantification, backtracking,
// lookahead, backreferences, inverted classes and lone surrogate boundaries.
check(/^🐱{2}$/u.test("🐱🐱") && !/^🐱{2}$/u.test("🐱"));
check(/^(🐱+)(🐱)$/u.exec("🐱🐱🐱")[1] === "🐱🐱");
check(/^(.)\1$/u.test("🐱🐱"));
check(/^(?=(🐱))\1$/u.test("🐱"));
check(/^[^a]$/u.test("🐱") && /^\D$/u.test("🐱"));
check(!/\ud800/u.test("\ud800\udc00"));
check(/^\ud800$/u.test("\ud800") && /^\udc00$/u.test("\udc00"));
check(!/^(\ud800)\1\udc00$/u.test("\ud800\ud800\udc00"));
check(!/^(\udc00)\ud800\1$/u.test("\udc00\ud800\udc00"));
check(/\udc00/u.exec("🐱\udc00").index === 2);
check(/\ud800/u.exec("🐱\ud800").index === 2);
a = /./uy; a.lastIndex = 1;
check(a.exec("🐱")[0] === "🐱" && a.lastIndex === 2);
a.lastIndex = 3;
check(a.exec("🐱") === null && a.lastIndex === 0);
a = /(.)/gu;
check(a.exec("x🐱").index === 0 && a.lastIndex === 1);
let m = a.exec("x🐱");
check(m.index === 1 && m[1] === "🐱" && a.lastIndex === 3);
check("🐱".replaceAll(/(?:)/gu, "-") === "-🐱-");
check("\ud800x\udc00".replace(/(?:)/gu, "-") === "-\ud800-x-\udc00-");
let offsets = [];
check("🐱".replace(/(?:)/gu, (s, i) => {offsets.push(i); return '-';}) === "-🐱-");
check(offsets.length === 2 && offsets[0] === 0 && offsets[1] === 2);
check(new RegExp('\\u{1F431}', 'u').test("🐱"));
check(/a/gu.unicode && /a/gu.flags === 'gu');
true;
