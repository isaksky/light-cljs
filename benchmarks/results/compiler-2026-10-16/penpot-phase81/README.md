# Phase81: retained session ownership categories

Diagnostic version of phase79. Session teardown clears each ownership category
in sequence, after analysis is complete. Counters are requested Rust allocation
bytes, excluding V8/C++ and allocator overhead; they are not RSS.

The main session has **13,367 analyzed units and no unfinished or failed units**.
Their original forms, definition snapshots and macro hosts have already been
released. Replacing the empty hosts with new defaults does not change live bytes
(it adds diagnostic allocation churn).

| Checkpoint | Live requested bytes |
| --- | ---: |
| start of session teardown | 646899665 |
| after clearing hosts, forms and snapshots | 646899665 |
| after clearing analyzed programs | 333161612 |
| after clearing catalog entries | 280682990 |
| after releasing bootstrap | 199392992 |
| after complete host release | 196537702 |

Analyzed programs retain **313.7 MB** beyond the final application's ownership.
Catalog entries account for another **52.5 MB**; bootstrap release frees about
81.3 MB. Shared ownership means these are releases in the stated order, not
independent object-size estimates.

The diagnostic run takes 20.75 s / 1159413760 peak RSS bytes. Complete output
matches phase42, including the known main ESM defect. No full suite is claimed
for this instrumentation. Production remains QuickJS phase72. Patch is relative
to phase79; scripts, counters, command and source/output hashes are preserved.
