# Bounded JavaScript runtime evidence

Checked on 2026-10-09 against Maven ClojureScript **1.12.134**. The runtime
implementation in `src/js_runtime.js` is original first-party code; the oracle
is used for behavior checks and is not embedded or vendored into output.

The October 10 [interop update](compiler-interop.md) supersedes the historical
frozen-public-wrapper restriction below. Public value objects now accept host
properties; private storage, prototypes and inspection snapshots remain frozen.
Collection operations still retain immutable contents and reject forged brands.

`node tests/support/js_runtime.test.js` passes independent expected-value and
ordinary-Error assertions without invoking the compiler or outcome encoder.
It checks keyword conversion, exact UTF-16, fixed/overloaded/variadic arities,
nil defaults, callable values, equality, frozen storage/prototypes,
opaque host arrays/objects, and rejection of forged brands. It also verifies
that shared caller-owned objects remain unfrozen and that array-based literal
construction copies caller storage and supports a 150,000-element vector.

A supplemental direct-runtime matrix made **14,812** comparisons with no
mismatches. It used the existing reference build at
`/tmp/light-cljs-milestone-probes`; its copied `cljs/core.cljs` was checked
byte-for-byte against the installed 1.12.134 Maven jar. This is runtime evidence,
not a fresh compiler build or a substitute for the normal execution suite.
The source SHA-256 was
`35344afe7d6a56a610d85dd759a96eb84a0aac398faa80d31eb81ecdbd127e80`;
the Maven jar SHA-256 was
`d019b73177e4805748f4ee995c4cfcd14ebf28bff685a84059813fbe4e8c878f`.

## Observed distinctions

| Operation | Pinned behavior retained |
| --- | --- |
| Vector `nth`, `get`, invocation | Nonnegative numeric fractions truncate; negative fractions fail/default. Strings such as `"0"` are not vector indices. Invocation ignores extra arguments and throws Error for a missing/invalid index. |
| Vector `contains?` | Only integer numeric indices in range are present. |
| String `nth` | Requires a numeric index; fractions greater than −1 and below the UTF-16 length truncate toward zero. |
| String `get`, `contains?` | Accept coercible indices including `"0"`, `"1.8"`, `""`, false and true; nullish indices are absent. |
| Nil `nth` | A nonnumeric index throws before checking the collection. With a numeric index, two-argument `nth` preserves null versus undefined; three-argument `nth` returns its default. |
| Core arities | `keyword` dispatches 1/2 and `nth`/`get` dispatch 2/3; other arities throw Error. Fixed `name`/`namespace`/`count`/`contains?` ignore extras. Missing `count` returns zero. Zero-argument equality returns true. |
| Keywords | One-argument keyword conversion preserves an existing keyword. `"/"` has no namespace. `"a/b/c"` has name `"a"`, no namespace, and full equality spelling `"a/b/c"`. Two-argument conversion extracts the name of keyword operands. |
| Equality | Nil equals undefined; −0 equals 0. Scalar NaN is unequal to itself. Identical vectors have an identity fast path, while distinct vectors compare nested elements. Foreign objects use identity, never fields. |

Two deliberate boundaries remain: host arrays/objects are opaque rather than
collections, and two-argument keyword conversion accepts only string/keyword/symbol
names and string/keyword/symbol/nullish namespaces. The upstream constructor also
accepts arbitrary JS values in these positions; that broader domain is not
part of this runtime. One-argument keyword conversion of other values returns
nil as upstream does. The symbol/list milestone below extends named conversion
and sequential values without widening the host collection domain.

The inspected Maven `cljs/core.cljs` definitions were `count`, `nth`, `get`,
`Keyword`, `namespace`, `keyword`, `PersistentVector`, and `name`. Generated
`cljs/core.js` additionally established `Keyword.prototype.call`,
`PersistentVector.prototype.call`, `contains_QMARK_`, and `_EQ_` dispatch.

## Reproduction

The existing reference launcher builds with
`{:optimizations :none, :static-fns false, :def-emits-var false}` and asserts
the compiler identity/settings. From the repository root, create a small
reference build using that launcher, then run the supplemental matrix below.
The JavaScript here is an inline runtime probe, not another fixture comparison
runner. Source execution, traces, stdout, typed outcomes and cardinality remain
the responsibility of `tests/execution.rs` and its existing harness.

