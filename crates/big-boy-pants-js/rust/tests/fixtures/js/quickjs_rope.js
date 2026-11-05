// QuickJS tests/test_builtin.js::rope_concat and test_rope, unchanged.
// Revision 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.
// Synthetic fixture, not light-cljs output. Added minimal assert and invocation.
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

function assert(actual, expected) { if (actual !== expected) throw Error("rope mismatch"); }

function rope_concat(n, dir)
{
    var i, s;
    s = "";
    if (dir > 0) {
        for(i = 0; i < n; i++)
            s += String.fromCharCode(i & 0xffff);
    } else {
        for(i = n - 1; i >= 0; i--)
            s = String.fromCharCode(i & 0xffff) + s;
    }
    
    for(i = 0; i < n; i++) {
        /* test before the assert to go faster */
        if (s.charCodeAt(i) != (i & 0xffff)) {
            assert(s.charCodeAt(i), i & 0xffff);
        }
    }
}

function test_rope()
{
    rope_concat(100000, 1);
    rope_concat(100000, -1);
}

test_rope();
true;
