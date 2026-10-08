"""Generate a deterministic 32 MiB UTF-8 file within the lexical subset.

Run before benchmarking: python3 benchmarks/generate_large_input.py
"""

from pathlib import Path

destination = Path("target/benchmark-inputs/large-irregular.cljs")
destination.parent.mkdir(parents=True, exist_ok=True)
chunks = [
    "x ", "alpha,beta ", "(+ 1 2)\n", "(()())()((()))", " \t,\n\r\v\f" * 8,
    "(café λ 你好 🦀 e\u0301)\n", "long-atom" * 128 + " ", "(a(b)c) ",
]
chunks = [chunk.encode("utf-8") for chunk in chunks]
state = 0x12345678
written = 0
with destination.open("wb") as output:
    while written < 32 * 1024 * 1024:
        # Explicit 32-bit LCG: reproducible without a PRNG-library dependency.
        state = (1664525 * state + 1013904223) & 0xFFFFFFFF
        chunk = chunks[(state >> 16) % len(chunks)]
        output.write(chunk)
        written += len(chunk)
print(f"{destination}: {written} bytes")
