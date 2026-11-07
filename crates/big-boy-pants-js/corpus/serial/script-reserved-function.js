// A function declaration over a global property serial_tests.goose
// installs (light-cljs-824): a non-configurable property that is not a
// writable, enumerable data property refuses it with a SyntaxError before
// any declaration takes effect; a non-configurable writable enumerable one
// is kept and the function is put into it. Completes with 1 when no such
// property exists.
function reserved() { return 1; }
reserved();
