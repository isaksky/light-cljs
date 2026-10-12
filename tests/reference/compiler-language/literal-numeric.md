# Canonical numeric collection literals

Pellet 176 preserves the exact original source:

```clojure
[(count {1 :int 1.0 :float}) (get {1 :int 1.0 :float} 1) (count #{1 1.0}) (vec (seq #{1 1.0}))]
```

Both independently executed ClojureScript pins, release `1.12.134` and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, return `[2 :int 2 [1 1]]` with
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
The sources and expected typed packets in `literal-numeric-contracts.json`
were derived through fresh whole-source compilation and execution in both pins,
without using local outcomes as canonical expectations. The producer freezes
the complete contract file's SHA-256 and every exact original source's SHA-256.
The derivation artifacts remain identified in the catalog, and delivery requires
new independent reference executions.

The 42-source compiled catalog covers source order, map count/lookup/membership,
keys/values/sequence, set count/lookup/membership/sequence, quoted and nested
literals, executable composite keys and members, hexadecimal/octal/exponent
spellings, BigInteger/BigDecimal categories, integral ratios, decimal precision,
JavaScript Number rounding, integers beyond signed 64-bit, signed zero,
infinities, and nonreflexive NaN. Map effects require exactly `[:first :second]`
and captured output `"2\n11\n"`; the executable set effects control requires the
same ordered effects and output `"2\n"`. Every complete typed observation packet
also compares trace and stdout exactly.

Reader identity distinguishes integer `1`, decimal `1M`, and floating `1.0`
although each lowers to JavaScript Number 1. Integer, BigInteger, hexadecimal,
octal, and integral-ratio alternate spellings retain the same reader identity
and must reject as true duplicates. Floating exponent alternatives and decimal
scale alternatives also reject within their own category. Repeated NaNs remain
separate reader entries and membership remains false.

The map catalog checks eight pairs and the hash-map construction boundary at
nine pairs. Numeric reader hash traversal affects the winning equal lowered
key: both source orders of integer 1 and float 1.0 yield `:int` above the
boundary; ten entries containing integer, decimal, and float yield `:decimal`.
Additional boundary controls require `:second` for two wide integer spellings
that round to one Number, `:rounded` for decimal precision collapse, and
`:float` for integer zero and negative floating zero. Set count and membership
are checked at eight and nine scalar entries. Above the construction boundary,
only scalar-number collision pairs are reordered according to canonical reader
hashing before existing Number construction; runtime equality/hash semantics
and general set order are not redefined.

Two exact existing reader regression sources apply metadata to a map or set
containing `1000000000000000000000` and `1000000000000000000001`. Both references
return count2. The catalog retains their historical expected count1 as
`original_expected`, with explicit basis and immutable source hashes. Their
historical comparison remains nonpassing, while their independently established
canonical count2 is required in all eight execution frames.

Twenty-six diagnostics include true duplicates in each numeric reader category,
reduced ratios, nested and quoted duplicates, odd/unclosed map literals,
unclosed sets, and nonintegral ratios. A nonintegral ratio such as `1/2` can be
read but fails as an invalid ClojureScript constant; it is not recorded as a
successful Number literal. Each diagnostic requires exact independently pinned
reference text and local rejection in all four application/macro and
fresh/reused host contexts, followed by a demanded recovery macro computing
`(+ 20 22)` on that same host and returning 42 in both engines.

The two reduced nonintegral-ratio duplicate sources reject as reader duplicates
in both references and as invalid numeric constants locally. The numeric tower
is outside this scope; these two diagnostic categories remain explicitly
nonpassing and receive zero canonical diagnostic agreement credit. The separate
nonintegral-ratio source rejects as an invalid constant in both references and
locally.

Five neighboring controls remain explicitly nonpassing:

* Small runtime `array-map` and `hash-set` constructors retain positional entries
  in both references; the current local constructors deduplicate. Pellet 177
  owns that behavior. A separate accepted `hash-map` and `set` control proves
  ordinary runtime deduplication.
* Dynamic duplicate scalar sets and executable duplicate vector-member sets
  return a singleton in the release pin and throw a caught duplicate diagnostic
  in the checkout pin. The local outcome matches the checkout; neither row
  earns two-pin agreement credit.
* The complete finite sequence of a numeric/NaN collision set and a heterogeneous
  quoted set follows canonical reader hash traversal, while the current local
  sequence follows insertion order. Pellet 178 owns traversal. A separate
  accepted numeric/NaN collision control still requires correct map/set count,
  first map lookup, and nonreflexive NaN membership.

`literal_numeric.py` builds and executes both independent references, then the
actual Rust audit driver. Each complete source compiles as an ordinary namespace
application and an actually demanded macro computation in fresh and reused
MacroHosts; both emitted programs run in QuickJS and Node. The ordinary Rust
regression installs a separately compiled empty-vector runtime so the typed
observer can inspect scalar macro results that omit runtime code; its tested
source and computed result remain exact. A demanded macro
quotes its computed result, never a frozen expected value. Reused hosts begin
with a real demanded warmup macro. The replayer binds current compiler and proof
sources, the exact Cargo-built audit binary and other executable hashes,
commands/environment, pin identity/settings, complete `.cljc` source modules,
emitted JavaScript, raw engine observations, and a complete artifact hash census.
Missing, extra, stale, detached, or changed evidence fails closed. A saved receipt
cannot replace the current raw executable proof.

