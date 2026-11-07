// A lexical declaration over a non-configurable global property
// serial_tests.goose installs (light-cljs-824, HasRestrictedGlobalProperty)
// is a SyntaxError. Completes with 2 when no such property exists.
let reserved = 2;
reserved;
