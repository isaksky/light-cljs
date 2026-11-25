//! Build-time inputs derived from source files:
//!
//! - The embedded runtime: `src/js_runtime.js` without its unit manifest header
//!   lines (lines whose first non-blank characters are `//@`; see
//!   `src/runtime_manifest.rs`). Output, the evaluator and the REPL embed the
//!   result, so the headers cost no emitted bytes.
//! - The terse runtime units (docs/advanced-optimization.md, Phase 3): each
//!   unit body without comments or whitespace and with its locals renamed to
//!   short names, which `Advanced` output embeds (see `src/terse.rs`). The
//!   lexer and scope analysis are `src/js_syntax.rs`, shared with the crate.
//! - The core operation index: `CoreFnId::ALL` and an exhaustive `match` from
//!   each `CoreFnId` to its row in `src/core_operations.tsv` (see
//!   `src/core_operations.rs`). A variant without a row fails to compile.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    env,
    fmt::Write as _,
    fs,
    path::Path,
};

// The module allows the parts the crate uses and the build does not.
#[path = "src/js_syntax.rs"]
mod js_syntax;
#[allow(dead_code)]
#[path = "src/sha256.rs"]
mod sha256;

use js_syntax::{Join, Kind, Shape, TerseWriter};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(light_v8)");
    println!(
        "cargo::rustc-env=LIGHT_CLJS_COMPILER_IDENTITY={}",
        sha256::hex(&compiler_identity())
    );
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    // The compiler runs on the main thread, which Windows gives 1 MiB by
    // default; reserve the 8 MiB macOS and Linux give it.
    if os == "windows" && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bins=/STACK:8388608");
    }
    if env::var_os("CARGO_FEATURE_V8_MACROS").is_some()
        && matches!(arch.as_str(), "aarch64" | "x86_64")
        && matches!(os.as_str(), "macos" | "linux" | "windows")
    {
        println!("cargo::rustc-cfg=light_v8");
        println!("cargo::rerun-if-changed=src/v8_stack.cc");
        println!("cargo::rerun-if-changed=src/v8_thread_stack.cc");
        println!("cargo::rerun-if-changed=src/v8_buffers.cc");
        println!("cargo::rerun-if-changed=vendor/v8-headers/include");
        let mut shims = cc::Build::new();
        shims
            .cpp(true)
            .std("c++20")
            .include("vendor/v8-headers/include");
        // MSVC reports `__cplusplus` as 199711L without this flag, whatever
        // `-std:c++20` says, and v8config.h requires C++20 by that macro.
        if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            shims.flag("/Zc:__cplusplus");
        }
        // Match the engine Cargo.toml selects: the prebuilt pointer-compressed
        // engines exist only for macOS and x86_64 Linux.
        if os == "macos" || (os == "linux" && arch == "x86_64") {
            shims.define("V8_COMPRESS_POINTERS", None);
        }
        shims
            .file("src/v8_stack.cc")
            .file("src/v8_thread_stack.cc")
            .file("src/v8_buffers.cc")
            .compile("light_cljs_v8_stack");
    }
    let out = env::var_os("OUT_DIR").expect("OUT_DIR");
    let out = Path::new(&out);

    println!("cargo::rerun-if-changed=src/js_runtime.js");
    println!("cargo::rerun-if-changed=src/js_syntax.rs");
    let source = fs::read_to_string("src/js_runtime.js").expect("read src/js_runtime.js");
    let runtime = source
        .split_inclusive('\n')
        .filter(|line| !line.trim_start().starts_with("//@"))
        .collect::<String>();
    fs::write(
        out.join("js_compiler_runtime.js"),
        compiler_runtime(&runtime),
    )
    .expect("write the compile-time runtime");
    fs::write(out.join("js_runtime.js"), runtime).expect("write the embedded runtime");
    fs::write(out.join("js_dev_runtime.js"), dev_runtime(&source)).expect("write the dev runtime");
    fs::write(out.join("terse_runtime.rs"), terse_runtime(&source))
        .expect("write the terse runtime");

    println!("cargo::rerun-if-changed=src/core_operations.tsv");
    let table =
        fs::read_to_string("src/core_operations.tsv").expect("read src/core_operations.tsv");
    fs::write(out.join("core_operations.rs"), core_operation_index(&table))
        .expect("write the core operation index");
}

/// The trees and files whose every byte the compiler identity covers: the
/// crate's sources (the embedded runtime, bootstrap, core macros and
/// bundled first-party libraries are under `src/`), this script, the
/// manifest and lock file (which pins the prebuilt V8 crate), and the
/// vendored V8 headers.
const IDENTITY_INPUTS: [&str; 5] = [
    "src",
    "build.rs",
    "Cargo.toml",
    "Cargo.lock",
    "vendor/v8-headers",
];

/// The SHA-256 identity of the compiler being built, which every cache key
/// includes (docs/compiler-cache.md, "Conservative keys and invalidation"):
/// each file of [`IDENTITY_INPUTS`] by path and content, in path order,
/// then the enabled features, the profile and optimization level, the
/// target and the `rustc` version. Any change to a source file, even a
/// comment, gives another compiler.
fn compiler_identity() -> [u8; 32] {
    let mut files = Vec::new();
    for input in IDENTITY_INPUTS {
        println!("cargo::rerun-if-changed={input}");
        collect_files(Path::new(input), &mut files);
    }
    files.sort();
    let mut fingerprint = sha256::Fingerprint::new("light-cljs compiler identity");
    fingerprint.count(files.len());
    for file in &files {
        let content = fs::read(file).unwrap_or_else(|error| panic!("read {file}: {error}"));
        fingerprint.text(file).bytes(&content);
    }
    let mut features: Vec<_> = env::vars()
        .filter(|(name, _)| name.starts_with("CARGO_FEATURE_"))
        .map(|(name, _)| name)
        .collect();
    features.sort();
    fingerprint.count(features.len());
    for feature in &features {
        fingerprint.text(feature);
    }
    for name in [
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "TARGET",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        fingerprint.optional_text(env::var(name).ok().as_deref());
    }
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let version = std::process::Command::new(&rustc)
        .arg("-vV")
        .output()
        .unwrap_or_else(|error| panic!("run {rustc} -vV: {error}"));
    fingerprint.bytes(&version.stdout);
    fingerprint.finish()
}

