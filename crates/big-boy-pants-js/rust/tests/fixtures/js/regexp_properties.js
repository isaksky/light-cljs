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
function check(v) { if (!v) throw new Error('RegExp property escape assertion'); }
check("aAbBcC#4".replace(/\p{Lower}/gu, "X") === "XAXBXC#4");
check("aAbBcC#4".replace(/\p{Lower}/gui, "X") === "XXXXXX#4");
check("aAbBcC#4".replace(/\p{Upper}/gui, "X") === "XXXXXX#4");
check("aAbBcC#4".replace(/\P{Lower}/gui, "X") === "XXXXXXXX");
check("aAbBcC#4".replace(/\P{Upper}/gui, "X") === "XXXXXXXX");
check(/\p{Lu}/u.test("A") && !/\p{Lu}/u.test("a"));
check(/\p{gc=Lu}/u.test("A") && !/\p{gc=Lu}/u.test("a"));
check(/\p{General_Category=Uppercase_Letter}/u.test("A"));
check(!/\p{General_Category=Uppercase_Letter}/u.test("a"));
check(/\p{Lowercase}/u.test("\u{1D41A}"));
check(/[\p{Nd}x]/u.test("x") && /[\p{Nd}x]/u.test("\u{1D7CE}"));
check(/\p{L}/.test("p{L}") && /[\p{L}]/.test("p"));
check(/\P{L}/.test("P{L}"));
check(/\P{Lowercase}/iu.test("a") && /\P{Lowercase}/iu.test("A"));
check(!/[^\p{Lowercase}]/iu.test("A"));
check(!/[^\P{Lowercase}]/iu.test("a"));
check(/\p{Upper}/iu.test("\u{10428}") && /\p{Lower}/iu.test("\u{10400}"));
check(/\p{Cs}/u.test("\uD800") && !/\p{Cs}/u.test("\u{10400}"));
check(/\p{Any}/u.test("\uDFFF") && /\p{ASCII}/u.test("\x00"));
check(!/\P{Any}/iu.test("a") && /\P{ASCII}/iu.test("k"));
check(!/\p{ASCII}/u.test("K") && /\p{ASCII}/iu.test("K"));
check(/\p{Emoji}/u.test("😀") && /\p{AHex}/u.test("F"));
check(/\p{space}/u.test("\u0085") && !/\s/u.test("\u0085"));
check(/\p{Assigned}/u.test("\uD800") && /\p{Cn}/u.test("\u0378"));
check(/(?i:\p{Lu})(?-i:\p{Lu})/u.test("aA"));
check(!/(?i:\p{Lu})(?-i:\p{Lu})/u.test("aa"));
let exhausted = false;
let longInput = '';
for (let i = 0; i < 25; i++) longInput += 'a';
try { /(\p{L}+)+b/u.test(longInput); }
catch(e) { exhausted = e instanceof RangeError && e.message.indexOf('step limit') >= 0; }
check(exhausted);
let invalidNames = ['lowercase', 'Foo', 'gc=Lowercase', 'General_Category=lu',
                  'Greek', 'Lowercase=Yes', 'Lower case', '', 'gc=', '=Lu',
                  'gc=Lu=Ll', 'Hyphen', 'InCB', 'Basic_Emoji'];
for (let i = 0; i < invalidNames.length; i++) {
    let caught = false;
    try { RegExp('\\p{' + invalidNames[i] + '}', 'u'); }
    catch(e) { caught = e instanceof SyntaxError; }
    check(caught);
}
let invalidPatterns = ['\\p', '\\pLu', '\\p{Lu', '[\\p{L}-a]', '[a-\\P{L}]'];
for (let i = 0; i < invalidPatterns.length; i++) {
    let caught = false;
    try { RegExp(invalidPatterns[i], 'u'); }
    catch(e) { caught = e instanceof SyntaxError; }
    check(caught);
}
true;
