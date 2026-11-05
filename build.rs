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
    println!("cargo::rustc-check-cfg=cfg(light_quickjs)");
    println!(
        "cargo::rustc-env=LIGHT_CLJS_COMPILER_IDENTITY={}",
        sha256::hex(&compiler_identity())
    );
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    // QuickJS is optional on native targets and absent on wasm32.
    if arch != "wasm32" && env::var_os("CARGO_FEATURE_QUICKJS").is_some() {
        println!("cargo::rustc-cfg=light_quickjs");
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
        cc::Build::new()
            .cpp(true)
            .std("c++20")
            .include("vendor/v8-headers/include")
            .define("V8_COMPRESS_POINTERS", None)
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
    fs::write(out.join("terse_runtime.rs"), terse_runtime(&source))
        .expect("write the terse runtime");

    println!("cargo::rerun-if-changed=src/js_shared_profile.js");
    let profile =
        fs::read_to_string("src/js_shared_profile.js").expect("read src/js_shared_profile.js");
    let (annotated, shared) = shared_runtime(&source, &profile);
    fs::write(out.join("js_shared_annotated.js"), annotated)
        .expect("write the annotated shared-heap runtime");
    fs::write(out.join("js_shared_runtime.js"), shared).expect("write the shared-heap runtime");

    println!("cargo::rerun-if-changed=src/core_operations.tsv");
    let table =
        fs::read_to_string("src/core_operations.tsv").expect("read src/core_operations.tsv");
    fs::write(out.join("core_operations.rs"), core_operation_index(&table))
        .expect("write the core operation index");
}

