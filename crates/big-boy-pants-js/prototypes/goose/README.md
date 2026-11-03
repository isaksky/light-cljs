# Tiny Goose VM bootstrap

This is a serial Goose interpreter with synthetic, hand-assembled programs. It
has no JavaScript frontend or macro execution support. VM execution, validation,
values, object allocation IDs and counter effects run in Goose. Babashka only
invokes the compiler and executable.

Run from the repository root:

```sh
bb crates/big-boy-pants-js/prototypes/goose/smoke.bb
```

Or from `crates/big-boy-pants-js`:

```sh
bb prototypes/goose/smoke.bb
```

The script resolves sources relative to itself. It checks formatting and types,
runs the synthetic cases with debug checks, builds a native executable, and runs
that executable. Its temporary native output is removed even after failure. Set
`GOOSE` to select a compiler executable. The initial validated toolchain is
`goose v2026.3-267-g369c7a2c (runtime 3025e5810379)` with Babashka. No Rust build
or external interpreter is involved.

## Prototype encoding

This deliberately small format can be extended in later experiments. It is not
a transport compatibility promise. An instruction starts with one opcode byte;
constant/local indices and branch operands are also **one byte**. Branches name
absolute byte offsets, which the loader resolves to instruction indices.

| Byte | Operation | Operand | Stack effect |
| --- | --- | --- | --- |
| 1 | Constant | constant index | 0 → 1 |
| 2 | Undefined | none | 0 → 1 |
| 3 | Get local | local index | 0 → 1 |
| 4 | Set local | local index | 1 → 0 |
| 5 | Pop | none | 1 → 0 |
| 6 | Dup | none | 1 → 2 |
| 7, 8, 9 | Add, subtract, multiply | none | 2 → 1 |
| 10 | Numeric less-than | none | 2 → 1 |
| 11 | Strict equality | none | 2 → 1 |
| 12 | Boolean not | none | 1 → 1 |
| 13 | Jump | byte offset | 0 → 0 |
| 14 | Jump if false | byte offset | 1 → 0 |
| 15 | Return | none | 1 → 0 |
| 16 | Fresh property-less object | none | 0 → 1 |
| 17 | Counter effect | none | 1 → 1 |

Code contains 1–256 bytes; constant and local counts are 0–256, allowing index
255. The assembler `emit` accepts wide integer inputs and checks their ranges
and the resulting code length before narrowing or changing the output. The
loader rejects oversized inputs before indexing them. Tests exercise index 255,
branch address 255, code length 256, and rejected 256 operands/257 counts. This
small limit is intentional and can be widened together with the format later.

The loader decodes every byte, including unreachable instructions. It rejects
unknown/truncated instructions, bad pool/local indices, and branches outside
code or into operands. Reachable control flow receives a worklist stack-depth
check: underflow, unequal join depths, and fallthrough past code are errors.
Unreachable stack operations need not have a meaningful incoming depth. Loops
need not terminate; use finite budgets for untrusted programs. Loading sets
`valid` only after every check passes and records an error kind and byte offset
on failure. Construct a fresh `Program` for each load; reload is unsupported.

Values are fixed-size tagged undefined, null, boolean, f64 number or object ID.
Constant pools accept scalar values only. Arithmetic and less-than require two
numbers; unsupported types fail the job explicitly without coercion. Equality
compares tags and payloads (numeric NaN stays unequal). Truthiness handles zero,
NaN, null and undefined as false, and objects as true. Strings, properties,
ordinary calls, closures, exceptions and other JavaScript types are unsupported.
The counter requires a number, records its argument once in the job's ordered
effect log and returns that job's new effect count as a number. Counters are
local job effects; this bootstrap has no shared authority or external callbacks.

## Owners, resumption and lifetime

`Program` owns flat decoded instructions/constants, and is borrowed read-only
by each `Job`. Keep the original owner immutable and alive for all such jobs.
`initialize` fills locals with undefined, then copies arguments into the first
local slots; extra arguments are ignored. The job owns locals, operands, effects,
next instruction PC, terminal status, result, fault location and claim bit. Each
run's native frame can disappear at yield: no continuation or borrowed temporary
is needed to resume. The smoke driver alternates two jobs at budgets 1 and 5 and
compares results and ordered effect arguments to independently executed unlimited
runs. It checks budget zero against **every explicit job field**, including
populated locals, operand stacks and effect logs, and the session allocator.

`run` acquires one local claim; another claim or `run` while claimed is rejected.
`run_claimed` consumes a previously acquired claim. Every run clears the claim
on completion, yield or failure. A type failure terminates just that job and
cannot replay earlier effects. Later calls on a terminal job return its existing
outcome. Claims are cooperative local admission checks; there are no threads,
atomics, movable snapshots, nested VM calls or worker migration in this slice.

`Session` allocates monotonically increasing property-less object IDs from 1
through 2,147,483,647 and fails before overflow/reuse. The allocator is shared by
local jobs in that session. IDs in a retained scalar result remain meaningful
after its producing job ends, throughout the **same session's lifetime**; no
object state or Goose pointer needs retaining. IDs from different sessions
must never be compared, imported or reused as if they shared an identity space.
Discarding the session invalidates all its object IDs. There is no reset,
collector, object table, cross-session transport, or general object model.

The behavior and validation invariants adapt the original engine's
`cc764dbe:src/vm/{instr,frames,dispatch,executable}.rs`, maximum-index cases from
`88fc62d3:tests/vm_core.rs`, and budget/effect shape from
`99d4f9fb:tests/resumable.rs`. Those historical references live at the repository
root paths at their commits. The Goose storage, compact encoding and local
counter implementation are new; Rust heap/root/locking machinery is not used.
