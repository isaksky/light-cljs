#!/usr/bin/env bash
set -uo pipefail
run=$1
cd "$run/i" || exit $?
exec >"$run/o/probe.log" 2>&1
uname -m
gcc --version
tar -xzf mimalloc-v3.tar.gz || exit $?
gcc -O2 -DMI_DEBUG=0 -DMI_BUILD_RELEASE -ftls-model=initial-exec -Iinclude -Isrc -c src/static.c -o mimalloc.o && gcc -O2 -Wall -Wextra -Werror -Iinclude allocator-probe.c mimalloc.o -pthread -ldl -o allocator-probe && ./allocator-probe
rc=$?
printf '%s\n' "$rc" >"$run/o/exit-code.txt"
exit "$rc"
