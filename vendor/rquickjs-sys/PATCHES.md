# Vendored rquickjs-sys patches

This directory holds the `rquickjs-sys` crate with local engine fixes on top.
`Cargo.toml` selects it with:

```toml
[patch.crates-io]
rquickjs-sys = { path = "vendor/rquickjs-sys" }
```

## Versions

- Crate: `rquickjs-sys` 0.14.0 (from crates.io).
- Engine: quickjs-ng 0.16.2, the `quickjs/` sources bundled by that crate.
- Host crate: `light-cljs`, which pins `rquickjs = "=0.14.0"`.

Engine changes are in `quickjs/quickjs.c`; `build.rs` also tracks their inputs.
The wasm-only sources, the
`Makefile` and the crate's `Cargo.lock` were removed because this project builds
for native targets only.

## Fix: interrupted `Array.prototype.find*` double frees its element

File: `quickjs/quickjs.c`, function `js_array_find` (which implements
`Array.prototype.find`, `findIndex`, `findLast` and `findLastIndex`).

```diff
--- a/quickjs/quickjs.c
+++ b/quickjs/quickjs.c
@@ -43990,6 +43990,12 @@ static JSValue js_array_find(JSContext *ctx, JSValueConst this_val,
         }
         JS_FreeValue(ctx, val);
         JS_FreeValue(ctx, index_val);
+        /* The interrupt check at the top of the next iteration jumps to
+           'exception', which frees val and index_val again. Reset them so an
+           interrupted find()/findIndex() never releases the previous
+           element's still live object a second time. */
+        val = JS_UNDEFINED;
+        index_val = JS_UNDEFINED;
     }
     JS_FreeValue(ctx, obj);
     if (mode == ArrayFindIndex || mode == ArrayFindLastIndex)
```

`js_array_find` releases the current element and index at the end of every
iteration but leaves the locals pointing at the released values. The
interrupt check at the top of the next iteration jumps to `exception`, which
frees `index_val` and `val` again:

- `index_val` is an integer, so the second release is harmless.
- `val` is the element returned by `JS_GetPropertyValue`, already released once
  at the bottom of the previous iteration. Releasing it again drops the
  reference that the source array still holds, so the element is freed even
  though the array still stores it. When that array is finalized the element is
  released once more, which is the corrupting free.

That second free corrupts the object list. The observed failure is a SIGSEGV in
`list_del` (`prev->next = next` with `prev == NULL`, so a write to `0x8`):

```
list_del
js_free_value_rt
JS_FreeValueRT
js_array_finalizer
free_object
free_gc_object
free_zero_refcount
js_free_value_rt
JS_FreeValue
JS_CallInternal
```

The interpreter calls `JS_FreeValue` on the corrupted array while unwinding the
interrupt out of the JavaScript caller. In light-cljs this is hit from the
compiler form bridge: `__lightEvaluator.importForm` -> `decode` -> `decodeNode`
-> `mapFromArray` -> `putEntry` -> `entryIndex` -> `findIndex`. Any host that
interrupts JavaScript inside an array `find*` loop can hit it. Upstream
quickjs-ng still has this shape on `master` (checked against quickjs-ng 0.17.0),
so an upstream fix is not yet available.

`js_array_every` and `js_array_reduce` already reset their locals after
freeing; this change makes `js_array_find` follow the same pattern.

## Fix: distribute native Map and WeakMap keys across buckets

File: `quickjs/quickjs.c`, function `map_hash_key`.

The original key hash multiplies pointers or the XOR of a double's two words
by an odd constant. The table selects a bucket using a power-of-two mask.
Multiplication preserves low zero bits: aligned object addresses and the
IEEE-754 encodings of small integers consequently use only a fraction of the
buckets. Increasing the table does not adequately distribute those keys.
Compiler value handles, provenance, runtime brands, and collection storage
exercise these native maps throughout macro expansion.

The patch avalanches the existing key/tag/seed hash with XOR shifts and two
odd multipliers before bucket selection. It changes neither key normalization
and SameValueZero comparison nor the separate insertion-order list. The
language runtime's ClojureScript hash values and collection order are unchanged.
`engine_maps_preserve_key_semantics_across_growth_and_deletion` checks native
Map/Set/WeakMap growth, deletion, iteration, object/symbol identity, numeric
equality, NaN, signed zero, and distinct BigInt/string/number keys.

## Fix: rebuild the engine after source or header edits

The build script copies engine inputs into `OUT_DIR` before invoking the C
compiler. It emitted environment dependencies but no dependencies on the
original C files and headers, so editing a vendored source could leave the
previous engine library in use. Each copied source/header and `quickjs.bind.h`
now emits `cargo:rerun-if-changed` for its original path.

## Removing the patches

Remove each change once a released `rquickjs-sys` bundles its equivalent:
resetting `val` and `index_val` in `js_array_find`, and distributing native map
hashes before their low bits select a bucket. Remove the `[patch.crates-io]`
entry and this directory only when both engine fixes are present upstream.
Keep input tracking while local engine sources are maintained.
`src/evaluator.rs::bridge_interrupt_tests` is the regression test for the fix;
it is expected to crash on an unpatched engine.

## WebAssembly support files

`wasm-shim/shim.c` and `vendor/wasi-libc/` are the crate's own files,
unmodified: `build.rs` needs them for the `wasm32-unknown-unknown` target,
which the browser playground (`playground/`) builds. This copy first left them
out as native-only; they were restored from the crate as published.
