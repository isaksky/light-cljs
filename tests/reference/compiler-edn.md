# Canonical EDN and explicit stream readers

The reader algorithms, cursors, local option maps, tag registrations and result
validation execute in the same first-party JavaScript runtime used by QuickJS,
Node and compiled macro helpers. `cljs.reader` and `clojure.edn` are compiler-owned
source facades loaded by the ordinary namespace resolver. No interpreter API is
used by the new fixtures or Rust input-provider adapter.

## Profiles and retained behavior

`clojure.edn/read-string` preserves the previously supported strict compatibility
profile: nil/false input returns nil; absent options return nil at EOF; explicit
options without `:eof` throw at EOF. Options are limited to `:eof`, `:readers` and
`:default`; `:readers` uses symbol keys. Registrations from the embedding or
`cljs.reader` do not alter a strict call's local readers. Tagged handlers are
called once, including tagged forms inside `#_` discarded data. Handler exceptions
retain identity. Results must be finite branded data, with bounded metadata;
functions, atoms, delays, lazy sequences, records, protocol instances and arbitrary
host objects are rejected without realizing lazy values. The strict profile
accepts metadata and explicit namespaced maps but rejects executable prefixes,
read-eval, regex, reader conditionals, auto-resolution, JVM classes and implicit
built-in host-valued tags.

`clojure.edn/read` accepts a real stream at one argument, an options map and
stream at two arguments, and `[stream eof-error? eof opts]` at four arguments.
One argument returns nil at EOF. Two arguments throw unless `:eof` is supplied.
At four arguments, a truthy error flag throws at EOF; nil/false selects the exact
EOF value, overriding any `:eof` in the options. Readers and default handlers
remain local to strict EDN options. The supported zero-argument embedding reads
the explicit current input and throws at EOF; it discovers no host input. A
one-argument map is an invalid stream, including inside `with-in-str`.

`cljs.reader` supplies the CLJS data reader facade and local/global tag parser
functions. It adds `#inst`, `#uuid`, `#queue` and `#js` literals and returns nil for
empty strings. Instants use UTC defaults, validate calendar fields, apply explicit
offsets and truncate fractional milliseconds. The public
`parse-and-validate-timestamp` helper returns the eight validated fields, including
offset minutes; `parse-timestamp` preserves JavaScript `Date.UTC` behavior for
years 0–99. Public dynamic `*tag-table*` and `*default-data-reader-fn*` atoms support
deref, reset, registration and binding. Facade calls pass the complete current
table/default directly, so restored/bound tables never fall back to stale runtime
registrations; tags absent from those tables need a local/default handler.
Core `read-string`/`read` retain the
code-reader profile with quotes, dereference/Var prefixes, regex, anonymous-function
shorthand, syntax quotation/unquotation, `:cljs` reader conditionals and splicing.
Skipped conditional branches do not invoke tag handlers. Unqualified syntax-quoted
symbols default to `user`; core names qualify to `cljs.core`. Ambient namespace
reflection/bindings and configurable embedding namespace/alias policies remain
with the broader compiler reader/embedding migration.
Those retained code-reader extensions use canonical runtime registration state;
`cljs.reader` registrations update it for compatibility. Direct facade atom reset
or binding controls facade reads. Strict EDN continues to use only its explicit
local options. No compiler-root service or interpreter namespace lookup is used
by the runtime reader.

`read` supports existing 0/1/2/3/4 arities, exact nil/false EOF values and the same
cursor as `read-line` and EDN reads. `with-in-str` restores the previous explicit
input after normal return or failure and closes the scoped handle; `*in*` exposes its current cursor. Streams
are opaque per-runtime identities. Reentry into an active stream fails with a
located host error; other streams can be read from a tag handler. Malformed reads
fuse that cursor. No filesystem, stdin, network or output provider is discovered.
`cljs.reader/input-provider` accepts a name and synchronous function returning a
nonempty string chunk or nil at EOF. `Evaluator::input_provider` adapts an explicit
Rust callback to this capability; Rust supplies chunks, and JavaScript parses them.
Provider failures expose bounded text and source/line/column/offset/category
properties. Cursor locations use UTF-16 units; CRLF and CR/form-feed partners share
one line break. Closed streams reject further use. The cursor drops consumed
provider chunks while retaining absolute locations.
Source names are capped at 256 UTF-16 units and runtime-owned diagnostic payloads
at 1,024 units before the bounded location suffix; user handler exceptions retain
their original identity.

The explicit bounds are 1 MiB of UTF-16 input units, 100,000 nodes per operation,
and 32 nested reader/result frames. The JavaScript depth bound is chosen before
QuickJS's default stack can overflow, preserving catchable located diagnostics.
Nested reads share depth and the session's sticky cancellation/deadline/fuel.
Validated reader-table conversions are cached weakly only for immutable privately
branded maps; changing an atom's map creates a new identity and is revalidated.

## Provenance and adaptations

