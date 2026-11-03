# Goose execution prototype

## Skills

- Use the [Goose language skill](/Users/admin/.codex/skills/goose/SKILL.md)
  before writing, reviewing, debugging or porting Goose code. Read the topic
  references needed for the task; verify unfamiliar syntax, storage rules and
  APIs against the installed compiler and small runnable examples.
- Use the [Goose skill improver](/Users/admin/.codex/skills/goose-skill-improver/SKILL.md)
  when this work reveals a reproducible mistake, missing reusable guidance or a
  verified compiler/library change. Follow its evidence, validation and local
  commit workflow, then resume the prototype task. Routine Goose programming
  does not require a skill audit. Keep project-specific decisions in this repo
  or the Pellets group context.
- These are local skill paths on the development machine. If they are missing
  in another environment, locate the installed skills before relying on
  remembered language rules. Do not confuse this language with the Goose AI
  agent.

## Scope and workflow

- The prototype integration branch is `codex/goose-prototype`. Keep the Goose
  implementation and its build/test entry points in this directory where
  practical; make narrow exporter and compiler-adapter changes elsewhere.
- Read the selected pellet and the shared `goose-prototype` group context.
  Pellets holds the implementation sequence and detailed acceptance criteria;
  do not duplicate that queue in repository Markdown.
- Begin with a small runnable serial VM and extend it for the selected actual
  emitted macro/helper workloads. Preserve the existing compiler, reference
  engines and production defaults for comparison.
- light-cljs owns contextual reading, name/dependency resolution, macro
  expansion orchestration and analysis of returned forms. Goose executes the
  emitted JavaScript subset. Reuse the Rust JavaScript frontend; do not add a
  Clojure interpreter or another JavaScript parser for this experiment.
- Keep Goose execution and its VM heap in Goose. Rust/C export and transport
  glue may connect the prototype; reference engines provide independent
  expectations. Unsupported jobs must fail explicitly without fallback.
- Use Babashka (`bb`) for new checked-in orchestration and fixture scripts.

## Storage and branching

- Choose owners and indexable layouts before implementing graphs. Use explicit
  IDs and flat tables/ranges for transported data, with checked references and
  documented lifetimes. Preserve exact UTF-16 runtime strings separately from
  source-byte spans and provenance.
- Branching means explicit snapshots into isolated workers. Include required
  allocator metadata; do not assume OS fork, copy-on-write or shared mutable
  Goose heaps. Coordinator producer claims and shared-effect authority remain
  outside worker snapshots.
- Publish immutable results through validated relocation. Keep compiler-owned
  forms and diagnostics usable after the producing job and Goose session end.
- Follow the selected pellet's bounded design. A general tracing collector,
  complete ECMAScript implementation and production backend migration are
  outside this experiment.

## Verification

- Check `goose version` and the relevant skill references before depending on
  compiler-specific behavior. Proposals in compiler design documents are not
  evidence of implemented features.
- Format changed Goose files, run `goose check`, and execute meaningful cases
  with `--debug`. Exercise the actual generic instantiations. Use native builds
  for threads and performance measurements.
- Run the focused tests required by the pellet and relevant existing compiler
  checks for changed integration boundaries. Keep synthetic VM, source-derived
  execution and real compiler/macro evidence distinct.
- Measure matching supported work against V8 and Pants, including setup,
  copying, conversion and cleanup. Separate instrumented allocation runs from
  timing runs. A measured performance loss is a valid experimental result.