/// The trees and files whose every byte the compiler identity covers: the
/// crate's sources (the embedded runtime, bootstrap, core macros and
/// bundled first-party libraries are under `src/`), this script, the
/// manifest and lock file, and the vendored engines with their patches.
const IDENTITY_INPUTS: [&str; 9] = [
    "src",
    "build.rs",
    "Cargo.toml",
    "Cargo.lock",
    "vendor/rquickjs-sys",
    "vendor/v8",
    "vendor/v8-headers",
    "crates/big-boy-pants-js/rust/src",
    "crates/big-boy-pants-js/rust/Cargo.toml",
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
///   counts in the runtime from zero the first time the runtime sees it; a
///   native one that has ended throws a named error (the compiler answers
///   `!`, light-cljs-802). In a scope it skips no
///   `generatedNames`, so its names do not depend on what the heap decoded
///   before; it still logs them. Scope 0 is none, where `gensym` uses its
///   counter (`src/generated_names.rs`, `Scope`). `gensymFrame` puts in
///   place, or takes away with `null`, a frozen initializer scope that
///   `gensym` numbers in ahead of every other, from one, and returns the
///   frame it replaced (`frozen_initializer` in `src/generated_names.rs`).
///   Once `takeGensyms` has
///   been called in a runtime with `__lightGensym`, it logs each name there,
///   so that the compiler's log keeps them in order with the names native
///   expansions allocate; a name with a surrogate goes as its code units.
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
///   `externalMethod` and `publicMethod` run with the calls between them
///   (`demandMethod`, `keySlot`, `nativeType`, `publicDeclared` and
///   `externalMethod` where it is still the hook) inlined. Where the stores
///   are WeakMaps (`weakMapStores`), a `storesPredicate` answers false for
///   a primitive, which no store holds, tests the stores whose test is a
///   WeakMap's first, and `maps` and `mapKinds`, whose record test walks
///   the prototype chain and can reach a proxy's trap, last and in order,
///   with the first eight tests unrolled.
fn compiler_runtime(runtime: &str) -> String {
    compiler_hooks(runtime, false)
}

/// The compile-time hooks inserted into `runtime` (see [`compiler_runtime`]).
/// For the shared-heap profile (`shared`), a hook whose anchor the profile
/// has swapped away is skipped when the profile's unit carries the hook's
/// marker itself, the hooks' own per-task `let`s are dropped (their defaults
/// live on `sharedTaskDefaults`), and their reads go through the task's
/// cells like every other unit's ([`rewrite_task_state`]).
fn compiler_hooks(runtime: &str, shared: bool) -> String {
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
        "  publicMethod = function (value, key, arity, fast = false, directOnly = false) { // interpreterDispatch\n",
        "    const info = key.info || (key.info = publicMethodInfo(key.key)), protocol = info.protocol, mask = info.mask, native = info.native, name = info.name; // interpreterDispatch\n",
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
        "    const declared = key.info || (key.info = publicMethodInfo(key.key)); // interpreterDispatch\n",
        "    if (declared.protocol === IMeta && typeof value === \"function\") return undefined; // interpreterDispatch\n",
        "    if (!declared.native?.(value)) throw new Error(\"No protocol method \" + key.key + \" defined for this value\"); // interpreterDispatch\n",
        "    return undefined; }; // interpreterDispatch\n",
    );
    const DISPATCH_STORES: &str = concat!(
        "  if (weakMapStores) storesPredicate = (...stores) => { const late = [maps, mapKinds, recordTypes, typeFields], weak = WeakMap.prototype.has; // interpreterDispatch\n",
        "    const ordered = [...stores.filter(store => !late.includes(store) && store.has === weak), ...stores.filter(store => !late.includes(store) && store.has !== weak), ...stores.filter(store => late.includes(store))], n = ordered.length, [a, b, c, d, e, f, g, h] = ordered; // interpreterDispatch\n",
        "    return value => { if (typeof value === \"object\" ? value === null : typeof value !== \"function\") return false; // interpreterDispatch\n",
        "      if (n > 0 && (a.has(value) || n > 1 && (b.has(value) || n > 2 && (c.has(value) || n > 3 && (d.has(value) || n > 4 && (e.has(value) || n > 5 && (f.has(value) || n > 6 && (g.has(value) || n > 7 && h.has(value))))))))) return true; // interpreterDispatch\n",
        "      for (let i = 8; i < n; i++) if (ordered[i].has(value)) return true; // interpreterDispatch\n",
        "      return false; }; }; // interpreterDispatch\n",
    );
    const HOOKS: [(&str, &str, &str); 15] = [
        (
            "    const ns = createNs(name);\n    const context = ns.readerContext ?? {aliases: new Map(), refers: new Map(), excludes: new Set(), bindings: new Set()};\n",
            "    const context",
            READER_CONTEXT,
        ),
        (
            "  const generatedNames = new Set();\n",
            "",
            concat!(
                "  let gensymLog = null;\n  let gensymScope = null; const gensymScopes = new Map(); let gensymFrame = null;\n",
                "  const gensymLogName = name => { if (gensymLog === true) globalThis.__lightGensym(/[\\ud800-\\udfff]/.test(name) ? \"U\" + Array.from({length: name.length}, (_, i) => name.charCodeAt(i)).join(\",\") : \"L\" + name); else gensymLog.push(name); };\n",
            ),
        ),
        (
            "    let name; do { name = base + ++gensymCounter; } while (generatedNames.has(name));\n",
            "    let name",
            "    if (gensymFrame !== null) { const name = base + gensymFrame.prefix + ++gensymFrame.count; if (gensymLog !== null) gensymLogName(name); return symbolCore(name); }\n    if (gensymScope !== null) { const suffix = gensymScope.native ? globalThis.__lightGensym(\"g\" + gensymScope.serial) : \"\"; if (suffix === \"!\") throw new Error(\"gensym called in the generated-name scope \" + gensymScope.prefix + \" after it ended, outside every expansion and initializer\"); const name = base + (suffix || gensymScope.prefix + ++gensymScope.count); if (gensymLog !== null) gensymLogName(name); return symbolCore(name); }\n",
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
                "      gensymScope: (serial, prefix, retired, native) => { if (retired) for (const ended of retired.split(\",\")) gensymScopes.delete(+ended); if (serial === 0) { gensymScope = null; return null; } let scope = gensymScopes.get(serial); if (scope === undefined) gensymScopes.set(serial, scope = {serial, prefix, count: 0, native: native === true && typeof globalThis.__lightGensym === \"function\"}); gensymScope = scope; return null; }, gensymScopeCount: () => gensymScopes.size, gensymFrame: frame => { const outer = gensymFrame; gensymFrame = frame; return outer; },\n",
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
        (
            "      for (let i = 0; i < stores.length; i++) if (stores[i].has(value)) return true;\n      return false;\n    };\n  }\n",
            "",
            DISPATCH_STORES,
        ),
    ];
    let mut runtime = runtime.to_owned();
    for (anchor, before, hook) in HOOKS {
        // In the shared-heap profile an anchor reading task state has been
        // rewritten like the unit around it (a declaring token has not).
        let rewritten = (shared && !runtime.contains(anchor))
            .then(|| (shared_hook(anchor), shared_hook(before)));
        let (anchor, before) = match &rewritten {
            Some((anchor, before)) => (anchor.as_str(), before.as_str()),
            None => (anchor, before),
        };
        let matches = runtime.matches(anchor).count();
        if shared && matches == 0 {
            // The profile swapped the unit holding the anchor; its unit must
            // carry the hook (`tests/compiler/runtime_manifest.rs` checks the
            // markers).
            let marker = hook
                .split('\n')
                .next()
                .unwrap()
                .rsplit("// ")
                .next()
                .unwrap()
                .trim();
            assert!(
                runtime.contains(marker),
                "shared-heap profile: the swapped unit at hook anchor {anchor:?} lacks the hook ({marker})"
            );
            continue;
        }
        assert_eq!(matches, 1, "compile-time runtime hook anchor {anchor:?}");
        let within = if before.is_empty() {
            anchor.len()
        } else {
            anchor.find(before).unwrap()
        };
        let hook = if shared {
            shared_hook(hook)
        } else {
            hook.to_owned()
        };
        runtime.insert_str(runtime.find(anchor).unwrap() + within, &hook);
    }
    runtime
}

/// The bindings of the runtime that functions assign after boot, which the
/// shared-heap profile keeps per task on the task's cells
/// (`globalThis.__lightSharedCells`, whose prototype chain ends at the
/// profile's `sharedTaskDefaults`): the module-level `let`s and the
/// per-task registries next to them, and the `let`s the compile-time hooks
/// declare. Every read and write of one in a unit body is rewritten to a
/// property of the task's cells at assembly ([`rewrite_task_state`]); the
/// declaring statement stays as an unread binding.
const SHARED_TASK_STATE: &[&str] = &[
    // gensym (the counter is the compiler's `__lightGensym` once the hooks
    // run; `generatedNames` and the scopes are the task's).
    "gensymCounter",
    "generatedNames",
    "gensymLog",
    "gensymScope",
    "gensymScopes",
    "gensymFrame",
    // Evaluator services and the declaration journal.
    "macroexpandServices",
    "declarationJournal",
    "demandProtocol",
    "demandMethod",
    "demandMulti",
    "demandGate",
    "demandHost",
    "patchDeclaration",
    "setDeclaration",
    // The identity counter: dead in this profile, where the engine hashes
    // a host object by its heap identity (`identityHash`); the verbatim
    // `hash` unit still names it in the branch the view never takes.
    "identityCounter",
    // The global hierarchy (an engine atom under effect lanes is 649's).
    "globalHierarchy",
    // Taps.
    "tapListeners",
    "tapQueue",
    "tapDraining",
    "tapDelivered",
    "tapPending",
    "tapLimit",
    // The slot-name memo of `protocolSlot`.
    "protocolSlots",
    // The dispatch and declaration registries written after boot: each
    // task's own Map, seeded with the boot's entries (`sharedSnapshots` in
    // the profile), until light-cljs-649 moves them to engine tables.
    "coreDispatchers",
    "nativeMethods",
    "protocolValues",
    "protocolFunctions",
    "protocolVariables",
    "protocolGetters",
    "reifySites",
    // The namespace registry `createNs` writes after boot: each task's own
    // Map, seeded with the boot's `cljs.user` and `cljs.core`.
    "namespaces",
];

/// The state the `reader` unit keeps at the top of its own function (the
/// IIFE's scope, scope 1 of the unit's body), which the shared-heap profile
/// also keeps per task: the stream and reader-map registries (WeakMaps),
/// the tag tables (Maps, filled while booting and registered into after
/// boot) and the default-tag and depth `let`s. Rewritten like
/// [`SHARED_TASK_STATE`] (light-cljs-666); the declaring statements stay as
/// unread locals.
const SHARED_READER_STATE: &[&str] = &[
    "streams",
    "readerMaps",
    "tags",
    "builtinTags",
    "defaultTag",
    "activeDepth",
];

/// The unit whose scope-1 declarations [`SHARED_READER_STATE`] names.
const SHARED_READER_UNIT: &str = "reader";

/// The objects the reader's functions capture that no binding reaches, so
/// the boot's walk cannot seal them: the IIFE's argument object (the
/// runtime services it reads as `rt`), its two frozen sentinels and the
/// builtin tag parsers' location (captured by the `inst` and `js` parsers,
/// which the tag tables' boot snapshots hold; light-cljs-688), sealed
/// where they are built (`__rt.seal` is shallow; the functions the argument
/// holds are top-level bindings, sealed by the boot).
const SHARED_READER_REWRITES: &[(&str, &str)] = &[
    (
        "  const builtinLocation = {name: '<tag-parser>', line: 1, column: 1, offset: 0};\n",
        "  const builtinLocation = __rt.seal({name: '<tag-parser>', line: 1, column: 1, offset: 0});\n",
    ),
    (
        "  })({keyword: freshKeyword, ",
        "  })(__rt.seal({keyword: freshKeyword, ",
    ),
    (
        "value instanceof Date ? 'instant' : null});\n",
        "value instanceof Date ? 'instant' : null}));\n",
    ),
    (
        "  const absent = Object.freeze({}), discarded = Object.freeze({});\n",
        "  const absent = __rt.seal({}), discarded = __rt.seal({});\n",
    ),
];

/// Where a unit reads the task's cells: `coreCells.X` in the V8 profile.
const SHARED_CELLS: &str = "globalThis.__lightSharedCells";

/// The immutable vocabularies the runtime keeps in a Set, which never
/// publishes: in the shared-heap profile their units build them with the
/// profile's `sharedWords` instead (`const NAME = new Set(` becomes
/// `const NAME = sharedWords(`), so the words are written once.
const SHARED_WORDS: &[&str] = &["reservedNames", "readerCoreNames", "specialSymbols"];

/// A compile-time hook's text for the shared-heap profile: its own `let`
/// declarations dropped and its reads of task state rewritten.
fn shared_hook(hook: &str) -> String {
    let mut text = hook
        .replace("  let gensymLog = null;\n", "")
        .replace(
            "  let gensymScope = null; const gensymScopes = new Map(); let gensymFrame = null;\n",
            "",
        )
        .replace(
            "  let demandGate = null, demandHost = null; // interpreterDispatch\n",
            "",
        );
    for name in SHARED_TASK_STATE {
        text = replace_identifier(&text, name, &format!("{SHARED_CELLS}.{name}"));
    }
    text.replace("coreCells.", &format!("{SHARED_CELLS}."))
        .replace("coreCells[", &format!("{SHARED_CELLS}["))
}

/// `text` with every whole-identifier occurrence of `name` that is not a
/// property name (`.name`) replaced.
fn replace_identifier(text: &str, name: &str, replacement: &str) -> String {
    let is_word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '$');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].chars().next();
        // A property key (`gensymScope: (serial, ...) =>`) keeps its name.
        let key = rest[at + name.len()..].trim_start().starts_with(':');
        out.push_str(&rest[..at]);
        if before.is_some_and(|c| is_word(c) || c == '.') || after.is_some_and(is_word) || key {
            out.push_str(name);
        } else {
            out.push_str(replacement);
        }
        rest = &rest[at + name.len()..];
    }
    out.push_str(rest);
    out
}

