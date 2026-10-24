#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 2 ]]; then
    echo "usage: bash $0 QUICKJS_SOURCE NEW_OUTPUT_DIRECTORY" >&2
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
