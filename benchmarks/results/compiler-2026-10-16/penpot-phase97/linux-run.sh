#!/usr/bin/env bash
set -uo pipefail
run=$1
cd "$run/i" || exit $?
exec >"$run/o/probe.log" 2>&1
trap 'rc=$?; printf "%s\n" "$rc" >"$run/o/exit-code.txt"' EXIT
uname -m
g++ --version
g++ -O2 -std=c++20 -Wall -Wextra -Werror -DV8_COMPRESS_POINTERS -Iinclude -ffunction-sections -fdata-sections -c v8_stack.cc -o shim.o || exit $?
g++ -O2 -std=c++20 -Wall -Wextra -Werror -pthread stack-probe.cc v8_thread_stack.cc -o stack-probe || exit $?
./stack-probe
