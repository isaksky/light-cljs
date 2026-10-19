// light-cljs builds the quota-aware V8 source patch instead of downloading the
// ABI-incompatible upstream archive. Upstream still owns toolchains and GN.
// A built engine is shared through a per-user cache keyed by everything that
// decides its output, so each machine builds a given engine once (README.md).
#[path = "upstream_build.rs"]
mod upstream;
#[path = "cache.rs"]
mod cache;

use cache::{key_env, static_lib, Engine};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    for file in ["build.rs", "cache.rs", "upstream_build.rs", "prepare.bb", "engine-budget.patch"] {
        println!("cargo:rerun-if-changed={file}");
    }
    for var in ["LIGHT_CLJS_V8_CACHE", "LIGHT_CLJS_V8_CACHE_DIR", "LIGHT_CLJS_V8_CACHE_SEED"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    for var in key_env() {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    if env::var_os("DOCS_RS").is_some() || env::var_os("DENO_TRYBUILD").is_some() {
        return upstream::main();
    }
    if env::var_os("RUSTY_V8_ARCHIVE").is_some() {
        assert!(env::var_os("RUSTY_V8_SRC_BINDING_PATH").is_some(),
            "RUSTY_V8_ARCHIVE requires matching RUSTY_V8_SRC_BINDING_PATH from this patched engine");
        // Override an inherited V8_FROM_SOURCE for this explicit pair.
        unsafe { env::remove_var("V8_FROM_SOURCE"); }
        return upstream::main();
    }
    let mode = env::var("LIGHT_CLJS_V8_CACHE").unwrap_or_default();
    assert!(matches!(mode.as_str(), "" | "on" | "off" | "require"),
        "LIGHT_CLJS_V8_CACHE must be on, off or require, not {mode:?}");
    if cache::is_asan() && env::var_os("OPT_LEVEL").unwrap_or_default() == "0" {
        // Upstream checks this only on its source path; cached engines too.
        panic!("v8 crate cannot be compiled with OPT_LEVEL=0 and ASAN.\nTry `[profile.dev.package.v8] opt-level = 1`.");
    }
    if mode == "off" {
        return build_from_source(&manifest);
    }
    let Some(cache) = cache_dir() else {
        assert!(mode != "require", "LIGHT_CLJS_V8_CACHE=require and no V8 cache directory; set LIGHT_CLJS_V8_CACHE_DIR");
        return build_from_source(&manifest);
    };
    let material = cache::key_material(&manifest);
    let name = format!("v8-{}-{}-{}-{:016x}", env::var("CARGO_PKG_VERSION").unwrap(),
        env::var("TARGET").unwrap(), env::var("PROFILE").unwrap(), cache::fnv1a(material.as_bytes()));
    let entry = cache.join(&name);
    // One machine-wide lock per key: a second build of the same engine waits
    // for the first and then reuses its entry instead of building again.
    let lock = fs::create_dir_all(&cache)
        .and_then(|()| fslock::LockFile::open(&cache.join(format!("{name}.lock"))));
    let mut lock = match lock {
        Ok(lock) => lock,
        Err(e) => {
            assert!(mode != "require", "LIGHT_CLJS_V8_CACHE=require and the V8 cache {} is unusable: {e}",
                cache.display());
            println!("cargo:warning=the V8 cache {} is unusable ({e}); building V8 from source", cache.display());
            return build_from_source(&manifest);
        }
    };
    if !lock.try_lock().unwrap_or(false) {
        eprintln!("waiting for another build of {name}");
        lock.lock().expect("lock the V8 cache entry");
    }
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let seed = env::var_os("LIGHT_CLJS_V8_CACHE_SEED").map(PathBuf::from);
    let engine = cache::obtain(&cache, &entry, &material, mode == "require", seed.as_deref(), &out_dir,
        || build_from_source(&manifest));
    if engine == Engine::Built {
        return;
    }
    let binding = cache::take_binding(&entry, &out_dir);
    println!("using the cached patched V8 engine {}", entry.display());
    // This single-threaded build process hands upstream the cached pair.
    unsafe {
        env::set_var("RUSTY_V8_ARCHIVE", entry.join(static_lib()));
        env::set_var("RUSTY_V8_SRC_BINDING_PATH", &binding);
        env::remove_var("V8_FROM_SOURCE");
    }
    upstream::main();
}

fn build_from_source(manifest: &Path) {
    let cache = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("source");
    let status = Command::new("bb").arg(manifest.join("prepare.bb")).arg(&cache)
        .status().expect("building the patched V8 engine requires babashka (bb)");
    assert!(status.success(), "preparing the pinned V8 sources failed");
    let root = cache.join("v8-152.2.0");
    env::set_current_dir(&root).expect("enter V8 sources");
    // This single-threaded build process launches upstream after setting
    // its source root. It does not modify Cargo's parent environment.
    unsafe {
        env::set_var("CARGO_MANIFEST_DIR", &root);
        env::set_var("V8_FROM_SOURCE", "1");
    }
    upstream::main();
}

/// `LIGHT_CLJS_V8_CACHE_DIR`, else the platform's per-user cache directory.
fn cache_dir() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("LIGHT_CLJS_V8_CACHE_DIR").filter(|dir| !dir.is_empty()) {
        let dir = PathBuf::from(dir);
        assert!(dir.is_absolute(), "LIGHT_CLJS_V8_CACHE_DIR must be absolute: {}", dir.display());
        return Some(dir);
    }
    let absolute = |var: &str| env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute());
    let base = if cfg!(target_os = "macos") {
        home::home_dir().map(|home| home.join("Library").join("Caches"))
    } else if cfg!(windows) {
        absolute("LOCALAPPDATA")
    } else {
        absolute("XDG_CACHE_HOME").or_else(|| home::home_dir().map(|home| home.join(".cache")))
    };
    if base.is_none() {
        println!("cargo:warning=no per-user cache directory; set LIGHT_CLJS_V8_CACHE_DIR to share V8 builds");
    }
    base.map(|base| base.join("light-cljs").join("v8"))
}
