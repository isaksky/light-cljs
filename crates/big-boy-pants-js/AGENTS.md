# big-boy-pants-js (bbp)

bbp is light-cljs's compile-time JavaScript runtime, implemented in Goose.
See [README.md](README.md) for the layout.

## Skills

- Invoke the `goose` skill (/Users/admin/.claude/skills/goose/SKILL.md)
  before writing, reviewing, debugging or porting Goose code. Read the topic
  references needed for the task; verify unfamiliar syntax, storage rules and
  APIs against the installed compiler and small runnable examples.
- Use the `goose-skill-improver` skill
  (/Users/admin/.claude/skills/goose-skill-improver/SKILL.md) when this work
  reveals a reproducible mistake, missing reusable guidance or a verified
  compiler/library change. Follow its evidence, validation and local commit
  workflow, then resume the bbp task. Routine Goose programming does not
  require a skill audit. Keep project-specific decisions in this repo or the
  Pellets group context.
- These are local skill paths on the development machine. If they are missing
  in another environment, locate the installed skills before relying on
  remembered language rules. Do not confuse this language with the Goose AI
  agent.

## Scope and workflow

- Work lands on `master`. The Goose runtime is `src/` here, with tests beside
  the modules they import, `corpus/` data, `scripts/` entry points and
  `evidence/` reports. Compiler adapters live in `src/goose/` at the repository
  root; the exporter is `rust/src/frontend/goose.rs`.
- `rust/` is bbp's first, Rust implementation. Read it freely as the
  specification of JavaScript semantics, and change its parser, bytecode
  compiler and exporter when Goose needs it. Do not extend or optimize its VM,
  and never satisfy a Goose task by making Rust execute JavaScript.
- Read the selected pellet and its group context (`goose-generated-200k` for
  the compiler path, `goose-javascript` for JavaScript coverage). Pellets holds
  the implementation sequence and detailed acceptance criteria; do not
  duplicate that queue in repository Markdown.
- light-cljs owns contextual reading, name/dependency resolution, macro
  expansion orchestration and analysis of returned forms. Goose executes the
  emitted JavaScript. Reuse the Rust JavaScript frontend; do not add a Clojure
  interpreter or another JavaScript parser.
- Keep Goose execution and its VM heap in Goose. Rust/C export and transport
  glue may connect the runtime; reference engines provide independent
  expectations. Unsupported operations must fail explicitly without fallback,
  and the Goose compiler path must not use V8.
- Use Babashka (`bb`) for new checked-in orchestration and fixture scripts.
- No raw opcode, descriptor or tag numbers in Goose sources. Use the names
  `src/export_schema.goose` generates from the Rust registry: `opcode::Add`,
  `form_op::CljCount`, `throw_kind::Uninitialized`, `constructor::Ordinary`,
  `well_known::ToPrimitive`, `operand::Target`, `count::Captures`,
  `flow::Branch`, and `constant::Symbol` for a GSP constant record's tag. A
  new numbering with a Rust-side source gets generated names the same way
  (`inspection_schema()` in `rust/src/frontend/goose.rs`, then
  `bb scripts/regenerate.bb`). Test a VM value's variant with the predicates
  in `src/source_vm.goose` (`v.is_object()`, `v.is_string()`, `v.is_hole()`)
  or a `match`; only ordered tests use `v.tag()`, against `value_tag::Null`
  and its siblings, never a number. A form-graph (GFB2) node's tag is a
  `form_tag::` name from `src/form_bridge.goose` (`form_tag::Keyword`,
  `form_tag::Cell`), which the compiler's encoder in
  `src/goose/compiler_bridge.rs` mirrors as `form_tag::KEYWORD` and a Rust
  test keeps equal. The cell owner's numbers are Goose-private names in
  `src/cell_owner.goose`: `flat_kind::Graph`, `cell_op::CompareAndSet`,
  `cell_flavor::Delay`, `delay_state::Realized`, `cell_error::Budget`.

## Storage and branching

- Write idiomatic Goose, not a conventional engine in Goose syntax. Memory is
  reclaimed by scope (job, worker, snapshot); do not rebuild a malloc-style
  heap with integer IDs, hand-made free lists, reference counting or a
  mark-and-sweep pass over a global table. Use typed relative references and
  validated index types inside VM images, slot_pool/slice_pool for reuse,
  compact enums for values, enum/match dispatch, and frames as data. The
  `goose-javascript` group context and the JavaScript coverage design document
  hold the exact rules.
- Choose owners and layouts before implementing graphs. Data crossing a worker
  boundary is flat: explicit snapshots and flat messages, with checked links
  and documented lifetimes. Preserve exact UTF-16 runtime strings separately
  from source-byte spans and provenance.
- Branching means explicit snapshots into isolated workers. Include required
  allocator metadata; do not assume OS fork, copy-on-write or shared mutable
  Goose heaps. Coordinator producer claims and shared-effect authority remain
  outside worker snapshots.
- Publish immutable results through validated images. Keep compiler-owned
  forms and diagnostics usable after the producing job and Goose session end.

## Verification

- Check `goose version` and the relevant skill references before depending on
  compiler-specific behavior. Proposals in compiler design documents are not
  evidence of implemented features.
- Format changed Goose files, run `goose check`, and execute meaningful cases
  with `--debug`. Exercise the actual generic instantiations. Use native builds
  for threads and performance measurements.
- Run the focused tests required by the pellet and relevant existing compiler
  checks for changed integration boundaries. Keep synthetic VM, source-derived
  execution and real compiler/macro evidence distinct. JavaScript coverage
  work also runs the conformance harness for its area.
- Measure matching supported work against V8 and Pants, including setup,
  copying, conversion and cleanup. Separate instrumented allocation runs from
  timing runs. A measured performance loss is a valid experimental result.
