/*
 * Adapted QuickJS test text; synthetic fixture, not light-cljs output.
 * Source: research/quickjs/tests/test_language.js, lines 1-32 and 677-993,
 * with calls from lines 1022-1023.
 * Revision 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2 (VERSION 2026-06-04).
 * Adapted case: test_using; test_await_using and all helpers are unchanged.
 * In test_using's final group, wrap both prototype.constructor assignments
 * (upstream lines 866-869 and 882) in assert_throws(TypeError, function() {...})
 * because realm intrinsics are immutable here (user decision, clj-quickjs-234).
 * All original assertions and expected values are unchanged. Otherwise only
 * this descriptive/license header and the two trailing calls are added.
 *
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
function assert(actual, expected, message) {
    if (arguments.length == 1)
        expected = true;

    if (Object.is(actual, expected))
        return;

    if (actual !== null && expected !== null
    &&  typeof actual == 'object' && typeof expected == 'object'
    &&  actual.toString() === expected.toString())
        return;

    throw Error("assertion failed: got |" + actual + "|" +
                ", expected |" + expected + "|" +
                (message ? " (" + message + ")" : ""));
}

function assert_throws(expected_error, func)
{
    var err = false;
    try {
        func();
    } catch(e) {
        err = true;
        if (!(e instanceof expected_error)) {
            throw Error("unexpected exception type");
        }
    }
    if (!err) {
        throw Error("expected exception");
    }
}
function assert_array_eq(actual, expected, message)
{
    assert(Array.isArray(actual), true, message);
    assert(actual.length, expected.length, message);
    for (var i = 0; i < expected.length; i++) {
        assert(actual[i], expected[i],
               (message ? message + ": " : "") + "index " + i);
    }
}

/* TC39 explicit resource management: `using` declaration */
function test_using()
{
    /* AddDisposableResource validates and snapshots the dispose method at
       declaration; the body must not run on failure. */

    /* Missing Symbol.dispose → TypeError before the body runs. */
    (function() {
        var bodyRan = false;
        var caught;
        try {
            (function() { using x = {}; bodyRan = true; })();
        } catch (e) { caught = e; }
        assert(bodyRan, false);
        assert(caught instanceof TypeError, true);
    })();

    /* Non-object, non-null → TypeError. */
    (function() {
        var bodyRan = false;
        var caught;
        try {
            (function() { using x = 42; bodyRan = true; })();
        } catch (e) { caught = e; }
        assert(bodyRan, false);
        assert(caught instanceof TypeError, true);
    })();

    /* Symbol.dispose getter throwing propagates at declaration. */
    (function() {
        var bodyRan = false;
        var caught;
        try {
            (function() {
                var o = {};
                Object.defineProperty(o, Symbol.dispose, {
                    get() { throw new Error("getter"); }
                });
                using y = o;
                bodyRan = true;
            })();
        } catch (e) { caught = e; }
        assert(bodyRan, false);
        assert(caught instanceof Error, true);
        assert(caught.message, "getter");
    })();

    /* Non-callable dispose → TypeError. */
    (function() {
        var bodyRan = false;
        var caught;
        try {
            (function() {
                using x = { [Symbol.dispose]: 123 };
                bodyRan = true;
            })();
        } catch (e) { caught = e; }
        assert(bodyRan, false);
        assert(caught instanceof TypeError, true);
    })();

    /* null / undefined are spec-permitted and must not throw. */
    (function() {
        var disposed = 0;
        (function() {
            using a = null;
            using b = undefined;
            using c = { [Symbol.dispose]() { disposed++; } };
        })();
        assert(disposed, 1);
    })();

    /* for-of using: validation applies per-iteration. */
    (function() {
        var iters = 0;
        var caught;
        try {
            (function() {
                for (using x of [{ [Symbol.dispose]() {} }, 42]) {
                    iters++;
                }
            })();
        } catch (e) { caught = e; }
        assert(iters, 1);
        assert(caught instanceof TypeError, true);
    })();

    /* for-of using: normal completion must not throw (regression for
       a prior bug where each iteration threw `undefined`). */
    (function() {
        var log = [];
        (function() {
            for (using x of [
                { tag: "a", [Symbol.dispose]() { log.push(this.tag); } },
                { tag: "b", [Symbol.dispose]() { log.push(this.tag); } },
                { tag: "c", [Symbol.dispose]() { log.push(this.tag); } },
            ]) {
                log.push("iter");
            }
        })();
        assert_array_eq(log, ["iter", "a", "iter", "b", "iter", "c"]);
    })();

    /* LIFO disposal order. */
    (function() {
        var log = [];
        (function() {
            using a = { [Symbol.dispose]() { log.push("a"); } };
            using b = { [Symbol.dispose]() { log.push("b"); } };
            using c = { [Symbol.dispose]() { log.push("c"); } };
        })();
        assert_array_eq(log, ["c", "b", "a"]);
    })();

    /* Multiple dispose errors chain as SuppressedError (error=new,
       suppressed=previous). */
    (function() {
        var caught;
        try {
            (function() {
                using a = { [Symbol.dispose]() { throw new Error("a"); } };
                using b = { [Symbol.dispose]() { throw new Error("b"); } };
                using c = { [Symbol.dispose]() { throw new Error("c"); } };
            })();
        } catch (e) { caught = e; }
        /* LIFO: c first → error_state=c. b → Suppressed(b, c).
           a → Suppressed(a, Suppressed(b, c)). */
        assert(caught instanceof SuppressedError, true);
        assert(caught.error.message, "a");
        assert(caught.suppressed instanceof SuppressedError, true);
        assert(caught.suppressed.error.message, "b");
        assert(caught.suppressed.suppressed.message, "c");
    })();

    /* Symbol.dispose is read exactly once at declaration (snapshot). */
    (function() {
        var reads = 0;
        var disposed = 0;
        var target = {};
        Object.defineProperty(target, Symbol.dispose, {
            get() {
                reads++;
                return function() { disposed++; };
            }
        });
        (function() { using x = target; })();
        assert(reads, 1);
        assert(disposed, 1);
    })();

    /* DisposableStack.prototype.move uses the %DisposableStack%
       intrinsic; tampering with or deleting the global binding must
       not redirect it. */
    (function() {
        var saved = globalThis.DisposableStack;
        try {
            var ds = new saved();
            var disposed = 0;
            ds.use({ [Symbol.dispose]() { disposed++; } });
            globalThis.DisposableStack = class Other {};
            var moved = ds.move();
            assert(Object.getPrototypeOf(moved), saved.prototype);
            moved.dispose();
            assert(disposed, 1);
            delete globalThis.DisposableStack;
            var ds2 = new saved();
            ds2.use({ [Symbol.dispose]() {} });
            var moved2 = ds2.move();
            assert(Object.getPrototypeOf(moved2), saved.prototype);
        } finally {
            globalThis.DisposableStack = saved;
        }
    })();

    /* SuppressedError wrapping can't be hijacked via prototype.constructor. */
    (function() {
        var savedCtor = SuppressedError.prototype.constructor;
        try {
            var hijacked = false;
            assert_throws(TypeError, function() {
                SuppressedError.prototype.constructor = function() {
                    hijacked = true;
                    throw new Error("hijack");
                };
            });
            var caught;
            try {
                (function() {
                    using a = { [Symbol.dispose]() { throw new Error("a"); } };
                    using b = { [Symbol.dispose]() { throw new Error("b"); } };
                })();
            } catch (e) { caught = e; }
            assert(hijacked, false);
            assert(caught instanceof SuppressedError, true);
            assert(caught.error.message, "a");
            assert(caught.suppressed.message, "b");
        } finally {
            assert_throws(TypeError, function() {
                SuppressedError.prototype.constructor = savedCtor;
            });
        }
    })();
}

