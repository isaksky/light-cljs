# Generated QuickJS data experiment

This experiment converts the three checked-in QuickJS datasets into immutable,
safe Rust using a repeatable Bash/Perl/C pipeline. It does not integrate them with
the runtime or alter the reference checkout. All build and generated artifacts
go into a caller-selected new directory.

## Run

From the repository root, with the repository's development environment:

```sh
nix develop --command bash scripts/quickjs-data/generate.sh \
  research/quickjs \
  /tmp/quickjs-generated-data-new
```

The last directory must not already exist. The script needs Bash, Perl, a C11
compiler (`CC`, default `cc`), `rustc`, and standard Unix tools. It performs no
downloads and requires no Cargo dependencies. The Perl program is internal to
the script: invoke `generate.sh` so input checks and verification are enforced.

To repeat generation twice, check byte-for-byte reproducibility, run semantic
edge cases, and test rejection of corrupted output and changed input:

```sh
nix develop --command bash scripts/quickjs-data/verify.sh \
  research/quickjs \
  /tmp/quickjs-data-verification-new
```

## Conversion

Inputs are pinned to QuickJS revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. `inputs.sha256` pins all seven local
data/header files read by the exporter. Hashes are checked before and after
conversion; an upstream change needs an explicit manifest and converter review.

1. A restricted Perl reader inventories declarations, initializers and twelve
   data record types from the pinned C source and headers. It rejects unknown
   declaration/field shapes. This is not a general-purpose C parser.
2. It emits a C exporter that includes the original three data files with
   `CONFIG_ALL_UNICODE` enabled. The C compiler evaluates numeric expressions,
   enum constants, macros, strings, and bitfields.
3. The exporter prints `tables.rs`: immutable statics, fixed-width integers,
   ordinary Rust structs, slices, and optional references. There are no raw
   pointers, mutable statics, FFI calls, or C-layout bitfields in that output.
4. The same C exporter serializes the actual C values into an explicitly
   little-endian logical stream. A separately compiled Rust program walks every
   generated static using its Rust type, serializes it, and compares the files.
   Struct padding, native addresses, and native byte order are never serialized.

String data uses byte slices, preserving UTF-8 bytes, embedded NUL separators,
and the compiler-added trailing NUL. `NULL` is `None`; an empty C string is
`Some(b"\x00")`. Pointer references to named arrays become references to the
corresponding Rust statics. Numbers inside C bitfields become ordinary unsigned
Rust fields. Array lengths and expected initializer counts are checked.

The generated file and its Rust consumer both use `#![forbid(unsafe_code)]`.
The verifier builds on stable Rust with warnings denied. C is needed only to
regenerate/validate data, not to consume the generated Rust.

## Results

Two complete runs produced byte-identical Rust and comparison streams. Both
C/Rust value comparisons, all four edge-case tests, mutation detection, and
input-drift rejection passed. The observed results were:

| Item | Observed result |
| --- | ---: |
| Exported declarations, including enum constants | 666 |
| Rust record types | 12 |
| Locale records | 769 |
| Likely-subtag entries | 7,788 |
| Display-name locales | 2 |
| Zones with transition data | 59 |
| Available timezone names | 598 |
| Generated Rust source | 2,395,464 bytes |
| Logical comparison stream | 1,512,780 bytes |

The generated tables preserve the entire input snapshot, including Unicode's
compressed arrays. The different counts for timezone names and transition
records are an upstream coverage limitation, not dropped data. The locale count
above comes from the compiled data; it is larger than the older count recorded
in the locale-policy pellet.

`edge_cases.rs` checks embedded NULs in likely subtags and month pools, French
UTF-8 month names and U+202F grouping, negative timezone offsets, `INT64_MIN`,
transition ordering, alias indices, packed locale fields, and reference lengths.
`verify.sh` also changes one generated count and verifies the differential check
detects it, and changes an input file to prove the hash guard rejects it before
compilation.

## C2Rust comparison

```sh
bash scripts/quickjs-data/compare-c2rust.sh \
  research/quickjs \
  /tmp/quickjs-c2rust-new
```

This copies the source outside the reference checkout and runs installed C2Rust
0.22.1 with `--fail-on-error`. All three translation units transpiled without
diagnostics after adding explicit exported roots for Unicode's static tables.
Without those roots, C2Rust silently omitted the unused header tables and
emitted an almost empty Unicode module.

The resulting C2Rust files total 8,043,979 source bytes and 162,698 lines. They
contain raw pointers, `static mut`, bitfield support derives, and a feature
attribute. These are successful translation results, not a claim that the
generated modules compile or validate as a standalone Rust crate; that step was
not attempted. Source-size differences do not measure runtime binary size.

For data-only conversion, the direct exporter gives a more suitable result:
safe Rust that compiles and has a complete value comparison, without a cleanup
pass or C2Rust support dependencies.

## Limits and next steps

- This verifies data fidelity, not Unicode/Intl/Temporal runtime semantics.
  Unicode compression remains unchanged; safe decoders or offline expansion
  still need their own semantic comparisons with QuickJS.
- This is a snapshot importer. It does not reconstruct the original CLDR
  release or the system tzdb used by QuickJS's generators. Future upstream-data
  updates need separately pinned source releases and a reviewed upgrade process.
- Locale records use wider fields than the C bitfields. Measure linked binary
  size before selecting a production representation or supported locale subset.
- Rust exposes C byte pools including terminators; a production API should
  provide appropriate string/pool accessors, and split modules by service.
- `provenance.txt` records input and generator hashes plus tool versions.
  `QUICKJS-LICENSE` accompanies each result. Preserve applicable upstream data
  notices when preparing a distributable data package.
- The converter has no build.rs integration, runtime registration, dependency
  changes, or Pellets edits. It can be evaluated independently of current work.

The generated C is intentionally straightforward and can take tens of seconds
to compile. Its size and compilation strategy can be improved independently of
the Rust data layout if this experiment becomes maintained tooling.
