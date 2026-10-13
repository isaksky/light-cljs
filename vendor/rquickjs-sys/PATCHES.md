# Vendored rquickjs-sys patches

This directory holds an unmodified copy of the `rquickjs-sys` crate with a
single engine fix on top. `Cargo.toml` selects it with:

```toml
[patch.crates-io]
rquickjs-sys = { path = "vendor/rquickjs-sys" }
```

## Versions

- Crate: `rquickjs-sys` 0.14.0 (from crates.io).
- Engine: quickjs-ng 0.16.2, the `quickjs/` sources bundled by that crate.
- Host crate: `light-cljs`, which pins `rquickjs = "=0.14.0"`.

Only `quickjs/quickjs.c` differs from the crate. The wasm-only sources, the
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

## Removing the patch

Remove the `[patch.crates-io]` entry and this directory once a released
`rquickjs-sys` bundles a quickjs-ng that resets `val` and `index_val` in
`js_array_find`. A release that includes the upstream fix makes
`vendor/rquickjs-sys/quickjs/quickjs.c` identical to the crate's copy again.
`src/evaluator.rs::bridge_interrupt_tests` is the regression test for the fix;
it is expected to crash on an unpatched engine.

## WebAssembly support files

`wasm-shim/shim.c` and `vendor/wasi-libc/` are the crate's own files,
unmodified: `build.rs` needs them for the `wasm32-unknown-unknown` target,
which the browser playground (`playground/`) builds. This copy first left them
out as native-only; they were restored from the crate as published.