/// A unit body with its reads and writes of [`SHARED_TASK_STATE`] bindings
/// rewritten to the task's cells, exactly: an identifier token that resolves
/// to the unit's own top-level declaration of the name (other than the
/// declaring token itself) or is free (another unit's binding). Locals of
/// the same name are untouched. In the unit [`SHARED_READER_UNIT`], the
/// [`SHARED_READER_STATE`] names declared in a scope directly under the top
/// level (the reader's own function) are rewritten the same way.
fn rewrite_task_state(unit: &str, source: &str, body: std::ops::Range<usize>) -> String {
    let tokens = js_syntax::lex(source, body.clone()).unwrap_or_else(|error| {
        panic!(
            "shared-heap runtime byte {}: {}",
            error.offset, error.message
        )
    });
    let resolution = js_syntax::resolve(source, &tokens)
        .unwrap_or_else(|(offset, message)| panic!("shared-heap runtime byte {offset}: {message}"));
    let reader = unit == SHARED_READER_UNIT;
    let mut out = String::with_capacity(body.len());
    let mut at = body.start;
    for (position, token) in tokens.iter().enumerate() {
        let text = token.text(source);
        let task_state = SHARED_TASK_STATE.contains(&text);
        let reader_state = reader && SHARED_READER_STATE.contains(&text);
        if !task_state && !reader_state {
            continue;
        }
        let Some(occurrence) = resolution.occurrences[position] else {
            continue;
        };
        let rewrite = match occurrence.declaration {
            None => task_state,
            Some(declaration) => {
                let declaration = &resolution.declarations[declaration];
                declaration.token != Some(position)
                    && ((task_state && declaration.scope == 0)
                        || (reader_state
                            && declaration.scope != 0
                            && resolution.parents[declaration.scope] == 0))
            }
        };
        if !rewrite {
            continue;
        }
        assert!(
            !occurrence.shorthand,
            "shared-heap runtime: `{text}` as a shorthand property at byte {}",
            token.start
        );
        out.push_str(&source[at..token.start]);
        out.push_str(SHARED_CELLS);
        out.push('.');
        out.push_str(text);
        at = token.start + text.len();
    }
    out.push_str(&source[at..body.end]);
    out
}