Focused commands:

```sh
cargo test --locked --test compiler_literal_numeric canonical_numeric_literal_constants_and_same_host_diagnostic_recovery -- --exact
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_literal_numeric.py
python3 -B tests/reference/compiler-language/literal_numeric.py --output /tmp/light-cljs-176-literal-numeric-delivery
```

A complete successful proof records 296 canonical acceptance frames from 37
sources, 40 explicitly nonpassing neighbor frames from five sources, and 208
same-host recovery outcomes after 104 local diagnostic contexts. These focused
controls grant zero original SCI-body credit and zero full-language completion
credit. Existing inventory and historical expectations are not substituted or
redefined by this catalog. No cancellation or singular/plural limit suites are
part of these commands; externally blocked 156 is outside this work.

Delivery evidence is retained in `literal-numeric-receipt.json`, byte-identical
to `/private/tmp/light-cljs-176-numeric-delivery-final-v2/evidence.json` (SHA-256
`963ca6b4f36024cdcf7daff3da6890dcc983c66a9c1c4e5aa3b960872c7a4de7`).
The exact Cargo-built audit executable also passed the ordinary 42-source,
26-diagnostic regression. All five Python guards passed. Eight restored
raw-artifact mutations failed closed: complete phase source, emitted namespace,
Node input, raw engine outcome, recovery computation, audit executable hash,
audit environment, and detached serialized summary. The five file mutations
were rejected even after updating the artifact hash census. The final pristine
whole-document replay passed after restoration. Those observations are retained
in `literal-numeric-raw-guard-receipt.json` (SHA-256
`7440697f2833b63098ae0ece637c334efa0df53b5ea0fbfae17ad521b94df353`).

Earlier 40-source and first 42-source captures remain immutable historical
artifacts and are superseded by this final producer/driver snapshot. No earlier
capture or saved receipt serves as a fallback for current executable evidence.
The focused proof does not establish general map effect traversal or set
sequence order, and cannot close the strict full-language gate.

The surrounding verification passed 181 ordinary analyzer/bootstrap/case/equality/literal/macro/reader regressions plus the new numeric regression, 18 inventory guards, 22 full-ledger guards and four character/string literal guards. Byte-exact inventory regeneration changed only the analyzer source hash; all original bodies, obligations and expectations remain unchanged. Six phase and five operation witness file hashes were rebound only after checking that the linked complete test bodies were byte-identical. The fresh current static ledger still has `strict_complete: false`; these controls add no original-language obligation credit. The compiler benchmark completed 423 validated measurements (nine samples, two iterations). Strict Clippy retains 55 pre-existing diagnostics. No cancellation or singular/plural limit suite, push or PR ran; externally blocked 156 remained untouched.

A distinct generic map source evaluation-order defect has a duplicate-checked live owner, Pellet 179, before gate 125. The complete keyword-only nine-entry source in `literal-map-effects-gap-receipt.json` (SHA-256 `7f80685267a423177be8d047cf92371d0811d6686fb8fa2e05848fc89b128a78`) returns `[9 [:e :g :c :h :b :d :f :i :a]]` in both independent pins; all eight actual local frames return `[9 [:a :b :c :d :e :f :g :h :i]]`. The discovery binds reference artifacts, exact source wrappers, emitted JavaScript, executable/environment/source hashes and complete typed packets. It earns zero acceptance or original/full-language credit. Its raw references and local executions remain at `/private/tmp/light-cljs-176-generic-map-effects-reference` and `/private/tmp/light-cljs-176-map-effects-neighbor`. This follow-up was recorded without implementing another Pellet.


## Constructor follow-through after Pellet 177

The earlier delivery above remains historical. Pellet 177 fixes the exact small
direct `array-map`/`hash-set` constructor source without changing its independently
pinned expectation. Saved/apply calls retain deduplication. Fresh current proof
includes both pins and all eight actual source/phase/engine/lifecycle frames.
Constructor updates, metadata/hash, sizes and effects are covered separately in
[positional-constructors.md](positional-constructors.md).

The current `literal-numeric-receipt.json` is byte-identical to
`/private/tmp/light-cljs-177-numeric-followthrough-v1/evidence.json` (SHA-256
`de356e8a0e6a2de282179fed0651a6ea26f0b436c9b23f58e9f43ed14cf0b3a1`). It establishes
38 accepted complete sources (304 frames), while
4 complete sources stay explicitly nonpassing. Both pin disagreements
and Pellet 178 traversal sources remain unchanged and receive zero acceptance
credit. Every diagnostic and same-host demanded macro recovery was rerun.

The previous receipt is preserved byte-exact as `literal-numeric-historical-176-receipt.json`,
and its contract catalog is preserved as `literal-numeric-historical-176-contracts.json`;
historical originals, outcomes and the strict full-language criteria survive.
The constructor follow-through does not provide original-body or full-language
completion credit.