```sh
runtime_probe_root="$PWD"
runtime_probe_out="$(mktemp -d /tmp/light-cljs-runtime-probe.XXXXXX)"
printf 'runtime-bootstrap\t[1]\n' > "$runtime_probe_out/cases.tsv"
clojure -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' \
  tests/reference/execution.clj release 1.12.134 - "$runtime_probe_out" \
  cases "$runtime_probe_out/cases.tsv"
cd "$runtime_probe_out"
LIGHT_RUNTIME_SOURCE="$runtime_probe_root/src/js_runtime.js" node <<'JS'
const fs = require('node:fs');
require('./reference.js');
const rt = (0, eval)(fs.readFileSync(process.env.LIGHT_RUNTIME_SOURCE, 'utf8'));
const c = cljs.core;
const defs = [null, undefined, false, true, 0, -0, -1, -.9, .9, 1, 1.8, 2, 3,
  NaN, Infinity, -Infinity, '', 'a', 'abc', '0', '1.8', '-0.9', '\ud800',
  'x/y', 'x/y/z', {k:[null,'x']}, {k:['ns','x']}, {k:[null,'0']},
  {v:[]}, {v:[null,false,undefined]}, {v:[10,20,30]},
  {v:[{k:['ns','x']},{v:[1,2]}]}];
const build = (d, reference) => d && typeof d === 'object'
  ? (d.k ? (reference ? c.keyword(...d.k) : rt.keyword(...d.k))
    : (reference ? c.vector(...d.v.map(x => build(x, reference)))
      : rt.vector(...d.v.map(x => build(x, reference))))) : d;
function encode(x, reference) {
  if (x === undefined) return ['undefined'];
  if (x === null) return ['nil'];
  if (typeof x === 'number') return ['number', Object.is(x, -0) ? '-0' : String(x)];
  if (typeof x === 'boolean' || typeof x === 'string') return [typeof x, x];
  if (reference && x instanceof c.Keyword)
    return ['kw', encode(x.ns, reference), encode(x.name, reference), x.fqn];
  if (reference && x instanceof c.PersistentVector)
    return ['vector', Array.from({length:c.count(x)}, (_, i) => encode(c.nth(x, i), reference))];
  if (!reference) {
    const a = rt.inspect(x);
    if (a) return a.kind === 'keyword'
      ? ['kw', encode(a.namespace, reference), encode(a.name, reference), a.spelling]
      : ['vector', a.items.map(y => encode(y, reference))];
  }
  throw new Error('unexpected result');
}
let checks = 0;
function check(name, args) {
  function outcome(reference) {
    try {
      const fn = reference
        ? c[({contains:'contains_QMARK_', equal:'_EQ_'})[name] || name]
        : rt.core[name];
      return ['ok', encode(fn(...args.map(x => build(x, reference))), reference)];
    } catch (e) { return ['throw', e.name]; }
  }
  const actual = outcome(false), expected = outcome(true);
  checks++;
  require('node:assert/strict').deepEqual(actual, expected, name + ': ' + JSON.stringify(args));
}
for (const name of ['count','name','namespace','keyword']) {
  check(name, []);
  for (const x of defs) {
    check(name, [x]);
    if (name !== 'keyword') { check(name, [x,123]); check(name, [x,123,456]); }
  }
}
for (const name of ['nth','get','contains','equal']) {
  check(name, []);
  for (const x of defs) {
    check(name, [x]);
    for (const y of defs) {
      check(name, [x,y]);
      if (name === 'nth' || name === 'get') {
        for (const fallback of [null,undefined,false,'missing']) check(name, [x,y,fallback]);
        check(name, [x,y,'missing',123]);
      }
    }
  }
}
for (const x of [null,undefined,'','ns',{k:['other','ns']}])
  for (const y of ['','x','a/b',{k:['other','x']}]) check('keyword', [x,y]);
console.log('Reference comparisons:', checks);
JS
```

Expected final line: `Reference comparisons: 14812`. The independent durable
assertions run with `node tests/support/js_runtime.test.js`. End-to-end
comparison uses `LIGHT_CLJS_REFERENCE=release cargo test --locked --test
execution -- --ignored --nocapture`, keeping the established oracle selection
and outcome transport.


## Quote, symbols and finite sequences (2026-10-09)

The next milestone passed **353** compiled fixture outcomes (56 additions to
297), all **48** generated cases, and **119** analyzer boundary classifications
through the existing pinned Maven 1.12.134 runner. All seven opt-in reference
tests passed. Quoted versus executable forms, nested symbols, list/vector/seq
identity, empty list versus nil, effects and errors are observed structurally;
no `pr-str` or `clj->js` normalization is used. Backend-independent typed encoder
assertions cover the additional tags and trusted adapter boundary.

The reference adapter identifies List/EmptyList concretely and reports Cons and
other finite seqs as `seq`. Upstream `list?` also accepts Cons, so that predicate
alone would erase the representation distinction required for `conj` on a seq.
The independent runtime checks verify tail identity sharing, frozen wrappers and
owned storage, failed-update independence, consistent internal hashes, noncallable
lists/seqs, and linear traversal of **100,000** cells and vector views. Caller
arrays/objects remain unfrozen. Quoted generated UTF-16 symbols retain lone
surrogates; executable generated names reject them with macro call-site spans.

A supplemental matrix made **19,771** direct runtime comparisons with **zero
mismatches**, using the same verified Maven runtime described above. It covered
symbol/keyword conversions, first/rest/next/seq, count, indexing, lookup, equality,
conj and vector assoc across scalar, list, vector, seq and seq-cons operands.
Hash checks assert equality consistency locally, not exact upstream hash numbers.
Two-argument symbol construction deliberately accepts strings and a nullish
namespace only, excluding arbitrary host values accepted by upstream.

