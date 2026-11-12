// bbp-authored conformance case (light-cljs-839).
// Derives from: ~/src/quickjs/quickjs.c lines 7611-7641, 7908-7922,
// 18121-18129 and 21245-21260 (revision 7d1c13c): invalid calls have no
// source name; invalid constructors quote only a string data property
// `name`, own or on the immediate prototype, without running JavaScript.
// The Pants engine instead quotes a constructor expression's source span
// and a non-callable value's type (areas.edn :pants-gap).
(function () {
    var rows = 0, observed = 0;
    function expect(run, message) {
        rows++;
        try { run(); } catch (e) {
            if (e instanceof TypeError && e.message === message) return;
            throw "row " + rows + ": " + e;
        }
        throw "row " + rows + ": no exception";
    }
    function* G() {}
    var H = G, o = { method: G };
    expect(function () { new G(); }, "G is not a constructor");
    expect(function () { new H(); }, "G is not a constructor");
    expect(function () { new o.method(); }, "G is not a constructor");
    expect(function () { new o["method"](...[]); }, "G is not a constructor");
    expect(function () { new (function* () {})(); }, " is not a constructor");
    var arrow = () => {};
    expect(function () { new arrow(); }, "arrow is not a constructor");
    async function A() {}
    expect(function () { new A(); }, "A is not a constructor");
    var method = { m() {} }.m;
    expect(function () { new method(); }, "m is not a constructor");
    expect(function () { new Math.abs(); }, "abs is not a constructor");
    expect(function () { new Symbol(); }, "Symbol is not a constructor");
    var bound = G.bind(null);
    expect(function () { new bound(); }, "bound G is not a constructor");
    Object.defineProperty(G, "name", { value: "renamed μ😀" });
    expect(function () { new H(); }, "renamed μ😀 is not a constructor");
    Object.defineProperty(G, "name", { value: "a\0b" });
    expect(function () { new G(); }, "a is not a constructor");
    Object.defineProperty(G, "name", { value: 17 });
    expect(function () { new G(); }, "not a constructor");
    Object.defineProperty(G, "name", { value: { toString() { observed++; throw 1; } } });
    expect(function () { new G(); }, "not a constructor");
    Object.defineProperty(G, "name", { get() { observed++; throw 2; } });
    expect(function () { new G(); }, "not a constructor");
    delete G.name;
    var parent = { name: "inherited" };
    Object.setPrototypeOf(G, parent);
    expect(function () { new G(); }, "inherited is not a constructor");
    Object.setPrototypeOf(parent, { name: "too far" });
    delete parent.name;
    expect(function () { new G(); }, "not a constructor");
    Object.setPrototypeOf(G, new Proxy({}, { get() { observed++; throw 3; } }));
    expect(function () { new G(); }, "not a constructor");
    var proxy = new Proxy(H, { get() { observed++; throw 4; } });
    expect(function () { new proxy(); }, "not a constructor");
    var notCallable = 3;
    expect(function () { notCallable(); }, "not a function");
    expect(function () { notCallable(...[]); }, "not a function");
    expect(function () { ({ x: 3 }).x(); }, "not a function");
    expect(function () { ({ x: 3 })["x"](...[]); }, "not a function");
    expect(function () { Function.prototype.call.call(notCallable); }, "not a function");
    expect(function () { new notCallable(); }, "not a function");
    expect(function () { new notCallable(...[]); }, "not a function");
    expect(function () { new ({ name: "ignored" })(); }, "not a constructor");
    if (observed !== 0) throw "diagnostic ran JavaScript";
    return rows;
})();
