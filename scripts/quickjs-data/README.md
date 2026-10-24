# Generated QuickJS data experiment

This experiment converts three checked-in QuickJS datasets and NumberFormat
digit mappings into immutable,
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
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. `inputs.sha256` pins all eight local
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

## Checked-in Intl modules

The runtime uses the same verified output, repacked by `repack-intl.pl` into
`src/intl/data/{numbers,dates,locale,display_names,likely_subtags,preferences}.rs`.
The repacker selects the agreed test-locale records plus required parents and
only their referenced compact-pattern arrays. Shared byte pools and all their
indices remain intact, including embedded and terminal NULs. Keeping unused
pool entries is intentional: there is no binary-size cap in this stage.
Timezone and compressed Unicode integration remain with their consumer pellets.

Regenerate entirely inside a worktree (`target/intl-import` must be new):

```sh
nix develop --command bash scripts/quickjs-data/verify.sh \
  /home/dev/projects/clj-quickjs/research/quickjs target/intl-import
nix develop --command perl scripts/quickjs-data/repack-intl.pl \
  target/intl-import/first target/intl-repacked-new
cp target/intl-repacked-new/*.rs target/intl-repacked-new/provenance.txt \
  target/intl-repacked-new/QUICKJS-LICENSE src/intl/data/
```

`data/mod.rs` is handwritten; `docs/UNICODE-LICENSE` holds the Unicode v3
notice and is not regenerated. The provenance includes the exporter and
repacker hashes. To check the repack, repeat it against `second` in a fresh
output directory and compare the six `.rs` files and `provenance.txt` with
`cmp`. Run `cargo test --lib intl:: -- --test-threads=3` and
`cargo test --test js_source intl_namespace_object -- --exact --test-threads=3`
for consumer semantics. Normal Cargo builds need none of these tools or inputs.

## Checked-in Date timezone module

`repack-timezone.pl` selects the four timezone record types and all timezone
statics from the verified exporter output, preserving every value, reference,
name, alias and region. It writes `tables.rs`, `provenance.txt` and the QuickJS
license. Regenerate inside a worktree (both output directories must be new):

```sh
nix develop --command bash scripts/quickjs-data/verify.sh \
  /home/dev/projects/clj-quickjs/research/quickjs target/tz-import
nix develop --command perl scripts/quickjs-data/repack-timezone.pl \
  target/tz-import/first target/tz-repacked
cp target/tz-repacked/{tables.rs,provenance.txt,QUICKJS-LICENSE} src/host/timezone/
```

Repeat the repack on `second` into another new directory and compare all three
files with `cmp`. The safe runtime reader is `src/host/timezone.rs`; its Date
semantics tests run with `cargo test --test date_host -- --test-threads=3`.
The 59 transition-bearing zones and 36 aliases are supported; the 598-name
inventory alone never grants offset coverage. First/last offsets extend beyond
the recorded transitions without future-rule extrapolation.

### NumberFormat digit mappings (clj-quickjs-213)

`intl/intl.c` is an additional pinned input. The exporter extracts only
`intl_all_numbering_systems`, `nf_digit_base` and `hanidec_cps`, compiles their
original initializers alongside the existing data exporter, and compares every
C/Rust value through the existing wire format. Designated digit-base indices
are evaluated by C, including zero entries for algorithmic systems. No Intl
service code is linked or executed. The repacker includes the three immutable
statics in `numbers.rs`. Verification includes numbering indices, non-BMP digit
blocks, algorithmic zero slots and Han decimal digits.

## Unicode identifiers, RegExp simple folding and property escapes

`repack-unicode.pl` uses the verified import's `case_conv_*`, alias pools and ID table
snapshot as the starting data. It checks the input hashes again and compiles
the pinned `libunicode.c` solely as an offline reference decoder. It emits
safe immutable Rust mappings/ranges, not a linked C runtime. The extra decoder
and UCD hashes are pinned in the repacker and recorded in the generated
`src/regexp/case_fold_provenance.txt`. Unicode and QuickJS notices remain in
`docs/UNICODE-LICENSE` and `src/intl/data/QUICKJS-LICENSE`.

