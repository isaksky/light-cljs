# Complete original duplicate record method remains rejected

The portable original `sci-record-method-duplicate-method` is unchanged:

```clojure
(defprotocol P (p [x])) (defrecord R [x] P (p [_] x) (p [_] 9)) (p (->R 3))
```

Source SHA-256 is
`a665b0da5f38af34aa5198dbe3e476edb3703dc2fdb1bc98bc8e6feb005965a6`;
original corpus row SHA-256 is
`afbfd88e0edf6735ff6783f2a4550ba9b90eeeed2d1b87993ed36cc3f005aaf9`;
original disposition row SHA-256 is
`91595b83c937b97adcbccba11672f288e289a9c6f553c6d023dac2ae0b489119`.
The original outcome is integer 9, with empty typed effects and stdout/stderr.

Both required pins accept the complete original source in `user` and return 9
with `optimizations:none/static-fns:false/def-emits-var:false`. No optional core
exclusion, library, renamed receiver, dropped implementation or changed result
is supplied. Current ordinary namespace compilation rejects the second method
with the located diagnostic `duplicate protocol method arity`, empty stdout and
exit 1. Fresh/reused actual MacroHosts reject it both when the original declaration
setup remains top level and when the entire unchanged contiguous original executes
inside a macro `do`. A clean macro compiles after each retained rejection.
No application engine execution or macro pass is inferred from a compile failure.

This is a specific pinned public record implementation discrepancy. The group
keeps protocols/types/records/extensions in intended scope. The record contract
promises pinned user protocol method behavior and does not retire repeated
same-arity implementations. Earlier native verification described stricter
validation, but that historical infrastructure cannot override the current
pinned language contract. This result does not imply relaxing genuine duplicate
protocol signature declarations, unknown protocols, missing receivers, invalid
method arities or duplicate/immutable field diagnostics.

All source, pin identity/settings, emitted dependencies, raw observations,
current compiler/CLI/audit-driver/binary hashes, four phase rejections, clean
controls, empty open duplicate searches and proposed focused acceptance are at
`/tmp/light-cljs-125-record-duplicate-method-confirmed-gap.json`, SHA-256
`c56459058e24afd870a2455ab73afef4412b855bca5a3a8c23d970fbc668811d`.
Raw artifacts are retained under:

- `/tmp/light-cljs-125-record-declaration-candidate-local-v1`
- `/tmp/light-cljs-125-record-declaration-candidate-reference-v1`
- `/tmp/light-cljs-125-record-duplicate-method-phase-v1` and its matching log.

Focused acceptance must preserve this complete original and literal 9, execute
both application engines and fresh/reused MacroHosts, and follow canonical
replacement of the existing method/arity by the later implementation. It must
protect other arities, receiver fields, declaration bundle publication, ordered
once-only effects, source/frontier/generation visibility and recovery after
failure. Existing malformed declaration/method tests remain required. This
cohort implements no production repair and changes no original row. After the
fresh live duplicate check, Pellet **light-cljs-165**, **Accept repeated record
protocol method arities with canonical last-method dispatch**, was added
immediately before 125. The live queue owns its status and implementation;
these artifacts retain the demonstrated discrepancy rather than a task list.

```sh
LIGHT_SCI_RECORD_DUPLICATE_METHOD_ROOT=/tmp/light-cljs-duplicate-method-rejection-fresh cargo test --locked --test compiler_sci_record_duplicate_method_probe original_duplicate_method_retains_phase_rejection -- --ignored --exact --nocapture
```
