# Exact private tap registry scope

`private_tap_scope.validate()` returns only the two original assertions in
`core_test.cljs::test-cljs-2549`. These inspect the representation and contents
of the explicitly private upstream `cljs.core/tapset` registry. This applies the
user-adopted private storage/ABI boundary from compiler-language-completion group
186, revision 2; it grants no semantic or execution credit. The group context
SHA256 is `f3e25d6a72914bb406675ce959d086d5112288d036622c224d1fd38b1c2f151a`.
The group explicitly states that “private aliases/internal storage classes must
not be falsely counted as missing public features.” The narrower application to
these two assertions follows source inspection, rather than a blanket exclusion
of public tap delivery or a response to missing implementation.

The complete original body remains:

```clojure
(deftest test-cljs-2549
  (let [tap (fn [_])]
    (add-tap tap)
    (is (set? @tapset))
    (is (contains? @tapset tap))
    (remove-tap tap)))
```

Body SHA256: `0c5c65c2832893c54f613b4851f936d1062518fce1004acc29a250eda25c752e`.
The callback is never delivered a value. Public calls supply setup and cleanup;
both assertions directly dereference private storage. A new public delivery
oracle would be independent evidence, not an unchanged execution of this body.

| Exact assertion ID suffix | Original assertion | SHA256 |
| --- | --- | --- |
| `test-cljs-2549::assertion-001` | `(is (set? @tapset))` | `806b993a37887e8989048f8b1c57439cd83c76e39f92476fed1d40fbac328d98` |
| `test-cljs-2549::assertion-002` | `(is (contains? @tapset tap))` | `d055bc96f584c4e4e7b0f0a6c05279d526b4c78cd83010cb76b93d9b60b0434f` |

Both required reference pins declare the identical form:

```clojure
(defonce ^{:jsdoc ["@type {*}"] :private true}
  tapset nil)
```

Declaration SHA256:
`35e0d01a09454fd1d4e308cd81b33fe9ade1d0957b553b233fc4dda87b26094a`.
It begins at line 12034 in the actual release JAR and line 12061 in checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.

| Retained source evidence | SHA256 |
| --- | --- |
| Actual `clojurescript-1.12.134.jar` | `d019b73177e4805748f4ee995c4cfcd14ebf28bff685a84059813fbe4e8c878f` |
| Actual release JAR entry `cljs/core.cljs` | `35344afe7d6a56a610d85dd759a96eb84a0aac398faa80d31eb81ecdbd127e80` |
| Pinned checkout `src/main/cljs/cljs/core.cljs` | `9734617a341b58f65bdf0786d895d920c83b5ca5b5ca4863e768ec65053927ca` |
| Pinned checkout complete `src/test/cljs/cljs/core_test.cljs` | `9371b1c18ea3ad5b8f0f1eab540ffbdb7241b24a6e00acfbd6bd036addba1dd8` |
| Release tag complete `src/test/cljs/cljs/core_test.cljs` | `2e1c82c99f78301afbc7e4bd26a32ff060d50754972907e3d819d148fba12b5d` |

The release tag `r1.12.134` resolves to
`c4295f303100bbf5afac449242d30bca1126f1a1`. Its complete test-cljs-2549 body
and both assertions match the checkout hashes above. The release JAR contains
core source, not the test source. Its core file differs from the release tag only
at the build-generated `*clojurescript-version*` initializer; the private
declaration itself was verified directly inside the actual JAR. Typed
`RELEASE_EVIDENCE` retains that read-only receipt. Ordinary `validate()` does not
open a JAR, call Maven or execute ClojureScript.

The helper fails closed on the checkout revision, whole source files, working
checkout versus Git pin, explicit private metadata, exact declaration, complete
original setup/body/cleanup and both original assertions. It also guards the
unchanged public declarations and the complete `test-cljs-2552` body.
`add-tap`, `remove-tap`, `tap>` and `(is (boolean? (tap> nil)))` remain intended;
registration, removal, callback delivery and applicable actual phases still need
independent meaningful acceptance. The helper returns no public exclusions.

Run the independent guards with:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B -m unittest discover -s tests/reference/compiler-language -p test_private_tap_scope.py -v
```

Parent integration must wait for the frozen comprehensive gate to finish. Only
the two exact returned assertion bindings may follow the existing
`PRIVATE_EQUALITY_ASSERTIONS` disposition pattern; whole body/source retention
and public acceptance stay unchanged. The helper and guards do not modify the
shared ledger, reports, inventories or manifests, and no tap/host facility is
executed by this scope review.

All nine guards passed in 3.449 seconds on production HEAD
`8be52ad6d2e37e0c9e9a7a64a61244f14f7094d9`. Scope-only receipt:
`/tmp/light-cljs-125-private-tap-scope-receipt-v1.json`, SHA256
`e0151925edf79fc4837114db23c4d7f662528e42b8effb6e60e8deb9312b1e34`.
It retains both exact bindings, helper/guard/log hashes, recorded release evidence
and unchanged shared producer/table hashes. The raw guard log is
`/tmp/light-cljs-125-private-tap-scope-guards-v1.log`. Neither this receipt nor the
passing Python guards establish any tap semantic observation.