Unicode 17.0.0 is verified independently: every code point's folding agrees
with CaseFolding.txt C/S entries, and both identifier properties agree with
DerivedCoreProperties.txt. The pinned QuickJS reference chooses FB05 as the
representative of the FB05/FB06 equivalence class; the repacker checks that
exact exception and reverses it to the normative C/S representative FB06.
No other difference is permitted. Identifiers replace the old host-Perl
Unicode 16.0.0 tables; regeneration no longer depends on Perl's UCD version.
The shared policy for -191/-192/-209 is Unicode 17.0.0, with each additional
property, normalization, collation or segmentation consumer verifying its own
data. The locale snapshot does not identify a separately verified CLDR release.

Acquire the versioned inputs once (the repacker rejects a wrong version or
hash), then regenerate inside the worktree. C, Perl, rustc and the UCD files
are needed only for this explicit maintenance command:

```sh
mkdir -p target/ucd-17
curl -fsSL https://www.unicode.org/Public/17.0.0/ucd/CaseFolding.txt \
  -o target/ucd-17/CaseFolding.txt
curl -fsSL https://www.unicode.org/Public/17.0.0/ucd/DerivedCoreProperties.txt \
  -o target/ucd-17/DerivedCoreProperties.txt
for file in UnicodeData.txt PropList.txt DerivedNormalizationProps.txt; do
  curl -fsSL "https://www.unicode.org/Public/17.0.0/ucd/$file" -o "target/ucd-17/$file"
done
curl -fsSL https://www.unicode.org/Public/17.0.0/ucd/extracted/DerivedBinaryProperties.txt \
  -o target/ucd-17/DerivedBinaryProperties.txt
curl -fsSL https://www.unicode.org/Public/17.0.0/ucd/emoji/emoji-data.txt \
  -o target/ucd-17/emoji-data.txt
nix develop --command bash scripts/quickjs-data/verify.sh \
  /home/dev/projects/clj-quickjs/research/quickjs target/unicode-import
nix develop --command bash scripts/gen-unicode-ident.sh \
  target/unicode-import/first /home/dev/projects/clj-quickjs/research/quickjs \
  target/ucd-17 target/unicode-repacked-new
nix develop --command bash scripts/quickjs-data/verify.sh \
  /home/dev/projects/clj-quickjs/research/quickjs target/unicode-check-new target/ucd-17
```

The optional third verification argument also checks byte-identical repeated
expansion and provenance, compares the actual installed Rust folding and
identifier and property consumers across all 1,114,112 code points against the reference,
and rejects deliberately corrupted decoded values and changed UCD inputs.
It must be run after installing regenerated data. RegExp semantics are covered
separately by `cargo test --lib regexp::` and
`cargo test --test js_source regexp_case_insensitive_unicode -- --exact`.

The same command installs `src/regexp/property_data.rs`. It contains all 53
ECMA-262 binary properties and 38 General_Category values; exact aliases come
from the verified NUL-separated pools. QuickJS-only binary names are filtered
by an explicit ECMAScript allowlist. Script/Script_Extensions and v string
properties remain rejected, with language-level tests pinning that scope.
Every raw set (including derived binary properties) is independently compared
with pinned Unicode 17.0.0 UCD input. The offline decoder's intervals are
intersected with U+0000..U+10FFFF, including lone surrogates. Compact ranges
for extra folded targets are generated separately for members and nonmembers,
preserving u-mode complement-before-folding semantics. Verification covers
all aliases, positive/complement membership and both iu images across the
entire domain, and rejects an intentionally corrupted property bit. Run the
language tests with `cargo test --test js_source regexp_property_escapes -- --exact`.
