/*
Adapted QuickJS module fixture; synthetic fixture, not light-cljs output.
Source: research/quickjs/tests/test_cyclic_import.js (whole file)
Revision: 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2
Adapted source lines: none; the complete upstream body follows this header.
Harness adaptation: upstream lines 1-5 (negative resolution SyntaxError
metadata) are retained but not honored; normal completion is expected.
A synthetic observer emits a host effect only after this body completes.
The negative control changes fixture_cyclic_import.js line 2 from 2 * to 3 *.

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
/*---
negative:
  phase: resolution
  type: SyntaxError
---*/
// FIXME(bnoordhuis) shouldn't throw SyntaxError but that's still better
// than segfaulting, see https://github.com/quickjs-ng/quickjs/issues/567
import {assert} from "./assert.js"
import {f} from "./fixture_cyclic_import.js"
export {f}
export function g(x) { return x + 1 }
assert(f(1), 4)