/// One section of an annotated runtime file: a unit (its name, header and
/// body) or the export section (name `//@exports`).
struct Section {
    name: String,
    header: std::ops::Range<usize>,
    body: std::ops::Range<usize>,
}

/// The sections of an annotated runtime, after the prologue (whose end is
/// returned first): a header is a `//@unit NAME ...` line with its `//@ `
/// continuation lines, or `//@exports`; a body runs to the next header.
fn sections(source: &str) -> (usize, Vec<Section>) {
    let mut prologue_end = None;
    let mut sections: Vec<Section> = Vec::new();
    let mut offset = 0;
    let mut in_header = false;
    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let header = line.trim_start();
        if !header.starts_with("//@") {
            if let Some(section) = sections.last_mut() {
                section.body.end = offset;
            }
            in_header = false;
            continue;
        }
        prologue_end.get_or_insert(start);
        if let Some(rest) = header.strip_prefix("//@unit ") {
            let name = rest
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned();
            sections.push(Section {
                name,
                header: start..offset,
                body: offset..offset,
            });
            in_header = true;
        } else if header.trim_end() == "//@exports" {
            sections.push(Section {
                name: "//@exports".to_owned(),
                header: start..offset,
                body: offset..offset,
            });
            in_header = true;
        } else if in_header {
            let section = sections.last_mut().unwrap();
            section.header.end = offset;
            section.body = offset..offset;
        } else {
            panic!("a `//@` continuation line must follow a `//@unit` header");
        }
    }
    (
        prologue_end.expect("the runtime has unit headers"),
        sections,
    )
}

