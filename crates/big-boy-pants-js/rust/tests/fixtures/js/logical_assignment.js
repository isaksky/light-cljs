// Synthetic fixture, not light-cljs output.
let calls = 0;
function rhs() { calls++; return 42; }
let a = null, b, c = 0, d = false, e = '';
let ok = (a ?? rhs()) === 42 && (b ?? 3) === 3 && (c ?? rhs()) === 0
    && (d ?? rhs()) === false && (e ?? rhs()) === '' && calls === 1;
ok = ok && (null ?? undefined ?? 7) === 7 && (false || (null ?? 9)) === 9
    && ((false || null) ?? 8) === 8 && (null ?? (1 && 2)) === 2;
ok = ok && (a ??= 4) === 4 && (a ??= rhs()) === 4
    && (b ||= 5) === 5 && (b ||= rhs()) === 5
    && (c &&= rhs()) === 0 && (d &&= rhs()) === false
    && (e ||= 6) === 6 && (e &&= 7) === 7 && calls === 1;
let x, y;
ok = ok && (x ??= y ||= 10) === 10 && x === 10 && y === 10;
const zero = 0, one = 1;
ok = ok && (zero &&= rhs()) === 0 && (one ||= rhs()) === 1
    && (one ??= rhs()) === 1 && calls === 1;
let caught = 0;
try { zero ||= rhs(); } catch (err) { if (err instanceof TypeError) caught++; }
try { missing ??= rhs(); } catch (err) { if (err instanceof ReferenceError) caught++; }
try { tdz &&= rhs(); let tdz; } catch (err) { if (err instanceof ReferenceError) caught++; }
ok = ok && caught === 3 && calls === 2;
let log = '', stored = null;
const object = {
    get p() { log += 'g'; return stored; },
    set p(v) { log += 's'; stored = v; }
};
function base() { log += 'b'; return object; }
const key = { toString() { log += 'k'; return 'p'; } };
let result = base()[key] ??= (log += 'r', 11);
ok = ok && result === 11 && stored === 11 && log === 'bkgrs';
log = '';
result = base()[key] ||= rhs();
ok = ok && result === 11 && log === 'bkg' && calls === 2;
log = '';
result = base().p &&= (log += 'r', 12);
ok = ok && result === 12 && log === 'bgrs' && stored === 12;
const frozen = {};
Object.defineProperty(frozen, 'yes', {value: 1});
Object.defineProperty(frozen, 'no', {value: 0});
ok = ok && (frozen.yes ||= rhs()) === 1 && (frozen.no &&= rhs()) === 0
    && (frozen.yes ??= rhs()) === 1 && calls === 2;
const getterOnly = {get p() { return 1; }};
ok = ok && (getterOnly.p ||= rhs()) === 1 && (getterOnly.p ??= rhs()) === 1;
let f, arrow, generator, asyncFn, klass;
f ||= function() {}; arrow ??= () => {}; generator ||= function*() {};
asyncFn ??= async function() {}; klass ||= class {};
ok = ok && f.name === 'f' && arrow.name === 'arrow' && generator.name === 'generator'
    && asyncFn.name === 'asyncFn' && klass.name === 'klass';
let named; named ||= function explicit() {};
let notNamed; notNamed ||= (0, function() {});
let holder = {}; holder.p ||= function() {}; holder['q'] ??= () => {};
ok = ok && named.name === 'explicit' && notNamed.name === ''
    && holder.p.name === '' && holder.q.name === '';
let hits = 0;
const noCoercion = {toString() { hits++; throw 99; }, valueOf() { hits++; throw 99; }};
ok = ok && (noCoercion ?? rhs()) === noCoercion && hits === 0;
let bigint = 0n, nan = NaN;
ok = ok && (bigint ?? rhs()) === 0n && (bigint &&= rhs()) === 0n
    && (bigint ||= 2n) === 2n && Number.isNaN(nan ?? rhs()) && calls === 2;
let truthy = true; truthy &&= function() {};
let asyncArrow; asyncArrow ??= async () => {};
ok = ok && truthy.name === 'truthy' && asyncArrow.name === 'asyncArrow'
    && ('abc'.length ||= rhs()) === 3 && calls === 2;
ok;
