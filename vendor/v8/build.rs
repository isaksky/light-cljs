// light-cljs builds the quota-aware V8 source patch instead of downloading the
// ABI-incompatible upstream archive. Upstream still owns toolchains and GN.
// A built engine is shared through a per-user cache keyed by everything that
// decides its output, so each machine builds a given engine once (README.md).
#[path = "upstream_build.rs"]
mod upstream;

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Bump when this wrapper changes what a source build produces.
const CACHE_SCHEMA: u32 = 1;
/// The source recipe, the engine patch and upstream's GN/bindgen logic.
const KEY_FILES: [&str; 3] = ["prepare.bb", "engine-budget.patch", "upstream_build.rs"];
/// Environment that upstream turns into GN or bindgen arguments.
const KEY_ENV: [&str; 8] = [
    "V8_FORCE_DEBUG",
    "GN_ARGS",
    "EXTRA_GN_ARGS",
    "DISABLE_CLANG",
    "CLANG_BASE_PATH",
    "RUSTY_V8_MUSL_SYSROOT",
    "RUSTY_V8_GLIBC_PREFIX",
    "RUSTY_V8_BINDGEN_RESOURCE_DIR",
];
const BINDING: &str = "src_binding.rs";

fn main() {
    for file in ["build.rs", "upstream_build.rs", "prepare.bb", "engine-budget.patch"] {
        println!("cargo:rerun-if-changed={file}");
    }
    for var in ["LIGHT_CLJS_V8_CACHE", "LIGHT_CLJS_V8_CACHE_DIR", "LIGHT_CLJS_V8_CACHE_SEED"] {
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
    let Some(cache) = (mode != "off").then(cache_dir).flatten() else {
        return build_from_source(&manifest);
    };
    let material = key_material(&manifest);
    let name = format!("v8-{}-{}-{}-{:016x}", env::var("CARGO_PKG_VERSION").unwrap(),
        env::var("TARGET").unwrap(), env::var("PROFILE").unwrap(), fnv1a(material.as_bytes()));
    let entry = cache.join(&name);
    fs::create_dir_all(&cache)
        .unwrap_or_else(|e| panic!("create the V8 cache {}: {e}", cache.display()));
    // One machine-wide lock per key: a second build of the same engine waits
    // for the first and then reuses its entry instead of building again.
    let mut lock = fslock::LockFile::open(&cache.join(format!("{name}.lock")))
        .unwrap_or_else(|e| panic!("open the V8 cache lock in {}: {e}", cache.display()));
    if !lock.try_lock().unwrap_or(false) {
        eprintln!("waiting for another build of {name}");
        lock.lock().expect("lock the V8 cache entry");
    }
    if !is_complete(&entry, &material) {
        if let Some(seed) = env::var_os("LIGHT_CLJS_V8_CACHE_SEED").map(PathBuf::from) {
            publish(&cache, &entry, &material, &seed.join(static_lib()), &seed.join(BINDING))
                .unwrap_or_else(|e| panic!("seed the V8 cache from {}: {e}", seed.display()));
        } else if mode == "require" {
            panic!("LIGHT_CLJS_V8_CACHE=require and no cached patched V8 matches this build: {}",
                entry.display());
        } else {
            println!("cargo:warning=building the patched V8 from source for {}", entry.display());
            build_from_source(&manifest);
            let gn_out = target_profile_dir().join("gn_out");
            let built = (gn_out.join("obj").join(static_lib()), gn_out.join(BINDING));
            if let Err(e) = publish(&cache, &entry, &material, &built.0, &built.1) {
                println!("cargo:warning=could not publish the built V8 to {}: {e}", entry.display());
            }
            return;
        }
    }
    // The binding is compiled into the crate, so keep a copy in this build's
    // output; upstream copies the archive next to its other build output.
    let binding = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join(BINDING);
    fs::copy(entry.join(BINDING), &binding).expect("copy the cached V8 binding");
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

/// Everything that decides the archive and binding, in full: an entry is used
/// only when its recorded material equals this byte for byte.
fn key_material(manifest: &Path) -> String {
    let var = |name: &str| env::var(name).unwrap_or_default();
    let mut text = format!("light-cljs patched V8 cache schema {CACHE_SCHEMA}\n");
    // Archives from the two Cargo profiles differ only in the build directory
    // names their source paths embed; keying the profile keeps each faithful.
    for name in ["CARGO_PKG_VERSION", "TARGET", "HOST", "PROFILE"] {
        text.push_str(&format!("{name}={}\n", var(name)));
    }
    let mut features: Vec<String> = env::vars().map(|(name, _)| name)
        .filter(|name| name.starts_with("CARGO_FEATURE_")).collect();
    features.sort();
    text.push_str(&format!("features={}\n", features.join(",")));
    let rustflags = var("CARGO_ENCODED_RUSTFLAGS");
    let asan = rustflags.contains("-Z sanitizer=address") || rustflags.contains("-Zsanitizer=address");
    text.push_str(&format!("asan={asan}\n"));
    for name in KEY_ENV {
        text.push_str(&format!("{name}={}\n", var(name)));
    }
    for file in KEY_FILES {
        let content = fs::read_to_string(manifest.join(file))
            .unwrap_or_else(|e| panic!("read {file}: {e}"));
        text.push_str(&format!("file {file} {}\n{content}\n", content.len()));
    }
    text
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn static_lib() -> &'static str {
    if env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" { "rusty_v8.lib" } else { "librusty_v8.a" }
}

/// `target/<profile>`, where upstream keeps `gn_out`.
fn target_profile_dir() -> PathBuf {
    PathBuf::from(env::var_os("OUT_DIR").unwrap()).ancestors().nth(3).unwrap().to_path_buf()
}

fn sizes(dir: &Path) -> io::Result<String> {
    Ok(format!("{} {}\n{BINDING} {}\n", static_lib(),
        fs::metadata(dir.join(static_lib()))?.len(), fs::metadata(dir.join(BINDING))?.len()))
}

/// An entry appears only by renaming a fully written directory, so a complete
/// one has this key's material and the file sizes it was published with.
fn is_complete(entry: &Path, material: &str) -> bool {
    fs::read_to_string(entry.join("key.txt")).is_ok_and(|key| key == material)
        && fs::read_to_string(entry.join("sizes.txt"))
            .is_ok_and(|recorded| sizes(entry).is_ok_and(|actual| actual == recorded))
}

/// Write the pair into a private temporary directory, flush it, then rename
/// it into place. Readers never see a partial entry; an interrupted publish
/// leaves only a `.tmp-` directory that no lookup reads.
fn publish(cache: &Path, entry: &Path, material: &str, archive: &Path, binding: &Path) -> io::Result<()> {
    remove_stale_temporaries(cache);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let temporary = cache.join(format!(".tmp-{}-{nanos}", std::process::id()));
    fs::create_dir(&temporary)?;
    let result = (|| {
        fs::copy(archive, temporary.join(static_lib()))?;
        fs::copy(binding, temporary.join(BINDING))?;
        fs::write(temporary.join("key.txt"), material)?;
        fs::write(temporary.join("sizes.txt"), sizes(&temporary)?)?;
        for file in [static_lib(), BINDING, "key.txt", "sizes.txt"] {
            fs::File::open(temporary.join(file))?.sync_all()?;
        }
        sync_dir(&temporary)?;
        if entry.exists() {
            // Unusable (damaged, or another key's): move it aside first.
            let stale = cache.join(format!(".tmp-stale-{}-{nanos}", std::process::id()));
            fs::rename(entry, &stale)?;
            let _ = fs::remove_dir_all(&stale);
        }
        match fs::rename(&temporary, entry) {
            Err(_) if is_complete(entry, material) => {}
            result => result?,
        }
        sync_dir(cache)
    })();
    if temporary.exists() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    if cfg!(unix) { fs::File::open(dir)?.sync_all() } else { Ok(()) }
}

/// Temporaries live for seconds; one a day old was left by a killed publish.
fn remove_stale_temporaries(cache: &Path) {
    let Ok(entries) = fs::read_dir(cache) else { return };
    for item in entries.flatten() {
        let old = item.metadata().and_then(|m| m.modified()).ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 60 * 60));
        if old && item.file_name().to_string_lossy().starts_with(".tmp-") {
            let _ = fs::remove_dir_all(item.path());
        }
    }
}