/// The shared-heap profile of the runtime (docs/real-core-design.md,
/// section 1): `src/js_runtime.js` with the units `src/js_shared_profile.js`
/// names replaced by its units of the same names, its other units placed
/// first (they declare what the swapped units need), its export section the
/// profile's (which seals the booted runtime), everything else the V8
/// profile's text. Returns the annotated result (which
/// `tests/compiler/runtime_manifest.rs` parses and validates like the
/// runtime itself) and the runtime to boot: the annotated text without
/// headers, with `coreCells.X` and the [`SHARED_TASK_STATE`] bindings
/// rewritten to the task's cells, and with the compile-time hooks.
fn shared_runtime(base: &str, profile: &str) -> (String, String) {
    let constructor_base = shared_owned_allocations(base);
    let update_base = shared_persistent_updates(&constructor_base);
    let scalar_base = shared_scalar_reads(&update_base);
    let base = scalar_base.as_str();
    let (base_prologue, base_sections) = sections(base);
    let (_, profile_sections) = sections(profile);
    let mut swapped: HashMap<&str, &Section> = HashMap::new();
    for section in &profile_sections {
        assert!(
            swapped.insert(&section.name, section).is_none(),
            "src/js_shared_profile.js: duplicate unit `{}`",
            section.name
        );
    }
    let base_names = base_sections
        .iter()
        .map(|section| section.name.as_str())
        .collect::<HashSet<_>>();
    assert!(
        swapped.contains_key("//@exports"),
        "src/js_shared_profile.js: no `//@exports` section"
    );
    // Each part of the annotated text: its source and the ranges within it.
    let mut parts: Vec<(&str, &Section)> = Vec::new();
    for section in &profile_sections {
        if !base_names.contains(section.name.as_str()) && section.name != "//@exports" {
            parts.push((profile, section));
        }
    }
    for section in &base_sections {
        match swapped.get(section.name.as_str()) {
            Some(swap) => parts.push((profile, swap)),
            None => parts.push((base, section)),
        }
    }
    // The boot unit seals what every top-level binding holds, so it names
    // them all: its list and its `requires:` header are generated here
    // (`SHARED_BINDINGS` in the profile's body), in declaration order.
    let bindings = parts
        .iter()
        .filter(|(_, section)| section.name != "//@exports")
        .flat_map(|(source, section)| top_level_bindings(source, section.body.clone()))
        .collect::<Vec<_>>();
    let boot = parts
        .iter()
        .find(|(_, section)| section.name == SHARED_BOOT)
        .map(|(source, section)| {
            let own = top_level_bindings(source, section.body.clone());
            let names = bindings
                .iter()
                .filter(|name| !own.contains(name) && !SHARED_UNLISTED.contains(name))
                .copied()
                .collect::<Vec<_>>();
            let quoted = names
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>();
            let body = source[section.body.clone()]
                .replace(SHARED_BINDINGS, &wrap_list(&names, "      ", 100))
                .replace(
                    SHARED_BINDING_NAMES,
                    &wrap_list(
                        &quoted.iter().map(String::as_str).collect::<Vec<_>>(),
                        "        ",
                        100,
                    ),
                );
            assert!(
                body != source[section.body.clone()],
                "src/js_shared_profile.js: unit `{SHARED_BOOT}` has no `{SHARED_BINDINGS}`"
            );
            (shared_boot_header(&names), body)
        })
        .unwrap_or_else(|| panic!("src/js_shared_profile.js: no unit `{SHARED_BOOT}`"));
    let mut annotated = String::with_capacity(base.len());
    let mut runtime = String::with_capacity(base.len());
    annotated.push_str(&base[..base_prologue]);
    runtime.push_str(&base[..base_prologue]);
    for (source, section) in &parts {
        if section.name == SHARED_BOOT {
            annotated.push_str(&boot.0);
            annotated.push_str(&boot.1);
            runtime.push_str(&boot.1);
            continue;
        }
        annotated.push_str(&source[section.header.clone()]);
        annotated.push_str(&source[section.body.clone()]);
        if section.name == "//@exports" {
            runtime.push_str(&source[section.body.clone()]);
        } else {
            let mut body = rewrite_task_state(&section.name, source, section.body.clone())
                .replace("coreCells.", &format!("{SHARED_CELLS}."))
                .replace("coreCells[", &format!("{SHARED_CELLS}["));
            if SHARED_WORDS.contains(&section.name.as_str()) {
                let set = format!("const {} = new Set(", section.name);
                assert_eq!(body.matches(&set).count(), 1, "{set}");
                body = body.replace(&set, &format!("const {} = sharedWords(", section.name));
            }
            if section.name == SHARED_READER_UNIT {
                // The reader's functions capture objects no binding reaches:
                // sealed where they are built.
                for (from, to) in SHARED_READER_REWRITES {
                    assert_eq!(body.matches(from).count(), 1, "{from}");
                    body = body.replace(from, to);
                }
            }
            runtime.push_str(&body);
        }
    }
    (annotated, compiler_hooks(&runtime, true))
}

