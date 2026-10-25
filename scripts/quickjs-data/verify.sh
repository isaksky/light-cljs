#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 2 && $# != 3 ]]; then
    echo "usage: bash $0 QUICKJS_SOURCE NEW_OUTPUT_DIRECTORY [UCD_17_DIRECTORY]" >&2
    exit 2
fi
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source_dir=$(realpath "$1")
output_dir=$(realpath -m "$2")
case "$output_dir/" in
    "$source_dir/"*) echo 'output must be outside the QuickJS source' >&2; exit 2 ;;
esac
mkdir -- "$2"
output_dir=$(realpath "$2")
bash "$script_dir/generate.sh" "$source_dir" "$output_dir/first"
bash "$script_dir/generate.sh" "$source_dir" "$output_dir/second"
cmp "$output_dir/first/tables.rs" "$output_dir/second/tables.rs"
cmp "$output_dir/first/script_names.rs" "$output_dir/second/script_names.rs"
cmp "$output_dir/first/c-values.bin" "$output_dir/second/c-values.bin"
cp "$script_dir/edge_cases.rs" "$output_dir/first/edge_cases.rs"
rustc --edition=2021 -D warnings --test "$output_dir/first/edge_cases.rs" -o "$output_dir/first/edge_cases"
"$output_dir/first/edge_cases"

# Prove the cross-language comparison detects an incorrect generated value.
mkdir "$output_dir/corrupt"
cp "$output_dir/first/verify.rs" "$output_dir/corrupt/verify.rs"
perl -pe 's/pub static locale_data_count: i32 = 769;/pub static locale_data_count: i32 = 768;/' \
    "$output_dir/first/tables.rs" > "$output_dir/corrupt/tables.rs"
if cmp -s "$output_dir/first/tables.rs" "$output_dir/corrupt/tables.rs"; then
    echo 'mutation did not apply' >&2; exit 1
fi
rustc --edition=2021 "$output_dir/corrupt/verify.rs" -o "$output_dir/corrupt/verify"
"$output_dir/corrupt/verify" > "$output_dir/corrupt/rust-values.bin"
if cmp -s "$output_dir/first/c-values.bin" "$output_dir/corrupt/rust-values.bin"; then
    echo 'incorrect data escaped comparison' >&2; exit 1
fi

# Input drift must fail before conversion. Only copy the pinned input files.
mkdir -p "$output_dir/drift/intl" "$output_dir/drift/temporal"
while read -r digest path; do
    cp "$source_dir/$path" "$output_dir/drift/$path"
done < "$script_dir/inputs.sha256"
echo '/* intentional drift */' >> "$output_dir/drift/libunicode-table.h"
if bash "$script_dir/generate.sh" "$output_dir/drift" "$output_dir/drift-result" \
    > "$output_dir/drift.log" 2>&1; then
    echo 'unpinned input was accepted' >&2; exit 1
fi
test ! -e "$output_dir/drift-result/export.c"
echo 'PASS: repeatable generation, edge cases, mutation detection, and input pinning'

