# Third-party notices

light-cljs is licensed under the MIT License (see [LICENSE](LICENSE)), except
for the material listed here, which keeps its upstream license. Each entry
names the upstream project, its license, the pinned revision, and the files in
this repository that derive from it. When adapting more upstream material, keep
the upstream notice in the file, name the pinned revision, and add the path
here.

## ClojureScript

- Upstream: <https://github.com/clojure/clojurescript>, pinned at commit
  `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.
- Copyright (c) Rich Hickey. All rights reserved.
- License: Eclipse Public License 1.0, reproduced in [epl-v10.html](epl-v10.html).
- Files in this repository adapted from ClojureScript are EPL-1.0 and say so in
  a header notice that must not be removed:
  - `src/compiler_sequence_macros.cljs` (bootstrap macros adapted from
    `cljs/core.cljc`)
  - `src/compiler_libraries/clojure/set.cljs`
  - `src/compiler_libraries/clojure/string.cljs`
  - Test fixtures under `tests/fixtures/` whose header carries the ClojureScript
    notice. These hold complete or adapted bodies from the upstream test suite
    (`src/test/cljs/...`), with per-body SHA-256 provenance recorded in the
    fixture files and checked by `tests/provenance.rs`.
  - `tests/reference/generate_core_var_metadata.clj` and the metadata it reads
    from the upstream checkout.
- Compiled programs that use `clojure.string` or `clojure.set` embed JavaScript
  compiled from the EPL-1.0 files above. The rest of the emitted runtime
  (`src/js_runtime.js`) is original MIT-licensed work.

Some tests additionally read the upstream checkout at run time from the
gitignored `research/clojurescript` directory. Nothing from that checkout is
committed beyond the adapted files listed above.

## SCI (Small Clojure Interpreter)

- Upstream: <https://github.com/babashka/sci>, pinned at commit
  `5fe89056384418be50d232b4278344cb3a1a526b`.
- Copyright © 2019-2022 Michiel Borkent.
- License: Eclipse Public License 1.0, reproduced in
  `tests/reference/sci/SCI-LICENSE.txt`.
- Files: `tests/fixtures/sci-outcomes.tsv` records assertions adapted from the
  SCI test suite and their observed results, with per-row origins in
  `tests/reference/fixture-origins.tsv`. The detailed attribution, including
  a few assertions that SCI itself took from Clojure's test suite (Copyright (c)
  Rich Hickey, EPL-1.0), is in
  [tests/reference/sci/NOTICE.md](tests/reference/sci/NOTICE.md).

## QuickJS-ng (via rquickjs-sys)

- Upstream: <https://github.com/quickjs-ng/quickjs>, version 0.16.2, as bundled
  by the `rquickjs-sys` 0.14.0 crate and vendored with one engine patch in
  `vendor/rquickjs-sys/` (see `vendor/rquickjs-sys/PATCHES.md`).
- Copyright (c) 2017-2026 Fabrice Bellard, (c) 2017-2024 Charlie Gordon,
  (c) 2023-2026 Ben Noordhuis, (c) 2023-2026 Saúl Ibarra Corretgé.
- License: MIT, reproduced in `vendor/rquickjs-sys/quickjs/LICENSE`.

## rquickjs and rquickjs-sys

- Upstream: <https://github.com/DelSkayn/rquickjs>, version 0.14.0. The
  `rquickjs-sys` crate is vendored in `vendor/rquickjs-sys/`; `rquickjs` is a
  normal Cargo dependency.
- License: MIT.

## Other Cargo dependencies

Transitive crates are listed in `Cargo.lock` with their own licenses in their
package metadata. `cargo tree` or `cargo about` can produce a full report.