The 61 EDN cases selected from `tests/fixtures/sci-outcomes.tsv` retain the native
fixture bodies, exact structural values, ordered effects and empty output
observations. A native top-level `require` remains top-level in the compiled
fixture. Native `Exception` catches become `js/Error`; integers/ratios become
JavaScript Number and character values become one-unit UTF-16 strings. NaNs are
normalized to a marker before structural comparison because JavaScript equality
intentionally treats NaN as unequal. Eighteen core reader/IO fixture cases retain
arities, mixed reads, first-form-only reads and EOF behavior; the feature case
selects `:cljs` rather than the native oracle's `:clj`.

Upstream source: ClojureScript checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, release oracle 1.12.134,
`research/clojurescript/src/test/cljs/cljs/reader_test.cljs`, SHA-256
`68d9e3b617c595360a8b87b595bb39855264c3c2e4d9555ba06afdeaa5852c39`.
`tests/fixtures/compiler-reader-upstream.cljs` preserves all eight complete active
`deftest` bodies, their `T`/`R` setup, comments and every loop from upstream
lines 14–256. Its SHA-256 is
`b14648e7cc6670fdd0962837193d4cdd6a5cf05f38bd5410d6ac5452a59eefba`.
The copied fixture retains upstream's complete copyright and EPL-1.0 notice.
Only the namespace/test harness changes: local `deftest`/`testing` macros create
functions and retain every body; `is` increments a counter and throws on a false
assertion. `goog.object/get` is supplied explicitly by the test embedding. No
active body or assertion is excluded. QuickJS and Node each execute 7,487
assertions, including the complete nested timestamp loops, map representation
checks and exact duplicate-literal diagnostic messages.

The unchanged original namespace was also compiled by the existing
`tests/reference/execution.clj` harness against Maven ClojureScript 1.12.134 and
the verified clean pinned checkout. Both Node reference runs report 8 tests,
7,487 passes, 0 failures and 0 errors. Builds verified `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`; Java 25.0.2 and Clojure 1.12.4.
The checkout reported development version `0.0.135109660`; source identity was
verified by the full pinned Git SHA, not that version string.

`tests/reference/compiler-reader-program.cljs` is the reference-only observer.
For these runs it was copied to `/tmp/light-edn-reference.4Bq1Vp/src/probe/main.cljs`
and unchanged upstream `reader_test.cljs` to `src/cljs/reader_test.cljs`. Exact
build commands (each exited 0):

```sh
clojure -J-Xmx512m -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' -M tests/reference/execution.clj release 1.12.134 - /tmp/light-edn-reference.4Bq1Vp/release program /tmp/light-edn-reference.4Bq1Vp/src probe.main run
clojure -J-Xmx512m -Sdeps '{:deps {org.clojure/clojurescript {:local/root "/Users/admin/src/light-cljs/research/clojurescript"}}}' -M tests/reference/execution.clj checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e /Users/admin/src/light-cljs/research/clojurescript /tmp/light-edn-reference.4Bq1Vp/checkout program /tmp/light-edn-reference.4Bq1Vp/src probe.main run
```

In each respective output directory, the following command exited 0 and printed
`{"type":"end-run-tests","fail":0,"error":0,"pass":7487,"test":8}`:

```sh
node -e 'require("./reference.js"); const result = globalThis.lightProgramRun(); console.log(JSON.stringify(result)); if (result.fail || result.error) process.exit(1);'
```

The additional first-party fixture
`tests/fixtures/compiler-reader-public-contracts.cljs` tests both timestamp public
functions, field vectors, leap seconds, fractional truncation, offsets, year 0–99
conversion, invalid dates, direct atom mutation, parser/default registration and
dynamic binding/restoration. `tests/reference/compiler-reader-public-program.cljs`
retains that complete body and changes only its namespace/function observer. It
was copied to the same reference source directory as `probe/public.cljs`; both
references were built with the commands above substituting `probe.public` and
output directories `public-release`/`public-checkout`. Each build exited 0 with the
same verified settings. Running `node -e 'require("./reference.js");
console.log(globalThis.lightProgramRun());'` in each output directory exited 0 and
printed the exact value used by the compiled QuickJS/Node assertion:

```clojure
[[2012 2 29 23 59 60 123 -330] [7 1 1 0 0 0 0 0] true true [true true true true true] [5 [unknown 8] 3 8 true true true true [10 [other 7]] true true true] true true]
```

## Verification

`cargo test --test compiler_edn` runs values/effects in QuickJS and Node and
compiled macros, plus direct Rust provider, bounded result, depth/input limit,
callback reentry, cancellation and sticky terminal-control tests.
The final reader suite passes all 14 tests, including scoped handle closure and
nested exception restoration, public reader EOF arities/defaults, built-in parser
registration replacement/restoration, local parser precedence, public timestamp
helpers, dynamic parser atoms in compiled macros and applications, and named-stream
built-in diagnostic locations. The complete-body harness uses an explicit bounded
60-second session deadline for its 7,487 assertions and source facade operations;
production defaults and provider/node/depth/fuel limits remain unchanged.
