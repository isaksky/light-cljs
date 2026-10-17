# Phase84: share historical namespace tables

Isolated phase83 variant. Program namespace names use the existing frozen arena;
namespace entry maps use the existing layered map. Definition snapshots retain
the same names, IDs and entries while sharing unchanged storage. Dev-hook lookup
accepts either a slice or the arena through iteration, preserving order.

Main none takes **20.46 s / 1059471360 peak RSS bytes**, versus phase83's
20.21 s / 1136787456 bytes. The memory improvement is useful; there is no timing
improvement. The library suite passes **124 tests, zero failures, one ignored**.
Complete output matches phase42, including the known main ESM defect. Full-suite
validation is still pending. Production remains QuickJS phase72.