Pinned edge behavior retained:

- List `nth` fractions cannot reach exact zero, and return a default or throw.
- String seqs and vector seqs of at most 32 elements are indexed views. Their
  `nth` reads the absolute storage property, so an in-range fraction can yield
  undefined and a negative index on a tail can reach preceding storage. Larger
  vector seqs traverse linearly. A seq-cons can reach an indexed tail after
  consuming its own cells; these rules compose without treating all seqs alike.
- `assoc` permits in-range nonnegative numeric fractions by truncation, appends
  exactly at count, rejects invalid indices, and supplies nil for an odd final
  key. Its missing fixed value follows JavaScript undefined argument binding.
- Symbols parse strings at the first slash after a nonempty namespace, whereas
  keyword string parsing retains its separate full-spelling rules. Conversion
  from symbol to keyword preserves semantic namespace/name and full spelling.

Output-size checks use the unchanged primitive source `42` and repeated nested
quoted literals; they make no optimizer or constant pooling claim. Measured
sources and bytes were:

| Source | UTF-8 emitted bytes |
| --- | ---: |
| `42` | 104 |
| `[1 2]` | 15,385 |
| `[` + `"'[x (a/y 1) :k] "` repeated 1 time + `]` | 15,490 |
| Same, repeated 10 times | 16,489 |
| Same, repeated 100 times | 26,479 |

The runtime source is **15,243 bytes**. Each repeated nested datum adds exactly
**111 bytes**; there is one prelude in every collection compilation unit. The
analyzer test asserts this linear relation, while primitive output remains
byte-for-byte unchanged. The CLI acceptance command returns `true`:

```sh
cargo run --locked --quiet -- --compile --text '(= (conj (list 1) 2 3) [3 2 1])' | \
  node --print 'eval(require("fs").readFileSync(0,"utf8"))'
```

The direct matrix can be reproduced after the earlier reference bootstrap.
Set `LIGHT_RUNTIME_SOURCE` to the absolute first-party source and
`LIGHT_REFERENCE_OUTPUT` to that bootstrap's output directory:

```js
const fs=require('node:fs'),assert=require('node:assert/strict');
process.chdir(process.env.LIGHT_REFERENCE_OUTPUT);require(process.cwd() + '/reference.js');
const rt=(0,eval)(fs.readFileSync(process.env.LIGHT_RUNTIME_SOURCE,'utf8')),c=cljs.core;
const defs=[null,undefined,false,true,0,-0,-1,-.9,.9,1,1.8,2,3,NaN,Infinity,-Infinity,'','abc','0','\ud800','a/b/c',
 {k:[null,'x']},{s:[null,'x']},{s:['a','x']},{s:[null,'a/b/c']},{s:['a','b/c']},
 {v:[]},{v:[10,20,30]},{l:[]},{l:[10,20,30]},{l:[null,false,undefined]},
 {q:{v:[10,20,30]}},{q:'abc'},{q:{v:Array.from({length:33},(_,i)=>i)}},
 {r:{q:'abc'}},{r:{q:{v:[10,20,30]}}},{j:[{q:'abc'},0]},
 {l:[{v:[1,2]},{s:['a','x']}]}];
function build(d,r){if(!d||typeof d!=='object')return d;const f=r?c:rt.core;
 if(d.k)return f.keyword(...d.k);if(d.s)return f.symbol(...d.s);
 if(d.v)return f.vector(...d.v.map(x=>build(x,r)));if(d.l)return f.list(...d.l.map(x=>build(x,r)));
 if(d.q)return f.seq(build(d.q,r));if(d.r)return f.rest(build(d.r,r));
 if(d.j)return f.conj(...d.j.map(x=>build(x,r)));throw Error('bad def');}
function encode(x,r){if(x===undefined)return ['undefined'];if(x===null)return ['nil'];if(typeof x==='number')return ['number',Object.is(x,-0)?'-0':String(x)];if(typeof x!=='object')return [typeof x,x];
 let data;
 if(r){if(x instanceof c.Keyword)data={kind:'keyword',namespace:x.ns,name:x.name,spelling:x.fqn};else if(x instanceof c.Symbol)data={kind:'symbol',namespace:x.ns,name:x.name,spelling:x.str};else if(c.sequential_QMARK_(x)) { const items=[];for(let s=c.seq(x);s!==null;s=c.next(s))items.push(c.first(s));data={kind:c.vector_QMARK_(x)?'vector':x instanceof c.List||x instanceof c.EmptyList?'list':'seq',items};}}
 else data=rt.inspect(x);
 if(!data)throw Error('unexpected value');return data.items?[data.kind,data.items.map(v=>encode(v,r))]:[data.kind,encode(data.namespace,r),encode(data.name,r),data.spelling];}
let checks=0,bad=0;
function check(name,args){const outcome=r=>{try{const f=(r?c:rt.core)[r?({contains:'contains_QMARK_',equal:'_EQ_'})[name]||name:name];return ['ok',encode(f(...args.map(x=>build(x,r))),r)];}catch(e){return ['error',e.name]}};checks++;const a=outcome(false),b=outcome(true);if(JSON.stringify(a)!==JSON.stringify(b)){if(bad++<20)console.log(name,JSON.stringify(args),JSON.stringify(a),JSON.stringify(b));}}
for(const name of ['count','name','namespace','keyword','symbol','seq','first','rest','next']){check(name,[]);for(const x of defs){check(name,[x]);if(!['keyword','symbol'].includes(name))check(name,[x,42]);}}
for(const name of ['nth','get','contains','equal'])for(const x of defs)for(const y of defs){check(name,[x,y]);if(name==='nth'||name==='get')for(const fallback of [null,undefined,false,'NF'])check(name,[x,y,fallback]);}
for(const x of defs)for(const y of defs)check('conj',[x,y]);
for(const v of [{v:[]},{v:[10,20,30]},{v:Array.from({length:33},(_,i)=>i)}])for(const key of defs)for(const value of [null,undefined,9])check('assoc',[v,key,value]);
for(const ns of [null,undefined,'','a','\ud800'])for(const name of ['','x','b/c','\udc00']){check('symbol',[ns,name]);check('keyword',[ns,name]);}
console.log({checks,bad});if(bad)process.exitCode=1;
```

