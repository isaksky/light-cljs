# Direct Var metadata fixture reconciliation

At starting HEAD `10549fc2a1a873fbf287e5ea35f546dffe76ec5d`, the complete,
unchanged direct JavaScript suite failed because its fourth `definitionVar`
argument was Boolean `true`. The current emitter passes a callable metadata
provider. `meta` invokes that provider and caches the actual public `:test`
callback, independently of the ordinary root getter. The byte-exact suite also
failed against committed runtime `3235870bfabc745d32596489bdd52eca9353553e`.
`direct-var-metadata-reconciliation.json` preserves the entire original Var
fixture, both full-suite failure logs, source hashes and commands.

The direct fixture now supplies a callback provider. Construction, equality and
hashing invoke neither provider nor root. Reflection invokes the provider once,
preserves the callback identity, and reuses the same metadata map through both
`meta` and `_meta`. Dereference calls the live root getter. Testing the reflected
callback preserves `:ok` and the callback counter without another root demand.
A second explicit dereference retains the two-demand live-getter assertion;
metadata overrides retain their callback and demand checks. Every surrounding
original assertion remains. Production sources are unchanged.

Three isolated runtime mutations (eager root consumption, retaining an uncached
provider, and omitting its callback) each fail the strengthened direct assertions.
Their complete suite inputs and raw failure logs remain under
`/tmp/light-cljs-194-controls`.

Fresh execution of the complete unchanged `cljs-3411-function` declaration and
`cljs-3411` body passes release ClojureScript 1.12.134 and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. Both use optimizations `none`,
static functions disabled and def-emits-var disabled. The original source SHA256
remains `a6aea8026522804962ec49a6f5e2531c482eb9830c1c16b5962cf21f22b847b0`.
Fresh/reused applications and genuinely demanded macros pass in QuickJS and
Node (eight observations), with eight identical-host recovery observations.
The existing exact producer validates complete sources, emitted modules, actual
input hashes, both pin artifacts and its immutable executed binary copy. Its
full fresh receipt is nested in the reconciliation JSON; its inherited
`implementation_starting_head` field describes the original189 implementation,
while this reconciliation has its own top-level `starting_head`. All historical
metadata receipts remain byte-exact.

The complete direct Node runtime suite and exact Rust
`execution::runtime_values_have_private_immutable_storage` test pass. All six
focused Var/core/source metadata, callback, reflection and deferred targets pass
25 tests. Definition execution and six macro metadata selections pass, including
lazy Var identity/reflection. Strict all-target Clippy passes with warnings denied.
Normal compiler performance validates 459 samples across 51 scenario/phase pairs
(nine samples, four iterations). Nine integrated fresh Node samples preserve the
complete expected values, effects and stdout. These are regression observations,
with no controlled timing improvement claim.

An optional repository-wide `cargo fmt --all -- --check` reports preexisting
drift in 77 Rust files. Every tracked Rust source is byte-exact to starting HEAD;
the exact log and hashes are retained. All-status searches and full live queue
inspection confirm125 already documents unchanged repository rustfmt drift.
No formatting repair is included in this focused fixture change.

The JSON records exact verification commands, raw paths/hashes, mutations and
limits. No cancellation/resource-limit suites or156 investigation/retry/repair
ran. This change grants no additional source/operation credit and does not
establish125's unchanged full-language acceptance.