/// Every file under `path`, `/`-separated, or `path` itself when a file.
fn collect_files(path: &Path, files: &mut Vec<String>) {
    let metadata = fs::metadata(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    if !metadata.is_dir() {
        let text = path.to_str().expect("UTF-8 source paths");
        files.push(text.replace('\\', "/"));
        return;
    }
    let entries = fs::read_dir(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    for entry in entries {
        collect_files(&entry.expect("a directory entry").path(), files);
    }
}

/// The runtime the compile-time evaluators run: the embedded runtime with
/// hooks only the compiler uses, so that output, which embeds the runtime,
/// carries none of their bytes. Each hook goes into an anchor that occurs
/// once in the runtime, before the anchor's text given with it, or at its
/// end when that is empty; each is whole lines.
///
/// - `gensym` logs what it returns once the form bridge's `takeGensyms` has
///   been called, and `takeGensyms` hands over the counter and the log and
///   clears the log, so that a namespace artifact records the spellings its
///   analysis generated and where the counter stood; `raiseGensyms` moves
///   the counter past an installed artifact's (`src/generated_names.rs`).
/// - `gensymScope` makes `gensym` number in the compiler's scope of
///   generated names that the macro being expanded runs in: its prefix and a
///   count of the scope's own, so that two runtimes expanding one unit give
///   the same names and two units never give the same one. A native scope
///   (the compile-time runtime's) takes each number from the compiler's
///   count of the scope through `__lightGensym`, which core macros expanded
///   natively count from too (light-cljs-432); another (a data reader's)
///   counts in the runtime from zero the first time the runtime sees it, as
///   does a native one the compiler no longer knows. In a scope it skips no
///   `generatedNames`, so its names do not depend on what the heap decoded
///   before; it still logs them. Scope 0 is none, where `gensym` uses its
///   counter (`src/generated_names.rs`, `Scope`). Once `takeGensyms` has
///   been called in a runtime with `__lightGensym`, it logs each name there,
///   so that the compiler's log keeps them in order with the names native
///   expansions allocate; a name with a surrogate goes as its code units.
/// - `createNs` logs the name of each namespace it adds to the registry,
///   after `c`, once the form bridge's `takeNamespaces` has been called, and
///   `registerReaderNamespace` those a registration adds, after `r`;
///   `takeNamespaces` hands over the log and clears it, so that a namespace
///   artifact records the namespaces its analysis made `all-ns` and
///   `find-ns` see; `createNamespaces` adds an installed artifact's, unlogged
///   (`src/compilation_session.rs`, `take_namespaces`; light-cljs-1004).
///   `takeNamespaces(true)` starts the log of an analysis a unit pool may
///   place on a worker; a namespace it adds is marked `pooled`, and every
///   later touch of a pooled one (`createNs` or a registration) is logged
///   too, once per log, until an analysis the coordinator always runs
///   touches it, which clears the mark: with worker engines that touch adds
///   it to the coordinator's registry, so the artifact records it at every
///   `--jobs`, and the compiler keeps what each placement logged first
///   (light-cljs-1006). `createNamespaces` takes each name after `p`
///   (pooled) or `c`.
/// - `encode` records what each encoding charged its node limit when it
///   ends, and `takeEncodePeak` hands over the most that any encoding which
///   ended since the last call charged and starts again, so that a
///   capturing compilation can record the node budget an expansion's
///   conversions needed (`src/macro_host.rs`, `WorkMarks`). An encoding
///   can run another one, such as a macro expansion that a lazy sequence's
///   realization demands; the active ones are a stack, so each records its
///   own cost, and a nested `takeEncodePeak` leaves the outer ones running.
/// - `withMacroReaderContext` builds the aliases, refers, excludes and
///   bindings of the reader context it gives a source macro when the reader
///   first reads them, from the expansion's immutable environment, instead
///   of at every expansion: only a read in the code profile consults them
///   (light-cljs-427). It registers the view's declarations at once, as
///   before, since later views build on them.
/// - `extendType` and `extendInstance` note the first extension that
///   environment data could reach, one of a core runtime protocol on a
///   type other than a source `deftype` or `defrecord`, or on any instance,
///   whose properties it does not read (a getter could run), and tell the compiler once through
///   `__lightBuildService`, which then keeps giving source
///   macros their whole environment (light-cljs-433,
///   `crate::evaluator::EnvironmentExtensions`).
/// - `interpreterDispatch` makes the protocol lookups that every generic
///   operation makes on every call cheaper for an interpreter, which pays
///   for each call and lookup that V8 inlines (light-cljs-439). Each answers
///   as before and reads the value, runs a demand and tests a store in the
///   same order. `setDeclarationDemand` takes the registry's `extensions`
///   and demands only a protocol some bundle extends, since no other can
///   force one (compiler_roots.js); a resolved key (`methodKey`) keeps the
///   protocol its method demands; and `keyMethod`, `builtin`,
///   `externalMethod` and `extendedMethod` run with the calls between them
///   (`demandMethod`, `keySlot`, `nativeType`, `extendedDeclared` and
///   `externalMethod` where it is still the hook) inlined. A
///   `storesPredicate` tests only its class's store (light-cljs-1046).
fn compiler_runtime(runtime: &str) -> String {
    compiler_hooks(runtime)
}

/// The dev-build runtime (`src/dev_build.rs::runtime_file`): `src/js_runtime.js`
/// with the `:advanced`-only `hostExt` disablers (`hostExt.core`,
/// `hostExtEscaped.on`, `hostExtSeqEscaped.on`, `hostExtPrintEscaped.on`,
/// `slotWrites.hostExt`) omitted and replaced by an exported `markHostExt(flags)`
/// hook invoked by dev units that escape a core protocol/dispatcher as a
/// first-class value or directly write a core protocol method slot by name.
/// All 20 cold `slotWrites.<op>` fallback wrappers remain active at boot so
/// both static and computed/reflected core-slot clears preserve base semantics.
fn dev_runtime(source: &str) -> String {
    const SKIP_UNITS: &[&str] = &[
        "hostExt.core",
        "hostExtEscaped.on",
        "hostExtSeqEscaped.on",
        "hostExtPrintEscaped.on",
        "slotWrites.hostExt",
    ];
    const FAST_IMPLEMENTS_PROTOCOL: &str = concat!(
        "  function implementsProtocol(protocol, value) {\n",
        "    if (protocol == null) return false;\n",
        "    const ci = protocol.coreInfo$LKEY;\n",
        "    if (ci !== undefined) {\n",
        "      const mask = ci.mask;\n",
        "      if (value != null) {\n",
        "        if (mask && (value[mask[0]] & mask[1])) return true;\n",
        "        if (value[ci.marker] === protocolSentinel) return true;\n",
        "        if (ci.storeNative && mask && value.cljs$lang$kind$$LKEY < 64 && value[mask[0]]) return false;\n",
        "      }\n",
        "      return !!ci.native?.(value);\n",
        "    }\n",
        "    let slot = protocol.slot$LKEY;\n",
        "    if (slot === undefined) {\n",
        "      const key = protocolNames.get(protocol);\n",
        "      if (key) slot = protocol.slot$LKEY = protocolSlot(key, 0);\n",
        "      else return coreProtocolImplementation(protocol, value);\n",
        "    }\n",
        "    return value != null && value[slot] === protocolSentinel;\n",
        "  }\n",
    );
    const FAST_SATISFIES: &str = concat!(
        "  function satisfies(protocol, value) {\n",
        "    if (protocol == null) return false;\n",
        "    const ci = protocol.coreInfo$LKEY;\n",
        "    if (ci !== undefined) {\n",
        "      const mask = ci.mask;\n",
        "      if (value != null) {\n",
        "        if (mask && (value[mask[0]] & mask[1])) return true;\n",
        "        if (value[ci.marker] === protocolSentinel) return true;\n",
        "        if (ci.storeNative && mask && value.cljs$lang$kind$$LKEY < 64 && value[mask[0]]) return false;\n",
        "      }\n",
        "      if (ci.native?.(value)) return true;\n",
        "      if (ownedProtocolValue(value)) return false;\n",
        "      if (mask && value?.[mask[0]]) return false;\n",
        "      const type = nativeType(value);\n",
        "      if (protocol[type] || protocol._) return true;\n",
        "      const natives = nativeMethods.get(ci.name);\n",
        "      return natives !== undefined && !!(natives.get(type) || natives.get(\"_\"));\n",
        "    }\n",
        "    let slot = protocol.slot$LKEY;\n",
        "    if (slot === undefined) {\n",
        "      const key = protocolNames.get(protocol);\n",
        "      if (key) slot = protocol.slot$LKEY = protocolSlot(key, 0);\n",
        "      else {\n",
        "        if (coreProtocolImplementation(protocol, value)) return true;\n",
        "        if (ownedProtocolValue(value)) return false;\n",
        "        const coreName = coreProtocolNames.get(protocol);\n",
        "        const mask = protocolMasks.get(coreName);\n",
        "        if (mask && value?.[mask[0]]) return false;\n",
        "        return coreName ? !!(protocol[nativeType(value)] || protocol._ || nativeMethods.get(coreName)?.get(nativeType(value)) || nativeMethods.get(coreName)?.get(\"_\")) : false;\n",
        "      }\n",
        "    }\n",
        "    if (value != null && value[slot] === protocolSentinel) return true;\n",
        "    return !!(protocol[nativeType(value)] || protocol._);\n",
        "  }\n",
    );
    const DEV_PRINT_SEQUENTIAL: &str = concat!(
        "  function printSequential(writer, printOne, begin, separator, end, options, coll, step = 1) {\n",
        "    const saved = coreCells.printLevel;\n",
        "    coreCells.printLevel = saved == null ? null : saved - 1;\n",
        "    try {\n",
        "      if (coreCells.printLevel != null && coreCells.printLevel < 0) return writeText(writer, \"#\");\n",
        "      writeText(writer, begin);\n",
        "      let items = Array.isArray(coll) && (!protocolHooks.programMethod || protocolHooks.noSeqExt());\n",
        "      let current = items ? (coll.length ? 0 : null) : printHooks.seq(coll), remaining = printOption(options, \"print-length\", \"printLength\"), printed = false;\n",
        "      while (current != null && (remaining == null || remaining !== 0)) {\n",
        "        if (printed) writeText(writer, separator);\n",
        "        if (step === 2) printOne.call(null, coll[current], writer, options, coll[current + 1]);\n",
        "        else printOne.call(null, items ? coll[current] : printHooks.first(current), writer, options);\n",
        "        printed = true;\n",
        "        if (items && protocolHooks.programMethod && !protocolHooks.noSeqExt()) {\n",
        "          items = false;\n",
        "          if (step === 2) {\n",
        "            const objs = [];\n",
        "            for (let j = 0; j < coll.length; j += 2) objs.push({key: coll[j], value: coll[j + 1]});\n",
        "            const orig = printOne;\n",
        "            printOne = (entry, target, opts) => orig.call(null, entry.key, target, opts, entry.value);\n",
        "            current = printHooks.next(IndexedSeqValue(objs, current >> 1, null));\n",
        "            step = 1;\n",
        "          } else {\n",
        "            current = printHooks.next(IndexedSeqValue(coll, current, null));\n",
        "          }\n",
        "        } else {\n",
        "          current = items ? (current + step < coll.length ? current + step : null) : printHooks.next(current);\n",
        "        }\n",
        "        if (remaining != null) remaining--;\n",
        "      }\n",
        "      if (current != null) {\n",
        "        if (printed) writeText(writer, separator);\n",
        "        const marker = options == null ? null : printHooks.option(options, \"more-marker\"); writeText(writer, truth(marker) ? marker : \"...\");\n",
        "      }\n",
        "      return writeText(writer, end);\n",
        "    } finally { coreCells.printLevel = saved; }\n",
        "  }\n",
    );
    const DEV_PRINT_MAP_ENTRIES: &str = concat!(
        "  function printMapEntries(entries, writer, options, prefix = \"\", printKey = printWriter, pairs = false) {\n",
        "    const ns = !prefix && kindHooks.namespaceOf?.(entries, pairs);\n",
        "    if (prefix) writeText(writer, prefix);\n",
        "    if (pairs && !ns && options == null && coreCells.printLevel == null && coreCells.printLength == null && (!protocolHooks.programMethod || protocolHooks.noSeqExt())) {\n",
        "      writeText(writer, \"{\");\n",
        "      for (let i = 0; i < entries.length; i += 2) {\n",
        "        if (i) writeText(writer, \", \");\n",
        "        printKey(entries[i], writer, null);\n",
        "        writeText(writer, \" \");\n",
        "        printWriterImpl(entries[i + 1], writer, null);\n",
        "        if (protocolHooks.programMethod && !protocolHooks.noSeqExt()) {\n",
        "          const objs = [];\n",
        "          for (let j = 0; j < entries.length; j += 2) objs.push({key: entries[j], value: entries[j + 1]});\n",
        "          let s = printHooks.next(IndexedSeqValue(objs, i >> 1, null));\n",
        "          while (s != null) {\n",
        "            writeText(writer, \", \");\n",
        "            const e = printHooks.first(s);\n",
        "            printKey(e.key, writer, null);\n",
        "            writeText(writer, \" \");\n",
        "            printWriter(e.value, writer, null);\n",
        "            s = printHooks.next(s);\n",
        "          }\n",
        "          break;\n",
        "        }\n",
        "      }\n",
        "      return writeText(writer, \"}\");\n",
        "    }\n",
        "    const begin = (ns ? \"#:\" + ns : \"\") + \"{\";\n",
        "    if (pairs) return printSequential(writer, (key, target, opts, value) => {\n",
        "      printKey(ns ? kindHooks.unqualified(key) : key, target, opts); writeText(target, \" \"); return printWriter(value, target, opts);\n",
        "    }, begin, \", \", \"}\", options, entries, 2);\n",
        "    return printSequential(writer, (entry, target, opts) => {\n",
        "      printKey(ns ? kindHooks.unqualified(entry.key) : entry.key, target, opts); writeText(target, \" \"); return printWriter(entry.value, target, opts);\n",
        "    }, begin, \", \", \"}\", options, entries);\n",
        "  }\n",
    );
    const DEV_PRINT_VALUES: &str = concat!(
        "  function printValues(values, options) {\n",
        "    if (protocolHooks.programMethod && !protocolHooks.noSeqExt()) return printHooks.seqText(values, options);\n",
        "    if (!values.length) return \"\";\n",
        "    const {buffer, writer} = stringWriter();\n",
        "    for (let i = 0; i < values.length; i++) {\n",
        "      if (i) write(writer, \" \");\n",
        "      printWriter(values[i], writer, options);\n",
        "      if (protocolHooks.programMethod && !protocolHooks.noSeqExt()) {\n",
        "        let s = printHooks.next(IndexedSeqValue(values, i, null));\n",
        "        while (s != null) {\n",
        "          write(writer, \" \");\n",
        "          printWriter(printHooks.first(s), writer, options);\n",
        "          s = printHooks.next(s);\n",
        "        }\n",
        "        break;\n",
        "      }\n",
        "    }\n",
        "    flushWriter(writer); return buffer.text;\n",
        "  }\n",
    );
    const DEV_INIT: &str = concat!(
        "  function markHostExt(flags) {\n",
        "    if (flags & 1) { hostExt.p0 = hostExt.p1 = -1; hostExt.fn = hostExt.seq = true; hostExt.imeta.on = true; }\n",
        "    if (flags & 2) hostExt.seq = true;\n",
        "    if (flags & 4) hostExt.p0 = hostExt.p0 | -2147483648;\n",
        "  }\n",
        "  for (const [protocol, name] of coreProtocolNames) {\n",
        "    const mask = protocolMasks.get(name);\n",
        "    Object.defineProperty(protocol, \"coreInfo$LKEY\", {\n",
        "      value: {\n",
        "        name, mask, marker: protocolSlot(name, 0),\n",
        "        native: nativeImplementations.get(protocol),\n",
        "        storeNative: storeNativeProtocols.has(name)\n",
        "      }\n",
        "    });\n",
        "  }\n",
    );

    let mut out = String::with_capacity(source.len() + 4096);
    let mut current_unit: Option<&str> = None;
    let mut skipped_count = 0usize;
    let mut replaced_implements = false;
    let mut replaced_satisfies = false;
    let mut replaced_print_seq = false;
    let mut replaced_print_map = false;
    let mut replaced_print_values = false;
    let mut seen_exports = false;
    let mut exports_patched = false;

    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("//@unit ") {
            let name = rest.split_whitespace().next().unwrap_or("");
            current_unit = Some(name);
            if SKIP_UNITS.contains(&name) {
                skipped_count += 1;
            } else if name == "implementsProtocol" {
                out.push_str(FAST_IMPLEMENTS_PROTOCOL);
                replaced_implements = true;
            } else if name == "satisfies" {
                out.push_str(FAST_SATISFIES);
                replaced_satisfies = true;
            } else if name == "printSequential" {
                out.push_str(DEV_PRINT_SEQUENTIAL);
                replaced_print_seq = true;
            } else if name == "printMapEntries" {
                out.push_str(DEV_PRINT_MAP_ENTRIES);
                replaced_print_map = true;
            } else if name == "printValues" {
                out.push_str(DEV_PRINT_VALUES);
                replaced_print_values = true;
            }
            continue;
        }
        if trimmed.starts_with("//@exports") {
            current_unit = None;
            seen_exports = true;
            out.push_str(DEV_INIT);
            continue;
        }
        if trimmed.starts_with("//@") {
            continue;
        }
        if let Some(unit) = current_unit
            && (SKIP_UNITS.contains(&unit)
                || matches!(
                    unit,
                    "implementsProtocol"
                        | "satisfies"
                        | "printSequential"
                        | "printMapEntries"
                        | "printValues"
                ))
        {
            continue;
        }
        if seen_exports && !exports_patched && line.contains("return Object.freeze({") {
            out.push_str(&line.replacen(
                "return Object.freeze({",
                "return Object.freeze({markHostExt, ",
                1,
            ));
            exports_patched = true;
        } else {
            out.push_str(line);
        }
    }
    assert_eq!(
        skipped_count,
        SKIP_UNITS.len(),
        "dev_runtime expected all hostExt disabler units"
    );
    assert!(
        replaced_implements,
        "dev_runtime expected implementsProtocol unit"
    );
    assert!(replaced_satisfies, "dev_runtime expected satisfies unit");
    assert!(
        replaced_print_seq,
        "dev_runtime expected printSequential unit"
    );
    assert!(
        replaced_print_map,
        "dev_runtime expected printMapEntries unit"
    );
    assert!(
        replaced_print_values,
        "dev_runtime expected printValues unit"
    );
    assert!(exports_patched, "dev_runtime could not patch exports line");
    out
}

/// The compile-time hooks inserted into `runtime` (see [`compiler_runtime`]).
/// For the shared-heap profile (`shared`), a hook whose anchor the profile
/// has swapped away is skipped when the profile's unit carries the hook's
/// marker itself, the hooks' own per-task `let`s are dropped (their defaults
/// live on `sharedTaskDefaults`), and their reads go through the task's
/// cells like every other unit's ([`rewrite_task_state`]).
fn compiler_hooks(runtime: &str) -> String {
    const ENCODE_END: &str = "      return visit(value, \"$\", 0);\n    }\n    return Object.freeze({version: 5, decode, encode,";
    // Every line holds `readerViews`, which tells the hook's lines from the
    // embedded runtime's (tests/compiler/runtime_manifest.rs).
    const READER_CONTEXT: &str = concat!(
        "    { const previous = ns.readerContext, view = get(args[1], keyword(\"light-cljs\", \"reader-context\")); let readerViews = null;\n",
        "      let readerViewsDeclared = previous?.declared, readerViewsVisible = previous?.visible ?? 0; const added = view == null ? null : get(view, keyword(null, \"declared\"));\n",
        "      if (added != null) { const generation = get(added, keyword(null, \"generation\")); if (ns.readerDeclared?.generation !== generation) ns.readerDeclared = {generation, index: new Map()}; let position = get(added, keyword(null, \"from\")); for (const name of toArray(get(added, keyword(null, \"names\")))) ns.readerDeclared.index.set(name, position++); readerViewsDeclared = ns.readerDeclared.index; readerViewsVisible = get(view, keyword(null, \"visible\")); }\n",
        "      const readerViewsBuild = () => { if (readerViews !== null) return readerViews; const context = previous ?? {aliases: new Map(), refers: new Map(), excludes: new Set(), bindings: new Set()}; const aliases = new Map(), refers = new Map(), excludes = new Set(), bindings = new Set();\n",
        "        for (const pair of toArray(get(env, keyword(null, \"requires\")))) aliases.set(str(nth(pair, 0)), str(nth(pair, 1))); if (view != null) { for (const entry of toArray(get(view, keyword(null, \"refers\")))) refers.set(nth(entry, 0), [nth(entry, 1), nth(entry, 2)]); for (const name of toArray(get(view, keyword(null, \"excludes\")))) excludes.add(name); for (const name of toArray(get(view, keyword(null, \"bindings\")))) bindings.add(name); } return readerViews = {aliases: readerOverlay(context.aliases, aliases), refers: readerOverlay(context.refers, refers), excludes: readerOverlay(context.excludes, excludes), bindings: readerOverlay(context.bindings, bindings)}; };\n",
        "      ns.readerContext = {get aliases() { return readerViewsBuild().aliases; }, get refers() { return readerViewsBuild().refers; }, get excludes() { return readerViewsBuild().excludes; }, get bindings() { return readerViewsBuild().bindings; }, declared: readerViewsDeclared, visible: readerViewsVisible}; coreCells.currentNs = ns;\n",
        "      try { return Reflect.apply(fn, null, args); } finally { ns.readerContext = previous; coreCells.currentNs = saved; } } // readerViews\n",
    );
    // Every line holds `environmentExtensions`, as above.
    const ENVIRONMENT_EXTENSIONS: &str = concat!(
        "  function noteEnvironmentExtension(type, methods) { if (globalThis.__lightEnvironmentExtended) return; const name = typeof type === \"function\" ? typeNames.get(type) : undefined; // environmentExtensions\n",
        "    if (name?.includes(\"/\") && !name.startsWith(\"cljs.core/\")) return; // environmentExtensions\n",
        "    if (!methods.some(([dispatch, arity]) => { const key = typeof dispatch === \"string\" ? dispatch : protocolKeys.get(dispatch); if (typeof key !== \"string\") return true; const protocol = arity === 0 ? key : key.slice(0, key.lastIndexOf(\"/\")); return !protocol.includes(\"/\") || protocol.startsWith(\"cljs.core/\"); })) return; // environmentExtensions\n",
        "    globalThis.__lightEnvironmentExtended = true; globalThis.__lightBuildService?.(\"environment-extension\"); } // environmentExtensions\n",
    );
    const EXTEND_TYPE: &str = "  function extendType(target, methods) {\n    let result = null;\n";
    // Every line holds `interpreterDispatch`, as above.
    const DISPATCH_KEYS: &str = "  methodKey = (resolve => key => { const resolved = resolve(key); resolved.demand = key.slice(0, key.lastIndexOf(\"/\")); return resolved; })(methodKey); // interpreterDispatch\n";
    const DISPATCH_DEMAND: &str = concat!(
        "    const extended = arguments[1]; demandGate = demand && extended ? extended : null; demandHost = demand; // interpreterDispatch\n",
        "    if (demandGate !== null) demandProtocol = (key, value) => { if (extended.has(key)) demand(key, value, typeNames.get(value?.constructor)); }; // interpreterDispatch\n",
    );
    // `externalMethod`, inlined where the hook is still this runtime's: it
    // sets `method`.
    macro_rules! external_inline {
        ($indent:literal) => {
            concat!(
                $indent,
                "const hook = protocolHooks.externalMethod; if (hook !== externalMethod) method = hook?.(value, key, arity); else { // interpreterDispatch\n",
                $indent,
                "  const cell = key.cell ?? (key.cell = dispatchCell(key.key)), external = cell.dispatcher, type = value == null ? \"null\" : Array.isArray(value) ? \"array\" : typeof value; // interpreterDispatch\n",
                $indent,
                "  const dispatch = external?.[type] ?? external?._; // interpreterDispatch\n",
                $indent,
                "  if (!nativeMethods.size && !dispatch) method = undefined; else { const protocol = cell.protocol ?? (cell.protocol = coreProtocolsByName.get(key.name) ?? null); // interpreterDispatch\n",
                $indent,
                "    method = protocol && nativeImplementations.get(protocol)?.(value) ? undefined : dispatch ?? cell.natives?.get(type)?.get(arity) ?? cell.natives?.get(\"_\")?.get(arity); } } // interpreterDispatch\n",
            )
        };
    }
    const DISPATCH: &str = concat!(
        "  keyMethod = function (value, key, arity) { const protocol = arity === 0 ? key.key : key.demand; // interpreterDispatch\n",
        "    if (demandGate === null) demandProtocol(protocol, value); else if (demandGate.has(protocol)) demandHost(protocol, value, typeNames.get(value?.constructor)); // interpreterDispatch\n",
        "    return value == null ? undefined : value[key.slots[arity] ?? (key.slots[arity] = protocolSlot(key.key, arity))]; }; // interpreterDispatch\n",
        "  builtin = function (value, key, arity) { const protocol = arity === 0 ? key.key : key.demand; // interpreterDispatch\n",
        "    if (demandGate === null) demandProtocol(protocol, value); else if (demandGate.has(protocol)) demandHost(protocol, value, typeNames.get(value?.constructor)); // interpreterDispatch\n",
        "    let method = value == null ? undefined : value[key.slots[arity] ?? (key.slots[arity] = protocolSlot(key.key, arity))]; // interpreterDispatch\n",
        "    if (method != null) return method; // interpreterDispatch\n",
        external_inline!("    "),
        "    return method; }; // interpreterDispatch\n",
        "  externalMethod = function (value, key, arity) { // interpreterDispatch\n",
        "    const cell = key.cell ?? (key.cell = dispatchCell(key.key)), external = cell.dispatcher, type = value == null ? \"null\" : Array.isArray(value) ? \"array\" : typeof value; // interpreterDispatch\n",
        "    const dispatch = external?.[type] ?? external?._; if (!nativeMethods.size && !dispatch) return undefined; // interpreterDispatch\n",
        "    const protocol = cell.protocol ?? (cell.protocol = coreProtocolsByName.get(key.name) ?? null); if (protocol && nativeImplementations.get(protocol)?.(value)) return undefined; // interpreterDispatch\n",
        "    return dispatch ?? cell.natives?.get(type)?.get(arity) ?? cell.natives?.get(\"_\")?.get(arity); }; // interpreterDispatch\n",
        "  extendedMethod = function (value, key, arity, fast = false, directOnly = false) { // interpreterDispatch\n",
        "    const info = key.info || (key.info = protocolMethodInfo(key.key)), protocol = info.protocol, mask = info.mask, native = info.native, name = info.name; // interpreterDispatch\n",
        "    let direct = !!(mask && (value?.[mask[0]] & mask[1])); // interpreterDispatch\n",
        "    if (!direct && name) { // interpreterDispatch\n",
        "      if (demandGate === null) demandProtocol(name, value); else if (demandGate.has(name)) demandHost(name, value, typeNames.get(value?.constructor)); // interpreterDispatch\n",
        "      direct = value != null && value[info.marker] === protocolSentinel; } // interpreterDispatch\n",
        "    if (!direct) direct = !!native?.(value); // interpreterDispatch\n",
        "    if (!direct && (directOnly || (fast && (Array.isArray(value) || typeof value === \"string\")))) return undefined; // interpreterDispatch\n",
        "    if (!direct && !satisfies(protocol, value)) return undefined; // interpreterDispatch\n",
        "    const demanded = arity === 0 ? key.key : key.demand; // interpreterDispatch\n",
        "    if (demandGate === null) demandProtocol(demanded, value); else if (demandGate.has(demanded)) demandHost(demanded, value, typeNames.get(value?.constructor)); // interpreterDispatch\n",
        "    let method = value == null ? undefined : value[key.slots[arity] ?? (key.slots[arity] = protocolSlot(key.key, arity))]; // interpreterDispatch\n",
        "    if (method == null) { // interpreterDispatch\n",
        external_inline!("      "),
        "    } // interpreterDispatch\n",
        "    if (method) return method; // interpreterDispatch\n",
        "    const declared = key.info || (key.info = protocolMethodInfo(key.key)); // interpreterDispatch\n",
        "    if (declared.protocol === IMeta && typeof value === \"function\") return undefined; // interpreterDispatch\n",
        "    if (!declared.native?.(value)) throw new Error(\"No protocol method \" + key.key + \" defined for this value\"); // interpreterDispatch\n",
        "    return undefined; }; // interpreterDispatch\n",
    );
    const HOOKS: [(&str, &str, &str); 17] = [
        (
            "  const namespaces = new Map();\n",
            "",
            "  let namespaceLog = null; // namespaceLog\n",
        ),
        (
            "    if (!namespaces.has(key)) namespaces.set(key, new Namespace(obj, name));\n",
            "    if",
            "    if (namespaceLog !== null) { const known = namespaces.get(key); if (known === undefined) { namespaceLog.push(\"c\" + key); if (namespaceLog.pooled) { const ns = new Namespace(obj, name); ns.pooled = true; namespaces.set(key, ns); } } else if (known.pooled) { if (!namespaceLog.pooled) known.pooled = false; if (!namespaceLog.includes(\"c\" + key) && !namespaceLog.includes(\"r\" + key)) namespaceLog.push(\"c\" + key); } } // namespaceLog\n",
        ),
        (
            "    const ns = createNs(symbol(null, name));\n    if (added && ns.readerContext)",
            "    const ns",
            "    if (namespaceLog !== null) { const known = namespaces.get(name); if (known === undefined) { namespaceLog.push(\"r\" + name); const ns = new Namespace(null, symbol(null, name)); if (namespaceLog.pooled) ns.pooled = true; namespaces.set(name, ns); } else if (known.pooled) { if (!namespaceLog.pooled) known.pooled = false; if (!namespaceLog.includes(\"r\" + name)) namespaceLog.push(\"r\" + name); } } // namespaceLog\n",
        ),
        (
            "    const ns = createNs(name);\n    const context = ns.readerContext ?? {aliases: new Map(), refers: new Map(), excludes: new Set(), bindings: new Set()};\n",
            "    const context",
            READER_CONTEXT,
        ),
        (
            "  const generatedNames = new Set();\n",
            "",
            concat!(
                "  let gensymLog = null;\n  let gensymScope = null; const gensymScopes = new Map();\n",
                "  const gensymLogName = name => { if (gensymLog === true) globalThis.__lightGensym(/[\\ud800-\\udfff]/.test(name) ? \"U\" + Array.from({length: name.length}, (_, i) => name.charCodeAt(i)).join(\",\") : \"L\" + name); else gensymLog.push(name); };\n",
            ),
        ),
        (
            "    let name; do { name = base + ++gensymCounter; } while (generatedNames.has(name));\n",
            "    let name",
            "    if (gensymScope !== null) { const suffix = gensymScope.native ? globalThis.__lightGensym(\"g\" + gensymScope.serial) : \"\"; const name = base + (suffix || gensymScope.prefix + ++gensymScope.count); if (gensymLog !== null) gensymLogName(name); return symbolCore(name); }\n",
        ),
        (
            "    generatedNames.add(name);\n    return symbolCore(name);\n",
            "    return",
            "    if (gensymLog !== null) gensymLogName(name);\n",
        ),
        (
            "      check: () => { if (terminal != null) throw new Error(terminal); },\n",
            "",
            concat!(
                "      takeGensyms: () => { const names = Array.isArray(gensymLog) ? gensymLog : []; gensymLog = typeof globalThis.__lightGensym === \"function\" ? true : []; return vectorFromArray([gensymCounter, ...names]); }, raiseGensyms: floor => { if (floor > gensymCounter) gensymCounter = floor; },\n",
                "      takeEncodePeak: () => { const peak = encode.peak ?? 0; encode.peak = 0; return peak; },\n",
                "      takeNamespaces: pooled => { const names = namespaceLog ?? []; namespaceLog = []; if (pooled === true) namespaceLog.pooled = true; return vectorFromArray(names); }, // namespaceLog\n",
                "      createNamespaces: names => { const log = namespaceLog; namespaceLog = null; try { for (const entry of names) { const ns = createNs(symbol(null, entry.slice(1))); if (entry[0] === \"p\") ns.pooled = true; else if (ns.pooled) ns.pooled = false; } } finally { namespaceLog = log; } return null; }, // namespaceLog\n",
                "      gensymScope: (serial, prefix, retired, native) => { if (retired) for (const ended of retired.split(\",\")) gensymScopes.delete(+ended); if (serial === 0) { gensymScope = null; return null; } let scope = gensymScopes.get(serial); if (scope === undefined) gensymScopes.set(serial, scope = {serial, prefix, count: 0, native: native === true && typeof globalThis.__lightGensym === \"function\"}); gensymScope = scope; return null; }, gensymScopeCount: () => gensymScopes.size,\n",
            ),
        ),
        (
            ENCODE_END,
            "      return visit",
            "      (encode.active ??= []).push(() => limit - remaining); try {\n",
        ),
        (
            ENCODE_END,
            "    }\n    return Object.freeze",
            "      } finally { encode.peak = Math.max(encode.peak ?? 0, encode.active.pop()()); }\n",
        ),
        (EXTEND_TYPE, "  function extendType", ENVIRONMENT_EXTENSIONS),
        (
            EXTEND_TYPE,
            "    let result",
            "    noteEnvironmentExtension(target, methods); // environmentExtensions\n",
        ),
        (
            "  function extendInstance(target, methods, returnTarget) {\n",
            "",
            "    noteEnvironmentExtension(undefined, methods); // environmentExtensions\n",
        ),
        (
            "  function keySlot(key, arity) { return key.slots[arity] ?? (key.slots[arity] = protocolSlot(key.key, arity)); }\n",
            "  function keySlot",
            DISPATCH_KEYS,
        ),
        (
            "  function setDeclarationDemand(demand) {\n",
            "  function",
            "  let demandGate = null, demandHost = null; // interpreterDispatch\n",
        ),
        (
            "    demandMulti = mf => { if (demand) demand(\"multifn:\" + print(mf.name, false), mf); };\n",
            "",
            DISPATCH_DEMAND,
        ),
        (
            "  protocolHooks.builtin = builtin;\n",
            "  protocolHooks",
            DISPATCH,
        ),
    ];
    let mut runtime = runtime.to_owned();
    for (anchor, before, hook) in HOOKS {
        assert_eq!(
            runtime.matches(anchor).count(),
            1,
            "compile-time runtime hook anchor {anchor:?}"
        );
        let within = if before.is_empty() {
            anchor.len()
        } else {
            anchor.find(before).unwrap()
        };
        runtime.insert_str(runtime.find(anchor).unwrap() + within, hook);
    }
    const NS_REFLECT_HOOKS: [(&str, &str, &str); 5] = [
        (
            "  const namespaces = new Map();\n",
            "",
            "  let _regNsDepth = 0, _flushingNs = false; // namespaceLog\n",
        ),
        (
            "    if (!namespaces.has(key)) namespaces.set(key, new Namespace(obj, name));\n",
            "    if (!namespaces",
            "    if (!_regNsDepth && !_flushingNs && typeof globalThis.__lightRootService === \"function\") { const _add = globalThis.__lightRootService(\"c\\u0000\" + key); if (_add) { _flushingNs = true; try { for (const _n of JSON.parse(_add)) createNs(symbol(null, _n)); } finally { _flushingNs = false; } } } // namespaceLog\n",
        ),
        (
            "      excludes: new Set(context.excludes ?? []), bindings: new Set(context.bindings ?? [])};\n    return ns;\n  }\n",
            "",
            "  const _origRegNs = registerReaderNamespace; registerReaderNamespace = function registerReaderNamespace(name, context, added) { _regNsDepth++; try { return _origRegNs(name, context, added); } finally { _regNsDepth--; } }; // namespaceLog\n",
        ),
        (
            "    return namespaces.get(str(name)) ?? null;\n",
            "    return",
            "    if (!_flushingNs && typeof globalThis.__lightRootService === \"function\") { const _add = globalThis.__lightRootService(\"r\\u0000\" + str(name)); if (_add) { _flushingNs = true; try { for (const _n of JSON.parse(_add)) createNs(symbol(null, _n)); } finally { _flushingNs = false; } } } // namespaceLog\n",
        ),
        (
            "  function allNs() { if (arguments.length) return invalidArity(arguments.length); return listFromArray([...namespaces.values()]); }\n",
            "",
            "  const _origAllNs = allNs; allNs = function allNs() { if (!_flushingNs && typeof globalThis.__lightRootService === \"function\") { const _add = globalThis.__lightRootService(\"r\\u0000*\"); if (_add) { _flushingNs = true; try { for (const _n of JSON.parse(_add)) createNs(symbol(null, _n)); } finally { _flushingNs = false; } } } return _origAllNs.apply(this, arguments); }; // namespaceLog\n",
        ),
    ];
    for (anchor, before, hook) in NS_REFLECT_HOOKS {
        assert_eq!(
            runtime.matches(anchor).count(),
            1,
            "compile-time ns-reflect hook anchor {anchor:?}"
        );
        let within = if before.is_empty() {
            anchor.len()
        } else {
            anchor.find(before).unwrap()
        };
        runtime.insert_str(runtime.find(anchor).unwrap() + within, hook);
    }
    // Each runtime class constructor's brand marker becomes the compile-time
    // stamp (docs/value-layout.md, section 4), which output never has.
    assert!(
        runtime.contains(BRAND_MARKER),
        "no runtime class carries {BRAND_MARKER}"
    );
    // A marked vector test of a value that may be a host object (`inspect`)
    // tests that stamp first, which reads no property.
    assert!(
        runtime.contains(STAMPED_MARKER),
        "no kind test carries {STAMPED_MARKER}"
    );
    // `ExceptionInfo` records string messages in `exceptionMessages` only in
    // the compile-time runtime so application output pays no WeakMap write.
    assert!(
        runtime.contains(EXCEPTION_MESSAGE_MARKER),
        "no ExceptionInfo constructor carries {EXCEPTION_MESSAGE_MARKER}"
    );
    runtime
        .replace(BRAND_MARKER, BRAND_STAMP)
        .replace(STAMPED_MARKER, STAMPED_TEST)
        .replace(EXCEPTION_MESSAGE_MARKER, EXCEPTION_MESSAGE_RECORD)
}

/// The marker that ends each runtime class constructor, and what the
/// compile-time runtime has in its place ([`compiler_hooks`]).
const BRAND_MARKER: &str = "/*@brand*/";
const BRAND_STAMP: &str = "stamp(this);";
/// The marker of a kind test (a vector's or a view's) whose value may be a
/// host object at compile time, and the compile-time runtime's text in its
/// place: the stamp first, so that the keyed kind is read only from the
/// runtime's own values.
const STAMPED_MARKER: &str = "/*@stamped*/";
const STAMPED_TEST: &str = "stamped(value) && ";
/// The marker in `ExceptionInfo` that records string messages for evaluator
/// diagnostics in the compile-time runtime only ([`compiler_hooks`]).
const EXCEPTION_MESSAGE_MARKER: &str = "/*@exmsg*/";
const EXCEPTION_MESSAGE_RECORD: &str =
    "if (typeof message === \"string\") exceptionMessages.set(target, message);";

/// The marker that opens a top-level binding's placeholder in a terse unit:
/// `\u{1}<binding index>\u{2}`, which the slice replaces with the binding's
/// name in that slice.
const OPEN: char = '\u{1}';
const CLOSE: char = '\u{2}';

/// Rust source for `TERSE_PROLOGUE`, `TERSE_BINDINGS`, `TERSE_KEPT`,
/// `TERSE_UNITS` and `TERSE_GLOBALS`: the runtime's prologue and every unit
/// body (in manifest order) written by `js_syntax::TerseWriter`, with locals
/// renamed (`Resolution::local_names`) and each top-level binding, declared
/// or read, as a placeholder; per unit, for each binding it names, the local
/// names visible at some occurrence, which that binding's name must avoid;
/// the names that keep their spelling (`shows_name`); and the free names
/// the units read, globals, which no name declared next to units written at
/// module level may take. The manifest itself
/// (`src/runtime_manifest.rs`) parses the annotated source at run time; the
/// unit tests of `src/terse.rs` check that these units match it and rename
/// without capture.
fn terse_runtime(source: &str) -> String {
    assert!(
        !source.contains([OPEN, CLOSE]),
        "src/js_runtime.js must not contain U+0001 or U+0002"
    );
    // Section ranges: the prologue, then one per `//@unit` header; the
    // export section is not embedded by slices.
    let mut prologue_end = None;
    let mut units: Vec<std::ops::Range<usize>> = Vec::new();
    let mut offset = 0;
    let mut in_unit = false;
    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let header = line.trim_start();
        if !header.starts_with("//@") {
            if in_unit {
                units.last_mut().unwrap().end = offset;
            }
            continue;
        }
        prologue_end.get_or_insert(start);
        if header.starts_with("//@unit") {
            units.push(offset..offset);
            in_unit = true;
        } else if header.trim_end() == "//@exports" {
            in_unit = false;
        } else if in_unit {
            // A continuation line of the header.
            units.last_mut().unwrap().start = offset;
            units.last_mut().unwrap().end = offset;
        }
    }
    let prologue_end = prologue_end.expect("the runtime has unit headers");
    let line_starts = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(at, _)| at + 1))
        .collect::<Vec<_>>();
    let prologue = js_syntax::lex_section(source, 0..prologue_end, false)
        .unwrap_or_else(|error| panic!("runtime prologue: {}", error.message));
    let prologue = js_syntax::terse(source, &prologue);

    let parsed = units
        .iter()
        .map(|range| {
            let tokens = js_syntax::lex(source, range.clone()).unwrap_or_else(|error| {
                panic!("src/js_runtime.js byte {}: {}", error.offset, error.message)
            });
            let resolution =
                js_syntax::resolve(source, &tokens).unwrap_or_else(|(offset, message)| {
                    panic!("src/js_runtime.js byte {offset}: {message}")
                });
            (tokens, resolution)
        })
        .collect::<Vec<_>>();
    let bindings = parsed
        .iter()
        .flat_map(|(_, resolution)| {
            resolution
                .declarations
                .iter()
                .filter(|declaration| declaration.scope == 0)
                .map(|declaration| declaration.name)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let index = bindings
        .iter()
        .enumerate()
        .map(|(index, &name)| (name, index))
        .collect::<HashMap<_, _>>();
    // Names that keep their spelling (`shows_name`), top-level or local.
    let kept = parsed
        .iter()
        .flat_map(|(tokens, resolution)| {
            resolution
                .declarations
                .iter()
                .filter(|declaration| shows_name(source, tokens, declaration))
                .map(|declaration| declaration.name)
        })
        .collect::<BTreeSet<_>>();

    let mut code =
        String::from("// Generated by build.rs from src/js_runtime.js; see src/terse.rs.\n\n");
    writeln!(
        code,
        "pub(crate) const TERSE_PROLOGUE: &str = {prologue:?};"
    )
    .unwrap();
    writeln!(
        code,
        "pub(crate) static TERSE_BINDINGS: [&str; {}] = {bindings:?};",
        bindings.len()
    )
    .unwrap();
    writeln!(
        code,
        "pub(crate) static TERSE_KEPT: [&str; {}] = {:?};",
        kept.len(),
        kept.iter().collect::<Vec<_>>()
    )
    .unwrap();
    writeln!(
        code,
        "pub(crate) static TERSE_UNITS: [TerseUnit; {}] = [",
        parsed.len()
    )
    .unwrap();
    let mut globals = BTreeSet::new();
    for (tokens, resolution) in &parsed {
        let names = resolution.local_names(
            |declaration| shows_name(source, tokens, declaration),
            |name| js_syntax::unavailable(name) || kept.contains(name),
        );
        let partners = js_syntax::partners(source, tokens);
        let redundant = js_syntax::redundant_semicolons(source, tokens, &partners);
        let joins = js_syntax::joined_declarations(source, tokens, &resolution.statements);
        let mut writer = TerseWriter::default();
        let mut avoid: Vec<(usize, HashSet<String>)> = Vec::new();
        let mut lines = LineMarks::default();
        for (position, token) in tokens.iter().enumerate() {
            lines.token(&writer.out, &line_starts, token.start as usize);
            match joins[position] {
                _ if redundant[position] => continue,
                Join::Drop => continue,
                Join::Comma => {
                    let comma = Shape {
                        kind: Kind::Punct,
                        text: ",",
                    };
                    writer.write(comma, ",", token.newline_before);
                    continue;
                }
                Join::Keep => {}
            }
            let text = token.text(source);
            let Some(occurrence) = resolution.occurrences[position] else {
                writer.token(source, token);
                continue;
            };
            let declaration = occurrence.declaration.map(|d| &resolution.declarations[d]);
            let binding = match declaration {
                Some(declaration) if declaration.scope == 0 && kept.contains(text) => None,
                Some(declaration) if declaration.scope == 0 => Some(index[text]),
                Some(_) => None,
                None => index.get(text).copied().filter(|_| !kept.contains(text)),
            };
            let written = match (binding, occurrence.declaration) {
                (Some(binding), _) => {
                    let visible = resolution.visible_locals(occurrence.scope, &names);
                    match avoid.iter_mut().find(|(b, _)| *b == binding) {
                        Some((_, set)) => set.extend(visible.into_iter().map(str::to_owned)),
                        None => {
                            avoid.push((binding, visible.into_iter().map(str::to_owned).collect()))
                        }
                    }
                    format!("{OPEN}{binding}{CLOSE}")
                }
                (None, Some(declaration)) => names[declaration]
                    .clone()
                    .unwrap_or_else(|| text.to_owned()),
                (None, None) => {
                    if !index.contains_key(text) {
                        globals.insert(text);
                    }
                    text.to_owned()
                }
            };
            let shape = Shape {
                kind: Kind::Name,
                text,
            };
            if occurrence.shorthand && written != text {
                // `{a}` becomes `{a:b}`.
                writer.write(shape, text, token.newline_before);
                writer.write(
                    Shape {
                        kind: Kind::Punct,
                        text: ":",
                    },
                    ":",
                    false,
                );
                writer.write(shape, &written, false);
            } else {
                writer.write(shape, &written, token.newline_before);
            }
        }
        lines.settle(&writer.out);
        // The next unit of a slice starts a statement.
        writer.end_statements();
        // A unit that starts with a `let` or `const` statement continues one
        // that the unit before it in a slice ends with.
        let keyword = |token: usize| {
            let text = tokens[token].text(source);
            matches!(text, "let" | "const").then_some(text)
        };
        let opens = resolution
            .statements
            .iter()
            .find(|(start, _)| *start == 0)
            .and_then(|(start, _)| keyword(*start));
        let closes = resolution
            .statements
            .iter()
            .find(|(_, end)| *end + 1 == tokens.len())
            .and_then(|(start, _)| keyword(*start));
        avoid.sort_by_key(|(binding, _)| *binding);
        let avoid = avoid
            .into_iter()
            .filter(|(_, names)| !names.is_empty())
            .map(|(binding, names)| {
                let mut names = names.into_iter().collect::<Vec<_>>();
                names.sort();
                format!("({binding}, {:?})", names.join(" "))
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            code,
            "    TerseUnit {{ text: {:?}, avoid: &[{avoid}], opens: {opens:?}, closes: {closes:?}, lines: {:?} }},",
            writer.out, lines.encoded
        )
        .unwrap();
    }
    code.push_str("];\n");
    writeln!(
        code,
        "pub(crate) static TERSE_GLOBALS: [&str; {}] = {:?};",
        globals.len(),
        globals.iter().collect::<Vec<_>>()
    )
    .unwrap();
    code
}

/// The source lines of a terse unit for its source map
/// (`TerseUnit::lines` in `src/terse.rs`): where in the unit's text the
/// first token of each source line starts, and that token's byte offset in
/// `src/js_runtime.js`, as pairs of Base64 VLQ deltas from the pair before.
#[derive(Default)]
struct LineMarks {
    encoded: String,
    line: Option<usize>,
    /// The first token of the current line, not yet written: the length of
    /// the text before it and its source offset.
    pending: Option<(usize, usize)>,
    previous: (usize, usize),
}

impl LineMarks {
    /// Note a token at byte `start` of the source, about to be written
    /// after `out`, the unit's text so far.
    fn token(&mut self, out: &str, line_starts: &[usize], start: usize) {
        self.settle(out);
        let line = line_starts.partition_point(|&at| at <= start);
        if self.line != Some(line) {
            self.line = Some(line);
            self.pending = Some((out.len(), start));
        }
    }

    /// Record the pending token once the writer has written it (a dropped
    /// token writes nothing and stays pending).
    fn settle(&mut self, out: &str) {
        let Some((before, start)) = self.pending.filter(|(before, _)| out.len() > *before) else {
            return;
        };
        self.pending = None;
        // The writer may separate the token by one space or line break.
        let at = before + usize::from(matches!(out.as_bytes()[before], b' ' | b'\n'));
        for (value, previous) in [(at, self.previous.0), (start, self.previous.1)] {
            vlq(&mut self.encoded, value as i64 - previous as i64);
        }
        self.previous = (at, start);
    }
}

/// Append `value` as a Base64 VLQ, as source maps write numbers.
fn vlq(out: &mut String, value: i64) {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rest = (value.unsigned_abs() << 1) | u64::from(value < 0);
    loop {
        let digit = rest & 31;
        rest >>= 5;
        out.push(char::from(
            DIGITS[(digit | if rest > 0 { 32 } else { 0 }) as usize],
        ));
        if rest == 0 {
            break;
        }
    }
}

/// Whether a declaration keeps its name in terse units: a constructor, whose
/// `name` the host shows (Node's `console.log` prints an instance as
/// `KeywordValue {}`) and the printer writes (`#object[...]`). That is a
/// function or class declaration with an uppercase initial (`KeywordValue`,
/// `Atom`), or a local one bound to a function or class expression (the
/// `Type` of `makeType`). Protocol markers (`const IEquiv = function () {}`)
/// have no instances and are renamed.
fn shows_name(
    source: &str,
    tokens: &[js_syntax::Token],
    declaration: &js_syntax::Declaration,
) -> bool {
    let Some(token) = declaration.token else {
        return false;
    };
    let text = |index: usize| tokens.get(index).map(|token| token.text(source));
    declaration
        .name
        .starts_with(|c: char| c.is_ascii_uppercase())
        && (matches!(
            token.checked_sub(1).and_then(text),
            Some("function" | "class")
        ) || (declaration.scope != 0
            && text(token + 1) == Some("=")
            && matches!(text(token + 2), Some("function" | "class"))))
}

/// Rust source for `CoreFnId::ALL` and `CoreFnId::table_row`, in row order.
/// Only the first column (the variant) is read here; `src/core_operations.rs`
/// parses the rest.
fn core_operation_index(table: &str) -> String {
    let mut variants = Vec::new();
    let mut seen = HashSet::new();
    for (index, line) in table.lines().enumerate() {
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let variant = line.split('\t').next().unwrap_or_default();
        let valid = variant.starts_with(|c: char| c.is_ascii_uppercase())
            && variant.chars().all(|c| c.is_ascii_alphanumeric());
        assert!(
            valid,
            "src/core_operations.tsv:{}: `{variant}` is not a CoreFnId variant name",
            index + 1
        );
        assert!(
            seen.insert(variant),
            "src/core_operations.tsv:{}: duplicate row for `{variant}`",
            index + 1
        );
        variants.push(variant);
    }
    let mut code = String::from(
        "// Generated by build.rs from src/core_operations.tsv. A `CoreFnId` variant\n\
         // missing from the table is a non-exhaustive match error below: add its row.\n\n\
         impl crate::analyzer::CoreFnId {\n",
    );
    writeln!(
        code,
        "    /// Every core operation, in the row order of `src/core_operations.tsv`.\n    \
         pub const ALL: [Self; {}] = [",
        variants.len()
    )
    .unwrap();
    for variant in &variants {
        writeln!(code, "        Self::{variant},").unwrap();
    }
    code.push_str(
        "    ];\n\n    /// Index of this operation's row among the table rows.\n    \
         pub(crate) const fn table_row(self) -> usize {\n        match self {\n",
    );
    for (row, variant) in variants.iter().enumerate() {
        writeln!(code, "            Self::{variant} => {row},").unwrap();
    }
    code.push_str("        }\n    }\n}\n");
    code
}
