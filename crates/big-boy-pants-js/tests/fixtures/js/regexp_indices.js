// Synthetic fixture, not light-cljs output. The first lastIndex row adapts
// QuickJS tests/test_builtin.js::test_regexp (803-807), revision 7d1c13c5d8f8,
// using u in place of the deferred v flag.
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
function check(v) { if (!v) throw new Error('RegExp indices assertion'); }
function own(o, k) { return Object.getOwnPropertyDescriptor(o, k) !== undefined; }
function pair(p, start, end) {
    check(Array.isArray(p) && p.length === 2 && p[0] === start && p[1] === end);
}
let a = /\u{10000}/ugd;
a.lastIndex = 1;
let r = a.exec("\u{10000}_\u{10000}");
pair(r.indices[0], 0, 2);
check(a.lastIndex === 2 && r.index === 0 && r[0].length === 2);
pair(a.exec("\u{10000}_\u{10000}").indices[0], 3, 5);
check(a.exec("\u{10000}_\u{10000}") === null && a.lastIndex === 0);
r = /a(b)?(c)/d.exec('xac');
check(Array.isArray(r.indices) && r.indices.length === r.length && r.indices.length === 3);
pair(r.indices[0], 1, 3);
check(r.indices[1] === undefined && 1 in r.indices);
pair(r.indices[2], 2, 3);
check(r.indices.groups === undefined && own(r.indices, 'groups'));
r = /(?<n>b)(?<missing>c)?/d.exec('ab');
pair(r.indices.groups.n, 1, 2);
check(r.indices.groups.n === r.indices[1]);
check(r.indices.groups.missing === undefined && own(r.indices.groups, 'missing'));
check(Object.getPrototypeOf(r.indices.groups) === null);
let d = Object.getOwnPropertyDescriptor(r, 'indices');
check(d.value === r.indices && d.writable && d.enumerable && d.configurable);
d = Object.getOwnPropertyDescriptor(r.indices, 'groups');
check(d.writable && d.enumerable && d.configurable);
d = Object.getOwnPropertyDescriptor(r.indices.groups, 'n');
check(d.writable && d.enumerable && d.configurable);
r.indices.groups.n[0] = 7;
check(r.indices[1][0] === 7);
r = /(?<n>a)|(?<n>b)/d.exec('b');
check(r.indices[1] === undefined && r.indices.groups.n === r.indices[2]);
pair(r.indices.groups.n, 0, 1);
r = /(?<n>a)|(?<n>b)/d.exec('a');
check(r.indices[2] === undefined && r.indices.groups.n === r.indices[1]);
r = /(?<__proto__>a)/d.exec('a');
check(r.indices.groups.__proto__ === r.indices[1] && Object.getPrototypeOf(r.indices.groups) === null);
pair(/(?=(ab))a/d.exec('xab').indices[1], 1, 3);
pair(/()/d.exec('x').indices[1], 0, 0);
pair(/(a)+/d.exec('xaa').indices[1], 2, 3);
pair(/./d.exec('\u{10000}').indices[0], 0, 1);
pair(/./du.exec('\udc00').indices[0], 0, 1);
a = /./duy; a.lastIndex = 1;
pair(a.exec('\u{10000}').indices[0], 0, 2);
check(a.lastIndex === 2);
a = /a/d; a.lastIndex = 99;
pair(a.exec('ba').indices[0], 1, 2);
check(a.lastIndex === 99 && a.hasIndices && !/a/.hasIndices);
check(/a/.exec('a').indices === undefined && !own(/a/.exec('a'), 'indices'));
check(/a/yusmgd.flags === 'dgmsuy' && /a/diu.flags === 'diu');
check(RegExp('a', 'd').hasIndices && new RegExp('a', 'd').hasIndices);
check(new RegExp(a).hasIndices && new RegExp(a, '').hasIndices === false);
check(a.toString() === '/a/d' && /a/d.test('a'));
check(RegExp.prototype.hasIndices === undefined);
let get = Object.getOwnPropertyDescriptor(RegExp.prototype, 'hasIndices').get;
let caught = false;
try { get.call({}); } catch (e) { caught = e instanceof TypeError; }
check(caught);
caught = false;
try { RegExp('a', 'dd'); } catch (e) { caught = e instanceof SyntaxError; }
check(caught);
get = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get;
check(get.call({hasIndices:true, global:true, unicode:true}) === 'dgu');
// Each exec creates fresh pairs; named references share only within one result.
a = /(?<n>a)/d;
let first = a.exec('a'); let second = a.exec('a');
check(first.indices !== second.indices && first.indices[1] !== second.indices[1]);
true;
