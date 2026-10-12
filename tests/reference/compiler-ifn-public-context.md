# Genuine callable IFn receiver contracts

Five complete literal contexts execute all 22 canonical receiver-inclusive IFn
method signatures, the public IFn protocol value, and the corresponding two
local API operation rows. These are IFn method dispatch contracts. They do not
claim that direct public `-invoke` function calls on these receivers pass.

Three contexts implement and demand fixed user arities 0–6, 7–14 and 15–20.
Every method runs through a direct callable receiver, saved callable receiver,
`apply` receiver and explicit receiver-bound compiler slot. The exact
`cljs$core$IFn$_invoke$arity$N` function executes via `.call`, with N user
arguments; the canonical protocol signature includes the receiver as argument
N+1. Every result checks the method selected, all positional arguments, a live
captured field and the actual receiver identity. Ordered effect vectors record
all four paths for every arity. `satisfies? IFn` and `ifn?` observe actual callable
protocol behavior, rather than using catalog or name presence as evidence.

The terminal context executes the canonical `[this a ... t rest]` signature.
Its direct/saved/explicit-slot paths receive twenty values and a sequence tail;
its `apply` path receives 22 flattened inputs and must produce the same `[21 22]`
tail. An earlier apply input containing a single nested `(list 21 22)` correctly
produced a one-item tail and failed the original expectation. That entire
original source/expectation and both-pin failure remain preserved; no flattened
shape is inferred from a declaration.

The final context copies callable metadata, mutates the original receiver's
capture field, compares original/copied identity and metadata, demands a saved
copied receiver that throws `ex-info`, observes its exact message and full data,
and records catch/finally/recovery order. Recovery executes the exact copied
compiler slot. The independent expected map values are compared by EDN equality,
which accepts printer whitespace/comma separators while requiring every
original value. Raw actual printed observations are retained by the Rust audit.
The complete actual value is evaluated once before equality, in both phases.

The Rust audit uses fresh and reused MacroHost objects, and executes each full
source as an application and inside an actual MacroHost body. MacroHost runs in
the project's QuickJS compiler environment. The resulting application/macro
expansion value executes in both QuickJS and Node, for 40 observed frames. Node
results do not claim a separate Node compiler host. Each canonical pin compiles
and executes the whole source beside its independent literal expectation.
Native JavaScript macro execution is the adopted local phase contract; ordinary
JVM-hosted ClojureScript compilation provides the independent shared semantic
oracle. No JVM execution of JavaScript-only macro forms is claimed.

`ifn_public_context.py --export-owned DIRECTORY` writes only new cohort exports.
It binds canonical body/signature hashes, complete driver/fixture/helper hashes,
reviewed actual call and method source hashes, and exact named application/macro
tests. It rejects method-only evidence, native-function aliases, undemanded
compiler slots, wrong slot/formal counts, incorrect receiver bindings and
unreviewed nested terminal apply shapes. Every complete source and expectation
is frozen against independent observations; eligibility never supplies a pass.
The parent merges shared tables and regenerates inventory after its snapshot
gate finishes. The local API rows describe operation execution and do not claim
complete metadata values.

All initial controls are retained under `/tmp/light-cljs-125-ifn-public-draft`,
`...-draft2`, `...-original-error`, `...-callable-draft` and `...-callable-verified`.
The first unmatched reify delimiter was rejected by both pin launchers. After
that syntax correction, the explicit public `-invoke` routes throw `No protocol
method IFn.-invoke` in both pins because those requested receiver-inclusive
protocol slots differ from the adapted callable compiler entry slots. Those
failures remain failures and provide no coverage or independent production gap.
The new callable/slot contexts retain all complete setup and semantic paths;
they do not retrofit the invalid controls as passing tests.
