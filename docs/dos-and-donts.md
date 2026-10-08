For **building a fast compiler**, these are the rules we can take from Go and Jai. The recommendations are mine; each is grounded in a concrete implementation choice or demonstration.

## 1. Use integer IDs for compiler entities — Go

**Do:** Give values, blocks, and other frequently referenced entities compact IDs within a compilation unit. Use arrays for per-entity facts where those IDs are dense:

```go
visited[id] = true
useCount[id]++
```

**Don’t:** Automatically make every analysis table a hash map keyed by names or syntax objects. Repeated hashing and lookup are unnecessary when the entities already have suitable integer identities.

Go’s SSA documentation explicitly says its sequential IDs allow the compiler to avoid maps. It retains source-position information separately for diagnostics. [go.dev](https://go.dev/src/cmd/compile/internal/ssa/README?utm_source=chatgpt.com)

## 2. Reuse temporary memory across functions — Go

**Do:** Give each compilation worker reusable scratch storage. Reset it between functions while retaining useful capacity. Clear references when necessary so completed work does not remain reachable.

**Don’t:** Allocate and discard a fresh collection of temporary arrays and analysis structures for every function and every pass.

Go has an explicit `ssa.Cache` for reusable values, blocks, locations, and analysis state. Its compilation code selects the cache belonging to the current worker and resets it before building the next function. This saves allocations even during a completely fresh build. [go.dev](https://go.dev/src/cmd/compile/internal/ssa/cache.go?utm_source=chatgpt.com)

## 3. Make importing a dependency a cheap operation — Go

**Do:** Produce a compiled description containing the information consumers need. Index it so consumers can load individual pieces on demand.

**Don’t:** Reanalyze a dependency’s implementation whenever another unit imports it, or eagerly reconstruct every declaration in a large dependency when only a few are used.

Go’s export data supports lazy decoding. It includes types, relevant function bodies, and analysis summaries. [go.dev](https://go.dev/src/cmd/compile/README?utm_source=chatgpt.com)

That last point matters: **define the boundary by what compilation needs.** A list of public names is insufficient if consumers also depend on an inline body or another derived fact.

## 4. Put explicit budgets on optional optimization — Go

**Do:** Give transformations that can expand code an explicit cost budget. Stop evaluating a candidate once you know it exceeds that budget. Become more conservative when the destination function is already large.

**Don’t:** Finish traversing a huge candidate merely to conclude that it was too expensive, or allow repeated inlining to grow a function without a limit.

Go’s inliner has explicit budgets, stricter rules for large callers, and early rejection when its budget is exhausted. Diagnostic modes can deliberately continue to calculate the full cost. [go.dev](https://go.dev/src/cmd/compile/internal/inline/inl.go?utm_source=chatgpt.com)

Apply this to **optional optimization**. Required semantic analysis must still finish correctly.

## 5. Parallelize work with clear ownership, and schedule large jobs early — Go

**Do:** Complete shared-state preparation before launching work that assumes it is finished. Give workers their own mutable scratch data. Start expensive eligible jobs early so they do not become the last task keeping the build alive.

**Don’t:** Add threads around code whose shared-state dependencies remain implicit, or let queue order routinely leave the largest function until the end.

Go’s compiler explicitly separates preparation that is not safe to run concurrently, then distributes function compilation among workers. Its normal scheduling puts longer functions first to reduce stragglers. During race testing, it randomizes compilation order to expose bugs. [go.dev](https://go.dev/src/cmd/compile/internal/gc/compile.go?utm_source=chatgpt.com)

**Randomized scheduling in tests is a particularly useful practice to copy.**

## 6. Make full-build speed a regression metric — Jai

**Do:** Time a fresh invocation through usable output on a representative program. Preserve a fast ordinary compilation path.

**Don’t:** Let warm caches conceal increasingly expensive compilation, or make expensive optimization compulsory for everyday iteration.

Blow explicitly demonstrates full rebuilds and distinguishes them from Jai’s slower optimizing path. [YouTubeToTranscript.com](https://youtubetotranscript.com/transcript?current_language_code=en\&v=IdpD5QIVOKQ\&utm_source=chatgpt.com)

## 7. Attribute generated work to its source — Jai

**Do:** Report compile-time execution time, expansion size, and generated specializations by originating definition.

**Don’t:** Judge compilation cost by handwritten line count, or repeatedly generate an equivalent specialization.

Jai exposes these costs. Blow demonstrates a surprisingly large function caused by macro expansion, along with reuse of previously generated specializations. [YouTubeToTranscript.com](https://youtubetotranscript.com/transcript?current_language_code=en\&v=IdpD5QIVOKQ\&utm_source=chatgpt.com)

## 8. Block only the work that depends on an unavailable result — Jai

**Do:** Represent a waiting task’s dependency explicitly and continue unrelated work, while preserving required ordering.

**Don’t:** Introduce a global barrier for all compile-time execution unless the semantics require it.

Jai’s controlling metaprogram runs alongside compilation; expressions requiring compile-time results must wait for those results. [YouTubeToTranscript.com](https://youtubetotranscript.com/transcript?current_language_code=en\&v=IdpD5QIVOKQ\&utm_source=chatgpt.com)

## 9. Make performance changes easy to verify — Go

**Do:** Keep a known-good compiler available. For a performance refactor, compare its output with the changed compiler on substantial inputs, and measure repeated runs.

**Don’t:** Accept “it seems faster” from one run, or assume that successful compilation proves a refactor preserved behavior.

Go’s compiler-development documentation describes comparing a changed compiler’s object output against a saved compiler, and provides tooling for repeated performance comparisons. [go.dev](https://go.dev/src/cmd/compile/README?utm_source=chatgpt.com)

For a change intended only to improve compiler speed, require three concrete results:

- **What work or allocation disappeared?**
- **How much did the representative build improve?**
- **What comparison established that compilation behavior stayed correct?**