## Structural maps (2026-10-09)

The map milestone passes **410** typed source outcomes, **48** generated cases,
and **124** analyzer boundaries against the same Maven 1.12.134 compiler. All
seven opt-in reference tests pass. The exact user source returns
`[[1 2] [1 2 3] :pair true]` through the full typed encoder. Map normalization
sorts complete encoded entries only after entry/count/duplicate checks. No
ordinary sequence, user trace or language map is sorted.

The independent runtime assertions include actual `Aa`/`BB` polynomial-hash
collisions (also nested vector/map keys), nested separately built equivalent
keys, keyword/string/symbol distinction, callable maps/names, nil/false/undefined
presence, frozen wrappers/prototypes/payloads, shared unchanged nested values,
invalid-entry failure isolation, and every retained version of 512 updates.
Reader identity checks distinguish integer from float tokens and retain named
namespace/name parts before full-spelling runtime equality,
and generated map tests retain 32 ordered key/value effects through owned Form
children, even beyond the native small-map representation.

Three source/storage distinctions are intentional and tested, rather than hidden
by observation normalization:

- The pinned direct `hash-map` inliner can emit separate key/value arrays:
  `(hash-map (observe :a) (observe 1) (observe :a) (observe 2))` traces
  `[:a :a 1 2]`. The bounded compiler emits ordinary call order `[:a 1 :a 2]`.
  A saved first-class `hash-map` value has the shared ordinary ABI and is used
  for differential effect checks.
- Pinned unchecked map literals can retain reader-distinct constants that become
  the same Number key. `(let* [m {1 :int 1.0 :float}] [(count m) (get m 1)])`
  returned `[2 :int]`; checked first-party construction returns `[1 :float]`,
  preserving the required last-write behavior. Typed observation would detect
  duplicate encoded keys if such an unchecked reference map were serialized.
- Pinned hash maps canonicalize undefined keys to nil, while array maps can
  retain undefined. The first-party representation consistently retains the
  first equivalent key. Nil/undefined lookup/equality/hash agree, and stored
  undefined values remain distinct from nil/missing in typed observations.

A supplemental **3,938** direct runtime comparisons have zero mismatches in the
shared slice: paired construction, lookup, contains, map calls, count, updates,
conj and equality across scalar/sequential/nested map keys and nil/undefined
**values**. Undefined-key presentation and missing fixed assoc arguments are
outside this matrix, as documented above. Public hash numbers are not compared.
The runtime source is 19,391 bytes; primitive output stays 104 bytes.
[Sizes 8/64/512](../../benchmarks/results/compiler-2026-10-09/maps-smoke/README.md)
record resident scaling with raw samples and reproducible commands, without a
speedup claim.

After the earlier verified reference bootstrap, set `LIGHT_RUNTIME_SOURCE` and
`LIGHT_REFERENCE_OUTPUT` as in the preceding matrix and run this direct probe:

```js
const fs=require('node:fs'), assert=require('node:assert/strict');
const rt=(0,eval)(fs.readFileSync(process.env.LIGHT_RUNTIME_SOURCE,'utf8'));
const referenceRoot=process.env.LIGHT_REFERENCE_OUTPUT;
process.chdir(referenceRoot);
require(referenceRoot+'/reference.js');
const c=cljs.core;
const key = name => ({k:name});
const vector = (...items) => ({v:items});
const list = (...items) => ({l:items});
const map = (...items) => ({m:items});
function build(x,r) {
 if (!x || typeof x!=='object') return x;
 if ('k' in x) return r?c.keyword(x.k):rt.core.keyword(x.k);
 if ('s' in x) return r?c.symbol(x.s):rt.core.symbol(x.s);
 if ('v' in x) return (r?c.vector:rt.vector)(...x.v.map(v=>build(v,r)));
 if ('l' in x) return (r?c.list:rt.core.list)(...x.l.map(v=>build(v,r)));
 if ('m' in x) return (r?c.hash_map:rt.core.hashMap)(...x.m.map(v=>build(v,r)));
 throw Error('bad input');
}
function encode(x,r) {
 if (x===undefined) return ['undefined'];
 if (typeof x==='number') return ['number',Number.isNaN(x)?'nan':Object.is(x,-0)?'-0':x];
 if (x===null||typeof x==='boolean'||typeof x==='string') return x;
 let data;
 if (!r) data=rt.inspect(x);
 else if(c.keyword_QMARK_(x)) data={kind:'keyword',spelling:x.fqn};
 else if(c.symbol_QMARK_(x)) data={kind:'symbol',spelling:x.str};
 else if(c.map_QMARK_(x)) { const entries=[]; for(let s=c.seq(x);s;s=c.next(s)) {const e=c.first(s);entries.push([c.nth(e,0),c.nth(e,1)]);}data={kind:'map',entries}; }
 else {const items=[];for(let s=c.seq(x);s;s=c.next(s)) items.push(c.first(s));data={kind:c.vector_QMARK_(x)?'vector':x instanceof c.List||x instanceof c.EmptyList?'list':'seq',items};}
 if(data.kind==='map') return ['map',data.entries.map(e=>e.map(v=>encode(v,r))).sort((a,b)=>JSON.stringify(a).localeCompare(JSON.stringify(b)))];
 if(data.spelling!==undefined)return [data.kind,data.spelling];
 return [data.kind,data.items.map(v=>encode(v,r))];
}
let checks=0;
const mismatches=[];
function check(name, args) {
 function outcome(r){try {const values=args.map(v=>build(v,r)); const f=r?c[({hashMap:'hash_map',contains:'contains_QMARK_',equal:'_EQ_'})[name]||name]:rt.core[name];const out=name==='call'? values[0].call(null,...values.slice(1)):f(...values); return ['ok',encode(out,r)];}catch(e){return ['error',e.constructor.name];}}
 const a=outcome(false), b=outcome(true);checks++;
 try{assert.deepEqual(a,b)}catch {mismatches.push({name,args,a,b});}
}
const keys=[null,false,0,-0,1,1.5,'1','Aa','BB',key('a'),key('ns/a'),{s:'a'},vector(1,2),list(1,2),map(key('a'),vector(1)),map(key('a'),list(1))];
const maps=[map(),map(key('a'),1),map(key('a'),null,key('f'),false,key('u'),undefined),map(vector(1,2),'pair',map(key('a'),vector(1)),'nested'),map('Aa',1,'BB',2),map(null,1,'1',2,1,3)];
for(const m of maps){
 for(const name of ['count','dissoc','conj','call'])check(name,[m]);
 for(const k of keys){
  for(const name of ['get','contains','call','dissoc'])check(name,[m,k]);
  for(const v of [null,undefined,false,9,key('v'),vector(1,2),map(key('a'),1)]){
   for(const name of ['get','call','assoc'])check(name,[m,k,v]);
   check('hashMap',[k,v]);
   check('assoc',[m,k,v,key('tail')]);
  }
 }
 for(const other of [...maps,vector(),list(),null,undefined,1])check('equal',[m,other]);
 for(const entry of [vector(),vector(key('a')),vector(key('a'),1),vector(key('a'),1,2),list(vector(key('a'),1)),list(vector(key('a'),1),vector(key('b'),2)),list(list(key('a'),1)),list(vector(key('a'),1),null),...maps,null,false,1])check('conj',[m,entry]);
}
for(const name of ['dissoc','hashMap'])check(name,[]);
for(const x of mismatches.slice(0,15)) console.log(JSON.stringify(x));
console.log(JSON.stringify({checks,mismatches:mismatches.length}));
if(mismatches.length)process.exit(1);
```


## Structural sets (2026-10-09)

The set milestone passes **460** compiled typed outcomes (50 new set cases),
**48** generated programs and **130** analyzer boundary classifications through
the existing pinned Maven ClojureScript 1.12.134 launcher. All seven reference
checks passed (the program comparison reran after correcting explicit source
optimizer distinctions and adding the undefined-disj probe). The set adapter
feeds the common recursive encoder; count, duplicate complete encodings and
cycles are checked before only the set member encodings are sorted. Sequence
results and effect traces retain their order.

