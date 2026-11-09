// bbp-authored conformance case (light-cljs-880).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 3085-3136:
// array_spread_and_spread_arguments (line 3085) and
// spread_protocol_errors_and_order (3102) (bbp-130): the array-literal rows
// spread_calls and array_literal_holes do not already pin: spread of arrays,
// strings and user iterables, a live array iterator, empty spreads, the
// protocol's `done` and `value` reads, and the TypeErrors of a value that is
// not iterable or an iterator that is not an object. Left out: the
// `Object.setPrototypeOf` row (light-cljs-883), the Uint8Array spread (typed
// arrays, light-cljs-769), and `next()` throwing, after which QuickJS at
// 7d1c13c calls `return()` (the Pants engine and Goose do not, as specified).
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// array_spread_and_spread_arguments (line 3085, bbp-130): rows 1, 5, 8 and the `[...1]` half of row 6.
{
  let x = [1, 2, ...[3, 4]];
  row(x.length === 4 && x[0] === 1 && x[1] === 2 && x[2] === 3 && x[3] === 4);
}
{
  let n = 0; let it = {}; it[Symbol.iterator] = function() { return {next() { n++; return {done: n > 3, value: n}; }}; }; let x = [0, ...it, n];
  row(n === 4 && x.length === 5 && x[1] === 1 && x[3] === 3 && x[4] === 4);
}
{
  let n = 0; try { [...1]; } catch(e) { if (e instanceof TypeError) n++; }
  row(n === 1);
}
{
  let it = [1].values();
  row(Array.prototype[Symbol.iterator] === Array.prototype.values && it[Symbol.iterator]() === it);
}
// spread_protocol_errors_and_order (line 3102, bbp-130): the array-literal rows.
{
  let a = [1]; let it = a.values(); a.push(2); let x = [...it];
  row(x.length === 2 && x[1] === 2 && [...it].length === 0);
}
row([...[], 1, ...[], 2].length === 2);
{
  let n = 0; let it = {next() { n++; this.next = function() { throw 99; }; return {get done() { return n > 2; }, get value() { return n; }}; }}; it[Symbol.iterator] = function() { return this; }; let a = [...it];
  row(n === 3 && a[0] === 1 && a[1] === 2);
}
{
  let it = {next() { return {done: true, get value() { throw 99; }}; }}; it[Symbol.iterator] = function() { return this; };
  row([...it].length === 0);
}
{
  let a = [1]; a[Symbol.iterator] = function() { return [7].values(); };
  row([...a][0] === 7 && [...'A\u{1f600}'].length === 2);
}
{
  let n = 0; let it = {}; it[Symbol.iterator] = 1; try { [...it]; } catch(e) { if (e instanceof TypeError) n++; } it[Symbol.iterator] = function() { return 1; }; try { [...it]; } catch(e) { if (e instanceof TypeError) n++; } it[Symbol.iterator] = function() { return {next() { return 1; }}; }; try { [...it]; } catch(e) { if (e instanceof TypeError) n++; }
  row(n === 3);
}
row((name(() => [...1])) === "TypeError");
failed === 0 ? rows : -failed;
