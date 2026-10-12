# Canonical character/string literal collections

The unchanged source owned by Pellet 174 is:

```clojure
[(count {\a 1 "a" 2}) (get {\a 1 "a" 2} \a)
 (count #{\a "a"}) (contains? #{\a "a"} "a")]
```

Both independently executed ClojureScript pins, release `1.12.134` and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, return `[2 1 2 true]` under
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`. The earlier
case receipt's neighboring literal mismatch remains unchanged as historical
evidence. This proof grants no historical SCI body or comprehensive language
credit.

`literal_char_string.py` runs fresh builds and observations against both pins,
then builds and invokes the actual Rust source driver. Every complete source is
compiled as an ordinary namespace application and an actually demanded macro
computation, in fresh and reused MacroHosts, and executed in QuickJS and Node.
The macro evaluates the expression and quotes its result; it does not substitute
a reviewed expected value. Reused hosts start with a demanded warmup macro
computing `(+ 20 22)`. Eleven true duplicate or malformed sources each reject in
both pins and all four local source/lifecycle contexts, followed on the same
host by a real recovery macro returning 42 in both engines. Existing map lowering through `defn` or a demanded macro reports
`DuplicateMapKey`; direct ordinary analysis reports `duplicate map key`. The
proof retains the exact error emitted for each complete source context.

The reviewed contracts were derived from fresh whole-source compilation and
execution in both independent pins before local expectations. They cover map
source order, count, lookup, membership, keys, values and sequence; set count,
membership, lookup and finite sequence; quoted and nested literals; Unicode and
named character spellings; executable map values with ordered atom effects and
captured print output; eight/nine-pair map construction boundaries; more than
eight scalar set members; executable vector map keys and distinct vector set
members; quoted vector/map/set keys and members; and quote/list data inside a
quoted collection. Map effects must be exactly `[:first :second]`, with captured
output exactly `"2\n11\n"`. Every complete typed packet also compares observer
trace and stdout exactly, including their empty values in demanded macro frames.

The canonical behavior arises from source identity surviving the reader while
characters lower to JavaScript strings. A small validated constant literal can
retain two equal lowered keys or members. A lookup then finds the first map
entry for either spelling. Reversed source order reverses the first value.
Larger literal maps use a deduplicating construction path: eight pairs retain
both entries, while nine pairs yield eight entries and the later value. Quoted
composite keys and members remain constant data; unquoted executable composite
keys use runtime construction. Quote syntax nested inside an already quoted
collection remains list data.

Five neighboring contracts are explicitly nonpassing and grant zero canonical
agreement credit:

* Runtime `array-map` and small `hash-set` preserve lowered duplicates in both
  upstream pins. The existing local constructors deduplicate. The separate
  accepted `hash-map` and `set` control requires runtime deduplication.
* Dynamic duplicate sets and executable vector-member duplicate sets return a
  singleton in the release pin and raise `Duplicate key` in the checkout pin.
  The retained local diagnostic behavior matches the checkout; both pin packets
  remain independently recorded.
* Numeric-only literals `{1 :int 1.0 :float}` and `#{1 1.0}` preserve both reader
  identities in both pins. Existing local behavior returns one later-valued map
  entry and one set member. Numeric-only construction is outside this repair.
* A quoted set containing both equal lowered character/string members and inner
  quote data has canonical hash traversal order differing from the local
  insertion order. Its complete finite sequence is recorded as nonpassing;
  a separate accepted control proves quote identity, count and membership.

The replayer validates current compiler/producer/driver sources, reviewed
contract hash, both checked pins and settings, exact executable hashes and
commands, complete namespace sources, emitted JavaScript and raw engine
observations, artifact census, all eight frames per source, and same-host
recovery artifacts. Missing, extra, modified, stale or detached artifacts fail
closed. The saved receipt is a retained observation; it is never a fallback for
the raw executable proof.

Focused commands:

```sh
cargo test --locked --test compiler_literal_char_string canonical_literal_constants_and_same_host_diagnostic_recovery -- --exact
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_literal_char_string.py
python3 -B tests/reference/compiler-language/literal_char_string.py --output /tmp/light-cljs-174-literal-delivery-v2
```

The Rust regression covers the reviewed application and demanded macro
computations plus diagnostic recovery. The ignored test
`literal_char_string_sources_execute_in_fresh_and_reused_macro_hosts` is invoked
by the Python producer after an exact Cargo build. No cancellation or singular
or plural limit suites are involved.

Delivery verification produced `literal-char-string-receipt.json`, byte-identical
to the fresh raw artifact
`/private/tmp/light-cljs-174-literal-delivery-v2/evidence.json` (SHA-256
`1b3b04c33553cb5f3c1502af37a85e3d9a836195334913c77c554a252875a94d`). It verifies 200 canonical acceptance frames from
25 complete sources, 40 explicitly nonpassing neighboring frames from five
complete sources, and 88 recovery engine outcomes after 44 diagnostic contexts.
The four source/hash/reconciliation unit guards and the normal Rust regression
passed. Seven restored raw-artifact mutations were rejected even when the
artifact hash census was updated: engine outcome, complete phase source, emitted
namespace, Node input, recovery computation, executable hash and serialized
summary. `literal-char-string-raw-guard-receipt.json` retains those observations;
a final pristine whole-document replay passed after restoration. Neither
receipt substitutes for raw evidence. The prior first delivery attempt remains
preserved and nonpassing after exposing a namespace diagnostic-verifier issue;
its exact driver itself passed and prompted a fresh producer-bound delivery.

## Remaining owners and regression verification

Fresh all-status duplicate searches and the complete active queue found no
focused owners for the distinct neighboring omissions. They remain open before
the full language gate: 176 owns numeric reader entries in literal maps/sets;
177 owns positional entries in small `array-map`/`hash-set` constructors; 178 owns
heterogeneous quoted-set reader traversal. Numeric case validation remains owned
by 175. The two release/checkout dynamic-set differences remain explicitly
nonpassing. Pellet 125 is incomplete under its original strict full-language
criteria; this 25-source cohort does not establish zero missing.

The focused Rust suites passed 136 ordinary tests: analyzer (36), reader (19),
case acceptance (1), equality and equality contracts (5), public collection
constructors (5), application execution (53), and generated macro regressions
(17). The normal new literal regression also passed. The audited ignored literal
driver executed separately during the fresh producer run; other ignored pinned
suites were not claimed as executed. Existing original sources, bodies and
historical expectations were preserved. Inventory regeneration changed only the
analyzer source hash in `full-manifest.json`; its 97 files, 1,328 bodies, 4,728
assertions, 5,263 obligations and 1,327 public arities remain unchanged, and
`extract_inventory.py --check` passes.

Nine fresh Node benchmark processes each matched the integrated fixture's full
value/effects/stdout, plus primitive42 and core-call3 controls. The final report
is `benchmarks/results/literal-char-string-2026-10-12/output-startup.json`:
357,655 emitted bytes, 85,037 gzip bytes, median measured total15.085416ms.
This is a validated local measurement, not a cross-platform performance claim.

`cargo clippy --locked --lib -- -D warnings` fails with 55 existing lint
diagnostics. A separately materialized source-only copy of starting commit
`ee9c0c87ff5ddcda5acba6cb2633be91fc533508` produced the exact same 55 diagnostic
codes/messages/primary source snippets, with zero new or removed diagnostics.
Raw current/baseline JSON diagnostics remain at
`/tmp/light-cljs-174-{current,baseline}-clippy.jsonl`.
The new Rust driver formatting and `git diff --check` pass. The unrelated
`.DS_Store` is preserved and excluded. Blocked 156 was not retried, delegated,
investigated or repaired; no cancellation or singular/plural limit suites ran.
No push or pull request was performed.
