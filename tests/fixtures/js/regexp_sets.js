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
function check(v) { if (!v) throw new Error('RegExp unicode sets assertion'); }
check("aAbBcC#4".replace(/\p{Lower}/gvi,"X") === "XXXXXX#4");
check("aAbBcC#4".replace(/\P{Lower}/gvi,"X") === "aAbBcCXX");
check("aAbBcC#4".replace(/[^\P{Lower}]/gvi,"X") === "XXXXXX#4");
check("aAbBcC#4".replace(/\P{Upper}/gvi,"X") === "aAbBcCXX");
check("aAbBcC".replace(/[^b]/gvi,"X") === "XXbBXX");
check("aAbBcC".replace(/[^A-B]/gvi,"X") === "aAbBXX");
check("aAbBcC".replace(/[[a-c]&&B]/gvi,"X") === "aAXXcC");
check("aAbBcC".replace(/[[a-c]--B]/gvi,"X") === "XXbBXX");
check("abcAbC".replace(/[\q{AbC}]/gvi,"X") === "XX");
check("abcAbC".replace(/[\q{BC|A}]/gvi,"X") === "XXXX");
check("abcAbC".replace(/[\q{BC|A}--a]/gvi,"X") === "aXAX");
let indexed = /\u{10000}/vgd;
indexed.lastIndex = 1;
let m = indexed.exec("\u{10000}_\u{10000}");
check(m.indices[0][0] === 0 && m.indices[0][1] === 2);
check(/-&&/v.test('-&&'));
check(/[\q{abc|ab|a}]/v.exec('abcd')[0] === 'abc');
check(!/[[a-z]--[aeiou]]/v.test('e'));
check(!/[\p{L}&&\p{Lu}]/v.test('a'));
check(/[\q{abc|ab|a}]c/v.exec('abc')[0] === 'abc');
check(/[\q{ab|a}]b/v.exec('ab')[0] === 'ab');
check(/[\q{a|}]/v.exec('b')[0] === '');
check(/[\q{ab|a|}]/v.exec('ab')[0] === 'ab');
check(/^([\q{}]){2}$/v.exec('')[1] === '');
check(/^([\q{a|}])*$/v.exec('a')[1] === 'a');
check('😀'.replace(/[\q{}]/gv, '-') === '-😀-');
check(/[\q{ab|abc}&&\q{abc|x}]/v.exec('abc')[0] === 'abc');
check(/[\q{ab|abc}--\q{abc}]/v.exec('abc')[0] === 'ab');
check(/[\q{A|ab}--a]/vi.exec('Ab')[0] === 'Ab');
check(/[a-z]/vi.test('K') && /[[a-z]--k]/vi.test('ſ'));
check(!/[[a-z]--k]/vi.test('K'));
check(!/[\W&&\q{K|ſ}]/vi.test('K'));
check(/[\q{😀a|😀}]/v.exec('😀ab')[0] === '😀a');
check(/(?i:[a--B])(?-i:[A])/v.test('aA'));
check(!/(?i:[a--B])(?-i:[A])/v.test('aa'));
check(/[^[\q{ab}]&&a]/v.test('b'));
check(/[^a--\q{ab}]/v.test('b'));
check(/[\(\)\[\]\{\}\/\-\|\!\#\%\&\,\:\;\<\=\>\@\`\~]/v.test('&'));
check(/[&!#%,:;<=>@`~]/v.test('&'));
check(/\p{L}/v.test('a') && !/\P{L}/v.test('a'));
check(/\u{1f600}/v.test('😀') && /^.$/v.test('😀'));
let r = /a/dgimsvy;
check(r.flags === 'dgimsvy' && r.unicodeSets && !r.unicode && r.hasIndices);
check(RegExp.prototype.unicodeSets === undefined);
let invalid = ['[a&&&b]', '[a&&&]', '[(]', '[^\\q{ab}]', '[a-z&&b]', '[a-z&&]',
 '[a--]', '[a&&]', '[a&&b--c]', '[ab&&c]', '[a&&bc]', '[a--b-c]',
 '[a-]', '[-a]', '[a/b]', '[{]', '[}]', '[|]', '[a&&[b]c]',
 '[!!]', '[##]', '[$$]', '[%%]', '[**]', '[++]', '[,,]', '[..]', '[::]',
 '[;;]', '[<<]', '[==]', '[>>]', '[??]', '[@@]', '[a^^]', '[``]', '[~~]',
 '[^\\q{}]', '[^[\\q{ab}]--\\q{ab}]', '[\\d-a]', '[a-\\d]',
 '[\\q{\\d}]', '[\\q{a(b}]', '\\q{ab}', '\\p{RGI_Emoji}', '[^\\p{RGI_Emoji}]'];
for (let i = 0; i < invalid.length; i++) {
 let caught = false;
 try { RegExp(invalid[i], 'v'); } catch(e) { caught = e instanceof SyntaxError; }
 if (!caught) throw new Error('accepted invalid v pattern ' + invalid[i]);
}
let caught = false;
try { RegExp('a', 'uv'); } catch(e) { caught = e instanceof SyntaxError; }
check(caught);
let largeSet = "[";
for (let i = 0; i < 1000; i++) largeSet += "\\p{L}";
largeSet += "]";
caught = false;
try { RegExp(largeSet, "v"); } catch(e) { caught = e instanceof RangeError; }
check(caught);
caught = false;
try { /([\q{aa|a}]+)+b/v.test("aaaaaaaaaaaaaaaaaaaaaaaaa"); }
catch(e) { caught = e instanceof RangeError; }
check(caught);
true;
