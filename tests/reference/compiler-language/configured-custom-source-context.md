# Complete original custom source reader context

This source adapter retains the byte-identical complete original
`data_readers_test/core.cljc` module, its sole top-level assertion and complete
original `data_readers.cljc` map. It supplies two reviewed compiler reader
handlers before reading: `test/custom-identity` uses the original identity alias;
`test/custom-form` uses the exact form-producing body of `custom-form-cljs`'s
original `:clj` compilation branch. The latter is `(fn [x] `(js/Array.of ~x))`.
The source compiler selects the map's `:cljs` handler name, then the reference
JVM executes that handler's `:clj` body. Replacing this handler with identity
would change the full original module's result. Both failed initial identity
assumptions remain in the temporary reference control history.

The local handler is ordinary compiled ClojureScript source implementing the
original form-producing branch. It does not load a JVM companion. The original
module's reader conditionals, documented runtime `custom-form-cljs` function,
complete original assertion and result initialization remain intact. The
supplemental driver checks that the initialized result is a native array
containing exactly `"foo"`; both canonical pins must independently agree with
that literal observation. The supplemental check never replaces the original
assertion.

Application execution initializes the entire unchanged module before calling
the driver. Actual macro execution wraps every original non-namespace form in
one `do`, preserving source bytes/order/setup/assertion and reader conditionals,
then checks the original result. Both phases return literal true after all checks.
Fresh and reused hosts each execute independently in Node and QuickJS, with an
actual prior warmup macro for reuse. The helper requires eight complete ordered
raw packets and current both-pin builds, exact original/configuration/profile
hashes, compiler/producer/build/binary provenance, and unchanged raw values,
effects and output. It rejects shortened or changed source bodies and missing
configuration. Generic CLI tags, arbitrary custom handlers, JVM companions and
implicit record-tag constructors receive no coverage claim.

The shared hosted dispatcher binds exactly the sole original upstream assertion
after this adapter's current raw proof passes. The standalone helper itself
returns zero catalog credit. No public API, assertion-free body or additional
source program is registered by this adapter. See full-scope.md and final-gate.md
for the integrated source witness and current execution account.

```sh
python3 -B tests/reference/compiler-language/configured_custom_source_context.py --output /tmp/light-cljs-configured-custom-source-fresh
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_configured_custom_source_context.py
```
