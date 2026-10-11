# Public core protocol identity and dispatch evidence

The implementation exposes `Inst`, `IDerefWithTimeout`, `IUUID`, `ASeq`,
`inst-ms*` and `-deref-with-timeout` through Rust analysis/lowering and the
canonical first-party JavaScript runtime. `inst?` uses `satisfies? Inst`,
`inst-ms` invokes `inst-ms*`, and `uuid?` checks direct `IUUID` implementation.
Native marker extensions satisfy a protocol without implying direct
implementation. Date's built-in millisecond method takes precedence over a
native/default object extension. `deref` retains its one-argument contract.

`ASeq` follows the pinned marker declarations: IndexedSeq (including small
vector/string sequences), nonempty List, Cons, ChunkedCons and the runtime's
NodeSeq representation. EmptyList, ranges, ChunkedSeq, lazy and sorted-map
sequences are not universally marked merely because they implement ISeq.

## Exact executing evidence

`compiler-public-protocols.py --reference both --artifacts FRESH_DIRECTORY`
executes eight independent programs with the identity/settings checked
`execution.clj` harness: Maven ClojureScript **1.12.134** and clean checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`. All eight returned their expected
`true` value, with empty stdout and trace, on both references. The committed
[receipt](compiler-public-protocols-receipt.json) retains exact observations,
pins/settings and fixture/runtime hashes. Raw builds and process logs from this
run are disposable under `/tmp/light-cljs-138-pinned-final`.

Six independent contracts assert all four original missing-name probes,
user types/reify, saved/apply methods, `this-as` receiver identity, protocol
identity, native/nil/default extensions, Date/invalid-Date dispatch, direct
marker slots, collection marker membership and negative dispatch. The remaining
two programs retain the complete upstream `core_test.cljs::test-uuid` and
`predicates_test.cljs::test-preds` bodies. The latter includes its entire
`pred-val-table` definition, every row, nested doseq/dotimes and the original
expected predicates/message; its sole auto-qualified keyword is explicitly
qualified to the original source namespace. `is` becomes `assert`, `testing`
becomes `do`, and deftest supplies a boolean result. The adapter verifies full
body/setup spans against pinned Git bytes and SHA-256; original copyright/EPL
notices are retained. No assertion, setup or iteration is removed.

`public_protocol_api_arities_and_macro_phase_execute` runs all eight programs
in both QuickJS and Node applications and as canonical QuickJS macro
computations. It also verifies source metadata, method arglists and Var roots,
and demonstrates separate macro/application mutable counters. Separate tests
require located implementation-arity/method errors and unsupported protocol
instance/function expansion-result diagnostics, then recover using the same
MacroHost. Eligible computations return persistent data; the expansion-result
domain is unchanged.

## Ledger and regression boundary

The pinned-source metadata generator adds exactly six declaration rows.
Independent source obligations, registrations and ownership are regenerated;
18 public/local declaration rows bind exact body/signature hashes to the complete
named application/macro test and both support-file hashes. Existing UUID source
witnesses remain authoritative; the complete previously unwitnessed predicate
body receives its original assertion-site witness. Generated registration and
static ledger freshness alone grant no pass.

The protocol, type, reify, extension, core metadata, language-accounting and
new public-protocol regression targets passed (25 Rust tests); provenance also
passed after correcting its existing namespace test rename and registering all
new fixture identities. Direct canonical runtime assertions passed, along with
all-target/all-feature Cargo checking and pinned metadata/inventory generation
checks. Repository-wide rustfmt drift already exists and was not reformatted;
the new Rust test is formatted and `git diff --check` passed.

## Performance observations

The normal integrated language benchmark validated the complete
value/effect/stdout outcome on nine fresh Node processes. The retained raw
measurement is `benchmarks/results/compiler-2026-10-11/public-protocols/output-startup.json`:
302,429 runtime bytes (72,819 gzip), 313,806 integrated bytes (75,302 gzip),
14.351 ms median parse/initialize/execute total. Primitive `42` output remains
88 bytes and omits the runtime. These are fresh unpaired observations, not
claimed thresholds or a paired speed comparison.

The twenty-iteration evaluator benchmark validates fresh sessions, reused
sessions, and bounded-loop reset/recovery. Raw nanosecond totals are retained
in `benchmarks/results/compiler-2026-10-11/public-protocols/evaluator.tsv`.

This completes the focused public protocol gap. Public constructor/source-file
owners and the unfiltered zero-missing language gate remain open; this evidence
makes no whole-language completion claim.