Independent runtime assertions cover first retained vector/list representatives,
nil/false/undefined presence, callable 1/2 arities, ordered construction failures,
actual string/nested-set hash collisions, immutable wrappers/prototypes/storage,
host mutation resistance, shallow nested sharing, 512 retained versions and
150,000 duplicate input members without host argument-stack expansion. Set
sequence checks count each visited member without requiring an iteration order.
Compiler macro tests retain 16 source-ordered effects and duplicate executable
operands; literal/quoted duplicates fail before native macro expansion, including
metadata-wrapped input that the macro would otherwise discard. Ordinary native
reader duplicate behavior remains unchanged and its reader suites pass.

Pinned source distinctions, observed rather than normalized away:

- The reference reader rejects `#{(observe 1) (observe 1)}` before compilation.
  The bounded compiler deliberately evaluates both operands, records `[1,1]`,
  and retains one member. Literal and quoted duplicate data still fail locally.
- `(do #{(observe 1) (observe 2)} 3)` and a function returning that set trace
  `[2,1]` in the pinned compiler, owing to reader set traversal. This compiler
  preserves `[1,2]` source operand order. A literal containing a throwing member
  likewise has a different pinned failure frontier. Dedicated independent
  tests assert these exact sources; differential effects use ordinary constructors.
- Direct `(hash-set 1 1.0)` and `(hash-set 0 -0.0)` each have count 2 with the
  pinned unchecked inliner. Saved first-class `hash-set` calls and this runtime
  both collapse each pair to count 1. The direct sources have independent tests;
  comparisons do not serialize or hide duplicate reference member encodings.
- Retained undefined member presentation can depend on upstream array-map/hash-map
  storage, as described for maps. Shared tests use nil first or scalar projections;
  local checks verify consistent first-member retention. `(disj undefined)` keeps
  undefined; `(disj undefined 1)` returns nil in both runtimes.

A supplemental **4,436** direct runtime comparisons have **zero mismatches** for
set construction, membership/defaults, callable arities, equality, conj/disj,
sequential representatives, nested map/set keys, collisions and nullish removal.
The matrix compares typed structures and ordinary errors, not exact hash integers
or unspecified traversal order. Reproduce after a verified reference bootstrap
using `LIGHT_RUNTIME_SOURCE` and `LIGHT_REFERENCE_OUTPUT` as above:

```js
const fs=require('node:fs'),assert=require('node:assert/strict');
const source=process.env.LIGHT_RUNTIME_SOURCE, root=process.env.LIGHT_REFERENCE_OUTPUT;
const rt=(0,eval)(fs.readFileSync(source,'utf8'));
process.chdir(root);require(root+'/reference.js');const c=cljs.core;
function build(x,r) {
 if (!x || typeof x!=='object') return x;
 const f=r?c:rt.core;
 if ('k' in x) return f.keyword(x.k);
 if ('s' in x) return f.symbol(x.s);
 if ('v' in x) return f.vector(...x.v.map(v=>build(v,r)));
 if ('l' in x) return f.list(...x.l.map(v=>build(v,r)));
 if ('q' in x) return f.seq(build(x.q,r));
 if ('m' in x) return (r?c.hash_map:f.hashMap)(...x.m.map(v=>build(v,r)));
 if ('t' in x) return (r?c.hash_set:f.hashSet)(...x.t.map(v=>build(v,r)));
 throw Error('bad input');
}
function encode(x,r) {
 if(x===undefined)return ['undefined'];if(x===null)return ['nil'];
 if(typeof x==='number')return ['number',Object.is(x,-0)?'-0':String(x)];
 if(typeof x==='boolean'||typeof x==='string')return [typeof x,x];
 let d;
 if(!r)d=rt.inspect(x);
 else if(c.keyword_QMARK_(x))d={kind:'keyword',spelling:x.fqn};
 else if(c.symbol_QMARK_(x))d={kind:'symbol',spelling:x.str};
 else if(c.map_QMARK_(x)){const entries=[];for(let q=c.seq(x);q;q=c.next(q)){const e=c.first(q);entries.push([c.nth(e,0),c.nth(e,1)]);}d={kind:'map',entries};}
 else {const items=[];for(let q=c.seq(x);q;q=c.next(q))items.push(c.first(q));d={kind:c.set_QMARK_(x)?'set':c.vector_QMARK_(x)?'vector':x instanceof c.List||x instanceof c.EmptyList?'list':'seq',items};}
 if(d.spelling!==undefined)return [d.kind,d.spelling];
 const sort=xs=>xs.sort((a,b)=>JSON.stringify(a)<JSON.stringify(b)?-1:JSON.stringify(a)>JSON.stringify(b)?1:0);
 if(d.kind==='map')return ['map',sort(d.entries.map(e=>e.map(y=>encode(y,r))))];
 const items=d.items.map(y=>encode(y,r));return [d.kind,d.kind==='set'?sort(items):items];
}
let checks=0;const mismatches=[];
function check(name,args){function outcome(r){try{const vs=args.map(v=>build(v,r));const fn=r?c[({hashSet:'hash_set',hashMap:'hash_map',contains:'contains_QMARK_',equal:'_EQ_'})[name]||name]:rt.core[name];return ['ok',encode(name==='call'?vs[0].call(null,...vs.slice(1)):fn(...vs),r)];}catch(e){return ['error',e.constructor.name];}}
 const a=outcome(false),b=outcome(true);checks++;try{assert.deepEqual(a,b)}catch{mismatches.push({name,args,a,b});}}
const keys=[null,false,true,0,-0,1,1.5,'0','Aa','BB',{k:'a'},{s:'a'},{v:[1,2]},{l:[1,2]},{q:{v:[1,2]}},{m:[{k:'a'},1]},{t:[1,2]},{t:[{m:[{k:'a'},1]}, {v:[1,2]}]}];
const sets=[{t:[]},{t:[null,false]},{t:[1,2]},{t:['Aa','BB']},{t:[{v:[1,2]},{m:[{k:'a'},1]},{t:[3,4]}]},{t:[{s:'a'},'a',{k:'a'}]}];
for(const s of sets){for(const name of ['count','disj','conj','call'])check(name,[s]);
 for(const k of keys){for(const name of ['get','contains','call','disj','conj','hashSet','equal'])check(name,[s,k]);
  for(const v of [null,undefined,false,9,{v:[1,2]},{t:[1,2]}]){for(const name of ['get','call'])check(name,[s,k,v]);check('disj',[s,k,v]);check('conj',[s,k,v]);}
 }
 for(const other of [...sets,{v:[]},{l:[]},{m:[]},null,false])check('equal',[s,other]);
 check('call',[s,1,2,3]);check('count',[s,1,2]);
}
for(const a of keys)for(const b of keys){check('hashSet',[a,b]);check('equal',[{t:[a,b]},{t:[b,a]}]);check('get',[{m:[{t:[a,b]},'nested']},{t:[b,a]}]);}
for(const name of ['disj','hashSet'])check(name,[]);
for(const v of [null,undefined,1,false,{v:[]},{m:[]}]){check('disj',[v]);check('disj',[v,1]);}
console.log(JSON.stringify({checks,mismatches:mismatches.length,examples:mismatches.slice(0,15)},null,2));if(mismatches.length)process.exitCode=1;
```