/// Public constructors and the public tree builder retain their canonical
/// algorithms. Only their allocation boundary differs: constructorSlots and
/// publicTree need runtime-owned objects on which they can attach fields.
fn shared_owned_allocations(base: &str) -> String {
    let (prologue, units) = sections(base);
    let mut result = base[..prologue].to_owned();
    for unit in units {
        let builder = match unit.name.as_str() {
            "publicArrayMap" | "PersistentHashMapValue" => Some(("makeMap", "sharedOwnedMap")),
            "publicHashSet" => Some(("makeSet", "sharedOwnedSet")),
            "publicTree" => Some(("mapEntry", "sharedTreeEntry")),
            _ => None,
        };
        let header = &base[unit.header];
        let body = &base[unit.body];
        if let Some((from, to)) = builder {
            let call = format!("{from}(");
            assert_eq!(body.matches(&call).count(), 1, "constructor {}", unit.name);
            result.push_str(&replace_identifier(header, from, to));
            result.push_str(&body.replace(
                &call,
                &format!("/*public-constructor*/{to}/*public-constructor*/("),
            ));
        } else {
            result.push_str(header);
            result.push_str(body);
        }
    }
    result
}

/// Keep canonical protocol dispatch, arity handling and runtime fallbacks.
/// Only shared engine forms take the persistent leaves, before an entry-array
/// read. Mark every insertion so the manifest test restores the V8 bytes.
fn shared_persistent_updates(base: &str) -> String {
    let (prologue, units) = sections(base);
    let mut result = base[..prologue].to_owned();
    for unit in units {
        let header = &base[unit.header];
        let body = &base[unit.body];
        let leaf = match unit.name.as_str() {
            "assoc" => Some((
                "    const mapSlots = publicCollectionSlots.get(value);",
                "    if (sharedUpdates.accepts(value, [key], \"map\", true)) return sharedUpdates.assoc(value, key, item, pairs);\n",
            )),
            "dissoc" => Some((
                "    const original = maps.get(value);",
                "    if (sharedUpdates.accepts(value, keys, \"map\")) return sharedUpdates.dissoc(value, keys);\n",
            )),
            "makeSet.conj" => Some((
                "    if (publicCollectionSlots.has(result)) {",
                "    if (sharedUpdates.accepts(result, [item], \"set\", true)) return sharedUpdates.conj(result, item);\n",
            )),
            "disj" => Some((
                "    const original = sets.get(value);",
                "    if (sharedUpdates.accepts(value, keys, \"set\")) return sharedUpdates.disj(value, keys);\n",
            )),
            "lookup" => Some((
                "    const entries = maps.get(value);",
                "    if (sharedUpdates.accepts(value, [key], \"map\")) return sharedUpdates.lookup(value, key, notFound);\n",
            )),
            "lookup.sets" => Some((
                "    const members = sets.get(value);",
                "    if (sharedUpdates.accepts(value, [key], \"set\")) return sharedUpdates.lookup(value, key, notFound);\n",
            )),
            _ => None,
        };
        if let Some((before, leaf)) = leaf {
            assert_eq!(body.matches(before).count(), 1, "update {}", unit.name);
            result.push_str(&header.replacen("requires:", "requires: sharedUpdates,", 1));
            result.push_str(&body.replace(
                before,
                &format!("/*persistent-update*/{leaf}/*end-persistent-update*/{before}"),
            ));
        } else if unit.name == "updatedMap" {
            let call = "mapFromEntries(entries,";
            assert_eq!(body.matches(call).count(), 1, "updatedMap allocation");
            result.push_str(&replace_identifier(
                header,
                "mapFromEntries",
                "sharedUpdatedMapFromEntries",
            ));
            result.push_str(&body.replace(
                call,
                "/*updated-map*/sharedUpdatedMapFromEntries(original, entries,/*end-updated-map*/",
            ));
        } else {
            result.push_str(header);
            result.push_str(body);
        }
    }
    result
}

