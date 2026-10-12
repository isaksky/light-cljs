# Original SCI record contexts with explicit core-name shadows

The source cohort retains every byte, setup declaration, result and effect
of the complete original `sci-record-constructors`, `sci-record-empty` and
`sci-record-method-acceptance` sources. Original names `Box`, `Empty`, `Size`,
fields, record constructors, protocol methods and reify closure remain intact.

Canonical `cljs.core` exposes internal types named Box and Empty. The original
unchanged sources in the default `user` header produce mixed core constructor
and user prototype references, then TypeError in both checked references. These
exact failed controls must remain nonpassing. The reviewed source profile uses
`(ns user (:refer-clojure :exclude [Box]))` or `[Empty]` for the one name declared
by each original. No constructor is renamed and no other core name is excluded.
This explicit namespace environment permits each original user declaration to
own its constructor. The profile never grants a generic/default-header pass or
changes an original SCI expectation or disposition.

Application and actual macro execution retain all original source declarations
in their original order, then evaluate the complete original final expression.
The macro quotes its computed finite result after consuming genuine record and
reify intermediates; it does not substitute a literal expected value. A real
completed warmup macro precedes reused-host compilation. All24 original-source
frames must independently match the immutable result, effects and output in
fresh/reused Node and QuickJS application and macro heaps. Both fresh pinned
builds must pass each exact namespace profile and reproduce all three original
default-header TypeErrors. Source/row/disposition/profile, compiler/build,
producer/test/oracle, binary, source-tree and raw observation hashes are checked.
Missing, reordered, reduced, renamed, drifted or partial evidence fails closed.

This establishes at most three original SCI source obligations under explicit
namespace profiles. It adds no upstream assertions, public API declarations,
library bodies or source programs and does not resolve the distinct five
original record-tag reader assertions. The full report now runs this cohort
through `reviewed_sci_context_dispatch.py`; the ledger must replay its current
raw proof before assigning these exact three original source obligations.

```sh
python3 -B tests/reference/compiler-language/sci_record_source_context.py --output /tmp/light-cljs-sci-record-context-fresh
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_sci_record_source_context.py
```