Runtime source size is **21,992 bytes**; primitive `42` output remains **104 bytes**.
[Resident set measurements](../../benchmarks/results/compiler-2026-10-09/sets-smoke/README.md)
retain all construction/lookup/update/seq samples at sizes 8/64/512. They describe
the deliberate linear search/copy and quadratic bulk construction, not a speedup.
The CLI acceptance command returns `1`:

```sh
cargo run --locked --quiet -- --compile --text '(count (hash-set [1 2] (list 1 2)))' |
  node --print 'eval(require("fs").readFileSync(0,"utf8"))'
```

## Finite reductions (2026-10-09)

The pinned Maven ClojureScript **1.12.134** comparison passes **517** complete
compiled outcomes, traces and stdout records: **57** new reduction/Number/Reduced
cases join the previous 460. All seven existing opt-in reference tests pass,
including **48** generated programs, **130** analyzer boundary classifications
and the namespace programs. Settings remain `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`; Node is v24.18.0, Clojure 1.12.4,
Java 25.0.2. Fixed/overloaded arity warnings are retained reference evidence,
not compilation or runtime success shortcuts. Fixture origins are independent,
hand-authored cases in `fixture-origins.tsv`.

The pinned `cljs/core.cljs` definitions of `reduce`, `ci-reduce`, `array-reduce`,
`+`, `Reduced`, `reduced`, `reduced?`, `unreduced` and `deref` were inspected from
the 1.12.134 Maven jar. The implementation is original first-party JavaScript;
it adds no protocol system, third-party runtime or native interpreter callback.

Compared behavior includes omitted/explicit init; every empty source kind;
singleton callback elision; ordered steps and abrupt exceptions; one-layer early
exit and nested markers; saved/qualified/aliased/shadowed core functions; reducer
redefinition during execution; keyword/symbol/map/set/vector callbacks; fixed
user missing/extra parameters; returned closures and collections; map entries,
sets, mixed list/seq tails and UTF-16 units. Number cases cover all + arities,
rounding beyond the safe literal boundary, negative zero, NaN and overflow.
No new differential normalization or reduction-specific source exception was
needed. Host arrays remain outside the declared input domain despite reference
support; general deref, protocols and transducers remain excluded.

Reduced wrappers have private weak branding, frozen wrappers/prototypes and
identity equality. Backend adapters report raw markers as opaque to the shared
encoder. Independent transport checks reject direct, nested and traced markers
outside the compiled-case catch, preserving harness failure rather than encoding
an empty object/map. Fixtures use `reduced?`, `deref`, `unreduced` and identity
observations. Independent runtime assertions additionally read an effectful
`.call` getter on every step, preserve exact thrown-object identity, prove no
completion invocation, and reject forged/inherited marker brands.

