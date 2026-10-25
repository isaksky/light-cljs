#!/usr/bin/env bash
# Regenerate identifiers and folding together from the pinned QuickJS import
# and independently pinned Unicode 17.0.0 inputs. Never use the host Perl UCD.
# See scripts/quickjs-data/README.md for input acquisition and verification.
set -euo pipefail
if [[ $# != 4 ]]; then
    echo "usage: $0 VERIFIED_IMPORT QUICKJS_SOURCE UCD_17_DIRECTORY NEW_OUTPUT" >&2
    exit 2
fi
perl scripts/quickjs-data/repack-unicode.pl "$@"
cp "$4/unicode_ident.rs" src/frontend/unicode_ident.rs
cp "$4/case_fold_data.rs" src/regexp/case_fold_data.rs
cp "$4/property_data.rs" src/regexp/property_data.rs
cp "$4/provenance.txt" src/regexp/case_fold_provenance.txt
cp "$4/string_data.rs" src/value/string_data.rs
cp "$4/provenance.txt" src/value/string_provenance.txt