/// Imported scalar occurrences retain compiler identity until an operation
/// reads their primitive. Adapt the canonical units only in the shared profile:
/// do not fork the algorithms or erase arguments at calls/collection storage.
/// Mark inserted text so the manifest test can restore every canonical byte.
fn shared_scalar_reads(base: &str) -> String {
    let (prologue, units) = sections(base);
    let mut result = base[..prologue].to_owned();
    for unit in units {
        let mut body = base[unit.body.clone()].to_owned();
        let original = body.clone();
        let name = unit.name.as_str();
        let scalar = |value: &str| format!("sharedScalar(/*scalar*/{value}/*scalar*/)");
        // Scalar predicates and arithmetic always produce a computed result.
        // The canonical arrow expression stays byte-for-byte inside the body.
        let parameters = match name {
            "inc" | "dec" | "uncheckedInc" | "uncheckedIncInt" | "uncheckedDec"
            | "uncheckedDecInt" | "uncheckedNegate" | "uncheckedNegateInt" | "abs" | "int"
            | "fix" | "bitNot" | "isZero" | "isPositive" | "isNegative" | "isInteger"
            | "isPositiveInt" | "isNegativeInt" | "isNaturalInt" | "isNumber" | "isFloat"
            | "isDouble" | "isInfinite" | "boolean" | "not" | "isBoolean" | "isTrue"
            | "isFalse" | "isNil" | "isSome" | "isUndefined" | "isString" | "isChar"
            | "isSeqable" => "x",
            "mod" | "quot" | "jsMod" | "rem" => "n,d",
            "bitClear"
            | "bitFlip"
            | "bitSet"
            | "bitTest"
            | "bitShiftLeft"
            | "bitShiftRight"
            | "bitShiftRightZeroFill"
            | "unsignedBitShiftRight" => "x,n",
            "truth" => "value",
            _ => "",
        };
        if !parameters.is_empty() {
            assert_eq!(body.matches(" => ").count(), 1, "scalar arrow {name}");
            let args = if parameters.contains(',') {
                format!("({})", parameters.replace(',', ", "))
            } else {
                parameters.to_owned()
            };
            assert!(
                body.contains(&format!("const {name} = {args} => ")),
                "scalar arrow {name}"
            );
            let (head, expression) = body.split_once(" => ").expect("scalar arrow");
            let (expression, tail) = expression.rsplit_once(';').expect("scalar arrow end");
            let inputs = parameters
                .split(',')
                .map(|p| format!("{p} = sharedScalar({p}); "))
                .collect::<String>();
            body = format!(
                "{head} => /*scalar-inputs*/{{ {inputs}return /*end-scalar-inputs*/{expression}/*scalar-inputs*/; }}/*end-scalar-inputs*/;{tail}"
            );
        } else {
            // Read-only receivers, numeric indexes and sizes. Defaults, stored
            // values, callbacks and collection occurrences remain untouched.
            let (signature, inputs) = match name {
                "seq" | "first" | "rest" | "next" | "count" | "hash" | "parsingString"
                | "numberArgument" => (format!("function {name}(value) {{"), "value"),
                "nth" => (
                    "function nth(value, index, notFound) {".into(),
                    "value,index",
                ),
                "lookup" | "get" => (format!("function {name}(value, key, notFound) {{"), "value"),
                "equalPair" => ("function equalPair(left, right) {".into(), "left,right"),
                "compareKeys" => ("function compareKeys(a, b) {".into(), "a,b"),
                "printWriterImpl" => (
                    "function printWriterImpl(value, writer, options, skipMetadata = false) {"
                        .into(),
                    "value",
                ),
                "subs" => (
                    "function subs(value, start, end) {".into(),
                    "value,start,end",
                ),
                "range" => (
                    "function range(start, end, step = 1) {".into(),
                    "start,end,step",
                ),
                "take" | "drop" | "takeNth" => (format!("function {name}(n, coll) {{"), "n"),
                "nthrest" | "nthnext" => (format!("function {name}(value, n) {{"), "n"),
                "isEven" | "hostRand" | "hostRandInt" => (format!("function {name}(n) {{"), "n"),
                "bitCount" => ("function bitCount(v) {".into(), "v"),
                "symbolCore" | "keywordCore" => (
                    format!("function {name}(namespace, name) {{"),
                    "namespace,name",
                ),
                "partition" | "partitionV" => {
                    (format!("function {name}(n, step, pad, coll) {{"), "n,step")
                }
                _ => (String::new(), ""),
            };
            if !inputs.is_empty() {
                let reads = inputs
                    .split(',')
                    .map(|p| format!("{p} = sharedScalar({p}); "))
                    .collect::<String>();
                assert_eq!(
                    body.matches(&signature).count(),
                    1,
                    "scalar adapter {name}: {signature}"
                );
                body = body.replacen(
                    &signature,
                    &format!("{signature}/*scalar-inputs*/{reads}/*end-scalar-inputs*/"),
                    1,
                );
            }
            // Selectors keep their original operands on every return path.
            for (from, to) in match name {
                "add" => vec![("x + y", format!("{} + {}", scalar("x"), scalar("y"))),
                    ("result + arguments[i]", format!("result + {}", scalar("arguments[i]")))],
                "subtract" => vec![("? -x :", format!("? -{} :", scalar("x"))),
                    ("a - b", format!("{} - {}", scalar("a"), scalar("b")))],
                "multiply" => vec![("a * b", format!("{} * {}", scalar("a"), scalar("b")))],
                "divide" => vec![("? 1 / x :", format!("? 1 / {} :", scalar("x"))),
                    ("a / b", format!("{} / {}", scalar("a"), scalar("b")))],
                "bitOperation" => vec![("numericFold(operation, x, y, more)", format!("numericFold(operation, {}, {}, more/*scalar-inputs*/.map(sharedScalar)/*end-scalar-inputs*/)", scalar("x"), scalar("y")))],
                "foldSeq" => vec![("operation(x, y)", format!("operation({}, {})", scalar("x"), scalar("y"))),
                    ("operation(result, first(s))", format!("operation(result, {})", scalar("first(s)")))],
                "comparison" | "comparisonVariadic" => vec![("test(x, y)", format!("test({}, {})", scalar("x"), scalar("y"))),
                    ("test(y, item)", format!("test({}, {})", scalar("y"), scalar("item")))],
                "extrema" => vec![("isNaN(a)", format!("isNaN({})", scalar("a"))), ("isNaN(b)", format!("isNaN({})", scalar("b"))),
                    ("test(a, b)", format!("test({}, {})", scalar("a"), scalar("b")))],
                "char" => vec![("typeof x", format!("typeof {}", scalar("x"))), ("String.fromCharCode(x)", format!("String.fromCharCode({})", scalar("x"))), ("x.length", format!("{}.length", scalar("x")))],
                "name" => vec![("typeof value", format!("typeof {}", scalar("value")))],
                "isNaNCore" => vec![("isNaN(x)", format!("isNaN({})", scalar("x")))],
                "lookup" | "contains" => vec![("const store = vectors.get(value);", "/*scalar-inputs*/key = sharedScalar(key); /*end-scalar-inputs*/const store = vectors.get(value);".into())],
                "get" => vec![("if (Array.isArray(value)) {", "if (Array.isArray(value)) {/*scalar-inputs*/key = sharedScalar(key); /*end-scalar-inputs*/".into())],
                "identical" => vec![("a === b", format!("{} === {}", scalar("a"), scalar("b")))],
                "fnil" => vec![("x == null", format!("{} == null", scalar("x")))],
                "indexedSequence" => vec![("value != null", format!("{} != null", scalar("value")))],
                "indexedTransform" => vec![("value == null", format!("{} == null", scalar("value")))],
                "complement" => vec![("value == null", format!("{} == null", scalar("value"))),
                    ("value === false", format!("{} === false", scalar("value")))],
                "bufferedTransform" => vec![("buffer.length === group.n", format!("buffer.length === {}", scalar("group.n")))],
                "iteration" => vec![("value != null", format!("{} != null", scalar("value"))),
                    ("key == null", format!("{} == null", scalar("key")))],
                _ => vec![],
            } {
                // A few reads occur twice in the same canonical expression.
                let expected = match (name, from) {
                    ("char", "typeof x") | ("indexedSequence", "value != null") | ("iteration", "key == null") => 2,
                    _ => 1,
                };
                assert_eq!(body.matches(from).count(), expected, "scalar adapter {name}: {from}");
                body = body.replace(from, &to);
            }
        }
        let header = &base[unit.header];
        if body == original {
            result.push_str(header);
        } else if header.contains("requires:") {
            result.push_str(&header.replacen("requires:", "requires: sharedScalar,", 1));
        } else {
            let (first, rest) = header.split_once('\n').expect("unit header");
            result.push_str(&format!("{first} requires: sharedScalar\n{rest}"));
        }
        result.push_str(&body);
    }
    result
}

