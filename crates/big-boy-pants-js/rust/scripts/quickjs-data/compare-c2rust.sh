#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 2 ]]; then
    echo "usage: bash $0 QUICKJS_SOURCE OUTPUT_DIRECTORY" >&2
    exit 2
fi
source_dir=$(realpath "$1")
output_dir=$(realpath -m "$2")
case "$output_dir/" in
    "$source_dir/"*) echo 'output must be outside the QuickJS source' >&2; exit 2 ;;
esac
mkdir -- "$2"
output_dir=$(realpath "$2")
# Work on copies: the transpiler can create files alongside its inputs.
mkdir -p "$output_dir/input"
tar -C "$source_dir" --exclude=.git -cf - . | tar -C "$output_dir/input" -xf -
cat > "$output_dir/input/unicode_tables.c" <<'EOF'
#include "cutils.h"
#define CONFIG_ALL_UNICODE
#include "libunicode-table.h"
EOF
# C2Rust drops unused static header tables. Export roots to retain them.
perl -0777 -ne '
    print "const void *unicode_export_roots[] = {\n";
    while (/^static const [^\n=]+?\b(\w+)\s*\[[^\]]*\]\s*=/mg) {
        print "    $1,\n";
    }
    print "};\n";
' "$source_dir/libunicode-table.h" >> "$output_dir/input/unicode_tables.c"
c2rust --version > "$output_dir/version.txt"
c2rust transpile --fail-on-error --output-dir "$output_dir/rust" \
    "$output_dir/input/intl/locale-data.c" \
    "$output_dir/input/temporal/tzdata_generated.c" \
    "$output_dir/input/unicode_tables.c" \
    > "$output_dir/stdout.log" 2> "$output_dir/stderr.log"
echo "C2Rust output: $output_dir/rust"
