// Synthetic fixture, not light-cljs output. Assertions adapted from QuickJS
// tests/test_builtin.js::test_regexp, revision 7d1c13c5d8f8.
// Copyright (c) 2017-2021 Fabrice Bellard. MIT license; see docs/attribution.md.
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
function check(v) { if (!v) throw new Error('RegExp assertion'); }
function row(a, b) {
    check(a.length === b.length);
    for (let i = 0; i < b.length; i++) check(a[i] === b[i] && i in a);
}
let str = 'abbbbbc'; let a = /(b+)c/.exec(str);
check(a[0] === 'bbbbbc' && a[1] === 'bbbbb' && a.index === 1 && a.input === str);
check(a.groups === undefined && /(b+)c/.test(str));
check(/\x61/.exec('a')[0] === 'a');
check(/\u0061/.exec('a')[0] === 'a');
check(/\ca/.exec('\x01')[0] === '\x01');
check(/\\a/.exec('\\a')[0] === '\\a');
check(/\c0/.exec('\\c0')[0] === '\\c0');
a = /(\.(?=com|org)|\/)/.exec('ah.com'); check(a.index === 2 && a[0] === '.');
check(/(\.(?!com|org)|\/)/.exec('ah.com') === null);
a = /(?=(a+))/.exec('baaabac'); check(a.index === 1 && a[0] === '' && a[1] === 'aaa');
a = /(z)((a+)?(b+)?(c))*/.exec('zaacbbbcac');
row(a, ['zaacbbbcac', 'z', 'ac', 'a', undefined, 'c']); check(a.length === 6 && 4 in a && a[4] === undefined);
a = RegExp('\0a'); check(a.toString() === '/\0a/' && a.exec('\0a')[0] === '\0a');
check(/{1a}/.toString() === '/{1a}/');
row(/a{1+/.exec('a{11'), ['a{11']);
row(/(?:(?=(abc)))a/.exec('abc'), ['a', 'abc']);
row(/(?:(?=(abc)))?a/.exec('abc'), ['a', undefined]);
row(/(?:(?=(abc))){0,2}a/.exec('abc'), ['a', undefined]);
row(/(?:|[\w])+([0-9])/.exec('123a23'), ['123a23', '3']);
check(/()*?a/.exec(',') === null);
check(/[\q{a\b}]/.test('a\b') && /[\b]/.test('\b'));
check(new RegExp('a/b').source === 'a\\/b');
check(RegExp('').toString() === '/(?:)/');
check(/a/ymsg.flags === 'gmsy');
let r = /a/g; check(r.exec('aba').index === 0 && r.lastIndex === 1);
check(r.exec('aba').index === 2 && r.lastIndex === 3);
check(r.exec('aba') === null && r.lastIndex === 0);
r = /a/y; check(r.exec('ba') === null); r.lastIndex = 1; check(r.test('ba') && r.lastIndex === 2);
let previous; for (let i = 0; i < 2; i++) { r = /a/; check(r !== previous); previous = r; }
let caught = false; try { RegExp('('); } catch (e) { caught = e instanceof SyntaxError; } check(caught);
true;
