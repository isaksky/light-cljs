// bbp-authored conformance case (light-cljs-886).
// Derives from: ~/src/quickjs/tests/test_language.js lines 281-354
// (test_class, revision 7d1c13c): its base-class rows, the classes C, E1, S
// and P and their asserts, with the same text. The rows that need `extends`
// and `super` (class D) stay in quickjs/test_language.js/test_class, which
// passes once light-cljs-887 and -888 land. Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function test_class_base()
{
    var o;
    class C {
        constructor() {
            this.x = 10;
        }
        f() {
            return 1;
        }
        static F() {
            return -1;
        }
        get y() {
            return 12;
        }
    };

    row(C.F() === -1);
    row(Object.getOwnPropertyDescriptor(C.prototype, "y").get.name === "get y");

    o = new C();
    row(o.f() === 1);
    row(o.x === 10);

    /* test class name scope */
    var E1 = class E { static F() { return E; } };
    row(E1 === E1.F());

    class S {
        static x = 42;
        static y = S.x;
        static z = this.x;
    }
    row(S.x === 42);
    row(S.y === 42);
    row(S.z === 42);

    class P {
        get = () => "123";
        static() { return 42; }
    }
    row(new P().get() === "123");
    row(new P().static() === 42);
}
test_class_base();
failed === 0 ? rows : -failed;
