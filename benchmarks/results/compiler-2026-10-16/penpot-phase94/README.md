# Phase94: shorter protocol-demand path, rejected

An isolated phase87 variant remembers the last native/type selector per
protocol, retains the existing selector inventory and invalidation epoch, and
removes one JavaScript forwarding call from method demand. Receiver constructor
lookups still occur on every call.

Main none takes **90.54 seconds / 908,197,888 peak RSS bytes**, versus phase87's
isolated 91.34 seconds / 893,534,208 bytes. The small time difference and higher
memory do not justify retaining it. Output differs from phase42 only in the
edited `setDeclarationDemand` runtime unit; the complete application body is
identical, including the known main ESM defect.

One focused regression passes, covering repeated and alternating type demands,
new registration after a negative result, frontier restoration and observable
constructor getters. No full suite was run. The test was added after the timing
binary was built and is excluded from release code. Scripts, patch, logs,
exact command and hashes are retained; no change is promoted.