/* Async counterpart: `await using` and AsyncDisposableStack. */
async function test_await_using()
{
    /* await using validates the async/sync dispose method at declaration. */
    {
        var bodyRan = false;
        var caught;
        try {
            await (async function() {
                await using x = {};
                bodyRan = true;
            })();
        } catch (e) { caught = e; }
        assert(bodyRan, false);
        assert(caught instanceof TypeError, true);
    }

    /* Falls back to Symbol.dispose when @@asyncDispose is absent. */
    {
        var ok = false;
        await (async function() {
            await using x = { [Symbol.dispose]() {} };
            ok = true;
        })();
        assert(ok, true);
    }

    /* AsyncDisposableStack.disposeAsync must await each dispose's returned
       promise before invoking the next one (regression for a prior bug
       that called all disposes in a single synchronous pass). */
    {
        var log = [];
        var stack = new AsyncDisposableStack();
        stack.use({
            async [Symbol.asyncDispose]() { log.push("outer-called"); }
        });
        stack.use({
            async [Symbol.asyncDispose]() {
                log.push("inner-called");
                await null;
                log.push("inner-after-await");
            }
        });
        var p = stack.disposeAsync();
        log.push("after-sync-dispose");
        await p;
        assert_array_eq(log, [
            "inner-called",
            "after-sync-dispose",
            "inner-after-await",
            "outer-called"
        ]);
    }

    /* disposeAsync called twice without awaiting the first must still
       dispose all resources in LIFO order. */
    {
        var log = [];
        var stack = new AsyncDisposableStack();
        stack.use({ [Symbol.asyncDispose]() { log.push(42); } });
        stack.use({ [Symbol.asyncDispose]() { log.push(43); } });
        stack.disposeAsync();
        assert(stack.disposed, true);
        await stack.disposeAsync();
        assert_array_eq(log, [43, 42]);
    }

    /* LIFO. */
    {
        var log = [];
        var stack = new AsyncDisposableStack();
        stack.use({ [Symbol.asyncDispose]() { log.push("a"); } });
        stack.use({ [Symbol.asyncDispose]() { log.push("b"); } });
        stack.use({ [Symbol.asyncDispose]() { log.push("c"); } });
        await stack.disposeAsync();
        assert_array_eq(log, ["c", "b", "a"]);
    }

    /* SuppressedError chaining across async dispose errors. */
    {
        var stack = new AsyncDisposableStack();
        stack.use({
            async [Symbol.asyncDispose]() { throw new Error("first"); }
        });
        stack.use({
            async [Symbol.asyncDispose]() { throw new Error("second"); }
        });
        var caught;
        try {
            await stack.disposeAsync();
        } catch (e) { caught = e; }
        /* LIFO: second throws → completion=second. first throws →
           Suppressed(error=first, suppressed=second). */
        assert(caught instanceof SuppressedError, true);
        assert(caught.error.message, "first");
        assert(caught.suppressed.message, "second");
    }

    /* disposeAsync always resolves to undefined. */
    {
        var stack = new AsyncDisposableStack();
        stack.use({ [Symbol.asyncDispose]() { return 42; } });
        stack.use({ async [Symbol.asyncDispose]() { return "hello"; } });
        var result = await stack.disposeAsync();
        assert(result, undefined);
    }
}
test_using();
test_await_using().catch(e => { throw e; });
