# Fresh gap and printing proofs after raw replay guard repair

The exact fresh default gap and printing APIs ran under production HEAD
`b0f361d222ad7eacffcf0b402f278d4a9094057b` and the final frozen common helper.
No source, original expectation, canonical contract or production file changed.
The previous durable receipts were copied byte-for-byte to explicit historical
files before the current receipt copies were replaced.

| Cohort | Fresh evidence SHA-256 | Current result |
| --- | --- | --- |
| Default 30 canonical gap sources | `3cfedcfc53db239d566cb9d875cfe40bfc1ada510a62b36468cab6f4d01f22c7` | 3 original agreements, 27 explicit canonical differences, 0 missing; 240 actual frames |
| Printing 14 sources | `71246eae3075f93a617c03c6698a46eb6211417e789b403fc97b08c79a3a3ce6` | 14 explicit canonical differences, 0 missing; 112 actual frames |

The fresh raw proofs are
`/tmp/light-cljs-125-sci-gap-raw-replay-after172-v2/evidence.json` and
`/tmp/light-cljs-125-sci-print-raw-replay-after172-v2/evidence.json`. Their exact
copies are `sci-gap-canonical-receipt.json` and
`sci-print-canonical-receipt.json`. Current `reconcile` reconstructs each proof
from raw pin analysis, execution source rows, isolated Node observations,
source-bearing markers, original phase input, actual compilation/execution,
artifact census and current producer/compiler/executable hashes. All 352
fresh/reused application/actual-macro frames pass in QuickJS and Node.
The established sequence/list comparison projection remains unchanged; raw
typed packets and sequence/list brands remain retained in the evidence.

The default producer retains all 35 original diagnostic candidates. Each pin
honestly rejects the unselected ratio source; that source receives no canonical
credit. Every selected canonical-positive source must have accepted/warning
analysis and its actual raw execution. Every accepted diagnostic original is
also replayed with its correct original index and source markers. A serialized
positive observation cannot survive a rejected positive source's preflight.

Final common helper SHA-256 is
`82b91dfea3f7a5065213f1c7a682f37178ab3bc8c0c6176ed810ff21a0b568a6`;
the bound focused replay guard SHA-256 is
`29c91689e3dffd90c8bf48967f8ab2dbfc7afdd04281e619ca063adf26acc41a`.
The fresh run APIs verified their source snapshots before and after execution.
The final receipt replacement additionally checked unchanged historical/current
source bindings and every typed result/effect comparison against the original
explicit canonical contracts. Combined accounting is retained in
`sci-raw-replay-refresh-after172-receipt.json`: three original agreements,
41 canonical differences and zero missing across 44 selected sources. This
focused evidence does not establish comprehensive strict completion.

Historical copies remain:

- `sci-gap-canonical-after171-historical-receipt.json`, SHA-256
  `f46de9ecef5ca633a8266869318b1bad16459eb783ce8c95f872f8bed495745b`.
- `sci-print-canonical-after171-historical-receipt.json`, SHA-256
  `e4278f6c18f16803f82a88acedcbbea55941234a11b3f0da810d95223f31f7b4`.

The v1 roots at `/tmp/light-cljs-125-sci-gap-raw-replay-after172-v1` and
`/tmp/light-cljs-125-sci-print-raw-replay-after172-v1` are historical trials and
supply no current proof. Gap v1 exposed an overstrict guard requiring acceptance
of all 35 diagnostic originals. Printing v1 passed its then-current guard, but
the common helper's subsequent correction invalidated its source snapshot.
Both v2 producers were rerun from empty roots after the final correction.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/sci_gap_canonical_context.py --output /tmp/light-cljs-125-sci-gap-raw-replay-after172-v2 --timeout 600
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/sci_print_context.py --output /tmp/light-cljs-125-sci-print-raw-replay-after172-v2 --timeout 600
```

No shared producer/catalog, production source, queue, staging or commit was
modified by this refresh. The after172 arity proof remains retained. Pellet 156
and cancellation/limit suites remain untouched.