if [[ $# == 3 ]]; then
    perl "$script_dir/repack-unicode.pl" "$output_dir/first" "$source_dir" "$3" "$output_dir/unicode-first"
    perl "$script_dir/repack-unicode.pl" "$output_dir/second" "$source_dir" "$3" "$output_dir/unicode-second"
    for file in case_fold_data.rs unicode_ident.rs property_data.rs provenance.txt reference.bin legacy-reference.bin property-reference.bin; do
        cmp "$output_dir/unicode-first/$file" "$output_dir/unicode-second/$file"
    done
    rustc --edition=2024 -D warnings "$output_dir/unicode-first/verify.rs" -o "$output_dir/unicode-first/verify"
    "$output_dir/unicode-first/verify" "$output_dir/unicode-first/reference.bin" "$output_dir/unicode-first/legacy-reference.bin"
    rustc --edition=2024 -O -D warnings "$output_dir/unicode-first/verify-properties.rs" -o "$output_dir/unicode-first/verify-properties"
    "$output_dir/unicode-first/verify-properties" "$output_dir/unicode-first/property-reference.bin"
    perl -e 'local $/; open my $f, "<:raw", $ARGV[0] or die $!; my $s = <$f>; substr($s, 0, 1) = chr(ord(substr($s, 0, 1)) ^ 1); print $s' \
        "$output_dir/unicode-first/property-reference.bin" > "$output_dir/unicode-first/property-corrupt.bin"
    if "$output_dir/unicode-first/verify-properties" "$output_dir/unicode-first/property-corrupt.bin" > "$output_dir/property-corruption.log" 2>&1; then
        echo 'incorrect property value escaped comparison' >&2; exit 1
    fi
    # A wrong decoded value must fail the runtime differential comparison.
    perl -e 'local $/; open my $f, "<:raw", $ARGV[0] or die $!; my $s = <$f>; substr($s, 65 * 5, 4) = pack("V", 65); print $s' \
        "$output_dir/unicode-first/reference.bin" > "$output_dir/unicode-first/corrupt.bin"
    if "$output_dir/unicode-first/verify" "$output_dir/unicode-first/corrupt.bin" "$output_dir/unicode-first/legacy-reference.bin" > "$output_dir/unicode-corruption.log" 2>&1; then
        echo 'incorrect decoded Unicode value escaped comparison' >&2; exit 1
    fi
    perl -e 'local $/; open my $f, "<:raw", $ARGV[0] or die $!; my $s = <$f>; substr($s, 97 * 4, 4) = pack("V", 97); print $s' \
        "$output_dir/unicode-first/legacy-reference.bin" > "$output_dir/unicode-first/legacy-corrupt.bin"
    if "$output_dir/unicode-first/verify" "$output_dir/unicode-first/reference.bin" "$output_dir/unicode-first/legacy-corrupt.bin" > "$output_dir/legacy-corruption.log" 2>&1; then
        echo 'incorrect legacy Canonicalize value escaped comparison' >&2; exit 1
    fi
    mkdir "$output_dir/ucd-drift"
    cp "$3/"*.txt "$output_dir/ucd-drift/"
    echo '# unexpected input version' >> "$output_dir/ucd-drift/CaseFolding.txt"
    if perl "$script_dir/repack-unicode.pl" "$output_dir/first" "$source_dir" "$output_dir/ucd-drift" "$output_dir/ucd-drift-result" \
        > "$output_dir/ucd-drift.log" 2>&1; then
        echo 'unpinned UCD accepted' >&2; exit 1
    fi
    test ! -e "$output_dir/ucd-drift-result/export.c"
    echo 'PASS: Unicode repeatability, full-domain runtime comparison, corruption and version guards'
fi

# Root collation consumes the same verified Unicode snapshot through a pinned
# offline decoder. Compare every expanded NFD/fold/CCC value, repeat generation,
# and prove that corrupting the emitted representation is detected.
perl "$script_dir/repack-collation.pl" "$output_dir/first" "$source_dir" "$output_dir/collation-first"
perl "$script_dir/repack-collation.pl" "$output_dir/second" "$source_dir" "$output_dir/collation-second"
for file in collation.rs provenance.txt reference.txt; do
    cmp "$output_dir/collation-first/$file" "$output_dir/collation-second/$file"
done
rustc --edition=2024 -D warnings "$output_dir/collation-first/verify.rs" -o "$output_dir/collation-first/verify"
"$output_dir/collation-first/verify" "$output_dir/collation-first/reference.txt"
mkdir "$output_dir/collation-corrupt"
cp "$output_dir/collation-first/verify.rs" "$output_dir/collation-corrupt/"
sed '0,/0x41/s/0x41/0x40/' "$output_dir/collation-first/collation.rs" > "$output_dir/collation-corrupt/collation.rs"
if cmp -s "$output_dir/collation-first/collation.rs" "$output_dir/collation-corrupt/collation.rs"; then
    echo 'collation mutation did not apply' >&2; exit 1
fi
rustc --edition=2024 -D warnings "$output_dir/collation-corrupt/verify.rs" -o "$output_dir/collation-corrupt/verify"
if "$output_dir/collation-corrupt/verify" "$output_dir/collation-first/reference.txt" > "$output_dir/collation-corruption.log" 2>&1; then
    echo 'incorrect collation value escaped comparison' >&2; exit 1
fi
echo 'PASS: collation full-domain expansion, repeatability and corruption detection'

# Tailoring weights come from the pinned offline transform, independently of
# Rust readers. Corrupt a phonebook weight and prove that comparison detects it.
sed 's/(0x41,&\[0x61,0x65\])/(0x41,\&[0x61,0x64])/' \
    "$output_dir/collation-first/collation.rs" > "$output_dir/collation-corrupt/collation.rs"
if cmp -s "$output_dir/collation-first/collation.rs" "$output_dir/collation-corrupt/collation.rs"; then
    echo 'tailoring mutation did not apply' >&2; exit 1
fi
rustc --edition=2024 -D warnings "$output_dir/collation-corrupt/verify.rs" -o "$output_dir/collation-corrupt/verify"
if "$output_dir/collation-corrupt/verify" "$output_dir/collation-first/reference.txt" > "$output_dir/tailoring-corruption.log" 2>&1; then
    echo 'incorrect tailoring value escaped comparison' >&2; exit 1
fi
cp "$source_dir/cutils.c" "$output_dir/drift/cutils.c"
cp "$source_dir/intl/collator.c" "$output_dir/drift/intl/collator.c"
echo '/* intentional tailoring drift */' >> "$output_dir/drift/intl/collator.c"
if perl "$script_dir/repack-collation.pl" "$output_dir/first" "$output_dir/drift" "$output_dir/tailoring-drift-result" \
    > "$output_dir/tailoring-drift.log" 2>&1; then
    echo 'unpinned tailoring input was accepted' >&2; exit 1
fi
test ! -e "$output_dir/tailoring-drift-result/export.c"
test "$(cat "$output_dir/tailoring-drift.log")" = 'unpinned intl/collator.c'
echo 'PASS: phonebook corruption detection and tailoring input pinning'

# List patterns live in the snapshot's separator selector. Export all agreed
# locales/types/styles and compare every byte through a separate Rust reader.
perl "$script_dir/repack-lists.pl" "$output_dir/first" "$source_dir" "$output_dir/lists-first"
perl "$script_dir/repack-lists.pl" "$output_dir/second" "$source_dir" "$output_dir/lists-second"
for file in lists.rs provenance.txt reference.txt; do
    cmp "$output_dir/lists-first/$file" "$output_dir/lists-second/$file"
done
rustc --edition=2024 -D warnings "$output_dir/lists-first/verify.rs" -o "$output_dir/lists-first/verify"
"$output_dir/lists-first/verify" "$output_dir/lists-first/reference.txt"
mkdir "$output_dir/lists-corrupt"
cp "$output_dir/lists-first/verify.rs" "$output_dir/lists-corrupt/"
sed 's/\\x61\\x6e\\x64/\\x61\\x6e\\x65/' "$output_dir/lists-first/lists.rs" > "$output_dir/lists-corrupt/lists.rs"
if cmp -s "$output_dir/lists-first/lists.rs" "$output_dir/lists-corrupt/lists.rs"; then
    echo 'list mutation did not apply' >&2; exit 1
fi
rustc --edition=2024 -D warnings "$output_dir/lists-corrupt/verify.rs" -o "$output_dir/lists-corrupt/verify"
if "$output_dir/lists-corrupt/verify" "$output_dir/lists-first/reference.txt" > "$output_dir/list-corruption.log" 2>&1; then
    echo 'incorrect list separator escaped comparison' >&2; exit 1
fi
cp "$source_dir/intl/listformat.c" "$output_dir/drift/intl/listformat.c"
echo '/* intentional list drift */' >> "$output_dir/drift/intl/listformat.c"
if perl "$script_dir/repack-lists.pl" "$output_dir/first" "$output_dir/drift" "$output_dir/lists-drift-result" > "$output_dir/list-drift.log" 2>&1; then
    echo 'unpinned list selector accepted' >&2; exit 1
fi
test ! -e "$output_dir/lists-drift-result/export.c"
echo 'PASS: list separators, repeatability, corruption detection and input pinning'

# Unit data passes through the same complete C/Rust wire comparison above.
# Verify the split representation, repeatability and a targeted corruption.
perl "$script_dir/repack-intl.pl" "$output_dir/first" "$output_dir/intl-first"
perl "$script_dir/repack-intl.pl" "$output_dir/second" "$output_dir/intl-second"
for file in "$output_dir/intl-first/"*; do
    cmp "$file" "$output_dir/intl-second/${file##*/}"
done
# Normalize Rust layout only; spaces inside byte strings are significant.
cmp <(sed -n '/^pub static nf_unit_patterns:/,$p' "$output_dir/intl-first/units.rs" | perl -0pe 's{(b?"(?:\\.|[^"\\])*")|\s+}{defined $1 ? $1 : ""}gse') \
    <(sed -n '/^pub static nf_unit_patterns:/,/^];/p' "$output_dir/first/tables.rs" | perl -0pe 's{(b?"(?:\\.|[^"\\])*")|\s+}{defined $1 ? $1 : ""}gse')
mkdir "$output_dir/unit-corrupt"
cp "$output_dir/first/verify.rs" "$output_dir/unit-corrupt/"
sed 's/{number} mins/{number} minx/' "$output_dir/first/tables.rs" > "$output_dir/unit-corrupt/tables.rs"
if cmp -s "$output_dir/first/tables.rs" "$output_dir/unit-corrupt/tables.rs"; then
    echo 'unit mutation did not apply' >&2; exit 1
fi
rustc --edition=2021 "$output_dir/unit-corrupt/verify.rs" -o "$output_dir/unit-corrupt/verify"
"$output_dir/unit-corrupt/verify" > "$output_dir/unit-corrupt/rust-values.bin"
if cmp -s "$output_dir/first/c-values.bin" "$output_dir/unit-corrupt/rust-values.bin"; then
    echo 'incorrect unit pattern escaped comparison' >&2; exit 1
fi
echo 'PASS: Intl repack repeatability, unit preservation and corruption detection'
