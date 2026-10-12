# Original SCI symbol and keyword source context

The source adapter preserves the entire original `sci-symbols-keywords`
expression `['x 'alpha/x :x :alpha/x ::x]`, its original literal vector and the
original `user` namespace. This is one complete SCI source obligation. It adds
no upstream source assertions, public declarations, library bodies or programs.
The original source hash is
`06aa443910912f086e15bef5090fded87e5421e0e38cf9e0d831cb30a4bf8d8f`.

The application wrapper returns the unchanged expression from `run`. The actual
macro wrapper evaluates that same expression and quotes its complete returned
finite datum. The emitted application result must retain each symbol/keyword
namespace, name and spelling, including the `user/x` auto-resolved keyword.
A distinct completed warmup macro runs before each reused-host cohort. Node and
QuickJS observe each fresh/reused application and actual macro compilation;
all eight raw packets must match the immutable original result, empty trace
and empty stdout. Both checked references execute the exact original expression
with the unchanged source-context oracle in `user` and checked compiler options.

The helper snapshots original corpus/disposition bytes, compiler/build files,
exact producer/test/observer/oracle bytes, build output and execution binaries.
It rejects missing/reordered/duplicate frames, reduced or namespace-changed
sources, drifted profiles/results and absent reused-host warmup. No cached
receipt or static catalog row grants coverage. The full report runs this adapter
through `reviewed_sci_context_dispatch.py` and replays that invocation's proof
before assigning its one original source obligation.

Reproduction, using a fresh output path:

```sh
python3 -B tests/reference/compiler-language/sci_symbol_keyword_context.py --output /tmp/light-cljs-sci-symbol-keyword-fresh
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_sci_symbol_keyword_context.py
```
