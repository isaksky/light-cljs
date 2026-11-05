// Synthetic fixture, not light-cljs output.
function check(ok, label) { if (!ok) throw label; }
function row(r, value, done) {
    check(r.value === value && r.done === done, 'iterator result');
}
function thrown(f, expected) {
    let caught = false;
    try { f(); } catch(e) { caught = e === expected; }
    check(caught, 'thrown identity');
}
function typeError(f) {
    let caught = false;
    try { f(); } catch(e) { caught = e instanceof TypeError; }
    check(caught, 'expected TypeError');
}
let entered = 0;
function* untouched() { entered++; try { yield 1; } finally { entered++; } }
let g = untouched(), token = {n: 17};
row(g.return(token), token, true);
row(g.next(), undefined, true);
row(g.return(9), 9, true);
thrown(() => g.throw(token), token);
g = untouched();
thrown(() => g.throw(token), token);
row(g.next(), undefined, true);
check(entered === 0, 'suspended start body');
let proto = Object.getPrototypeOf(Object.getPrototypeOf(g));
for (let name of ['return', 'throw']) {
    let d = Object.getOwnPropertyDescriptor(proto, name);
    check(d.writable && !d.enumerable && d.configurable, 'method descriptor');
    check(d.value.name === name && d.value.length === 1, 'method metadata');
    typeError(() => d.value.call({}));
    typeError(() => d.value.call(proto));
}
function* reentrant() {
    typeError(() => g.return(1));
    typeError(() => g.throw(2));
    yield 3;
}
g = reentrant(); row(g.next(), 3, false);
row(g.return(), undefined, true);

let log = '';
function* cleanup() {
    try {
        using resource = {[Symbol.dispose]() { log += 'd'; }};
        try { yield 1; log += 'unreachable'; }
        finally { log += 'a'; yield 2; log += 'b'; }
    } finally { log += 'c'; }
}
g = cleanup(); row(g.next(), 1, false);
row(g.return(token), 2, false);
check(log === 'a', 'return suspended finally');
row(g.next(), token, true);
check(log === 'abdc', 'return cleanup order');
log = ''; g = cleanup(); g.next(); row(g.throw(token), 2, false);
thrown(() => g.next(), token);
check(log === 'abdc', 'throw cleanup order');
function* catches() { try { yield 1; } catch(e) { yield e; } return 8; }
g = catches(); g.next(); row(g.throw(token), token, false); row(g.next(), 8, true);
function* overridden() { try { yield 1; } finally { yield 2; return 10; } }
g = overridden(); g.next(); row(g.return(5), 2, false); row(g.next(), 10, true);
g = overridden(); g.next(); g.return(5); row(g.return(6), 6, true);
g = overridden(); g.next(); g.return(5); thrown(() => g.throw(token), token);
function* disposalFails() {
    using resource = {[Symbol.dispose]() { throw token; }};
    yield 1;
}
g = disposalFails(); g.next(); thrown(() => g.return(5), token);
g = disposalFails(); g.next();
try { g.throw(42); throw 'no suppression'; }
catch(e) { check(e instanceof SuppressedError && e.error === token && e.suppressed === 42, 'suppression'); }
log = '';
for (let x of cleanup()) { break; }
// IteratorClose accepts a non-done result and leaves cleanup suspended.
check(log === 'a', 'for-of closes generator');

function iterable(it) { it[Symbol.iterator] = function() { return this; }; return it; }
function* delegate(it) { return 100 + (yield* it); }
let valueReads = 0, fixed = {done: false, get value() { valueReads++; throw 'eager value'; }};
let calls = '', steps = 0;
let it = iterable({
    next(v) {
        check(this === it && arguments.length === 1, 'delegate next receiver/arity');
        calls += 'n';
        if (steps++ === 0) { check(v === undefined, 'initial next'); return fixed; }
        check(v === 7, 'next after non-done return');
        return {done: true, value: 8};
    },
    get return() {
        calls += 'R';
        return function(v) {
            check(this === it && arguments.length === 1 && v === token, 'delegate return');
            calls += 'r'; return fixed;
        };
    },
    get throw() {
        calls += 'T';
        return function(v) {
            check(this === it && arguments.length === 1 && v === token, 'delegate throw');
            calls += 't'; return {done: true, value: 9};
        };
    }
});
g = delegate(it); check(g.next() === fixed, 'next identity');
check(g.return(token) === fixed, 'return identity');
check(valueReads === 0, 'non-done value untouched');
row(g.next(7), 108, true); check(calls === 'nRrn', 'return non-done resets completion');
steps = 0; calls = ''; g = delegate(it); g.next();
row(g.throw(token), 109, true); check(calls === 'nTt', 'throw done continues expression');
Object.defineProperty(it, 'return', {value(v) { return {done: true, value: v}; }});
steps = 0; g = delegate(it); g.next(); row(g.return(token), token, true);
for (let absent of [undefined, null]) {
    let plain = iterable({next() { return fixed; }, return: absent});
    g = delegate(plain); g.next(); row(g.return(token), token, true);
    let closed = 0;
    plain = iterable({next() { return fixed; }, throw: absent, return() {
        check(this === plain && arguments.length === 0, 'missing throw closes without arg');
        closed++; return {get done() { throw 'close done'; }, get value() { throw 'close value'; }};
    }});
    g = delegate(plain); g.next(); typeError(() => g.throw(token));
    check(closed === 1 && g.next().done, 'missing throw closes and completes');
    plain = iterable({next() { return fixed; }, return: absent});
    g = delegate(plain); g.next(); typeError(() => g.throw(token));
}
for (let name of ['return', 'throw']) {
    for (let failure of ['getter', 'call', 'primitive', 'done', 'value', 'noncallable']) {
        let closed = 0;
        let plain = iterable({next() { return fixed; }, return() { closed++; return {}; }});
        Object.defineProperty(plain, name, {get() {
            if (failure === 'getter') throw token;
            if (failure === 'noncallable') return 7;
            return function() {
                if (failure === 'call') throw token;
                if (failure === 'primitive') return 7;
                return {get done() { if (failure === 'done') throw token; return true; },
                    get value() { throw token; }};
            };
        }});
        g = delegate(plain); g.next();
        if (failure === 'primitive' || failure === 'noncallable') typeError(() => g[name](token));
        else thrown(() => g[name](token), token);
        check(closed === 0 && g.next().done, 'protocol error does not close');
    }
}
for (let failure of ['getter', 'call', 'primitive', 'noncallable']) {
    let plain = iterable({next() { return fixed; }});
    Object.defineProperty(plain, 'return', {get() {
        if (failure === 'getter') throw token;
        if (failure === 'noncallable') return 7;
        return function() { if (failure === 'call') throw token; return 7; };
    }});
    g = delegate(plain); g.next();
    if (failure === 'primitive' || failure === 'noncallable') typeError(() => g.throw(42));
    else thrown(() => g.throw(42), token);
}
function* inner() { try { yield 1; } catch(e) { yield e; } finally { log += 'i'; } return 4; }
function* middle() { return yield* inner(); }
function* outer() { try { return yield* middle(); } finally { log += 'o'; } }
log = ''; g = outer(); g.next(); row(g.throw(token), token, false);
row(g.return(8), 8, true); check(log === 'io', 'nested delegate cleanup');
let throws = 0;
it = iterable({next() { return fixed; }, get throw() {
    throws++;
    return function(v) { return throws === 1 ? fixed : {done: true, value: v}; };
}});
g = delegate(it); g.next(); check(g.throw(3) === fixed, 'throw non-done identity');
row(g.throw(4), 104, true); check(throws === 2, 'throw method read again');
true;
