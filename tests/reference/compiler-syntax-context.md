# Complete syntax execution contexts

The seven source contexts in `tests/fixtures/compiler-syntax-context.tsv` supply
independent literal observations for 21 pinned analyzer special forms. The
Rust audit runs each complete source as an application and in an actual
MacroHost body, then evaluates the emitted result in QuickJS and Node. MacroHost
itself uses the project's QuickJS compiler environment; a Node result does not
claim a separate Node compiler host. The six expression contexts serialize the
whole observed value inside MacroHost before exporting a string expansion, so
quoted lists remain data. Every ordered effect/error observation is part of the
adjacent independent literal, rather than inferred from form presence.

| Context | Executed syntax contract |
| --- | --- |
| control-rest-recursion | `if`, `fn*`, `do`, `let*`, `loop*`, `letfn*`, `recur`, `&`: chosen/unchosen paths, accumulation, mutually recursive functions, variadic binding, ordered effects |
| exception-interop-template | `new`, `set!`, `.`, `js*`, `try`, `throw`: Array construction/mutation, method/property access, ordered template operands, Error message, catch and finally effects |
| quote-case-star | `quote`, `case*`: retained list/map data, grouped/default paths, exactly one selector evaluation |
| definition-var-state | `def`, `var`: initial/current root values, repeated var identity, name and exception data |
| raw-type-constructor | `deftype*`: exact raw constructor, field value/mutation, instance identity and one argument effect |
| raw-record-constructor | `defrecord*`: exact raw constructor, field mutation, instance identity, internal nil slots and one argument effect |
| namespace-header-context | `ns`: required alias, referred imported macro, declared/qualified var identity and actual caller `&env` namespace |

Raw constructor evidence covers the compiler IR contracts directly; it claims
no high-level record map protocols or arbitrary native interop domain. Canonical
pin oracles execute the same complete source next to its literal equality
expectation. Native JavaScript macro execution is the adopted local dialect;
the JVM reference checks the complete value/effect/error contract in its
ordinary ClojureScript compilation phase. It does not pretend that JVM macros
can execute JavaScript-only forms such as `js*` or native constructors.

For the namespace context, the explicit reference source tree changes only the
import clause to the checked JVM `:require-macros` companion model. The local
ordinary `:require` discovers the macro in the `.cljs` observer. Both `.cljs`
and `.clj` companions preserve the same macro body, and the runtime definition,
full function body, namespace identity and independent expectation remain
byte-identical. The generator verifies that precise adapter and hashes every
file in both trees. This evidence never places `ns` inside a value expression.

`syntax_context.py --export-owned DIRECTORY` emits only owned candidate
ordinary/phase/case/receipt rows. The parent coordinates shared table replacement
and source inventory regeneration. Candidates bind the exact canonical
specials body/signature, complete named Rust driver, fixture/helper and both
namespace trees. Every source/expectation and reviewed form list is frozen;
quoted, discarded, metadata-only, string and free `&` text cannot supply an
executing form witness. The report must run the named phase test and both
checked pins before crediting a candidate. Existing reader `syntax::&` evidence
is preserved in its original test; coordinated replacement connects the public
syntax row to this complete independent rest-binding oracle as well.

`syntax::ns*` is deliberately absent. The unchanged complete probe at
`/tmp/light-cljs-125-syntax-nsstar-minimal/source/syntax/nsstar.cljs` preloads
`clojure.string`, executes raw `ns*` to introduce `text`, then returns the
independent `['A', '1-2']` observation through that alias. Its source SHA-256 is
`ebe59a510ec23b6b0d43bbf8927dc149714e1c248abee2c6ca06faef9180e957`.
Both checked pins return the literal `true` contract while local compilation
rejects `unsupported construct: ns*` at bytes 48..51. Complete source, generated
pin modules, pin identities/options and local failure are preserved in
`/tmp/light-cljs-125-syntax-nsstar-minimal/receipt.json`. No operation was removed
to force a local pass and no production fix is included here.

Earlier failed drafts remain under `/tmp/light-cljs-125-syntax-draft*` and
`/tmp/light-cljs-125-syntax-namespace-draft*`. The invalid numeric dot member,
unquoted list-valued macro expansion, retired local `:require-macros`, invalid
CLI namespace evaluation option and initial upstream missing dependency launch
are retained as history. Their observations are not promoted as execution
passes or independent production gaps.
