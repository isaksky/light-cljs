# Exact original character/string case rejection

The unchanged SCI original is:

```clojure
[(case \a "a" 1 \a 2 3) (case "a" \a 1 "a" 2 3)]
```

Original source SHA-256:
`2ffde7c522ee281f970f51bd142f5c19ec3dea01dba215b1b08d596f180a15c9`.
The historical SCI typed result remains `[2 2]`. Fresh independent release
1.12.134 and checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e builds
both execute the complete original and return `[1 1]`, with empty trace and
stdout, checked compiler identity/settings and exact source-bearing markers.
The explicit namespace is `user`; `.cljc` reference filename selection does not
alter this original, which contains no reader conditional.

Current application sources in fresh and reused MacroHosts reject
`duplicate case test constant` at the original `case` form (wrapper line 2,
column 34, bytes 43..45). Complete demanded macro sources preserve the same
original inside `(list 'quote (do ...))` and invoke `(upstream)`. They reject
while defining the macro at wrapper line 2, column 1, bytes 10..100. This is a
compilation rejection of the demanded source; the rejected macro body does not
produce an exported result. No runtime observation of this original is invented.

Every failed compilation is followed on the same actual MacroHost by the
control `(defmacro case-recovery [] (+ 20 22)) (case-recovery)`. QuickJS and
Node both execute its compiled result as 42 for each of the four failed source
contexts. The reused host was first warmed with another actual macro. All
original sources, errors, control source/code, Node stdin/stdout/stderr, Cargo
build and exact test command are retained. The receipt binds current compiler,
producer and executable hashes and a complete raw artifact census.

This demonstrates one focused public compatibility gap: the local `case`
expansion rejects character/string constants that the two canonical pins
accept under their character/string representation. The canonical `[1 1]`
observation also differs from immutable SCI `[2 2]`; repairing local compilation
must retain that distinction. This diagnostic supplies zero semantic, local
execution and macro-phase credit, and modifies no shared ledger or production
implementation.

Fresh proof:
`/tmp/light-cljs-125-case-char-string-after172/evidence.json`.
The durable byte-exact copy is `case-char-string-gap-receipt.json`, SHA-256
`5a6da45d7b33e1e4a30797c90907c1aa8f79b9f074e046e120f4db8b630666c7`.
Three source/binding guards passed. The exact ignored Rust test executed once,
with four original rejections and eight same-host engine recovery observations.
The helper is diagnostic-only and is not an accepted positive replayer.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/case_char_string_gap.py --output /tmp/light-cljs-125-case-char-string-after172
PYTHONDONTWRITEBYTECODE=1 python3 -B -m unittest discover -s tests/reference/compiler-language -p test_case_char_string_gap.py
```