The focused compiler suite passes **103** tests, with the seven reference tests
run separately. Independent tests execute 150,000-element vectors, lists, seq
views and strings; the emitted JavaScript test sums 150,000 elements without
recursive traversal. A GC probe observes zero retained payloads after 256
throwing map reductions. [Resident prebuilt-vector samples](../../benchmarks/results/compiler-2026-10-09/reduce-smoke/README.md)
exclude compilation, source generation, startup and collection construction.
Primitive `42` output remains **104 bytes**. The CLI command returns **6**:

```sh
cargo test --locked --test execution --test analyzer --test compiler_macros --test namespaces --test boundaries --test provenance
LIGHT_CLJS_REFERENCE=release cargo test --locked --test execution -- --ignored --nocapture
cargo run --locked --quiet -- --compile --text '(reduce + 0 [1 2 3])' |
  node --print 'eval(require("fs").readFileSync(0,"utf8"))'
```

## Complete macro and namespace collection program (2026-10-09)

The original four-file `batch.data`, `batch.ops`, `batch.main`, and
`batch.macros.clj` program is checked in under both `examples/collections` and
`tests/fixtures/namespaces/collections`. A fast test requires byte-identical
sources. It compiles through the existing dependency loader and native macro
host without a new runtime or loader path.

The program returns `[[1 2] [1 2 3] :pair true]`, and its first trace value is
`report`'s `[6 batch/data true "batch.data"]`. The shared typed encoder checks
these values, quoted generated nested vector/list/map/set/symbol/keyword data,
source/generated equality and ordered map/key/item evaluation. Initializer
stdout is exactly `data\nops\nmain\n`; `data` is reached through both main and
ops, and its initializer is macro-generated. Macro statistics confirm one
initializer expansion per compilation. Two builds with a reused macro host,
two emitted units in one JS process, and fresh CLI/Node processes preserve
separate runtime globals and immutable results. Production output installs no
global observer or runtime; tests capture the lexical runtime only for encoding.

Reference program mode now accepts an optional exported function name. A
reference-only generated entry installs the same shallow adapter as individual
cases and exposes the selected function. Loading occurs inside the existing
case runner so initialization stdout is captured. The launcher adds explicit
`.clj` source roots to `-Sdeps :paths`; merely passing the `.cljs` build root had
failed with `Could not locate batch/macros` during verification. Both normal and
observed program paths now pass. Release/checkout identity and effective-setting
checks, per-process state, timeouts and retained artifacts remain in place.

New rejection tests cover lazy sequences, record/protocol forms, multi-arity
functions, unsafe integer output and regex construction. Native regex reading
already rejects regex data; a macro attempting `read-string` reports its outer
call location and macro name. This exposed an unreadable native macro identity
in the prior compiler diagnostic; the analyzer now prefixes expansion failures
with the actual operator name. File/line/byte spans and empty CLI stdout remain
asserted. Cancellation fails compilation and a cleared token permits reuse of
the same host. Core-qualified generated operations preserve resolution under
lexical/global shadowing and a runtime/macro alias collision.

Verification on Maven ClojureScript **1.12.134**, Clojure **1.12.4**, Java
**25.0.2**, and Node **v24.18.0**, with `:optimizations :none`, `:static-fns false`
and `:def-emits-var false`: **115** focused tests and all **8** reference checks
passed. The reference checks include **517** individual outcomes, **48**
generated cases, **130** boundary classifications and the new complete program
with two fresh-process executions. No supplemental checkout run is claimed.

The runnable example emits **27,352 bytes** including the CLI newline and one
**24,090-byte** runtime source. The rest is **3,262 bytes** of emitted program and
helpers. Primitive `42` output remains **104 bytes**. These are packaging/size
checks, not throughput measurements; the separate benchmark pellet owns costs.

```sh
cargo test --locked --test execution --test compiler_macros --test namespaces --test analyzer --test boundaries --test cli --test provenance
LIGHT_CLJS_REFERENCE=release cargo test --locked --test execution -- --ignored --nocapture
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked --quiet -- --compile --entry batch.main --source-root examples/collections > /tmp/light-cljs-collections.js
node --check /tmp/light-cljs-collections.js
node -e 'const run=eval(require("fs").readFileSync("/tmp/light-cljs-collections.js","utf8")); const result=run(x=>x); if(result==null) throw Error("missing result");'
```

## Set integration update — 2026-10-10

The earlier duplicate-executable-form divergence above is superseded. Readable
identical set member forms now reject before evaluation in ordinary and macro
input, matching both pinned readers. Distinct forms whose evaluated results are
structurally equal throw Error after source-order evaluation, matching the pinned
checkout. Release 1.12.134 returns a deduplicated set; the two old independent
execution probes retain those explicit versioned expectations. Constructor
`hash-set`/`set` deduplication is unchanged. Hash-set no-op `conj`/`disj` now preserve
identity, metadata and the retained representative, checked independently in Node
and against the original source assertions in both references.