/// The profile's boot unit, whose body names every top-level binding at
/// [`SHARED_BINDINGS`].
const SHARED_BOOT: &str = "sharedProfileBoot";
const SHARED_BINDINGS: &str = "/* SHARED_BINDINGS */";
/// Where the boot unit names them, in the same order, for its diagnostic of
/// the bindings a seal cannot take.
const SHARED_BINDING_NAMES: &str = "/* SHARED_BINDING_NAMES */";
/// Bindings the boot unit's list leaves out: `coreMethod`, which the
/// manifest allows only as `const x = coreMethod(...)` (its load-time
/// callers hold what it makes; nothing captures the function after boot).
const SHARED_UNLISTED: &[&str] = &["coreMethod"];

/// The header of the boot unit: `requires:` every other binding, wrapped
/// like `RuntimeManifest::with_computed_requires` wraps.
fn shared_boot_header(names: &[&str]) -> String {
    let mut header = format!("  //@unit {SHARED_BOOT} requires:");
    let mut column = header.len();
    for (position, name) in names.iter().enumerate() {
        let word = if position + 1 < names.len() {
            format!("{name},")
        } else {
            name.to_string()
        };
        if column + 1 + word.len() > 100 {
            header.push_str("\n  //@");
            column = 5;
        }
        header.push(' ');
        header.push_str(&word);
        column += 1 + word.len();
    }
    header.push('\n');
    header
}

/// `names` joined by `, ` on lines of at most `width` columns, each line
/// after the first indented by `indent`.
fn wrap_list(names: &[&str], indent: &str, width: usize) -> String {
    let mut out = String::new();
    let mut column = 0;
    for (position, name) in names.iter().enumerate() {
        let word = if position + 1 < names.len() {
            format!("{name},")
        } else {
            name.to_string()
        };
        if column > 0 && column + 1 + word.len() > width {
            out.push('\n');
            out.push_str(indent);
            column = indent.len();
        } else if column > 0 {
            out.push(' ');
            column += 1;
        }
        out.push_str(&word);
        column += word.len();
    }
    out
}

/// The bindings a unit body declares at its top level, in source order.
fn top_level_bindings(source: &str, body: std::ops::Range<usize>) -> Vec<&str> {
    let tokens = js_syntax::lex(source, body).unwrap_or_else(|error| {
        panic!(
            "shared-heap runtime byte {}: {}",
            error.offset, error.message
        )
    });
    let resolution = js_syntax::resolve(source, &tokens)
        .unwrap_or_else(|(offset, message)| panic!("shared-heap runtime byte {offset}: {message}"));
    resolution
        .declarations
        .iter()
        .filter(|declaration| declaration.scope == 0)
        .map(|declaration| declaration.name)
        .collect()
}

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
            lines.token(&writer.out, &line_starts, token.start);
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
